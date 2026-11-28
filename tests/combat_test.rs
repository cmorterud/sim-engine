//! Phase 3 collision tests: swept hits, terrain masking, dodging, and friendly fire.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp, Vec3Fp};
use sim_engine::math::rect::RectFp;
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::combat::firing_solution;
use sim_engine::sim::components::{Combat, Health, DischargeOrder, Position, Projectile, SimId, Team, Turning, Turret, Veterancy};
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
    // One terrain cell thick, raised twice, well above any arc the shooter's slow shells fly,
    // with the target just in range on the far side.
    let wall = RectFp::new(point(120, 60), point(140, 140));
    world.add_box_obstacle(wall);
    world.add_box_obstacle(wall);
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
/// A shot flies on past the map's edge, as aircraft do (the design owner, 2026-11-27), and is
/// removed at the outer margin, half the map's width past it.
fn projectile_leaving_the_map_is_removed() {
    let mut world = flat_world(false);
    let source = passive_unit(&mut world, Team::Blue, point(100, 100));
    let start = Vec3Fp::new(fp_int(5_100), fp_int(100), fp_int(50));
    let velocity = Vec3Fp::new(fp_int(100), Fp::ZERO, fp_int(5));
    world.spawn_projectile(start, Projectile { velocity, damage: fp_int(1), source, source_team: Team::Blue, lifetime_ticks: 1_000, splash_radius: Fp::ZERO, allied_safe_ticks: 0 });
    world.advance_tick(&[]);
    world.advance_tick(&[]);
    assert_eq!(world.projectiles.len(), 1, "past the edge it flies on");
    // From 5,100 at 100 a tick it passes 5,120 + 2,560 within 27 ticks.
    for _ in 0..30 {
        world.advance_tick(&[]);
    }
    assert!(world.projectiles.is_empty(), "and is gone past the outer margin");
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
    let (tank, bot) = (world.blueprints.id("t2_heavy_tank"), world.blueprints.id("t1_light_walker"));
    let shooter = world.spawn_mobile(tank, Team::Blue, point(1_000, 1_000));
    let east = Vec2Fp::new(Fp::ONE, Fp::ZERO);
    let entity = world.unit_entity(shooter).unwrap();
    world.ecs.get::<&mut Turret>(entity).unwrap().facing = east;
    let target = world.spawn_mobile(bot, Team::Red, point(700, 1_000));
    // The bot must not shoot back, so that only the tank's aim is measured, nor die, with
    // The inspiration's 60 health, before the turret has turned.
    world.ecs.get::<&mut Combat>(world.unit_entity(target).unwrap()).unwrap().range = Fp::ZERO;
    {
        let mut health = world.ecs.get::<&mut Health>(world.unit_entity(target).unwrap()).unwrap();
        health.max = fp_int(100_000);
        health.current = health.max;
    }
    (world, shooter, target)
}

