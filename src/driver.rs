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

use crate::ai::{advance_ai_vs_ai, AiBrain, Difficulty};
use crate::ai_strategy::Strategy;
use crate::sim::commands::SimCommand;
use crate::sim::components::Team;
use crate::sim::scenario::{advance_scripted, Scenario};
use crate::sim::world::{SimSnapshot, SimWorld};

pub const TICK_DURATION: Duration = Duration::from_millis(100);

/// The fastest a watched match can play, in ticks per tick of real time.
pub const MAX_SPEED: u32 = 16;

/// A viewer as one number, for sharing between threads: 0 for everything, 1 Blue, 2 Red.
#[cfg(not(target_arch = "wasm32"))]
const fn viewer_code(viewer: Option<Team>) -> u8 {
    match viewer {
        None => 0,
        Some(Team::Blue) => 1,
        Some(Team::Red) => 2,
    }
}

#[cfg(not(target_arch = "wasm32"))]
const fn viewer_from_code(code: u8) -> Option<Team> {
    match code {
        1 => Some(Team::Blue),
        2 => Some(Team::Red),
        _ => None,
    }
}

/// How long a tick lasts at a speed.
fn tick_interval(speed: u32) -> Duration {
    TICK_DURATION / speed.clamp(1, MAX_SPEED)
}

