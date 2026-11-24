//! Application entry point, window loop, and mode switch (headless vs windowed).
#![allow(clippy::disallowed_types)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use web_time::Instant;

use sim_engine::ai::{advance_ai_vs_ai, AiBrain, Difficulty};
use sim_engine::ai_strategy::Strategy;
use sim_engine::driver::{CommandSource, DriverOptions, DriverResult, SimDriver, TICK_DURATION};
use sim_engine::audio::{self, AttackAlarm, SoundEngine, SoundSettings};
use sim_engine::input::feedback::{MarkerKind, OrderFeedback};
use sim_engine::input::hotkeys;
use sim_engine::input::orders::{
    assist_orders, attack_ground_orders, capture_orders, deposit_position, formation_commands, free_deposits, site_position, line_cells, mobile_units, move_orders, discharge_orders, pending_builds, queued_commands, PendingBuild, placement_cell, placement_is_valid, placement_target, right_click_orders, PlacementMap, RightClick,
};
use sim_engine::input::selection::{group_center, ControlGroups, Selection};
use sim_engine::math::fp::{Fp, Vec2Fp};
use sim_engine::render::camera::{Camera, HeightField};
use sim_engine::render::effects::Effects;
use sim_engine::icons::ICON_ALTITUDE;
use sim_engine::render::instances::{self, InstanceBatches, Motion};
use sim_engine::render::links;
use sim_engine::render::meshes::Shape;
use sim_engine::render::pipeline::{FrameScene, Renderer, UiFrame};
use sim_engine::sim::blueprints::{BlueprintDb, BlueprintId};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{SimId, Team};
use sim_engine::sim::config::TERRAIN_CELL_UNITS;
use sim_engine::sim::scenario::{self, advance_scripted, MapSource, Scenario};
use sim_engine::sim::terrain::HEIGHT_FRAC_BITS;
use sim_engine::sim::world::{PlanStep, PlanStepKind, SensorRangeView, ShieldView, SimSnapshot, UnitPlanView};
use sim_engine::soak::run_soak;
use sim_engine::sim::components::BuildTask;
use sim_engine::ui::{self, ArmedOrder, DraggableOrder, OrderHandle, QuickSelectState, UiAction};
#[cfg(not(target_arch = "wasm32"))]
use winit::dpi::LogicalSize;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowBuilder;

const DEFAULT_SEED: u64 = 0x5EED_2026;
const PLAYER_TEAM: Team = Team::Blue;
/// How far the camera orbits for each pixel the mouse moves with Space held: across a wide
/// screen is most of a turn.
const ORBIT_RADIANS_PER_PIXEL: f64 = 0.004;
const ORBIT_DEGREES_PER_PIXEL: f64 = 0.08;
/// A press of Space shorter than this, with no orbit, is a tap.
const SPACE_TAP: Duration = Duration::from_millis(300);
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
/// How high a watcher who sees everything starts, as a share of the highest the camera goes.
const WATCH_ALTITUDE_FRACTION: f64 = 0.55;

/// The soak's match for a seed, played out to the soak's time limit.
/// `--ai`'s value: a difficulty, and "turtle" for Normal playing the Turtle strategy, as it was
/// once a difficulty of its own.
fn parse_opponent(name: &str) -> Option<(Difficulty, Option<String>)> {
    match name {
        "turtle" => Some((Difficulty::Normal, Some("turtle".to_string()))),
        _ => Difficulty::from_name(name).map(|difficulty| (difficulty, None)),
    }
}

/// `--teams`' value, as players a side: "2v2" is 2.
fn parse_teams(name: &str) -> Option<usize> {
    let (a, b) = name.split_once('v')?;
    let (a, b): (usize, usize) = (a.parse().ok()?, b.parse().ok()?);
    (a == b && (1..=4).contains(&a)).then_some(a)
}

/// Whether `--strategy` may be this: a strategy's id, or "random".
fn is_strategy(name: &str) -> bool {
    name == "random" || Strategy::named(name).is_some()
}

/// The strategies' ids, for messages.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn strategy_names() -> String {
    Strategy::all().iter().map(|strategy| strategy.id.as_str()).collect::<Vec<_>>().join(", ")
}

const fn watch_source(seed: u64) -> CommandSource {
    CommandSource::AiVsAi { seed, ticks: sim_engine::soak::MATCH_TICK_LIMIT }
}

