//! Phase 3 collision tests: swept hits, terrain masking, dodging, and friendly fire.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp, Vec3Fp};
use sim_engine::math::rect::RectFp;
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::combat::firing_solution;
use sim_engine::sim::components::{Combat, Health, OverchargeOrder, Position, Projectile, SimId, Team, Turning, Turret, Veterancy};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::scenario::{self, standard_unit};
use sim_engine::sim::world::SimWorld;

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

fn flat_world(friendly_fire: bool) -> SimWorld {
    SimWorld::new(
        SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, friendly_fire, ..SimConfig::default() },
        1,
    )
}

/// Spawns a unit that does not shoot back, so tests only see the projectile under test.
fn passive_unit(world: &mut SimWorld, team: Team, at: Vec2Fp) -> SimId {
    let id = world.spawn_unit(standard_unit(team, at));
    let entity = world.unit_entity(id).unwrap();
    world.ecs.get::<&mut Combat>(entity).unwrap().range = Fp::ZERO;
    id
}

fn shoot(world: &mut SimWorld, from: Vec2Fp, to: Vec2Fp, speed: Fp, source: SimId, team: Team) {
    let z = |w: &SimWorld, p: Vec2Fp| w.terrain.height_at(p) + fp_int(1);
    let start = Vec3Fp::new(from.x, from.y, z(world, from));
    let end = Vec3Fp::new(to.x, to.y, z(world, to));
    let solution = firing_solution(start, end, speed);
    world.spawn_projectile(
        start,
        Projectile {
            velocity: solution.velocity,
            damage: fp_int(10),
            source,
            source_team: team,
            lifetime_ticks: solution.flight_ticks + 20,
            splash_radius: Fp::ZERO,
            allied_safe_ticks: 0,
        },
    );
}

fn run_until_projectiles_gone(world: &mut SimWorld, limit: u32) {
    for _ in 0..limit {
        world.advance_tick(&[]);
        if world.projectiles.is_empty() {
            return;
        }
    }
    panic!("projectiles still alive after {limit} ticks");
}

#[test]
fn fast_shell_cannot_pass_through_a_unit_between_ticks() {
    let mut world = flat_world(false);
    let source = passive_unit(&mut world, Team::Blue, point(100, 100));
    let target = passive_unit(&mut world, Team::Red, point(225, 100));
    // 50 units per tick against a 2-unit-wide target: endpoint checks would land at
    // 150, 200, and 250, all far from the target at 225.
    shoot(&mut world, point(100, 100), point(400, 100), fp_int(50), source, Team::Blue);
    run_until_projectiles_gone(&mut world, 40);
    assert_eq!(world.unit_health(target), Some(fp_int(90)));
}

#[test]
fn wall_blocks_a_low_shot() {
    let mut world = flat_world(false);
    let source = passive_unit(&mut world, Team::Blue, point(100, 100));
    let target = passive_unit(&mut world, Team::Red, point(200, 100));
    world.add_box_obstacle(RectFp::new(point(140, 60), point(160, 140)));
    shoot(&mut world, point(100, 100), point(200, 100), fp_int(20), source, Team::Blue);
    run_until_projectiles_gone(&mut world, 40);
    assert_eq!(world.unit_health(target), Some(fp_int(100)));
}

#[test]
fn high_arc_clears_the_same_wall() {
    let mut world = flat_world(false);
    let source = passive_unit(&mut world, Team::Blue, point(100, 100));
    let target = passive_unit(&mut world, Team::Red, point(200, 100));
    world.add_box_obstacle(RectFp::new(point(140, 60), point(160, 140)));
    // A slow shell flies for 200 ticks, which puts the apex well above the 12-unit wall.
    shoot(&mut world, point(100, 100), point(200, 100), Fp::lit("0.5"), source, Team::Blue);
    run_until_projectiles_gone(&mut world, 400);
    assert_eq!(world.unit_health(target), Some(fp_int(90)));
}

