//! Runs the simulation on its own thread, so that a slow tick cannot stall a frame.
//! This is presentation code: it paces with wall-clock time, but ticks and their
//! commands are the only way anything reaches the simulation.
#![allow(clippy::disallowed_types)]

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::ai::{advance_ai_vs_ai, AiBrain};
use crate::sim::commands::SimCommand;
use crate::sim::components::Team;
use crate::sim::scenario::{advance_scripted, Scenario};
use crate::sim::world::{SimSnapshot, SimWorld};

pub const TICK_DURATION: Duration = Duration::from_millis(100);

/// Where commands come from.
#[derive(Copy, Clone, Debug)]
pub enum CommandSource {
    /// Player input, sent through `SimDriver::send`.
    Player,
    /// `scenario::advance_scripted`, stopping after a fixed number of ticks.
    Script { seed: u64, ticks: u32 },
    /// The player commands Blue through `SimDriver::send`, against a scripted Red.
    PlayerVsAi { seed: u64 },
    /// Two scripted opponents, stopping after a fixed number of ticks or at the end of the match.
    AiVsAi { seed: u64, ticks: u32 },
}

/// Extra settings for `SimDriver::start_with`.
#[derive(Copy, Clone, Debug, Default)]
pub struct DriverOptions {
    /// Whose fog of war snapshots respect. `None` shows everything.
    pub viewer: Option<Team>,
    /// Ticks to run back to back before pacing starts, to reach a later point in a match.
    pub fast_forward_ticks: u32,
}

/// The simulation's state when its thread ended.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DriverResult {
    pub tick: u32,
    pub checksum: u64,
}

pub struct SimDriver {
    commands: Sender<SimCommand>,
    snapshots: Receiver<Arc<SimSnapshot>>,
    tick_micros: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<DriverResult>>,
}

impl SimDriver {
    /// Starts the simulation. `paced` runs at 10 Hz; otherwise ticks run back to back.
    pub fn start(scenario: Scenario, source: CommandSource, paced: bool) -> Self {
        Self::start_with(scenario, source, paced, DriverOptions::default())
    }

    pub fn start_with(scenario: Scenario, source: CommandSource, paced: bool, options: DriverOptions) -> Self {
        let (command_sender, command_receiver) = mpsc::channel();
        let (snapshot_sender, snapshot_receiver) = mpsc::channel();
        let tick_micros = Arc::new(AtomicU32::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_micros = Arc::clone(&tick_micros);
        let thread_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("simulation".into())
            .spawn(move || {
                run(scenario, source, paced, options, &command_receiver, &snapshot_sender, &thread_micros, &thread_stop)
            })
            .expect("simulation thread starts");
        Self {
            commands: command_sender,
            snapshots: snapshot_receiver,
            tick_micros,
            stop,
            thread: Some(thread),
        }
    }

    pub fn send(&self, command: SimCommand) {
        // The thread only ends on shutdown, when a late command no longer matters.
        let _ = self.commands.send(command);
    }

    pub fn try_snapshot(&self) -> Option<Arc<SimSnapshot>> {
        self.snapshots.try_recv().ok()
    }

    pub fn last_tick_micros(&self) -> u32 {
        self.tick_micros.load(Ordering::Relaxed)
    }

    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Stops the simulation and returns its final state.
    pub fn finish(mut self) -> DriverResult {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().expect("finish is only called once").join().expect("simulation thread did not panic")
    }
}

impl Drop for SimDriver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    scenario: Scenario,
    source: CommandSource,
    paced: bool,
    options: DriverOptions,
    commands: &Receiver<SimCommand>,
    snapshots: &Sender<Arc<SimSnapshot>>,
    tick_micros: &AtomicU32,
    stop: &AtomicBool,
) -> DriverResult {
    let Scenario { mut world, opening_commands } = scenario;
    let seed = match source {
        CommandSource::PlayerVsAi { seed } | CommandSource::AiVsAi { seed, .. } => seed,
        CommandSource::Player | CommandSource::Script { .. } => 0,
    };
    let mut brains = [AiBrain::new(Team::Blue, seed, world.starts), AiBrain::new(Team::Red, seed, world.starts)];
    for _ in 0..options.fast_forward_ticks {
        if world.match_result.is_some() {
            break;
        }
        step(&mut world, source, &opening_commands, commands, &mut brains);
    }
    let _ = snapshots.send(Arc::new(world.snapshot_for(options.viewer)));
    let mut next_tick = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let started = Instant::now();
        match source {
            CommandSource::Script { ticks, .. } | CommandSource::AiVsAi { ticks, .. } => {
                if world.current_tick >= ticks || world.match_result.is_some() {
                    break;
                }
            }
            CommandSource::Player | CommandSource::PlayerVsAi { .. } => {}
        }
        // A finished match stays on screen until the player leaves it.
        if world.match_result.is_none() {
            step(&mut world, source, &opening_commands, commands, &mut brains);
        }
        tick_micros.store(started.elapsed().as_micros() as u32, Ordering::Relaxed);
        if snapshots.send(Arc::new(world.snapshot_for(options.viewer))).is_err() {
            break;
        }
        if paced {
            next_tick += TICK_DURATION;
            let now = Instant::now();
            if next_tick > now {
                thread::sleep(next_tick - now);
            } else {
                // Fell behind: run slower instead of bursting to catch up.
                next_tick = now;
            }
        }
    }
    DriverResult { tick: world.current_tick, checksum: world.compute_checksum() }
}

/// Advances one tick from the command source. Everything that reaches the simulation goes through here.
fn step(world: &mut SimWorld, source: CommandSource, opening: &[SimCommand], commands: &Receiver<SimCommand>, brains: &mut [AiBrain; 2]) {
    match source {
        CommandSource::Player => {
            let mut pending = if world.current_tick == 0 { opening.to_vec() } else { Vec::new() };
            pending.extend(commands.try_iter());
            world.advance_tick(&pending);
        }
        CommandSource::Script { seed, .. } => advance_scripted(world, opening, seed),
        CommandSource::PlayerVsAi { .. } => {
            let mut pending: Vec<SimCommand> = commands.try_iter().collect();
            pending.extend(brains[1].think(world));
            world.advance_tick(&pending);
        }
        CommandSource::AiVsAi { .. } => {
            advance_ai_vs_ai(world, brains);
        }
    }
}