#[test]
fn a_turret_must_swivel_round_before_it_can_fire() {
    let (mut world, shooter, _) = tank_with_enemy_behind();
    let entity = world.unit_entity(shooter).unwrap();
    // The turret rule turns the heavy tank's gun fast enough to follow anything alongside
    // (balance::turret_follow), round in three ticks, so this one is slowed to 60 degrees a
    // second to watch it swivel.
    world.ecs.get::<&mut Turret>(entity).unwrap().max_turn = Turret::new(Vec2Fp::new(Fp::ONE, Fp::ZERO), 60).max_turn;
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

/// A tank driving across a gun's line of fire at full speed used to walk out from under every
/// shell, which aimed where it was. Shells now aim where it will be (§7.1, Leading Shots).
#[test]
fn shells_lead_a_target_driving_across_the_line_of_fire() {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    let (tank, target_kind) = (world.blueprints.id("t2_heavy_tank"), world.blueprints.id("t1_medium_tank"));
    let shooter = world.spawn_mobile(tank, Team::Blue, point(1_500, 1_000));
    let target = world.spawn_mobile(target_kind, Team::Red, point(1_000, 1_350));
    let entity = world.unit_entity(target).unwrap();
    world.ecs.get::<&mut Combat>(entity).unwrap().range = Fp::ZERO;
    {
        let mut health = world.ecs.get::<&mut Health>(entity).unwrap();
        health.max = fp_int(100_000);
        health.current = health.max;
    }
    // Across the tank's front, 350 away at its nearest, at full speed.
    world.advance_tick(&[SimCommand::Move { units: vec![target], goal: point(2_200, 1_350) }]);
    let damage = world.ecs.get::<&Combat>(world.unit_entity(shooter).unwrap()).unwrap().projectile_damage;
    let mut shots = 0;
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..120 {
        world.advance_tick(&[]);
        for &(id, entity) in &world.projectiles {
            if world.ecs.get::<&Projectile>(entity).is_ok_and(|projectile| projectile.source == shooter) && seen.insert(id) {
                shots += 1;
            }
        }
    }
    run_ticks(&mut world, 20);
    let hits = (fp_int(100_000) - world.unit_health(target).unwrap()) / damage;
    assert!(shots >= 4, "it should have fired several times, not {shots}");
    assert!(hits * 4 >= Fp::from_num(shots * 3), "most shells should hit a target moving steadily: {hits} of {shots}");
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
    let (rig, enemy_blueprint) = (world.blueprints.id("command_rig"), world.blueprints.id(enemy));
    let commander = world.spawn_mobile(rig, Team::Blue, point(1_000, 1_000));
    // Its normal gun is out of the picture, so that only Discharge does damage.
    world.ecs.get::<&mut Combat>(world.unit_entity(commander).unwrap()).unwrap().projectile_damage = Fp::ZERO;
    let target = if world.blueprints.get(enemy_blueprint).is_structure() {
        world.spawn_finished_structure(enemy_blueprint, Team::Red, 60, 50).unwrap()
    } else {
        world.spawn_mobile(enemy_blueprint, Team::Red, point(1_200, 1_000))
    };
    world.ecs.get::<&mut Combat>(world.unit_entity(target).unwrap()).unwrap().range = Fp::ZERO;
    let economy = &mut world.economies[0];
    economy.energy_capacity = Res::from_num(200_000);
    economy.base_energy_capacity = Res::from_num(200_000);
    economy.energy = Res::from_num(energy);
    (world, commander, target)
}

/// Fires one Discharge at the target, and returns the most energy that left the store in one
/// tick, which is the tick it landed, and the damage the target took.
fn discharge_once(world: &mut SimWorld, commander: SimId, target: SimId) -> (f64, Fp) {
    let full = world.ecs.get::<&Health>(world.unit_entity(target).unwrap()).unwrap().current;
    world.advance_tick(&[SimCommand::Discharge { unit: commander, target }]);
    let mut biggest_drop = 0.0_f64;
    for _ in 0..120 {
        let before = world.economies[0].energy.to_num::<f64>();
        world.advance_tick(&[]);
        biggest_drop = biggest_drop.max(before - world.economies[0].energy.to_num::<f64>());
        if biggest_drop > 100.0 {
            break;
        }
    }
    let left = world.unit_entity(target).map_or(Fp::ZERO, |entity| world.ecs.get::<&Health>(entity).unwrap().current);
    (biggest_drop, full - left)
}

#[test]
fn discharge_spends_only_what_destroys_its_target_and_at_least_its_least() {
    // A shot spends the heavy tank's health in damage, at the Discharge's energy a point.
    let (mut world, commander, target) = commander_facing("t2_heavy_tank", 100_000);
    let health = world.blueprints.get(world.blueprints.id("t2_heavy_tank")).health.to_num::<f64>();
    let per_damage = world.blueprints.get(world.blueprints.id("command_rig")).discharge.expect("the commander has a Discharge").energy_per_damage;
    let cost = health * f64::from(per_damage);
    let (drop, _) = discharge_once(&mut world, commander, target);
    assert!(world.unit_entity(target).is_none(), "one shot should destroy a heavy tank");
    assert!((cost - 100.0..=cost).contains(&drop), "it spends what destroys it, {cost} energy, not {drop}");
    // A medium tank has 300 health, under the least a shot does, 1,250, which costs 7,500.
    let (mut world, commander, target) = commander_facing("t1_medium_tank", 100_000);
    let (drop, _) = discharge_once(&mut world, commander, target);
    assert!(world.unit_entity(target).is_none());
    assert!((7_400.0..=7_500.0).contains(&drop), "the least shot costs 7,500 energy, not {drop}");
    assert!(world.ecs.get::<&DischargeOrder>(world.unit_entity(commander).unwrap()).is_err(), "the order is done");
}

#[test]
fn discharge_does_what_the_store_can_pay_for_against_something_tough() {
    // Tougher than 1,350: with 9,000 in store, 90% of it pays for 1,350 damage, all of which it spends.
    let (mut world, commander, target) = commander_facing("t3_siege_walker", 9_000);
    let health = world.blueprints.get(world.blueprints.id("t3_siege_walker")).health.to_num::<f64>();
    let per_damage = f64::from(world.blueprints.get(world.blueprints.id("command_rig")).discharge.expect("the commander has a Discharge").energy_per_damage);
    assert!(health > 1_350.0, "the siege walker outlasts what 9,000 energy pays for");
    let (drop, dealt) = discharge_once(&mut world, commander, target);
    assert!((dealt.to_num::<f64>() - 1_350.0).abs() < 2.0, "it does 1,350, not {dealt}");
    assert!((drop - 8_100.0).abs() < 20.0, "and spends 8,100, not {drop}");
    // With plenty in store, it spends what destroys it: its health in damage.
    let (mut world, commander, target) = commander_facing("t3_siege_walker", 100_000);
    let (drop, _) = discharge_once(&mut world, commander, target);
    assert!(world.unit_entity(target).is_none(), "destroyed");
    assert!((drop - health * per_damage).abs() < 50.0, "for {} energy, not {drop}", health * per_damage);
}

#[test]
fn discharge_does_little_to_a_commander() {
    let (mut world, commander, target) = commander_facing("command_rig", 100_000);
    let (_, dealt) = discharge_once(&mut world, commander, target);
    assert_eq!(dealt, fp_int(400), "a commander takes 400");
}

#[test]
fn discharge_waits_for_the_energy_and_then_fires() {
    let (mut world, commander, target) = commander_facing("t2_heavy_tank", 1_000);
    world.advance_tick(&[SimCommand::Discharge { unit: commander, target }]);
    run_ticks(&mut world, 60);
    assert!(world.unit_entity(target).is_some(), "it cannot fire without the energy");
    assert!(world.ecs.get::<&DischargeOrder>(world.unit_entity(commander).unwrap()).is_ok(), "it keeps waiting");
    world.economies[0].energy = Res::from_num(4_000);
    run_ticks(&mut world, 60);
    assert!(world.unit_entity(target).is_some(), "it needs 7,500 in store, more than a commander alone holds");
    // 10,000 pays for the 1,500 a heavy tank takes.
    world.economies[0].energy = Res::from_num(10_000);
    run_ticks(&mut world, 60);
    assert!(world.unit_entity(target).is_none(), "it fires once the energy is there");
}

#[test]
fn discharge_does_little_to_a_structure() {
    let (mut world, commander, target) = commander_facing("t1_point_defense", 9_000);
    let full = world.ecs.get::<&Health>(world.unit_entity(target).unwrap()).unwrap().current;
    world.advance_tick(&[SimCommand::Discharge { unit: commander, target }]);
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
fn discharge_is_refused_at_a_friend_and_ends_when_the_target_dies() {
    let (mut world, commander, target) = commander_facing("t2_heavy_tank", 9_000);
    let bot = world.blueprints.id("t1_light_walker");
    let friend = world.spawn_mobile(bot, Team::Blue, point(1_100, 1_000));
    world.advance_tick(&[SimCommand::Discharge { unit: commander, target: friend }]);
    assert!(world.ecs.get::<&DischargeOrder>(world.unit_entity(commander).unwrap()).is_err(), "a friend is not a target");

    world.economies[0].energy = Res::ZERO;
    world.advance_tick(&[SimCommand::Discharge { unit: commander, target }]);
    world.ecs.get::<&mut Health>(world.unit_entity(target).unwrap()).unwrap().current = Fp::ZERO;
    world.ecs.get::<&mut Health>(world.unit_entity(target).unwrap()).unwrap().current = Fp::ONE;
    let enemy_killer = world.spawn_mobile(world.blueprints.id("command_rig"), Team::Blue, point(1_250, 1_000));
    let _ = enemy_killer;
    run_ticks(&mut world, 80);
    assert!(world.unit_entity(target).is_none(), "the test needs the target dead");
    assert!(world.ecs.get::<&DischargeOrder>(world.unit_entity(commander).unwrap()).is_err(), "there is nothing left to Discharge");
}

/// A Blue bot and a weakened Red bot right beside it, from the real data.
fn bot_duel() -> (SimWorld, SimId, SimId) {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    let bot = world.blueprints.id("t1_light_walker");
    let hunter = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    let prey = world.spawn_mobile(bot, Team::Red, point(1_100, 1_000));
    world.ecs.get::<&mut Combat>(world.unit_entity(prey).unwrap()).unwrap().range = Fp::ZERO;
    (world, hunter, prey)
}

fn veterancy_of(world: &SimWorld, unit: SimId) -> Option<Veterancy> {
    world.ecs.get::<&Veterancy>(world.unit_entity(unit).unwrap()).ok().map(|veterancy| *veterancy)
}

#[test]
fn destroying_twice_its_own_mass_earns_a_rank() {
    let (mut world, hunter, prey) = bot_duel();
    let mass = world.blueprints.get(world.blueprints.id("t1_light_walker")).mass;
    let base = world.ecs.get::<&Health>(world.unit_entity(hunter).unwrap()).unwrap().max;
    let kill = |world: &mut SimWorld, prey: SimId| {
        let mut ticks = 0;
        while world.unit_entity(prey).is_some() {
            world.advance_tick(&[]);
            ticks += 1;
            assert!(ticks < 400, "the prey never died");
        }
    };
    // The inspiration's veterancy: a T1 unit's first rank takes twice its own mass destroyed.
    kill(&mut world, prey);
    let veterancy = veterancy_of(&world, hunter).expect("it should have experience");
    assert_eq!((veterancy.experience, veterancy.rank), (mass, 0), "one light bot is half a rank");
    // Hurt it, so that the heal on promotion shows.
    world.ecs.get::<&mut Health>(world.unit_entity(hunter).unwrap()).unwrap().current = base / 2;
    let second = world.spawn_mobile(world.blueprints.id("t1_light_walker"), Team::Red, point(1_100, 1_000));
    world.ecs.get::<&mut Combat>(world.unit_entity(second).unwrap()).unwrap().range = Fp::ZERO;
    kill(&mut world, second);
    let veterancy = veterancy_of(&world, hunter).expect("it should have experience");
    assert_eq!((veterancy.experience, veterancy.rank), (mass * 2, 1));
    let health = *world.ecs.get::<&Health>(world.unit_entity(hunter).unwrap()).unwrap();
    assert_eq!(health.max, base * 11 / 10, "rank 1 adds a tenth of its base health");
    assert!(health.current >= base / 2 + base / 10 + base / 10, "promotion heals a tenth of its base health, on top of what it gained");
    assert_eq!(world.snapshot_for(Some(Team::Blue)).veterans, [(hunter, 1)]);
}

#[test]
fn experience_is_shared_by_the_damage_each_attacker_did() {
    let (mut world, hunter, prey) = bot_duel();
    let bot = world.blueprints.id("t1_light_walker");
    let mass = world.blueprints.get(bot).mass;
    let helper = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_100));
    // The helper has done three quarters of the damage; the hunter will do the rest.
    let entity = world.unit_entity(prey).unwrap();
    let max = world.ecs.get::<&Health>(entity).unwrap().max;
    world.ecs.get::<&mut Combat>(world.unit_entity(helper).unwrap()).unwrap().range = Fp::ZERO;
    world.ecs.insert_one(entity, sim_engine::sim::components::VetDamage { by: vec![(helper, max * 3)], total: max * 3 }).unwrap();
    world.ecs.get::<&mut Health>(entity).unwrap().current = Fp::ONE;
    let mut ticks = 0;
    while world.unit_entity(prey).is_some() {
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 400, "the prey never died");
    }
    let helper_share = veterancy_of(&world, helper).expect("the helper did most of the damage").experience;
    let hunter_share = veterancy_of(&world, hunter).expect("the hunter finished it").experience;
    assert!(helper_share > hunter_share * 10, "the helper did far more: {helper_share} against {hunter_share}");
    assert!((helper_share + hunter_share - mass).abs() < Res::from_num(0.01), "the shares add up to its materiel");
}