#[test]
fn moving_unit_dodges_a_slow_shell() {
    let mut world = flat_world(false);
    let source = passive_unit(&mut world, Team::Blue, point(100, 100));
    let target = passive_unit(&mut world, Team::Red, point(200, 100));
    shoot(&mut world, point(100, 100), point(200, 100), Fp::lit("1"), source, Team::Blue);
    world.advance_tick(&[SimCommand::Move { units: vec![target], goal: point(200, 400) }]);
    run_until_projectiles_gone(&mut world, 300);
    assert_eq!(world.unit_health(target), Some(fp_int(100)));
}

#[test]
fn stationary_unit_is_hit_by_the_same_slow_shell() {
    let mut world = flat_world(false);
    let source = passive_unit(&mut world, Team::Blue, point(100, 100));
    let target = passive_unit(&mut world, Team::Red, point(200, 100));
    shoot(&mut world, point(100, 100), point(200, 100), Fp::lit("1"), source, Team::Blue);
    run_until_projectiles_gone(&mut world, 300);
    assert_eq!(world.unit_health(target), Some(fp_int(90)));
}

#[test]
fn friendly_fire_setting_controls_allied_hits() {
    for (friendly_fire, expected) in [(false, 100), (true, 90)] {
        let mut world = flat_world(friendly_fire);
        let source = passive_unit(&mut world, Team::Blue, point(100, 100));
        let ally = passive_unit(&mut world, Team::Blue, point(150, 100));
        shoot(&mut world, point(100, 100), point(150, 100), fp_int(5), source, Team::Blue);
        run_until_projectiles_gone(&mut world, 60);
        assert_eq!(world.unit_health(ally), Some(fp_int(expected)), "friendly_fire = {friendly_fire}");
        assert_eq!(world.unit_health(source), Some(fp_int(100)), "the firing unit must never hit itself");
    }
}

#[test]
fn units_acquire_the_nearest_enemy_and_kill_it() {
    let mut world = flat_world(false);
    let shooter = world.spawn_unit(standard_unit(Team::Blue, point(100, 100)));
    let near = passive_unit(&mut world, Team::Red, point(115, 100));
    let far = passive_unit(&mut world, Team::Red, point(125, 100));
    for _ in 0..600 {
        world.advance_tick(&[]);
        if world.unit_position(near).is_none() {
            break;
        }
    }
    assert!(world.unit_position(near).is_none(), "the nearest enemy should have died");
    assert!(world.unit_position(far).is_some());
    assert_eq!(world.unit_health(far), Some(fp_int(100)));
    assert!(world.unit_position(shooter).is_some());
}

/// A shooter with an enemy in range on the far side of a wall too high for its shells.
fn enemy_behind_a_wall() -> (SimWorld, SimId, SimId) {
    let mut world = flat_world(false);
    // One terrain cell thick, with the target just in range on the far side.
    world.add_box_obstacle(RectFp::new(point(120, 60), point(140, 140)));
    world.prepare_map();
    let shooter = world.spawn_unit(standard_unit(Team::Blue, point(112, 100)));
    let target = passive_unit(&mut world, Team::Red, point(141, 100));
    (world, shooter, target)
}

#[test]
fn a_unit_does_not_shell_a_wall_in_front_of_its_target() {
    let (mut world, shooter, target) = enemy_behind_a_wall();
    for _ in 0..100 {
        world.advance_tick(&[]);
        assert!(world.projectiles.is_empty(), "the wall would stop every shell");
    }
    assert_eq!(world.unit_health(target), Some(fp_int(100)));
    let combat = *world.ecs.get::<&Combat>(world.unit_entity(shooter).unwrap()).unwrap();
    assert_eq!(combat.blocked.map(|(blocked, _)| blocked), Some(target));
}

