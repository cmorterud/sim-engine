//! Every armed unit and structure can hurt what its weapons are for (GAME_SPECIFICATION.md §7):
//! ground weapons a target at their full range, from where they stand; bombers a target pass
//! after pass, without a pass that drops nothing, and a moving one too; anti-air weapons an
//! aircraft flying over; torpedoes ships and submarines. Checked against the real unit data, so
//! that a new unit, or a change to one, is covered.
//!
//! Every unit also deals the damage a second its weapons add up to, standing still and on the
//! move; aircraft that make passes fire whenever a target is in reach; and launchers land every
//! missile they are ordered to fire.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::{Blueprint, BlueprintId, Res, Targets, WeaponDef, WeaponKind};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Combat, Health, MoveClass, Projectile, SimId, Team, UnitStats};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::terrain::{Terrain, HEIGHT_FRAC_BITS};
use sim_engine::sim::world::SimWorld;

/// Dummies take this much before they would die, more than any test does to them.
const DUMMY_HEALTH: i32 = 400_000;
/// The lake, deep water, on every map: x and y from 1,000 to these.
const LAKE_X_END: i32 = 5_000;
const LAKE_Y_END: i32 = 9_000;
/// Where land units and structures stand, and where ships sail, on the usual map.
const LAND: (i32, i32) = (5_600, 5_000);
const SHORE: (i32, i32) = (4_880, 5_000);
const OPEN_WATER: (i32, i32) = (2_000, 5_000);
/// Most weapons fit on a 10 km map. Longer ones, from where they stand, need 20 km.
const MAP: i32 = 10_240;
const BIG_MAP: i32 = 20_480;

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

/// Flat land 20 up, with a lake 40 deep, and plenty of materiel and energy for both teams.
fn world(map: i32) -> SimWorld {
    let cells = (map / 20) as usize;
    let samples = cells + 1;
    let height = |x: usize, y: usize| {
        let (wx, wy) = (x as i32 * 20, y as i32 * 20);
        if (1_000..=LAKE_X_END).contains(&wx) && (1_000..=LAKE_Y_END).contains(&wy) { -40 } else { 20 }
    };
    let heights: Vec<i16> = (0..samples * samples).map(|index| (height(index % samples, index / samples) << HEIGHT_FRAC_BITS) as i16).collect();
    let blocked: Vec<bool> = (0..cells * cells)
        .map(|cell| {
            let (x, y) = (cell % cells, cell / cells);
            [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)].iter().any(|&(cx, cy)| height(cx, cy) < 0)
        })
        .collect();
    let terrain = Terrain::from_heights(map, heights, &blocked, Fp::ZERO);
    let config = SimConfig { map_units: map, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::with_terrain(config, 1, terrain);
    world.prepare_map();
    for economy in &mut world.economies {
        for value in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.base_energy_capacity, &mut economy.energy_capacity] {
            *value = Res::from_num(10_000_000);
        }
        economy.mass = Res::from_num(5_000_000);
        economy.energy = Res::from_num(5_000_000);
    }
    world
}

/// Places a unit, or a finished structure centred on the point.
fn place(world: &mut SimWorld, key: &str, team: Team, at: (i32, i32)) -> SimId {
    let blueprint = world.blueprints.id(key);
    let def = world.blueprints.get(blueprint);
    if def.is_structure() {
        let half = u32::from(def.footprint) / 2;
        let (cell_x, cell_y) = (at.0 as u32 / 20 - half, at.1 as u32 / 20 - half);
        world.spawn_finished_structure(blueprint, team, cell_x, cell_y).unwrap_or_else(|| panic!("no room for {key} at {at:?}"))
    } else {
        world.spawn_mobile(blueprint, team, point(at.0, at.1))
    }
}

/// A Red target that never shoots back and takes a great deal to destroy.
fn dummy(world: &mut SimWorld, key: &str, at: (i32, i32)) -> SimId {
    let id = place(world, key, Team::Red, at);
    let entity = world.unit_entity(id).unwrap();
    world.ecs.get::<&mut Combat>(entity).unwrap().enabled = false;
    let mut health = world.ecs.get::<&mut Health>(entity).unwrap();
    health.max = fp_int(DUMMY_HEALTH);
    health.current = health.max;
    id
}

fn damage(world: &SimWorld, id: SimId) -> i32 {
    DUMMY_HEALTH - world.unit_health(id).map_or(0, |health| health.to_num::<i32>())
}

/// What a weapon can be pointed at, and whether this test covers it.
fn covered(weapon: &WeaponDef) -> bool {
    match weapon.kind {
        WeaponKind::Shell | WeaponKind::Bomb | WeaponKind::Flak => true,
        WeaponKind::TacticalMissile => weapon.automatic,
        WeaponKind::StrategicMissile | WeaponKind::Tractor => false,
    }
}

fn weapons(def: &Blueprint) -> Vec<WeaponDef> {
    def.weapon.iter().chain(&def.extra_weapons).copied().filter(covered).collect()
}

fn hits(weapon: &WeaponDef, targets: Targets) -> bool {
    weapon.targets == targets || (weapon.targets == Targets::Both && targets != Targets::Naval)
}

