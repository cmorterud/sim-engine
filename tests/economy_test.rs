//! The vertical slice's rules: flow economy, construction, factories, reclaim, adjacency,
//! fog of war, and assassination (GAME_SPECIFICATION.md §2-9, §16).

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp, Vec3Fp};
use sim_engine::sim::blueprints::{BlueprintId, Res};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{BuildTask, Builder, Combat, Construction, Factory, Health, OrderKind, Projectile, SimId, Team};
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
    assert!((19.0..=20.5).contains(&economy.mass.to_num::<f64>()), "mass was {}", economy.mass);
    // 20 energy/s has nowhere to go, because no structure stores energy.
    assert_eq!(economy.energy, Res::ZERO);
    run(&mut world, 3_000);
    assert_eq!(world.economies[0].mass, Res::from_num(500), "mass must stop at capacity");
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
    assert!((74.0..=76.0).contains(&spent), "spent {spent} mass, but the cost is 75");
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
    assert_eq!(world.economies[0].mass, Res::from_num(10_000), "no mass should be spent during an energy stall");
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
    let bot = id(&world, "t1_light_bot");
    let factory = world.spawn_finished_structure(factory_blueprint, Team::Blue, 50, 50).unwrap();
    let count_bots = |world: &SimWorld| world.units.len() - 1;
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: bot, count: 2 }]);
    // 90 build time at 20 power is 4.5 s, or 45 ticks.
    run(&mut world, 40);
    assert_eq!(count_bots(&world), 0);
    run(&mut world, 10);
    assert_eq!(count_bots(&world), 1, "the first bot should be out by tick 50");
    run(&mut world, 60);
    assert_eq!(count_bots(&world), 2);
    run(&mut world, 60);
    assert_eq!(count_bots(&world), 2, "the queue should be empty");

    // With an assisting engineer, the same bot takes 90 / 25 = 3.6 s.
    let engineer_blueprint = id(&world, "t1_engineer");
    let helper = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_100, 1_100));
    let before = world.units.len();
    world.advance_tick(&[
        SimCommand::Assist { units: vec![helper], target: factory },
        SimCommand::QueueUnit { factory, blueprint: bot, count: 1 },
    ]);
    run(&mut world, 39);
    assert_eq!(world.units.len(), before + 1, "assist should finish the bot before 45 ticks");
}

#[test]
fn factory_repeat_loops_and_rally_sends_units() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 10_000, 100_000);
    let factory_blueprint = id(&world, "land_factory_t1");
    let bot = id(&world, "t1_light_bot");
    let factory = world.spawn_finished_structure(factory_blueprint, Team::Blue, 50, 50).unwrap();
    world.advance_tick(&[
        SimCommand::QueueUnit { factory, blueprint: bot, count: 1 },
        SimCommand::SetRepeat { factory, repeat: true },
        SimCommand::SetRally { factory, point: point(2_000, 1_000) },
    ]);
    run(&mut world, 200);
    let units: Vec<SimId> = world.units.iter().map(|unit| unit.0).filter(|unit| *unit != factory).collect();
    assert!(units.len() >= 4, "repeat should keep building, got {}", units.len());
    assert_eq!(world.unit_order(units[1]).unwrap().kind, OrderKind::Move, "new units should head to the rally point");
}

