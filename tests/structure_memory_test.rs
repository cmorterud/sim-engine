//! Remembered structures (GAME_SPECIFICATION.md §9): an enemy structure seen once stays on the
//! map as it was when last seen, faded, and can be fired at, until its team sees it is gone. One
//! found only on radar stays as an unidentified contact.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Combat, Health, Projectile, SimId, Team};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::world::{SimWorld, UnitView};

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

fn foggy_world() -> SimWorld {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: true, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    for economy in &mut world.economies {
        for value in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.mass, &mut economy.base_energy_capacity, &mut economy.energy_capacity, &mut economy.energy] {
            *value = Res::from_num(100_000);
        }
    }
    world
}

fn run(world: &mut SimWorld, ticks: u32) {
    for _ in 0..ticks {
        world.advance_tick(&[]);
    }
}

/// What Blue's view shows of a unit.
fn blue_sees(world: &SimWorld, id: SimId) -> Option<UnitView> {
    world.snapshot_for(Some(Team::Blue)).units.into_iter().find(|unit| unit.id == id)
}

fn shells_from(world: &SimWorld, source: SimId) -> usize {
    world.projectiles.iter().filter(|&&(_, entity)| world.ecs.get::<&Projectile>(entity).is_ok_and(|shell| shell.source == source)).count()
}

/// Runs, and returns how many shells `source` fired.
fn shots(world: &mut SimWorld, source: SimId, ticks: u32) -> usize {
    let mut fired = std::collections::BTreeSet::new();
    for _ in 0..ticks {
        world.advance_tick(&[]);
        for &(id, entity) in &world.projectiles {
            if world.ecs.get::<&Projectile>(entity).is_ok_and(|shell| shell.source == source) {
                fired.insert(id);
            }
        }
    }
    fired.len()
}

/// A Red generator at x 4,000, too tough to be destroyed by accident.
fn red_generator(world: &mut SimWorld) -> SimId {
    let generator = world.spawn_finished_structure(world.blueprints.id("power_generator"), Team::Red, 200, 128).expect("room");
    let entity = world.unit_entity(generator).unwrap();
    let mut health = world.ecs.get::<&mut Health>(entity).unwrap();
    health.max = fp_int(100_000);
    health.current = health.max;
    generator
}

/// A Blue scout, that does not shoot, looks at `structure` and then drives away.
fn scout_once(world: &mut SimWorld, structure: SimId) {
    let scout = world.spawn_mobile(world.blueprints.id("t1_land_scout"), Team::Blue, point(3_700, 2_560));
    world.ecs.get::<&mut Combat>(world.unit_entity(scout).unwrap()).unwrap().range = Fp::ZERO;
    run(world, 10);
    assert!(blue_sees(world, structure).is_some_and(|view| !view.remembered), "the scout sees it");
    world.advance_tick(&[SimCommand::Move { units: vec![scout], goal: point(300, 300) }]);
    run(world, 200);
    let at = world.unit_position(structure).unwrap();
    assert!(!world.visibility.is_visible(Team::Blue, at.x.to_bits(), at.y.to_bits()), "the scout has gone");
}

#[test]
fn a_structure_seen_once_stays_as_it_was_last_seen() {
    let mut world = foggy_world();
    let generator = red_generator(&mut world);
    run(&mut world, 10);
    assert!(blue_sees(&world, generator).is_none(), "never seen, it does not show");
    scout_once(&mut world, generator);
    let view = blue_sees(&world, generator).expect("seen once, it stays on the map");
    assert!(view.remembered && view.health_permille == 1000, "remembered, at full health: {view:?}");
    // Damaged out of sight: the map still shows it as it was.
    {
        let entity = world.unit_entity(generator).unwrap();
        let mut health = world.ecs.get::<&mut Health>(entity).unwrap();
        health.current = health.max / 2;
    }
    run(&mut world, 20);
    assert_eq!(blue_sees(&world, generator).unwrap().health_permille, 1000, "its health does not update out of sight");
    // Red never sees Blue's structures, so it remembers none.
    assert!(world.known_structures[Team::Red.index()].is_empty());
    // Seen again, it is up to date.
    world.spawn_mobile(world.blueprints.id("t1_land_scout"), Team::Blue, point(3_700, 2_560));
    run(&mut world, 10);
    let view = blue_sees(&world, generator).unwrap();
    assert!(!view.remembered && view.health_permille == 500, "seen again, as it is now: {view:?}");
}

/// T3 artillery's sight is short of its reach. A position spotted once can be shelled with
/// nothing watching it; before that, the artillery cannot fire at it.
#[test]
fn a_remembered_structure_can_be_fired_at_unseen() {
    for scouted in [false, true] {
        let mut world = foggy_world();
        let artillery = world.spawn_mobile(world.blueprints.id("t3_mobile_artillery"), Team::Blue, point(2_600, 2_560));
        let range = world.blueprints.get(world.blueprints.id("t3_mobile_artillery")).weapon.unwrap().range.to_num::<i32>();
        assert!(4_000 - 2_600 < range, "the generator is in reach");
        let generator = red_generator(&mut world);
        if scouted {
            scout_once(&mut world, generator);
        } else {
            run(&mut world, 210);
        }
        let fired = shots(&mut world, artillery, 200);
        if scouted {
            assert!(fired > 0, "the artillery shells the remembered generator");
        } else {
            assert_eq!(fired, 0, "unseen and unremembered, it is not fired at");
            assert!(world.unit_entity(generator).is_some());
        }
    }
}