fn sails(def: &Blueprint) -> bool {
    matches!(def.movement, MoveClass::Naval | MoveClass::Submarine) || def.on_water
}

/// Runs until the target has taken damage, or `ticks` have passed. Returns the ticks it took.
fn until_hurt(world: &mut SimWorld, target: SimId, ticks: u32) -> Option<u32> {
    (0..ticks).find(|_| {
        world.advance_tick(&[]);
        damage(world, target) > 0
    })
}

/// The units the test checks, by what they are armed to hit.
fn armed_with(targets: Targets, bombs: bool) -> Vec<(String, Blueprint)> {
    let world = world(MAP);
    world
        .blueprints
        .iter()
        .filter(|def| weapons(def).iter().any(|weapon| hits(weapon, targets) && (weapon.kind == WeaponKind::Bomb) == bombs))
        .map(|def| (def.key.clone(), def.clone()))
        .collect()
}

/// Every weapon that hits the ground hits a target at its full range from where the unit stands,
/// without closing in: land units and structures a land unit, ships both a land unit across the
/// shore and a ship. Aircraft, which fly to what they attack, are only checked for hitting it.
#[test]
fn every_ground_weapon_hits_a_target_at_its_full_range() {
    let mut failures = Vec::new();
    for (key, def) in armed_with(Targets::Ground, false) {
        let reach = weapons(&def).iter().filter(|weapon| hits(weapon, Targets::Ground) && weapon.kind != WeaponKind::Bomb).map(|weapon| weapon.range.to_num::<i32>()).max().unwrap();
        let mut cases = Vec::new();
        if def.is_air() {
            cases.push(("t1_engineer", LAND, (LAND.0 + 1_500, LAND.1)));
        } else if sails(&def) {
            cases.push(("t1_engineer", SHORE, (SHORE.0 + reach, SHORE.1)));
            cases.push(("t1_frigate", OPEN_WATER, (OPEN_WATER.0, OPEN_WATER.1 + reach.min(LAKE_Y_END - OPEN_WATER.1 - 200))));
        } else {
            cases.push(("t1_engineer", LAND, (LAND.0 + reach, LAND.1)));
        }
        for (target_key, from, to) in cases {
            let big = to.0 > MAP - 200;
            // Beyond the lake, at the map's west edge, facing east.
            let (from, to) = if big { ((600, 10_000), (600 + reach, 10_000)) } else { (from, to) };
            let mut world = world(if big { BIG_MAP } else { MAP });
            let shooter = place(&mut world, &key, Team::Blue, from);
            let target = dummy(&mut world, target_key, to);
            // Submarines surface to use a deck gun.
            if def.movement == MoveClass::Submarine {
                world.advance_tick(&[SimCommand::SetSurfaced { units: vec![shooter], surfaced: true }]);
            }
            if !def.is_structure() {
                world.advance_tick(&[SimCommand::Attack { units: vec![shooter], target }]);
            }
            let start = world.unit_position(shooter).unwrap().xy();
            let hurt = until_hurt(&mut world, target, 1_500);
            let Some(end) = world.unit_position(shooter) else {
                failures.push(format!("{key} was destroyed attacking a {target_key}"));
                continue;
            };
            let moved = (end.xy() - start).length_squared().to_num::<f64>().sqrt();
            if hurt.is_none() {
                failures.push(format!("{key} did no damage to a {target_key} {} away", reach));
            } else if !def.is_air() && moved > 30.0 {
                failures.push(format!("{key} moved {moved:.0} to hit a {target_key} at its full range of {reach}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A bomber attacking a target makes pass after pass over it, and lets a bomb go on every pass
/// once it has reloaded, each of them landing on the target: a still one, and one driving across
/// its path, which bombs have to be aimed ahead of.
#[test]
fn every_bomber_bombs_on_every_pass_and_hits() {
    let mut failures = Vec::new();
    for (key, def) in armed_with(Targets::Ground, true) {
        let bomb = weapons(&def).into_iter().find(|weapon| weapon.kind == WeaponKind::Bomb).unwrap();
        for moving in [false, true] {
            let mut world = world(MAP);
            let shooter = place(&mut world, &key, Team::Blue, LAND);
            let at = (LAND.0 + 2_000, LAND.1);
            let target = dummy(&mut world, "t1_medium_tank", at);
            if moving {
                world.advance_tick(&[SimCommand::Patrol { units: vec![target], points: vec![point(at.0, 1_000), point(at.0, 9_000)], queued: false }]);
            }
            world.advance_tick(&[SimCommand::Attack { units: vec![shooter], target }]);
            let throw = sim_engine::sim::air::bomb_throw(def.flight.unwrap().elevation, def.speed).to_num::<f64>();
            let (mut bombs, mut seen) = (0u32, std::collections::BTreeSet::new());
            let (mut passes, mut bombs_at_last_pass, mut skipped) = (0u32, 0u32, 0u32);
            let mut last_distance = f64::MAX;
            let mut closing = false;
            for _ in 0..3_000 {
                world.advance_tick(&[]);
                for &(id, entity) in &world.projectiles {
                    if world.ecs.get::<&Projectile>(entity).is_ok_and(|projectile| projectile.source == shooter) && seen.insert(id) {
                        bombs += 1;
                    }
                }
                // A pass is the closest it comes to the target, within a bomb's throw.
                let here = world.unit_position(shooter).unwrap().xy();
                let there = world.unit_position(target).unwrap().xy();
                let distance = (here - there).length_squared().to_num::<f64>().sqrt();
                if closing && distance > last_distance && last_distance < throw {
                    passes += 1;
                    // Every pass after the first two, by which it is flying and reloaded, drops.
                    if passes > 2 && bombs == bombs_at_last_pass {
                        skipped += 1;
                    }
                    bombs_at_last_pass = bombs;
                }
                closing = distance < last_distance;
                last_distance = distance;
            }
            let landed = damage(&world, target) as f64 / bomb.damage.to_num::<f64>();
            let what = if moving { "a moving target" } else { "a target standing still" };
            if passes < 4 || bombs < 3 {
                failures.push(format!("{key} made {passes} passes and dropped {bombs} bombs on {what}"));
            }
            if skipped > 0 {
                failures.push(format!("{key} dropped nothing on {skipped} of {passes} passes over {what}"));
            }
            // A bomb that lands within its blast of the target does all its damage.
            if landed < f64::from(bombs) * 0.75 {
                failures.push(format!("{key} landed {landed:.1} of {bombs} bombs on {what}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Every anti-air weapon hits an aircraft flying back and forth over it, or that it is sent
/// after if it flies itself.
#[test]
fn every_anti_air_weapon_hits_an_aircraft_flying_over() {
    let mut failures = Vec::new();
    for (key, def) in armed_with(Targets::Air, false) {
        let from = if sails(&def) { OPEN_WATER } else { LAND };
        let mut world = world(MAP);
        let shooter = place(&mut world, &key, Team::Blue, from);
        let target = dummy(&mut world, "t1_air_scout", (from.0, from.1 - 1_500));
        world.advance_tick(&[SimCommand::Patrol { units: vec![target], points: vec![point(from.0, from.1 - 1_500), point(from.0, from.1 + 1_500)], queued: false }]);
        if !def.is_structure() {
            world.advance_tick(&[SimCommand::Attack { units: vec![shooter], target }]);
        }
        if until_hurt(&mut world, target, 1_500).is_none() {
            failures.push(format!("{key} did no damage to an aircraft flying over"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Every torpedo weapon hits a ship at its full range, and a submarine under the water.
#[test]
fn every_torpedo_hits_ships_and_submarines() {
    let mut failures = Vec::new();
    for (key, def) in armed_with(Targets::Naval, false) {
        let reach = weapons(&def).iter().filter(|weapon| hits(weapon, Targets::Naval)).map(|weapon| weapon.range.to_num::<i32>()).max().unwrap();
        for target_key in ["t1_frigate", "t1_attack_sub"] {
            let mut world = world(MAP);
            // Aircraft fly out over the lake to it; the rest are on the water already.
            let from = if def.is_air() { LAND } else { OPEN_WATER };
            let shooter = place(&mut world, &key, Team::Blue, from);
            let to = if def.is_air() { (OPEN_WATER.0 + 400, OPEN_WATER.1) } else { (OPEN_WATER.0, OPEN_WATER.1 + reach) };
            let target = dummy(&mut world, target_key, to);
            if !def.is_structure() {
                world.advance_tick(&[SimCommand::Attack { units: vec![shooter], target }]);
            }
            if until_hurt(&mut world, target, 1_500).is_none() {
                failures.push(format!("{key} did no damage to a {target_key} {reach} away"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// What a dummy target is, which decides which weapons count against it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Kind {
    /// A unit on land: ground weapons.
    Land,
    /// A ship on the water: ground weapons and torpedoes.
    Ship,
    /// A submarine under the water: torpedoes.
    Sub,
    /// An aircraft in flight: anti-air.
    Air,
}

impl Kind {
    const fn dummy(self) -> &'static str {
        match self {
            Self::Land => "t1_engineer",
            Self::Ship => "t1_frigate",
            Self::Sub => "t1_attack_sub",
            Self::Air => "t1_air_scout",
        }
    }
}

/// Whether a weapon fires at this kind of target. Under the water, only torpedoes and missiles
/// fire. Bombs are left to the bomber test, since a bomber's damage comes once a pass.
fn counts(weapon: &WeaponDef, kind: Kind, submerged: bool) -> bool {
    let fires = match kind {
        Kind::Land => hits(weapon, Targets::Ground),
        Kind::Ship => hits(weapon, Targets::Ground) || weapon.targets == Targets::Naval,
        Kind::Sub => weapon.targets == Targets::Naval,
        Kind::Air => hits(weapon, Targets::Air),
    };
    let missile = weapon.kind == WeaponKind::TacticalMissile;
    fires && weapon.kind != WeaponKind::Bomb && (!submerged || weapon.targets == Targets::Naval || missile)
}

/// Damage a tick, every shot landing.
fn per_tick(weapons: &[WeaponDef]) -> f64 {
    weapons.iter().map(|weapon| weapon.damage.to_num::<f64>() / f64::from(weapon.reload_ticks.max(1))).sum()
}

/// One damage-a-second case: where the shooter and target start, and whether they move.
struct Setting {
    what: &'static str,
    map: i32,
    shooter: (i32, i32),
    target: (i32, i32),
    /// Both go north at this speed, a tick, for a moving case. An aircraft target always flies
    /// north, at this speed or else slowly enough to stay in reach.
    speed: Option<Fp>,
}

/// How far ahead, north, a moving unit's goal is kept: less than a pathfinding sector, 32 cells,
/// so that it walks straight at it rather than towards the middle of the next sector's edge.
const GOAL_AHEAD: i32 = 400;

/// Every unit does the damage a second its weapons add up to, to each kind of target they are
/// for, over a stretch of time long enough for its slowest weapon to fire eight times or more:
/// standing still at a target standing still (for anti-air, an aircraft flying slowly past), and
/// moving north at a constant speed beside a target doing the same, so that every shot has to be
/// aimed ahead. That a unit does all of its damage means that every one of its weapons fires, at
/// its rate, and lands: a battleship's three guns at a ship, and its anti-air at an aircraft.
///
/// Artillery scatters its shells on purpose, so its target is made wide enough to take every one:
/// what is checked is how often it fires and what each shell does. Cruise missiles fly to where
/// the target was, so are not counted on against a moving one, though they may hit it. Aircraft
/// that make passes, rather than hovering, deal their damage a pass at a time, and bombers are
/// left to the tests above; structures only stand still.
#[test]
fn every_unit_deals_its_damage_a_second_to_each_kind_of_target() {
    let mut failures = Vec::new();
    let mut checked = 0;
    let db = world(MAP).blueprints;
    for def in db.iter() {
        let all = weapons(def);
        let flies = def.is_air();
        let hovers = def.flight.is_some_and(|flight| flight.hovers);
        if all.is_empty() || (flies && !hovers) {
            continue;
        }
        let at_sea = sails(def);
        let submarine = def.movement == MoveClass::Submarine;
        for kind in [Kind::Land, Kind::Ship, Kind::Sub, Kind::Air] {
            // Submarines fight ships and submarines under the water, and surface for the land.
            let submerged = submarine && kind != Kind::Land;
            let counted: Vec<WeaponDef> = all.iter().copied().filter(|weapon| counts(weapon, kind, submerged)).collect();
            // Land units are not set against ships, nor aircraft against submarines.
            if counted.is_empty() || ((kind == Kind::Ship || kind == Kind::Sub) && !at_sea) {
                continue;
            }
            let reach = counted.iter().map(|weapon| weapon.range.to_num::<i32>()).min().unwrap();
            let least = counted.iter().map(|weapon| weapon.min_range.to_num::<i32>()).max().unwrap();
            let slowest = counted.iter().map(|weapon| weapon.reload_ticks).max().unwrap();
            let window = (slowest * 8).max(300);
            let warm_up = 600;
            let length = if at_sea { 7_000 } else { 8_500 };
            // Slow enough to keep going north for the whole case.
            let moving_speed = def.speed.min(Fp::from_num(length) / Fp::from_num(window + warm_up));
            let mut settings = Vec::new();
            if kind == Kind::Air {
                // The aircraft flies north, from well within reach, beside a line through the shooter.
                let (x, y) = if at_sea { (2_000, 1_500) } else { (LAND.0, 1_000) };
                let shooter = (x, y + reach * 7 / 10);
                settings.push(Setting { what: "standing still, at an aircraft flying past", map: MAP, shooter, target: (x + reach * 3 / 10, y), speed: None });
                if !def.is_structure() && !flies {
                    settings.push(Setting { what: "moving, beside an aircraft flying alongside", map: MAP, shooter: (x, y), target: (x + reach / 2, y), speed: Some(moving_speed) });
                }
            } else {
                let distance = (reach * 85 / 100).max(least + 20);
                if at_sea && kind == Kind::Land {
                    // From just off the shore, at a unit on the land.
                    settings.push(Setting { what: "standing still, at a target standing still", map: MAP, shooter: SHORE, target: (SHORE.0 + distance, SHORE.1), speed: None });
                } else if at_sea {
                    // On the lake, as far as it allows.
                    let distance = distance.min(LAKE_X_END - 1_300 - 300);
                    settings.push(Setting { what: "standing still, at a target standing still", map: MAP, shooter: (1_300, 5_000), target: (1_300 + distance, 5_000), speed: None });
                    if !def.is_structure() {
                        settings.push(Setting { what: "moving, beside a target moving alongside", map: MAP, shooter: (1_300, 1_500), target: (1_300 + distance, 1_500), speed: Some(moving_speed) });
                    }
                } else if LAND.0 + distance > MAP - 300 {
                    settings.push(Setting { what: "standing still, at a target standing still", map: BIG_MAP, shooter: (600, 10_000), target: (600 + distance, 10_000), speed: None });
                } else {
                    settings.push(Setting { what: "standing still, at a target standing still", map: MAP, shooter: LAND, target: (LAND.0 + distance, LAND.1), speed: None });
                    // Artillery that sets up never fires on the move (artillery_test.rs).
                    if !def.is_structure() && def.weapon.is_none_or(|weapon| weapon.setup_ticks == 0) {
                        settings.push(Setting { what: "moving, beside a target moving alongside", map: MAP, shooter: (LAND.0, 1_000), target: (LAND.0 + distance, 1_000), speed: Some(moving_speed) });
                    }
                }
            }
            for setting in settings {
                let moving = setting.speed.is_some();
                // What is sure to land, and what may.
                let sure: Vec<WeaponDef> = counted.iter().copied().filter(|weapon| !(moving && weapon.kind == WeaponKind::TacticalMissile)).collect();
                if sure.is_empty() {
                    continue;
                }
                checked += 1;
                let name = format!("{} {} ({:?})", def.key, setting.what, kind);
                let mut world = world(setting.map);
                let shooter = place(&mut world, &def.key, Team::Blue, setting.shooter);
                let target = dummy(&mut world, kind.dummy(), setting.target);
                let target_entity = world.unit_entity(target).unwrap();
                // Wide enough to take every scattered shell. A wide unit far off on the shooter's
                // own side makes shells look that far around them for something to hit.
                let spread = counted.iter().map(|weapon| weapon.spread).max().unwrap();
                if spread > Fp::ZERO && kind != Kind::Air {
                    let wide = spread + fp_int(4);
                    world.ecs.get::<&mut UnitStats>(target_entity).unwrap().radius = wide;
                    let mut spawn = sim_engine::sim::scenario::standard_unit(Team::Blue, point(setting.map - 200, setting.map - 200));
                    spawn.radius = wide;
                    world.spawn_unit(spawn);
                }
                let mut commands = Vec::new();
                if submarine && kind == Kind::Land {
                    commands.push(SimCommand::SetSurfaced { units: vec![shooter], surfaced: true });
                }
                if kind == Kind::Air {
                    // Already flying north, at the speed it is to keep.
                    let speed = setting.speed.unwrap_or_else(|| Fp::from_num(reach * 14 / 10) / Fp::from_num(window + warm_up));
                    world.ecs.get::<&mut UnitStats>(target_entity).unwrap().max_speed = speed;
                    world.ecs.get::<&mut sim_engine::sim::components::Flight>(target_entity).unwrap().speed = speed;
                    world.ecs.get::<&mut sim_engine::sim::components::Turning>(target_entity).unwrap().facing = Vec2Fp::new(Fp::ZERO, Fp::ONE);
                } else if let Some(speed) = setting.speed {
                    world.ecs.get::<&mut UnitStats>(target_entity).unwrap().max_speed = speed;
                }
                if let (Some(speed), false) = (setting.speed, flies) {
                    world.ecs.get::<&mut UnitStats>(world.unit_entity(shooter).unwrap()).unwrap().max_speed = speed;
                } else if !def.is_structure() {
                    commands.push(SimCommand::Attack { units: vec![shooter], target });
                }
                // Moving units are kept going north, the target always, the shooter if it walks.
                let drive = |world: &SimWorld| -> Vec<SimCommand> {
                    let mut orders = Vec::new();
                    let mut ahead = |id: SimId, x: i32| {
                        if let Some(at) = world.unit_position(id) {
                            orders.push(SimCommand::Move { units: vec![id], goal: point(x, at.y.to_num::<i32>() + GOAL_AHEAD) });
                        }
                    };
                    if moving || kind == Kind::Air {
                        ahead(target, setting.target.0);
                    }
                    if moving && !flies {
                        ahead(shooter, setting.shooter.0);
                    }
                    orders
                };
                commands.extend(drive(&world));
                world.advance_tick(&commands);
                let mut tick = 0;
                let mut step = |world: &mut SimWorld| {
                    tick += 1;
                    let orders = if tick % 10 == 0 { drive(world) } else { Vec::new() };
                    world.advance_tick(&orders);
                };
                let mut started = false;
                for _ in 0..warm_up {
                    step(&mut world);
                    if damage(&world, target) > 0 {
                        started = true;
                        break;
                    }
                }
                if !started {
                    failures.push(format!("{name}: no damage within {warm_up} ticks"));
                    continue;
                }
                let before = damage(&world, target);
                for _ in 0..window {
                    step(&mut world);
                }
                let done = f64::from(damage(&world, target) - before);
                let (low, high) = (per_tick(&sure) * f64::from(window), per_tick(&counted) * f64::from(window));
                let volley: f64 = counted.iter().map(|weapon| weapon.damage.to_num::<f64>()).sum();
                if done < low * 0.92 - volley || done > high * 1.08 + volley {
                    let seconds = f64::from(window) / 10.0;
                    failures.push(format!("{name}: {:.1} damage a second, expected {:.1} (over {seconds} s)", done / seconds, low / seconds));
                }
            }
        }
    }
    assert!(checked >= 60, "only {checked} cases were checked");
    assert!(failures.is_empty(), "{} of {checked} cases:\n{failures:#?}", failures.len());
}

/// The test covers what it should: a unit of each kind, so that a mistake in the filters above
/// cannot quietly test nothing.
#[test]
fn the_weapon_tests_cover_each_kind_of_unit() {
    let keys = |targets, bombs| armed_with(targets, bombs).into_iter().map(|(key, _)| key).collect::<Vec<_>>();
    let ground = keys(Targets::Ground, false);
    for key in ["t1_medium_tank", "t1_light_artillery", "t2_artillery", "t3_artillery", "t1_point_defense", "t1_frigate", "t3_battleship", "t2_gunship", "command_rig", "t2_mobile_missile_launcher"] {
        assert!(ground.contains(&key.to_string()), "{key} should be checked against ground targets");
    }
    assert!(keys(Targets::Ground, true).contains(&"t2_fighter_bomber".to_string()));
    for key in ["t1_mobile_aa", "t1_aa_turret", "t1_interceptor", "t2_fighter_bomber", "t2_destroyer"] {
        assert!(keys(Targets::Air, false).contains(&key.to_string()), "{key} should be checked against aircraft");
    }
    for key in ["t1_attack_sub", "t2_torpedo_bomber", "t1_torpedo_launcher"] {
        assert!(keys(Targets::Naval, false).contains(&key.to_string()), "{key} should be checked against ships");
    }
    let _ = BlueprintId(0);
}

/// Keeps an aircraft in the air for as long as a test runs.
fn endless_fuel(world: &mut SimWorld, id: SimId) {
    let entity = world.unit_entity(id).unwrap();
    if let Ok(mut flight) = world.ecs.get::<&mut sim_engine::sim::components::Flight>(entity) {
        flight.max_fuel = u32::MAX / 2;
        flight.fuel = flight.max_fuel;
    }
}

/// Aircraft that make passes fire whenever a weapon is reloaded and its target in reach: over a
/// long stretch of passes at a target moving in a straight line, each weapon does its damage for
/// every reload's worth of time its target was within its range. Interceptors and air superiority
/// fighters at an aircraft, bombers' own guns at one, and torpedo bombers at a ship.
#[test]
fn every_aircraft_that_makes_passes_fires_whenever_its_target_is_in_reach() {
    let mut failures = Vec::new();
    let mut checked = 0;
    let db = world(MAP).blueprints;
    for def in db.iter().filter(|def| def.is_air() && !def.flight.is_some_and(|flight| flight.hovers)) {
        for kind in [Kind::Air, Kind::Ship, Kind::Land] {
            let counted: Vec<WeaponDef> = weapons(def).into_iter().filter(|weapon| counts(weapon, kind, false)).collect();
            if counted.is_empty() {
                continue;
            }
            checked += 1;
            let name = format!("{} ({kind:?})", def.key);
            let mut world = world(MAP);
            let (from, to) = match kind {
                Kind::Ship => (LAND, (2_500, 1_500)),
                _ => ((LAND.0, 2_000), (LAND.0 + 1_500, 1_000)),
            };
            let shooter = place(&mut world, &def.key, Team::Blue, from);
            let target = dummy(&mut world, kind.dummy(), to);
            endless_fuel(&mut world, shooter);
            endless_fuel(&mut world, target);
            let target_entity = world.unit_entity(target).unwrap();
            let speed = if kind == Kind::Air { fp_int(4) } else { fp_int(2) };
            world.ecs.get::<&mut UnitStats>(target_entity).unwrap().max_speed = speed;
            if kind == Kind::Air {
                world.ecs.get::<&mut sim_engine::sim::components::Flight>(target_entity).unwrap().speed = speed;
                world.ecs.get::<&mut sim_engine::sim::components::Turning>(target_entity).unwrap().facing = Vec2Fp::new(Fp::ZERO, Fp::ONE);
            }
            let ahead = |world: &SimWorld| -> Vec<SimCommand> {
                world.unit_position(target).map(|at| SimCommand::Move { units: vec![target], goal: point(to.0, at.y.to_num::<i32>() + GOAL_AHEAD) }).into_iter().collect()
            };
            let mut first = ahead(&world);
            first.push(SimCommand::Attack { units: vec![shooter], target });
            world.advance_tick(&first);
            let target_radius = world.ecs.get::<&UnitStats>(target_entity).unwrap().radius.to_num::<f64>();
            let mut in_reach = vec![0u32; counted.len()];
            let ticks = 1_500;
            for tick in 0..ticks {
                world.advance_tick(&if tick % 10 == 0 { ahead(&world) } else { Vec::new() });
                let (Some(here), Some(there)) = (world.unit_position(shooter), world.unit_position(target)) else { break };
                let apart = (here.xy() - there.xy()).length_squared().to_num::<f64>().sqrt();
                for (count, weapon) in in_reach.iter_mut().zip(&counted) {
                    if apart <= weapon.range.to_num::<f64>() + target_radius {
                        *count += 1;
                    }
                }
            }
            let expected: f64 = counted.iter().zip(&in_reach).map(|(weapon, &count)| weapon.damage.to_num::<f64>() * f64::from(count) / f64::from(weapon.reload_ticks.max(1))).sum();
            let done = f64::from(damage(&world, target));
            if expected < 1.0 {
                failures.push(format!("{name}: its target was never in reach"));
            // A pass's first and last ticks in reach may or may not give a shot, so a little over
            // or under is in order.
            } else if done < expected * 0.8 || done > expected * 1.3 {
                failures.push(format!("{name}: {done:.0} damage, expected {expected:.0} from {in_reach:?} ticks in reach"));
            }
        }
    }
    assert!(checked >= 5, "only {checked} cases were checked");
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Launchers that fire only the missiles they have built land each one they are ordered to fire,
/// for its full damage: the tactical missile launcher's three, one after another at one order,
/// and a nuclear missile for each of three orders from the strategic launcher and the missile
/// submarine.
#[test]
fn every_missile_launcher_lands_each_missile_it_is_ordered_to_fire() {
    let mut failures = Vec::new();
    let mut checked = 0;
    let db = world(MAP).blueprints;
    for def in db.iter() {
        let Some(weapon) = def.weapon.filter(|weapon| matches!(weapon.kind, WeaponKind::TacticalMissile | WeaponKind::StrategicMissile) && !weapon.automatic) else {
            continue;
        };
        checked += 1;
        let strategic = weapon.kind == WeaponKind::StrategicMissile;
        let reach = weapon.range.to_num::<i32>();
        let at_sea = sails(def);
        let (map, from, distance) = if at_sea { (MAP, (1_300, 5_000), (reach * 85 / 100).min(MAP - 1_300 - 600)) } else { (BIG_MAP, (600, 10_000), (reach * 85 / 100).min(BIG_MAP - 1_200)) };
        let mut world = world(map);
        let shooter = place(&mut world, &def.key, Team::Blue, from);
        let target = dummy(&mut world, "t1_engineer", (from.0 + distance, from.1));
        let missiles = 3;
        world.ecs.get::<&mut sim_engine::sim::components::Launcher>(world.unit_entity(shooter).unwrap()).unwrap().stock = missiles;
        let per_missile = weapon.damage.to_num::<f64>() + weapon.inner_damage.to_num::<f64>();
        let flight = f64::from(distance) / weapon.projectile_speed.to_num::<f64>();
        let wait = (flight * 2.0) as u32 + 300;
        let orders = if strategic { missiles } else { 1 };
        for _ in 0..orders {
            world.advance_tick(&[SimCommand::Attack { units: vec![shooter], target }]);
            for _ in 0..wait {
                world.advance_tick(&[]);
            }
        }
        if !strategic {
            for _ in 0..(weapon.reload_ticks * missiles + wait) {
                world.advance_tick(&[]);
            }
        }
        let done = f64::from(damage(&world, target));
        let expected = per_missile * f64::from(missiles);
        if (done - expected).abs() > expected * 0.01 {
            failures.push(format!("{}: {done:.0} damage from {missiles} missiles, expected {expected:.0}", def.key));
        }
    }
    assert!(checked >= 3, "only {checked} launchers were checked");
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The weapons that can be aimed at a spot on the ground, for checking where their blasts reach.
fn ground_attack_weapons(def: &Blueprint) -> Vec<WeaponDef> {
    weapons(def).into_iter().filter(|weapon| hits(weapon, Targets::Ground) && weapon.kind != WeaponKind::Bomb).collect()
}

/// Nothing is hurt by a unit or structure standing idle with it just beyond the reach of every
/// weapon that could hit it: on land a unit there, on the water a ship.
#[test]
fn nothing_is_hit_beyond_a_weapons_range() {
    let mut failures = Vec::new();
    let mut checked = 0;
    let db = world(MAP).blueprints;
    for def in db.iter().filter(|def| !def.is_air()) {
        let at_sea = sails(def);
        for kind in [Kind::Land, Kind::Ship] {
            let counted: Vec<WeaponDef> = weapons(def).into_iter().filter(|weapon| counts(weapon, kind, false)).collect();
            if counted.is_empty() || (kind == Kind::Ship && !at_sea) || (kind == Kind::Land && at_sea) {
                continue;
            }
            let reach = counted.iter().map(|weapon| weapon.range.to_num::<i32>()).max().unwrap();
            let (map, from) = if at_sea { (MAP, (1_300, 5_000)) } else if LAND.0 + reach + 400 > MAP { (BIG_MAP, (600, 10_000)) } else { (MAP, LAND) };
            if at_sea && from.0 + reach + 200 > LAKE_X_END {
                continue;
            }
            checked += 1;
            let mut world = world(map);
            let shooter = place(&mut world, &def.key, Team::Blue, from);
            let radius = world.blueprints.get(world.blueprints.id(kind.dummy())).radius.to_num::<i32>();
            let probe = dummy(&mut world, kind.dummy(), (from.0 + reach + radius + 40, from.1));
            for _ in 0..300 {
                world.advance_tick(&[]);
            }
            let fired = world.projectiles.iter().any(|&(_, entity)| world.ecs.get::<&Projectile>(entity).is_ok_and(|projectile| projectile.source == shooter));
            if damage(&world, probe) > 0 || fired {
                failures.push(format!("{} fired at a {kind:?} target {} beyond its {reach} range", def.key, radius + 40));
            }
        }
    }
    assert!(checked >= 25, "only {checked} cases were checked");
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Artillery cannot hit what is closer than its minimum range: told to attack a target there, it
/// does it no harm.
#[test]
fn artillery_cannot_hit_inside_its_minimum_range() {
    let mut failures = Vec::new();
    let mut checked = 0;
    let db = world(MAP).blueprints;
    for def in db.iter() {
        let counted = ground_attack_weapons(def);
        if counted.is_empty() || counted.iter().any(|weapon| weapon.min_range == Fp::ZERO) {
            continue;
        }
        checked += 1;
        let least = counted.iter().map(|weapon| weapon.min_range.to_num::<i32>()).min().unwrap();
        let mut world = world(MAP);
        let shooter = place(&mut world, &def.key, Team::Blue, LAND);
        let target = dummy(&mut world, Kind::Land.dummy(), (LAND.0 + least * 6 / 10, LAND.1));
        if !def.is_structure() {
            world.advance_tick(&[SimCommand::Attack { units: vec![shooter], target }]);
            // It may back off to fire; held in place, it must not fire at all.
            world.ecs.get::<&mut UnitStats>(world.unit_entity(shooter).unwrap()).unwrap().max_speed = Fp::ZERO;
        }
        for _ in 0..400 {
            world.advance_tick(&[]);
        }
        if damage(&world, target) > 0 {
            failures.push(format!("{} hurt a target {} away, inside its {least} minimum range", def.key, least * 6 / 10));
        }
    }
    assert!(checked >= 4, "only {checked} cases were checked");
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A blast reaches as far as its splash radius, plus the radius of what it reaches: told to fire
/// at a spot on the ground, every weapon with a blast hurts a unit just inside that reach of the
/// spot, its own side's too, and never one just outside it, allowing for a scattering weapon's
/// spread.
#[test]
fn every_blast_reaches_exactly_its_splash_radius() {
    let mut failures = Vec::new();
    let mut checked = 0;
    let db = world(MAP).blueprints;
    for def in db.iter().filter(|def| !def.is_air()) {
        let counted: Vec<WeaponDef> = ground_attack_weapons(def).into_iter().filter(|weapon| weapon.splash_radius > Fp::ZERO).collect();
        if counted.is_empty() || ground_attack_weapons(def).len() != counted.len() {
            continue;
        }
        let reach = counted.iter().map(|weapon| weapon.range.to_num::<i32>()).min().unwrap();
        let least = counted.iter().map(|weapon| weapon.min_range.to_num::<i32>()).max().unwrap();
        let distance = (reach * 7 / 10).max(least + 50);
        let at_sea = sails(def);
        let (map, from) = if at_sea { (MAP, SHORE) } else if LAND.0 + distance + 600 > MAP { (BIG_MAP, (600, 10_000)) } else { (MAP, LAND) };
        checked += 1;
        let smallest = counted.iter().map(|weapon| weapon.splash_radius.to_num::<i32>()).min().unwrap();
        let widest = counted.iter().map(|weapon| (weapon.splash_radius + weapon.spread).to_num::<i32>()).max().unwrap();
        let scatters = counted.iter().any(|weapon| weapon.spread > Fp::ZERO);
        let spot = (from.0 + distance, from.1);
        let mut world = world(map);
        let shooter = place(&mut world, &def.key, Team::Blue, from);
        // A friendly unit just inside the smallest blast, beside the spot, and an enemy just
        // outside the widest, on the other side.
        let inside = dummy(&mut world, Kind::Land.dummy(), spot);
        let inner_radius = world.ecs.get::<&UnitStats>(world.unit_entity(inside).unwrap()).unwrap().radius.to_num::<i32>();
        world.ecs.get::<&mut UnitStats>(world.unit_entity(inside).unwrap()).unwrap().team = Team::Blue;
        let outside = dummy(&mut world, Kind::Land.dummy(), spot);
        let outer_radius = world.ecs.get::<&UnitStats>(world.unit_entity(outside).unwrap()).unwrap().radius.to_num::<i32>();
        world.ecs.get::<&mut sim_engine::sim::components::Position>(world.unit_entity(inside).unwrap()).unwrap().0.y = fp_int(spot.1 + smallest + inner_radius - 3);
        world.ecs.get::<&mut sim_engine::sim::components::Position>(world.unit_entity(outside).unwrap()).unwrap().0.y = fp_int(spot.1 - widest - outer_radius - 6);
        // The friendly one is not a target, and the enemy one is not shot at directly.
        world.ecs.get::<&mut Combat>(world.unit_entity(shooter).unwrap()).unwrap().blocked = Some((outside, u32::MAX));
        world.advance_tick(&[SimCommand::AttackGround { units: vec![shooter], point: point(spot.0, spot.1) }]);
        let slowest = counted.iter().map(|weapon| weapon.reload_ticks).max().unwrap();
        for _ in 0..(slowest * 4 + 400) {
            world.advance_tick(&[]);
        }
        if !scatters && damage(&world, inside) == 0 {
            failures.push(format!("{}: a friend {} from the spot, inside its {smallest} blast, took nothing", def.key, smallest + inner_radius - 3));
        }
        if damage(&world, outside) > 0 {
            failures.push(format!("{}: a unit {} from the spot, beyond its {widest} blast and spread, was hurt", def.key, widest + outer_radius + 6));
        }
    }
    assert!(checked >= 8, "only {checked} cases were checked");
    assert!(failures.is_empty(), "{failures:#?}");
}
