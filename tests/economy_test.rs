//! The vertical slice's rules: flow economy, construction, factories, reclaim, adjacency,
//! fog of war, and assassination (GAME_SPECIFICATION.md §2-9, §16).

use sim_engine::input::orders::{pending_builds, placement_is_valid, PlacementMap};
use sim_engine::math::fp::{fp_int, Fp, Vec2Fp, Vec3Fp};
use sim_engine::sim::blueprints::{BlueprintId, Res};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Blueprinted, BuildTask, Builder, Combat, Construction, Factory, Health, LeavingFactory, Order, OrderKind, Projectile, SimId, Team};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::scenario::{self, skirmish_start};
use sim_engine::sim::world::{MatchResult, PlanStepKind, SimWorld};

const CELL: i32 = 20;

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

/// A flat, open world with a commander far from anything, plenty of storage, and no fog.
fn test_world() -> SimWorld {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, ..SimConfig::default() };
    SimWorld::new(config, 1)
}

fn id(world: &SimWorld, key: &str) -> BlueprintId {
    world.blueprints.id(key)
}

fn set_resources(world: &mut SimWorld, team: Team, mass: i64, energy: i64) {
    let economy = &mut world.economies[team.index()];
    economy.base_mass_capacity = economy.base_mass_capacity.max(Res::from_num(mass));
    economy.base_energy_capacity = economy.base_energy_capacity.max(Res::from_num(energy));
    economy.mass_capacity = economy.mass_capacity.max(Res::from_num(mass));
    economy.energy_capacity = economy.energy_capacity.max(Res::from_num(energy));
    economy.mass = Res::from_num(mass);
    economy.energy = Res::from_num(energy);
}

fn run(world: &mut SimWorld, ticks: u32) {
    for _ in 0..ticks {
        world.advance_tick(&[]);
    }
}

fn entity(world: &SimWorld, unit: SimId) -> hecs::Entity {
    world.unit_entity(unit).expect("unit is alive")
}

fn is_finished(world: &SimWorld, unit: SimId) -> bool {
    world.ecs.get::<&Construction>(entity(world, unit)).is_err()
}

fn progress(world: &SimWorld, unit: SimId) -> f64 {
    world.ecs.get::<&Construction>(entity(world, unit)).map_or(1.0, |site| site.progress.to_num())
}

#[test]
fn income_flows_into_storage_up_to_capacity() {
    let mut world = test_world();
    let generator = id(&world, "power_generator");
    let storage = id(&world, "mass_storage");
    let extractor = id(&world, "mass_extractor");
    world.deposits.push((50, 50));
    world.spawn_finished_structure(extractor, Team::Blue, 50, 50).unwrap();
    world.spawn_finished_structure(generator, Team::Blue, 60, 60).unwrap();
    world.spawn_finished_structure(storage, Team::Blue, 70, 70).unwrap();
    run(&mut world, 100);
    let economy = world.economies[0];
    // 2 mass/s for 10 s, minus the single tick where income is first calculated.
    assert!((19.0..=20.5).contains(&economy.mass.to_num::<f64>()), "materiel was {}", economy.mass);
    // 20 energy/s has nowhere to go, because no structure stores energy.
    assert_eq!(economy.energy, Res::ZERO);
    run(&mut world, 3_000);
    assert_eq!(world.economies[0].mass, Res::from_num(500), "materiel must stop at capacity");
}

#[test]
fn extractors_can_only_go_on_deposits() {
    let mut world = test_world();
    let extractor = id(&world, "mass_extractor");
    let generator = id(&world, "power_generator");
    world.deposits.push((50, 50));
    assert!(!world.can_place(extractor, 51, 50));
    assert!(world.can_place(extractor, 50, 50));
    assert!(!world.can_place(generator, 50, 50), "other structures must not cover a deposit");
}

/// Sets up one engineer already next to a site, with resources under the test's control.
fn engineer_at_site(world: &mut SimWorld) -> (SimId, SimId) {
    let engineer_blueprint = id(world, "t1_engineer");
    let generator = id(world, "power_generator");
    let engineer = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_000, 1_000));
    let (cell_x, cell_y) = (1_040 / CELL as u32, 1_000 / CELL as u32);
    world.advance_tick(&[SimCommand::Build { units: vec![engineer], blueprint: generator, cell_x, cell_y, queued: false }]);
    world.advance_tick(&[]);
    let site = world.occupancy[&(cell_y * world.terrain.cells_per_side() as u32 + cell_x)];
    (engineer, site)
}

#[test]
fn construction_takes_build_time_divided_by_build_power() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 10_000, 100_000);
    let (_, site) = engineer_at_site(&mut world);
    // 125 build time at 5 power is 25 s, or 250 ticks.
    let mut ticks = 0;
    while !is_finished(&world, site) {
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 400, "still at {}", progress(&world, site));
    }
    assert!((248..=252).contains(&ticks), "took {ticks} ticks");
    let health = *world.ecs.get::<&Health>(entity(&world, site)).unwrap();
    assert!(health.current >= health.max * 99 / 100, "a finished site should have nearly full health, not {}", health.current);
    let spent = 10_000.0 - world.economies[0].mass.to_num::<f64>();
    assert!((74.0..=76.0).contains(&spent), "spent {spent} materiel, but the cost is 75");
}

#[test]
fn mass_stall_slows_work_in_proportion_to_supply() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 0, 100_000);
    let extractor = id(&world, "mass_extractor");
    world.deposits.push((10, 10));
    world.spawn_finished_structure(extractor, Team::Blue, 10, 10).unwrap();
    let (_, site) = engineer_at_site(&mut world);
    run(&mut world, 5);
    let before = progress(&world, site);
    run(&mut world, 100);
    // Demand is 3 mass/s (75 mass over 25 s) against 2 mass/s of income, so work runs at 2/3 speed.
    let rate = (progress(&world, site) - before) / 100.0;
    let expected = (1.0 / 250.0) * (2.0 / 3.0);
    assert!((rate - expected).abs() < expected * 0.02, "rate {rate}, expected {expected}");
    assert!(world.economies[0].mass.to_num::<f64>() < 0.5, "a stalled team spends what comes in");
}

#[test]
fn energy_stall_stops_work_even_with_mass_available() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 10_000, 0);
    let (_, site) = engineer_at_site(&mut world);
    run(&mut world, 100);
    assert_eq!(progress(&world, site), 0.0);
    assert_eq!(world.economies[0].mass, Res::from_num(10_000), "no materiel should be spent during an energy stall");
}

#[test]
fn storage_buffers_a_stall_until_it_runs_out() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 30, 100_000);
    let (_, site) = engineer_at_site(&mut world);
    // At 3 mass/s, 30 mass lasts 10 s (100 ticks), which is 40% of the job.
    run(&mut world, 150);
    let done = progress(&world, site);
    assert!((0.39..=0.41).contains(&done), "progress was {done}");
}

#[test]
fn assisting_pools_build_power_and_drains_faster() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 10_000, 100_000);
    let (_, site) = engineer_at_site(&mut world);
    let engineer_blueprint = id(&world, "t1_engineer");
    let helper = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_000, 1_040));
    world.advance_tick(&[SimCommand::Assist { units: vec![helper], target: site }]);
    run(&mut world, 5);
    let before = progress(&world, site);
    run(&mut world, 50);
    let rate = (progress(&world, site) - before) / 50.0;
    assert!((rate - 2.0 / 250.0).abs() < 0.0002, "two engineers should work at double speed, got {rate}");
}

#[test]
fn factory_builds_its_queue_and_assist_speeds_it_up() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 10_000, 100_000);
    let factory_blueprint = id(&world, "land_factory_t1");
    let bot = id(&world, "t1_light_walker");
    let factory = world.spawn_finished_structure(factory_blueprint, Team::Blue, 50, 50).unwrap();
    let count_bots = |world: &SimWorld| world.units.len() - 1;
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: bot, count: 2 }]);
    // Its build time at the factory's build power, in ticks.
    let power = world.blueprints.get(factory_blueprint).build_power;
    let ticks = (world.blueprints.get(bot).build_time / Res::from_num(power) * Res::from_num(10)).to_num::<u32>();
    run(&mut world, ticks - 5);
    assert_eq!(count_bots(&world), 0);
    run(&mut world, 10);
    assert_eq!(count_bots(&world), 1, "the first bot should be out by tick {}", ticks + 5);
    run(&mut world, ticks);
    assert_eq!(count_bots(&world), 2);
    run(&mut world, ticks + 20);
    assert_eq!(count_bots(&world), 2, "the queue should be empty");

    // With an assisting engineer, the same bot takes its build time over both build powers.
    let engineer_blueprint = id(&world, "t1_engineer");
    let both = Res::from_num(power + world.blueprints.get(engineer_blueprint).build_power);
    let assisted = (world.blueprints.get(bot).build_time / both * Res::from_num(10)).to_num::<u32>() + 4;
    assert!(assisted < ticks, "assisting should be faster");
    let helper = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_100, 1_100));
    let before = world.units.len();
    world.advance_tick(&[
        SimCommand::Assist { units: vec![helper], target: factory },
        SimCommand::QueueUnit { factory, blueprint: bot, count: 1 },
    ]);
    run(&mut world, assisted);
    assert_eq!(world.units.len(), before + 1, "assist should finish the bot within {assisted} ticks, not {ticks}");
}

#[test]
fn work_links_show_real_work_and_nothing_else() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 10_000, 100_000);
    let engineer_blueprint = id(&world, "t1_engineer");
    let generator = id(&world, "power_generator");
    let engineer = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_000, 1_000));
    let links = |world: &SimWorld| world.snapshot_for(Some(Team::Blue)).work_links;

    // Walking to a far-off site is not work.
    let (cell_x, cell_y) = (1_400 / CELL as u32, 1_000 / CELL as u32);
    world.advance_tick(&[SimCommand::Build { units: vec![engineer], blueprint: generator, cell_x, cell_y, queued: false }]);
    run(&mut world, 5);
    assert!(links(&world).is_empty(), "still walking");

    let cell = cell_y * world.terrain.cells_per_side() as u32 + cell_x;
    let mut ticks = 0;
    while links(&world).is_empty() {
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 2_000, "the engineer never started work");
    }
    let site = world.occupancy[&cell];
    let link = links(&world)[0];
    assert_eq!(link.builder, engineer);
    assert!(!link.reclaim);
    let view = *world.snapshot_for(Some(Team::Blue)).units.iter().find(|unit| unit.id == site).unwrap();
    assert_eq!(link.target[..2], [view.x, view.y], "the link should point at the site");

    // The link lasts as long as the work does.
    while !is_finished(&world, site) {
        assert_eq!(links(&world).len(), 1, "work is still going at {}", progress(&world, site));
        world.advance_tick(&[]);
    }
    world.advance_tick(&[]);
    assert!(links(&world).is_empty(), "nothing is left to do");
}

#[test]
fn a_team_with_nothing_to_spend_shows_no_work() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 10_000, 100_000);
    let (_, site) = engineer_at_site(&mut world);
    run(&mut world, 5);
    assert_eq!(world.snapshot_for(Some(Team::Blue)).work_links.len(), 1);
    set_resources(&mut world, Team::Blue, 0, 100_000);
    let before = progress(&world, site);
    run(&mut world, 5);
    assert_eq!(progress(&world, site), before, "the site must have stalled");
    assert!(world.snapshot_for(Some(Team::Blue)).work_links.is_empty());
    set_resources(&mut world, Team::Blue, 10_000, 100_000);
    run(&mut world, 2);
    assert_eq!(world.snapshot_for(Some(Team::Blue)).work_links.len(), 1, "the beam returns with the resources");
}

#[test]
fn work_links_are_hidden_by_fog() {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: true, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    set_resources(&mut world, Team::Blue, 10_000, 100_000);
    engineer_at_site(&mut world);
    run(&mut world, 5);
    assert_eq!(world.snapshot_for(Some(Team::Blue)).work_links.len(), 1, "a team sees its own work");
    assert!(world.snapshot_for(Some(Team::Red)).work_links.is_empty(), "the enemy cannot see it through fog");
}

#[test]
fn a_factory_under_construction_holds_its_orders_until_it_is_built() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 100_000, 1_000_000);
    let engineer_blueprint = id(&world, "t1_engineer");
    let (factory_blueprint, bot) = (id(&world, "land_factory_t1"), id(&world, "t1_light_walker"));
    let engineer = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_000, 1_000));
    let helper = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_000, 1_020));
    let (cell_x, cell_y) = (1_100 / CELL as u32, 1_000 / CELL as u32);
    world.advance_tick(&[SimCommand::Build { units: vec![engineer], blueprint: factory_blueprint, cell_x, cell_y, queued: false }]);
    let cell = cell_y * world.terrain.cells_per_side() as u32 + cell_x;
    let mut ticks = 0;
    while !world.occupancy.contains_key(&cell) {
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 200, "the engineer never started the site");
    }
    let site = world.occupancy[&cell];
    assert!(!is_finished(&world, site));

    let rally = point(1_500, 1_000);
    world.advance_tick(&[
        SimCommand::QueueUnit { factory: site, blueprint: bot, count: 2 },
        SimCommand::SetRally { factory: site, point: rally },
        SimCommand::Assist { units: vec![helper], target: site },
    ]);

    // Nothing comes out early, however much help the site gets.
    let mut ticks = 0;
    while !is_finished(&world, site) {
        let factory = world.ecs.get::<&Factory>(entity(&world, site)).unwrap();
        assert_eq!(factory.queue.len(), 2, "the queue must wait for the factory");
        assert_eq!(factory.progress, Res::ZERO, "no unit is started before the factory is built");
        assert_eq!(factory.rally, Some(rally));
        drop(factory);
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 5_000, "still at {}", progress(&world, site));
    }
    assert_eq!(world.units.len(), 3, "no unit may appear before the factory is built");

    // Once it is built, the queue runs and the units go to the rally point.
    let mut ticks = 0;
    while !world.ecs.get::<&Factory>(entity(&world, site)).unwrap().queue.is_empty() {
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 5_000, "the queue never finished");
    }
    assert_eq!(world.units.len(), 5, "both queued units should have been built");
    let newest = world.units.iter().map(|unit| unit.0).max().unwrap();
    // It drives out of the factory first, then heads for the rally point.
    let mut ticks = 0;
    while world.ecs.get::<&LeavingFactory>(entity(&world, newest)).is_ok() {
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 100, "the new unit never left the factory");
    }
    let order = *world.ecs.get::<&Order>(entity(&world, newest)).unwrap();
    assert_ne!(order.kind, OrderKind::Idle, "the new unit should head for the rally point");
}