#[test]
fn an_attack_move_walks_on_past_a_target_it_cannot_hit() {
    // It used to stop for the target in range and stay there for good.
    let (mut world, shooter, _) = enemy_behind_a_wall();
    let goal = point(110, 300);
    world.advance_tick(&[SimCommand::AttackMove { units: vec![shooter], goal }]);
    for _ in 0..400 {
        world.advance_tick(&[]);
    }
    let position = world.unit_position(shooter).unwrap();
    let (dx, dy) = ((position.x - goal.x).to_num::<i32>(), (position.y - goal.y).to_num::<i32>());
    assert!(dx.abs() < 20 && dy.abs() < 20, "stuck at {position:?}");
}

#[test]
fn an_ordered_attack_fires_even_into_a_wall() {
    // The player chose this target, so the shells go where they were told.
    let (mut world, shooter, target) = enemy_behind_a_wall();
    world.advance_tick(&[SimCommand::Attack { units: vec![shooter], target }]);
    let mut fired = false;
    for _ in 0..60 {
        world.advance_tick(&[]);
        fired |= !world.projectiles.is_empty();
    }
    assert!(fired);
}

#[test]
fn projectile_leaving_the_map_is_removed() {
    let mut world = flat_world(false);
    let source = passive_unit(&mut world, Team::Blue, point(100, 100));
    let start = Vec3Fp::new(fp_int(5_100), fp_int(100), fp_int(50));
    let velocity = Vec3Fp::new(fp_int(30), Fp::ZERO, fp_int(5));
    world.spawn_projectile(start, Projectile { velocity, damage: fp_int(1), source, source_team: Team::Blue, lifetime_ticks: 1_000, splash_radius: Fp::ZERO, allied_safe_ticks: 0 });
    world.advance_tick(&[]);
    world.advance_tick(&[]);
    assert!(world.projectiles.is_empty());
}

#[test]
fn battle_on_a_maximum_size_map_runs_to_casualties() {
    let scenario = scenario::battle(3, 200, SimConfig::default());
    let opening = scenario.opening_commands;
    let mut world = scenario.world;
    world.advance_tick(&opening);
    for _ in 0..600 {
        world.advance_tick(&[]);
    }
    assert!(world.units.len() < 400);
    for &(_, entity) in world.units.iter().take(50) {
        let z = world.ecs.get::<&Position>(entity).unwrap().0.z;
        assert!(z.abs() < fp_int(200), "unit height {z} looks wrong");
    }
}

/// A heavy tank facing east with a Red bot behind it, both from the real data.
fn tank_with_enemy_behind() -> (SimWorld, SimId, SimId) {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    let (tank, bot) = (world.blueprints.id("t2_heavy_tank"), world.blueprints.id("t1_light_bot"));
    let shooter = world.spawn_mobile(tank, Team::Blue, point(1_000, 1_000));
    let east = Vec2Fp::new(Fp::ONE, Fp::ZERO);
    let entity = world.unit_entity(shooter).unwrap();
    world.ecs.get::<&mut Turret>(entity).unwrap().facing = east;
    let target = world.spawn_mobile(bot, Team::Red, point(700, 1_000));
    // The bot must not shoot back, so that only the tank's aim is measured.
    world.ecs.get::<&mut Combat>(world.unit_entity(target).unwrap()).unwrap().range = Fp::ZERO;
    (world, shooter, target)
}

#[test]
fn a_turret_must_swivel_round_before_it_can_fire() {
    let (mut world, shooter, _) = tank_with_enemy_behind();
    let entity = world.unit_entity(shooter).unwrap();
    let mut first_shot = None;
    for tick in 0..60 {
        world.advance_tick(&[]);
        if !world.projectiles.is_empty() {
            first_shot = Some(tick);
            break;
        }
    }
    // 60 degrees a second is 6 degrees a tick, so half a turn takes about 30 ticks, plus finding the target.
    let tick = first_shot.expect("it should fire once it has come round");
    assert!(tick >= 25, "it fired after only {tick} ticks, before it could have turned");
    let facing = world.ecs.get::<&Turret>(entity).unwrap().facing;
    assert!(facing.x < Fp::from_num(-0.99), "it should be aiming back at the bot, not {facing:?}");
}

