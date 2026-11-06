//! Application entry point, window loop, and mode switch (headless vs windowed).
#![allow(clippy::disallowed_types)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sim_engine::ai::{advance_ai_vs_ai, AiBrain};
use sim_engine::driver::{CommandSource, DriverOptions, DriverResult, SimDriver, TICK_DURATION};
use sim_engine::audio::{self, AttackAlarm, SoundEngine, SoundSettings};
use sim_engine::input::feedback::{MarkerKind, OrderFeedback};
use sim_engine::input::hotkeys;
use sim_engine::input::orders::{
    assist_orders, deposit_position, formation_commands, free_deposits, line_cells, mobile_units, move_orders, overcharge_orders, pending_builds, queued_commands, PendingBuild, placement_cell, placement_is_valid, placement_target, right_click_orders, PlacementMap, RightClick,
};
use sim_engine::input::selection::{ControlGroups, Selection};
use sim_engine::math::fp::{Fp, Vec2Fp};
use sim_engine::render::camera::{Camera, HeightField};
use sim_engine::render::instances::{self, Headings, InstanceBatches};
use sim_engine::render::meshes::Shape;
use sim_engine::render::pipeline::{FrameScene, Renderer, UiFrame};
use sim_engine::sim::blueprints::{BlueprintDb, BlueprintId};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{SimId, Team};
use sim_engine::sim::config::TERRAIN_CELL_UNITS;
use sim_engine::sim::scenario::{self, advance_scripted, skirmish_start, Scenario};
use sim_engine::sim::terrain::HEIGHT_FRAC_BITS;
use sim_engine::sim::world::{PlanStep, PlanStepKind, SimSnapshot, UnitPlanView};
use sim_engine::soak::run_soak;
use sim_engine::sim::components::BuildTask;
use sim_engine::ui::{self, ArmedOrder, DraggableOrder, OrderHandle, QuickSelectState, UiAction};
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowBuilder;

const DEFAULT_SEED: u64 = 0x5EED_2026;
const PLAYER_TEAM: Team = Team::Blue;
/// Frames ignored at the start of a test run, while shaders and buffers warm up.
const WARMUP_FRAMES: usize = 120;
/// At 60 Hz, a frame interval this long means a vsync was missed. Intervals near
/// the nominal 16.67 ms carry timestamp jitter, so they cannot be compared directly.
const DROPPED_FRAME_MS: f64 = 25.0;
/// Height of each wall segment. Deep enough to reach below the terrain's raised samples.
const OBSTACLE_DRAW_HEIGHT: f64 = 18.0;
const STARTING_ALTITUDE: f64 = 900.0;
/// How close the camera can get in a skirmish. Stress test units are far smaller, and keep the camera's own limit.
const SKIRMISH_MIN_ALTITUDE: f64 = 130.0;
/// A Shift-press on an order's icon that moves less than this is only a click.
const MIN_ORDER_DRAG_PIXELS: f64 = 6.0;
/// How long the Shift view of everyone's orders takes to fade after letting go, in seconds.
const ORDERS_FADE_SECONDS: f64 = 1.0;

const USAGE: &str = "\
Usage: sim-engine [options]

  (no options)         Play a skirmish against the scripted AI
  --battle             Use the 5,000-unit stress test scenario instead of a skirmish
  --headless <ticks>   Run with no window, with AI or scripted orders, and print the checksum
  --autotest <secs>    Run the same thing in a window, sweep the zoom, print frame times,
                       and print the checksum
  --fast-forward <n>   With --autotest, run n ticks at full speed first, to test a later match
  --capture <dir>      With --autotest, save screenshots at several zoom levels
  --soak <matches>     Play and replay AI-vs-AI matches, and report on the exit criteria
  --threads <n>        Threads for --soak (default 6)
  --seed <number>      Scenario seed
  --sound-check        Play each sound in turn, and exit

Controls:
  Left-drag or click   Select your units
  Double-click         Select all your units of that type on screen
  Shift + click        Add a unit to the selection, or remove it
  Shift (hold)         Show every unit's pending orders
  Shift + drag         Add a box of units to the selection
  Right-click          Attack an enemy, assist or repair a friend, reclaim a wreck,
                       move to the ground, or set a factory's rally point
  Shift + a factory unit  Queue 5 at a time, by button or key
  Shift + drag an icon Move an order or rally point somewhere else
  Shift + right-click  Queue the order after what the selection is already doing
  Shift + left-click   Queue a structure, and keep placing
  Shift + left-drag    Queue a row of structures, and keep placing
  M, then left-click   Move there, whatever is under the cursor
  F, then left-click   Attack-move
  Y, then left-click   Assist a friendly unit: stay with it and protect it
  Alt + right-click    Attack-move
  P, then left-clicks  Add patrol points, then right-click or press P to start
  P + right-click      Hold P and add patrol points, then release P
  X                    Stop
  L, then left-click   Overcharge an enemy with the commander
  O                    Pause or resume the selected factories and construction sites
  Ctrl + 1 to 0        Save the selection as a group, or clear it if nothing is selected.
                       Command works too
  1 to 0               Select that group
  N                    Mute or unmute
  - and =              Sound quieter and louder
  Space                Jump to your commander
  Scroll               Zoom, towards the cursor
  WASD or arrow keys   Pan
  Esc                  Cancel placement or attack-move, or clear the selection

A marker and a line of text confirm each order. A click that shows nothing never reached
the game.";

#[derive(Default)]
struct Options {
    battle: bool,
    headless_ticks: Option<u32>,
    autotest_seconds: Option<u32>,
    fast_forward: u32,
    capture_dir: Option<PathBuf>,
    soak: Option<u64>,
    threads: Option<usize>,
    seed: Option<u64>,
    sound_check: bool,
}

fn parse_options() -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        let number = |name: &str, text: String| text.parse::<u64>().map_err(|_| format!("{name} needs a number"));
        match arg.as_str() {
            "--battle" => options.battle = true,
            "--headless" => options.headless_ticks = Some(number("--headless", value("--headless")?)? as u32),
            "--autotest" => options.autotest_seconds = Some(number("--autotest", value("--autotest")?)? as u32),
            "--fast-forward" => options.fast_forward = number("--fast-forward", value("--fast-forward")?)? as u32,
            "--capture" => options.capture_dir = Some(PathBuf::from(value("--capture")?)),
            "--soak" => options.soak = Some(number("--soak", value("--soak")?)?),
            "--threads" => options.threads = Some(number("--threads", value("--threads")?)? as usize),
            "--seed" => options.seed = Some(number("--seed", value("--seed")?)?),
            "--sound-check" => options.sound_check = true,
            "--help" | "-h" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown option {other}\n\n{USAGE}")),
        }
    }
    Ok(options)
}

fn main() {
    env_logger::init();
    let options = match parse_options() {
        Ok(options) => options,
        Err(message) => {
            println!("{message}");
            return;
        }
    };
    let seed = options.seed.unwrap_or(DEFAULT_SEED);
    if options.sound_check {
        sound_check();
        return;
    }
    if let Some(matches) = options.soak {
        let mut report = run_soak(options.seed.unwrap_or(1), matches, options.threads.unwrap_or(6));
        if !report.print() {
            std::process::exit(1);
        }
        return;
    }
    if let Some(ticks) = options.headless_ticks {
        run_headless(seed, ticks, options.battle);
        return;
    }
    run_windowed(seed, options);
}

fn make_scenario(seed: u64, battle: bool) -> Scenario {
    if battle {
        scenario::default_battle(seed)
    } else {
        scenario::skirmish(seed)
    }
}