#[test]
fn factory_repeat_loops_and_rally_sends_units() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 10_000, 100_000);
    let factory_blueprint = id(&world, "land_factory_t1");
    let bot = id(&world, "t1_light_walker");
    let factory = world.spawn_finished_structure(factory_blueprint, Team::Blue, 50, 50).unwrap();
    world.advance_tick(&[
        SimCommand::QueueUnit { factory, blueprint: bot, count: 1 },
        SimCommand::SetRepeat { factory, repeat: true },
        SimCommand::SetRally { factory, point: point(2_000, 1_000) },
    ]);
    // Long enough for four, at its build time and the factory's build power.
    let ticks = (world.blueprints.get(bot).build_time / Res::from_num(world.blueprints.get(factory_blueprint).build_power) * Res::from_num(10)).to_num::<u32>();
    run(&mut world, ticks * 4 + 20);
    let units: Vec<SimId> = world.units.iter().map(|unit| unit.0).filter(|unit| *unit != factory).collect();
    assert!(units.len() >= 4, "repeat should keep building, got {}", units.len());
    // At FAF's speed, a light bot may already have reached the rally point.
    let at = world.unit_position(units[1]).unwrap().xy();
    let heading = world.unit_order(units[1]).unwrap().kind == OrderKind::Move;
    assert!(heading || at.distance_squared(&point(2_000, 1_000)) < sim_engine::math::fp::wide_mul(fp_int(200), fp_int(200)), "new units should head to the rally point");
}

#[test]
fn upgrade_halts_production_then_improves_the_factory() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 10_000, 100_000);
    let (t1, t2, bot, tank) = (id(&world, "land_factory_t1"), id(&world, "land_factory_t2"), id(&world, "t1_light_walker"), id(&world, "t2_heavy_tank"));
    let factory = world.spawn_finished_structure(t1, Team::Blue, 50, 50).unwrap();
    // A T1 factory cannot build tanks, so this order is refused.
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: tank, count: 1 }]);
    assert!(world.ecs.get::<&Factory>(entity(&world, factory)).unwrap().queue.is_empty());

    world.advance_tick(&[
        SimCommand::QueueUnit { factory, blueprint: t2, count: 1 },
        SimCommand::QueueUnit { factory, blueprint: bot, count: 1 },
    ]);
    // FAF's 2,300 build time at 20 power is 115 s. The bot behind it must wait.
    run(&mut world, 1_100);
    assert_eq!(world.units.len(), 1, "production must halt during an upgrade");
    run(&mut world, 60);
    let power = world.ecs.get::<&Factory>(entity(&world, factory)).unwrap().power;
    assert_eq!(power, fp_int(40));
    assert_eq!(world.ecs.get::<&Health>(entity(&world, factory)).unwrap().max, fp_int(8_000));
    run(&mut world, 30);
    assert_eq!(world.units.len(), 2, "the queued bot should follow at the new build power");
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: tank, count: 1 }]);
    assert!(!world.ecs.get::<&Factory>(entity(&world, factory)).unwrap().queue.is_empty(), "a T2 factory can build tanks");
}

#[test]
fn an_upgrade_on_a_repeating_factory_happens_once_and_units_keep_looping() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 100_000, 1_000_000);
    let (t1, t2, bot) = (id(&world, "land_factory_t1"), id(&world, "land_factory_t2"), id(&world, "t1_light_walker"));
    let factory = world.spawn_finished_structure(t1, Team::Blue, 50, 50).unwrap();
    world.advance_tick(&[
        SimCommand::SetRepeat { factory, repeat: true },
        SimCommand::QueueUnit { factory, blueprint: t2, count: 1 },
        SimCommand::QueueUnit { factory, blueprint: bot, count: 1 },
    ]);
    run(&mut world, 1_400);
    let queue = world.ecs.get::<&Factory>(entity(&world, factory)).unwrap().queue.clone();
    assert_eq!(queue, [bot], "only the bot should loop");
    assert_eq!(world.ecs.get::<&Factory>(entity(&world, factory)).unwrap().power, fp_int(40));
    assert!(world.units.len() >= 3, "the bots should keep coming after the upgrade, got {} units", world.units.len());
}

#[test]
fn asking_for_an_upgrade_twice_only_queues_it_once() {
    let mut world = test_world();
    let (t1, t2) = (id(&world, "land_factory_t1"), id(&world, "land_factory_t2"));
    let factory = world.spawn_finished_structure(t1, Team::Blue, 50, 50).unwrap();
    world.advance_tick(&[
        SimCommand::QueueUnit { factory, blueprint: t2, count: 1 },
        SimCommand::QueueUnit { factory, blueprint: t2, count: 3 },
    ]);
    assert_eq!(world.ecs.get::<&Factory>(entity(&world, factory)).unwrap().queue, [t2]);
}

#[test]
fn several_factories_can_take_the_same_orders() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 100_000, 1_000_000);
    let (t1, t2, bot, tank) = (id(&world, "land_factory_t1"), id(&world, "land_factory_t2"), id(&world, "t1_light_walker"), id(&world, "t2_heavy_tank"));
    let first = world.spawn_finished_structure(t1, Team::Blue, 50, 50).unwrap();
    let second = world.spawn_finished_structure(t1, Team::Blue, 60, 50).unwrap();
    let third = world.spawn_finished_structure(t2, Team::Blue, 70, 50).unwrap();
    let factories = [first, second, third];
    let mut commands: Vec<SimCommand> = factories.iter().map(|&factory| SimCommand::QueueUnit { factory, blueprint: bot, count: 1 }).collect();
    commands.extend(factories.iter().map(|&factory| SimCommand::QueueUnit { factory, blueprint: tank, count: 1 }));
    commands.extend(factories.iter().map(|&factory| SimCommand::SetRepeat { factory, repeat: true }));
    commands.extend(factories.iter().map(|&factory| SimCommand::SetRally { factory, point: point(2_000, 2_000) }));
    world.advance_tick(&commands);
    let state = world.snapshot_for(Some(Team::Blue)).factories;
    assert_eq!(state[0].queue, [bot.0], "a T1 factory cannot take the tank");
    assert_eq!(state[1].queue, [bot.0]);
    assert_eq!(state[2].queue, [bot.0, tank.0], "the T2 factory takes both");
    assert!(state.iter().all(|factory| factory.repeat && factory.rally.is_some()));
}

#[test]
fn extractor_output_grows_with_each_touching_storage() {
    let mut world = test_world();
    let (extractor, storage) = (id(&world, "mass_extractor"), id(&world, "mass_storage"));
    world.deposits.push((50, 50));
    world.spawn_finished_structure(extractor, Team::Blue, 50, 50).unwrap();
    world.advance_tick(&[]);
    let per_second = |world: &SimWorld| (world.economies[0].mass_income * 10).to_num::<f64>();
    assert!((per_second(&world) - 2.0).abs() < 0.001);
    for (count, (x, y)) in [(49, 50), (51, 50), (50, 49), (50, 51)].into_iter().enumerate() {
        world.spawn_finished_structure(storage, Team::Blue, x, y).unwrap();
        world.advance_tick(&[]);
        let expected = 2.0 * (1.0 + 0.125 * (count + 1) as f64);
        assert!((per_second(&world) - expected).abs() < 0.001, "{} storages gave {}", count + 1, per_second(&world));
    }
}

/// What a factory building a bot asks for, with these neighbors on its west side.
fn factory_demand(neighbors: &[&str]) -> (f64, f64) {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 100_000, 100_000);
    let (t1, bot) = (id(&world, "land_factory_t1"), id(&world, "t1_light_walker"));
    let factory = world.spawn_finished_structure(t1, Team::Blue, 50, 50).unwrap();
    for (index, key) in neighbors.iter().enumerate() {
        if *key == "mass_extractor" {
            world.deposits.push((49, 50 + index as u32));
        }
        world.spawn_finished_structure(id(&world, key), Team::Blue, 49, 50 + index as u32).unwrap();
    }
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: bot, count: 1 }]);
    run(&mut world, 3);
    let economy = world.economies[0];
    (economy.mass_demand.to_num::<f64>(), economy.energy_demand.to_num::<f64>())
}

#[test]
fn neighbors_cut_a_factorys_costs_as_in_faf() {
    let (base_mass, base_energy) = factory_demand(&[]);
    // A T1 generator takes 1.563% off a factory's energy, and nothing off its mass.
    let (mass, energy) = factory_demand(&["power_generator", "power_generator"]);
    assert!((mass - base_mass).abs() < 1e-5, "materiel should not change: {mass} against {base_mass}");
    assert!((energy / base_energy - 0.968_74).abs() < 0.000_2, "two T1 generators: {}", energy / base_energy);
    // A T1 extractor takes 7.5% off its mass.
    let (mass, _) = factory_demand(&["mass_extractor"]);
    assert!((mass / base_mass - 0.925).abs() < 0.000_2, "an extractor: {}", mass / base_mass);
    // Mass storage gives a factory nothing.
    let (mass, energy) = factory_demand(&["mass_storage"]);
    assert!((mass - base_mass).abs() < 1e-5 && (energy - base_energy).abs() < 1e-5);
}

#[test]
fn energy_storage_holds_energy_and_adds_to_the_generators_it_touches() {
    let mut world = test_world();
    let (generator, storage) = (id(&world, "power_generator"), id(&world, "energy_storage"));
    world.spawn_finished_structure(generator, Team::Blue, 50, 50).unwrap();
    world.advance_tick(&[]);
    let per_second = |world: &SimWorld| (world.economies[0].energy_income * 10).to_num::<f64>();
    let capacity = world.economies[0].energy_capacity;
    assert!((per_second(&world) - 20.0).abs() < 0.001);
    world.spawn_finished_structure(storage, Team::Blue, 51, 50).unwrap();
    world.advance_tick(&[]);
    assert!((per_second(&world) - 25.0).abs() < 0.001, "a T1 generator makes a quarter more: {}", per_second(&world));
    assert_eq!(world.economies[0].energy_capacity - capacity, Res::from_num(10_000));
    // A T2 generator, of the 3 by 3 size, gets a twelfth more from each.
    let mut world = test_world();
    world.spawn_finished_structure(id(&world, "power_generator_t2"), Team::Blue, 50, 50).unwrap();
    world.spawn_finished_structure(storage, Team::Blue, 53, 50).unwrap();
    world.advance_tick(&[]);
    assert!((per_second(&world) - 500.0 * 1.083_34).abs() < 0.01, "{}", per_second(&world));
}

#[test]
fn structures_block_movement_and_units_walk_out_from_under_them() {
    let mut world = test_world();
    let (bot, factory) = (id(&world, "t1_light_walker"), id(&world, "land_factory_t1"));
    let unit = world.spawn_mobile(bot, Team::Blue, point(1_030, 1_030));
    let footprint = |at: Vec2Fp| (1_000..1_080).contains(&at.x.to_num::<i32>()) && (1_000..1_080).contains(&at.y.to_num::<i32>());
    assert!(footprint(world.unit_position(unit).unwrap().xy()), "the setup should place the unit where the factory goes");
    world.spawn_finished_structure(factory, Team::Blue, 50, 50).unwrap();
    // Moved off the footprint the moment the factory is placed.
    assert!(world.terrain.is_passable(world.unit_position(unit).unwrap().xy()), "the unit should have been moved off");
    run(&mut world, 30);

    world.advance_tick(&[SimCommand::Move { units: vec![unit], goal: point(1_030, 1_200) }]);
    let start = world.unit_position(unit).unwrap().xy();
    world.advance_tick(&[SimCommand::Move { units: vec![unit], goal: Vec2Fp::new(fp_int(2_060) - start.x, fp_int(2_060) - start.y) }]);
    for _ in 0..300 {
        world.advance_tick(&[]);
        assert!(world.terrain.is_passable(world.unit_position(unit).unwrap().xy()), "the unit entered the factory");
    }
}

#[test]
fn destroying_a_structure_opens_the_ground_and_leaves_a_wreck() {
    let mut world = test_world();
    let generator = id(&world, "power_generator");
    let structure = world.spawn_finished_structure(generator, Team::Blue, 50, 50).unwrap();
    assert!(!world.terrain.cell_is_open(50, 50));
    world.ecs.get::<&mut Health>(entity(&world, structure)).unwrap().current = Fp::ONE;
    let enemy = id(&world, "t1_light_walker");
    world.spawn_mobile(enemy, Team::Red, point(1_010, 1_100));
    run(&mut world, 60);
    assert!(world.unit_position(structure).is_none(), "the structure should have been destroyed");
    assert!(world.terrain.cell_is_open(50, 50), "the ground should be open again");
    assert_eq!(world.wrecks.len(), 1);
    let wreck = *world.ecs.get::<&sim_engine::sim::components::Wreck>(world.wrecks[0].1).unwrap();
    assert_eq!(wreck.mass.to_num::<i64>(), 60, "81% of a 75-materiel generator");
}