#[test]
fn a_turret_without_a_target_turns_back_to_the_front() {
    let (mut world, shooter, target) = tank_with_enemy_behind();
    run_ticks(&mut world, 40);
    let entity = world.unit_entity(shooter).unwrap();
    assert!(world.ecs.get::<&Turret>(entity).unwrap().facing.x < Fp::ZERO);
    // The bot leaves, and the tank's turret goes back over its hull.
    let far = world.unit_entity(target).unwrap();
    world.ecs.get::<&mut Position>(far).unwrap().0 = Vec3Fp::new(fp_int(4_000), fp_int(4_000), Fp::ZERO);
    run_ticks(&mut world, 60);
    let turret = world.ecs.get::<&Turret>(entity).unwrap().facing;
    let body = world.ecs.get::<&Turning>(entity).unwrap().facing;
    assert!(turret.x * body.x + turret.y * body.y > Fp::from_num(0.99), "turret {turret:?}, hull {body:?}");
}

fn run_ticks(world: &mut SimWorld, ticks: u32) {
    for _ in 0..ticks {
        world.advance_tick(&[]);
    }
}

/// A Blue commander with plenty of energy, facing a Red unit in range that does not shoot back.
fn commander_facing(enemy: &str, energy: i64) -> (SimWorld, SimId, SimId) {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    let (acu, enemy_blueprint) = (world.blueprints.id("acu"), world.blueprints.id(enemy));
    let commander = world.spawn_mobile(acu, Team::Blue, point(1_000, 1_000));
    // Its normal gun is out of the picture, so that only Overcharge does damage.
    world.ecs.get::<&mut Combat>(world.unit_entity(commander).unwrap()).unwrap().projectile_damage = Fp::ZERO;
    let target = if world.blueprints.get(enemy_blueprint).is_structure() {
        world.spawn_finished_structure(enemy_blueprint, Team::Red, 60, 50).unwrap()
    } else {
        world.spawn_mobile(enemy_blueprint, Team::Red, point(1_200, 1_000))
    };
    world.ecs.get::<&mut Combat>(world.unit_entity(target).unwrap()).unwrap().range = Fp::ZERO;
    let economy = &mut world.economies[0];
    economy.energy_capacity = Res::from_num(10_000);
    economy.base_energy_capacity = Res::from_num(10_000);
    economy.energy = Res::from_num(energy);
    (world, commander, target)
}

#[test]
fn overcharge_destroys_a_heavy_tank_in_one_shot_and_costs_its_energy() {
    let (mut world, commander, target) = commander_facing("t2_heavy_tank", 9_000);
    world.advance_tick(&[SimCommand::Overcharge { unit: commander, target }]);
    // The commander also earns a little energy every tick, so look at the tick it fires.
    let mut biggest_drop = 0.0_f64;
    for _ in 0..120 {
        let before = world.economies[0].energy.to_num::<f64>();
        world.advance_tick(&[]);
        biggest_drop = biggest_drop.max(before - world.economies[0].energy.to_num::<f64>());
        if world.unit_entity(target).is_none() {
            break;
        }
    }
    assert!(world.unit_entity(target).is_none(), "one shot should destroy a heavy tank");
    assert!((2_990.0..=3_000.0).contains(&biggest_drop), "it should cost 3,000 energy, not {biggest_drop}");
    assert!(world.ecs.get::<&OverchargeOrder>(world.unit_entity(commander).unwrap()).is_err(), "the order is done");
}

#[test]
fn overcharge_waits_for_the_energy_and_then_fires() {
    let (mut world, commander, target) = commander_facing("t2_heavy_tank", 1_000);
    world.advance_tick(&[SimCommand::Overcharge { unit: commander, target }]);
    run_ticks(&mut world, 60);
    assert!(world.unit_entity(target).is_some(), "it cannot fire without the energy");
    assert!(world.ecs.get::<&OverchargeOrder>(world.unit_entity(commander).unwrap()).is_ok(), "it keeps waiting");
    world.economies[0].energy = Res::from_num(4_000);
    run_ticks(&mut world, 60);
    assert!(world.unit_entity(target).is_none(), "it fires once the energy is there");
}

