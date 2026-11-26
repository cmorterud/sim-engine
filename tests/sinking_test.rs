//! Wrecks on their way down (GAME_SPECIFICATION.md §6.1): a ship lost at sea sinks to the sea
//! floor and lies there, a downed aircraft falls on the way it was flying and comes to rest where
//! it strikes the ground, or sinks where it strikes the water, and builders on the water reclaim
//! what lies on the floor under them.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp, Vec3Fp};
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Settling, SimId, Team, Wreck};
use sim_engine::sim::config::{SimConfig, TerrainStyle, WRECK_MASS_PERMILLE, WRECK_SUNK_PERMILLE, WRECK_TECH_PERMILLE};
use sim_engine::sim::grounds::Ground;
use sim_engine::sim::terrain::SEA_DEPTH_UNITS;
use sim_engine::sim::world::{SimWorld, WreckMotion};

const MAP: i32 = 5_120;

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

/// A shore: the sea in the west, 50 deep over a flat floor, and land in the east, from x 2,260.
fn shore() -> SimWorld {
    let config = SimConfig { map_units: MAP, terrain: TerrainStyle::Flat, ..SimConfig::default() };
    let mut world = Ground::Shore.world(config, 1);
    world.prepare_map();
    for economy in &mut world.economies {
        for value in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.base_energy_capacity, &mut economy.energy_capacity] {
            *value = Res::from_num(1_000_000);
        }
    }
    world
}

fn spawn(world: &mut SimWorld, key: &str, team: Team, at: Vec2Fp) -> SimId {
    world.spawn_mobile(world.blueprints.id(key), team, at)
}

/// Destroys a unit outright, and returns the wreck it leaves.
fn destroy(world: &mut SimWorld, unit: SimId) -> SimId {
    let before = world.wrecks.last().map(|wreck| wreck.0);
    world.strike(unit, fp_int(100_000));
    world.advance_tick(&[]);
    assert!(world.unit_entity(unit).is_none(), "the unit should be destroyed");
    let wreck = world.wrecks.last().expect("it left a wreck").0;
    assert_ne!(Some(wreck), before, "it left a wreck");
    wreck
}

fn wreck(world: &SimWorld, id: SimId) -> Option<(Wreck, Option<Settling>, hecs::Entity)> {
    let index = world.wrecks.binary_search_by_key(&id, |wreck| wreck.0).ok()?;
    let entity = world.wrecks[index].1;
    let settling = world.ecs.get::<&Settling>(entity).ok().map(|settling| *settling);
    Some((*world.ecs.get::<&Wreck>(entity).ok()?, settling, entity))
}

/// The wreck as the viewer sees it: its middle, in units, and whether it is on its way down.
fn seen(world: &SimWorld, id: SimId) -> (f64, WreckMotion) {
    let snapshot = world.snapshot();
    let view = snapshot.wrecks.iter().find(|view| view.id == id).expect("the wreck is in sight");
    (f64::from(view.z) / 4096.0, view.motion)
}

/// What a wreck of a unit at full health holds, by its mass and tech level, sunk or not.
fn wreck_mass(world: &SimWorld, key: &str, sunk: bool) -> Res {
    let unit = world.blueprints.get(world.blueprints.id(key));
    let mass = unit.mass * WRECK_MASS_PERMILLE / 1000 * WRECK_TECH_PERMILLE[usize::from(unit.tier) - 1] / 1000;
    if sunk { mass * WRECK_SUNK_PERMILLE / 1000 } else { mass }
}