#[test]
fn a_builder_walking_to_its_site_past_an_idle_friend_gets_there() {
    // A unit that bumps into a settled friend near its goal counts as arrived, for crowds at a
    // rally point. A builder walking to a site 200 away, outside its reach of 100 but inside
    // that crowd distance, stopped against the friend, was given the same walk again, and
    // stood there for good.
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 1_000, 1_000);
    let engineer = world.spawn_mobile(id(&world, "t1_engineer"), Team::Blue, point(1_000, 1_000));
    world.spawn_mobile(id(&world, "t1_light_walker"), Team::Blue, point(1_000, 988));
    let generator = id(&world, "power_generator");
    world.advance_tick(&[SimCommand::Build { units: vec![engineer], blueprint: generator, cell_x: 60, cell_y: 50, queued: false }]);
    run(&mut world, 400);
    let started = world.structures.iter().any(|&(_, entity)| world.ecs.get::<&sim_engine::sim::components::Blueprinted>(entity).is_ok_and(|unit| unit.0 == generator));
    assert!(started, "the engineer never reached its site: it is at {:?}", world.unit_position(engineer).map(|at| (at.x.to_num::<i32>(), at.y.to_num::<i32>())));
}

#[test]
fn engineers_reclaim_wrecks_into_mass() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 1_000, 1_000);
    world.economies[0].mass = Res::ZERO;
    let (engineer_blueprint, bot) = (id(&world, "t1_engineer"), id(&world, "t1_light_walker"));
    let doomed = world.spawn_mobile(bot, Team::Red, point(1_000, 1_000));
    let engineer = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_400, 1_000));
    world.ecs.get::<&mut Health>(entity(&world, doomed)).unwrap().current = Fp::ONE;
    let attacker = world.spawn_mobile(bot, Team::Blue, point(1_100, 1_000));
    run(&mut world, 40);
    assert!(world.unit_position(doomed).is_none());
    let wreck_id = world.wrecks[0].0;
    world.ecs.get::<&mut Combat>(entity(&world, attacker)).unwrap().range = Fp::ZERO;

    world.advance_tick(&[SimCommand::Reclaim { units: vec![engineer], target: wreck_id }]);
    // The wreck holds 81% of 30 mass, or 24.3. It takes time to walk there and 2.4 s to eat.
    let mut saw_reclaim_link = false;
    let mut most_reclaim_rate = 0;
    for _ in 0..400 {
        world.advance_tick(&[]);
        let snapshot = world.snapshot_for(Some(Team::Blue));
        saw_reclaim_link |= snapshot.work_links.iter().any(|link| link.builder == engineer && link.reclaim);
        most_reclaim_rate = most_reclaim_rate.max(snapshot.economy.mass_reclaim);
    }
    assert!(saw_reclaim_link, "reclaiming should show as work");
    assert!(world.wrecks.is_empty(), "the wreck should be gone");
    let mass = world.economies[0].mass.to_num::<f64>();
    let held = world.blueprints.get(world.blueprints.id("t1_light_walker")).mass.to_num::<f64>() * 0.81;
    assert!((held - 0.1..=held + 0.1).contains(&mass), "reclaimed {mass} materiel, not the wreck's {held}");
    // The economy panel shows reclaim coming in while it lasts, and the total once it is done.
    let economy = world.snapshot_for(Some(Team::Blue)).economy;
    assert!(most_reclaim_rate > 0, "reclaim showed no rate");
    assert_eq!(economy.mass_reclaim, 0, "nothing is being reclaimed now");
    assert_eq!(economy.mass_reclaimed, held.floor() as i64, "all of it, in whole units");
    assert!(world.ecs.get::<&Builder>(entity(&world, engineer)).unwrap().tasks.is_empty());
}

#[test]
fn patrolling_engineers_reclaim_on_their_own_then_carry_on() {
    let mut world = test_world();
    let (engineer_blueprint, bot) = (id(&world, "t1_engineer"), id(&world, "t1_light_walker"));
    let doomed = world.spawn_mobile(bot, Team::Red, point(1_000, 1_000));
    world.ecs.get::<&mut Health>(entity(&world, doomed)).unwrap().current = Fp::ONE;
    let attacker = world.spawn_mobile(bot, Team::Blue, point(1_100, 1_000));
    run(&mut world, 40);
    world.ecs.get::<&mut Combat>(entity(&world, attacker)).unwrap().range = Fp::ZERO;
    assert_eq!(world.wrecks.len(), 1);
    set_resources(&mut world, Team::Blue, 0, 1_000);

    let engineer = world.spawn_mobile(engineer_blueprint, Team::Blue, point(900, 1_100));
    world.advance_tick(&[SimCommand::Patrol { units: vec![engineer], points: vec![point(800, 1_100), point(1_200, 1_100)], queued: false }]);
    run(&mut world, 300);
    assert!(world.wrecks.is_empty(), "the patrol should have found the wreck");
    let task = world.ecs.get::<&Builder>(entity(&world, engineer)).unwrap().tasks.first().copied();
    assert!(!matches!(task, Some(BuildTask::Reclaim { .. })));
    // Once back on patrol, it keeps moving between its points.
    let start = world.unit_position(engineer).unwrap().xy();
    run(&mut world, 40);
    let moved = world.unit_position(engineer).unwrap().xy();
    assert_ne!(start, moved, "it should have gone back to patrolling");
}

#[test]
fn patrolling_combat_units_loop_through_their_points() {
    let mut world = test_world();
    let bot = id(&world, "t1_light_walker");
    let unit = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[SimCommand::Patrol { units: vec![unit], points: vec![point(1_100, 1_000), point(1_000, 1_000)], queued: false }]);
    let mut visited_far = false;
    let mut returned = false;
    for _ in 0..200 {
        world.advance_tick(&[]);
        let x = world.unit_position(unit).unwrap().x.to_num::<i32>();
        visited_far |= x > 1_080;
        returned |= visited_far && x < 1_020;
    }
    assert!(visited_far && returned, "the unit should go out and come back");
}

#[test]
fn attack_order_closes_range_and_ignores_nearer_enemies() {
    let mut world = test_world();
    let bot = id(&world, "t1_light_walker");
    let attacker = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    let near = world.spawn_mobile(bot, Team::Red, point(1_100, 1_000));
    let far = world.spawn_mobile(bot, Team::Red, point(2_000, 1_000));
    for unit in [near, far] {
        world.ecs.get::<&mut Combat>(entity(&world, unit)).unwrap().range = Fp::ZERO;
    }
    world.advance_tick(&[SimCommand::Attack { units: vec![attacker], target: far }]);
    for _ in 0..1_000 {
        world.advance_tick(&[]);
        if world.unit_position(far).is_none() {
            break;
        }
    }
    assert!(world.unit_position(far).is_none(), "the chosen target should die");
    world.advance_tick(&[]);
    assert_eq!(world.unit_order(attacker).unwrap().kind, OrderKind::Idle, "the order ends with the target");
    assert_eq!(world.ecs.get::<&Combat>(entity(&world, attacker)).unwrap().forced_target, None);
}

#[test]
fn shells_do_not_hit_allies_on_the_way_but_strays_and_blasts_do() {
    let mut world = SimWorld::new(
        SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, friendly_fire: true, ..SimConfig::default() },
        1,
    );
    let bot = id(&world, "t1_light_walker");
    let shooter = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    let front_rank = world.spawn_mobile(bot, Team::Blue, point(1_100, 1_000));
    let target = world.spawn_mobile(bot, Team::Red, point(1_200, 1_000));
    let bystander = world.spawn_mobile(bot, Team::Blue, point(1_300, 1_000));
    for unit in [shooter, front_rank, bystander, target] {
        world.ecs.get::<&mut Combat>(entity(&world, unit)).unwrap().range = Fp::ZERO;
    }
    let shoot = |world: &mut SimWorld, safe: u32, splash: Fp| {
        let z = fp_int(7);
        world.spawn_projectile(
            Vec3Fp::new(fp_int(1_000), fp_int(1_000), z),
            Projectile {
                velocity: Vec3Fp::new(fp_int(40), Fp::ZERO, Fp::ZERO),
                damage: fp_int(10),
                source: shooter,
                source_team: Team::Blue,
                lifetime_ticks: 3,
                splash_radius: splash,
                allied_safe_ticks: safe,
            },
        );
    };
    let health = |world: &SimWorld, unit: SimId| world.unit_health(unit).unwrap().to_num::<i32>();

    // Safe past the front rank: the shell reaches the enemy.
    shoot(&mut world, 4, Fp::ZERO);
    run(&mut world, 1);
    // Gravity drops the shell below the sphere only slowly, so run until it is gone.
    run(&mut world, 5);
    let full = world.blueprints.get(bot).health.to_num::<i32>();
    assert_eq!(health(&world, front_rank), full, "a shell must fly clear of friendly lines");

    // No longer safe: the same shell is a stray that hits the first ally in its path.
    shoot(&mut world, 0, Fp::ZERO);
    run(&mut world, 6);
    assert_eq!(health(&world, front_rank), full - 10, "a stray should hit an ally");

    // A blast hurts allies around the impact regardless.
    world.spawn_projectile(
        Vec3Fp::new(fp_int(1_290), fp_int(1_000), fp_int(7)),
        Projectile {
            velocity: Vec3Fp::new(fp_int(5), Fp::ZERO, Fp::ZERO),
            damage: fp_int(10),
            source: shooter,
            source_team: Team::Blue,
            lifetime_ticks: 20,
            splash_radius: fp_int(40),
            allied_safe_ticks: 0,
        },
    );
    run(&mut world, 10);
    assert_eq!(health(&world, bystander), full - 10, "a blast should hurt allies");
}

#[test]
fn fog_hides_enemies_from_targeting_and_the_players_view() {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: true, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    let (artillery, bot) = (id(&world, "t1_light_artillery"), id(&world, "t1_light_walker"));
    world.spawn_mobile(artillery, Team::Blue, point(1_000, 1_000));
    let enemy = world.spawn_mobile(bot, Team::Red, point(1_560, 1_000));
    world.ecs.get::<&mut Combat>(entity(&world, enemy)).unwrap().range = Fp::ZERO;

    // The artillery's range reaches the enemy, but its sight does not.
    let full = world.blueprints.get(bot).health;
    run(&mut world, 30);
    assert_eq!(world.unit_health(enemy), Some(full), "a unit must not fire at what its team cannot see");
    let view = world.snapshot_for(Some(Team::Blue));
    assert_eq!(view.units.len(), 1, "the player must not see the enemy");
    assert!(world.snapshot_for(Some(Team::Red)).units.iter().any(|unit| unit.id == enemy), "a team sees its own units");

    // A scout brings the enemy into sight, and the artillery opens fire.
    let scout = world.spawn_mobile(bot, Team::Blue, point(1_300, 1_000));
    world.ecs.get::<&mut Combat>(entity(&world, scout)).unwrap().range = Fp::ZERO;
    // Long enough for a few shots, since each scatters and may miss.
    run(&mut world, 300);
    assert!(world.unit_health(enemy).is_none_or(|health| health < full), "a shot should land once there is sight");
    // The artillery may well have destroyed it by now.
    assert!(world.unit_health(enemy).is_none() || world.snapshot_for(Some(Team::Blue)).units.iter().any(|unit| unit.id == enemy));
}

fn commander_with_neighbors() -> (SimWorld, SimId, SimId, SimId, SimId) {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    let (rig, bot, generator) = (id(&world, "command_rig"), id(&world, "t1_light_walker"), id(&world, "power_generator"));
    let blue = world.spawn_mobile(rig, Team::Blue, point(1_000, 1_000));
    let red = world.spawn_mobile(rig, Team::Red, point(4_000, 4_000));
    let nearby_enemy = world.spawn_mobile(bot, Team::Red, point(1_150, 1_000));
    let blue_base = world.spawn_finished_structure(generator, Team::Blue, 150, 150).unwrap();
    world.ecs.get::<&mut Combat>(entity(&world, nearby_enemy)).unwrap().range = Fp::ZERO;
    (world, blue, red, nearby_enemy, blue_base)
}

#[test]
fn commander_death_detonates_and_eliminates_its_team() {
    let (mut world, blue, red, nearby_enemy, blue_base) = commander_with_neighbors();
    world.ecs.get::<&mut Health>(entity(&world, blue)).unwrap().current = Fp::ONE;
    // Blue's commander must not shoot first: a light bot has only 60 health.
    world.ecs.get::<&mut Combat>(entity(&world, blue)).unwrap().range = Fp::ZERO;
    let shooter = world.spawn_mobile(id(&world, "t1_light_walker"), Team::Red, point(1_100, 1_100));
    run(&mut world, 30);
    assert_eq!(world.match_result, Some(MatchResult::Winner(Team::Red)));
    assert!(world.unit_position(blue).is_none());
    assert!(world.unit_position(blue_base).is_none(), "the rest of the team goes with its commander");
    assert!(world.unit_position(nearby_enemy).is_none(), "the reactor blast should kill nearby enemies");
    assert!(world.unit_position(shooter).is_none(), "the reactor blast should kill nearby enemies");
    assert!(world.unit_position(red).is_some(), "the far commander survives");
    assert_eq!(world.snapshot_for(Some(Team::Blue)).match_result, Some(MatchResult::Winner(Team::Red)));
}

fn plan_kinds(world: &SimWorld, unit: SimId) -> Vec<PlanStepKind> {
    let snapshot = world.snapshot_for(Some(Team::Blue));
    snapshot
        .plans
        .iter()
        .find(|plan| plan.id == unit)
        .map_or_else(Vec::new, |plan| plan.steps.iter().map(|step| step.kind).collect())
}