#[test]
fn overcharge_does_little_to_a_structure() {
    let (mut world, commander, target) = commander_facing("t1_point_defense", 9_000);
    let full = world.ecs.get::<&Health>(world.unit_entity(target).unwrap()).unwrap().current;
    world.advance_tick(&[SimCommand::Overcharge { unit: commander, target }]);
    let mut lost = Fp::ZERO;
    for _ in 0..120 {
        world.advance_tick(&[]);
        lost = full - world.ecs.get::<&Health>(world.unit_entity(target).unwrap()).unwrap().current;
        if lost > Fp::ZERO {
            break;
        }
    }
    assert_eq!(lost, fp_int(800), "a structure takes at most 800 from one shot");
}

#[test]
fn overcharge_is_refused_at_a_friend_and_ends_when_the_target_dies() {
    let (mut world, commander, target) = commander_facing("t2_heavy_tank", 9_000);
    let bot = world.blueprints.id("t1_light_bot");
    let friend = world.spawn_mobile(bot, Team::Blue, point(1_100, 1_000));
    world.advance_tick(&[SimCommand::Overcharge { unit: commander, target: friend }]);
    assert!(world.ecs.get::<&OverchargeOrder>(world.unit_entity(commander).unwrap()).is_err(), "a friend is not a target");

    world.economies[0].energy = Res::ZERO;
    world.advance_tick(&[SimCommand::Overcharge { unit: commander, target }]);
    world.ecs.get::<&mut Health>(world.unit_entity(target).unwrap()).unwrap().current = Fp::ZERO;
    world.ecs.get::<&mut Health>(world.unit_entity(target).unwrap()).unwrap().current = Fp::ONE;
    let enemy_killer = world.spawn_mobile(world.blueprints.id("acu"), Team::Blue, point(1_250, 1_000));
    let _ = enemy_killer;
    run_ticks(&mut world, 80);
    assert!(world.unit_entity(target).is_none(), "the test needs the target dead");
    assert!(world.ecs.get::<&OverchargeOrder>(world.unit_entity(commander).unwrap()).is_err(), "there is nothing left to Overcharge");
}

/// A Blue bot and a weakened Red bot right beside it, from the real data.
fn bot_duel() -> (SimWorld, SimId, SimId) {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    let bot = world.blueprints.id("t1_light_bot");
    let hunter = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    let prey = world.spawn_mobile(bot, Team::Red, point(1_100, 1_000));
    world.ecs.get::<&mut Combat>(world.unit_entity(prey).unwrap()).unwrap().range = Fp::ZERO;
    (world, hunter, prey)
}

fn veterancy_of(world: &SimWorld, unit: SimId) -> Option<Veterancy> {
    world.ecs.get::<&Veterancy>(world.unit_entity(unit).unwrap()).ok().map(|veterancy| *veterancy)
}

#[test]
fn destroying_an_enemy_earns_its_mass_and_a_rank() {
    let (mut world, hunter, prey) = bot_duel();
    let base = world.ecs.get::<&Health>(world.unit_entity(hunter).unwrap()).unwrap().max;
    // Hurt it first, so that the heal on promotion shows.
    world.ecs.get::<&mut Health>(world.unit_entity(hunter).unwrap()).unwrap().current = base / 2;
    let mut ticks = 0;
    while world.unit_entity(prey).is_some() {
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 400, "the prey never died");
    }
    // One light bot destroying another of the same cost is one times its own mass.
    let veterancy = veterancy_of(&world, hunter).expect("it should have experience");
    assert_eq!(veterancy.experience, world.blueprints.get(world.blueprints.id("t1_light_bot")).mass);
    assert_eq!(veterancy.rank, 1);
    let health = *world.ecs.get::<&Health>(world.unit_entity(hunter).unwrap()).unwrap();
    assert_eq!(health.max, base * 6 / 5, "rank 1 adds a fifth of its base health");
    assert!(health.current > base / 2 + base / 5, "promotion should heal it");
    assert_eq!(world.snapshot_for(Some(Team::Blue)).veterans, [(hunter, 1)]);
}