#[test]
fn upgrade_halts_production_then_improves_the_factory() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 10_000, 100_000);
    let (t1, t2, bot, tank) = (id(&world, "land_factory_t1"), id(&world, "land_factory_t2"), id(&world, "t1_light_bot"), id(&world, "t2_heavy_tank"));
    let factory = world.spawn_finished_structure(t1, Team::Blue, 50, 50).unwrap();
    // A T1 factory cannot build tanks, so this order is refused.
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: tank, count: 1 }]);
    assert!(world.ecs.get::<&Factory>(entity(&world, factory)).unwrap().queue.is_empty());

    world.advance_tick(&[
        SimCommand::QueueUnit { factory, blueprint: t2, count: 1 },
        SimCommand::QueueUnit { factory, blueprint: bot, count: 1 },
    ]);
    // 1,000 build time at 20 power is 50 s. The bot behind it must wait.
    run(&mut world, 400);
    assert_eq!(world.units.len(), 1, "production must halt during an upgrade");
    run(&mut world, 110);
    let power = world.ecs.get::<&Factory>(entity(&world, factory)).unwrap().power;
    assert_eq!(power, fp_int(40));
    assert_eq!(world.ecs.get::<&Health>(entity(&world, factory)).unwrap().max, fp_int(6_000));
    run(&mut world, 30);
    assert_eq!(world.units.len(), 2, "the queued bot should follow at the new build power");
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: tank, count: 1 }]);
    assert!(!world.ecs.get::<&Factory>(entity(&world, factory)).unwrap().queue.is_empty(), "a T2 factory can build tanks");
}

#[test]
fn an_upgrade_on_a_repeating_factory_happens_once_and_units_keep_looping() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 100_000, 1_000_000);
    let (t1, t2, bot) = (id(&world, "land_factory_t1"), id(&world, "land_factory_t2"), id(&world, "t1_light_bot"));
    let factory = world.spawn_finished_structure(t1, Team::Blue, 50, 50).unwrap();
    world.advance_tick(&[
        SimCommand::SetRepeat { factory, repeat: true },
        SimCommand::QueueUnit { factory, blueprint: t2, count: 1 },
        SimCommand::QueueUnit { factory, blueprint: bot, count: 1 },
    ]);
    run(&mut world, 700);
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
    let (t1, t2, bot, tank) = (id(&world, "land_factory_t1"), id(&world, "land_factory_t2"), id(&world, "t1_light_bot"), id(&world, "t2_heavy_tank"));
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

#[test]
fn adjacent_generators_discount_a_factorys_energy_but_not_mass() {
    let run_case = |generators: u32| {
        let mut world = test_world();
        set_resources(&mut world, Team::Blue, 100_000, 100_000);
        let (t1, generator, bot) = (id(&world, "land_factory_t1"), id(&world, "power_generator"), id(&world, "t1_light_bot"));
        let factory = world.spawn_finished_structure(t1, Team::Blue, 50, 50).unwrap();
        for index in 0..generators {
            world.spawn_finished_structure(generator, Team::Blue, 49, 50 + index).unwrap();
        }
        world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: bot, count: 1 }]);
        run(&mut world, 3);
        let economy = world.economies[0];
        (economy.mass_demand.to_num::<f64>(), economy.energy_demand.to_num::<f64>())
    };
    let (base_mass, base_energy) = run_case(0);
    let (mass, energy) = run_case(2);
    assert!((mass - base_mass).abs() < 1e-5, "mass should not change: {mass} against {base_mass}");
    assert!((energy / base_energy - 0.874).abs() < 0.002, "two generators should cut energy by about 12.6%: {}", energy / base_energy);
}

#[test]
fn structures_block_movement_and_units_walk_out_from_under_them() {
    let mut world = test_world();
    let (bot, factory) = (id(&world, "t1_light_bot"), id(&world, "land_factory_t1"));
    let unit = world.spawn_mobile(bot, Team::Blue, point(1_030, 1_030));
    world.spawn_finished_structure(factory, Team::Blue, 50, 50).unwrap();
    assert!(!world.terrain.is_passable(world.unit_position(unit).unwrap().xy()), "the setup should place the unit inside the factory");
    run(&mut world, 30);
    assert!(world.terrain.is_passable(world.unit_position(unit).unwrap().xy()), "the unit should have walked out");

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
    let enemy = id(&world, "t1_light_bot");
    world.spawn_mobile(enemy, Team::Red, point(1_010, 1_100));
    run(&mut world, 60);
    assert!(world.unit_position(structure).is_none(), "the structure should have been destroyed");
    assert!(world.terrain.cell_is_open(50, 50), "the ground should be open again");
    assert_eq!(world.wrecks.len(), 1);
    let wreck = *world.ecs.get::<&sim_engine::sim::components::Wreck>(world.wrecks[0].1).unwrap();
    assert_eq!(wreck.mass.to_num::<i64>(), 60, "81% of a 75-mass generator");
}