#[test]
fn queued_builds_appear_in_order_and_leave_the_plan_when_finished() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 10_000, 100_000);
    let (engineer_blueprint, generator, storage) = (id(&world, "t1_engineer"), id(&world, "power_generator"), id(&world, "mass_storage"));
    let engineer = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[
        SimCommand::Build { units: vec![engineer], blueprint: generator, cell_x: 52, cell_y: 50, queued: false },
        SimCommand::Build { units: vec![engineer], blueprint: storage, cell_x: 56, cell_y: 50, queued: true },
        SimCommand::Build { units: vec![engineer], blueprint: generator, cell_x: 60, cell_y: 50, queued: true },
    ]);
    let snapshot = world.snapshot_for(Some(Team::Blue));
    let plan = snapshot.plans.iter().find(|plan| plan.id == engineer).expect("the engineer has a plan");
    assert_eq!(plan.steps.len(), 3);
    let cell_center = |cell: i32| fp_int(cell * CELL + CELL / 2).to_bits();
    let xs: Vec<i32> = plan.steps.iter().map(|step| step.x).collect();
    assert_eq!(xs, [cell_center(52), cell_center(56), cell_center(60)], "steps must be in build order");

    // A step shows an outline until its site exists.
    run(&mut world, 4);
    let snapshot = world.snapshot_for(Some(Team::Blue));
    let plan = snapshot.plans.iter().find(|plan| plan.id == engineer).unwrap();
    assert_eq!(plan.steps[0].pending_width, 0, "the first site has started");
    assert_eq!(plan.steps[1].pending_width, fp_int(CELL).to_bits(), "the second has not");

    // The three builds take 25, 50, and 25 seconds of work, plus the walks between them.
    run(&mut world, 1_400);
    assert!(plan_kinds(&world, engineer).is_empty(), "a finished queue leaves nothing to show");
}

#[test]
fn plans_show_moves_attacks_patrols_and_support() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 10_000, 100_000);
    let (bot, engineer_blueprint, generator) = (id(&world, "t1_light_walker"), id(&world, "t1_engineer"), id(&world, "power_generator"));
    let walker = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    let attacker = world.spawn_mobile(bot, Team::Blue, point(1_100, 1_000));
    let fighter = world.spawn_mobile(bot, Team::Blue, point(1_200, 1_000));
    let patroller = world.spawn_mobile(bot, Team::Blue, point(1_300, 1_000));
    let idle = world.spawn_mobile(bot, Team::Blue, point(1_400, 1_000));
    let helper = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_500, 1_000));
    let enemy = world.spawn_mobile(bot, Team::Red, point(4_000, 4_000));
    let site = world.spawn_site(generator, Team::Blue, 100, 100).unwrap();
    world.ecs.get::<&mut Combat>(entity(&world, enemy)).unwrap().range = Fp::ZERO;
    world.advance_tick(&[
        SimCommand::Move { units: vec![walker], goal: point(3_000, 1_000) },
        SimCommand::AttackMove { units: vec![attacker], goal: point(3_000, 1_500) },
        SimCommand::Attack { units: vec![fighter], target: enemy },
        SimCommand::Patrol { units: vec![patroller], points: vec![point(1_300, 1_200), point(1_500, 1_200)], queued: false },
        SimCommand::Assist { units: vec![helper], target: site },
    ]);
    world.advance_tick(&[]);
    assert_eq!(plan_kinds(&world, walker), [PlanStepKind::Move]);
    assert_eq!(plan_kinds(&world, attacker), [PlanStepKind::AttackMove]);
    assert_eq!(plan_kinds(&world, fighter), [PlanStepKind::Attack]);
    assert_eq!(plan_kinds(&world, helper), [PlanStepKind::Support]);
    assert_eq!(&plan_kinds(&world, patroller)[1..], [PlanStepKind::Patrol, PlanStepKind::Patrol]);
    assert!(plan_kinds(&world, idle).is_empty());
}

#[test]
fn plans_never_show_the_other_team_or_a_hidden_target() {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: true, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    let bot = id(&world, "t1_light_walker");
    let attacker = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    let enemy = world.spawn_mobile(bot, Team::Red, point(4_000, 4_000));
    world.ecs.get::<&mut Combat>(entity(&world, enemy)).unwrap().range = Fp::ZERO;
    world.advance_tick(&[
        SimCommand::Attack { units: vec![attacker], target: enemy },
        SimCommand::Move { units: vec![enemy], goal: point(4_500, 4_500) },
    ]);
    world.advance_tick(&[]);
    let snapshot = world.snapshot_for(Some(Team::Blue));
    assert!(snapshot.plans.iter().all(|plan| plan.id != enemy), "another team's orders must stay hidden");
    assert!(plan_kinds(&world, attacker).is_empty(), "a trail must not reveal a target in the fog");
}

#[test]
fn factory_rally_points_reach_the_snapshot() {
    let mut world = test_world();
    let t1 = id(&world, "land_factory_t1");
    let factory = world.spawn_finished_structure(t1, Team::Blue, 50, 50).unwrap();
    let rally_of = |world: &SimWorld| world.snapshot_for(Some(Team::Blue)).factories[0].rally;
    assert_eq!(rally_of(&world), None);
    world.advance_tick(&[SimCommand::SetRally { factory, point: point(2_000, 1_500) }]);
    let rally = rally_of(&world).expect("the rally point should be visible");
    assert_eq!((rally[0], rally[1]), (fp_int(2_000).to_bits(), fp_int(1_500).to_bits()));
}

/// Plans are read for the player's view, and must never disturb the simulation.
#[test]
fn reading_plans_does_not_change_the_checksum() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 1_000, 10_000);
    let (_, _) = engineer_at_site(&mut world);
    let before = world.compute_checksum();
    let _ = world.snapshot_for(Some(Team::Blue));
    assert_eq!(world.compute_checksum(), before);
}

#[test]
fn commanders_killed_together_is_a_draw() {
    let mut world = test_world();
    let rig = id(&world, "command_rig");
    let blue = world.spawn_mobile(rig, Team::Blue, point(1_000, 1_000));
    let red = world.spawn_mobile(rig, Team::Red, point(1_200, 1_000));
    world.ecs.get::<&mut Health>(entity(&world, blue)).unwrap().current = Fp::ONE;
    // When one reactor goes, its blast reaches the other, which detonates in the same tick.
    for _ in 0..30 {
        world.advance_tick(&[]);
        if world.match_result.is_some() {
            break;
        }
    }
    assert_eq!(world.match_result, Some(MatchResult::Draw));
    assert!(world.unit_position(red).is_none());
}

#[test]
fn skirmish_map_is_mirrored_and_starts_with_resources() {
    let scenario = scenario::skirmish(42);
    let world = scenario.world;
    let last = world.terrain.cells_per_side() as u32 - 1;
    assert_eq!(world.deposits.len(), 24);
    for &(x, y) in &world.deposits {
        assert!(world.deposits.contains(&(last - x, last - y)), "deposit ({x}, {y}) has no mirror image");
        assert!(world.terrain.cell_is_open(x, y));
    }
    let (blue, red) = (world.commanders[0].unwrap(), world.commanders[1].unwrap());
    let (blue_at, red_at) = (world.unit_position(blue).unwrap().xy(), world.unit_position(red).unwrap().xy());
    assert_eq!(blue_at, skirmish_start(Team::Blue));
    assert_eq!(red_at, skirmish_start(Team::Red));
    assert_eq!(world.economies[0].mass, world.economies[1].mass);
    assert_eq!(world.economies[0].mass, Res::from_num(650));
    assert_eq!(world.economies[0].energy, Res::from_num(4_000));
}

/// Every new kind of state must reach the checksum, or a desync in it goes unnoticed.
#[test]
fn production_state_changes_the_checksum() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 1_000, 10_000);
    let (engineer, site) = engineer_at_site(&mut world);
    let t1 = id(&world, "land_factory_t1");
    let bot = id(&world, "t1_light_walker");
    let factory = world.spawn_finished_structure(t1, Team::Blue, 150, 150).unwrap();
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: bot, count: 2 }]);
    run(&mut world, 3);
    let baseline = world.compute_checksum();
    let check = |world: &mut SimWorld, name: &str, change: &dyn Fn(&mut SimWorld), undo: &dyn Fn(&mut SimWorld)| {
        change(world);
        assert_ne!(world.compute_checksum(), baseline, "{name} is not in the checksum");
        undo(world);
        assert_eq!(world.compute_checksum(), baseline, "undoing {name} did not restore the checksum");
    };
    let tiny = Res::from_bits(1);
    check(&mut world, "stored materiel", &|w| w.economies[0].mass += tiny, &|w| w.economies[0].mass -= tiny);
    check(&mut world, "stored energy", &|w| w.economies[1].energy += tiny, &|w| w.economies[1].energy -= tiny);
    let site_entity = entity(&world, site);
    check(
        &mut world,
        "construction progress",
        &|w| w.ecs.get::<&mut Construction>(site_entity).unwrap().progress += tiny,
        &|w| w.ecs.get::<&mut Construction>(site_entity).unwrap().progress -= tiny,
    );
    let factory_entity = entity(&world, factory);
    check(
        &mut world,
        "factory progress",
        &|w| w.ecs.get::<&mut Factory>(factory_entity).unwrap().progress += tiny,
        &|w| w.ecs.get::<&mut Factory>(factory_entity).unwrap().progress -= tiny,
    );
    let engineer_entity = entity(&world, engineer);
    check(
        &mut world,
        "builder tasks",
        &|w| w.ecs.get::<&mut Builder>(engineer_entity).unwrap().tasks.push(BuildTask::Assist { target: SimId(9) }),
        &|w| {
            w.ecs.get::<&mut Builder>(engineer_entity).unwrap().tasks.pop();
        },
    );
    check(
        &mut world,
        "match result",
        &|w| w.match_result = Some(MatchResult::Draw),
        &|w| w.match_result = None,
    );
}

/// A unit stopping short of a point that is not in the middle of a terrain cell must still
/// finish its order. Otherwise a builder never starts the builds queued behind it.
#[test]
fn an_engineer_at_any_rally_point_goes_on_to_its_queued_builds() {
    for rally in [(1_500, 1_000), (1_513, 1_013), (1_519, 1_009), (1_503, 1_017), (1_501, 1_001), (1_519, 1_019)] {
        let mut world = test_world();
        set_resources(&mut world, Team::Blue, 100_000, 1_000_000);
        let (factory_blueprint, engineer_blueprint, generator) = (id(&world, "land_factory_t1"), id(&world, "t1_engineer"), id(&world, "power_generator"));
        let factory = world.spawn_finished_structure(factory_blueprint, Team::Blue, 50, 50).unwrap();
        world.advance_tick(&[
            SimCommand::SetRally { factory, point: point(rally.0, rally.1) },
            SimCommand::QueueUnit { factory, blueprint: engineer_blueprint, count: 1 },
        ]);
        let mut ticks = 0;
        while world.ecs.get::<&Factory>(entity(&world, factory)).unwrap().queue.len() == 1 {
            world.advance_tick(&[]);
            ticks += 1;
            assert!(ticks < 2_000, "the engineer never came out");
        }
        let engineer = world.units.iter().map(|unit| unit.0).max().unwrap();
        run(&mut world, 400);
        assert_eq!(world.ecs.get::<&Order>(entity(&world, engineer)).unwrap().kind, OrderKind::Idle, "stuck on the way to {rally:?}");
        let here = world.unit_position(engineer).unwrap().xy();
        let distance = (here - point(rally.0, rally.1)).length_squared().to_num::<f64>().sqrt();
        assert!(distance < 10.0, "stopped {distance} from {rally:?}");

        let (cell_x, cell_y) = (150, 150);
        world.advance_tick(&[SimCommand::Build { units: vec![engineer], blueprint: generator, cell_x, cell_y, queued: true }]);
        run(&mut world, 800);
        assert!(world.ecs.get::<&Builder>(entity(&world, engineer)).unwrap().tasks.first().is_none_or(|task| matches!(task, BuildTask::Build { site: Some(_), .. })), "never reached the build from {rally:?}");
        assert!(world.occupancy.contains_key(&(cell_y * world.terrain.cells_per_side() as u32 + cell_x)), "never started the build from {rally:?}");
    }
}

#[test]
fn a_unit_sent_onto_a_structure_finishes_its_order_beside_it() {
    let mut world = test_world();
    let (factory_blueprint, bot) = (id(&world, "land_factory_t1"), id(&world, "t1_light_walker"));
    let factory = world.spawn_finished_structure(factory_blueprint, Team::Blue, 50, 50).unwrap();
    let inside = world.unit_position(factory).unwrap().xy();
    let unit = world.spawn_mobile(bot, Team::Blue, point(1_400, 1_000));
    world.advance_tick(&[SimCommand::Move { units: vec![unit], goal: inside }]);
    run(&mut world, 800);
    assert_eq!(world.ecs.get::<&Order>(entity(&world, unit)).unwrap().kind, OrderKind::Idle);
}

fn cell_of_index(world: &SimWorld, cell_x: u32, cell_y: u32) -> u32 {
    cell_y * world.terrain.cells_per_side() as u32 + cell_x
}

fn tasks(world: &SimWorld, unit: SimId) -> Vec<BuildTask> {
    world.ecs.get::<&Builder>(entity(world, unit)).unwrap().tasks.clone()
}

/// An engineer with three far-off builds queued, walking to the first.
fn engineer_with_three_builds(world: &mut SimWorld) -> (SimId, [BuildTask; 3]) {
    set_resources(world, Team::Blue, 100_000, 1_000_000);
    let engineer_blueprint = id(world, "t1_engineer");
    let generator = id(world, "power_generator");
    let engineer = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_000, 1_000));
    let order = |cell_x: u32, queued: bool| SimCommand::Build { units: vec![engineer], blueprint: generator, cell_x, cell_y: 50, queued };
    world.advance_tick(&[order(80, false), order(90, true), order(100, true)]);
    let task = |cell_x: u32| BuildTask::Build { blueprint: generator, cell_x, cell_y: 50, site: None };
    (engineer, [task(80), task(90), task(100)])
}

#[test]
fn a_builders_queue_is_shown_to_its_own_team_only() {
    let mut world = test_world();
    let (engineer, orders) = engineer_with_three_builds(&mut world);
    let own = world.snapshot_for(Some(Team::Blue));
    assert_eq!(own.build_queues.len(), 1);
    assert_eq!(own.build_queues[0].id, engineer);
    assert_eq!(own.build_queues[0].tasks, orders);
    assert!(world.snapshot_for(Some(Team::Red)).build_queues.is_empty());
}