#[test]
fn veterans_heal_over_time_and_ranks_stop_at_five() {
    let (mut world, hunter, _) = bot_duel();
    let mass = world.blueprints.get(world.blueprints.id("t1_light_walker")).mass;
    // Destroying ten heavy tanks is far past rank 5, though each kill earns one rank at most.
    let prey: Vec<SimId> = (0..10).map(|index| world.spawn_mobile(world.blueprints.id("t2_heavy_tank"), Team::Red, point(1_050 + index * 15, 1_150))).collect();
    for target in &prey {
        let target_entity = world.unit_entity(*target).unwrap();
        world.ecs.get::<&mut Combat>(target_entity).unwrap().range = Fp::ZERO;
        world.ecs.get::<&mut Health>(target_entity).unwrap().current = Fp::ONE;
    }
    run_ticks(&mut world, 300);
    let veterancy = veterancy_of(&world, hunter).expect("it should have experience");
    assert_eq!(veterancy.rank, 5, "rank stops at 5");
    // The duel's light bot, half a rank, then a rank a tank, capped at each rank's width of
    // twice its own mass, and nothing past rank 5.
    assert_eq!(veterancy.experience, mass + mass * 10);
    let entity = world.unit_entity(hunter).unwrap();
    let max = world.ecs.get::<&Health>(entity).unwrap().max;
    world.ecs.get::<&mut Health>(entity).unwrap().current = max / 2;
    run_ticks(&mut world, 10);
    let healed = world.ecs.get::<&Health>(entity).unwrap().current - max / 2;
    // The inspiration's regeneration for a T1 unit: 1 a second for each rank.
    assert_eq!(healed, fp_int(5), "healed {healed} in a second");
}

#[test]
fn ships_regenerate_and_rank_up_by_their_own_tables() {
    let world = flat_map(false);
    let battleship = world.blueprints.get(world.blueprints.id("t3_battleship"));
    let commander = world.blueprints.get(world.blueprints.id("command_rig"));
    // The Summit's card: a level for 1.25 times its own mass destroyed (12,500 for its 10,000,
    // read from its mass, which balance passes change), and 9 a second of regeneration a level.
    assert_eq!(sim_engine::sim::world::vet_threshold(battleship, 1), battleship.mass * 5 / 4);
    assert_eq!(sim_engine::sim::world::vet_threshold(battleship, 3), battleship.mass * 15 / 4);
    assert_eq!(sim_engine::sim::world::vet_regen_per_second(battleship, 1), 9);
    // The commander has its own steps, and regenerates as T3 does.
    assert_eq!(sim_engine::sim::world::vet_threshold(commander, 1), Res::from_num(1_000));
    assert_eq!(sim_engine::sim::world::vet_threshold(commander, 5), Res::from_num(10_000));
    assert_eq!(sim_engine::sim::world::vet_regen_per_second(commander, 2), 12);
}