#[test]
fn engineers_reclaim_wrecks_into_mass() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 1_000, 1_000);
    world.economies[0].mass = Res::ZERO;
    let (engineer_blueprint, bot) = (id(&world, "t1_engineer"), id(&world, "t1_light_bot"));
    let doomed = world.spawn_mobile(bot, Team::Red, point(1_000, 1_000));
    let engineer = world.spawn_mobile(engineer_blueprint, Team::Blue, point(1_400, 1_000));
    world.ecs.get::<&mut Health>(entity(&world, doomed)).unwrap().current = Fp::ONE;
    let attacker = world.spawn_mobile(bot, Team::Blue, point(1_100, 1_000));
    run(&mut world, 40);
    assert!(world.unit_position(doomed).is_none());
    let wreck_id = world.wrecks[0].0;
    world.ecs.get::<&mut Combat>(entity(&world, attacker)).unwrap().range = Fp::ZERO;

    world.advance_tick(&[SimCommand::Reclaim { units: vec![engineer], target: wreck_id }]);
    // The wreck holds 81% of 36 mass, or 29.16. It takes time to walk there and 2.9 s to eat.
    run(&mut world, 400);
    assert!(world.wrecks.is_empty(), "the wreck should be gone");
    let mass = world.economies[0].mass.to_num::<f64>();
    assert!((29.0..=29.2).contains(&mass), "reclaimed {mass} mass");
    assert!(world.ecs.get::<&Builder>(entity(&world, engineer)).unwrap().tasks.is_empty());
}

#[test]
fn patrolling_engineers_reclaim_on_their_own_then_carry_on() {
    let mut world = test_world();
    let (engineer_blueprint, bot) = (id(&world, "t1_engineer"), id(&world, "t1_light_bot"));
    let doomed = world.spawn_mobile(bot, Team::Red, point(1_000, 1_000));
    world.ecs.get::<&mut Health>(entity(&world, doomed)).unwrap().current = Fp::ONE;
    let attacker = world.spawn_mobile(bot, Team::Blue, point(1_100, 1_000));
    run(&mut world, 40);
    world.ecs.get::<&mut Combat>(entity(&world, attacker)).unwrap().range = Fp::ZERO;
    assert_eq!(world.wrecks.len(), 1);
    set_resources(&mut world, Team::Blue, 0, 1_000);

    let engineer = world.spawn_mobile(engineer_blueprint, Team::Blue, point(900, 1_100));
    world.advance_tick(&[SimCommand::Patrol { units: vec![engineer], points: vec![point(800, 1_100), point(1_200, 1_100)] }]);
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
    let bot = id(&world, "t1_light_bot");
    let unit = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[SimCommand::Patrol { units: vec![unit], points: vec![point(1_100, 1_000), point(1_000, 1_000)] }]);
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
    let bot = id(&world, "t1_light_bot");
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
    let bot = id(&world, "t1_light_bot");
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
    assert_eq!(health(&world, front_rank), 200, "a shell must fly clear of friendly lines");

    // No longer safe: the same shell is a stray that hits the first ally in its path.
    shoot(&mut world, 0, Fp::ZERO);
    run(&mut world, 6);
    assert_eq!(health(&world, front_rank), 190, "a stray should hit an ally");

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
    assert_eq!(health(&world, bystander), 190, "a blast should hurt allies");
}