#[test]
fn a_ship_lost_at_sea_sinks_to_the_floor_and_lies_there() {
    let mut world = shore();
    let water = world.terrain.water_level().to_num::<f64>();
    let floor = water - f64::from(SEA_DEPTH_UNITS);
    let frigate = spawn(&mut world, "t1_frigate", Team::Red, point(1_200, 2_560));
    let radius = world.ecs.get::<&sim_engine::sim::components::UnitStats>(world.unit_entity(frigate).unwrap()).unwrap().radius.to_num::<f64>();
    let debris = destroy(&mut world, frigate);
    let (start, settling, _) = wreck(&world, debris).unwrap();
    assert!(settling.is_some_and(|settling| settling.wet), "it starts sinking at once: {settling:?}");
    assert_eq!(start.mass, wreck_mass(&world, "t1_frigate", true), "a ship's wreck keeps the share for sinking");
    let (mut height, motion) = seen(&world, debris);
    assert_eq!(motion, WreckMotion::Sinking);
    assert!(height >= water, "it starts on the water: {height:.1} under the water at {water}");
    // Down, never up, to the floor, at a steady pace, over some seconds, drifting on a little.
    let mut ticks = 0;
    while seen(&world, debris).1 == WreckMotion::Sinking {
        world.advance_tick(&[]);
        ticks += 1;
        let (now, _) = seen(&world, debris);
        assert!(now <= height, "tick {ticks}: it rose from {height:.2} to {now:.2}");
        assert!(height - now <= 0.61, "tick {ticks}: it sank {:.2} in a tick", height - now);
        height = now;
        assert!(ticks < 200, "it never came to rest");
    }
    assert!((60..=120).contains(&ticks), "it took {ticks} ticks to sink 50 units");
    let (rest, settling, _) = wreck(&world, debris).unwrap();
    assert!(settling.is_none(), "it lies still");
    assert_eq!(seen(&world, debris), (floor + radius / 2.0, WreckMotion::Still), "on the sea floor, half its radius up");
    let drift = (rest.position - start.position).length_squared().to_num::<f64>().sqrt();
    assert!(drift < radius * 2.0, "it drifted {drift:.1}");
    assert_eq!(rest.mass, start.mass, "sinking takes nothing more from it");
}

/// An engineer floating on the water reclaims a wreck on the sea floor under it, 50 units down,
/// and one still sinking, without going under itself.
#[test]
fn an_engineer_on_the_water_reclaims_what_lies_on_the_floor_under_it() {
    let mut world = shore();
    let water = world.terrain.water_level();
    let engineer = spawn(&mut world, "t1_engineer", Team::Blue, point(1_100, 2_500));
    let frigate = spawn(&mut world, "t1_frigate", Team::Red, point(1_200, 2_560));
    let debris = destroy(&mut world, frigate);
    let held = wreck(&world, debris).unwrap().0.mass;
    world.advance_tick(&[SimCommand::Reclaim { units: vec![engineer], target: debris }]);
    let mut reclaimed_while_sinking = Res::ZERO;
    for tick in 0..2_000 {
        let at = world.unit_position(engineer).unwrap();
        assert!(at.z >= water, "tick {tick}: the engineer went under the water, to {}", at.z);
        if wreck(&world, debris).is_none() {
            break;
        }
        world.advance_tick(&[]);
        if wreck(&world, debris).is_some_and(|(_, settling, _)| settling.is_some()) {
            reclaimed_while_sinking = world.economies[0].mass_reclaimed_total;
        }
    }
    assert!(wreck(&world, debris).is_none(), "it reclaimed the wreck");
    assert!(reclaimed_while_sinking > Res::ZERO, "it reclaimed some while it was still sinking");
    assert_eq!(world.economies[0].mass_reclaimed_total, held, "all of it");
    // Still on the water, over where the wreck lay.
    let at = world.unit_position(engineer).unwrap();
    assert!(world.terrain.under_water(at.xy()), "it floated out to the wreck");
}