#[test]
fn nothing_is_earned_for_destroying_your_own_side() {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: false, friendly_fire: true, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    let artillery = world.blueprints.id("t1_light_artillery");
    let bot = world.blueprints.id("t1_light_walker");
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

fn flat_map(fog_of_war: bool) -> SimWorld {
    SimWorld::new(SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war, ..SimConfig::default() }, 1)
}

/// A Blue unit of this kind at (1,000, 1,000), facing east.
fn spawn_gunner(world: &mut SimWorld, kind: &str) -> SimId {
    let blueprint = world.blueprints.id(kind);
    let id = world.spawn_mobile(blueprint, Team::Blue, point(1_000, 1_000));
    if let Ok(mut turret) = world.ecs.get::<&mut Turret>(world.unit_entity(id).unwrap()) {
        turret.facing = Vec2Fp::new(Fp::ONE, Fp::ZERO);
    }
    id
}

/// Leaves the debris of a Red unit at a spot, and returns the wreck. A shell from a unit off to
/// the side finishes it, so nothing else is disturbed.
fn debris_at(world: &mut SimWorld, kind: &str, at: Vec2Fp) -> SimId {
    let blueprint = world.blueprints.id(kind);
    let victim = world.spawn_mobile(blueprint, Team::Red, at);
    let entity = world.unit_entity(victim).unwrap();
    world.ecs.get::<&mut Combat>(entity).unwrap().range = Fp::ZERO;
    world.ecs.get::<&mut Health>(entity).unwrap().current = Fp::from_bits(1);
    let from = Vec2Fp::new(at.x, at.y + fp_int(60));
    let source = passive_unit(world, Team::Blue, from);
    shoot(world, from, at, fp_int(20), source, Team::Blue);
    run_until_projectiles_gone(world, 40);
    assert!(world.unit_entity(victim).is_none(), "the victim should be destroyed");
    world.ecs.despawn(world.unit_entity(source).unwrap()).unwrap();
    world.units.retain(|unit| unit.0 != source);
    world.wrecks.last().expect("it left debris").0
}

fn wreck(world: &SimWorld, id: SimId) -> Option<sim_engine::sim::components::Wreck> {
    let index = world.wrecks.binary_search_by_key(&id, |wreck| wreck.0).ok()?;
    world.ecs.get::<&sim_engine::sim::components::Wreck>(world.wrecks[index].1).ok().map(|wreck| *wreck)
}

#[test]
fn a_ground_attack_destroys_debris_and_the_mass_in_it() {
    let mut world = flat_map(false);
    let debris = debris_at(&mut world, "t2_heavy_tank", point(1_300, 1_000));
    let tank = spawn_gunner(&mut world, "t2_heavy_tank");
    let start = wreck(&world, debris).unwrap();
    // The wreck of a T2 unit on land: 90% of the heavy tank's health, and 90% of its mass less a
    // fifth for its tech level, 72% in all.
    let heavy = world.blueprints.get(world.blueprints.id("t2_heavy_tank")).clone();
    assert_eq!(start.health, Fp::from_num(Res::from_num(heavy.health) * 900 / 1000));
    assert_eq!(start.mass, heavy.mass * 900 / 1000 * 800 / 1000);
    let mass_before = world.economies[0].mass;
    world.advance_tick(&[SimCommand::AttackGround { units: vec![tank], point: start.position }]);

    // Each shell takes away the same share of the mass as of the health.
    let mut first_hit = None;
    let mut gone_at = None;
    for tick in 0..400 {
        world.advance_tick(&[]);
        match wreck(&world, debris) {
            Some(now) if first_hit.is_none() && now.health < start.health => first_hit = Some(now),
            None => {
                gone_at = Some(tick);
                break;
            }
            _ => {}
        }
    }
    let hit = first_hit.expect("a shell should land on the debris");
    let gun = heavy.weapon.expect("the tank has a gun");
    assert_eq!(hit.health, start.health - gun.damage);
    let expected = start.mass * Res::from_num(hit.health) / Res::from_num(start.health);
    assert!((hit.mass - expected).abs() < Res::from_num(0.001), "{} left, not {expected}", hit.mass);
    // As many shells as its health takes, one a reload, after the first has flown.
    let shells = (start.health / gun.damage).ceil().to_num::<u32>();
    let gone_at = gone_at.expect("the debris should be destroyed");
    assert!(gone_at < shells * gun.reload_ticks + 30, "it took {gone_at} ticks");
    assert_eq!(world.economies[0].mass_reclaimed, Res::ZERO);
    assert!(world.economies[0].mass <= mass_before + Res::from_num(1), "nobody got the materiel");

    // It keeps firing at the spot until given another order.
    let fired = |world: &mut SimWorld, ticks: u32| {
        let mut fired = false;
        for _ in 0..ticks {
            world.advance_tick(&[]);
            fired |= world.projectiles.iter().any(|&(_, entity)| world.ecs.get::<&Projectile>(entity).unwrap().source == tank);
        }
        fired
    };
    assert!(fired(&mut world, 30), "it should still be firing at the ground");
    world.advance_tick(&[SimCommand::Stop { units: vec![tank] }]);
    assert_eq!(world.ecs.get::<&Combat>(world.unit_entity(tank).unwrap()).unwrap().ground_target, None);
    run_until_projectiles_gone(&mut world, 40);
    assert!(!fired(&mut world, 30), "Stop should end the ground attack");
}