#[test]
fn removing_a_later_order_leaves_the_others_running() {
    let mut world = test_world();
    let (engineer, orders) = engineer_with_three_builds(&mut world);
    run(&mut world, 5);
    assert_ne!(world.ecs.get::<&Order>(entity(&world, engineer)).unwrap().kind, OrderKind::Idle);
    world.advance_tick(&[SimCommand::CancelTask { units: vec![engineer], task: orders[1] }]);
    assert_eq!(tasks(&world, engineer), [orders[0], orders[2]]);
    assert_ne!(world.ecs.get::<&Order>(entity(&world, engineer)).unwrap().kind, OrderKind::Idle, "it should keep walking to the first");
}

#[test]
fn removing_the_order_being_walked_to_moves_on_to_the_next() {
    let mut world = test_world();
    let (engineer, orders) = engineer_with_three_builds(&mut world);
    run(&mut world, 5);
    world.advance_tick(&[SimCommand::CancelTask { units: vec![engineer], task: orders[0] }]);
    assert_eq!(tasks(&world, engineer), [orders[1], orders[2]]);
    let mut ticks = 0;
    while !world.occupancy.contains_key(&cell_of_index(&world, 90, 50)) {
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 1_000, "it never started the next build");
    }
    assert!(!world.occupancy.contains_key(&cell_of_index(&world, 80, 50)), "the removed build must never start");
}

#[test]
fn removing_an_order_under_way_leaves_its_site_and_stops_spending_on_it() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 10_000, 100_000);
    let (engineer, site) = engineer_at_site(&mut world);
    run(&mut world, 20);
    let task = tasks(&world, engineer)[0];
    assert!(matches!(task, BuildTask::Build { site: Some(_), .. }));
    // Named as the panel might have seen it, before the site appeared.
    let BuildTask::Build { blueprint, cell_x, cell_y, .. } = task else {
        unreachable!();
    };
    world.advance_tick(&[SimCommand::CancelTask { units: vec![engineer], task: BuildTask::Build { blueprint, cell_x, cell_y, site: None } }]);
    assert!(tasks(&world, engineer).is_empty());
    let (before, mass) = (progress(&world, site), world.economies[0].mass);
    run(&mut world, 50);
    assert_eq!(progress(&world, site), before, "nobody is building it now");
    assert_eq!(world.economies[0].mass, mass, "nothing should be spent on it");
    assert!(world.snapshot_for(Some(Team::Blue)).work_links.is_empty());
    assert!(world.unit_entity(site).is_some(), "the site stays");

    // It can still be finished.
    world.advance_tick(&[SimCommand::Assist { units: vec![engineer], target: site }]);
    run(&mut world, 400);
    assert!(is_finished(&world, site));
}

#[test]
fn removing_an_order_that_is_already_gone_does_nothing() {
    let mut world = test_world();
    let (engineer, orders) = engineer_with_three_builds(&mut world);
    let other = BuildTask::Build { blueprint: id(&world, "mass_storage"), cell_x: 80, cell_y: 50, site: None };
    world.advance_tick(&[SimCommand::CancelTask { units: vec![engineer], task: other }]);
    assert_eq!(tasks(&world, engineer), orders, "a different structure on the same spot is a different order");
    let dead = SimId(9_999);
    world.advance_tick(&[SimCommand::CancelTask { units: vec![dead], task: orders[0] }]);
    assert_eq!(tasks(&world, engineer), orders);
}

#[test]
fn removing_an_order_only_affects_the_builders_named() {
    let mut world = test_world();
    let (engineer, orders) = engineer_with_three_builds(&mut world);
    let helper_blueprint = id(&world, "t1_engineer");
    let helper = world.spawn_mobile(helper_blueprint, Team::Blue, point(1_000, 1_040));
    let generator = id(&world, "power_generator");
    world.advance_tick(&[SimCommand::Build { units: vec![helper], blueprint: generator, cell_x: 90, cell_y: 50, queued: false }]);
    world.advance_tick(&[SimCommand::CancelTask { units: vec![engineer], task: orders[1] }]);
    assert_eq!(tasks(&world, helper), [orders[1]]);
    assert_eq!(tasks(&world, engineer), [orders[0], orders[2]]);
}

#[test]
fn removing_a_factory_item_takes_the_last_one_and_keeps_work_under_way() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 100_000, 1_000_000);
    let (factory_blueprint, bot, artillery) = (id(&world, "land_factory_t1"), id(&world, "t1_light_walker"), id(&world, "t1_light_artillery"));
    let factory = world.spawn_finished_structure(factory_blueprint, Team::Blue, 50, 50).unwrap();
    world.advance_tick(&[
        SimCommand::QueueUnit { factory, blueprint: bot, count: 1 },
        SimCommand::QueueUnit { factory, blueprint: artillery, count: 1 },
        SimCommand::QueueUnit { factory, blueprint: bot, count: 1 },
    ]);
    run(&mut world, 5);
    let state = |world: &SimWorld| {
        let factory = world.ecs.get::<&Factory>(entity(world, factory)).unwrap();
        (factory.queue.clone(), factory.progress)
    };
    let under_way = state(&world).1;
    assert!(under_way > Res::ZERO);

    world.advance_tick(&[SimCommand::RemoveQueued { factory, blueprint: bot }]);
    let (queue, progress_now) = state(&world);
    assert_eq!(queue, [bot, artillery]);
    assert!(progress_now > under_way, "the bot under way should carry on");

    world.advance_tick(&[SimCommand::RemoveQueued { factory, blueprint: id(&world, "t2_heavy_tank") }]);
    assert_eq!(state(&world).0, [bot, artillery], "an item not in the queue changes nothing");

    // The last bot is the one under way, so its work goes with it.
    world.advance_tick(&[SimCommand::RemoveQueued { factory, blueprint: bot }]);
    let (queue, _) = state(&world);
    assert_eq!(queue, [artillery]);
    let artillery_progress = state(&world).1;
    assert!(artillery_progress < under_way * 2, "the next item starts from the beginning, not {artillery_progress}");
}

/// A T1 factory with an upgrade queued, and everything needed for it.
fn factory_upgrading(world: &mut SimWorld) -> (SimId, BlueprintId, BlueprintId, BlueprintId) {
    set_resources(world, Team::Blue, 100_000, 1_000_000);
    let (t1, t2, tank, bot) = (id(world, "land_factory_t1"), id(world, "land_factory_t2"), id(world, "t2_heavy_tank"), id(world, "t1_light_walker"));
    let factory = world.spawn_finished_structure(t1, Team::Blue, 50, 50).unwrap();
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: t2, count: 1 }]);
    (factory, t2, tank, bot)
}

fn queue_of(world: &SimWorld, factory: SimId) -> Vec<BlueprintId> {
    world.ecs.get::<&Factory>(entity(world, factory)).unwrap().queue.clone()
}

#[test]
fn units_queued_during_an_upgrade_wait_for_it_and_then_get_built() {
    let mut world = test_world();
    let (factory, t2, tank, _) = factory_upgrading(&mut world);
    let units_before = world.units.len();
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: tank, count: 2 }]);
    assert_eq!(queue_of(&world, factory), [t2, tank, tank]);

    let mut ticks = 0;
    while world.ecs.get::<&Blueprinted>(entity(&world, factory)).unwrap().0 != t2 {
        assert_eq!(world.units.len(), units_before, "no tank may appear before the upgrade finishes");
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 5_000, "the upgrade never finished");
    }
    assert_eq!(queue_of(&world, factory), [tank, tank], "the tanks should still be waiting");
    let mut ticks = 0;
    while !queue_of(&world, factory).is_empty() {
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 5_000, "the tanks never finished");
    }
    let newest = world.units.iter().map(|unit| unit.0).max().unwrap();
    assert_eq!(world.ecs.get::<&Blueprinted>(entity(&world, newest)).unwrap().0, tank);
    assert_eq!(world.units.len(), units_before + 2);
}

#[test]
fn a_factory_without_an_upgrade_queued_still_refuses_higher_units() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 100_000, 1_000_000);
    let (t1, tank, bot) = (id(&world, "land_factory_t1"), id(&world, "t2_heavy_tank"), id(&world, "t1_light_walker"));
    let factory = world.spawn_finished_structure(t1, Team::Blue, 50, 50).unwrap();
    world.advance_tick(&[
        SimCommand::QueueUnit { factory, blueprint: bot, count: 1 },
        SimCommand::QueueUnit { factory, blueprint: tank, count: 1 },
    ]);
    assert_eq!(queue_of(&world, factory), [bot]);
}

#[test]
fn removing_an_upgrade_removes_what_was_waiting_for_it() {
    let mut world = test_world();
    let (factory, t2, tank, bot) = factory_upgrading(&mut world);
    world.advance_tick(&[
        SimCommand::QueueUnit { factory, blueprint: tank, count: 2 },
        SimCommand::QueueUnit { factory, blueprint: bot, count: 1 },
    ]);
    assert_eq!(queue_of(&world, factory), [t2, tank, tank, bot]);
    world.advance_tick(&[SimCommand::RemoveQueued { factory, blueprint: t2 }]);
    assert_eq!(queue_of(&world, factory), [bot], "a T1 factory must be left with nothing it cannot build");
    assert_eq!(world.ecs.get::<&Blueprinted>(entity(&world, factory)).unwrap().0, id(&world, "land_factory_t1"));
    run(&mut world, 400);
    assert!(world.units.iter().all(|unit| world.ecs.get::<&Blueprinted>(unit.1).unwrap().0 != tank), "no tank may ever come out");
}

#[test]
fn removing_a_waiting_unit_leaves_the_upgrade_alone() {
    let mut world = test_world();
    let (factory, t2, tank, _) = factory_upgrading(&mut world);
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: tank, count: 1 }]);
    run(&mut world, 5);
    let under_way = world.ecs.get::<&Factory>(entity(&world, factory)).unwrap().progress;
    world.advance_tick(&[SimCommand::RemoveQueued { factory, blueprint: tank }]);
    assert_eq!(queue_of(&world, factory), [t2]);
    assert!(world.ecs.get::<&Factory>(entity(&world, factory)).unwrap().progress > under_way, "the upgrade should carry on");
}

#[test]
fn a_repeating_factory_keeps_a_waiting_unit_behind_the_upgrade() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 100_000, 1_000_000);
    let (t1, t2, tank, bot) = (id(&world, "land_factory_t1"), id(&world, "land_factory_t2"), id(&world, "t2_heavy_tank"), id(&world, "t1_light_walker"));
    let factory = world.spawn_finished_structure(t1, Team::Blue, 50, 50).unwrap();
    world.advance_tick(&[
        SimCommand::SetRepeat { factory, repeat: true },
        SimCommand::QueueUnit { factory, blueprint: bot, count: 1 },
        SimCommand::QueueUnit { factory, blueprint: t2, count: 1 },
        SimCommand::QueueUnit { factory, blueprint: tank, count: 1 },
    ]);
    let mut ticks = 0;
    while world.ecs.get::<&Blueprinted>(entity(&world, factory)).unwrap().0 != t2 {
        let queue = queue_of(&world, factory);
        assert_ne!(queue.first(), Some(&tank), "a tank reached the front before the upgrade finished: {queue:?}");
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 5_000, "the upgrade never finished");
    }
    assert!(queue_of(&world, factory).contains(&tank));
}

fn distance_between(world: &SimWorld, first: SimId, second: SimId) -> f64 {
    let (first, second) = (world.unit_position(first).unwrap().xy(), world.unit_position(second).unwrap().xy());
    (first - second).length_squared().to_num::<f64>().sqrt()
}

#[test]
fn a_follower_waits_beside_its_unit_without_pushing_it() {
    let mut world = test_world();
    let (rig, bot) = (id(&world, "command_rig"), id(&world, "t1_light_walker"));
    let ward = world.spawn_mobile(rig, Team::Blue, point(1_000, 1_000));
    let guard = world.spawn_mobile(bot, Team::Blue, point(1_300, 1_000));
    let start = world.unit_position(ward).unwrap();
    world.advance_tick(&[SimCommand::Follow { units: vec![guard], target: ward }]);
    run(&mut world, 300);
    let distance = distance_between(&world, ward, guard);
    assert!((16.0..=60.0).contains(&distance), "the guard should wait close by, not at {distance}");
    assert_eq!(world.unit_position(ward).unwrap(), start, "the guard must not shove the unit it follows");
    assert_ne!(world.ecs.get::<&Order>(entity(&world, guard)).unwrap().kind, OrderKind::Idle, "it should still be on guard");

    // And it goes with the unit when it moves.
    world.advance_tick(&[SimCommand::Move { units: vec![ward], goal: point(1_000, 1_800) }]);
    run(&mut world, 800);
    assert!(distance_between(&world, ward, guard) < 80.0, "the guard was left behind");
}

#[test]
fn a_follower_fires_at_enemies_in_range_without_leaving_its_unit() {
    let mut world = test_world();
    let (engineer, bot) = (id(&world, "t1_engineer"), id(&world, "t1_light_walker"));
    let ward = world.spawn_mobile(engineer, Team::Blue, point(1_000, 1_000));
    let guard = world.spawn_mobile(bot, Team::Blue, point(1_030, 1_000));
    world.advance_tick(&[SimCommand::Follow { units: vec![guard], target: ward }]);
    run(&mut world, 30);
    let enemy = world.spawn_mobile(bot, Team::Red, point(1_100, 1_000));
    let full = world.ecs.get::<&Health>(entity(&world, enemy)).unwrap().current;
    for _ in 0..80 {
        world.advance_tick(&[]);
        if world.unit_entity(enemy).is_none() {
            break;
        }
    }
    let hurt = world.unit_entity(enemy).is_none_or(|enemy| world.ecs.get::<&Health>(enemy).unwrap().current < full);
    assert!(hurt, "the guard should fire at an enemy in range");
    if world.unit_entity(guard).is_some() {
        assert!(distance_between(&world, ward, guard) < 80.0, "the guard must not chase away from its unit");
    }
}