/// Flies an interceptor east at its height for a while, then destroys it, and returns where it
/// was, which way it was flying, and its wreck.
fn shoot_down(world: &mut SimWorld, from: Vec2Fp) -> (Vec3Fp, Vec2Fp, SimId) {
    let plane = spawn(world, "t1_interceptor", Team::Red, from);
    world.advance_tick(&[SimCommand::Move { units: vec![plane], goal: Vec2Fp::new(from.x + fp_int(4_000), from.y) }]);
    for _ in 0..30 {
        world.advance_tick(&[]);
    }
    let before = world.unit_position(plane).unwrap();
    world.advance_tick(&[]);
    let at = world.unit_position(plane).unwrap();
    let heading = (at.xy() - before.xy()).normalized();
    let wreck = destroy(world, plane);
    (at, heading, wreck)
}

/// A downed aircraft keeps on the way it was flying as it falls, and lies where it comes down,
/// whole: nothing reclaims it on the way.
#[test]
fn a_downed_aircraft_falls_on_ahead_and_lies_where_it_strikes_the_ground() {
    let mut world = shore();
    let engineer = spawn(&mut world, "t1_engineer", Team::Blue, point(3_400, 2_500));
    let (hit, heading, debris) = shoot_down(&mut world, point(2_400, 2_560));
    let (start, settling, _) = wreck(&world, debris).unwrap();
    assert!(settling.is_some_and(|settling| !settling.wet), "it falls: {settling:?}");
    assert_eq!(start.mass, wreck_mass(&world, "t1_interceptor", false), "over land it keeps it all");
    assert_eq!(seen(&world, debris).1, WreckMotion::Falling);
    world.advance_tick(&[SimCommand::Reclaim { units: vec![engineer], target: debris }]);
    let (mut height, _) = seen(&world, debris);
    let mut ticks = 0;
    while seen(&world, debris).1 == WreckMotion::Falling {
        assert_eq!(world.economies[0].mass_reclaimed_total, Res::ZERO, "nothing is reclaimed in the air");
        world.advance_tick(&[]);
        ticks += 1;
        let (now, _) = seen(&world, debris);
        assert!(now < height, "tick {ticks}: it did not fall");
        height = now;
        assert!(ticks < 100, "it never came down");
    }
    let (rest, settling, _) = wreck(&world, debris).unwrap();
    assert!(settling.is_none(), "it lies still");
    let ground = world.terrain.height_at(rest.position);
    assert_eq!(seen(&world, debris), ((ground + rest.radius / 2).to_num::<f64>(), WreckMotion::Still));
    // It came down well ahead of where it was hit, on the way it was flying.
    let way = rest.position - hit.xy();
    let ahead = (way.x * heading.x + way.y * heading.y).to_num::<f64>();
    let aside = (way.x * heading.y - way.y * heading.x).to_num::<f64>().abs();
    assert!(ahead > 200.0 && aside < ahead * 0.2, "{ahead:.0} ahead and {aside:.0} aside of where it was hit, after {ticks} ticks");
    assert!((15..=40).contains(&ticks), "it fell for {ticks} ticks");
    // Once down, it is reclaimed.
    for _ in 0..600 {
        world.advance_tick(&[]);
    }
    assert!(world.economies[0].mass_reclaimed_total > Res::ZERO, "the engineer reclaims it once it is down");
}

/// One downed over the sea falls into it, loses the share for sinking there, and sinks to the
/// floor.
#[test]
fn a_downed_aircraft_over_the_sea_sinks_to_the_floor() {
    let mut world = shore();
    let water = world.terrain.water_level();
    let (_, _, debris) = shoot_down(&mut world, point(400, 2_560));
    let whole = wreck_mass(&world, "t1_interceptor", false);
    assert_eq!(wreck(&world, debris).unwrap().0.mass, whole, "in the air it is whole");
    let mut motions = vec![WreckMotion::Falling];
    for _ in 0..300 {
        world.advance_tick(&[]);
        let (height, motion) = seen(&world, debris);
        if motions.last() != Some(&motion) {
            motions.push(motion);
            if motion == WreckMotion::Sinking {
                assert!(height <= water.to_num::<f64>() + 5.0, "it sinks once it is in the water, not at {height:.1}");
            }
        }
    }
    assert_eq!(motions, [WreckMotion::Falling, WreckMotion::Sinking, WreckMotion::Still], "it fell, sank, and came to rest");
    let (rest, _, _) = wreck(&world, debris).unwrap();
    assert!(world.terrain.under_water(rest.position));
    assert_eq!(rest.mass, wreck_mass(&world, "t1_interceptor", true), "it lost the share for sinking");
    assert_eq!(seen(&world, debris).0, (world.terrain.height_at(rest.position) + rest.radius / 2).to_num::<f64>(), "on the floor");
}