fn run_headless(seed: u64, ticks: u32, battle: bool) {
    let scenario = make_scenario(seed, battle);
    let opening = scenario.opening_commands;
    let mut world = scenario.world;
    let mut brains = [AiBrain::new(Team::Blue, seed), AiBrain::new(Team::Red, seed)];
    let started = Instant::now();
    while world.current_tick < ticks && world.match_result.is_none() {
        if battle {
            advance_scripted(&mut world, &opening, seed);
        } else {
            advance_ai_vs_ai(&mut world, &mut brains);
        }
    }
    println!(
        "headless: tick {} checksum {:016x} ({} units, {} projectiles, {:.2} ms per tick)",
        world.current_tick,
        world.compute_checksum(),
        world.units.len(),
        world.projectiles.len(),
        started.elapsed().as_secs_f64() * 1000.0 / f64::from(world.current_tick.max(1)),
    );
}

/// What a test run measures and which screenshots it still owes.
struct Autotest {
    duration: Duration,
    started: Option<Instant>,
    frame_times: Vec<Duration>,
    /// Frame time with the wait for a drawable removed: the work this program actually does.
    work_times: Vec<Duration>,
    /// Frame time paired with altitude, for finding which zoom levels are slow.
    frames_by_altitude: Vec<(f64, Duration)>,
    capture_dir: Option<PathBuf>,
    capture_points: Vec<f64>,
    peak_projectiles: usize,
    peak_instances: usize,
}

impl Autotest {
    fn new(seconds: u32, capture_dir: Option<PathBuf>) -> Self {
        Self {
            duration: Duration::from_secs(u64::from(seconds)),
            started: None,
            frame_times: Vec::new(),
            work_times: Vec::new(),
            frames_by_altitude: Vec::new(),
            capture_dir,
            // Progress through the run at which to take screenshots.
            capture_points: vec![0.02, 0.15, 0.22, 0.30, 0.42, 0.50],
            peak_projectiles: 0,
            peak_instances: 0,
        }
    }

    /// Zooms from the minimum to the maximum altitude and back over the whole run.
    fn altitude_at(progress: f64, min_altitude: f64, max_altitude: f64) -> f64 {
        let sweep = 1.0 - (progress * 2.0 - 1.0).abs();
        (min_altitude.ln() + (max_altitude.ln() - min_altitude.ln()) * sweep).exp()
    }

    fn report(&mut self, result: DriverResult) {
        let mut times = self.frame_times.clone();
        times.sort_unstable();
        if times.is_empty() {
            println!("autotest: no frames recorded");
            return;
        }
        let at = |percent: usize| times[(times.len() * percent / 100).min(times.len() - 1)].as_secs_f64() * 1000.0;
        let total: f64 = times.iter().map(Duration::as_secs_f64).sum();
        let average_fps = times.len() as f64 / total;
        let dropped = times.iter().filter(|time| time.as_secs_f64() * 1000.0 > DROPPED_FRAME_MS).count();
        let mut work = self.work_times.clone();
        work.sort_unstable();
        let work_p99 = work[(work.len() * 99 / 100).min(work.len() - 1)].as_secs_f64() * 1000.0;
        let verdict = |passed: bool| if passed { "PASS" } else { "FAIL" };
        println!("autotest: {} frames over {total:.1} s", times.len());
        println!("  average FPS {average_fps:.1}   limit 59.5: {}", verdict(average_fps >= 59.5));
        println!(
            "  frame interval p50 {:.2} ms   p99 {:.2} ms   max {:.2} ms   p99 limit {DROPPED_FRAME_MS:.0} ms: {}",
            at(50),
            at(99),
            times[times.len() - 1].as_secs_f64() * 1000.0,
            verdict(at(99) <= DROPPED_FRAME_MS),
        );
        println!("  dropped frames (over {DROPPED_FRAME_MS:.0} ms): {dropped}");
        println!("  work per frame p99 {work_p99:.2} ms   limit 16.70 ms: {}", verdict(work_p99 <= 16.7));
        let bands = [(0.0, 100.0), (100.0, 1_000.0), (1_000.0, 10_000.0), (10_000.0, f64::MAX)];
        for (low, high) in bands {
            let mut band: Vec<Duration> = self
                .frames_by_altitude
                .iter()
                .filter(|(altitude, _)| *altitude >= low && *altitude < high)
                .map(|(_, time)| *time)
                .collect();
            band.sort_unstable();
            if !band.is_empty() {
                let at = |percent: usize| band[(band.len() * percent / 100).min(band.len() - 1)].as_secs_f64() * 1000.0;
                println!("  altitude {low:>6.0}+  {:>5} frames   p50 {:>5.2} ms   p99 {:>5.2} ms", band.len(), at(50), at(99));
            }
        }
        println!("  peak {} instances, {} projectiles", self.peak_instances, self.peak_projectiles);
        println!("autotest: tick {} checksum {:016x}", result.tick, result.checksum);
    }
}

fn raw_to_world(raw: i32) -> f64 {
    f64::from(raw) / 4096.0
}

/// The parts of a scenario that never change, for drawing and placement previews.
struct MapInfo {
    heights: HeightField,
    map_units: f64,
    cells_per_side: u32,
    /// Terrain cells covered by walls: the lower corners for drawing, and sorted (y, x) for lookups.
    wall_cells: Vec<([f64; 2], f64)>,
    blocked_cells: Vec<(u32, u32)>,
    deposits: Vec<(u32, u32)>,
    blueprints: Arc<BlueprintDb>,
    fog_side: Option<usize>,
}

fn read_map(scenario: &Scenario) -> MapInfo {
    let world = &scenario.world;
    let cell = f64::from(TERRAIN_CELL_UNITS);
    let heights = HeightField::new(
        world.terrain.samples_per_side(),
        world.terrain.heights().to_vec(),
        cell,
        1.0 / f64::from(1 << HEIGHT_FRAC_BITS),
    );
    let mut wall_cells = Vec::new();
    let mut blocked_cells = Vec::new();
    // Walls are drawn one terrain cell at a time, so that they follow the hills.
    for rect in &world.obstacles {
        let raw = |value: Fp| raw_to_world(value.to_bits());
        let mut y = (raw(rect.min.y) / cell).floor();
        while y * cell < raw(rect.max.y) {
            let mut x = (raw(rect.min.x) / cell).floor();
            while x * cell < raw(rect.max.x) {
                let top = heights.height_at((x + 0.5) * cell, (y + 0.5) * cell) + 1.0;
                wall_cells.push(([x * cell, y * cell], top));
                blocked_cells.push((y as u32, x as u32));
                x += 1.0;
            }
            y += 1.0;
        }
    }
    blocked_cells.sort_unstable();
    MapInfo {
        map_units: f64::from(world.terrain.map_units()),
        cells_per_side: world.terrain.cells_per_side() as u32,
        wall_cells,
        blocked_cells,
        deposits: world.deposits.clone(),
        blueprints: Arc::clone(&world.blueprints),
        fog_side: world.config.fog_of_war.then(|| world.visibility.side()),
        heights,
    }
}

impl MapInfo {
    fn placement_map(&self) -> PlacementMap<'_> {
        PlacementMap {
            cells_per_side: self.cells_per_side,
            blocked_cells: &self.blocked_cells,
            deposits: &self.deposits,
            blueprints: &self.blueprints,
        }
    }
}

/// Everything the window needs from the running simulation.
struct Session {
    driver: Option<SimDriver>,
    previous: Arc<SimSnapshot>,
    current: Arc<SimSnapshot>,
    current_arrived: Instant,
}

impl Session {
    fn start(scenario: Scenario, source: CommandSource, options: DriverOptions) -> Self {
        Self {
            driver: Some(SimDriver::start_with(scenario, source, true, options)),
            previous: Arc::new(SimSnapshot::default()),
            current: Arc::new(SimSnapshot::default()),
            current_arrived: Instant::now(),
        }
    }

    fn send(&self, command: SimCommand) {
        if let Some(driver) = &self.driver {
            driver.send(command);
        }
    }
}

