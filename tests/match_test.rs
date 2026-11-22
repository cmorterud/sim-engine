//! A player's session against the AI, end to end through the threaded driver. This is the
//! path the window uses, covering everything except the operating system's input events.

use std::sync::Arc;

use sim_engine::ai::Difficulty;
use sim_engine::driver::{CommandSource, DriverOptions, SimDriver};
use sim_engine::input::orders::{placement_cell, right_click_orders, RightClick};
use sim_engine::render::camera::Camera;
use sim_engine::sim::blueprints::{BlueprintDb, BlueprintId};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{SimId, Team};
use sim_engine::sim::scenario::{self, skirmish_start, SKIRMISH_MAP_UNITS};
use sim_engine::sim::world::{MatchResult, SimSnapshot};

const SEED: u64 = 77;
const VIEW: Option<Team> = Some(Team::Blue);

fn start_session() -> SimDriver {
    let options = DriverOptions { viewer: VIEW, ..DriverOptions::default() };
    SimDriver::start_with(scenario::skirmish(SEED), CommandSource::PlayerVsAi { seed: SEED, difficulty: Difficulty::Normal, strategy: sim_engine::ai_strategy::Strategy::balanced() }, false, options)
}

/// Waits until a condition holds on a new snapshot, or the tick limit passes.
fn run_until(driver: &SimDriver, limit: u32, mut done: impl FnMut(&SimSnapshot) -> bool) -> Arc<SimSnapshot> {
    loop {
        match driver.try_snapshot() {
            Some(snapshot) => {
                if done(&snapshot) || snapshot.tick >= limit {
                    return snapshot;
                }
            }
            None => std::thread::yield_now(),
        }
    }
}

fn own_commander(snapshot: &SimSnapshot, db: &BlueprintDb) -> SimId {
    let rig = db.id("command_rig").0;
    snapshot.units.iter().find(|unit| unit.team == Team::Blue && unit.blueprint == rig).expect("commander is alive").id
}

#[test]
fn a_player_who_does_nothing_loses_to_the_ai() {
    let driver = start_session();
    let last = run_until(&driver, 30_000, |snapshot| snapshot.match_result.is_some());
    assert_eq!(last.match_result, Some(MatchResult::Winner(Team::Red)), "ended on tick {}", last.tick);
    driver.finish();
}

#[test]
fn a_player_can_build_an_economy_and_army_through_commands() {
    let db = BlueprintDb::standard();
    let world_view = scenario::skirmish(SEED).world;
    let deposit = world_view.deposits.iter().copied().min_by_key(|&(x, y)| x.abs_diff(80) + y.abs_diff(80)).unwrap();
    drop(world_view);

    let driver = start_session();
    let first = run_until(&driver, 1, |_| true);
    assert!(first.fog.is_some(), "a skirmish has fog of war");
    let commander = own_commander(&first, &db);

    // Choose a factory, and place it next to the commander at a ground point.
    let (factory_blueprint, extractor, bot) = (db.id("land_factory_t1"), db.id("mass_extractor"), db.id("t1_light_walker"));
    let start = skirmish_start(Team::Blue);
    let ground = [start.x.to_num::<f64>() + 150.0, start.y.to_num::<f64>()];
    let (cell_x, cell_y) = placement_cell(ground, db.get(factory_blueprint).footprint);
    driver.send(SimCommand::Build { units: vec![commander], blueprint: factory_blueprint, cell_x, cell_y, queued: false });
    driver.send(SimCommand::Build { units: vec![commander], blueprint: extractor, cell_x: deposit.0, cell_y: deposit.1, queued: true });

    let is_blue = |snapshot: &SimSnapshot, blueprint: BlueprintId, finished: bool| {
        snapshot
            .units
            .iter()
            .find(|unit| unit.team == Team::Blue && unit.blueprint == blueprint.0 && (!finished || unit.built_permille == 1000))
            .map(|unit| unit.id)
    };
    let snapshot = run_until(&driver, 1_500, |snapshot| is_blue(snapshot, factory_blueprint, true).is_some());
    let factory = is_blue(&snapshot, factory_blueprint, true).expect("the factory should be finished");

    driver.send(SimCommand::QueueUnit { factory, blueprint: bot, count: 3 });
    let snapshot = run_until(&driver, 2_000, |snapshot| {
        snapshot.units.iter().filter(|unit| unit.team == Team::Blue && unit.blueprint == bot.0).count() >= 3
    });
    assert!(snapshot.units.iter().filter(|unit| unit.team == Team::Blue && unit.blueprint == bot.0).count() >= 3, "the bots should come out");
    assert!(is_blue(&snapshot, extractor, false).is_some(), "the queued extractor should have been started");
    assert!(snapshot.economy.mass_income >= 1, "the resource bar should have numbers to show");
    assert!(snapshot.factories.iter().any(|view| view.id == factory), "the build menu should have the factory's queue");

    // Order the bots to the ground the way a right-click does, and watch them leave.
    let bots: Vec<SimId> = snapshot.units.iter().filter(|unit| unit.team == Team::Blue && unit.blueprint == bot.0).map(|unit| unit.id).collect();
    let mut camera = Camera::new(f64::from(SKIRMISH_MAP_UNITS), [1600.0, 1000.0]);
    camera.look_at(start.x.to_num::<f64>(), start.y.to_num::<f64>(), 900.0);
    let target = [start.x.to_num::<f64>() - 300.0, start.y.to_num::<f64>() + 400.0];
    let cursor = camera.world_to_screen([target[0], target[1], 0.0]).unwrap();
    let orders = right_click_orders(&RightClick {
        selected: &bots,
        units: &snapshot.units,
        wrecks: &snapshot.wrecks,
        blueprints: &db,
        team: Team::Blue,
        camera: &camera,
        cursor,
        ground: Some(target),
        attack_move: false,
        contacts: &snapshot.contacts,
        surfaced: &snapshot.surfaced,
    });
    assert_eq!(orders.len(), 1);
    assert!(matches!(orders[0], SimCommand::Move { .. }));
    driver.send(orders.into_iter().next().unwrap());
    let raw = |value: f64| (value * 4096.0) as i64;
    let arrived = run_until(&driver, 3_000, |snapshot| {
        snapshot.units.iter().filter(|unit| bots.contains(&unit.id)).all(|unit| {
            (i64::from(unit.x) - raw(target[0])).abs() < raw(40.0) && (i64::from(unit.y) - raw(target[1])).abs() < raw(40.0)
        })
    });
    assert!(arrived.tick < 3_000, "the bots should have reached the point");
    driver.finish();
}

#[test]
fn the_players_view_never_shows_enemies_outside_of_sight() {
    let db = BlueprintDb::standard();
    let driver = start_session();
    let snapshot = run_until(&driver, 2_000, |snapshot| snapshot.tick >= 2_000);
    // At 200 s, Red's base is far outside of Blue's sight, and no fighting has reached Blue yet.
    let enemy_rig = db.id("command_rig").0;
    assert!(!snapshot.units.iter().any(|unit| unit.team == Team::Red && unit.blueprint == enemy_rig), "Red's commander should be hidden");
    driver.finish();
}
