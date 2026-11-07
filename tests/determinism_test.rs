//! Phase 1 gates: dual-instance, golden checksum, sensitivity, and full-length determinism.

use std::fs;
use std::path::PathBuf;

use sim_engine::math::fp::{Fp, Vec2Fp};
use sim_engine::sim::components::{Combat, Health, Order, OrderKind, OrderQueue, OverchargeOrder, Paused, Position, SimId, Turning, Turret, Velocity, Veterancy};
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::scenario::{self, Scenario};
use sim_engine::sim::world::SimWorld;

const SEED: u64 = 0x5EED_2026;
const SMALL_MAP: i32 = 5_120;

fn small_config() -> SimConfig {
    SimConfig { map_units: SMALL_MAP, terrain: TerrainStyle::Hills, ..SimConfig::default() }
}

/// Runs a scenario with scripted orders and returns the checksum after each interval.
fn run(mut scenario: Scenario, ticks: u32, interval: u32) -> (Vec<u64>, SimWorld) {
    let mut checksums = Vec::new();
    let opening = std::mem::take(&mut scenario.opening_commands);
    let mut world = scenario.world;
    for tick in 0..ticks {
        scenario::advance_scripted(&mut world, &opening, SEED);
        if (tick + 1) % interval == 0 {
            checksums.push(world.compute_checksum());
        }
    }
    (checksums, world)
}

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.txt"))
}

/// Compares with a checked-in value. Run with UPDATE_GOLDEN=1 after an intended behavior change.
fn check_golden(name: &str, checksums: &[u64]) {
    let actual: String = checksums.iter().map(|value| format!("{value:016x}\n")).collect();
    let path = golden_path(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        fs::write(&path, &actual).expect("write golden file");
        return;
    }
    let expected = fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing {}; run with UPDATE_GOLDEN=1", path.display()));
    assert_eq!(actual, expected, "checksums for {name} differ from the golden file");
}

#[test]
fn two_instances_match_on_every_interval() {
    let make = || scenario::battle(SEED, 500, small_config());
    let (a, world_a) = run(make(), 1_000, 50);
    let (b, world_b) = run(make(), 1_000, 50);
    assert_eq!(a, b);
    assert_eq!(world_a.compute_checksum(), world_b.compute_checksum());
    // The scenario must actually have exercised combat and orders.
    assert!(world_a.units.len() < 1_000, "no unit died, so combat did not run");
    assert!(world_a.next_sim_id as usize > 1_000, "no projectile was fired");
}

#[test]
fn different_seed_changes_the_checksum() {
    let (a, _) = run(scenario::battle(1, 100, small_config()), 10, 10);
    let (b, _) = run(scenario::battle(2, 100, small_config()), 10, 10);
    assert_ne!(a, b);
}

#[test]
fn golden_checksums_are_stable() {
    let (checksums, _) = run(scenario::battle(SEED, 500, small_config()), 1_000, 100);
    check_golden("battle_1000_units_1000_ticks", &checksums);
}

/// The full-size scenario for 10,000 ticks. Release builds only, for run time.
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn full_battle_matches_for_10000_ticks() {
    let make = || scenario::default_battle(SEED);
    let (a, _) = run(make(), 10_000, 1_000);
    let (b, _) = run(make(), 10_000, 1_000);
    assert_eq!(a, b);
    check_golden("battle_5000_units_10000_ticks", &a);
}

/// A whole AI-vs-AI match, covering economy, construction, fog of war, and AI decisions.
/// Run on a second architecture, this proves that they all stay deterministic across platforms.
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn full_ai_match_matches_its_golden_checksums() {
    let record = sim_engine::ai::play_match(SEED, sim_engine::soak::MATCH_TICK_LIMIT, |_| {});
    assert!(record.result.is_some(), "the match should finish");
    check_golden("ai_match", &record.checksums);
}