#[test]
fn artillery_blasts_all_the_debris_it_lands_among_but_not_trees_or_rocks() {
    use sim_engine::sim::components::WreckKind;
    let mut world = flat_map(false);
    let spot = point(1_400, 1_000);
    let near = [debris_at(&mut world, "t1_light_walker", spot), debris_at(&mut world, "t1_light_walker", point(1_425, 1_010))];
    let far = debris_at(&mut world, "t1_light_walker", point(1_500, 1_000));
    let far_mass = wreck(&world, far).unwrap().mass;
    let tree = world.spawn_scenery(WreckKind::Tree, point(1_405, 995), Fp::ZERO);
    let rock = world.spawn_scenery(WreckKind::Rock, point(1_395, 1_008), fp_int(8));
    let (tree_before, rock_before) = (wreck(&world, tree).unwrap(), wreck(&world, rock).unwrap());
    let artillery = spawn_gunner(&mut world, "t1_light_artillery");

    world.advance_tick(&[SimCommand::AttackGround { units: vec![artillery], point: spot }]);
    // A bot's debris has 100 health, so two 90-damage blasts finish it.
    run_ticks(&mut world, 200);
    for id in near {
        assert!(wreck(&world, id).is_none(), "debris inside the blast should be gone");
    }
    assert_eq!(wreck(&world, far).unwrap().mass, far_mass, "debris outside the blast is untouched");
    let (tree_after, rock_after) = (wreck(&world, tree).unwrap(), wreck(&world, rock).unwrap());
    assert_eq!((tree_after.mass, tree_after.energy), (tree_before.mass, tree_before.energy));
    assert_eq!(rock_after.mass, rock_before.mass);
}

#[test]
fn a_ground_attack_walks_into_range_and_fires_into_the_fog_at_nothing_else() {
    let mut world = flat_map(true);
    let bot = spawn_gunner(&mut world, "t1_light_walker");
    // An enemy right behind, which it would otherwise turn and shoot.
    let enemy = passive_unit(&mut world, Team::Red, point(900, 1_000));
    let enemy_health = world.unit_health(enemy).unwrap();
    let spot = point(2_000, 1_000);
    world.advance_tick(&[SimCommand::AttackGround { units: vec![bot], point: spot }]);
    assert!(!world.visibility.is_visible(Team::Blue, spot.x.to_bits(), spot.y.to_bits()), "the spot should start out of sight");

    // The player's own target shows in the plan, even out of sight.
    let plan = world.snapshot_for(Some(Team::Blue)).plans.into_iter().find(|plan| plan.id == bot).expect("a plan");
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.steps[0].kind, sim_engine::sim::world::PlanStepKind::AttackGround);

    let mut shots = 0;
    for _ in 0..400 {
        world.advance_tick(&[]);
        shots += world.projectiles.iter().filter(|&&(_, entity)| world.ecs.get::<&Projectile>(entity).unwrap().source == bot).count();
    }
    let here = world.unit_position(bot).unwrap().xy();
    let distance = (here.x - spot.x).to_num::<f64>().hypot((here.y - spot.y).to_num::<f64>());
    // It stops once the spot is in its range instead of walking on to it.
    let range = world.blueprints.get(world.blueprints.id("t1_light_walker")).weapon.unwrap().range.to_num::<f64>();
    assert!((range - 40.0..=range).contains(&distance), "it stopped {distance} from the spot, with a range of {range}");
    assert!(shots > 0, "it should fire at the spot");
    assert_eq!(world.unit_health(enemy), Some(enemy_health), "it fires at the spot and nothing else");
}

#[test]
fn ground_attack_shells_land_on_the_smallest_debris_at_any_range() {
    // Artillery cannot fire within its least range, half its reach.
    for (gunner, distances) in [("t2_heavy_tank", [60, 250, 455]), ("t1_light_artillery", [320, 450, 595])] {
        for distance in distances {
            let mut world = flat_map(false);
            let debris = debris_at(&mut world, "t1_light_walker", point(1_000 + distance, 1_000));
            let unit = spawn_gunner(&mut world, gunner);
            let spot = wreck(&world, debris).unwrap().position;
            world.advance_tick(&[SimCommand::AttackGround { units: vec![unit], point: spot }]);
            let mut hit = false;
            for _ in 0..80 {
                world.advance_tick(&[]);
                if wreck(&world, debris).is_none_or(|now| now.health < fp_int(100)) {
                    hit = true;
                    break;
                }
            }
            assert!(hit, "{gunner} missed debris {distance} away");
        }
    }
}

/// A light bot on an attack-move east, and an engineer in range of it heading north, which
/// soon walks out of range.
fn bot_meets_a_fleeing_engineer() -> (SimWorld, SimId, SimId) {
    let mut world = SimWorld::new(SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: true, ..SimConfig::default() }, 1);
    let bot = world.spawn_mobile(world.blueprints.id("t1_light_walker"), Team::Blue, point(1_000, 2_500));
    let engineer = world.spawn_mobile(world.blueprints.id("t1_engineer"), Team::Red, point(1_200, 2_500));
    sturdy(&mut world, engineer);
    world.advance_tick(&[SimCommand::AttackMove { units: vec![bot], goal: point(4_000, 2_500) }, SimCommand::Move { units: vec![engineer], goal: point(1_200, 5_000) }]);
    (world, bot, engineer)
}

/// Shells lead their targets, so an engineer walking straight away is hit as it goes: enough health
/// that it lives to walk out of range, which is what these tests are about.
fn sturdy(world: &mut SimWorld, id: SimId) {
    let mut health = world.ecs.get::<&mut Health>(world.unit_entity(id).unwrap()).unwrap();
    health.max = fp_int(1_500);
    health.current = health.max;
}

#[test]
fn an_attack_moving_unit_chases_its_target_until_it_is_destroyed_then_moves_on() {
    let (mut world, bot, engineer) = bot_meets_a_fleeing_engineer();
    let entity = world.unit_entity(bot).unwrap();
    let mut chased = false;
    let mut destroyed_at = None;
    for tick in 0..2_000 {
        world.advance_tick(&[]);
        let combat = *world.ecs.get::<&Combat>(entity).unwrap();
        if world.unit_health(engineer).is_some() {
            if combat.target == Some(engineer) || combat.forced_target == Some(engineer) {
                chased = true;
            } else {
                assert!(!chased, "tick {tick}: it gave up on the engineer while it lived");
            }
        } else if destroyed_at.is_none() {
            destroyed_at = Some(tick);
        }
    }
    assert!(chased, "it took the engineer as its target");
    assert!(destroyed_at.is_some(), "it chased the engineer down");
    let here = world.unit_position(bot).unwrap().xy();
    let goal = point(4_000, 2_500);
    assert!((here.x - goal.x).abs() < fp_int(200) && (here.y - goal.y).abs() < fp_int(200), "then went on to its goal, and is at {here:?}");
}