/// Units that die on the ground, or on the sea floor, leave wrecks at rest at once.
#[test]
fn a_unit_on_the_ground_or_the_sea_floor_leaves_its_wreck_at_rest() {
    let mut world = shore();
    let tank = spawn(&mut world, "t1_medium_tank", Team::Red, point(4_000, 2_560));
    let on_land = destroy(&mut world, tank);
    assert!(wreck(&world, on_land).unwrap().1.is_none());
    assert_eq!(seen(&world, on_land).1, WreckMotion::Still);
    assert_eq!(wreck(&world, on_land).unwrap().0.mass, wreck_mass(&world, "t1_medium_tank", false));
    let commander = spawn(&mut world, "command_rig", Team::Red, point(1_000, 1_000));
    for _ in 0..5 {
        world.advance_tick(&[]);
    }
    let on_floor = destroy(&mut world, commander);
    let (rest, settling, _) = wreck(&world, on_floor).unwrap();
    assert!(settling.is_none(), "a seabed walker's wreck lies where it fell");
    assert!(world.terrain.under_water(rest.position));
    assert_eq!(seen(&world, on_floor).1, WreckMotion::Still);
}

/// Two worlds that down the same ships and aircraft match, and a wreck's way down is in the
/// checksum.
#[test]
fn wrecks_on_their_way_down_are_deterministic_and_in_the_checksum() {
    let run = || {
        let mut world = shore();
        let (_, _, falling) = shoot_down(&mut world, point(400, 2_000));
        let frigate = spawn(&mut world, "t1_frigate", Team::Red, point(1_200, 3_000));
        destroy(&mut world, frigate);
        let mut checksums = Vec::new();
        for _ in 0..150 {
            world.advance_tick(&[]);
            checksums.push(world.compute_checksum());
        }
        (world, checksums, falling)
    };
    let (_, first, _) = run();
    let (_, again, _) = run();
    assert_eq!(first, again);

    let mut world = shore();
    let (_, _, falling) = shoot_down(&mut world, point(400, 2_000));
    let entity = wreck(&world, falling).unwrap().2;
    let baseline = world.compute_checksum();
    let bit = Fp::from_bits(1);
    type Edit = Box<dyn Fn(&mut Settling, bool)>;
    let edits: Vec<(&str, Edit)> = vec![
        ("height", Box::new(move |s, undo| s.z += if undo { -bit } else { bit })),
        ("velocity x", Box::new(move |s, undo| s.velocity.x += if undo { -bit } else { bit })),
        ("velocity y", Box::new(move |s, undo| s.velocity.y += if undo { -bit } else { bit })),
        ("velocity z", Box::new(move |s, undo| s.velocity.z += if undo { -bit } else { bit })),
        ("in the water", Box::new(|s, _| s.wet = !s.wet)),
    ];
    for (name, edit) in edits {
        edit(&mut world.ecs.get::<&mut Settling>(entity).unwrap(), false);
        assert_ne!(world.compute_checksum(), baseline, "a settling wreck's {name} is not covered by the checksum");
        edit(&mut world.ecs.get::<&mut Settling>(entity).unwrap(), true);
        assert_eq!(world.compute_checksum(), baseline, "undoing {name} did not restore the checksum");
    }
}