/// The builders in a selection that can build a blueprint.
fn builders_for(selected: &[SimId], snapshot: &SimSnapshot, blueprints: &BlueprintDb, blueprint: BlueprintId) -> Vec<SimId> {
    selected
        .iter()
        .copied()
        .filter(|id| {
            snapshot.units.binary_search_by_key(id, |unit| unit.id).is_ok_and(|index| {
                let unit = &snapshot.units[index];
                unit.blueprint != u16::MAX && blueprints.get(BlueprintId(unit.blueprint)).builds.contains(&blueprint)
            })
        })
        .collect()
}

fn commander_of(snapshot: &SimSnapshot, blueprints: &BlueprintDb) -> Option<SimId> {
    snapshot
        .units
        .iter()
        .find(|unit| unit.team == PLAYER_TEAM && unit.blueprint != u16::MAX && blueprints.get(BlueprintId(unit.blueprint)).is_commander())
        .map(|unit| unit.id)
}

/// Puts an order that was dragged where the cursor is now, if the normal rules allow it there.
fn drop_order(session: &Session, selection: &Selection, camera: &Camera, map: &MapInfo, feedback: &mut OrderFeedback, handle: &OrderHandle, pixels_per_point: f64) {
    let cursor = selection.cursor();
    let ground = camera.screen_to_ground(cursor, &map.heights);
    let Some(point) = ground else {
        feedback.show_rejected(None, "Can't move an order off the map");
        return;
    };
    let goal = Vec2Fp::new(Fp::from_num(point[0]), Fp::from_num(point[1]));
    let position = ground_point(ground, &map.heights);
    let orders = match &handle.order {
        DraggableOrder::Walk { units, task } => vec![SimCommand::MoveOrder { units: units.clone(), task: *task, goal }],
        DraggableOrder::Rally { factories } => factories.iter().map(|factory| SimCommand::SetRally { factory: *factory, point: goal }).collect(),
        DraggableOrder::Build { units, task } => {
            let BuildTask::Build { blueprint, .. } = *task else {
                return;
            };
            let pending = pending_without(pending_builds(&session.current), handle);
            let target = placement_target(&map.placement_map(), blueprint, cursor, ground, camera, &map.heights, &session.current.units, &pending, pixels_per_point);
            match target.filter(|cell| placement_is_valid(&map.placement_map(), blueprint, *cell, &session.current.units, &pending)) {
                Some((cell_x, cell_y)) => vec![SimCommand::MoveBuild { units: units.clone(), task: *task, cell_x, cell_y }],
                // The order stays where it was.
                None => {
                    feedback.show_rejected(position, "Can't build there");
                    return;
                }
            }
        }
    };
    feedback.show_orders(&orders, position, &map.blueprints, "");
    for order in orders {
        session.send(order);
    }
}

/// Queued builds, apart from the one being dragged, which must not block its own new spot.
fn pending_without(mut pending: Vec<PendingBuild>, handle: &OrderHandle) -> Vec<PendingBuild> {
    pending.retain(|build| (build.center[0] - handle.position[0]).abs() + (build.center[1] - handle.position[1]).abs() > 0.01);
    pending
}

/// Plays each sound in turn, to hear them without playing a match.
fn sound_check() {
    let engine = match SoundEngine::start() {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("sound could not start: {error}");
            std::process::exit(1);
        }
    };
    engine.set_volume(SoundSettings::default().volume());
    let wait = |seconds: u64| std::thread::sleep(std::time::Duration::from_secs(seconds));
    for (name, build, factory) in [("one builder", 0.33, 0.0), ("ten builders", 0.83, 0.0), ("one factory", 0.0, 0.33), ("ten factories", 0.0, 0.83)] {
        println!("{name}");
        engine.set_levels(audio::SoundLevels { build, factory });
        wait(3);
    }
    engine.set_levels(audio::SoundLevels::default());
    println!("selecting a unit");
    engine.play_select();
    wait(1);
    println!("ACU under attack");
    engine.play_alarm();
    wait(2);
    println!("{} frames played", engine.frames_played());
}