/// Wrecks, trees, and rocks, and what reclaim brings in.
#[test]
fn every_wreck_and_reclaim_field_changes_the_checksum() {
    use sim_engine::sim::components::{Wreck, WreckKind};
    let (_, mut world) = run(scenario::battle(SEED, 20, small_config()), 10, 10);
    let at = world.unit_position(world.units[0].0).unwrap().xy();
    world.spawn_scenery(WreckKind::Rock, at, Fp::from_num(6));
    let entity = world.wrecks[0].1;
    let baseline = world.compute_checksum();
    let one_bit = Fp::from_bits(1);
    let wide_bit = sim_engine::sim::blueprints::Res::from_bits(1);
    type Edit = Box<dyn Fn(&mut SimWorld, bool)>;
    let edits: Vec<(&str, Edit)> = vec![
        ("wreck mass", Box::new(move |w, undo| w.ecs.get::<&mut Wreck>(entity).unwrap().mass += if undo { -wide_bit } else { wide_bit })),
        ("wreck energy", Box::new(move |w, undo| w.ecs.get::<&mut Wreck>(entity).unwrap().energy += if undo { -wide_bit } else { wide_bit })),
        ("wreck x", Box::new(move |w, undo| w.ecs.get::<&mut Wreck>(entity).unwrap().position.x += if undo { -one_bit } else { one_bit })),
        ("wreck y", Box::new(move |w, undo| w.ecs.get::<&mut Wreck>(entity).unwrap().position.y += if undo { -one_bit } else { one_bit })),
        ("wreck radius", Box::new(move |w, undo| w.ecs.get::<&mut Wreck>(entity).unwrap().radius += if undo { -one_bit } else { one_bit })),
        ("wreck kind", Box::new(move |w, undo| w.ecs.get::<&mut Wreck>(entity).unwrap().kind = if undo { WreckKind::Rock } else { WreckKind::Tree })),
        ("wreck health", Box::new(move |w, undo| w.ecs.get::<&mut Wreck>(entity).unwrap().health += if undo { -one_bit } else { one_bit })),
        ("mass reclaimed", Box::new(move |w, undo| w.economies[0].mass_reclaimed += if undo { -wide_bit } else { wide_bit })),
        ("energy reclaimed", Box::new(move |w, undo| w.economies[0].energy_reclaimed += if undo { -wide_bit } else { wide_bit })),
    ];
    for (name, edit) in edits {
        edit(&mut world, false);
        assert_ne!(world.compute_checksum(), baseline, "{name} is not covered by the checksum");
        edit(&mut world, true);
        assert_eq!(world.compute_checksum(), baseline, "undoing {name} did not restore the checksum");
    }
}

/// A shield is hashed only on a generator, so it has a test of its own.
#[test]
fn every_shield_field_changes_the_checksum() {
    use sim_engine::sim::components::{Shield, Team};
    let (_, mut world) = run(scenario::battle(SEED, 20, small_config()), 10, 10);
    let blueprint = world.blueprints.id("shield_generator");
    let generator = (4..200).find_map(|cell| world.spawn_finished_structure(blueprint, Team::Blue, cell, 4)).expect("room for a generator");
    let entity = world.unit_entity(generator).unwrap();
    let baseline = world.compute_checksum();
    type Edit = fn(&mut Shield, bool);
    let edits: [(&str, Edit); 7] = [
        ("radius", |s, undo| s.radius += if undo { -Fp::from_bits(1) } else { Fp::from_bits(1) }),
        ("health", |s, undo| s.health += if undo { -Fp::from_bits(1) } else { Fp::from_bits(1) }),
        ("max health", |s, undo| s.max_health += if undo { -Fp::from_bits(1) } else { Fp::from_bits(1) }),
        ("regeneration", |s, undo| s.regen_per_tick += if undo { -Fp::from_bits(1) } else { Fp::from_bits(1) }),
        ("recharging", |s, undo| s.recharging = if undo { s.recharging - 1 } else { s.recharging + 1 }),
        ("recharge time", |s, undo| s.recharge_ticks = if undo { s.recharge_ticks - 1 } else { s.recharge_ticks + 1 }),
        ("upkeep", |s, undo| s.upkeep += if undo { -Res::from_bits(1) } else { Res::from_bits(1) }),
    ];
    for (name, edit) in edits {
        edit(&mut world.ecs.get::<&mut Shield>(entity).unwrap(), false);
        assert_ne!(world.compute_checksum(), baseline, "{name} is not covered by the checksum");
        edit(&mut world.ecs.get::<&mut Shield>(entity).unwrap(), true);
        assert_eq!(world.compute_checksum(), baseline, "undoing {name} did not restore the checksum");
    }
}

