//! Missile and torpedo defenses against the unit data (GAME_SPECIFICATION.md §8, §10): every
//! tactical defense shoots down every cruise missile fired slower than it reloads; every
//! strategic defense spends an interceptor on a nuclear missile; every torpedo defense destroys
//! torpedoes as fast as it reloads.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::{Blueprint, Res};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Combat, Health, Launcher, MissileDefense, MoveClass, SimId, Team};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::terrain::{Terrain, HEIGHT_FRAC_BITS};
use sim_engine::sim::world::SimWorld;

const MAP: i32 = 10_240;
/// A lake from here east to `LAKE_END`, with land on both sides.
const LAKE_START: i32 = 3_000;
const LAKE_END: i32 = 7_000;

fn world() -> SimWorld {
    let cells = (MAP / 20) as usize;
    let samples = cells + 1;
    let height = |x: usize, _y: usize| if (LAKE_START..=LAKE_END).contains(&(x as i32 * 20)) { -40 } else { 20 };
    let heights: Vec<i16> = (0..samples * samples).map(|index| (height(index % samples, index / samples) << HEIGHT_FRAC_BITS) as i16).collect();
    let blocked: Vec<bool> = (0..cells * cells)
        .map(|cell| {
            let (x, y) = (cell % cells, cell / cells);
            [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)].iter().any(|&(cx, cy)| height(cx, cy) < 0)
        })
        .collect();
    let terrain = Terrain::from_heights(MAP, heights, &blocked, Fp::ZERO);
    let config = SimConfig { map_units: MAP, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::with_terrain(config, 1, terrain);
    world.prepare_map();
    for economy in &mut world.economies {
        for value in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.base_energy_capacity, &mut economy.energy_capacity] {
            *value = Res::from_num(100_000_000);
        }
        economy.mass = Res::from_num(50_000_000);
        economy.energy = Res::from_num(50_000_000);
    }
    world
}

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

fn sails(def: &Blueprint) -> bool {
    matches!(def.movement, MoveClass::Naval | MoveClass::Submarine) || def.on_water
}

/// The defense under test, near the lake's west shore if it sails or else on land, holding its fire
/// so that it only defends, and a Blue unit beside it for the enemy to aim at: the ship itself, or
/// a unit on land beside the structure.
fn defended(world: &mut SimWorld, def: &Blueprint) -> (SimId, SimId) {
    let blueprint = world.blueprints.id(&def.key);
    let (defense, target) = if def.is_structure() {
        let defense = world.spawn_finished_structure(blueprint, Team::Blue, 100, 250).expect("room");
        let target = world.spawn_mobile(world.blueprints.id("t1_engineer"), Team::Blue, point(2_080, 5_100));
        (defense, target)
    } else {
        let ship = world.spawn_mobile(blueprint, Team::Blue, point(LAKE_START + 400, 5_000));
        (ship, ship)
    };
    world.ecs.get::<&mut Combat>(world.unit_entity(defense).unwrap()).unwrap().enabled = false;
    let mut health = world.ecs.get::<&mut Health>(world.unit_entity(target).unwrap()).unwrap();
    health.max = fp_int(400_000);
    health.current = health.max;
    (defense, target)
}

fn hurt(world: &SimWorld, id: SimId) -> bool {
    world.unit_health(id).is_some_and(|health| health < fp_int(400_000))
}