/// Where commands come from.
#[derive(Copy, Clone, Debug)]
pub enum CommandSource {
    /// Player input, sent through `SimDriver::send`.
    Player,
    /// `scenario::advance_scripted`, stopping after a fixed number of ticks.
    Script { seed: u64, ticks: u32 },
    /// The player commands Blue through `SimDriver::send`, against a scripted Red that plays
    /// `strategy` at `difficulty`.
    PlayerVsAi { seed: u64, difficulty: Difficulty, strategy: &'static Strategy },
    /// Two scripted opponents, stopping after a fixed number of ticks or at the end of the match.
    /// The same match the soak plays for the seed, so watching one replays it.
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
    /// Keeps the last snapshot coming once the command source is done, instead of ending, so
    /// that a watcher can still look it over from either side. Test runs end, so they report.
    pub linger: bool,
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
    controls: Arc<Controls>,
    thread: Option<JoinHandle<DriverResult>>,
}

/// What a watcher can change while a match runs: whose sight snapshots show, and how fast
/// ticks come. Neither reaches the simulation, so neither changes how the match plays out.
#[cfg(not(target_arch = "wasm32"))]
struct Controls {
    viewer: std::sync::atomic::AtomicU8,
    speed: AtomicU32,
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
        let controls = Arc::new(Controls { viewer: std::sync::atomic::AtomicU8::new(viewer_code(options.viewer)), speed: AtomicU32::new(1) });
        let thread_controls = Arc::clone(&controls);
        let thread = thread::Builder::new()
            .name("simulation".into())
            .spawn(move || {
                run(scenario, source, paced, options, &command_receiver, &snapshot_sender, &thread_micros, &thread_stop, &thread_paused, &thread_controls)
            })
            .expect("simulation thread starts");
        Self {
            commands: command_sender,
            snapshots: snapshot_receiver,
            tick_micros,
            stop,
            paused,
            controls,
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

    /// Changes whose sight the snapshots show, from the next one on, even while paused.
    pub fn set_viewer(&self, viewer: Option<Team>) {
        self.controls.viewer.store(viewer_code(viewer), Ordering::Relaxed);
    }

    pub fn viewer(&self) -> Option<Team> {
        viewer_from_code(self.controls.viewer.load(Ordering::Relaxed))
    }

    /// Sets how many ticks run for each tick of real time, from 1 to `MAX_SPEED`.
    pub fn set_speed(&self, speed: u32) {
        self.controls.speed.store(speed.clamp(1, MAX_SPEED), Ordering::Relaxed);
    }

    pub fn speed(&self) -> u32 {
        self.controls.speed.load(Ordering::Relaxed)
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
    controls: &Controls,
) -> DriverResult {
    let Scenario { mut world, opening_commands } = scenario;
    let mut brains = brains_for(&world, source);
    for _ in 0..options.fast_forward_ticks {
        if world.match_result.is_some() {
            break;
        }
        step(&mut world, source, &opening_commands, commands.try_iter().collect(), &mut brains);
    }
    let mut viewer = viewer_from_code(controls.viewer.load(Ordering::Relaxed));
    let _ = snapshots.send(Arc::new(world.snapshot_for(viewer)));
    let mut next_tick = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let wanted = viewer_from_code(controls.viewer.load(Ordering::Relaxed));
        if paused.load(Ordering::Relaxed) || source_done(&world, source) {
            // Nothing changes, so no snapshot is sent and the last one stays on screen, unless
            // the watcher asks for another side's view of it.
            if wanted != viewer {
                viewer = wanted;
                if snapshots.send(Arc::new(world.snapshot_for(viewer))).is_err() {
                    break;
                }
            }
            if source_done(&world, source) && !paused.load(Ordering::Relaxed) && !options.linger {
                break;
            }
            thread::sleep(TICK_DURATION / 10);
            next_tick = Instant::now();
            continue;
        }
        viewer = wanted;
        let started = Instant::now();
        // A finished match stays on screen until the player leaves it.
        if world.match_result.is_none() {
            step(&mut world, source, &opening_commands, commands.try_iter().collect(), &mut brains);
        }
        tick_micros.store(started.elapsed().as_micros() as u32, Ordering::Relaxed);
        if snapshots.send(Arc::new(world.snapshot_for(viewer))).is_err() {
            break;
        }
        if paced {
            next_tick += tick_interval(controls.speed.load(Ordering::Relaxed));
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
fn step(world: &mut SimWorld, source: CommandSource, opening: &[SimCommand], commands: Vec<SimCommand>, brains: &mut [AiBrain]) {
    match source {
        CommandSource::Player => {
            let mut pending = if world.current_tick == 0 { opening.to_vec() } else { Vec::new() };
            pending.extend(commands);
            world.advance_tick(&pending);
        }
        CommandSource::Script { seed, .. } => advance_scripted(world, opening, seed),
        CommandSource::PlayerVsAi { .. } => {
            let mut pending = commands;
            // The player is player 0; every other player is an AI, allies and enemies alike.
            for brain in brains.iter_mut().skip(1) {
                pending.extend(brain.think(world));
            }
            world.advance_tick(&pending);
        }
        CommandSource::AiVsAi { .. } => {
            advance_ai_vs_ai(world, brains);
        }
    }
}

/// The two AI opponents, for sources that have them.
fn brains_for(world: &SimWorld, source: CommandSource) -> Vec<AiBrain> {
    let (seed, difficulty, strategy) = match source {
        CommandSource::PlayerVsAi { seed, difficulty, strategy } => (seed, difficulty, strategy),
        CommandSource::AiVsAi { seed, .. } => (seed, Difficulty::Normal, Strategy::balanced()),
        CommandSource::Player | CommandSource::Script { .. } => (0, Difficulty::Normal, Strategy::balanced()),
    };
    (0..world.players.len()).map(|player| AiBrain::for_player(world, crate::sim::components::PlayerId(player as u8), seed, difficulty, strategy)).collect()
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
    brains: Vec<AiBrain>,
    viewer: Option<Team>,
    speed: u32,
    linger: bool,
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
                speed: 1,
                linger: options.linger,
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

    /// Changes whose sight the snapshots show, at once, even while paused.
    pub fn set_viewer(&self, viewer: Option<Team>) {
        let mut state = self.state.borrow_mut();
        if state.viewer != viewer {
            state.viewer = viewer;
            let snapshot = Arc::new(state.world.snapshot_for(viewer));
            state.snapshots.push_back(snapshot);
        }
    }

    pub fn viewer(&self) -> Option<Team> {
        self.state.borrow().viewer
    }

    /// Sets how many ticks run for each tick of real time, from 1 to `MAX_SPEED`.
    pub fn set_speed(&self, speed: u32) {
        self.state.borrow_mut().speed = speed.clamp(1, MAX_SPEED);
    }

    pub fn speed(&self) -> u32 {
        self.state.borrow().speed
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
    /// Runs the ticks that are due, if any: at most one a frame at normal speed, and when sped
    /// up, at most the speed's worth and no more than `FRAME_TICK_BUDGET` of work, so slow
    /// ticks slow the game instead of stalling the page to catch up.
    fn advance(&mut self) {
        const FRAME_TICK_BUDGET: Duration = Duration::from_millis(12);
        let started = Instant::now();
        for _ in 0..self.speed {
            if !self.advance_one() || started.elapsed() > FRAME_TICK_BUDGET {
                break;
            }
        }
    }

    /// Runs one tick if one is due, and returns whether it did.
    fn advance_one(&mut self) -> bool {
        let now = Instant::now();
        if self.paused || self.finished {
            self.next_tick = now;
            return false;
        }
        if self.paced && now < self.next_tick {
            return false;
        }
        if source_done(&self.world, self.source) {
            self.finished = !self.linger;
            return false;
        }
        let started = Instant::now();
        // A finished match stays on screen until the player leaves it.
        if self.world.match_result.is_none() {
            let commands = std::mem::take(&mut self.pending);
            step(&mut self.world, self.source, &self.opening, commands, &mut self.brains);
        }
        self.tick_micros = started.elapsed().as_micros() as u32;
        self.snapshots.push_back(Arc::new(self.world.snapshot_for(self.viewer)));
        let interval = tick_interval(self.speed);
        self.next_tick = if self.next_tick + interval > now { self.next_tick + interval } else { now + interval };
        true
    }
}