#[test]
fn a_new_order_calls_off_a_chase() {
    let (mut world, bot, engineer) = bot_meets_a_fleeing_engineer();
    let entity = world.unit_entity(bot).unwrap();
    // Until the engineer is out of range, and the bot has set off after it.
    for _ in 0..2_000 {
        world.advance_tick(&[]);
        let moved = world.unit_position(bot).unwrap().y > fp_int(2_520);
        if world.ecs.get::<&Combat>(entity).unwrap().target == Some(engineer) && moved {
            break;
        }
    }
    assert!(world.unit_position(bot).unwrap().y > fp_int(2_520), "it went after the engineer once it walked out of range");
    world.advance_tick(&[SimCommand::Move { units: vec![bot], goal: point(1_000, 500) }]);
    for _ in 0..20 {
        world.advance_tick(&[]);
    }
    let combat = *world.ecs.get::<&Combat>(entity).unwrap();
    assert_ne!(combat.target, Some(engineer), "it let the engineer go");
    assert_eq!(combat.forced_target, None);
    for _ in 0..400 {
        world.advance_tick(&[]);
    }
    assert!(world.unit_health(engineer).is_some(), "the engineer got away");
    let here = world.unit_position(bot).unwrap().xy();
    assert!(here.y < fp_int(1_000), "the bot went where it was told: {here:?}");
}

#[test]
fn an_armed_builder_on_patrol_chases_an_intruder_then_goes_on() {
    // A commander on patrol fights what comes within its range as any unit on patrol does: it
    // goes after an enemy walking away, and once that is destroyed, on to its next patrol point
    // (the design owner, 2026-11-28). It used to fire only while the enemy was in range.
    let mut world = SimWorld::new(SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: true, ..SimConfig::default() }, 1);
    let commander = world.spawn_mobile(world.blueprints.id("command_rig"), Team::Blue, point(1_000, 2_500));
    let engineer = world.spawn_mobile(world.blueprints.id("t1_engineer"), Team::Red, point(1_300, 2_500));
    // Enough health to walk out of range, and slower than the commander, so that it cannot walk
    // out of the commander's sight.
    {
        let mut health = world.ecs.get::<&mut Health>(world.unit_entity(engineer).unwrap()).unwrap();
        health.max = fp_int(4_000);
        health.current = health.max;
    }
    let commander_speed = world.ecs.get::<&sim_engine::sim::components::UnitStats>(world.unit_entity(commander).unwrap()).unwrap().max_speed;
    world.ecs.get::<&mut sim_engine::sim::components::UnitStats>(world.unit_entity(engineer).unwrap()).unwrap().max_speed = commander_speed / 2;
    let points = vec![point(1_000, 2_500), point(3_000, 2_500)];
    world.advance_tick(&[SimCommand::Patrol { units: vec![commander], points, queued: false }, SimCommand::Move { units: vec![engineer], goal: point(1_300, 5_000) }]);
    let entity = world.unit_entity(commander).unwrap();
    let mut north = fp_int(0);
    for _ in 0..2_000 {
        world.advance_tick(&[]);
        north = north.max(world.unit_position(commander).unwrap().y);
        if world.unit_health(engineer).is_none() {
            break;
        }
    }
    assert!(world.unit_health(engineer).is_none(), "the commander destroyed the engineer");
    assert!(north > fp_int(2_700), "it went after the engineer, as far as {north:?}");
    assert!(world.ecs.get::<&sim_engine::sim::components::Patrol>(entity).is_ok(), "and is still on patrol");
    let mut east = fp_int(0);
    for _ in 0..2_000 {
        world.advance_tick(&[]);
        east = east.max(world.unit_position(commander).unwrap().x);
    }
    assert!(east > fp_int(2_700), "it went back to patrolling, as far east as {east:?}");
}

#[test]
fn a_patrol_gives_up_an_intruder_its_team_loses_sight_of() {
    // A scout that dashes through a bot's range and away, faster than the bot can follow, is let
    // go once out of sight, and the bot goes back to its patrol rather than wander after it.
    let mut world = SimWorld::new(SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: true, ..SimConfig::default() }, 1);
    let bot = world.spawn_mobile(world.blueprints.id("t1_light_walker"), Team::Blue, point(1_000, 2_500));
    let scout = world.spawn_mobile(world.blueprints.id("t1_land_scout"), Team::Red, point(1_150, 2_600));
    sturdy(&mut world, scout);
    world.ecs.get::<&mut Combat>(world.unit_entity(scout).unwrap()).unwrap().range = Fp::ZERO;
    let points = vec![point(1_000, 2_500), point(3_000, 2_500)];
    world.advance_tick(&[SimCommand::Patrol { units: vec![bot], points, queued: false }, SimCommand::Move { units: vec![scout], goal: point(1_150, 5_000) }]);
    let entity = world.unit_entity(bot).unwrap();
    let mut chased = false;
    for _ in 0..600 {
        world.advance_tick(&[]);
        chased |= world.ecs.get::<&Combat>(entity).unwrap().target == Some(scout) && world.unit_position(bot).unwrap().y > fp_int(2_600);
    }
    assert!(chased, "the bot went after the scout");
    assert!(world.unit_health(scout).is_some(), "the scout got away");
    let combat = *world.ecs.get::<&Combat>(entity).unwrap();
    assert_ne!(combat.target, Some(scout), "the bot let it go");
    assert_ne!(combat.forced_target, Some(scout));
    let mut east = fp_int(0);
    for _ in 0..1_000 {
        world.advance_tick(&[]);
        east = east.max(world.unit_position(bot).unwrap().x);
    }
    assert!(east > fp_int(2_700), "it went back to patrolling, as far east as {east:?}");
}

#[test]
fn a_patrol_goes_on_after_a_chase() {
    let mut world = SimWorld::new(SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: true, ..SimConfig::default() }, 1);
    let bot = world.spawn_mobile(world.blueprints.id("t1_light_walker"), Team::Blue, point(1_000, 2_500));
    let engineer = world.spawn_mobile(world.blueprints.id("t1_engineer"), Team::Red, point(1_200, 2_500));
    sturdy(&mut world, engineer);
    let points = vec![point(1_000, 2_500), point(3_000, 2_500)];
    world.advance_tick(&[SimCommand::Patrol { units: vec![bot], points, queued: false }, SimCommand::Move { units: vec![engineer], goal: point(1_200, 5_000) }]);
    let entity = world.unit_entity(bot).unwrap();
    let mut chased = false;
    for _ in 0..1_500 {
        world.advance_tick(&[]);
        chased |= world.ecs.get::<&Combat>(entity).unwrap().target == Some(engineer) && world.unit_position(bot).unwrap().y > fp_int(2_520);
        if world.unit_health(engineer).is_none() {
            break;
        }
    }
    assert!(chased && world.unit_health(engineer).is_none(), "it chased the engineer down");
    assert!(world.ecs.get::<&sim_engine::sim::components::Patrol>(entity).is_ok(), "and is still on patrol");
    let mut east = fp_int(0);
    for _ in 0..1_000 {
        world.advance_tick(&[]);
        east = east.max(world.unit_position(bot).unwrap().x);
    }
    assert!(east > fp_int(2_700), "it went back to patrolling, as far east as {east:?}");
}