#[test]
fn a_follower_stands_down_when_its_unit_dies() {
    let mut world = test_world();
    let (engineer, bot, rig) = (id(&world, "t1_engineer"), id(&world, "t1_light_walker"), id(&world, "command_rig"));
    let ward = world.spawn_mobile(engineer, Team::Blue, point(1_000, 1_000));
    let guard = world.spawn_mobile(rig, Team::Blue, point(1_200, 1_000));
    world.advance_tick(&[SimCommand::Follow { units: vec![guard], target: ward }]);
    world.ecs.get::<&mut Health>(entity(&world, ward)).unwrap().current = Fp::ONE;
    world.spawn_mobile(bot, Team::Red, point(1_000, 1_040));
    run(&mut world, 600);
    assert!(world.unit_entity(ward).is_none(), "the test needs the unit to die");
    assert_eq!(world.ecs.get::<&Order>(entity(&world, guard)).unwrap().kind, OrderKind::Idle, "there is nothing left to follow");
}

fn order_kind(world: &SimWorld, unit: SimId) -> OrderKind {
    world.ecs.get::<&Order>(entity(world, unit)).unwrap().kind
}

fn distance_to(world: &SimWorld, unit: SimId, place: Vec2Fp) -> f64 {
    (world.unit_position(unit).unwrap().xy() - place).length_squared().to_num::<f64>().sqrt()
}

/// Runs until a unit comes near a place, and returns how long that took.
fn run_until_near(world: &mut SimWorld, unit: SimId, place: Vec2Fp, within: f64) -> u32 {
    for tick in 0..3_000 {
        if distance_to(world, unit, place) <= within {
            return tick;
        }
        world.advance_tick(&[]);
    }
    panic!("never got near {place:?}, stopped at {:?}", world.unit_position(unit));
}

#[test]
fn a_unit_works_through_queued_moves_in_order_then_stops() {
    let mut world = test_world();
    let bot = id(&world, "t1_light_walker");
    let unit = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    let (first, second, third) = (point(1_300, 1_000), point(1_300, 1_300), point(1_000, 1_300));
    world.advance_tick(&[
        SimCommand::Move { units: vec![unit], goal: first },
        SimCommand::QueueMove { units: vec![unit], goal: second, attack: false },
        SimCommand::QueueMove { units: vec![unit], goal: third, attack: false },
    ]);
    assert!(distance_to(&world, unit, first) > 100.0);
    run_until_near(&mut world, unit, first, 12.0);
    assert!(distance_to(&world, unit, third) > 200.0, "it must not cut straight to the last stop");
    run_until_near(&mut world, unit, second, 12.0);
    run_until_near(&mut world, unit, third, 12.0);
    run(&mut world, 30);
    assert_eq!(order_kind(&world, unit), OrderKind::Idle);
    assert!(world.order_queues.is_empty());
    assert!(world.ecs.get::<&sim_engine::sim::components::OrderQueue>(entity(&world, unit)).is_err());
    assert_eq!(world.flow_field.route_count(), 0, "every route should have been handed back");
}

#[test]
fn a_queued_move_starts_straight_away_for_an_idle_unit() {
    let mut world = test_world();
    let bot = id(&world, "t1_light_walker");
    let unit = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[SimCommand::QueueMove { units: vec![unit], goal: point(1_300, 1_000), attack: true }]);
    assert_eq!(order_kind(&world, unit), OrderKind::AttackMove);
    assert!(world.order_queues.is_empty(), "nothing is left waiting");
}

#[test]
fn a_group_shares_a_route_for_each_stop_and_forms_up_at_each_one() {
    let mut world = test_world();
    let bot = id(&world, "t1_light_walker");
    let units: Vec<SimId> = (0..16).map(|index| world.spawn_mobile(bot, Team::Blue, point(1_000 + index % 4 * 30, 1_000 + index / 4 * 30))).collect();
    let (first, second) = (point(1_600, 1_000), point(1_600, 1_600));
    world.advance_tick(&[
        SimCommand::Move { units: units.clone(), goal: first },
        SimCommand::QueueMove { units: units.clone(), goal: second, attack: false },
    ]);
    assert_eq!(world.flow_field.route_count(), 2, "one route per stop, not one per unit");
    assert_eq!(world.order_queues.len(), 16);

    let mut ticks = 0;
    while units.iter().any(|unit| order_kind(&world, *unit) != OrderKind::Idle) || !world.order_queues.is_empty() {
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 3_000, "the group never finished");
    }
    let spots: Vec<Vec2Fp> = units.iter().map(|unit| world.unit_position(*unit).unwrap().xy()).collect();
    for (index, spot) in spots.iter().enumerate() {
        let away = (*spot - second).length_squared().to_num::<f64>().sqrt();
        assert!(away < 90.0, "unit {index} ended {away} from the last stop");
        for other in &spots[index + 1..] {
            let gap = (*spot - *other).length_squared().to_num::<f64>().sqrt();
            assert!(gap > 10.0, "units should end in a formation, but two are {gap} apart");
        }
    }
    assert_eq!(world.flow_field.route_count(), 0);
}

#[test]
fn an_engineer_finishes_its_move_before_a_queued_build_even_one_within_reach() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 100_000, 1_000_000);
    let (engineer_blueprint, extractor) = (id(&world, "t1_engineer"), id(&world, "mass_extractor"));
    let engineer = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_000, 1_000));
    // The deposit is right next to it, and the move goes the other way.
    world.deposits.push((51, 50));
    let goal = point(1_500, 1_000);
    world.advance_tick(&[SimCommand::Move { units: vec![engineer], goal }]);
    run(&mut world, 5);
    world.advance_tick(&[SimCommand::Build { units: vec![engineer], blueprint: extractor, cell_x: 51, cell_y: 50, queued: true }]);
    let cell = cell_of_index(&world, 51, 50);
    run(&mut world, 5);
    assert!(!world.occupancy.contains_key(&cell), "the build must wait for the move");
    assert_ne!(order_kind(&world, engineer), OrderKind::Idle, "it should still be walking");
    run_until_near(&mut world, engineer, goal, 12.0);
    assert!(!world.occupancy.contains_key(&cell), "the build must wait for the move");

    // Then it comes back and builds it.
    let mut ticks = 0;
    while !world.occupancy.contains_key(&cell) {
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 2_000, "it never started the extractor");
    }
    assert!(distance_to(&world, engineer, goal) > 100.0, "it should have walked back");
}

#[test]
fn an_engineer_can_mix_moves_and_builds_in_any_order() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 100_000, 1_000_000);
    let (engineer_blueprint, generator) = (id(&world, "t1_engineer"), id(&world, "power_generator"));
    let engineer = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_000, 1_000));
    let (out, back) = (point(1_400, 1_000), point(1_000, 1_400));
    world.advance_tick(&[
        SimCommand::QueueMove { units: vec![engineer], goal: out, attack: false },
        SimCommand::Build { units: vec![engineer], blueprint: generator, cell_x: 80, cell_y: 60, queued: true },
        SimCommand::QueueMove { units: vec![engineer], goal: back, attack: false },
    ]);
    let cell = cell_of_index(&world, 80, 60);
    let plan = world.snapshot_for(Some(Team::Blue));
    assert_eq!(plan.plans[0].steps.len(), 3, "the trail should show all three");
    assert_eq!(plan.plans[0].steps.iter().map(|step| step.kind).collect::<Vec<_>>(), [PlanStepKind::Move, PlanStepKind::Build, PlanStepKind::Move]);
    assert_eq!(plan.build_queues[0].tasks.len(), 3, "the panel should list all three");

    run_until_near(&mut world, engineer, out, 12.0);
    assert!(!world.occupancy.contains_key(&cell));
    let mut ticks = 0;
    while !world.occupancy.get(&cell).is_some_and(|site| is_finished(&world, *site)) {
        assert!(distance_to(&world, engineer, back) > 12.0, "it must build before walking on");
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 2_000, "it never finished the build");
    }
    run_until_near(&mut world, engineer, back, 12.0);
    run(&mut world, 5);
    assert!(tasks(&world, engineer).is_empty());
    assert_eq!(world.flow_field.route_count(), 0);
}

#[test]
fn a_new_order_or_stop_drops_the_queue_and_hands_its_routes_back() {
    let mut world = test_world();
    let (bot, engineer_blueprint) = (id(&world, "t1_light_walker"), id(&world, "t1_engineer"));
    let unit = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    let engineer = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_000, 1_100));
    let both = vec![unit, engineer];
    let queue = |goal: Vec2Fp| SimCommand::QueueMove { units: vec![unit, engineer], goal, attack: false };
    world.advance_tick(&[queue(point(1_500, 1_000)), queue(point(1_500, 1_500)), queue(point(1_000, 1_500))]);
    assert_eq!(world.flow_field.route_count(), 3);

    let goal = point(800, 1_000);
    world.advance_tick(&[SimCommand::Move { units: both.clone(), goal }]);
    assert!(world.order_queues.is_empty());
    assert!(tasks(&world, engineer).is_empty());
    assert_eq!(world.flow_field.route_count(), 1, "only the new order's route is left");

    world.advance_tick(&[queue(point(1_500, 1_000))]);
    world.advance_tick(&[SimCommand::Stop { units: both }]);
    assert_eq!(order_kind(&world, unit), OrderKind::Idle);
    assert_eq!(order_kind(&world, engineer), OrderKind::Idle);
    assert_eq!(world.flow_field.route_count(), 0);
}

#[test]
fn a_unit_that_dies_hands_its_queued_routes_back() {
    let mut world = test_world();
    let (bot, engineer_blueprint) = (id(&world, "t1_light_walker"), id(&world, "t1_engineer"));
    let unit = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    let engineer = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_000, 1_100));
    let queue = |goal: Vec2Fp| SimCommand::QueueMove { units: vec![unit, engineer], goal, attack: false };
    world.advance_tick(&[queue(point(4_000, 1_000)), queue(point(4_000, 2_000))]);
    assert_eq!(world.flow_field.route_count(), 2);
    let killer = id(&world, "command_rig");
    for doomed in [unit, engineer] {
        world.ecs.get::<&mut Health>(entity(&world, doomed)).unwrap().current = Fp::ONE;
    }
    // On their way, so that the light bot, at FAF's speed, cannot outrun it.
    world.spawn_mobile(killer, Team::Red, point(1_300, 1_050));
    run(&mut world, 150);
    assert!(world.unit_entity(unit).is_none() && world.unit_entity(engineer).is_none(), "the test needs them dead");
    assert!(world.order_queues.is_empty());
    assert_eq!(world.flow_field.route_count(), 0);
}

#[test]
fn a_queued_move_can_be_removed_from_the_panel_whether_waiting_or_under_way() {
    let mut world = test_world();
    let (bot, engineer_blueprint) = (id(&world, "t1_light_walker"), id(&world, "t1_engineer"));
    let unit = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    let engineer = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_000, 1_100));
    let both = vec![unit, engineer];
    let queue = |goal: Vec2Fp| SimCommand::QueueMove { units: vec![unit, engineer], goal, attack: false };
    world.advance_tick(&[SimCommand::Move { units: both.clone(), goal: point(1_500, 1_000) }, queue(point(1_500, 1_500)), queue(point(1_000, 1_500))]);
    run(&mut world, 3);

    let view = world.snapshot_for(Some(Team::Blue));
    for queue_view in &view.build_queues {
        assert_eq!(queue_view.tasks.len(), 3, "{:?} should list its walk and two more", queue_view.id);
    }
    assert_eq!(view.build_queues.len(), 2);

    // The middle one goes, for both of them, and the same order matches on each.
    let middle = view.build_queues[0].tasks[1];
    assert!(view.build_queues[1].tasks[1].is_same_order(&middle));
    world.advance_tick(&[SimCommand::CancelTask { units: both.clone(), task: middle }]);
    let view = world.snapshot_for(Some(Team::Blue));
    assert!(view.build_queues.iter().all(|queue| queue.tasks.len() == 2));
    assert_eq!(world.flow_field.route_count(), 2);

    // Removing the one under way moves on to what is left.
    let current = view.build_queues[0].tasks[0];
    world.advance_tick(&[SimCommand::CancelTask { units: both.clone(), task: current }]);
    run_until_near(&mut world, unit, point(1_000, 1_500), 40.0);
    run_until_near(&mut world, engineer, point(1_000, 1_500), 40.0);
}

#[test]
fn queueing_an_order_ends_a_patrol_and_only_builders_take_queued_tasks() {
    let mut world = test_world();
    let (bot, engineer_blueprint) = (id(&world, "t1_light_walker"), id(&world, "t1_engineer"));
    let unit = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    let engineer = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_000, 1_100));
    world.advance_tick(&[SimCommand::Patrol { units: vec![unit], points: vec![point(1_200, 1_000), point(1_000, 1_000)], queued: false }]);
    run(&mut world, 5);
    assert_eq!(world.patrols.len(), 1);
    let goal = point(1_500, 1_500);
    world.advance_tick(&[SimCommand::QueueMove { units: vec![unit], goal, attack: false }]);
    assert!(world.patrols.is_empty());
    run_until_near(&mut world, unit, goal, 12.0);

    world.advance_tick(&[SimCommand::QueueTask { units: vec![unit, engineer], task: BuildTask::Assist { target: unit } }]);
    assert_eq!(tasks(&world, engineer).len(), 1);
    assert!(world.ecs.get::<&Builder>(entity(&world, unit)).is_err());
}

#[test]
fn structures_in_a_queued_group_are_left_out() {
    let mut world = test_world();
    let (bot, factory_blueprint) = (id(&world, "t1_light_walker"), id(&world, "land_factory_t1"));
    let unit = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    let factory = world.spawn_finished_structure(factory_blueprint, Team::Blue, 80, 80).unwrap();
    world.advance_tick(&[SimCommand::QueueMove { units: vec![unit, factory], goal: point(1_300, 1_000), attack: false }]);
    assert_eq!(order_kind(&world, factory), OrderKind::Idle);
    assert_eq!(order_kind(&world, unit), OrderKind::Move);
    run(&mut world, 200);
    assert_eq!(world.flow_field.route_count(), 0, "a structure must not hold the route open");
}