/// An upgrade in place is hashed only while it is under way.
#[test]
fn every_upgrade_field_changes_the_checksum() {
    use sim_engine::sim::commands::SimCommand;
    use sim_engine::sim::components::{Team, Upgrading};
    use sim_engine::sim::blueprints::BlueprintId;
    let (_, mut world) = run(scenario::battle(SEED, 20, small_config()), 10, 10);
    let at = world.unit_position(world.units[0].0).unwrap().xy();
    let acu = world.blueprints.id("acu");
    let commander = world.spawn_mobile(acu, Team::Blue, at);
    world.advance_tick(&[SimCommand::Upgrade { units: vec![commander] }]);
    let entity = world.unit_entity(commander).unwrap();
    let baseline = world.compute_checksum();
    let bit = Res::from_bits(1);
    type Edit = fn(&mut Upgrading, bool, Res);
    let edits: [(&str, Edit); 2] = [
        ("upgrade target", |u, undo, _| u.into = BlueprintId(if undo { u.into.0 - 1 } else { u.into.0 + 1 })),
        ("upgrade progress", |u, undo, bit| u.progress += if undo { -bit } else { bit }),
    ];
    for (name, edit) in edits {
        edit(&mut world.ecs.get::<&mut Upgrading>(entity).unwrap(), false, bit);
        assert_ne!(world.compute_checksum(), baseline, "{name} is not covered by the checksum");
        edit(&mut world.ecs.get::<&mut Upgrading>(entity).unwrap(), true, bit);
        assert_eq!(world.compute_checksum(), baseline, "undoing {name} did not restore the checksum");
    }
}