/// A map's file name as players see it: `twin_passes` as Twin Passes.
fn display_name(name: &str) -> String {
    name.split('_')
        .map(|word| {
            let mut letters = word.chars();
            letters.next().map_or_else(String::new, |first| first.to_uppercase().chain(letters).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}
/// A Shift-press on an order's icon that moves less than this is only a click.
const MIN_ORDER_DRAG_PIXELS: f64 = 6.0;
/// How long the Shift view of everyone's orders takes to fade after letting go, in seconds.
const ORDERS_FADE_SECONDS: f64 = 1.0;

// The browser build takes its options from the page address instead.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
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
  --strategies <blue>,<red>
                       With --soak, the strategies Blue and Red play (default
                       balanced,balanced)
  --balance            Measure the unit data with the balance harness's duels (BALANCE.md)
  --balance-derive <t1|t2|t3|t1-air>
                       Derive a tier of land units' numbers, or T1 air's, from
                       data/balance_targets.toml, priced by duels against the tiers below as
                       the unit data has them
  --no-tick-gate       With --soak, report tick times without failing on them, for
                       shared machines such as CI runners
  --seed <number>      Scenario seed
  --sound-check        Play each sound in turn, and exit
  --teams <1v1|2v2>    Players a side (default 1v1): 2v2 puts an AI ally beside you, against
                       two AIs, on a map with team starts, such as saddleback
  --map <name>         Play on a map that comes with the game: highland, twin_passes, or saddleback,
                       for any of the modes above, or playtest: a sandbox with 5 of every
                       unit and no commander, against two coastal bases that a turtling
                       AI holds
  --ai <easy|normal>   How well the AI opponent plays (default easy; normal in the
                       playtest). --ai turtle is normal with --strategy turtle
  --strategy <name>    What the AI opponent plays (data/ai_strategies.toml): balanced
                       (the default; turtle in the playtest), rush, tech, air, naval,
                       turtle, which builds up and defends and never attacks, or random
  --watch              Watch the AI-vs-AI match the soak plays for --map and --seed,
                       seeing everything. V switches to Blue's or Red's sight, - and =
                       change the speed, a tap of Space pauses, and Space held
                       with the mouse turns the camera
  --view <all|blue|red>  With --watch, whose sight to start with (default all)
  --fog <on|off>       Fog of war, for every team and the AI too (default on)
  --scenario <id>      Play a playtest scenario from data/scenarios.toml: a balance fight,
                       with Blue's units yours and Red's scripted (README, Playtest scenarios)

Controls:
  Left-drag or click   Select your units. A box over fighting units and engineers or
                       structures selects only the fighting units, commander included
  Double-click         Select all your units of that type on screen
  Shift + click        Add a unit to the selection, or remove it
  Shift (hold)         Show every unit's pending orders
  Shift + drag         Add a box of units to the selection
  Right-click          Attack an enemy, assist or repair a friend, reclaim a wreck,
                       tree, or rock, move to the ground, or set a factory's rally point
  Shift + a factory unit  Queue 5 at a time, by button or key
  Shift + drag an icon Move an order or rally point somewhere else
  Shift + right-click  Queue the order after what the selection is already doing
  Shift + left-click   Queue a structure, and keep placing
  Shift + left-drag    Queue a row of structures, and keep placing
  M, then left-click   Move there, whatever is under the cursor
  F, then left-click   Attack-move
  Y, then left-click   Assist a friendly unit: stay with it and protect it
  Alt + right-click    Attack-move
  Ctrl + right-click   Formation move: travel together at the slowest unit's pace.
                       Add Alt to attack-move. Command works too
  J, then left-click   Formation move. Hold Alt when clicking to attack-move
  I, then left-click   Attack ground: keep firing at a spot, even out of sight.
                       Shells that land on debris destroy it, and its materiel
  P, then left-click   Patrol there, from where the units are. Shift queues it after their
                       orders, and each further Shift-click adds a point to the loop
  P + right-click      Hold P: the first right-click patrols, the rest add points
  X                    Stop
  L, then left-click   Discharge an enemy with the commander
  O                    Pause or resume the selected factories and construction sites,
                       or switch a cloak on and off
  Ctrl + C, then click Capture an enemy unit or structure with the selected builders.
                       Right-clicking an enemy with engineers alone does it too
  Ctrl + T, then click Teleport a commander with a teleporter. Command works too
  Ctrl + K             Self-destruct the selection in 5 seconds, or stop counting.
                       Command works too
  Ctrl + 1 to 0        Save the selection as a group, or clear it if nothing is selected.
                       Command works too
  1 to 0               Select that group, and press again quickly to jump the camera to it
  W E S D A Z C V     With builders: place a structure. Related ones share a key, and
                       pressing it again moves on to the next: W factories, E materiel,
                       S energy, D defenses and anti-air, A artillery and missiles,
                       Z radar and sonar, C shields and missile defense, V support
  N                    Mute or unmute
  - and =              Sound quieter and louder
  Space                Tap: jump to your commander. Hold and move the mouse:
                       turn the camera round the middle of the view; let go
                       and it looks as usual again
  Right-click a transport  With land units selected, board it
  Ctrl + U             Unload the selected transports where you click next
  Ctrl + A             Select all your units that fight, or all your units if none do
  Ctrl + P             Pause or resume the game. Orders given while paused wait for
                       it to resume. Command works too, and so does the Pause button
  Scroll               Zoom, towards the cursor
  Arrow keys           Pan
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
    /// Print the balance harness's measurements of today's unit data (BALANCE.md).
    balance: bool,
    /// Print T1 land's first-pass numbers, derived from the balance targets.
    balance_derive: Option<String>,
    threads: Option<usize>,
    no_tick_gate: bool,
    seed: Option<u64>,
    sound_check: bool,
    map: Option<PathBuf>,
    /// How hard the AI opponent plays: easy unless asked otherwise.
    difficulty: Option<Difficulty>,
    /// What it plays, by its id in data/ai_strategies.toml, or "random": balanced unless asked.
    strategy: Option<String>,
    /// What Blue and Red play in a soak.
    soak_strategies: Option<[&'static Strategy; 2]>,
    /// Players a side: 1, a 1v1, unless asked for a team match on a map with the starts for it.
    per_team: usize,
    /// Watch the soak's AI-vs-AI match for the map and seed instead of playing.
    watch: bool,
    /// Whose sight a watched match starts in: `None` for everything.
    view: Option<Team>,
    /// Play with fog of war turned off, for every team. It is on unless asked otherwise.
    no_fog: bool,
    /// Play a playtest scenario from data/scenarios.toml instead of a match.
    scenario: Option<String>,
}

/// On or off, for --fog and ?fog=.
fn parse_switch(name: &str) -> Option<bool> {
    match name {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

/// A view by name, for --view and ?view=: everything, or one team's sight.
fn parse_view(name: &str) -> Option<Option<Team>> {
    match name {
        "all" => Some(None),
        "blue" => Some(Some(Team::Blue)),
        "red" => Some(Some(Team::Red)),
        _ => None,
    }
}

/// In the browser, options come from the page's address instead: `?map=highland`, `?seed=7`,
/// `?battle` for the stress test, and `?watch` with `&view=blue` to watch an AI match. `map=skirmish` is the generated map, as no map is. An
/// address without options shows the page's map choice (index.html), so the game waits.
#[cfg(target_arch = "wasm32")]
fn parse_options() -> Result<Option<Options>, String> {
    let mut options = Options::default();
    let search = web_sys::window().and_then(|window| window.location().search().ok()).unwrap_or_default();
    if search.trim_start_matches('?').is_empty() {
        return Ok(None);
    }
    let params = web_sys::UrlSearchParams::new_with_str(&search).map_err(|_| "the page address cannot be read".to_string())?;
    options.map = params.get("map").filter(|name| name != "skirmish").map(PathBuf::from);
    options.seed = params.get("seed").and_then(|seed| seed.parse().ok());
    options.battle = params.has("battle");
    if let Some(name) = params.get("ai") {
        (options.difficulty, options.strategy) = parse_opponent(&name).map_or((None, None), |(difficulty, strategy)| (Some(difficulty), strategy));
    }
    options.strategy = params.get("strategy").filter(|name| is_strategy(name)).or(options.strategy);
    options.per_team = params.get("teams").and_then(|name| parse_teams(&name)).unwrap_or(1);
    options.watch = params.has("watch");
    options.view = params.get("view").and_then(|name| parse_view(&name)).flatten();
    options.no_fog = params.get("fog").and_then(|name| parse_switch(&name)) == Some(false);
    options.scenario = params.get("scenario");
    Ok(Some(options))
}

/// In the browser, `?units` shows the unit database instead of a game: the page of every unit's
/// numbers, which the game writes from the blueprints built into it, the ones it plays with, so
/// that what the page says is what the game does. Whether it did.
#[cfg(target_arch = "wasm32")]
fn show_unit_database() -> bool {
    use wasm_bindgen::JsCast;
    let Some(window) = web_sys::window() else {
        return false;
    };
    let search = window.location().search().unwrap_or_default();
    let wanted = web_sys::UrlSearchParams::new_with_str(&search).is_ok_and(|params| params.has("units"));
    let Some(document) = window.document().and_then(|document| document.dyn_into::<web_sys::HtmlDocument>().ok()).filter(|_| wanted) else {
        return false;
    };
    let html = sim_engine::unit_database::page(&BlueprintDb::standard());
    if document.open().is_err() || document.write_1(&html).is_err() {
        log::error!("the unit database could not be written");
    }
    let _ = document.close();
    true
}

/// In the browser, `?scenarios` shows the playtest scenarios page instead of a game, written
/// from the scenarios and blueprints built into the game, the ones it plays. Whether it did.
#[cfg(target_arch = "wasm32")]
fn show_scenario_page() -> bool {
    use wasm_bindgen::JsCast;
    let Some(window) = web_sys::window() else {
        return false;
    };
    let search = window.location().search().unwrap_or_default();
    let wanted = web_sys::UrlSearchParams::new_with_str(&search).is_ok_and(|params| params.has("scenarios"));
    let Some(document) = window.document().and_then(|document| document.dyn_into::<web_sys::HtmlDocument>().ok()).filter(|_| wanted) else {
        return false;
    };
    let html = sim_engine::scenario_page::page(&sim_engine::sim::playtest_scenarios::all(), &BlueprintDb::standard());
    if document.open().is_err() || document.write_1(&html).is_err() {
        log::error!("the playtest scenarios page could not be written");
    }
    let _ = document.close();
    true
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_options() -> Result<Option<Options>, String> {
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
            "--balance" => options.balance = true,
            "--balance-derive" => options.balance_derive = Some(value("--balance-derive")?),
            "--threads" => options.threads = Some(number("--threads", value("--threads")?)? as usize),
            "--no-tick-gate" => options.no_tick_gate = true,
            "--seed" => options.seed = Some(number("--seed", value("--seed")?)?),
            "--sound-check" => options.sound_check = true,
            "--map" => options.map = Some(PathBuf::from(value("--map")?)),
            "--ai" => {
                let (difficulty, strategy) = parse_opponent(&value("--ai")?).ok_or("--ai needs easy or normal, or turtle")?;
                options.difficulty = Some(difficulty);
                options.strategy = strategy.or(options.strategy.take());
            }
            "--teams" => {
                options.per_team = parse_teams(&value("--teams")?).ok_or("--teams needs 1v1, 2v2, 3v3, or 4v4")?;
            }
            "--strategies" => {
                let names = value("--strategies")?;
                let pair: Vec<Option<&'static Strategy>> = names.split(',').map(Strategy::named).collect();
                match pair.as_slice() {
                    [Some(blue), Some(red)] => options.soak_strategies = Some([blue, red]),
                    _ => return Err(format!("--strategies needs two of {}, with a comma between", strategy_names())),
                }
            }
            "--strategy" => {
                let name = value("--strategy")?;
                if !is_strategy(&name) {
                    return Err(format!("--strategy needs one of {}, or random", strategy_names()));
                }
                options.strategy = Some(name);
            }
            "--watch" => options.watch = true,
            "--view" => options.view = parse_view(&value("--view")?).ok_or("--view needs all, blue, or red")?,
            "--fog" => options.no_fog = !parse_switch(&value("--fog")?).ok_or("--fog needs on or off")?,
            "--scenario" => options.scenario = Some(value("--scenario")?),
            "--help" | "-h" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown option {other}\n\n{USAGE}")),
        }
    }
    Ok(Some(options))
}

fn main() {
    #[cfg(target_arch = "wasm32")]
    {
        console_error_panic_hook::set_once();
        let _ = console_log::init_with_level(log::Level::Warn);
        if show_unit_database() || show_scenario_page() {
            return;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    env_logger::init();
    let options = match parse_options() {
        Ok(Some(options)) => options,
        // The page is asking which map to play.
        Ok(None) => return,
        Err(message) => {
            println!("{message}");
            return;
        }
    };
    let seed = options.seed.unwrap_or(DEFAULT_SEED);
    let bundled = options.map.as_ref().and_then(|name| name.to_str()).and_then(sim_engine::sim::maps::bundled);
    let scenario = options.scenario.as_deref().map(|id| {
        sim_engine::sim::playtest_scenarios::find(id).unwrap_or_else(|| {
            let ids: Vec<String> = sim_engine::sim::playtest_scenarios::all().into_iter().map(|scenario| scenario.id).collect();
            println!("unknown scenario {id}: the scenarios are {}", ids.join(", "));
            std::process::exit(1);
        })
    });
    let map_source = match &options.map {
        _ if scenario.is_some() => MapSource::Scenario(Arc::new(scenario.expect("just checked"))),
        None => MapSource::Skirmish,
        Some(name) if name.to_str() == Some("playtest") => MapSource::Playtest,
        Some(_) if bundled.is_some() => {
            let map = bundled.expect("just checked");
            let trees = map.scenery.iter().filter(|(kind, _)| *kind == sim_engine::sim::components::WreckKind::Tree).count();
            println!("map {}: {} km, {} materiel deposits, {} trees and {} rocks", map.name, map.map_units / 1024, map.deposits.len(), trees, map.scenery.len() - trees);
            MapSource::Map(Arc::new(map))
        }
        Some(name) => {
            println!("unknown map {}: the maps are {}, and the playtest sandbox", name.display(), sim_engine::sim::maps::BUNDLED.join(", "));
            std::process::exit(1);
        }
    };
    // A team match, on a map with starts for that many a side (GAME_SPECIFICATION.md §17.2).
    let per_team = options.per_team.max(1);
    let map_source = match map_source.clone().teams(per_team) {
        Some(source) => source,
        None => {
            println!("this map has no starts for {per_team}v{per_team}: Saddleback holds 2v2");
            #[cfg(not(target_arch = "wasm32"))]
            std::process::exit(1);
            #[cfg(target_arch = "wasm32")]
            map_source
        }
    };
    if options.sound_check {
        sound_check();
        return;
    }
    if let Some(tier) = options.balance_derive.as_deref() {
        match tier {
            "t1" => sim_engine::balance::derive_t1_report(),
            "t2" => sim_engine::balance::derive_t2_report(),
            "t3" => sim_engine::balance::derive_t3_report(),
            "t1-air" => sim_engine::balance::derive_t1_air_report(),
            other => println!("--balance-derive takes t1, t2, t3, or t1-air, not {other}"),
        }
        return;
    }
    if options.balance {
        sim_engine::balance::report(&std::sync::Arc::new(sim_engine::sim::blueprints::BlueprintDb::standard()));
        return;
    }
    if let Some(matches) = options.soak {
        let strategies = options.soak_strategies.unwrap_or([Strategy::balanced(); 2]);
        println!("strategies: blue {}, red {}", strategies[0].id, strategies[1].id);
        if map_source.per_team() > 1 {
            println!("teams: {0}v{0}", map_source.per_team());
        }
        let mut report = run_soak(options.seed.unwrap_or(1), matches, options.threads.unwrap_or(6), &map_source, strategies);
        if !report.print(!options.no_tick_gate) {
            std::process::exit(1);
        }
        return;
    }
    if let Some(ticks) = options.headless_ticks {
        run_headless(seed, ticks, options.battle, &map_source, !options.no_fog);
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    pollster::block_on(run_windowed(seed, options, map_source));
    // The browser cannot block while the graphics device starts, so the game starts once it has.
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(run_windowed(seed, options, map_source));
}

/// The match for the seed, or the scripted battle, with fog of war on or off. The battle shows
/// everything either way.
fn make_scenario(seed: u64, battle: bool, map: &MapSource, fog: bool) -> Scenario {
    if battle {
        scenario::default_battle(seed)
    } else {
        map.scenario(seed).with_fog_of_war(fog)
    }
}

fn run_headless(seed: u64, ticks: u32, battle: bool, map: &MapSource, fog: bool) {
    let scenario = make_scenario(seed, battle, map, fog);
    let opening = scenario.opening_commands;
    let mut world = scenario.world;
    let mut brains: Vec<AiBrain> = (0..world.players.len()).map(|player| AiBrain::for_player(&world, sim_engine::sim::components::PlayerId(player as u8), seed, Difficulty::Normal, Strategy::balanced())).collect();
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
        // The checksum says the match played, whether or not a frame was drawn in time.
        if times.is_empty() {
            println!("autotest: no frames recorded");
            println!("autotest: tick {} checksum {:016x}", result.tick, result.checksum);
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
    water_cells: Vec<(u32, u32)>,
    deposits: Vec<(u32, u32)>,
    vents: Vec<(u32, u32)>,
    blueprints: Arc<BlueprintDb>,
    fog_side: Option<usize>,
    /// Below this height, ground is drawn as water.
    water_level: f32,
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
    // Ground too steep or too deep to build on, from maps that have it.
    let cells = world.terrain.cells_per_side() as u32;
    for y in 0..cells {
        for x in 0..cells {
            if !world.terrain.cell_is_open(x, y) {
                blocked_cells.push((y, x));
            }
        }
    }
    blocked_cells.sort_unstable();
    blocked_cells.dedup();
    let water_cells: Vec<(u32, u32)> = (0..cells).flat_map(|y| (0..cells).map(move |x| (y, x))).filter(|&(y, x)| world.terrain.cell_is_navigable(x, y)).collect();
    MapInfo {
        water_level: world.terrain.water_level().to_num::<f32>(),
        map_units: f64::from(world.terrain.map_units()),
        cells_per_side: world.terrain.cells_per_side() as u32,
        wall_cells,
        blocked_cells,
        water_cells,
        deposits: world.deposits.clone(),
        vents: world.vents.clone(),
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
            vents: &self.vents,
            water_cells: &self.water_cells,
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

/// Stops or restarts the game clock, which only the one human player controls for now.
fn toggle_pause(session: &Session) {
    if let Some(driver) = &session.driver {
        driver.set_paused(!driver.is_paused());
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
    println!("Commander under attack");
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
        contacts: &session.current.contacts,
        surfaced: &session.current.surfaced,
    });
    let orders = if queued { queued_commands(orders) } else { orders };
    let orders = if formation { formation_commands(orders) } else { orders };
    let reason = if selection.selected().is_empty() { "No units selected" } else { reason };
    feedback.show_orders(&orders, ground_point(ground, &map.heights), &map.blueprints, reason);
    let ordered = !orders.is_empty();
    for order in orders {
        session.send(order);
    }
    ordered
}

/// Orders the selection to a point picked on the minimap: a move, or with Alt an attack-move.
/// Nothing on the map is under the cursor, so it never attacks, assists, or reclaims.
#[allow(clippy::too_many_arguments)]
fn issue_minimap_order(session: &Session, selection: &Selection, camera: &Camera, map: &MapInfo, feedback: &mut OrderFeedback, point: [f64; 2], attack_move: bool, queued: bool, formation: bool) {
    let ground = Some(point);
    let click = RightClick {
        selected: selection.selected(),
        units: &session.current.units,
        wrecks: &session.current.wrecks,
        blueprints: &map.blueprints,
        team: PLAYER_TEAM,
        camera,
        cursor: [f64::MIN, f64::MIN],
        ground,
        attack_move,
        contacts: &[],
        surfaced: &session.current.surfaced,
    };
    let orders = if attack_move { right_click_orders(&click) } else { move_orders(&click) };
    let orders = if queued { queued_commands(orders) } else { orders };
    let orders = if formation { formation_commands(orders) } else { orders };
    let reason = if selection.selected().is_empty() { "No units selected" } else { "The selected units cannot move" };
    feedback.show_orders(&orders, ground_point(ground, &map.heights), &map.blueprints, reason);
    for order in orders {
        session.send(order);
    }
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
/// Sends the selection on patrol to the point under the cursor: from where each unit is, or with
/// `queued`, from where its queue ends, or, if it is already on patrol, onto the end of its loop.
/// Returns whether there was ground there.
fn patrol_to_cursor(session: &Session, selection: &Selection, camera: &Camera, map: &MapInfo, feedback: &mut OrderFeedback, queued: bool) -> bool {
    let ground = camera.screen_to_ground(selection.cursor(), &map.heights);
    let Some(point) = ground else {
        return false;
    };
    if let Some(position) = ground_point(ground, &map.heights) {
        feedback.add_marker(MarkerKind::Patrol, position, None);
    }
    let mut points = vec![Vec2Fp::new(Fp::from_num(point[0]), Fp::from_num(point[1]))];
    start_patrol(session, selection, map, feedback, &mut points, queued);
    true
}

/// Sends the selection on patrol through the points so far.
/// Sends a patrol through the clicked points, starting where each unit is, or with Shift, where
/// it will be once its queued orders are done, so that it walks back there each time round.
fn start_patrol(session: &Session, selection: &Selection, map: &MapInfo, feedback: &mut OrderFeedback, points: &mut Vec<Vec2Fp>, queued: bool) {
    let units = mobile_units(selection.selected(), &session.current.units, &map.blueprints);
    let points = std::mem::take(points);
    if units.is_empty() {
        feedback.show_rejected(None, "No units selected that can patrol");
        return;
    }
    let orders = sim_engine::input::orders::patrol_commands(&units, &session.current, &points, queued);
    // One line for the whole group, as if it were one order, counting each unit's own start.
    let summary = SimCommand::Patrol { units: units.clone(), points: std::iter::once(Vec2Fp::ZERO).chain(points.iter().copied()).collect(), queued };
    feedback.show_orders(std::slice::from_ref(&summary), None, &map.blueprints, "");
    for order in orders {
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
    // Only the first can replace existing orders, or each would cancel the one before it. The
    // whole selection goes, so that builders that cannot build it help those that can.
    let units = selection.selected().to_vec();
    for (index, &(cell_x, cell_y)) in valid.iter().enumerate() {
        feedback.add_marker(MarkerKind::Build, position_of((cell_x, cell_y)), None);
        session.send(SimCommand::Build { units: units.clone(), blueprint, cell_x, cell_y, queued: queued || index > 0 });
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

async fn run_windowed(mut seed: u64, options: Options, map_source: MapSource) {
    let battle = options.battle;
    // Watching plays the soak's match for the seed, with no player, seen from any side.
    let watching = options.watch && !battle;
    let mut watch_view = options.view;
    let map_name = match &map_source {
        MapSource::Skirmish => "Skirmish".to_string(),
        MapSource::Map(map) => display_name(&map.name),
        MapSource::Teams(map, per_team) => format!("{} ({per_team}v{per_team})", display_name(&map.name)),
        MapSource::Playtest => "Playtest".to_string(),
        MapSource::Scenario(scenario) => scenario.title.clone(),
    };
    // A playtest scenario's materiel on each side at the start, for its score.
    let scenario_start = match &map_source {
        MapSource::Scenario(scenario) => {
            let db = BlueprintDb::standard();
            let worth = |groups: &[sim_engine::sim::playtest_scenarios::Group]| groups.iter().map(|group| db.get(db.id(&group.unit)).mass.to_num::<f64>() * f64::from(group.count)).sum::<f64>();
            Some([worth(&scenario.blue), worth(&scenario.red)])
        }
        _ => None,
    };
    let fog = !options.no_fog;
    let scenario = make_scenario(seed, battle, &map_source, fog);
    // A watcher seeing through one team's eyes starts at its base, as its player would.
    let player_start = scenario.world.starts[if watching { watch_view.unwrap_or(PLAYER_TEAM) } else { PLAYER_TEAM }.index()];
    let map = read_map(&scenario);
    let is_match = !battle;
    let autotest_seconds = options.autotest_seconds;
    let fast_forward = options.fast_forward;
    // The playtest sandbox is played against a turtle unless asked otherwise.
    let playtest = matches!(map_source, MapSource::Playtest);
    let difficulty = options.difficulty.unwrap_or(if playtest { Difficulty::Normal } else { Difficulty::Easy });
    let strategy = match options.strategy.as_deref() {
        Some("random") => Strategy::random(seed, 1),
        Some(name) => Strategy::named(name).expect("checked when read"),
        None if playtest => Strategy::named("turtle").expect("data/ai_strategies.toml has turtle"),
        None => Strategy::balanced(),
    };

    let mut autotest = autotest_seconds.map(|seconds| Autotest::new(seconds, options.capture_dir.clone()));
    let source = match (autotest_seconds, battle) {
        (Some(seconds), true) => CommandSource::Script { seed, ticks: fast_forward + seconds * 10 },
        (Some(seconds), false) => CommandSource::AiVsAi { seed, ticks: fast_forward + seconds * 10 },
        (None, true) => CommandSource::Player,
        (None, false) if watching => watch_source(seed),
        // In a playtest scenario Red only follows its opening orders.
        (None, false) if scenario_start.is_some() => CommandSource::Player,
        (None, false) => CommandSource::PlayerVsAi { seed, difficulty, strategy },
    };
    let viewer = if watching { watch_view } else { is_match.then_some(PLAYER_TEAM) };
    let mut driver_options = DriverOptions { viewer, fast_forward_ticks: fast_forward, start_paused: false, linger: watching && autotest_seconds.is_none() };
    let mut session = Session::start(scenario, source, driver_options);

    let event_loop = EventLoop::new().expect("event loop starts");
    let builder = WindowBuilder::new().with_title("sim-engine");
    #[cfg(not(target_arch = "wasm32"))]
    let builder = builder.with_inner_size(LogicalSize::new(1440.0, 900.0));
    // In the browser, a canvas the page sizes, which keeps the keys and right-clicks it is given.
    #[cfg(target_arch = "wasm32")]
    let builder = {
        use winit::platform::web::WindowBuilderExtWebSys;
        builder.with_append(true).with_prevent_default(true).with_focusable(true)
    };
    let window = Arc::new(builder.build(&event_loop).expect("window opens"));
    let size = window.inner_size();
    let mut renderer = Renderer::new_async(Arc::clone(&window), &map.heights, map.fog_side).await;
    renderer.set_water_level(map.water_level);
    let mut camera = Camera::new(map.map_units, [f64::from(size.width), f64::from(size.height)]);
    if is_match {
        camera.set_min_altitude(SKIRMISH_MIN_ALTITUDE);
        camera.look_at(raw_to_world(player_start.x.to_bits()), raw_to_world(player_start.y.to_bits()), STARTING_ALTITUDE);
    }
    // A watcher who sees everything starts over the middle, high enough for both bases.
    if watching && watch_view.is_none() {
        camera.look_at(map.map_units / 2.0, map.map_units / 2.0, camera.max_altitude() * WATCH_ALTITUDE_FRACTION);
    }
    // Whether the fog has been cleared for seeing everything, since snapshots then carry none.
    let mut fog_cleared = false;
    // Which kinds of range are outlined for all the viewer's units, and those outlines' rings,
    // gathered again only when the snapshot, the toggles, or the view changes.
    let mut range_toggles = ui::RangeToggles::default();
    let mut overlay_cache: ((u32, ui::RangeToggles, Option<Team>), ui::RangeRings) = ((u32::MAX, range_toggles, None), Vec::new());

    let egui_context = egui::Context::default();
    ui::install_fonts(&egui_context);
    ui::install_style(&egui_context);
    sim_engine::hud::install_portraits(&egui_context);
    let mut minimap = sim_engine::minimap::Minimap::default();
    let mut egui_state = egui_winit::State::new(
        egui_context.clone(),
        egui::ViewportId::ROOT,
        &window,
        Some(window.scale_factor() as f32),
        None,
    );

    let mut selection = Selection::default();
    selection.set_armed(sim_engine::input::selection::army_table(&map.blueprints));
    let mut placing: Option<BlueprintId> = None;
    let mut armed: Option<ArmedOrder> = None;
    // Where a Shift-drag row of structures began, while the button is held.
    let mut line_start: Option<(u32, u32)> = None;
    let mut orders_view_strength = 0.0_f32;
    let clock = Instant::now();
    let mut feedback = OrderFeedback::default();
    // One entry for each patrol point right-clicked while P is held: once there is one, the
    // patrol is under way, and the rest join its loop.
    let mut patrol_points: Vec<Vec2Fp> = Vec::new();
    // Whether P is held down.
    let mut patrol_held = false;
    let mut pause_chord = false;
    let mut last_frame = Instant::now();
    let mut frames_seen = 0usize;
    let mut held_keys = [false; 4];
    // Space held: when it went down, whether the mouse has turned the camera since, and where
    // the cursor last was.
    let mut orbit_key: Option<(Instant, bool, [f64; 2])> = None;
    let (mut alt_held, mut shift_held, mut command_held) = (false, false, false);
    let mut hotkey_actions: Vec<UiAction> = Vec::new();
    let mut quick_select = QuickSelectState::default();
    let mut control_groups = ControlGroups::default();
    // An order being dragged, and where the drag started.
    let mut order_drag: Option<(OrderHandle, [f64; 2])> = None;
    // How many cancelled orders the player has been told about.
    let mut cancelled_seen = 0;
    // The selection last frame, so that a new one opens the build bars at its highest level.
    let mut last_selected: Vec<SimId> = Vec::new();
    let mut attack_alarm = AttackAlarm::default();
    let mut sound_settings = SoundSettings::default();
    // Whether the player has clicked or pressed a key yet, which the browser waits for before it
    // plays any sound.
    let mut sound_unlocked = false;
    // Test runs stay silent. The game plays on without sound if there is no device. Browsers
    // only let sound start once the page has been clicked or typed on: the first click or key
    // press resumes it (`sound_unlocked`).
    let sound = (is_match && !watching && autotest_seconds.is_none()).then(SoundEngine::start).and_then(|started| match started {
        Ok(engine) => Some(engine),
        Err(error) => {
            eprintln!("playing without sound: {error}");
            None
        }
    });
    let mut scene_instances = InstanceBatches::default();
    let mut motion = Motion::new();
    // Structures stand level on slopes, on a slab down to the ground.
    motion.set_ground(HeightField::new(map.heights.samples(), map.heights.heights().to_vec(), map.heights.cell_units(), map.heights.units_per_step()));
    let mut effects = Effects::new();
    let mut title_timer = Instant::now();
    let mut title_frames = 0u32;
    let mut autotest_selected = false;
    // The stress test's units have no blueprints, and stay plain cubes.
    let looks = instances::unit_looks(&map.blueprints);

    event_loop.set_control_flow(ControlFlow::Poll);
    let handle_event = move |event: Event<()>, target: &winit::event_loop::EventLoopWindowTarget<()>| match event {
            Event::AboutToWait => window.request_redraw(),
            Event::WindowEvent { event, .. } => {
                let response = egui_state.on_window_event(&window, &event);
                // The browser lets sound start only from a click or key press on the page.
                let gesture = matches!(event, WindowEvent::MouseInput { state: ElementState::Pressed, .. })
                    || matches!(&event, WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed);
                if gesture && !sound_unlocked {
                    sound_unlocked = true;
                    if let Some(engine) = &sound {
                        engine.resume();
                    }
                }
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
                    WindowEvent::CursorMoved { position, .. } => {
                        // With Space held, the mouse turns the camera round the middle of the view.
                        if let Some((_, moved, last)) = &mut orbit_key {
                            let (dx, dy) = (position.x - last[0], position.y - last[1]);
                            if dx != 0.0 || dy != 0.0 {
                                camera.orbit(dx * ORBIT_RADIANS_PER_PIXEL, -dy * ORBIT_DEGREES_PER_PIXEL);
                                *moved = true;
                            }
                            *last = [position.x, position.y];
                        }
                        selection.move_cursor([position.x, position.y]);
                    }
                    // Space let go while the window was elsewhere: back to the usual view.
                    WindowEvent::Focused(false) => {
                        orbit_key = None;
                        camera.end_orbit();
                    }
                    WindowEvent::MouseWheel { delta, .. } if !response.consumed => {
                        let steps = match delta {
                            MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                            MouseScrollDelta::PixelDelta(PhysicalPosition { y, .. }) => y / 40.0,
                        };
                        let anchor = camera.screen_to_ground(selection.cursor(), &map.heights);
                        camera.zoom(steps, anchor);
                    }
                    // A watcher selects nothing and gives no orders.
                    WindowEvent::MouseInput { .. } if watching => {}
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
                            let cells = if unit.requires_deposit || unit.requires_vent { vec![start] } else { line_cells(start, end, footprint) };
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
                                    ArmedOrder::AttackGround => {
                                        if issue_planned_orders(&session, &selection, &camera, &map, &mut feedback, false, false, false, "The selected units have no guns", attack_ground_orders) && !shift_held {
                                            armed = None;
                                        }
                                    }
                                    // A click patrols there at once; with Shift, after what the units are doing,
                                    // and each further Shift-click joins the loop, with patrol still armed.
                                    ArmedOrder::Patrol => {
                                        if patrol_to_cursor(&session, &selection, &camera, &map, &mut feedback, shift_held) && !shift_held {
                                            armed = None;
                                        }
                                    }
                                    ArmedOrder::Discharge => {
                                        if issue_planned_orders(&session, &selection, &camera, &map, &mut feedback, false, false, false, "Click an enemy to Discharge", discharge_orders) {
                                            if !shift_held {
                                                armed = None;
                                            }
                                            // It waits for the energy, so say so.
                                            let needed = map.blueprints.iter().find_map(|unit| unit.discharge).map_or(0, |discharge| discharge.energy_required.to_num::<i64>());
                                            if session.current.economy.energy < needed {
                                                feedback.show_rejected(None, &format!("Discharge is waiting for {needed} energy in store"));
                                            }
                                        }
                                    }
                                    ArmedOrder::Capture => {
                                        if issue_planned_orders(&session, &selection, &camera, &map, &mut feedback, false, shift_held, false, "Click an enemy unit or structure to capture", capture_orders) && !shift_held {
                                            armed = None;
                                        }
                                    }
                                    ArmedOrder::Unload => {
                                        let transports = ui::loaded_transports(&session.current, selection.selected());
                                        let ground = camera.screen_to_ground(selection.cursor(), &map.heights);
                                        match ground {
                                            _ if transports.is_empty() => feedback.show_rejected(None, "No transports selected with anything on board"),
                                            Some(point) => {
                                                armed = None;
                                                let order = SimCommand::Unload { transports, point: Vec2Fp::new(Fp::from_num(point[0]), Fp::from_num(point[1])) };
                                                feedback.show_orders(std::slice::from_ref(&order), ground_point(ground, &map.heights), &map.blueprints, "");
                                                session.send(order);
                                            }
                                            None => feedback.show_rejected(None, "Click the ground to unload"),
                                        }
                                    }
                                    ArmedOrder::Teleport => {
                                        let units = ui::teleporters(&session.current, selection.selected(), &map.blueprints);
                                        let ground = camera.screen_to_ground(selection.cursor(), &map.heights);
                                        match ground {
                                            _ if units.is_empty() => feedback.show_rejected(None, "No units selected that can teleport"),
                                            Some(point) if ui::senses_spot(&session.current, point) => {
                                                armed = None;
                                                let order = SimCommand::Teleport { units, point: Vec2Fp::new(Fp::from_num(point[0]), Fp::from_num(point[1])) };
                                                feedback.show_orders(std::slice::from_ref(&order), ground_point(ground, &map.heights), &map.blueprints, "");
                                                session.send(order);
                                            }
                                            // Stays armed, so that the player can try again.
                                            _ => feedback.show_rejected(None, "Can't teleport there: out of sight and radar"),
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
                            } else if armed.take().is_some() || placing.take().is_some() {
                                // Right-click only cancels what was armed.
                                patrol_points.clear();
                            } else if patrol_held {
                                // Each right-click while P is held is a patrol point: the first starts the
                                // patrol, and the rest join its loop.
                                let queued = shift_held || !patrol_points.is_empty();
                                if patrol_to_cursor(&session, &selection, &camera, &map, &mut feedback, queued) {
                                    patrol_points.push(Vec2Fp::ZERO);
                                }
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
                            PhysicalKey::Code(KeyCode::ArrowUp) => held_keys[0] = pressed,
                            PhysicalKey::Code(KeyCode::ArrowDown) => held_keys[1] = pressed,
                            PhysicalKey::Code(KeyCode::ArrowLeft) => held_keys[2] = pressed,
                            PhysicalKey::Code(KeyCode::ArrowRight) => held_keys[3] = pressed,
                            // Held, the mouse orbits the camera round the middle of the view, and
                            // letting go puts it back as it usually looks. A quick tap that does not
                            // move the mouse jumps to the commander, or pauses a watched match.
                            PhysicalKey::Code(KeyCode::Space) => {
                                if pressed {
                                    if orbit_key.is_none() {
                                        orbit_key = Some((Instant::now(), false, selection.cursor()));
                                    }
                                } else if let Some((since, moved, _)) = orbit_key.take() {
                                    camera.end_orbit();
                                    if !moved && since.elapsed() < SPACE_TAP {
                                        if watching {
                                            hotkey_actions.push(UiAction::TogglePause);
                                        } else if is_match {
                                            if let Some(commander) = commander_of(&session.current, &map.blueprints) {
                                                let index = session.current.units.binary_search_by_key(&commander, |unit| unit.id).expect("found in this list");
                                                let unit = &session.current.units[index];
                                                camera.look_at(raw_to_world(unit.x), raw_to_world(unit.y), 0.0);
                                                selection.select(vec![commander]);
                                            }
                                        }
                                    }
                                }
                            }
                            // A watcher's keys: pause, whose sight, and speed. No other key
                            // gives orders.
                            PhysicalKey::Code(code) if watching => {
                                if pressed && !event.repeat {
                                    let speed = session.driver.as_ref().map_or(1, SimDriver::speed);
                                    match code {
                                        KeyCode::KeyV => hotkey_actions.push(UiAction::SetView(ui::next_watch_view(watch_view))),
                                        KeyCode::Minus => hotkey_actions.push(UiAction::SetSpeed(ui::step_watch_speed(speed, false))),
                                        KeyCode::Equal => hotkey_actions.push(UiAction::SetSpeed(ui::step_watch_speed(speed, true))),
                                        _ => {}
                                    }
                                }
                            }
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
                            // Ctrl (or Cmd) + P stops the clock. Its release is not a patrol tap.
                            PhysicalKey::Code(KeyCode::KeyP) if pressed && command_held && is_match => {
                                if !event.repeat {
                                    pause_chord = true;
                                    toggle_pause(&session);
                                }
                            }
                            PhysicalKey::Code(KeyCode::KeyP) if !pressed && pause_chord => pause_chord = false,
                            PhysicalKey::Code(KeyCode::KeyP) => {
                                if pressed {
                                    patrol_held = true;
                                } else {
                                    patrol_held = false;
                                    if !patrol_points.is_empty() {
                                        // Points were placed while it was held: the patrol is already under way.
                                        patrol_points.clear();
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
                                        // Pressing the key again straight away jumps the camera to the group.
                                        if control_groups.recalled_twice(group, clock.elapsed().as_secs_f64()) {
                                            if let Some([x, y]) = group_center(&members, &session.current.units) {
                                                camera.look_at(x, y, 0.0);
                                            }
                                        }
                                        selection.select(members);
                                        armed = None;
                                        placing = None;
                                        line_start = None;
                                        patrol_points.clear();
                                    }
                                }
                            }
                            // Ctrl+A selects the whole army: every unit that fights, or every unit if none does.
                            PhysicalKey::Code(KeyCode::KeyA) if pressed && !event.repeat && command_held && is_match && !watching => {
                                selection.select_all(&session.current.units, PLAYER_TEAM);
                                armed = None;
                                placing = None;
                                line_start = None;
                                patrol_points.clear();
                            }
                            // Ctrl+C arms a capture.
                            PhysicalKey::Code(KeyCode::KeyC) if pressed && !event.repeat && command_held && is_match => {
                                if ui::has_mobile_builders(&session.current, selection.selected(), &map.blueprints) {
                                    armed = Some(ArmedOrder::Capture);
                                    placing = None;
                                    line_start = None;
                                    patrol_points.clear();
                                } else {
                                    feedback.show_rejected(None, "No builders selected");
                                }
                            }
                            // Ctrl+T arms a teleport, and Ctrl+K starts or stops a self-destruct.
                            PhysicalKey::Code(KeyCode::KeyU) if pressed && !event.repeat && command_held && is_match => {
                                if ui::loaded_transports(&session.current, selection.selected()).is_empty() {
                                    feedback.show_rejected(None, "No transports selected with anything on board");
                                } else {
                                    armed = Some(ArmedOrder::Unload);
                                    placing = None;
                                    line_start = None;
                                    patrol_points.clear();
                                }
                            }
                            PhysicalKey::Code(KeyCode::KeyT) if pressed && !event.repeat && command_held && is_match => {
                                if ui::teleporters(&session.current, selection.selected(), &map.blueprints).is_empty() {
                                    feedback.show_rejected(None, "No units selected that can teleport");
                                } else {
                                    armed = Some(ArmedOrder::Teleport);
                                    placing = None;
                                    line_start = None;
                                    patrol_points.clear();
                                }
                            }
                            PhysicalKey::Code(KeyCode::KeyK) if pressed && !event.repeat && command_held && is_match => {
                                let own: Vec<SimId> = selection
                                    .selected()
                                    .iter()
                                    .copied()
                                    .filter(|id| session.current.units.binary_search_by_key(id, |unit| unit.id).is_ok_and(|index| session.current.units[index].team == PLAYER_TEAM))
                                    .collect();
                                if own.is_empty() {
                                    feedback.show_rejected(None, "No units selected");
                                } else {
                                    let order = SimCommand::SelfDestruct { units: own };
                                    feedback.show_orders(std::slice::from_ref(&order), None, &map.blueprints, "");
                                    session.send(order);
                                }
                            }
                            // N builds what is in its slot of the build bar shown, and mutes otherwise.
                            PhysicalKey::Code(KeyCode::KeyN) if pressed && !event.repeat && !command_held => {
                                match ui::hotkey_action(&session.current, selection.selected(), &map.blueprints, 'N', ui::build_tiers(&egui_context), placing) {
                                    Some(action) => hotkey_actions.push(ui::with_shift(action, shift_held, &map.blueprints)),
                                    None => sound_settings.toggle_mute(),
                                }
                            }
                            // - and = build what is in their slots of the bar shown, and set the volume otherwise.
                            PhysicalKey::Code(code @ (KeyCode::Minus | KeyCode::Equal)) if pressed && !command_held => {
                                let letter = hotkeys::letter_for(code).expect("both are panel keys");
                                match ui::hotkey_action(&session.current, selection.selected(), &map.blueprints, letter, ui::build_tiers(&egui_context), placing) {
                                    Some(action) => hotkey_actions.push(ui::with_shift(action, shift_held, &map.blueprints)),
                                    None if code == KeyCode::Minus => sound_settings.quieter(),
                                    None => sound_settings.louder(),
                                }
                            }
                            // ` shows the next tech level in the build bars.
                            PhysicalKey::Code(KeyCode::Backquote) if pressed && !event.repeat && !command_held => {
                                ui::cycle_build_tiers(&egui_context, &session.current, selection.selected(), &map.blueprints);
                            }
                            // Command and Control belong to the system: Command-Q must not start a factory.
                            PhysicalKey::Code(code) if pressed && !event.repeat && !command_held => {
                                if let Some(letter) = hotkeys::letter_for(code) {
                                    match ui::hotkey_action(&session.current, selection.selected(), &map.blueprints, letter, ui::build_tiers(&egui_context), placing) {
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
                                match (&session.current.fog, map.fog_side) {
                                    (Some(fog), _) => {
                                        renderer.update_fog(fog);
                                        fog_cleared = false;
                                    }
                                    // Seeing everything: no fog anywhere.
                                    (None, Some(side)) if !fog_cleared => {
                                        renderer.update_fog(&vec![u8::MAX; side * side]);
                                        fog_cleared = true;
                                    }
                                    (None, _) => {}
                                }
                                let units = &session.current.units;
                                motion.retain(|id| units.binary_search_by_key(id, |unit| unit.id).is_ok());
                            }
                        }
                        selection.retain_alive(&session.current.units);
                        // A watched match sped up has ticks closer together.
                        let speed = session.driver.as_ref().map_or(1, SimDriver::speed);
                        let alpha = ((now - session.current_arrived).as_secs_f64() * f64::from(speed) / TICK_DURATION.as_secs_f64()).clamp(0.0, 1.0);

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
                                    // And a shield dome over the commander, at two-thirds strength.
                                    let radius = map.blueprints.get(map.blueprints.id("shield_generator")).shield.map_or(0, |shield| shield.radius.to_bits());
                                    let ground = z - snapshot.units[index].radius;
                                    snapshot.shields.push(ShieldView { id: commander, team: PLAYER_TEAM, center: [x, y, ground], radius, health_permille: 660, up: true, recharging: 0 });
                                    // Two overlapping radars and a deep scan array, for the coverage outline far out, and
                                    // a few blips beyond them.
                                    let unit = |value: i32| value << 12;
                                    snapshot.sensor_ranges = vec![
                                        SensorRangeView { center: [x, y], radar: unit(1_600), deep_scan: unit(400) },
                                        SensorRangeView { center: [x + unit(1_800), y + unit(600)], radar: unit(1_600), deep_scan: 0 },
                                        SensorRangeView { center: [x + unit(900), y - unit(900)], radar: unit(2_400), deep_scan: unit(800) },
                                    ];
                                    snapshot.blips = (0..6).map(|index| [x + unit(2_600 + 120 * index), y + unit(300 * (index % 3)), ground]).collect();
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
                        camera.set_pixels_per_point(f64::from(pixels_per_point));
                        let snapshot = demo_snapshot.unwrap_or_else(|| Arc::clone(&session.current));
                        let selected = selection.selected().to_vec();
                        let mut trails = if is_match { ui::build_trails(&session.previous, &snapshot, alpha, &selected) } else { Vec::new() };
                        ui::estimate_arrivals(&mut trails, &snapshot, &map.blueprints);
                        orders_view_strength = if shift_held {
                            1.0
                        } else {
                            (f64::from(orders_view_strength) - dt / ORDERS_FADE_SECONDS).max(0.0) as f32
                        };
                        if is_match && orders_view_strength > 0.0 {
                            trails.extend(ui::other_units_trails(&session.previous, &snapshot, alpha, &selected, orders_view_strength));
                        }
                        let beams = if is_match { links::work_beams(&session.previous, &snapshot, alpha) } else { Vec::new() };
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
                        // A geothermal plant marks the free vents the same way.
                        let extractor = placing.filter(|blueprint| {
                            let unit = map.blueprints.get(*blueprint);
                            unit.requires_deposit || unit.requires_vent
                        });
                        let site_footprint = extractor.map_or(1, |blueprint| map.blueprints.get(blueprint).footprint);
                        let open_sites: Vec<(u32, u32)> =
                            extractor.map_or_else(Vec::new, |blueprint| free_deposits(&map.placement_map(), blueprint, &snapshot.units, &pending));
                        let open_deposits: Vec<[f64; 3]> = open_sites.iter().map(|&cell| site_position(cell, site_footprint, &map.heights)).collect();
                        let snapped_deposit = placement_spot.filter(|cell| open_sites.contains(cell)).map(|cell| site_position(cell, site_footprint, &map.heights));
                        let group_rows = control_groups.in_use(&snapshot.units);
                        let game_paused = session.driver.as_ref().is_some_and(SimDriver::is_paused);
                        if selection.selected() != last_selected.as_slice() {
                            ui::reset_build_tiers(&egui_context);
                            last_selected = selection.selected().to_vec();
                        }
                        let output = egui_context.run(raw_input, |ctx| {
                            ui::release_keyboard_focus(ctx);
                            ui::draw_map_frame(ctx, &camera, &map.heights, pixels_per_point);
                            ui::draw_deposit_markers(ctx, &camera, &map.heights, &map.deposits, &map.vents, pixels_per_point);
                            ui::placement_grid(ctx, &camera, &grid_lines, f64::from(TERRAIN_CELL_UNITS), pixels_per_point);
                            ui::deposit_highlights(ctx, &camera, &open_deposits, snapped_deposit, pixels_per_point);
                            ui::pending_build_outlines(ctx, &camera, &pending, pixels_per_point);
                            ui::distant_selection_markers(ctx, &camera, &snapshot, &selected, pixels_per_point);
                            ui::draw_interceptions(ctx, &camera, &snapshot.interceptions, pixels_per_point);
                            ui::draw_teleports(ctx, &camera, &map.heights, &snapshot.teleports, pixels_per_point);
                            ui::draw_exotic(ctx, &camera, &map.heights, &snapshot, pixels_per_point);
                            // The kinds of range toggled on, for all the viewer's units.
                            let overlay_team = if watching { watch_view } else { viewer };
                            if overlay_cache.0 != (snapshot.tick, range_toggles, overlay_team) {
                                overlay_cache = ((snapshot.tick, range_toggles, overlay_team), ui::overlay_ranges(&snapshot, overlay_team, &range_toggles, &map.blueprints));
                            }
                            ui::draw_range_outlines(ctx, &camera, &map.heights, &overlay_cache.1, ui::RingStyle::Faint, pixels_per_point);
                            // What the structure being placed will reach, around where it would go.
                            if let (Some(blueprint), Some(cell)) = (placing, placement_spot) {
                                let unit = map.blueprints.get(blueprint);
                                let ranges = ui::placement_ranges(unit);
                                if !ranges.is_empty() {
                                    let center = site_position(cell, unit.footprint, &map.heights);
                                    ui::draw_placement_ranges(ctx, &camera, &map.heights, [center[0], center[1]], &ranges, pixels_per_point);
                                }
                            }
                            // What the selection reaches, while nothing is being placed.
                            if placing.is_none() {
                                let ranges = ui::selection_ranges(&snapshot, &selected, &map.blueprints);
                                ui::draw_range_outlines(ctx, &camera, &map.heights, &ranges, ui::RingStyle::Labelled, pixels_per_point);
                            }
                            ui::draw_blips(ctx, &camera, &snapshot.blips, pixels_per_point);
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
                            // Over an enemy, in sight or on radar, the pointer says what a right-click
                            // does: an attack, if any selected unit can hit it, or not, if none can.
                            if armed.is_none() && placing.is_none() && order_drag.is_none() && hovered_handle.is_none() && !selected.is_empty() && !watching && !ctx.is_pointer_over_area() {
                                let click = RightClick {
                                    selected: &selected,
                                    units: &snapshot.units,
                                    wrecks: &snapshot.wrecks,
                                    blueprints: &map.blueprints,
                                    team: PLAYER_TEAM,
                                    camera: &camera,
                                    cursor: selection.cursor(),
                                    ground: None,
                                    attack_move: false,
                                    contacts: &snapshot.contacts,
                                    surfaced: &snapshot.surfaced,
                                };
                                if let Some(hovered) = sim_engine::input::orders::hovered_enemy(&click) {
                                    let kind = match hovered {
                                        sim_engine::input::orders::Hovered::Unit(_, kind) | sim_engine::input::orders::Hovered::Contact(_, _, kind) => kind,
                                    };
                                    let icon = if sim_engine::input::orders::selection_reaches(&click, kind) { egui::CursorIcon::Crosshair } else { egui::CursorIcon::NotAllowed };
                                    ctx.set_cursor_icon(icon);
                                }
                            }
                            if watching {
                                ui::world_overlay(ctx, &camera, &snapshot, &selected, &map.blueprints, pixels_per_point);
                                let speed = session.driver.as_ref().map_or(1, SimDriver::speed);
                                let state = ui::WatchState { map: &map_name, seed, view: watch_view, speed, paused: game_paused };
                                ui::watch_bar(ctx, &snapshot, &map.blueprints, &state, &mut range_toggles, &mut actions);
                                minimap.show(ctx, &camera, &map.heights, f64::from(map.water_level), &snapshot, map.fog_side, &mut actions);
                                let stopped = snapshot.tick >= sim_engine::soak::MATCH_TICK_LIMIT;
                                ui::watch_end_window(ctx, snapshot.match_result, stopped, snapshot.tick, seed, &mut actions);
                            } else if is_match {
                                ui::world_overlay(ctx, &camera, &snapshot, &selected, &map.blueprints, pixels_per_point);
                                let sound_label = sound.as_ref().map(|_| sound_settings.label());
                                ui::resource_bar(ctx, &snapshot, &map.blueprints, &map_name, sound_label.as_deref(), game_paused, &mut range_toggles, &mut actions);
                                minimap.show(ctx, &camera, &map.heights, f64::from(map.water_level), &snapshot, map.fog_side, &mut actions);
                                ui::commander_alert(ctx, attack_alarm.alert_strength());
                                if let Some(start) = scenario_start {
                                    ui::scenario_score(ctx, &snapshot, &map.blueprints, start, fog);
                                }
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
                        for action in actions.into_iter().flat_map(UiAction::flatten) {
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
                                UiAction::SetSurfaced { units, surfaced } => {
                                    let order = SimCommand::SetSurfaced { units, surfaced };
                                    feedback.show_orders(std::slice::from_ref(&order), None, &map.blueprints, "");
                                    session.send(order);
                                }
                                UiAction::SetCloak { units, on } => {
                                    let word = if on { "Cloaked" } else { "Uncloaked" };
                                    feedback.set_label(MarkerKind::Support, format!("{word}: {}", units.len()));
                                    session.send(SimCommand::SetCloak { units, on });
                                }
                                UiAction::Enhance { units, enhancement } => {
                                    let order = SimCommand::Enhance { units, enhancement };
                                    feedback.show_orders(std::slice::from_ref(&order), None, &map.blueprints, "");
                                    session.send(order);
                                }
                                UiAction::Upgrade { units } => {
                                    feedback.set_label(MarkerKind::Build, format!("Upgrading: {}", units.len()));
                                    session.send(SimCommand::Upgrade { units });
                                }
                                UiAction::TogglePause => toggle_pause(&session),
                                UiAction::CenterCamera([x, y]) => camera.look_at(f64::from(x), f64::from(y), 0.0),
                                UiAction::MinimapOrder([x, y]) if is_match => {
                                    issue_minimap_order(&session, &selection, &camera, &map, &mut feedback, [f64::from(x), f64::from(y)], alt_held, shift_held, command_held);
                                }
                                UiAction::MinimapOrder(_) => {}
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
                                UiAction::Batch(_) => unreachable!("batches were flattened above"),
                                UiAction::SetView(view) => {
                                    watch_view = view;
                                    driver_options.viewer = view;
                                    if let Some(driver) = &session.driver {
                                        driver.set_viewer(view);
                                    }
                                }
                                UiAction::SetSpeed(speed) => {
                                    if let Some(driver) = &session.driver {
                                        driver.set_speed(speed);
                                    }
                                }
                                UiAction::WatchSeed(next) => {
                                    seed = next;
                                    let speed = session.driver.as_ref().map_or(1, SimDriver::speed);
                                    session = Session::start(map_source.scenario(seed).with_fog_of_war(fog), watch_source(seed), driver_options);
                                    if let Some(driver) = &session.driver {
                                        driver.set_speed(speed);
                                    }
                                }
                                UiAction::Restart => {
                                    attack_alarm = AttackAlarm::default();
                                    control_groups = ControlGroups::default();
                                    order_drag = None;
                                    let source = if watching { watch_source(seed) } else { CommandSource::PlayerVsAi { seed, difficulty, strategy } };
                                    session = Session::start(map_source.scenario(seed).with_fog_of_war(fog), source, driver_options);
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
                        motion.time = (f64::from(previous.tick) + alpha) * TICK_DURATION.as_secs_f64();
                        motion.note_shots(&previous.projectiles, &current.projectiles);
                        motion.note_beams(&current.landed, &current.units, &looks);
                        instances::build_unit_instances(&previous.units, &current.units, alpha, eye, selection.selected(), &looks, &current.work_links, &mut motion, &mut scene_instances);
                        let drawn_units_and_shells = {
                            let units = scene_instances.total();
                            instances::build_projectile_instances(&previous.projectiles, &current.projectiles, &current.landed, alpha, eye, &mut scene_instances);
                            units + current.projectiles.len()
                        };
                        // Explosions, missile trails, and wakes, made from the snapshots alone.
                        effects.note_events(previous, current, motion.time, &|x, y| map.heights.height_at(x, y), (map.water_level > -1.0e5).then_some(f64::from(map.water_level)), &looks);
                        effects.trail(previous, current, alpha, &looks, Some(f64::from(map.water_level)), motion.time);
                        effects.build(motion.time, eye, &mut scene_instances);
                        // Builders' beams and adjacency wiring are in the world, and only up close:
                        // zoomed out to icons, they would be clutter.
                        if camera.altitude() < ICON_ALTITUDE {
                            links::build_work_beam_instances(&beams, beam_time, eye, &mut scene_instances);
                            links::build_wiring_instances(&snapshot.adjacency, &snapshot.units, beam_time, eye, &mut scene_instances);
                        }
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
                        // A vent is wider, as wide as the plant that goes on it.
                        for &(x, y) in &map.vents {
                            let (center_x, center_y) = ((f64::from(x) + 0.5) * cell, (f64::from(y) + 0.5) * cell);
                            let center = [center_x, center_y, map.heights.height_at(center_x, center_y) + 0.5];
                            scene_instances.push(Shape::Cube, instances::vent_instance(center, cell * 2.6, eye));
                        }
                        for wreck in &current.wrecks {
                            let center = [raw_to_world(wreck.x), raw_to_world(wreck.y), raw_to_world(wreck.z)];
                            if wreck.kind.is_scenery() {
                                let (shape, instance) = instances::scenery_instance(wreck.kind, center, raw_to_world(wreck.radius), eye);
                                scene_instances.push(shape, instance);
                            } else if let Some((blueprint, facing)) = effects.wreck_origin(wreck.id).filter(|(blueprint, _)| usize::from(*blueprint) < looks.len()) {
                                instances::wreck_model_instances(center, raw_to_world(wreck.radius), &looks[usize::from(blueprint)], facing, eye, &mut scene_instances);
                            } else {
                                scene_instances.push(Shape::Wreck, instances::wreck_instance(center, raw_to_world(wreck.radius), eye));
                            }
                        }
                        for shield in snapshot.shields.iter().filter(|shield| shield.up) {
                            scene_instances.push(Shape::Dome, instances::dome_instance(shield, eye));
                        }
                        if let Some(blueprint) = placing {
                            if let Some(spot) = placement_spot {
                                let unit = map.blueprints.get(blueprint);
                                // While Shift-dragging, preview the whole row. Extractors stay where they snapped.
                                let spots = match line_start {
                                    Some(start) if unit.requires_deposit || unit.requires_vent => vec![start],
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
                            time: motion.time as f32,
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
    };
    #[cfg(not(target_arch = "wasm32"))]
    event_loop.run(handle_event).expect("event loop runs");
    // The browser's event loop runs on after this returns, driven by the page.
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn(handle_event);
    }
}