fn stops_of(world: &SimWorld, unit: SimId) -> Vec<BuildTask> {
    world.snapshot_for(Some(Team::Blue)).order_stops.iter().filter(|stop| stop.unit == unit).map(|stop| stop.task).collect()
}

#[test]
fn a_move_under_way_can_be_dragged_somewhere_else() {
    let mut world = test_world();
    let bot = id(&world, "t1_light_walker");
    let unit = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[SimCommand::Move { units: vec![unit], goal: point(1_500, 1_000) }]);
    run(&mut world, 5);
    let stops = stops_of(&world, unit);
    assert_eq!(stops.len(), 1, "a plain move can be dragged too");
    let goal = point(1_000, 1_500);
    world.advance_tick(&[SimCommand::MoveOrder { units: vec![unit], task: stops[0], goal }]);
    assert_eq!(world.flow_field.route_count(), 1, "the old route should be handed back");
    run_until_near(&mut world, unit, goal, 12.0);
    run(&mut world, 10);
    assert_eq!(order_kind(&world, unit), OrderKind::Idle);
    assert_eq!(world.flow_field.route_count(), 0);
}

#[test]
fn a_queued_move_can_be_dragged_and_keeps_its_place_in_line() {
    let mut world = test_world();
    let (bot, engineer_blueprint) = (id(&world, "t1_light_walker"), id(&world, "t1_engineer"));
    let unit = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    let engineer = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_000, 1_100));
    let both = vec![unit, engineer];
    let first = point(1_400, 1_000);
    world.advance_tick(&[
        SimCommand::Move { units: both.clone(), goal: first },
        SimCommand::QueueMove { units: both.clone(), goal: point(4_000, 4_000), attack: false },
    ]);
    // The move under way, and the queued one.
    assert_eq!(world.flow_field.route_count(), 2);
    let queued = *stops_of(&world, unit).last().unwrap();
    let last = point(1_400, 1_400);
    world.advance_tick(&[SimCommand::MoveOrder { units: both.clone(), task: queued, goal: last }]);
    assert_eq!(world.flow_field.route_count(), 2, "one route swapped for another, shared by both");
    assert_eq!(stops_of(&world, unit).len(), 2);
    assert_eq!(stops_of(&world, engineer).len(), 2);

    run_until_near(&mut world, unit, first, 40.0);
    assert!(distance_to(&world, unit, last) > 200.0, "it must still go to the first stop first");
    run_until_near(&mut world, unit, last, 40.0);
    run_until_near(&mut world, engineer, last, 40.0);
    run(&mut world, 100);
    assert_eq!(world.flow_field.route_count(), 0);
}

#[test]
fn a_build_that_has_not_started_can_be_dragged_and_one_that_has_cannot() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 100_000, 1_000_000);
    let (engineer, orders) = engineer_with_three_builds(&mut world);
    run(&mut world, 3);

    // Moving the one it is walking to sends it to the new spot instead.
    world.advance_tick(&[SimCommand::MoveBuild { units: vec![engineer], task: orders[0], cell_x: 60, cell_y: 70 }]);
    let moved = tasks(&world, engineer)[0];
    assert!(matches!(moved, BuildTask::Build { cell_x: 60, cell_y: 70, .. }));
    assert_eq!(tasks(&world, engineer)[1..], orders[1..], "the rest keep their places");
    let new_cell = cell_of_index(&world, 60, 70);
    let mut ticks = 0;
    while !world.occupancy.contains_key(&new_cell) {
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 1_000, "it never started at the new spot");
    }
    assert!(!world.occupancy.contains_key(&cell_of_index(&world, 80, 50)), "nothing should be built at the old spot");

    // Now that it has started, it stays.
    let started = tasks(&world, engineer)[0];
    assert!(!stops_of(&world, engineer).iter().any(|task| task.is_same_order(&started)), "a started site has no handle");
    world.advance_tick(&[SimCommand::MoveBuild { units: vec![engineer], task: started, cell_x: 20, cell_y: 20 }]);
    assert!(matches!(tasks(&world, engineer)[0], BuildTask::Build { cell_x: 60, cell_y: 70, site: Some(_), .. }));
}

#[test]
fn moving_an_order_that_is_already_finished_does_nothing() {
    let mut world = test_world();
    let bot = id(&world, "t1_light_walker");
    let unit = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[SimCommand::Move { units: vec![unit], goal: point(1_100, 1_000) }]);
    let task = stops_of(&world, unit)[0];
    run(&mut world, 200);
    assert_eq!(order_kind(&world, unit), OrderKind::Idle);
    world.advance_tick(&[SimCommand::MoveOrder { units: vec![unit], task, goal: point(2_000, 2_000) }]);
    assert_eq!(order_kind(&world, unit), OrderKind::Idle);
    assert_eq!(world.flow_field.route_count(), 0, "no route should be left open");
}

#[test]
fn patrols_and_attacks_and_follow_orders_have_no_handles() {
    let mut world = test_world();
    let bot = id(&world, "t1_light_walker");
    let patroller = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    let attacker = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_100));
    let follower = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_200));
    let enemy = world.spawn_mobile(bot, Team::Red, point(3_000, 3_000));
    world.advance_tick(&[
        SimCommand::Patrol { units: vec![patroller], points: vec![point(1_200, 1_000), point(1_000, 1_000)], queued: false },
        SimCommand::Attack { units: vec![attacker], target: enemy },
        SimCommand::Follow { units: vec![follower], target: patroller },
    ]);
    run(&mut world, 3);
    for unit in [patroller, attacker, follower] {
        assert!(stops_of(&world, unit).is_empty(), "{unit:?} should have nothing to drag");
    }
    assert!(world.snapshot_for(Some(Team::Red)).order_stops.is_empty());
}

#[test]
fn rates_show_real_output_and_what_a_factory_really_spends() {
    let mut world = test_world();
    let (extractor, generator, factory_blueprint, bot) = (id(&world, "mass_extractor"), id(&world, "power_generator"), id(&world, "land_factory_t1"), id(&world, "t1_light_walker"));
    world.deposits.push((10, 10));
    let mine = world.spawn_finished_structure(extractor, Team::Blue, 10, 10).unwrap();
    let power = world.spawn_finished_structure(generator, Team::Blue, 30, 30).unwrap();
    let factory = world.spawn_finished_structure(factory_blueprint, Team::Blue, 50, 50).unwrap();
    set_resources(&mut world, Team::Blue, 100_000, 1_000_000);
    run(&mut world, 2);
    let rates_of = |world: &SimWorld, unit: SimId| *world.snapshot_for(Some(Team::Blue)).rates.iter().find(|rates| rates.id == unit).expect("should be listed");
    assert_eq!(rates_of(&world, mine).mass_income, 20, "2 materiel per second, in tenths");
    assert_eq!(rates_of(&world, power).energy_income, 200, "20 energy per second, in tenths");
    assert_eq!(rates_of(&world, factory).mass_use, 0, "an idle factory spends nothing");

    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: bot, count: 5 }]);
    run(&mut world, 2);
    let full = rates_of(&world, factory);
    assert!(full.mass_use > 0 && full.energy_use > 0, "{full:?}");

    // With nothing left, it spends only what comes in.
    set_resources(&mut world, Team::Blue, 0, 1_000_000);
    run(&mut world, 3);
    let stalled = rates_of(&world, factory);
    assert!(stalled.mass_use < full.mass_use, "{stalled:?} against {full:?}");
    assert!(stalled.mass_use <= 21, "it cannot spend more than the 2 materiel per second coming in: {stalled:?}");
    assert!(world.snapshot_for(Some(Team::Red)).rates.iter().all(|rates| rates.id != factory), "the enemy must not see this");
}

#[test]
fn a_structure_being_built_shows_no_output_yet() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 10_000, 100_000);
    let (_, site) = engineer_at_site(&mut world);
    assert!(world.snapshot_for(Some(Team::Blue)).rates.iter().all(|rates| rates.id != site));
    run(&mut world, 400);
    assert!(is_finished(&world, site));
    assert_eq!(world.snapshot_for(Some(Team::Blue)).rates.iter().find(|rates| rates.id == site).unwrap().energy_income, 200);
}

/// The whole path from an order to what the interface draws and checks against. Each side was
/// once tested with its own idea of the width's units, which hid the outlines in real play.
#[test]
fn a_queued_build_gives_a_full_size_outline_and_blocks_its_spot() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 100_000, 1_000_000);
    let (engineer_blueprint, factory, generator) = (id(&world, "t1_engineer"), id(&world, "land_factory_t1"), id(&world, "power_generator"));
    let engineer = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[
        SimCommand::Build { units: vec![engineer], blueprint: generator, cell_x: 150, cell_y: 150, queued: false },
        SimCommand::Build { units: vec![engineer], blueprint: factory, cell_x: 100, cell_y: 100, queued: true },
    ]);
    let current = world.snapshot_for(Some(Team::Blue));
    let pending = pending_builds(&current);
    assert_eq!(pending.len(), 2);
    let cell = f64::from(CELL);
    assert_eq!(pending[0].width, f64::from(world.blueprints.get(generator).footprint) * cell);
    assert_eq!(pending[1].width, f64::from(world.blueprints.get(factory).footprint) * cell, "the outline should be the size of the factory");
    let size = f64::from(world.blueprints.get(factory).footprint);
    assert_eq!(pending[1].center[..2], [(100.0 + size / 2.0) * cell, (100.0 + size / 2.0) * cell]);

    let deposits: Vec<(u32, u32)> = world.deposits.clone();
    let cells_per_side = world.terrain.cells_per_side() as u32;
    let map = PlacementMap { cells_per_side, blocked_cells: &[], deposits: &deposits, vents: &[], water_cells: &[], blueprints: &world.blueprints };
    assert!(!placement_is_valid(&map, generator, (101, 101), &current.units, &pending), "a queued factory must block its spot");
    assert!(placement_is_valid(&map, generator, (120, 120), &current.units, &pending));
}

#[test]
fn a_build_that_cannot_be_reached_is_dropped_and_the_next_one_built() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 100_000, 1_000_000);
    // A closed ring of walls, with room for a generator inside.
    for x in 120..=126 {
        world.terrain.set_structure_cells(x, 120, 1, true);
        world.terrain.set_structure_cells(x, 126, 1, true);
    }
    for y in 120..=126 {
        world.terrain.set_structure_cells(120, y, 1, true);
        world.terrain.set_structure_cells(126, y, 1, true);
    }
    world.flow_field.invalidate();
    let (engineer_blueprint, generator) = (id(&world, "t1_engineer"), id(&world, "power_generator"));
    let engineer = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[
        SimCommand::Build { units: vec![engineer], blueprint: generator, cell_x: 123, cell_y: 123, queued: false },
        SimCommand::Build { units: vec![engineer], blueprint: generator, cell_x: 60, cell_y: 50, queued: true },
    ]);
    let reachable = cell_of_index(&world, 60, 50);
    let mut most_routes = 0;
    let mut ticks = 0;
    while !world.occupancy.contains_key(&reachable) {
        most_routes = most_routes.max(world.flow_field.route_count());
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 1_500, "it never moved on to the build it can reach");
    }
    assert!(!world.occupancy.contains_key(&cell_of_index(&world, 123, 123)));
    assert!(most_routes <= 1, "it must not keep making new routes to the sealed spot");
    assert_eq!(world.snapshot_for(Some(Team::Blue)).orders_cancelled, 1);
}

#[test]
fn a_paused_factory_spends_nothing_and_keeps_its_queue_until_resumed() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 100_000, 1_000_000);
    let (factory_blueprint, bot) = (id(&world, "land_factory_t1"), id(&world, "t1_light_walker"));
    let factory = world.spawn_finished_structure(factory_blueprint, Team::Blue, 50, 50).unwrap();
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: bot, count: 3 }]);
    run(&mut world, 5);
    let progress_of = |world: &SimWorld| world.ecs.get::<&Factory>(entity(world, factory)).unwrap().progress;
    let under_way = progress_of(&world);
    assert!(under_way > Res::ZERO);

    world.advance_tick(&[SimCommand::SetPaused { units: vec![factory], paused: true }]);
    let (mass, held) = (world.economies[0].mass, progress_of(&world));
    run(&mut world, 50);
    assert_eq!(progress_of(&world), held, "a paused factory makes no progress");
    assert_eq!(world.economies[0].mass, mass, "a paused factory spends nothing");
    assert_eq!(world.ecs.get::<&Factory>(entity(&world, factory)).unwrap().queue.len(), 3, "it keeps its queue");
    assert_eq!(world.snapshot_for(Some(Team::Blue)).paused, [factory]);
    assert!(world.snapshot_for(Some(Team::Red)).paused.is_empty(), "the enemy does not see it");

    world.advance_tick(&[SimCommand::SetPaused { units: vec![factory], paused: false }]);
    run(&mut world, 5);
    assert!(progress_of(&world) > held || world.ecs.get::<&Factory>(entity(&world, factory)).unwrap().queue.len() < 3, "it carries on where it left off");
    assert!(world.snapshot_for(Some(Team::Blue)).paused.is_empty());
}

#[test]
fn a_paused_site_stops_and_its_builders_wait_beside_it() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 10_000, 100_000);
    let (engineer, site) = engineer_at_site(&mut world);
    run(&mut world, 20);
    world.advance_tick(&[SimCommand::SetPaused { units: vec![site], paused: true }]);
    let before = progress(&world, site);
    run(&mut world, 50);
    assert_eq!(progress(&world, site), before, "nobody may build a paused site");
    assert!(world.snapshot_for(Some(Team::Blue)).work_links.is_empty(), "no beam should show for work that is not happening");
    assert!(!tasks(&world, engineer).is_empty(), "the builder keeps its task, waiting");

    world.advance_tick(&[SimCommand::SetPaused { units: vec![site], paused: false }]);
    run(&mut world, 400);
    assert!(is_finished(&world, site));
}