/// Launchers, missile defenses, and missiles are hashed only where they exist.
#[test]
fn every_missile_field_changes_the_checksum() {
    use sim_engine::sim::commands::SimCommand;
    use sim_engine::sim::components::{Launcher, Missile, MissileDefense, Team};
    let (_, mut world) = run(scenario::battle(SEED, 20, small_config()), 10, 10);
    let place = |world: &mut SimWorld, key: &str| {
        let blueprint = world.blueprints.id(key);
        (4..200).find_map(|cell| world.spawn_finished_structure(blueprint, Team::Blue, cell, 8)).expect("room")
    };
    let launcher = place(&mut world, "tactical_missile_launcher");
    let defense = place(&mut world, "tactical_missile_defense");
    let target = world.unit_position(launcher).unwrap().xy() + Vec2Fp::new(Fp::from_num(300), Fp::ZERO);
    world.advance_tick(&[SimCommand::AttackGround { units: vec![launcher], point: target }]);
    let missile = world.missiles.first().expect("a missile in flight").1;
    let (launcher, defense) = (world.unit_entity(launcher).unwrap(), world.unit_entity(defense).unwrap());
    let baseline = world.compute_checksum();
    let bit = Fp::from_bits(1);
    type Edit = Box<dyn Fn(&mut SimWorld, bool)>;
    let edits: Vec<(&str, Edit)> = vec![
        ("launcher kind", Box::new(move |w, _| { let mut l = w.ecs.get::<&mut Launcher>(launcher).unwrap(); l.strategic = !l.strategic; })),
        ("cruise height", Box::new(move |w, undo| w.ecs.get::<&mut Launcher>(launcher).unwrap().cruise_height += if undo { -bit } else { bit })),
        ("launcher stock", Box::new(move |w, undo| { let mut l = w.ecs.get::<&mut Launcher>(launcher).unwrap(); l.stock = if undo { l.stock - 1 } else { l.stock + 1 }; })),
        ("defense kind", Box::new(move |w, _| { let mut d = w.ecs.get::<&mut MissileDefense>(defense).unwrap(); d.strategic = !d.strategic; })),
        ("defense range", Box::new(move |w, undo| w.ecs.get::<&mut MissileDefense>(defense).unwrap().range += if undo { -bit } else { bit })),
        ("defense reload", Box::new(move |w, undo| { let mut d = w.ecs.get::<&mut MissileDefense>(defense).unwrap(); d.reload_ticks = if undo { d.reload_ticks - 1 } else { d.reload_ticks + 1 }; })),
        ("defense timer", Box::new(move |w, undo| { let mut d = w.ecs.get::<&mut MissileDefense>(defense).unwrap(); d.ticks_until_ready = if undo { d.ticks_until_ready - 1 } else { d.ticks_until_ready + 1 }; })),
        ("defense stock", Box::new(move |w, undo| { let mut d = w.ecs.get::<&mut MissileDefense>(defense).unwrap(); d.stock = if undo { d.stock - 1 } else { d.stock + 1 }; })),
        ("missile target x", Box::new(move |w, undo| w.ecs.get::<&mut Missile>(missile).unwrap().target.x += if undo { -bit } else { bit })),
        ("missile target y", Box::new(move |w, undo| w.ecs.get::<&mut Missile>(missile).unwrap().target.y += if undo { -bit } else { bit })),
        ("missile speed", Box::new(move |w, undo| w.ecs.get::<&mut Missile>(missile).unwrap().speed += if undo { -bit } else { bit })),
        ("missile cruise height", Box::new(move |w, undo| w.ecs.get::<&mut Missile>(missile).unwrap().cruise_height += if undo { -bit } else { bit })),
        ("missile kind", Box::new(move |w, _| { let mut m = w.ecs.get::<&mut Missile>(missile).unwrap(); m.strategic = !m.strategic; })),
    ];
    for (name, edit) in edits {
        edit(&mut world, false);
        assert_ne!(world.compute_checksum(), baseline, "{name} is not covered by the checksum");
        edit(&mut world, true);
        assert_eq!(world.compute_checksum(), baseline, "undoing {name} did not restore the checksum");
    }
}

/// A formation's state is hashed only while there is one, so it has a test of its own.
#[test]
fn every_formation_field_changes_the_checksum() {
    let (_, mut world) = run(scenario::battle(SEED, 100, small_config()), 10, 10);
    let units: Vec<SimId> = world.units.iter().take(6).map(|unit| unit.0).collect();
    let goal = world.unit_position(units[0]).unwrap().xy();
    world.advance_tick(&[sim_engine::sim::commands::SimCommand::FormationMove { units, goal, attack: false, queued: false }]);
    let route = *world.formations.keys().next().expect("a formation");
    let baseline = world.compute_checksum();
    let one_bit = Fp::from_bits(1);
    type Edit = fn(&mut sim_engine::sim::formation::Travel, Fp);
    let edits: [(&str, Edit); 11] = [
        ("leader x", |t, d| t.leader.x += d),
        ("leader y", |t, d| t.leader.y += d),
        ("heading x", |t, d| t.heading.x += d),
        ("heading y", |t, d| t.heading.y += d),
        ("speed", |t, d| t.speed += d),
        ("max turn", |t, d| t.max_turn += d),
        ("lag distance", |t, d| t.lag_distance += d),
        ("catch-up distance", |t, d| t.catch_up_distance += d),
        ("held ticks", |t, d| t.held_ticks = t.held_ticks.wrapping_add(d.to_bits() as u32)),
        ("spot ahead", |t, d| t.members[0].1 += d),
        ("spot aside", |t, d| t.members[0].2 += d),
    ];
    for (name, edit) in edits {
        edit(world.formations.get_mut(&route).unwrap(), one_bit);
        assert_ne!(world.compute_checksum(), baseline, "{name} is not covered by the checksum");
        edit(world.formations.get_mut(&route).unwrap(), -one_bit);
        assert_eq!(world.compute_checksum(), baseline, "undoing {name} did not restore the checksum");
    }
    let member = world.formations.get_mut(&route).unwrap().members.pop().unwrap();
    assert_ne!(world.compute_checksum(), baseline, "membership is not covered by the checksum");
    world.formations.get_mut(&route).unwrap().members.push(member);
    let travel = world.formations.remove(&route).unwrap();
    assert_ne!(world.compute_checksum(), baseline, "the formation's route is not covered by the checksum");
    world.formations.insert(route, travel);
    assert_eq!(world.compute_checksum(), baseline);
}