#[test]
fn every_tactical_defense_shoots_down_each_cruise_missile() {
    let db = world().blueprints;
    let mut failures = Vec::new();
    let mut checked = 0;
    for def in db.iter().filter(|def| def.missile_defense.is_some_and(|defense| !defense.strategic)) {
        checked += 1;
        let mut world = world();
        let (_, target) = defended(&mut world, def);
        let at = world.unit_position(target).unwrap().xy();
        // From well out on the land to the west, three missiles, a reload apart.
        let launcher = world.spawn_finished_structure(world.blueprints.id("tactical_missile_launcher"), Team::Red, 10, 250).expect("room");
        world.ecs.get::<&mut Launcher>(world.unit_entity(launcher).unwrap()).unwrap().stock = 3;
        world.advance_tick(&[SimCommand::AttackGround { units: vec![launcher], point: at }]);
        for _ in 0..1_200 {
            world.advance_tick(&[]);
        }
        let left = world.ecs.get::<&Launcher>(world.unit_entity(launcher).unwrap()).unwrap().stock;
        if left > 0 {
            failures.push(format!("{}: the launcher fired only {} of its 3 missiles", def.key, 3 - left));
        } else if hurt(&world, target) {
            failures.push(format!("{}: a cruise missile got through", def.key));
        }
    }
    assert!(checked >= 4, "only {checked} were checked");
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn every_strategic_defense_stops_a_nuclear_missile() {
    let db = world().blueprints;
    let mut failures = Vec::new();
    let mut checked = 0;
    for def in db.iter().filter(|def| def.missile_defense.is_some_and(|defense| defense.strategic)) {
        checked += 1;
        let mut world = world();
        let (defense, target) = defended(&mut world, def);
        world.ecs.get::<&mut MissileDefense>(world.unit_entity(defense).unwrap()).unwrap().stock = 1;
        let at = world.unit_position(target).unwrap().xy();
        let launcher = world.spawn_finished_structure(world.blueprints.id("strategic_missile_launcher"), Team::Red, 400, 50).expect("room");
        world.ecs.get::<&mut Launcher>(world.unit_entity(launcher).unwrap()).unwrap().stock = 1;
        world.advance_tick(&[SimCommand::AttackGround { units: vec![launcher], point: at }]);
        for _ in 0..1_500 {
            world.advance_tick(&[]);
        }
        let interceptors = world.ecs.get::<&MissileDefense>(world.unit_entity(defense).unwrap()).map_or(1, |defense| defense.stock);
        if world.unit_health(target).is_none() || hurt(&world, target) {
            failures.push(format!("{}: the nuclear missile got through", def.key));
        } else if interceptors != 0 {
            failures.push(format!("{}: stopped it without spending its interceptor", def.key));
        }
    }
    assert!(checked >= 1, "only {checked} were checked");
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn every_torpedo_defense_stops_torpedoes_as_fast_as_it_reloads() {
    let db = world().blueprints;
    let mut failures = Vec::new();
    let mut checked = 0;
    for def in db.iter().filter(|def| def.torpedo_defense.is_some() && sails(def)) {
        let defense_def = def.torpedo_defense.unwrap();
        checked += 1;
        let mut world = world();
        let (_, target) = defended(&mut world, def);
        let at = world.unit_position(target).unwrap().xy();
        // Submarines to the east, under the water, firing torpedoes at it, enough of them that
        // there is always one on its way for the defense to shoot at. They pick it as their target
        // as soon as they are there, so they are put there a quarter of a reload apart, so that
        // their torpedoes come one after another rather than together.
        let torpedo = world.blueprints.get(world.blueprints.id("t1_attack_sub")).weapon.unwrap();
        let mut subs = Vec::new();
        for index in 0..4 {
            let sub = world.spawn_mobile(world.blueprints.id("t1_attack_sub"), Team::Red, Vec2Fp::new(at.x + fp_int(500), at.y + fp_int(index * 60 - 90)));
            world.advance_tick(&[SimCommand::Attack { units: vec![sub], target }]);
            subs.push(sub);
            for _ in 0..torpedo.reload_ticks / 4 - 1 {
                world.advance_tick(&[]);
            }
        }
        // The defense drops a decoy for each torpedo it stops, which the torpedo turns to: the ones
        // stopped are counted from the decoys as they appear, since a torpedo close by strikes its
        // decoy the tick it turns, and those that got through from the damage.
        let mut decoys = std::collections::BTreeSet::new();
        let firing = 1_000;
        for tick in 0..firing + 200 {
            if tick == firing {
                for &sub in &subs {
                    world.ecs.get::<&mut Combat>(world.unit_entity(sub).unwrap()).unwrap().enabled = false;
                }
            }
            world.advance_tick(&[]);
            decoys.extend(world.snapshot().decoys);
        }
        let stopped = decoys.len();
        let taken = (fp_int(400_000) - world.unit_health(target).unwrap()).to_num::<f64>();
        let landed = (taken / torpedo.damage.to_num::<f64>()).round() as usize;
        let fired = stopped + landed;
        // It goes on stopping the torpedoes still running when the submarines stop firing, 500 away,
        // gathering speed for two seconds.
        let running = (500.0 / torpedo.projectile_speed.to_num::<f64>()).ceil() as usize + 20;
        let could = fired.min((firing as usize + running) / defense_def.reload_ticks as usize + 1);
        if fired < 5 {
            failures.push(format!("{}: the submarine fired only {fired} torpedoes", def.key));
        } else if (stopped as f64) < could as f64 * 0.8 || stopped > could + 1 {
            failures.push(format!("{}: stopped {stopped} of {fired} torpedoes, could have stopped {could}", def.key));
        }
    }
    assert!(checked >= 3, "only {checked} were checked");
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The fleet's tactical missile defense is one mount, the same on every ship that carries it:
/// each reaches as far and reloads as fast. A ship with two stops missiles twice as often as a
/// ship with one: the battleship and the cruiser carry two, the battlecruiser one.
#[test]
fn every_ships_tactical_missile_defense_mount_is_the_same() {
    let db = sim_engine::sim::blueprints::BlueprintDb::standard();
    let ships: Vec<_> = db.iter().filter(|def| def.movement == MoveClass::Naval).filter_map(|def| def.missile_defense.filter(|defense| !defense.strategic).map(|defense| (def.key.clone(), defense))).collect();
    let mounts_on = |ship: &str| ships.iter().find(|(key, _)| key == ship).unwrap_or_else(|| panic!("{ship} carries a tactical missile defense")).1.mounts;
    assert_eq!((mounts_on("t2_cruiser"), mounts_on("t3_battleship"), mounts_on("t3_battlecruiser")), (2, 2, 1));
    // One mount's reach, and its own reload.
    let one = |defense: &sim_engine::sim::blueprints::MissileDefenseDef| (defense.range, defense.reload_ticks * u32::from(defense.mounts));
    let (first, mount) = (&ships[0].0, one(&ships[0].1));
    for (key, defense) in &ships {
        assert_eq!(one(defense), mount, "{key}'s tactical missile defense mount is not {first}'s");
    }
}