#[test]
fn only_factories_and_sites_can_be_paused() {
    let mut world = test_world();
    let (generator, engineer_blueprint) = (id(&world, "power_generator"), id(&world, "t1_engineer"));
    let power = world.spawn_finished_structure(generator, Team::Blue, 50, 50).unwrap();
    let engineer = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_400, 1_400));
    world.advance_tick(&[SimCommand::SetPaused { units: vec![power, engineer], paused: true }]);
    assert!(world.snapshot_for(Some(Team::Blue)).paused.is_empty());
}

fn scenery_kind(world: &SimWorld, wreck: SimId) -> Option<sim_engine::sim::components::WreckKind> {
    let index = world.wrecks.binary_search_by_key(&wreck, |entry| entry.0).ok()?;
    world.ecs.get::<&sim_engine::sim::components::Wreck>(world.wrecks[index].1).ok().map(|wreck| wreck.kind)
}

#[test]
fn a_tree_gives_quick_energy_and_a_little_mass() {
    use sim_engine::sim::components::WreckKind;
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 1_000, 1_000);
    (world.economies[0].mass, world.economies[0].energy) = (Res::ZERO, Res::ZERO);
    let engineer = world.spawn_mobile(id(&world, "t1_engineer"), Team::Blue, point(1_000, 1_000));
    let tree = world.spawn_scenery(WreckKind::Tree, point(1_010, 1_000), Fp::ZERO);
    world.advance_tick(&[SimCommand::Reclaim { units: vec![engineer], target: tree }]);
    // 20 energy at 10 a second for each of the engineer's 5 build power: under half a second.
    run(&mut world, 10);
    assert_eq!(scenery_kind(&world, tree), None, "the tree should be gone");
    let (mass, energy) = (world.economies[0].mass.to_num::<f64>(), world.economies[0].energy.to_num::<f64>());
    assert!((0.99..=1.01).contains(&mass), "reclaimed {mass} materiel");
    assert!((19.9..=20.1).contains(&energy), "reclaimed {energy} energy");
}

#[test]
fn a_rock_gives_mass_by_its_size_and_no_energy() {
    use sim_engine::sim::components::WreckKind;
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 1_000, 1_000);
    (world.economies[0].mass, world.economies[0].energy) = (Res::ZERO, Res::ZERO);
    let engineer = world.spawn_mobile(id(&world, "t1_engineer"), Team::Blue, point(1_000, 1_000));
    let rock = world.spawn_scenery(WreckKind::Rock, point(1_015, 1_000), fp_int(8));
    world.advance_tick(&[SimCommand::Reclaim { units: vec![engineer], target: rock }]);
    // 5 mass for each unit of radius, so 40, at 10 a second.
    run(&mut world, 30);
    assert_eq!(scenery_kind(&world, rock), Some(WreckKind::Rock), "half eaten after 3 seconds");
    run(&mut world, 20);
    assert_eq!(scenery_kind(&world, rock), None, "gone after 5");
    let mass = world.economies[0].mass.to_num::<f64>();
    assert!((39.9..=40.1).contains(&mass), "reclaimed {mass} materiel");
    assert_eq!(world.economies[0].energy, Res::ZERO);
}

#[test]
fn building_over_trees_and_rocks_clears_them_for_what_they_held_but_leaves_debris() {
    use sim_engine::sim::components::{Wreck, WreckKind};
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 1_000, 1_000);
    (world.economies[0].mass, world.economies[0].energy) = (Res::from_num(500), Res::ZERO);
    // A generator is one cell, from (1000, 1000) to (1020, 1020).
    let tree = world.spawn_scenery(WreckKind::Tree, point(1_010, 1_010), Fp::ZERO);
    let rock = world.spawn_scenery(WreckKind::Rock, point(1_018, 1_002), fp_int(6));
    let outside = world.spawn_scenery(WreckKind::Tree, point(1_040, 1_010), Fp::ZERO);
    let debris_id = world.spawn_scenery(WreckKind::Rock, point(1_005, 1_015), fp_int(5));
    let index = world.wrecks.binary_search_by_key(&debris_id, |entry| entry.0).unwrap();
    world.ecs.get::<&mut Wreck>(world.wrecks[index].1).unwrap().kind = WreckKind::Debris;
    let generator = id(&world, "power_generator");
    assert!(world.spawn_site(generator, Team::Blue, 50, 50).is_some(), "trees and rocks do not stop a building");
    assert_eq!(scenery_kind(&world, tree), None);
    assert_eq!(scenery_kind(&world, rock), None);
    assert_eq!(scenery_kind(&world, outside), Some(WreckKind::Tree));
    assert_eq!(scenery_kind(&world, debris_id), Some(WreckKind::Debris), "debris has to be reclaimed");
    assert_eq!(world.economies[0].mass, Res::from_num(500 + 1 + 30));
    assert_eq!(world.economies[0].energy, Res::from_num(20));
}

#[test]
fn trees_and_rocks_show_through_fog_but_debris_does_not() {
    use sim_engine::sim::components::{Wreck, WreckKind};
    let mut world = scenario::skirmish(3).world;
    let hidden = Vec2Fp::new(fp_int(5_000), fp_int(9_000));
    let tree = world.spawn_scenery(WreckKind::Tree, hidden, Fp::ZERO);
    let debris = world.spawn_scenery(WreckKind::Rock, hidden + point(30, 0), fp_int(5));
    let index = world.wrecks.binary_search_by_key(&debris, |entry| entry.0).unwrap();
    world.ecs.get::<&mut Wreck>(world.wrecks[index].1).unwrap().kind = WreckKind::Debris;
    world.advance_tick(&[]);
    let seen: Vec<SimId> = world.snapshot_for(Some(Team::Blue)).wrecks.iter().map(|wreck| wreck.id).collect();
    assert!(seen.contains(&tree));
    assert!(!seen.contains(&debris));
}

#[test]
fn the_skirmish_map_has_mirrored_forests_and_rocks_clear_of_bases_and_deposits() {
    use sim_engine::sim::components::{Wreck, WreckKind};
    let world = scenario::skirmish(5).world;
    let map = fp_int(scenario::SKIRMISH_MAP_UNITS);
    let scenery: Vec<Wreck> = world.wrecks.iter().map(|(_, entity)| *world.ecs.get::<&Wreck>(*entity).unwrap()).collect();
    let trees = scenery.iter().filter(|wreck| wreck.kind == WreckKind::Tree).count();
    let rocks = scenery.iter().filter(|wreck| wreck.kind == WreckKind::Rock).count();
    assert!(trees >= 400 && rocks >= 50, "{trees} trees and {rocks} rocks");
    // Each comes with its mirror image straight after it.
    for pair in scenery.chunks(2) {
        let (first, second) = (pair[0], pair[1]);
        assert_eq!(second.position, Vec2Fp::new(map - first.position.x, map - first.position.y));
        assert_eq!((first.kind, first.mass, first.energy, first.radius), (second.kind, second.mass, second.energy, second.radius));
    }
    for wreck in &scenery {
        assert!(world.terrain.is_passable(wreck.position), "scenery in a wall at {:?}", wreck.position);
        for team in [Team::Blue, Team::Red] {
            let start = skirmish_start(team);
            let far = |a: Fp, b: Fp| (a - b).abs() > fp_int(20 * CELL);
            assert!(far(wreck.position.x, start.x) || far(wreck.position.y, start.y), "scenery in a base at {:?}", wreck.position);
        }
        let (x, y) = world.terrain.cell_of(wreck.position);
        for &(deposit_x, deposit_y) in &world.deposits {
            assert!((x as u32).abs_diff(deposit_x) > 2 || (y as u32).abs_diff(deposit_y) > 2, "scenery on a deposit at {:?}", wreck.position);
        }
    }
}

/// Structures sharing adjacency bonuses they use show as linked, for the cables drawn between
/// them: a generator beside a factory, which builds for less energy, but not beside a wall,
/// which has no use for it.
#[test]
fn adjacency_links_show_bonuses_that_are_used() {
    let mut world = test_world();
    let factory = world.spawn_finished_structure(id(&world, "land_factory_t1"), Team::Blue, 50, 50).unwrap();
    let generator = world.spawn_finished_structure(id(&world, "power_generator"), Team::Blue, 49, 50).unwrap();
    let wall = world.spawn_finished_structure(id(&world, "wall"), Team::Blue, 48, 50).unwrap();
    world.advance_tick(&[]);
    assert!(world.adjacency_links.contains(&(generator, factory)), "the generator gives the factory cheaper energy");
    assert!(!world.adjacency_links.iter().any(|&(_, receiver)| receiver == wall), "a wall uses nothing");
    assert_eq!(world.snapshot_for(Some(Team::Blue)).adjacency.len(), world.adjacency_links.len());
    assert!(world.snapshot_for(Some(Team::Red)).adjacency.is_empty(), "the enemy sees none of it");
}

#[test]
fn the_spending_breakdown_shares_each_job_by_build_power_and_adds_upkeep() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 100_000, 100_000);
    let (factory_blueprint, bot, engineer_blueprint) = (id(&world, "land_factory_t1"), id(&world, "t1_light_walker"), id(&world, "t1_engineer"));
    let factory = world.spawn_finished_structure(factory_blueprint, Team::Blue, 100, 100).unwrap();
    let helper = world.spawn_mobile(engineer_blueprint, Team::Blue, point(2_100, 2_100));
    let shield = world.spawn_finished_structure(id(&world, "shield_generator"), Team::Blue, 80, 80).unwrap();
    let (builder, _) = engineer_at_site(&mut world);
    // Mobile units pay upkeep too: once only structures' was listed.
    let mobile_shield = world.spawn_mobile(id(&world, "t2_mobile_shield"), Team::Blue, point(3_000, 1_000));
    let jammer = world.spawn_mobile(id(&world, "t2_combat_engineer"), Team::Blue, point(3_000, 1_200));
    // The other side's spending is none of the viewer's business.
    let red_factory = world.spawn_finished_structure(factory_blueprint, Team::Red, 150, 150).unwrap();
    world.advance_tick(&[
        SimCommand::QueueUnit { factory, blueprint: bot, count: 5 },
        SimCommand::QueueUnit { factory: red_factory, blueprint: bot, count: 5 },
        SimCommand::Assist { units: vec![helper], target: factory },
    ]);
    run(&mut world, 30);
    let snapshot = world.snapshot_for(Some(Team::Blue));
    let spent = |unit: SimId| snapshot.spending.iter().find(|spending| spending.id == unit).map(|spending| (spending.mass, spending.energy));
    assert_eq!(spent(red_factory), None);
    let (factory_mass, _) = spent(factory).expect("the factory is building");
    let (helper_mass, _) = spent(helper).expect("its helper is too");
    let power = |blueprint: BlueprintId| world.blueprints.get(blueprint).build_power.to_num::<i32>();
    assert_eq!(factory_mass * power(engineer_blueprint) / power(factory_blueprint), helper_mass, "each pays for its share of the power");
    assert!(spent(builder).is_some_and(|(mass, energy)| mass > 0 && energy > 0), "the engineer building on its own pays for all of its site");
    let upkeep = world.blueprints.get(id(&world, "shield_generator")).shield.as_ref().unwrap().upkeep;
    assert_eq!(spent(shield), Some((0, (upkeep * 10 * 10).round().to_num::<i32>())), "the dome pays its upkeep, and nothing else");
    for (unit, what) in [(mobile_shield, "a mobile shield"), (jammer, "a combat engineer, with its stealth")] {
        assert!(spent(unit).is_some_and(|(mass, energy)| mass == 0 && energy > 0), "{what} pays its upkeep: {:?}", spent(unit));
    }
    // Everything listed adds up to what the economy panel says goes out, give or take rounding.
    let (mass, energy) = snapshot.spending.iter().fold((0, 0), |(mass, energy), spending| (mass + spending.mass, energy + spending.energy));
    let close = |tenths: i32, whole: i64| (f64::from(tenths) / 10.0 - whole as f64).abs() <= 1.0;
    assert!(close(mass, snapshot.economy.mass_demand), "{mass} tenths against {}", snapshot.economy.mass_demand);
    assert!(close(energy, snapshot.economy.energy_demand), "{energy} tenths against {}", snapshot.economy.energy_demand);
}

#[test]
fn a_factory_is_idle_with_nothing_queued_and_nothing_to_help() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 100_000, 100_000);
    let (t1, bot) = (id(&world, "land_factory_t1"), id(&world, "t1_light_walker"));
    let busy = world.spawn_finished_structure(t1, Team::Blue, 50, 50).unwrap();
    let empty = world.spawn_finished_structure(t1, Team::Blue, 50, 70).unwrap();
    let helping = world.spawn_finished_structure(t1, Team::Blue, 50, 90).unwrap();
    let unfinished = world.spawn_site(t1, Team::Blue, 50, 110).unwrap();
    let launcher = world.spawn_finished_structure(id(&world, "tactical_missile_launcher"), Team::Blue, 80, 50).unwrap();
    let theirs = world.spawn_finished_structure(t1, Team::Red, 150, 150).unwrap();
    // The helper takes the next bot the busy one has not started, so there are three.
    world.advance_tick(&[SimCommand::QueueUnit { factory: busy, blueprint: bot, count: 3 }, SimCommand::Assist { units: vec![helping], target: busy }]);
    let idle = |world: &SimWorld| world.snapshot_for(Some(Team::Blue)).idle_factories;
    let shown = idle(&world);
    for (unit, expected, why) in [
        (busy, false, "a factory with a queue"),
        (empty, true, "a factory with an empty queue"),
        (helping, false, "a factory helping another"),
        (unfinished, false, "a factory still being built"),
        (launcher, false, "a missile launcher, which is ready with what it has"),
        (theirs, false, "the other side's factory"),
    ] {
        assert_eq!(shown.contains(&unit), expected, "{why}");
    }
    // Once its bots are out, the busy factory has nothing to do either.
    let mut ticks = 0;
    while !idle(&world).contains(&busy) {
        assert!(ticks < 3_000, "the queue should have run out by now");
        run(&mut world, 10);
        ticks += 10;
    }
}