#[test]
fn veterans_heal_over_time_and_ranks_stop_at_five() {
    let (mut world, hunter, _) = bot_duel();
    let mass = world.blueprints.get(world.blueprints.id("t1_light_bot")).mass;
    let tank = world.blueprints.get(world.blueprints.id("t2_heavy_tank")).mass;
    // Destroying ten heavy tanks' worth is far past rank 5.
    let entity = world.unit_entity(hunter).unwrap();
    let _ = entity;
    let prey: Vec<SimId> = (0..10).map(|index| world.spawn_mobile(world.blueprints.id("t2_heavy_tank"), Team::Red, point(1_150 + index * 20, 1_200))).collect();
    for target in &prey {
        let target_entity = world.unit_entity(*target).unwrap();
        world.ecs.get::<&mut Combat>(target_entity).unwrap().range = Fp::ZERO;
        world.ecs.get::<&mut Health>(target_entity).unwrap().current = Fp::ONE;
    }
    run_ticks(&mut world, 300);
    let veterancy = veterancy_of(&world, hunter).expect("it should have experience");
    assert!(veterancy.experience >= tank * 5, "it should have destroyed several tanks");
    assert_eq!(veterancy.rank, 5, "rank stops at 5");
    assert!(veterancy.experience >= mass * 5);
    let entity = world.unit_entity(hunter).unwrap();
    let max = world.ecs.get::<&Health>(entity).unwrap().max;
    world.ecs.get::<&mut Health>(entity).unwrap().current = max / 2;
    run_ticks(&mut world, 10);
    let healed = world.ecs.get::<&Health>(entity).unwrap().current - max / 2;
    // Rank 5 heals 1% of maximum health a second.
    assert!(healed >= max / 110 && healed <= max / 90, "healed {healed} of {max} in a second");
}

#[test]
fn nothing_is_earned_for_destroying_your_own_side() {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: false, friendly_fire: true, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    let artillery = world.blueprints.id("t1_light_artillery");
    let bot = world.blueprints.id("t1_light_bot");
    let gunner = world.spawn_mobile(artillery, Team::Blue, point(1_000, 1_000));
    let enemy = world.spawn_mobile(bot, Team::Red, point(1_400, 1_000));
    world.ecs.get::<&mut Combat>(world.unit_entity(enemy).unwrap()).unwrap().range = Fp::ZERO;
    // A friend standing beside the target, so close to dying that splash will finish it.
    let friend = world.spawn_mobile(bot, Team::Blue, point(1_410, 1_000));
    world.ecs.get::<&mut Health>(world.unit_entity(friend).unwrap()).unwrap().current = Fp::ONE;
    world.ecs.get::<&mut Health>(world.unit_entity(enemy).unwrap()).unwrap().current = fp_int(100_000);
    run_ticks(&mut world, 200);
    assert!(world.unit_entity(friend).is_none(), "the test needs the friend destroyed by splash");
    assert_eq!(veterancy_of(&world, gunner), None, "destroying a friend earns nothing");
}

#[test]
fn units_without_a_blueprint_earn_nothing() {
    let mut world = flat_world(false);
    let shooter = world.spawn_unit(standard_unit(Team::Blue, point(1_000, 1_000)));
    let victim = world.spawn_unit(standard_unit(Team::Red, point(1_020, 1_000)));
    world.ecs.get::<&mut Health>(world.unit_entity(victim).unwrap()).unwrap().current = Fp::ONE;
    run_ticks(&mut world, 100);
    assert!(world.unit_entity(victim).is_none());
    assert_eq!(veterancy_of(&world, shooter), None);
    assert!(world.veterans.is_empty());
}