/// A checksum that misses a field cannot detect a desync in that field.
#[test]
fn every_hashed_field_changes_the_checksum() {
    let (_, mut world) = run(scenario::battle(SEED, 100, small_config()), 60, 60);
    let baseline = world.compute_checksum();
    let (_, entity) = world.units[0];
    let one_bit = Fp::from_bits(1);

    let assert_changes = |name: &str, world: &mut SimWorld, change: &dyn Fn(&mut SimWorld), undo: &dyn Fn(&mut SimWorld)| {
        change(world);
        assert_ne!(world.compute_checksum(), baseline, "{name} is not covered by the checksum");
        undo(world);
        assert_eq!(world.compute_checksum(), baseline, "undoing {name} did not restore the checksum");
    };

    assert_changes(
        "position",
        &mut world,
        &|w| w.ecs.get::<&mut Position>(entity).unwrap().0.x += one_bit,
        &|w| w.ecs.get::<&mut Position>(entity).unwrap().0.x -= one_bit,
    );
    assert_changes(
        "veterancy",
        &mut world,
        &|w| w.ecs.insert_one(entity, Veterancy { experience: Res::ONE, rank: 1 }).unwrap(),
        &|w| {
            w.ecs.remove_one::<Veterancy>(entity).unwrap();
        },
    );
    assert_changes(
        "an overcharge order",
        &mut world,
        &|w| w.ecs.insert_one(entity, OverchargeOrder { target: SimId(0) }).unwrap(),
        &|w| {
            w.ecs.remove_one::<OverchargeOrder>(entity).unwrap();
        },
    );
    assert_changes(
        "paused",
        &mut world,
        &|w| w.ecs.insert_one(entity, Paused).unwrap(),
        &|w| {
            w.ecs.remove_one::<Paused>(entity).unwrap();
        },
    );
    assert_changes(
        "aim",
        &mut world,
        &|w| w.ecs.insert_one(entity, Turret::new(Vec2Fp::new(Fp::ONE, Fp::ZERO), 90)).unwrap(),
        &|w| {
            w.ecs.remove_one::<Turret>(entity).unwrap();
        },
    );
    assert_changes(
        "facing",
        &mut world,
        &|w| w.ecs.insert_one(entity, Turning::new(Vec2Fp::new(Fp::ONE, Fp::ZERO), 90, Fp::ZERO)).unwrap(),
        &|w| {
            w.ecs.remove_one::<Turning>(entity).unwrap();
        },
    );
    assert_changes(
        "queued orders",
        &mut world,
        &|w| w.ecs.insert_one(entity, OrderQueue(vec![Order { kind: OrderKind::Move, route: 1, slot: None, slot_ticks: 0 }])).unwrap(),
        &|w| {
            w.ecs.remove_one::<OrderQueue>(entity).unwrap();
        },
    );
    assert_changes(
        "velocity",
        &mut world,
        &|w| w.ecs.get::<&mut Velocity>(entity).unwrap().0.y += one_bit,
        &|w| w.ecs.get::<&mut Velocity>(entity).unwrap().0.y -= one_bit,
    );
    assert_changes(
        "health",
        &mut world,
        &|w| w.ecs.get::<&mut Health>(entity).unwrap().current += one_bit,
        &|w| w.ecs.get::<&mut Health>(entity).unwrap().current -= one_bit,
    );
    assert_changes(
        "reload timer",
        &mut world,
        &|w| w.ecs.get::<&mut Combat>(entity).unwrap().ticks_until_ready += 1,
        &|w| w.ecs.get::<&mut Combat>(entity).unwrap().ticks_until_ready -= 1,
    );
    // Nothing is blocked yet, which hashes as no target until tick 0. Each part must count alone.
    assert_eq!(world.ecs.get::<&Combat>(entity).unwrap().blocked, None);
    assert_changes(
        "blocked target",
        &mut world,
        &|w| w.ecs.get::<&mut Combat>(entity).unwrap().blocked = Some((SimId(7), 0)),
        &|w| w.ecs.get::<&mut Combat>(entity).unwrap().blocked = None,
    );
    assert_changes(
        "blocked until",
        &mut world,
        &|w| w.ecs.get::<&mut Combat>(entity).unwrap().blocked = Some((SimId(u32::MAX), 1)),
        &|w| w.ecs.get::<&mut Combat>(entity).unwrap().blocked = None,
    );
    for (name, spot) in [("ground target", Vec2Fp::ZERO), ("ground target x", Vec2Fp::new(one_bit, Fp::ZERO)), ("ground target y", Vec2Fp::new(Fp::ZERO, one_bit))] {
        assert_changes(
            name,
            &mut world,
            &move |w| w.ecs.get::<&mut Combat>(entity).unwrap().ground_target = Some(spot),
            &|w| w.ecs.get::<&mut Combat>(entity).unwrap().ground_target = None,
        );
    }
    let state = world.rng.state();
    assert_changes(
        "RNG state",
        &mut world,
        &|w| w.rng.set_state(state ^ 1),
        &|w| w.rng.set_state(state),
    );
    assert_changes(
        "tick",
        &mut world,
        &|w| w.current_tick += 1,
        &|w| w.current_tick -= 1,
    );
}

