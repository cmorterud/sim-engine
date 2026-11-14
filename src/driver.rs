//! Runs the simulation on its own thread, so that a slow tick cannot stall a frame. In the
//! browser, where threads need headers a static host cannot send, it runs on the page's own
//! thread instead, a tick at a time as frames ask for snapshots. Both have the same interface.
//! This is presentation code: it paces with wall-clock time, but ticks and their
//! commands are the only way anything reaches the simulation.
#![allow(clippy::disallowed_types)]

use std::sync::Arc;
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::{
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
};

use web_time::Instant;

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
    /// Starts with the clock stopped, after any fast-forward.
    pub start_paused: bool,
}

/// The simulation's state when its thread ended.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DriverResult {
    pub tick: u32,
    pub checksum: u64,
}

#[cfg(not(target_arch = "wasm32"))]
pub struct SimDriver {
    commands: Sender<SimCommand>,
    snapshots: Receiver<Arc<SimSnapshot>>,
    tick_micros: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    thread: Option<JoinHandle<DriverResult>>,
}

#[cfg(not(target_arch = "wasm32"))]
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
        let paused = Arc::new(AtomicBool::new(options.start_paused));
        let thread_paused = Arc::clone(&paused);
        let thread = thread::Builder::new()
            .name("simulation".into())
            .spawn(move || {
                run(scenario, source, paced, options, &command_receiver, &snapshot_sender, &thread_micros, &thread_stop, &thread_paused)
            })
            .expect("simulation thread starts");
        Self {
            commands: command_sender,
            snapshots: snapshot_receiver,
            tick_micros,
            stop,
            paused,
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

    /// Stops or restarts the clock. While paused no tick runs, and commands sent meanwhile wait
    /// in order for the next tick, so the player can still give orders. With one human player,
    /// the pause is theirs alone; in multiplayer it will have to be a command every peer applies.
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
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

#[cfg(not(target_arch = "wasm32"))]
impl Drop for SimDriver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(not(target_arch = "wasm32"))]
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
    paused: &AtomicBool,
) -> DriverResult {
    let Scenario { mut world, opening_commands } = scenario;
    let mut brains = brains_for(&world, source);
    for _ in 0..options.fast_forward_ticks {
        if world.match_result.is_some() {
            break;
        }
        step(&mut world, source, &opening_commands, commands.try_iter().collect(), &mut brains);
    }
    let _ = snapshots.send(Arc::new(world.snapshot_for(options.viewer)));
    let mut next_tick = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if paused.load(Ordering::Relaxed) {
            // Nothing changes, so no snapshot is sent: the last one stays on screen.
            thread::sleep(TICK_DURATION / 10);
            next_tick = Instant::now();
            continue;
        }
        let started = Instant::now();
        if source_done(&world, source) {
            break;
        }
        // A finished match stays on screen until the player leaves it.
        if world.match_result.is_none() {
            step(&mut world, source, &opening_commands, commands.try_iter().collect(), &mut brains);
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

/// Advances one tick from the command source, with the player's commands since the last tick.
/// Everything that reaches the simulation goes through here.
fn step(world: &mut SimWorld, source: CommandSource, opening: &[SimCommand], commands: Vec<SimCommand>, brains: &mut [AiBrain; 2]) {
    match source {
        CommandSource::Player => {
            let mut pending = if world.current_tick == 0 { opening.to_vec() } else { Vec::new() };
            pending.extend(commands);
            world.advance_tick(&pending);
        }
        CommandSource::Script { seed, .. } => advance_scripted(world, opening, seed),
        CommandSource::PlayerVsAi { .. } => {
            let mut pending = commands;
            pending.extend(brains[1].think(world));
            world.advance_tick(&pending);
        }
        CommandSource::AiVsAi { .. } => {
            advance_ai_vs_ai(world, brains);
        }
    }
}

/// The two AI opponents, for sources that have them.
fn brains_for(world: &SimWorld, source: CommandSource) -> [AiBrain; 2] {
    let seed = match source {
        CommandSource::PlayerVsAi { seed } | CommandSource::AiVsAi { seed, .. } => seed,
        CommandSource::Player | CommandSource::Script { .. } => 0,
    };
    [AiBrain::new(Team::Blue, seed, world.starts), AiBrain::new(Team::Red, seed, world.starts)]
}

/// Whether a source with a set length has run its course.
fn source_done(world: &SimWorld, source: CommandSource) -> bool {
    match source {
        CommandSource::Script { ticks, .. } | CommandSource::AiVsAi { ticks, .. } => world.current_tick >= ticks || world.match_result.is_some(),
        CommandSource::Player | CommandSource::PlayerVsAi { .. } => false,
    }
}

/// The browser's driver: the simulation runs on the page's own thread, a tick at a time when
/// one is due, whenever a frame asks for the latest snapshot.
#[cfg(target_arch = "wasm32")]
pub struct SimDriver {
    state: std::cell::RefCell<Inline>,
}

#[cfg(target_arch = "wasm32")]
struct Inline {
    world: SimWorld,
    source: CommandSource,
    opening: Vec<SimCommand>,
    brains: [AiBrain; 2],
    viewer: Option<Team>,
    paced: bool,
    paused: bool,
    finished: bool,
    pending: Vec<SimCommand>,
    snapshots: std::collections::VecDeque<Arc<SimSnapshot>>,
    next_tick: Instant,
    tick_micros: u32,
}

#[cfg(target_arch = "wasm32")]
impl SimDriver {
    /// Starts the simulation. `paced` runs at 10 Hz; otherwise each frame runs a tick.
    pub fn start(scenario: Scenario, source: CommandSource, paced: bool) -> Self {
        Self::start_with(scenario, source, paced, DriverOptions::default())
    }

    pub fn start_with(scenario: Scenario, source: CommandSource, paced: bool, options: DriverOptions) -> Self {
        let Scenario { mut world, opening_commands } = scenario;
        let mut brains = brains_for(&world, source);
        for _ in 0..options.fast_forward_ticks {
            if world.match_result.is_some() {
                break;
            }
            step(&mut world, source, &opening_commands, Vec::new(), &mut brains);
        }
        let first = Arc::new(world.snapshot_for(options.viewer));
        Self {
            state: std::cell::RefCell::new(Inline {
                world,
                source,
                opening: opening_commands,
                brains,
                viewer: options.viewer,
                paced,
                paused: options.start_paused,
                finished: false,
                pending: Vec::new(),
                snapshots: std::collections::VecDeque::from([first]),
                next_tick: Instant::now(),
                tick_micros: 0,
            }),
        }
    }

    pub fn send(&self, command: SimCommand) {
        self.state.borrow_mut().pending.push(command);
    }

    /// Runs a tick if one is due, then hands over the oldest snapshot not yet taken.
    pub fn try_snapshot(&self) -> Option<Arc<SimSnapshot>> {
        let mut state = self.state.borrow_mut();
        state.advance();
        state.snapshots.pop_front()
    }

    /// Stops or restarts the clock. While paused no tick runs, and commands wait for the next.
    pub fn set_paused(&self, paused: bool) {
        self.state.borrow_mut().paused = paused;
    }

    pub fn is_paused(&self) -> bool {
        self.state.borrow().paused
    }

    pub fn last_tick_micros(&self) -> u32 {
        self.state.borrow().tick_micros
    }

    pub fn is_finished(&self) -> bool {
        self.state.borrow().finished
    }

    /// Stops the simulation and returns its final state.
    pub fn finish(self) -> DriverResult {
        let state = self.state.into_inner();
        DriverResult { tick: state.world.current_tick, checksum: state.world.compute_checksum() }
    }
}

#[cfg(target_arch = "wasm32")]
impl Inline {
    /// Runs the tick that is due, if any. A frame never runs more than one, so a slow tick
    /// slows the game instead of stalling the page to catch up.
    fn advance(&mut self) {
        let now = Instant::now();
        if self.paused || self.finished {
            self.next_tick = now;
            return;
        }
        if self.paced && now < self.next_tick {
            return;
        }
        if source_done(&self.world, self.source) {
            self.finished = true;
            return;
        }
        let started = Instant::now();
        // A finished match stays on screen until the player leaves it.
        if self.world.match_result.is_none() {
            let commands = std::mem::take(&mut self.pending);
            step(&mut self.world, self.source, &self.opening, commands, &mut self.brains);
        }
        self.tick_micros = started.elapsed().as_micros() as u32;
        self.snapshots.push_back(Arc::new(self.world.snapshot_for(self.viewer)));
        self.next_tick = if self.next_tick + TICK_DURATION > now { self.next_tick + TICK_DURATION } else { now + TICK_DURATION };
    }
}