/// Destroyed out of sight, it stays on the map until Blue sees the spot, but nothing fires at it
/// of its own accord. An order to attack it fires at the spot.
#[test]
fn a_structure_destroyed_out_of_sight_stays_until_the_spot_is_seen() {
    let mut world = foggy_world();
    let artillery = world.spawn_mobile(world.blueprints.id("t3_mobile_artillery"), Team::Blue, point(2_600, 2_560));
    let generator = red_generator(&mut world);
    scout_once(&mut world, generator);
    let at = world.unit_position(generator).unwrap();
    // Let the artillery finish it off.
    {
        let entity = world.unit_entity(generator).unwrap();
        world.ecs.get::<&mut Health>(entity).unwrap().current = Fp::from_num(1);
    }
    for _ in 0..400 {
        world.advance_tick(&[]);
        if world.unit_entity(generator).is_none() {
            break;
        }
    }
    assert!(world.unit_entity(generator).is_none(), "the artillery destroyed it");
    run(&mut world, 80);
    let view = blue_sees(&world, generator).expect("Blue does not know it is gone");
    assert!(view.remembered);
    assert_eq!(shells_from(&world, artillery), 0, "nothing fires at it of its own accord");
    assert_eq!(shots(&mut world, artillery, 100), 0, "nor goes on firing");
    // Ordered to attack it, the artillery fires at the spot.
    world.advance_tick(&[SimCommand::Attack { units: vec![artillery], target: generator }]);
    let ground = world.ecs.get::<&Combat>(world.unit_entity(artillery).unwrap()).unwrap().ground_target;
    assert_eq!(ground, Some(at.xy()), "an attack on it fires at where it was");
    // Seen again, it is gone from the map.
    world.spawn_mobile(world.blueprints.id("t1_land_scout"), Team::Blue, point(3_700, 2_560));
    run(&mut world, 10);
    assert!(blue_sees(&world, generator).is_none(), "seen gone, it is forgotten");
    assert!(world.known_structures[Team::Blue.index()].is_empty());
}

/// Found only on radar, a structure stays as an unidentified contact once the radar is gone, and
/// its type and health are not known.
#[test]
fn a_structure_found_on_radar_stays_unidentified() {
    let mut world = foggy_world();
    let radar = world.spawn_finished_structure(world.blueprints.id("radar"), Team::Blue, 50, 128).expect("room");
    let generator = world.spawn_finished_structure(world.blueprints.id("power_generator"), Team::Red, 150, 128).expect("room");
    run(&mut world, 20);
    let contact = |world: &SimWorld| world.snapshot_for(Some(Team::Blue)).contacts.into_iter().find(|contact| contact.unit == Some(generator));
    assert!(contact(&world).is_some_and(|contact| contact.is_structure), "on radar");
    assert!(blue_sees(&world, generator).is_none(), "on radar only, it is not identified");
    world.advance_tick(&[SimCommand::SelfDestruct { units: vec![radar] }]);
    run(&mut world, 100);
    assert!(world.unit_entity(radar).is_none(), "the radar is gone");
    let record = world.known_structures[Team::Blue.index()].iter().find(|record| record.id == generator).copied().expect("remembered");
    assert_eq!(record.blueprint, None, "only on radar, it is unidentified");
    assert!(contact(&world).is_some(), "it stays as a contact with the radar gone");
    assert!(blue_sees(&world, generator).is_none());
}

/// When a team's last sensor is gone, so is what it had on radar: a dead scout's contacts left
/// blips on the map, which could be fired at, for the rest of the match.
#[test]
fn radar_contacts_go_with_the_last_sensor() {
    let mut world = foggy_world();
    let scout = world.spawn_mobile(world.blueprints.id("t1_land_scout"), Team::Blue, point(1_000, 2_560));
    world.ecs.get::<&mut Combat>(world.unit_entity(scout).unwrap()).unwrap().range = Fp::ZERO;
    let def = world.blueprints.get(world.blueprints.id("t1_land_scout"));
    let (sight, radar) = (def.vision.to_num::<i32>(), def.radar.to_num::<i32>());
    assert!(sight < radar, "the scout's radar reaches past its sight");
    let walker = world.spawn_mobile(world.blueprints.id("t1_light_walker"), Team::Red, point(1_000 + (sight + radar) / 2, 2_560));
    world.ecs.get::<&mut Combat>(world.unit_entity(walker).unwrap()).unwrap().range = Fp::ZERO;
    run(&mut world, 10);
    assert!(!world.snapshot_for(Some(Team::Blue)).blips.is_empty(), "the walker is on the scout's radar");
    world.advance_tick(&[SimCommand::SelfDestruct { units: vec![scout] }]);
    run(&mut world, 100);
    assert!(world.unit_entity(scout).is_none(), "the scout is gone");
    assert!(world.snapshot_for(Some(Team::Blue)).blips.is_empty(), "and its contacts with it");
}