/// Hecs entity handles and archetype order must not influence results. Spawning and
/// despawning unrelated entities first changes every handle, but no simulation state.
#[test]
fn entity_handles_do_not_affect_results() {
    let (clean, _) = run(scenario::battle(SEED, 200, small_config()), 200, 200);

    let mut shuffled = scenario::battle(SEED, 200, small_config());
    let world = &mut shuffled.world;
    let mut fresh = hecs::World::new();
    let mut remapped = Vec::new();
    // Rebuild the units in a new ECS world in reverse order, with decoys in between.
    for &(id, entity) in world.units.iter().rev() {
        let mut builder = hecs::EntityBuilder::new();
        let source = world.ecs.entity(entity).unwrap();
        builder.add(id);
        builder.add(*source.get::<&Position>().unwrap());
        builder.add(*source.get::<&Velocity>().unwrap());
        builder.add(*source.get::<&sim_engine::sim::components::UnitStats>().unwrap());
        builder.add(*source.get::<&Health>().unwrap());
        builder.add(*source.get::<&Combat>().unwrap());
        builder.add(*source.get::<&sim_engine::sim::components::Order>().unwrap());
        let decoy = fresh.spawn((0u8,));
        let moved = fresh.spawn(builder.build());
        fresh.despawn(decoy).unwrap();
        remapped.push((id, moved));
    }
    remapped.reverse();
    world.ecs = fresh;
    world.units = remapped;

    let (reordered, _) = run(shuffled, 200, 200);
    assert_eq!(clean, reordered);
}