/// Orders the selection from a click using `plan`, and shows what happened.
/// Returns whether anything was ordered.
#[allow(clippy::too_many_arguments)]
fn issue_planned_orders(
    session: &Session,
    selection: &Selection,
    camera: &Camera,
    map: &MapInfo,
    feedback: &mut OrderFeedback,
    attack_move: bool,
    queued: bool,
    formation: bool,
    reason: &str,
    plan: fn(&RightClick<'_>) -> Vec<SimCommand>,
) -> bool {
    let ground = camera.screen_to_ground(selection.cursor(), &map.heights);
    let orders = plan(&RightClick {
        selected: selection.selected(),
        units: &session.current.units,
        wrecks: &session.current.wrecks,
        blueprints: &map.blueprints,
        team: PLAYER_TEAM,
        camera,
        cursor: selection.cursor(),
        ground,
        attack_move,
    });
    let orders = if queued { queued_commands(orders, &session.current.units) } else { orders };
    let orders = if formation { formation_commands(orders) } else { orders };
    let reason = if selection.selected().is_empty() { "No units selected" } else { reason };
    feedback.show_orders(&orders, ground_point(ground, &map.heights), &map.blueprints, reason);
    let ordered = !orders.is_empty();
    for order in orders {
        session.send(order);
    }
    ordered
}

/// Orders the selection from a click, and shows what happened.
#[allow(clippy::too_many_arguments)]
fn issue_click_orders(
    session: &Session,
    selection: &Selection,
    camera: &Camera,
    map: &MapInfo,
    feedback: &mut OrderFeedback,
    attack_move: bool,
    queued: bool,
    formation: bool,
) {
    issue_planned_orders(session, selection, camera, map, feedback, attack_move, queued, formation, "The selected units cannot do that", right_click_orders);
}

/// Adds a patrol point under the cursor, and shows what to do next.
fn add_patrol_point(selection: &Selection, camera: &Camera, map: &MapInfo, feedback: &mut OrderFeedback, points: &mut Vec<Vec2Fp>, next_step: &str) {
    let ground = camera.screen_to_ground(selection.cursor(), &map.heights);
    if let Some(point) = ground {
        points.push(Vec2Fp::new(Fp::from_num(point[0]), Fp::from_num(point[1])));
        if let Some(position) = ground_point(ground, &map.heights) {
            feedback.add_marker(MarkerKind::Patrol, position, None);
        }
        feedback.set_label(MarkerKind::Patrol, format!("Patrol point {}: {next_step}", points.len()));
    }
}

/// Sends the selection on patrol through the points so far.
fn start_patrol(session: &Session, selection: &Selection, map: &MapInfo, feedback: &mut OrderFeedback, points: &mut Vec<Vec2Fp>) {
    let units = mobile_units(selection.selected(), &session.current.units, &map.blueprints);
    let points = std::mem::take(points);
    if units.is_empty() {
        feedback.show_rejected(None, "No units selected that can patrol");
    } else {
        let order = SimCommand::Patrol { units, points };
        feedback.show_orders(std::slice::from_ref(&order), None, &map.blueprints, "");
        session.send(order);
    }
}

fn stop_selection(session: &Session, selection: &Selection, feedback: &mut OrderFeedback) {
    if selection.selected().is_empty() {
        feedback.show_rejected(None, "No units selected");
    } else {
        feedback.set_label(MarkerKind::Move, format!("Stop: {} units", selection.selected().len()));
        session.send(SimCommand::Stop { units: selection.selected().to_vec() });
    }
}

/// Orders structures at each cell that can take one, and shows what happened.
/// Returns whether anything was ordered.
fn place_structures(
    session: &Session,
    selection: &Selection,
    map: &MapInfo,
    feedback: &mut OrderFeedback,
    blueprint: BlueprintId,
    cells: &[(u32, u32)],
    queued: bool,
) -> bool {
    let unit = map.blueprints.get(blueprint);
    let cell_units = f64::from(TERRAIN_CELL_UNITS);
    let position_of = |cell: (u32, u32)| {
        let half = f64::from(unit.footprint) * cell_units * 0.5;
        let (x, y) = (f64::from(cell.0) * cell_units + half, f64::from(cell.1) * cell_units + half);
        [x, y, map.heights.height_at(x, y)]
    };
    let last = cells[cells.len() - 1];
    let builders = builders_for(selection.selected(), &session.current, &map.blueprints, blueprint);
    if builders.is_empty() {
        feedback.show_rejected(Some(position_of(last)), "No selected unit can build that");
        return false;
    }
    let pending = pending_builds(&session.current);
    let valid: Vec<(u32, u32)> = cells
        .iter()
        .copied()
        .filter(|cell| placement_is_valid(&map.placement_map(), blueprint, *cell, &session.current.units, &pending))
        .collect();
    if valid.is_empty() {
        // Placement stays active, so that the player can try the next spot over.
        feedback.show_rejected(Some(position_of(last)), "Can't build there");
        return false;
    }
    // Only the first can replace existing orders, or each would cancel the one before it.
    for (index, &(cell_x, cell_y)) in valid.iter().enumerate() {
        feedback.add_marker(MarkerKind::Build, position_of((cell_x, cell_y)), None);
        session.send(SimCommand::Build { units: builders.clone(), blueprint, cell_x, cell_y, queued: queued || index > 0 });
    }
    let blocked = cells.len() - valid.len();
    let mut label = match valid.len() {
        1 => format!("Build: {}", unit.name),
        count => format!("Build: {count} x {}", unit.name),
    };
    if blocked > 0 {
        label.push_str(&format!(" ({blocked} blocked)"));
    }
    feedback.set_label(MarkerKind::Build, label);
    true
}

/// A ground position with its height, for placing a marker.
fn ground_point(ground: Option<[f64; 2]>, heights: &HeightField) -> Option<[f64; 3]> {
    ground.map(|ground| [ground[0], ground[1], heights.height_at(ground[0], ground[1])])
}

fn run_windowed(seed: u64, options: Options) {
    let battle = options.battle;
    let scenario = make_scenario(seed, battle);
    let map = read_map(&scenario);
    let is_match = !battle;
    let autotest_seconds = options.autotest_seconds;
    let fast_forward = options.fast_forward;

    let mut autotest = autotest_seconds.map(|seconds| Autotest::new(seconds, options.capture_dir.clone()));
    let source = match (autotest_seconds, battle) {
        (Some(seconds), true) => CommandSource::Script { seed, ticks: fast_forward + seconds * 10 },
        (Some(seconds), false) => CommandSource::AiVsAi { seed, ticks: fast_forward + seconds * 10 },
        (None, true) => CommandSource::Player,
        (None, false) => CommandSource::PlayerVsAi { seed },
    };
    let driver_options = DriverOptions { viewer: is_match.then_some(PLAYER_TEAM), fast_forward_ticks: fast_forward };
    let mut session = Session::start(scenario, source, driver_options);

    let event_loop = EventLoop::new().expect("event loop starts");
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("sim-engine")
            .with_inner_size(LogicalSize::new(1440.0, 900.0))
            .build(&event_loop)
            .expect("window opens"),
    );
    let size = window.inner_size();
    let mut renderer = Renderer::new(Arc::clone(&window), &map.heights, map.fog_side);
    let mut camera = Camera::new(map.map_units, [f64::from(size.width), f64::from(size.height)]);
    if is_match {
        camera.set_min_altitude(SKIRMISH_MIN_ALTITUDE);
        let start = skirmish_start(PLAYER_TEAM);
        camera.look_at(raw_to_world(start.x.to_bits()), raw_to_world(start.y.to_bits()), STARTING_ALTITUDE);
    }

    let egui_context = egui::Context::default();
    let mut egui_state = egui_winit::State::new(
        egui_context.clone(),
        egui::ViewportId::ROOT,
        &window,
        Some(window.scale_factor() as f32),
        None,
    );

    let mut selection = Selection::default();
    let mut placing: Option<BlueprintId> = None;
    let mut armed: Option<ArmedOrder> = None;
    // Where a Shift-drag row of structures began, while the button is held.
    let mut line_start: Option<(u32, u32)> = None;
    let mut orders_view_strength = 0.0_f32;
    let clock = Instant::now();
    let mut feedback = OrderFeedback::default();
    let mut patrol_points: Vec<Vec2Fp> = Vec::new();
    let mut patrol_held = false;
    let mut last_frame = Instant::now();
    let mut frames_seen = 0usize;
    let mut held_keys = [false; 4];
    let (mut alt_held, mut shift_held, mut command_held) = (false, false, false);
    let mut hotkey_actions: Vec<UiAction> = Vec::new();
    let mut quick_select = QuickSelectState::default();
    let mut control_groups = ControlGroups::default();
    // An order being dragged, and where the drag started.
    let mut order_drag: Option<(OrderHandle, [f64; 2])> = None;
    // How many cancelled orders the player has been told about.
    let mut cancelled_seen = 0;
    let mut attack_alarm = AttackAlarm::default();
    let mut sound_settings = SoundSettings::default();
    // Test runs stay silent. The game plays on without sound if there is no device.
    let sound = (is_match && autotest_seconds.is_none()).then(SoundEngine::start).and_then(|started| match started {
        Ok(engine) => Some(engine),
        Err(error) => {
            eprintln!("playing without sound: {error}");
            None
        }
    });
    let mut scene_instances = InstanceBatches::default();
    let mut headings = Headings::new();
    let mut title_timer = Instant::now();
    let mut title_frames = 0u32;
    let mut autotest_selected = false;
    // The stress test's units have no blueprints, and stay plain cubes.
    let looks = instances::unit_looks(&map.blueprints);

    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop
        .run(move |event, target| match event {
            Event::AboutToWait => window.request_redraw(),
            Event::WindowEvent { event, .. } => {
                let response = egui_state.on_window_event(&window, &event);
                match event {
                    WindowEvent::CloseRequested => target.exit(),
                    WindowEvent::Resized(new_size) => {
                        renderer.resize(new_size.width, new_size.height);
                        camera.set_viewport([f64::from(new_size.width.max(1)), f64::from(new_size.height.max(1))]);
                    }
                    WindowEvent::ModifiersChanged(modifiers) => {
                        alt_held = modifiers.state().alt_key();
                        command_held = modifiers.state().super_key() || modifiers.state().control_key();
                        shift_held = modifiers.state().shift_key();
                    }
                    WindowEvent::CursorMoved { position, .. } => selection.move_cursor([position.x, position.y]),
                    WindowEvent::MouseWheel { delta, .. } if !response.consumed => {
                        let steps = match delta {
                            MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                            MouseScrollDelta::PixelDelta(PhysicalPosition { y, .. }) => y / 40.0,
                        };
                        let anchor = camera.screen_to_ground(selection.cursor(), &map.heights);
                        camera.zoom(steps, anchor);
                    }
                    WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } if order_drag.is_some() => {
                        let (handle, start) = order_drag.take().expect("just checked");
                        placing = None;
                        let cursor = selection.cursor();
                        if (cursor[0] - start[0]).hypot(cursor[1] - start[1]) < MIN_ORDER_DRAG_PIXELS {
                            // Barely moved, so this was a Shift-click on whatever is there.
                            selection.set_clock(clock.elapsed().as_secs_f64());
                            selection.begin_drag();
                            selection.finish_drag(&session.current.units, &camera, PLAYER_TEAM, shift_held);
                        } else {
                            drop_order(&session, &selection, &camera, &map, &mut feedback, &handle, window.scale_factor());
                        }
                    }
                    WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => {
                        if let (Some(start), Some(blueprint)) = (line_start.take(), placing) {
                            let unit = map.blueprints.get(blueprint);
                            let footprint = unit.footprint;
                            let end = camera.screen_to_ground(selection.cursor(), &map.heights).map_or(start, |ground| placement_cell(ground, footprint));
                            // Deposits are never in a row, so an extractor stays where it snapped.
                            let cells = if unit.requires_deposit { vec![start] } else { line_cells(start, end, footprint) };
                            let placed = place_structures(&session, &selection, &map, &mut feedback, blueprint, &cells, true);
                            if placed && !shift_held {
                                placing = None;
                            }
                        }
                        selection.set_clock(clock.elapsed().as_secs_f64());
                        selection.finish_drag(&session.current.units, &camera, PLAYER_TEAM, shift_held);
                    }
                    WindowEvent::MouseInput { state: ElementState::Pressed, button, .. } if !response.consumed => match button {
                        MouseButton::Left => {
                            if let Some(order) = armed {
                                match order {
                                    // With Shift, each click queues another, and the order stays armed.
                                    ArmedOrder::Move => {
                                        if !shift_held {
                                            armed = None;
                                        }
                                        issue_planned_orders(&session, &selection, &camera, &map, &mut feedback, false, shift_held, false, "The selected units cannot move", move_orders);
                                    }
                                    ArmedOrder::AttackMove => {
                                        if !shift_held {
                                            armed = None;
                                        }
                                        issue_click_orders(&session, &selection, &camera, &map, &mut feedback, true, shift_held, false);
                                    }
                                    ArmedOrder::Formation => {
                                        if !shift_held {
                                            armed = None;
                                        }
                                        let plan = if alt_held { right_click_orders } else { move_orders };
                                        issue_planned_orders(&session, &selection, &camera, &map, &mut feedback, alt_held, shift_held, true, "The selected units cannot move", plan);
                                    }
                                    ArmedOrder::Patrol => {
                                        add_patrol_point(&selection, &camera, &map, &mut feedback, &mut patrol_points, "right-click or press P to start");
                                    }
                                    ArmedOrder::Overcharge => {
                                        if issue_planned_orders(&session, &selection, &camera, &map, &mut feedback, false, false, false, "Click an enemy to Overcharge", overcharge_orders) {
                                            if !shift_held {
                                                armed = None;
                                            }
                                            // It waits for the energy, so say so.
                                            let cost = map.blueprints.iter().find_map(|unit| unit.overcharge).map_or(0, |overcharge| overcharge.energy.to_num::<i64>());
                                            if session.current.economy.energy < cost {
                                                feedback.show_rejected(None, &format!("Overcharge is waiting for {cost} energy"));
                                            }
                                        }
                                    }
                                    ArmedOrder::Assist => {
                                        // Stays armed after a miss, so that the player can try again.
                                        if issue_planned_orders(&session, &selection, &camera, &map, &mut feedback, false, shift_held, false, "Click a friendly unit to assist", assist_orders) && !shift_held {
                                            armed = None;
                                        }
                                    }
                                }
                            } else if let Some(blueprint) = placing {
                                let ground = camera.screen_to_ground(selection.cursor(), &map.heights);
                                let pending = pending_builds(&session.current);
                                let target = placement_target(
                                    &map.placement_map(),
                                    blueprint,
                                    selection.cursor(),
                                    ground,
                                    &camera,
                                    &map.heights,
                                    &session.current.units,
                                    &pending,
                                    window.scale_factor(),
                                );
                                if let Some(cell) = target {
                                    if shift_held {
                                        // Placed on release, as one building or a row if the mouse moved.
                                        line_start = Some(cell);
                                    } else if place_structures(&session, &selection, &map, &mut feedback, blueprint, &[cell], false) {
                                        placing = None;
                                    }
                                }
                            } else {
                                // With Shift held, an order's icon can be picked up.
                                let handles = if is_match && shift_held { ui::order_handles(&session.current) } else { Vec::new() };
                                match ui::handle_under_cursor(&handles, &camera, selection.cursor(), window.scale_factor()) {
                                    Some(index) => {
                                        let handle = handles[index].clone();
                                        if let DraggableOrder::Build { task: BuildTask::Build { blueprint, .. }, .. } = handle.order {
                                            // Shows the same preview, grid, and deposits as placing a new one.
                                            placing = Some(blueprint);
                                        }
                                        order_drag = Some((handle, selection.cursor()));
                                    }
                                    None => selection.begin_drag(),
                                }
                            }
                        }
                        MouseButton::Right => {
                            line_start = None;
                            if order_drag.take().is_some() {
                                // Right-click cancels the drag, and the order stays where it was.
                                placing = None;
                            } else if armed == Some(ArmedOrder::Patrol) && !patrol_points.is_empty() {
                                armed = None;
                                start_patrol(&session, &selection, &map, &mut feedback, &mut patrol_points);
                            } else if armed.take().is_some() || placing.take().is_some() {
                                // Right-click only cancels what was armed.
                                patrol_points.clear();
                            } else if patrol_held {
                                add_patrol_point(&selection, &camera, &map, &mut feedback, &mut patrol_points, "release P to start");
                            } else {
                                // Ctrl (or Cmd) makes a move a formation move.
                                issue_click_orders(&session, &selection, &camera, &map, &mut feedback, alt_held, shift_held, command_held);
                            }
                        }
                        _ => {}
                    },
                    WindowEvent::KeyboardInput { event, .. } if !response.consumed => {
                        let pressed = event.state == ElementState::Pressed;
                        match event.physical_key {
                            PhysicalKey::Code(KeyCode::KeyW | KeyCode::ArrowUp) => held_keys[0] = pressed,
                            PhysicalKey::Code(KeyCode::KeyS | KeyCode::ArrowDown) => held_keys[1] = pressed,
                            PhysicalKey::Code(KeyCode::KeyA | KeyCode::ArrowLeft) => held_keys[2] = pressed,
                            PhysicalKey::Code(KeyCode::KeyD | KeyCode::ArrowRight) => held_keys[3] = pressed,
                            PhysicalKey::Code(KeyCode::Escape) if pressed && order_drag.is_some() => {
                                order_drag = None;
                                placing = None;
                            }
                            PhysicalKey::Code(KeyCode::Escape) if pressed => {
                                line_start = None;
                                let was_armed = armed.take().is_some();
                                patrol_points.clear();
                                if placing.take().is_none() && !was_armed {
                                    selection.clear();
                                }
                            }
                            PhysicalKey::Code(KeyCode::Space) if pressed && is_match => {
                                if let Some(commander) = commander_of(&session.current, &map.blueprints) {
                                    let index = session.current.units.binary_search_by_key(&commander, |unit| unit.id).expect("found in this list");
                                    let unit = &session.current.units[index];
                                    camera.look_at(raw_to_world(unit.x), raw_to_world(unit.y), 0.0);
                                    selection.select(vec![commander]);
                                }
                            }
                            PhysicalKey::Code(KeyCode::KeyP) => {
                                if pressed {
                                    patrol_held = true;
                                } else {
                                    patrol_held = false;
                                    if !patrol_points.is_empty() {
                                        armed = None;
                                        start_patrol(&session, &selection, &map, &mut feedback, &mut patrol_points);
                                    } else if ui::has_mobile_units(&session.current, selection.selected()) {
                                        // A tap, with no points placed while holding it.
                                        armed = Some(ArmedOrder::Patrol);
                                        placing = None;
                                        line_start = None;
                                    } else if selection.selected().is_empty() {
                                        feedback.show_rejected(None, "No units selected");
                                    } else {
                                        feedback.show_rejected(None, "Not available for this selection");
                                    }
                                }
                            }
                            // Control or Command saves a group, and the number alone brings it back.
                            PhysicalKey::Code(code) if pressed && !event.repeat && is_match && hotkeys::group_for(code).is_some() => {
                                let group = hotkeys::group_for(code).expect("just checked");
                                if command_held {
                                    if selection.selected().is_empty() {
                                        // Saving nothing means forgetting what was saved.
                                        if control_groups.clear(group) {
                                            feedback.set_label(MarkerKind::Move, format!("Group {group} cleared"));
                                        } else {
                                            feedback.show_rejected(None, &format!("Group {group} is already empty"));
                                        }
                                    } else {
                                        control_groups.store(group, selection.selected());
                                        feedback.set_label(MarkerKind::Move, format!("Group {group} saved: {} units", selection.selected().len()));
                                    }
                                } else {
                                    let members = control_groups.recall(group, &session.current.units).to_vec();
                                    if members.is_empty() {
                                        feedback.show_rejected(None, &format!("Group {group} is empty"));
                                    } else {
                                        selection.select(members);
                                        armed = None;
                                        placing = None;
                                        line_start = None;
                                        patrol_points.clear();
                                    }
                                }
                            }
                            PhysicalKey::Code(KeyCode::KeyN) if pressed && !event.repeat && !command_held => sound_settings.toggle_mute(),
                            PhysicalKey::Code(KeyCode::Minus) if pressed && !command_held => sound_settings.quieter(),
                            PhysicalKey::Code(KeyCode::Equal) if pressed && !command_held => sound_settings.louder(),
                            // Command and Control belong to the system: Command-Q must not start a factory.
                            PhysicalKey::Code(code) if pressed && !event.repeat && !command_held => {
                                if let Some(letter) = hotkeys::letter_for(code) {
                                    match ui::hotkey_action(&session.current, selection.selected(), &map.blueprints, letter) {
                                        Some(action) => hotkey_actions.push(ui::with_shift(action, shift_held, &map.blueprints)),
                                        None if selection.selected().is_empty() => feedback.show_rejected(None, "No units selected"),
                                        None => feedback.show_rejected(None, "Not available for this selection"),
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    WindowEvent::RedrawRequested => {
                        let now = Instant::now();
                        let frame_time = now - last_frame;
                        last_frame = now;
                        let dt = frame_time.as_secs_f64().min(0.1);

                        // Take the newest snapshots. The frame draws between the last two.
                        if let Some(driver) = session.driver.as_ref() {
                            let mut arrived = false;
                            while let Some(snapshot) = driver.try_snapshot() {
                                session.previous = std::mem::replace(&mut session.current, snapshot);
                                session.current_arrived = now;
                                arrived = true;
                            }
                            if arrived {
                                if let Some(fog) = &session.current.fog {
                                    renderer.update_fog(fog);
                                }
                                let units = &session.current.units;
                                headings.retain(|id, _| units.binary_search_by_key(id, |unit| unit.id).is_ok());
                            }
                        }
                        selection.retain_alive(&session.current.units);
                        let alpha = ((now - session.current_arrived).as_secs_f64() / TICK_DURATION.as_secs_f64()).clamp(0.0, 1.0);

                        let pan_x = f64::from(u8::from(held_keys[3])) - f64::from(u8::from(held_keys[2]));
                        let pan_y = f64::from(u8::from(held_keys[0])) - f64::from(u8::from(held_keys[1]));
                        camera.pan(pan_x, pan_y, dt);

                        // Test runs follow a fixed zoom sweep, with the commander selected
                        // so that screenshots show the build menu.
                        let mut progress = None;
                        if let Some(test) = autotest.as_mut() {
                            let started = *test.started.get_or_insert(now);
                            let fraction = ((now - started).as_secs_f64() / test.duration.as_secs_f64()).min(1.0);
                            camera.set_altitude(Autotest::altitude_at(fraction, camera.min_altitude(), camera.max_altitude()));
                            progress = Some(fraction);
                            if is_match && !autotest_selected {
                                if let Some(commander) = commander_of(&session.current, &map.blueprints) {
                                    selection.select(vec![commander]);
                                    autotest_selected = true;
                                }
                            }
                        }
                        camera.update(dt, &map.heights);
                        feedback.update(dt);

                        // Sound follows what is really happening in view.
                        let commander = commander_of(&session.current, &map.blueprints).and_then(|id| {
                            let index = session.current.units.binary_search_by_key(&id, |unit| unit.id).ok()?;
                            Some((id, session.current.units[index].health_permille))
                        });
                        let attacked = is_match && attack_alarm.update(commander, dt);
                        let cancelled = session.current.orders_cancelled;
                        if cancelled > cancelled_seen {
                            feedback.show_rejected(None, "Can't reach there: order cancelled");
                        }
                        // A new match starts counting again.
                        cancelled_seen = cancelled;
                        let picked = selection.take_picked();
                        if let Some(sound) = &sound {
                            if picked {
                                sound.play_select();
                            }
                            let viewport = camera.viewport();
                            let levels = audio::sound_levels(&session.current, camera.zoom_from_closest(), |unit| {
                                camera
                                    .world_to_screen([raw_to_world(unit.x), raw_to_world(unit.y), raw_to_world(unit.z)])
                                    .is_some_and(|at| (0.0..=viewport[0]).contains(&at[0]) && (0.0..=viewport[1]).contains(&at[1]))
                            });
                            sound.set_levels(levels);
                            sound.set_volume(sound_settings.volume());
                            if attacked {
                                sound.play_alarm();
                            }
                        }
                        if selection.selected().is_empty() {
                            armed = None;
                            if !patrol_held {
                                patrol_points.clear();
                            }
                        }

                        // Screenshots show one of each marker and a queue of builds, since test runs have no clicks.
                        let mut demo_snapshot = None;
                        if let (Some(test), Some(fraction)) = (autotest.as_ref(), progress) {
                            if test.capture_dir.is_some() && test.capture_points.first().is_some_and(|&point| fraction >= point) {
                                let mut snapshot = (*session.current).clone();
                                let own: Vec<SimId> = snapshot.units.iter().filter(|unit| unit.team == PLAYER_TEAM).map(|unit| unit.id).collect();
                                if let Some(commander) = commander_of(&snapshot, &map.blueprints) {
                                    let index = snapshot.units.binary_search_by_key(&commander, |unit| unit.id).expect("found in this list");
                                    let (x, y, z) = (snapshot.units[index].x, snapshot.units[index].y, snapshot.units[index].z);
                                    let reach = (camera.altitude() * 0.15 * 4096.0) as i32;
                                    let queued = |dx: i32, dy: i32, width: i32| PlanStep {
                                        kind: PlanStepKind::Build,
                                        x: x + dx * reach,
                                        y: y + dy * reach,
                                        z,
                                        pending_width: width << 12,
                                    };
                                    let steps = vec![queued(1, 1, 0), queued(3, 1, 60), queued(3, -1, 20), queued(1, -2, 20)];
                                    snapshot.plans.retain(|plan| plan.id != commander);
                                    let at = snapshot.plans.partition_point(|plan| plan.id < commander);
                                    snapshot.plans.insert(at, UnitPlanView { id: commander, steps });
                                }
                                // Early screenshots show true colors, and later ones the selection's trails.
                                if fraction >= 0.2 {
                                    selection.select(own);
                                } else {
                                    selection.clear();
                                }
                                demo_snapshot = Some(Arc::new(snapshot));
                                quick_select.expand("Land Factory");
                                // Place a generator at the middle of the screen, to show the preview and grid.
                                selection.move_cursor([camera.viewport()[0] * 0.55, camera.viewport()[1] * 0.3]);
                                if fraction >= 0.4 {
                                    // Zoomed out, show how extractors find deposits.
                                    placing = Some(map.blueprints.id("mass_extractor"));
                                    line_start = None;
                                } else {
                                    placing = Some(map.blueprints.id("power_generator"));
                                    let anchor = [camera.viewport()[0] * 0.3, camera.viewport()[1] * 0.3];
                                    line_start = camera.screen_to_ground(anchor, &map.heights).map(|ground| placement_cell(ground, 1));
                                }
                                let kinds = [
                                    MarkerKind::Move,
                                    MarkerKind::AttackMove,
                                    MarkerKind::Attack,
                                    MarkerKind::Support,
                                    MarkerKind::Rally,
                                    MarkerKind::Build,
                                    MarkerKind::Rejected,
                                ];
                                let focus = camera.focus();
                                for (index, kind) in kinds.into_iter().enumerate() {
                                    let x = focus[0] + (index as f64 - 3.0) * camera.altitude() * 0.12;
                                    feedback.add_marker(kind, [x, focus[1], map.heights.height_at(x, focus[1])], None);
                                }
                                feedback.set_label(MarkerKind::AttackMove, "Attack-move: 12 units".to_string());
                            }
                        }

                        // The interface.
                        let mut actions = std::mem::take(&mut hotkey_actions);
                        let raw_input = egui_state.take_egui_input(&window);
                        let pixels_per_point = window.scale_factor() as f32;
                        let snapshot = demo_snapshot.unwrap_or_else(|| Arc::clone(&session.current));
                        let selected = selection.selected().to_vec();
                        let trails = if is_match { ui::build_trails(&session.previous, &snapshot, alpha, &selected) } else { Vec::new() };
                        orders_view_strength = if shift_held {
                            1.0
                        } else {
                            (f64::from(orders_view_strength) - dt / ORDERS_FADE_SECONDS).max(0.0) as f32
                        };
                        let mut trails = trails;
                        if is_match && orders_view_strength > 0.0 {
                            trails.extend(ui::other_units_trails(&session.previous, &snapshot, alpha, &selected, orders_view_strength));
                        }
                        let beams = if is_match { ui::work_beams(&session.previous, &snapshot, alpha) } else { Vec::new() };
                        let beam_time = clock.elapsed().as_secs_f64();
                        let pending = match &order_drag {
                            Some((handle, _)) => pending_without(pending_builds(&snapshot), handle),
                            None => pending_builds(&snapshot),
                        };
                        // Icons for moving orders, while Shift shows them.
                        let handles = if is_match && orders_view_strength > 0.0 { ui::order_handles(&snapshot) } else { Vec::new() };
                        let can_grab = shift_held && armed.is_none() && placing.is_none() && order_drag.is_none();
                        let hovered_handle = if can_grab { ui::handle_under_cursor(&handles, &camera, selection.cursor(), window.scale_factor()) } else { None };
                        let cursor_ground = camera.screen_to_ground(selection.cursor(), &map.heights);
                        let placement_spot = placing.and_then(|blueprint| {
                            placement_target(&map.placement_map(), blueprint, selection.cursor(), cursor_ground, &camera, &map.heights, &snapshot.units, &pending, window.scale_factor())
                        });
                        let grid_lines = placement_spot
                            .map(|cell| {
                                let height = deposit_position(cell, &map.heights)[2];
                                ui::placement_grid_lines(cell, f64::from(TERRAIN_CELL_UNITS), height, map.cells_per_side)
                            })
                            .unwrap_or_default();
                        // While placing an extractor, mark the deposits it can go on.
                        let extractor = placing.filter(|blueprint| map.blueprints.get(*blueprint).requires_deposit);
                        let open_deposits: Vec<[f64; 3]> = extractor.map_or_else(Vec::new, |blueprint| {
                            free_deposits(&map.placement_map(), blueprint, &snapshot.units, &pending)
                                .into_iter()
                                .map(|cell| deposit_position(cell, &map.heights))
                                .collect()
                        });
                        let snapped_deposit = placement_spot
                            .filter(|cell| extractor.is_some() && map.deposits.contains(cell))
                            .map(|cell| deposit_position(cell, &map.heights));
                        let group_rows = control_groups.in_use(&snapshot.units);
                        let output = egui_context.run(raw_input, |ctx| {
                            ui::release_keyboard_focus(ctx);
                            ui::placement_grid(ctx, &camera, &grid_lines, f64::from(TERRAIN_CELL_UNITS), pixels_per_point);
                            ui::deposit_highlights(ctx, &camera, &open_deposits, snapped_deposit, pixels_per_point);
                            ui::pending_build_outlines(ctx, &camera, &pending, pixels_per_point);
                            ui::distant_selection_markers(ctx, &camera, &snapshot, &selected, pixels_per_point);
                            ui::draw_work_beams(ctx, &camera, &beams, beam_time, pixels_per_point);
                            ui::order_trails(ctx, &camera, &trails, pixels_per_point);
                            match &order_drag {
                                Some((handle, _)) => {
                                    ui::draw_dragged_order(ctx, &camera, handle, selection.cursor(), pixels_per_point);
                                    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                                }
                                None => ui::draw_order_handles(ctx, &camera, &handles, hovered_handle, orders_view_strength, pixels_per_point),
                            }
                            if hovered_handle.is_some() {
                                ctx.set_cursor_icon(egui::CursorIcon::Grab);
                            }
                            ui::order_feedback(ctx, &camera, &snapshot, &feedback, pixels_per_point);
                            if let Some(order) = armed {
                                ui::armed_order_hint(ctx, order);
                            }
                            if (armed.is_some() || placing.is_some()) && order_drag.is_none() {
                                ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                            }
                            if is_match {
                                ui::world_overlay(ctx, &camera, &snapshot, &selected, &map.blueprints, pixels_per_point);
                                let sound_label = sound.as_ref().map(|_| sound_settings.label());
                                ui::resource_bar(ctx, &snapshot, sound_label.as_deref());
                                ui::commander_alert(ctx, attack_alarm.alert_strength());
                                ui::control_group_bar(ctx, &group_rows, &snapshot, PLAYER_TEAM, &map.blueprints, &selected, &mut actions);
                                ui::quick_select_bar(ctx, &snapshot, PLAYER_TEAM, &map.blueprints, &selected, camera.focus(), &mut quick_select, &mut actions);
                                ui::command_panel(ctx, &snapshot, &selected, &map.blueprints, armed, &mut actions);
                                if let Some(blueprint) = placing.filter(|_| order_drag.is_none()) {
                                    ui::placement_hint(ctx, &map.blueprints, blueprint);
                                }
                                if let Some(result) = snapshot.match_result {
                                    ui::match_end_window(ctx, result, PLAYER_TEAM, &mut actions);
                                }
                            }
                        });
                        egui_state.handle_platform_output(&window, output.platform_output);
                        let ui_frame = UiFrame {
                            primitives: egui_context.tessellate(output.shapes, output.pixels_per_point),
                            textures: output.textures_delta,
                            screen: egui_wgpu::ScreenDescriptor {
                                size_in_pixels: [camera.viewport()[0] as u32, camera.viewport()[1] as u32],
                                pixels_per_point: output.pixels_per_point,
                            },
                        };
                        for action in actions {
                            match action {
                                UiAction::BeginPlacement(blueprint) => {
                                    placing = Some(blueprint);
                                    // A row being dragged out was for the previous structure.
                                    line_start = None;
                                    armed = None;
                                    patrol_points.clear();
                                }
                                UiAction::Queue { factories, blueprint, selected_count, count } => {
                                    let unit = &map.blueprints.get(blueprint).name;
                                    let name = if count > 1 { format!("{unit} x{count}") } else { unit.clone() };
                                    feedback.set_label(MarkerKind::Build, ui::queue_label(&name, factories.len(), selected_count));
                                    for factory in factories {
                                        session.send(SimCommand::QueueUnit { factory, blueprint, count });
                                    }
                                }
                                UiAction::SetRepeat { factories, repeat } => {
                                    let state = if repeat { "on" } else { "off" };
                                    feedback.set_label(MarkerKind::Build, format!("Repeat {state}: {}", ui::factory_count(factories.len())));
                                    for factory in factories {
                                        session.send(SimCommand::SetRepeat { factory, repeat });
                                    }
                                }
                                UiAction::ClearQueue { factories } => {
                                    feedback.set_label(MarkerKind::Build, format!("Queue cleared: {}", ui::factory_count(factories.len())));
                                    for factory in factories {
                                        session.send(SimCommand::ClearQueue { factory });
                                    }
                                }
                                UiAction::Arm(order) => {
                                    armed = Some(order);
                                    placing = None;
                                    line_start = None;
                                    patrol_points.clear();
                                }
                                UiAction::Stop => stop_selection(&session, &selection, &mut feedback),
                                UiAction::SetPaused { units, paused } => {
                                    let word = if paused { "Paused" } else { "Resumed" };
                                    feedback.set_label(MarkerKind::Build, format!("{word}: {}", units.len()));
                                    session.send(SimCommand::SetPaused { units, paused });
                                }
                                UiAction::QuickSelect { units, add } => {
                                    if add {
                                        selection.add(&units);
                                    } else {
                                        selection.select(units);
                                    }
                                    armed = None;
                                    placing = None;
                                    line_start = None;
                                    patrol_points.clear();
                                }
                                UiAction::CancelTask { units, task } => {
                                    feedback.set_label(MarkerKind::Build, format!("{} removed", ui::task_name(&task, &map.blueprints)));
                                    session.send(SimCommand::CancelTask { units, task });
                                }
                                UiAction::RemoveQueued { factories, blueprint } => {
                                    feedback.set_label(MarkerKind::Build, format!("{} removed", map.blueprints.get(blueprint).name));
                                    for factory in factories {
                                        session.send(SimCommand::RemoveQueued { factory, blueprint });
                                    }
                                }
                                UiAction::Restart => {
                                    attack_alarm = AttackAlarm::default();
                                    control_groups = ControlGroups::default();
                                    order_drag = None;
                                    session = Session::start(scenario::skirmish(seed), CommandSource::PlayerVsAi { seed }, driver_options);
                                    selection.clear();
                                    placing = None;
                                    line_start = None;
                                    armed = None;
                                    patrol_points.clear();
                                }
                            }
                        }

                        // The scene.
                        let eye = camera.eye();
                        let (previous, current) = (&session.previous, &session.current);
                        scene_instances.clear();
                        instances::build_unit_instances(&previous.units, &current.units, alpha, eye, selection.selected(), &looks, &current.work_links, &mut headings, &mut scene_instances);
                        let drawn_units_and_shells = {
                            let units = scene_instances.total();
                            instances::build_projectile_instances(&previous.projectiles, &current.projectiles, alpha, eye, &mut scene_instances);
                            units + current.projectiles.len()
                        };
                        let cell = f64::from(TERRAIN_CELL_UNITS);
                        for &(min, top) in &map.wall_cells {
                            let max = [min[0] + cell, min[1] + cell];
                            scene_instances.push(Shape::Cube, instances::obstacle_instance(min, max, top - OBSTACLE_DRAW_HEIGHT, OBSTACLE_DRAW_HEIGHT, eye));
                        }
                        for &(x, y) in &map.deposits {
                            let (center_x, center_y) = ((f64::from(x) + 0.5) * cell, (f64::from(y) + 0.5) * cell);
                            let center = [center_x, center_y, map.heights.height_at(center_x, center_y) + 0.5];
                            scene_instances.push(Shape::Cube, instances::deposit_instance(center, cell * 0.8, eye));
                        }
                        for wreck in &current.wrecks {
                            let center = [raw_to_world(wreck.x), raw_to_world(wreck.y), raw_to_world(wreck.z)];
                            scene_instances.push(Shape::Wreck, instances::wreck_instance(center, raw_to_world(wreck.radius), eye));
                        }
                        if let Some(blueprint) = placing {
                            if let Some(spot) = placement_spot {
                                let unit = map.blueprints.get(blueprint);
                                // While Shift-dragging, preview the whole row. Extractors stay where they snapped.
                                let spots = match line_start {
                                    Some(start) if unit.requires_deposit => vec![start],
                                    Some(start) => line_cells(start, spot, unit.footprint),
                                    None => vec![spot],
                                };
                                let queued = pending_builds(current);
                                let width = f64::from(unit.footprint) * cell;
                                for spot in spots {
                                    let valid = placement_is_valid(&map.placement_map(), blueprint, spot, &current.units, &queued);
                                    let (center_x, center_y) = (f64::from(spot.0) * cell + width * 0.5, f64::from(spot.1) * cell + width * 0.5);
                                    let ground = [center_x, center_y, map.heights.height_at(center_x, center_y)];
                                    let look = instances::unit_look(&unit.key);
                                    scene_instances.push(look.shape, instances::ghost_instance(ground, width, look.height_share, valid, eye));
                                }
                            }
                        }
                        let scene = FrameScene {
                            camera: &camera,
                            instances: &scene_instances,
                            selection_box: selection.drag_box(),
                        };

                        renderer.begin_ui(&ui_frame);
                        if let (Some(test), Some(fraction)) = (autotest.as_mut(), progress) {
                            if test.capture_points.first().is_some_and(|&point| fraction >= point) {
                                let point = test.capture_points.remove(0);
                                if let Some(directory) = &test.capture_dir {
                                    let path = directory.join(format!("zoom_{:03}.bmp", (point * 100.0) as u32));
                                    match renderer.capture(&scene, Some(&ui_frame), &path) {
                                        Ok(()) => println!(
                                            "captured {} at altitude {:.0}, {} units, {} projectiles",
                                            path.display(), camera.altitude(), session.current.units.len(), session.current.projectiles.len()
                                        ),
                                        Err(error) => println!("capture failed: {error}"),
                                    }
                                    // A capture is not part of the frame being measured.
                                    last_frame = Instant::now();
                                }
                            }
                        }
                        // Measured frame times span from the last frame's end to this one's,
                        // so they include the drawable wait from the frame before.
                        let previous_acquire = renderer.last_acquire();
                        renderer.render(&scene, Some(&ui_frame));
                        renderer.end_ui(&ui_frame);

                        frames_seen += 1;
                        if let Some(test) = autotest.as_mut() {
                            if frames_seen > WARMUP_FRAMES {
                                test.frame_times.push(frame_time);
                                test.work_times.push(frame_time.saturating_sub(previous_acquire));
                                test.frames_by_altitude.push((camera.altitude(), frame_time));
                            }
                            test.peak_projectiles = test.peak_projectiles.max(session.current.projectiles.len());
                            test.peak_instances = test.peak_instances.max(drawn_units_and_shells);
                            if session.driver.as_ref().is_some_and(SimDriver::is_finished) {
                                let result = session.driver.take().expect("driver is present").finish();
                                test.report(result);
                                target.exit();
                            }
                        }

                        // The FPS counter.
                        title_frames += 1;
                        let elapsed = title_timer.elapsed();
                        if elapsed >= Duration::from_millis(500) {
                            let fps = f64::from(title_frames) / elapsed.as_secs_f64();
                            let tick_ms = session.driver.as_ref().map_or(0.0, |driver| f64::from(driver.last_tick_micros()) / 1000.0);
                            let blue = session.current.units.iter().filter(|unit| unit.team == Team::Blue).count();
                            let red = session.current.units.len() - blue;
                            window.set_title(&format!(
                                "sim-engine | {fps:.0} FPS | tick {tick_ms:.1} ms | Blue {blue} vs Red {red} visible | {} shells | {} selected",
                                session.current.projectiles.len(),
                                selection.selected().len(),
                            ));
                            title_timer = Instant::now();
                            title_frames = 0;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        })
        .expect("event loop runs");
}