#[test]
fn fog_hides_enemies_from_targeting_and_the_players_view() {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: true, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    let (artillery, bot) = (id(&world, "t1_light_artillery"), id(&world, "t1_light_bot"));
    world.spawn_mobile(artillery, Team::Blue, point(1_000, 1_000));
    let enemy = world.spawn_mobile(bot, Team::Red, point(1_560, 1_000));
    world.ecs.get::<&mut Combat>(entity(&world, enemy)).unwrap().range = Fp::ZERO;

    // Range 600 reaches the enemy, but vision 500 does not.
    run(&mut world, 30);
    assert_eq!(world.unit_health(enemy), Some(fp_int(200)), "a unit must not fire at what its team cannot see");
    let view = world.snapshot_for(Some(Team::Blue));
    assert_eq!(view.units.len(), 1, "the player must not see the enemy");
    assert!(world.snapshot_for(Some(Team::Red)).units.iter().any(|unit| unit.id == enemy), "a team sees its own units");

    // A scout brings the enemy into sight, and the artillery opens fire.
    let scout = world.spawn_mobile(bot, Team::Blue, point(1_200, 1_000));
    world.ecs.get::<&mut Combat>(entity(&world, scout)).unwrap().range = Fp::ZERO;
    run(&mut world, 120);
    assert!(world.unit_health(enemy).unwrap() < fp_int(200), "the shot should land once there is sight");
    assert!(world.snapshot_for(Some(Team::Blue)).units.iter().any(|unit| unit.id == enemy));
}

fn commander_with_neighbors() -> (SimWorld, SimId, SimId, SimId, SimId) {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    let (acu, bot, generator) = (id(&world, "acu"), id(&world, "t1_light_bot"), id(&world, "power_generator"));
    let blue = world.spawn_mobile(acu, Team::Blue, point(1_000, 1_000));
    let red = world.spawn_mobile(acu, Team::Red, point(4_000, 4_000));
    let nearby_enemy = world.spawn_mobile(bot, Team::Red, point(1_150, 1_000));
    let blue_base = world.spawn_finished_structure(generator, Team::Blue, 150, 150).unwrap();
    world.ecs.get::<&mut Combat>(entity(&world, nearby_enemy)).unwrap().range = Fp::ZERO;
    (world, blue, red, nearby_enemy, blue_base)
}

#[test]
fn commander_death_detonates_and_eliminates_its_team() {
    let (mut world, blue, red, nearby_enemy, blue_base) = commander_with_neighbors();
    world.ecs.get::<&mut Health>(entity(&world, blue)).unwrap().current = Fp::ONE;
    let shooter = world.spawn_mobile(id(&world, "t1_light_bot"), Team::Red, point(1_100, 1_100));
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
    assert_eq!(plan.steps[1].pending_width, CELL, "the second has not");

    // The three builds take 25, 50, and 25 seconds of work, plus the walks between them.
    run(&mut world, 1_400);
    assert!(plan_kinds(&world, engineer).is_empty(), "a finished queue leaves nothing to show");
}

#[test]
fn plans_show_moves_attacks_patrols_and_support() {
    let mut world = test_world();
    set_resources(&mut world, Team::Blue, 10_000, 100_000);
    let (bot, engineer_blueprint, generator) = (id(&world, "t1_light_bot"), id(&world, "t1_engineer"), id(&world, "power_generator"));
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
        SimCommand::Patrol { units: vec![patroller], points: vec![point(1_300, 1_200), point(1_500, 1_200)] },
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
    let bot = id(&world, "t1_light_bot");
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
    let acu = id(&world, "acu");
    let blue = world.spawn_mobile(acu, Team::Blue, point(1_000, 1_000));
    let red = world.spawn_mobile(acu, Team::Red, point(1_200, 1_000));
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
    let bot = id(&world, "t1_light_bot");
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
    check(&mut world, "stored mass", &|w| w.economies[0].mass += tiny, &|w| w.economies[0].mass -= tiny);
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