#[test]
fn a_target_that_outruns_an_attack_move_is_chased_by_route_until_it_is_caught() {
    let mut world = SimWorld::new(SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: true, ..SimConfig::default() }, 1);
    let tank = world.spawn_mobile(world.blueprints.id("t2_heavy_tank"), Team::Blue, point(1_000, 1_000));
    let entity = world.unit_entity(tank).unwrap();
    // Twice as fast as the tank, so it gets well away before it stops.
    let bot = world.spawn_mobile(world.blueprints.id("t1_light_walker"), Team::Red, point(1_300, 1_000));
    let bot_entity = world.unit_entity(bot).unwrap();
    world.ecs.get::<&mut sim_engine::sim::components::UnitStats>(bot_entity).unwrap().max_speed = fp_int(9);
    world.ecs.get::<&mut Combat>(bot_entity).unwrap().range = Fp::ZERO;
    // And tough enough to live until then: half a minute of the tank's fire.
    let gun = world.blueprints.get(world.blueprints.id("t2_heavy_tank")).weapon.expect("the tank has a gun");
    let tough = Fp::from_num(gun.damage.to_num::<f64>() * 10.0 / f64::from(gun.reload_ticks) * 30.0);
    *world.ecs.get::<&mut Health>(bot_entity).unwrap() = Health { current: tough, max: tough };
    let goal = point(4_000, 1_000);
    world.advance_tick(&[SimCommand::AttackMove { units: vec![tank], goal }, SimCommand::Move { units: vec![bot], goal: point(1_300, 4_000) }]);
    let mut routed = false;
    for tick in 0..3_000 {
        world.advance_tick(&[]);
        let combat = *world.ecs.get::<&Combat>(entity).unwrap();
        routed |= combat.forced_target == Some(bot) && combat.chasing;
        if world.unit_health(bot).is_some() {
            assert!(combat.target == Some(bot) || combat.forced_target == Some(bot), "tick {tick}: it gave up on the bot");
        }
    }
    assert!(routed, "once the bot was far away, the tank was given a route to it");
    assert!(world.unit_health(bot).is_none(), "and caught it once it stopped");
    let here = world.unit_position(tank).unwrap().xy();
    assert!((here.x - goal.x).abs() < fp_int(200) && (here.y - goal.y).abs() < fp_int(200), "then went on to its goal, and is at {here:?}");
}

/// A shell that hits is reported where it struck, on the target, for drawing it that far: its
/// last position, which is all the renderer had, is a tick's flight short.
#[test]
fn a_shell_is_reported_where_it_struck() {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    let (tank, target_kind) = (world.blueprints.id("t1_medium_tank"), world.blueprints.id("t2_heavy_tank"));
    let shooter = world.spawn_mobile(tank, Team::Blue, point(1_000, 1_000));
    let target = world.spawn_mobile(target_kind, Team::Red, point(1_340, 1_000));
    world.ecs.get::<&mut Combat>(world.unit_entity(target).unwrap()).unwrap().range = Fp::ZERO;
    world.advance_tick(&[SimCommand::Attack { units: vec![shooter], target }]);
    let at = world.unit_position(target).unwrap();
    let radius = world.blueprints.get(target_kind).radius;
    let mut struck = Vec::new();
    for _ in 0..60 {
        world.advance_tick(&[]);
        struck.extend(world.snapshot().landed.into_iter().filter(|shell| shell.source == shooter));
    }
    assert!(!struck.is_empty(), "the tank should have fired and hit");
    for shell in struck {
        let (dx, dy) = (Fp::from_bits(shell.x) - at.x, Fp::from_bits(shell.y) - at.y);
        assert!(dx * dx + dy * dy <= radius * radius * 2, "struck {dx}, {dy} from the target's middle, beyond its {radius} radius");
    }
}

/// A tank told to attack a target behind a ridge used to shell the ridge, every shell landing
/// short. It walks on until the shot is clear, and then hits.
#[test]
fn a_tank_told_to_attack_past_a_ridge_closes_in_instead_of_shelling_it() {
    use sim_engine::sim::terrain::{Terrain, HEIGHT_FRAC_BITS};
    // Flat, with a ridge 30 high and 80 across at x 1,140 to 1,220.
    let cells = 256usize;
    let height = |x: usize| if (57..=61).contains(&x) { 30 } else { 0 };
    let heights: Vec<i16> = (0..(cells + 1) * (cells + 1)).map(|index| (height(index % (cells + 1)) << HEIGHT_FRAC_BITS) as i16).collect();
    let terrain = Terrain::from_heights(5_120, heights, &vec![false; cells * cells], Fp::from_num(-1_000));
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::with_terrain(config, 1, terrain);
    world.prepare_map();
    let (tank, target_kind) = (world.blueprints.id("t1_medium_tank"), world.blueprints.id("t2_heavy_tank"));
    let shooter = world.spawn_mobile(tank, Team::Blue, point(1_000, 1_000));
    let target = world.spawn_mobile(target_kind, Team::Red, point(1_330, 1_000));
    let entity = world.unit_entity(target).unwrap();
    world.ecs.get::<&mut Combat>(entity).unwrap().range = Fp::ZERO;
    {
        let mut health = world.ecs.get::<&mut Health>(entity).unwrap();
        health.max = fp_int(100_000);
        health.current = health.max;
    }
    world.advance_tick(&[SimCommand::Attack { units: vec![shooter], target }]);
    let mut short = 0;
    for _ in 0..300 {
        world.advance_tick(&[]);
        short += world.snapshot().landed.iter().filter(|shell| shell.source == shooter && Fp::from_bits(shell.x) < fp_int(1_250)).count();
    }
    assert_eq!(short, 0, "no shells into the ridge");
    assert!(world.unit_health(target).unwrap() < fp_int(100_000), "it should have closed in and hit");
}

fn distance(world: &SimWorld, unit: SimId, to: Vec2Fp) -> f64 {
    let here = world.unit_position(unit).unwrap().xy();
    (here.x - to.x).to_num::<f64>().hypot((here.y - to.y).to_num::<f64>())
}

/// A Shift-attack after a move: the unit finishes the move, firing only as anything would,
/// and then goes after its target wherever that has got to, which an attack-move to where
/// the target was would not. The plan shows both, the move then the attack.
#[test]
fn a_queued_attack_waits_its_turn_then_follows_its_target() {
    use sim_engine::sim::world::PlanStepKind;
    let mut world = flat_world(false);
    let shooter = world.spawn_unit(standard_unit(Team::Blue, point(500, 500)));
    let target = passive_unit(&mut world, Team::Red, point(2_500, 500));
    let (stop, refuge) = (point(500, 1_500), point(2_500, 3_000));
    world.advance_tick(&[
        SimCommand::Move { units: vec![shooter], goal: stop },
        SimCommand::QueueAttack { units: vec![shooter], target },
        SimCommand::Move { units: vec![target], goal: refuge },
    ]);
    let plan = world.snapshot_for(Some(Team::Blue)).plans.into_iter().find(|plan| plan.id == shooter).expect("a plan");
    let kinds: Vec<PlanStepKind> = plan.steps.iter().map(|step| step.kind).collect();
    assert_eq!(kinds, [PlanStepKind::Move, PlanStepKind::Attack]);

    let forced = |world: &SimWorld| world.ecs.get::<&Combat>(world.unit_entity(shooter).unwrap()).unwrap().forced_target;
    let mut ticks = 0;
    while distance(&world, shooter, stop) > 20.0 {
        assert_eq!(forced(&world), None, "the attack waits for the move");
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 2_000, "never reached its stop");
    }
    for _ in 0..300 {
        if forced(&world).is_some() {
            break;
        }
        world.advance_tick(&[]);
    }
    assert_eq!(forced(&world), Some(target), "then it attacks");
    for _ in 0..6_000 {
        if world.unit_position(target).is_none() {
            break;
        }
        world.advance_tick(&[]);
    }
    assert!(world.unit_position(target).is_none(), "it followed its target to where it went, and destroyed it");
    for _ in 0..200 {
        world.advance_tick(&[]);
    }
    assert!(world.order_queues.is_empty());
    assert_eq!(world.flow_field.route_count(), 0, "every route should have been handed back");
}

/// A queued attack on something destroyed before its turn is skipped, and what was queued
/// after it goes ahead.
#[test]
fn a_queued_attack_on_what_is_already_gone_is_skipped() {
    let mut world = flat_world(false);
    let shooter = world.spawn_unit(standard_unit(Team::Blue, point(500, 500)));
    let target = passive_unit(&mut world, Team::Red, point(2_500, 500));
    let (stop, last) = (point(500, 1_000), point(1_000, 1_000));
    world.advance_tick(&[
        SimCommand::Move { units: vec![shooter], goal: stop },
        SimCommand::QueueAttack { units: vec![shooter], target },
        SimCommand::QueueMove { units: vec![shooter], goal: last, attack: false },
    ]);
    world.ecs.get::<&mut Health>(world.unit_entity(target).unwrap()).unwrap().current = fp_int(1);
    shoot(&mut world, point(2_400, 500), point(2_500, 500), fp_int(4), shooter, Team::Blue);
    run_until_projectiles_gone(&mut world, 200);
    assert!(world.unit_position(target).is_none(), "destroyed before the attack's turn");
    for _ in 0..2_000 {
        world.advance_tick(&[]);
    }
    assert!(distance(&world, shooter, last) < 20.0, "it went on to its last stop");
    assert!(world.order_queues.is_empty());
    assert_eq!(world.flow_field.route_count(), 0, "every route should have been handed back");
}

/// The commander, a builder, keeps its orders as tasks, and a queued attack waits among them
/// in the same way.
#[test]
fn a_commanders_queued_attack_waits_for_its_move() {
    let mut world = flat_map(false);
    let commander = world.spawn_mobile(world.blueprints.id("command_rig"), Team::Blue, point(1_000, 1_000));
    let target = passive_unit(&mut world, Team::Red, point(3_000, 1_000));
    let stop = point(1_000, 1_600);
    world.advance_tick(&[SimCommand::Move { units: vec![commander], goal: stop }, SimCommand::QueueAttack { units: vec![commander], target }]);
    let forced = |world: &SimWorld| world.ecs.get::<&Combat>(world.unit_entity(commander).unwrap()).unwrap().forced_target;
    let mut ticks = 0;
    while distance(&world, commander, stop) > 30.0 {
        assert_eq!(forced(&world), None, "the attack waits for the move");
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 3_000, "never reached its stop");
    }
    for _ in 0..300 {
        if forced(&world).is_some() {
            break;
        }
        world.advance_tick(&[]);
    }
    assert_eq!(forced(&world), Some(target), "then it attacks");
}

/// A unit that makes the final hit on an enemy has destroyed it, which its nameplate counts.
/// The others that hurt it do not, however much they did.
#[test]
fn the_unit_that_makes_the_final_hit_counts_the_kill() {
    let mut world = flat_world(false);
    let tank = world.blueprints.id("t1_medium_tank");
    let first = world.spawn_mobile(tank, Team::Blue, point(1_000, 1_000));
    let second = world.spawn_mobile(tank, Team::Blue, point(1_000, 1_100));
    let enemy = world.spawn_mobile(world.blueprints.id("t1_land_scout"), Team::Red, point(1_250, 1_050));
    for _ in 0..300 {
        world.advance_tick(&[]);
        if world.unit_position(enemy).is_none() {
            break;
        }
    }
    assert!(world.unit_position(enemy).is_none(), "the scout is destroyed");
    let kills = |unit: SimId| world.unit_entity(unit).and_then(|entity| world.ecs.get::<&sim_engine::sim::components::Kills>(entity).ok().map(|kills| kills.0)).unwrap_or(0);
    assert_eq!(kills(first) + kills(second), 1, "one of the two made the final hit, and only it counts");
    let snapshot = world.snapshot_for(Some(Team::Blue));
    let killer = if kills(first) == 1 { first } else { second };
    assert_eq!(snapshot.kills, vec![(killer, 1)], "the viewer sees its count");
}
