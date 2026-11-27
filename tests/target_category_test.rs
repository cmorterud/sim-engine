//! Every weapon picks, and lands its shots on, only the kinds of unit it is for
//! (GAME_SPECIFICATION.md §7): anti-air only aircraft in flight, torpedoes only ships and
//! submarines, and ground weapons what is on the land or the water, never an aircraft in flight
//! nor a submarine or seabed walker under the water. A tractor beam lifts only what moves on land.
//!
//! Each weapon of each unit is checked on its own, the unit's other weapons taken off or made
//! harmless, so that one weapon cannot hide another's mistake. The cases come from the real unit
//! data, so a new weapon, or a change to one, is covered without touching this file.
//!
//! - **Targeting**: with only what it must not hit in reach, a weapon picks none of it and fires
//!   nothing, whether its unit is told to attack one, or has one forced on it as its target.
//! - **Hits**: with one of every kind of unit packed together by the shore, a submarine and a
//!   seabed walker right under a ship, and aircraft held still right over the ship and over a
//!   tank, a weapon set on each thing it may hit in turn hurts it, and does no damage at all to
//!   anything it must not hit: not by its blast, a shell passing through, a burst of flak, nor a
//!   torpedo.

use std::sync::Arc;

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::{Blueprint, BlueprintId, Res, Targets, WeaponDef, WeaponKind};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{
    Combat, ExtraWeapons, Flight, Health, MoveClass, Projectile, SimId, Team, UnitStats,
};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::terrain::{Terrain, HEIGHT_FRAC_BITS};
use sim_engine::sim::world::SimWorld;

/// Dummies take this much before they would die, more than any test does to them.
const DUMMY_HEALTH: i32 = 400_000;
/// Deep water, 40 down, for x from 1,000 to `SHORE_X` and y from 1,000 to 9,000; land 20 up elsewhere.
const SHORE_X: i32 = 5_000;
/// Where the units to shoot at are packed, by the shore, and the line through it, north and
/// south along the shore, on which shooters stand.
const CLUSTER_Y: i32 = 1_500;
const MAP: i32 = 20_480;

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

fn world() -> SimWorld {
    let cells = (MAP / 20) as usize;
    let samples = cells + 1;
    let height = |x: usize, y: usize| {
        let (wx, wy) = (x as i32 * 20, y as i32 * 20);
        if (1_000..=SHORE_X).contains(&wx) && (1_000..=9_000).contains(&wy) {
            -40
        } else {
            20
        }
    };
    let heights: Vec<i16> = (0..samples * samples)
        .map(|index| (height(index % samples, index / samples) << HEIGHT_FRAC_BITS) as i16)
        .collect();
    let blocked: Vec<bool> = (0..cells * cells)
        .map(|cell| {
            let (x, y) = (cell % cells, cell / cells);
            [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)]
                .iter()
                .any(|&(cx, cy)| height(cx, cy) < 0)
        })
        .collect();
    let terrain = Terrain::from_heights(MAP, heights, &blocked, Fp::ZERO);
    let config = SimConfig {
        map_units: MAP,
        terrain: TerrainStyle::Flat,
        fog_of_war: false,
        ..SimConfig::default()
    };
    let mut world = SimWorld::with_terrain(config, 1, terrain);
    world.prepare_map();
    for economy in &mut world.economies {
        for value in [
            &mut economy.base_mass_capacity,
            &mut economy.mass_capacity,
            &mut economy.base_energy_capacity,
            &mut economy.energy_capacity,
        ] {
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
        world
            .spawn_finished_structure(blueprint, team, cell_x, cell_y)
            .unwrap_or_else(|| panic!("no room for {key} at {at:?}"))
    } else {
        world.spawn_mobile(blueprint, team, point(at.0, at.1))
    }
}

fn health(world: &SimWorld, id: SimId) -> i32 {
    world
        .unit_health(id)
        .map_or(0, |health| health.to_num::<i32>())
}

/// Keeps an aircraft in the air for as long as a test runs.
fn endless_fuel(world: &mut SimWorld, id: SimId) {
    let entity = world.unit_entity(id).unwrap();
    if let Ok(mut flight) = world.ecs.get::<&mut Flight>(entity) {
        // Far more than a test lasts, yet small enough that fuel shares cannot overflow.
        flight.max_fuel = 1_000_000;
        flight.fuel = flight.max_fuel;
    }
}

/// Every kind of unit a weapon may or may not be for.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Kind {
    /// A tank on land.
    Land,
    /// A hovercraft out on the water.
    Hover,
    /// A structure on land.
    Structure,
    /// A structure standing in the water, as a sonar.
    WaterStructure,
    /// A ship.
    Ship,
    /// A submarine that has surfaced, which sails as a ship.
    SurfacedSub,
    /// A submarine under the water.
    Sub,
    /// A unit walking the seabed, under the water.
    Seabed,
    /// An aircraft in flight.
    Air,
}

/// Where things are packed, by the shore: the ship, with what is under and over it.
const SHIP: (i32, i32) = (4_880, CLUSTER_Y);
/// The tank, just up the shore, with an aircraft over it.
const TANK: (i32, i32) = (5_060, CLUSTER_Y);

/// The dummies to shoot at: their kind, the unit standing for it, and where it is put.
const DUMMIES: [(Kind, &str, (i32, i32)); 10] = [
    (Kind::Land, "t1_medium_tank", TANK),
    (Kind::Air, "t1_air_scout", TANK),
    (Kind::Structure, "power_generator", (5_170, CLUSTER_Y + 20)),
    (Kind::Ship, "t1_frigate", SHIP),
    (Kind::Sub, "t1_attack_sub", SHIP),
    (Kind::Seabed, "field_rig", SHIP),
    (Kind::Air, "t1_air_scout", SHIP),
    (Kind::Hover, "t1_engineer", (4_820, CLUSTER_Y + 40)),
    (Kind::WaterStructure, "t1_sonar", (4_780, CLUSTER_Y - 60)),
    (Kind::SurfacedSub, "t1_attack_sub", (4_900, CLUSTER_Y - 100)),
];

/// What a weapon is told to attack in turn, of what it may hit: things that something it must
/// not hit is right beside, under, or over.
const BAITS: [Kind; 5] = [Kind::Land, Kind::Ship, Kind::Hover, Kind::Air, Kind::Sub];

/// What a weapon is for: whether it may pick, and hit, a unit of this kind.
fn may_hit(weapon: &WeaponDef, kind: Kind) -> bool {
    use Kind::*;
    if weapon.kind == WeaponKind::Tractor {
        return matches!(kind, Land | Hover);
    }
    let ground = matches!(
        kind,
        Land | Hover | Structure | WaterStructure | Ship | SurfacedSub
    );
    match weapon.targets {
        Targets::Ground => ground,
        Targets::Air => kind == Air,
        Targets::Both => ground || kind == Air,
        Targets::Naval => matches!(kind, Ship | SurfacedSub | Sub | Seabed),
    }
}

/// The weapons checked: all that pick their own targets. Missiles fired only when ordered, from
/// missiles built for them, are left to missile_test.rs.
fn covered(weapon: &WeaponDef) -> bool {
    match weapon.kind {
        WeaponKind::Shell
        | WeaponKind::Bomb
        | WeaponKind::Flak
        | WeaponKind::Laser
        | WeaponKind::Tractor => true,
        WeaponKind::TacticalMissile => weapon.automatic,
        WeaponKind::StrategicMissile => false,
    }
}

/// One weapon of one unit, to be tried on its own.
struct Case {
    key: String,
    blueprint: BlueprintId,
    /// Which of the unit's weapons: the main one, or an extra one by its place in the list.
    extra: Option<usize>,
    weapon: WeaponDef,
}

impl Case {
    fn name(&self) -> String {
        match self.extra {
            None => format!(
                "{} (main {:?} at {:?})",
                self.key, self.weapon.kind, self.weapon.targets
            ),
            Some(index) => format!(
                "{} (extra #{index} {:?} at {:?})",
                self.key, self.weapon.kind, self.weapon.targets
            ),
        }
    }

    /// The unit with only this weapon, as `weapon` has it: the main one with its extras taken off, or one extra with
    /// the others taken off and the main one made harmless. The main one stays, as a unit with no
    /// main weapon fires none of its extras, and so that the unit moves and closes in as it would.
    fn alone(&self, world: &mut SimWorld) {
        let (extra, weapon) = (self.extra, self.weapon);
        world.blueprints = Arc::new(world.blueprints.with_unit(
            self.blueprint,
            |unit: &mut Blueprint| match extra {
                None => {
                    unit.extra_weapons.clear();
                    unit.weapon = Some(weapon);
                }
                Some(_) => {
                    let keep = weapon;
                    if let Some(main) = unit.weapon.as_mut() {
                        (
                            main.damage,
                            main.splash_radius,
                            main.inner_damage,
                            main.stun_ticks,
                        ) = (Fp::ZERO, Fp::ZERO, Fp::ZERO, 0);
                    }
                    unit.extra_weapons = vec![keep];
                }
            },
        ));
    }
}

fn cases() -> Vec<Case> {
    let world = world();
    let mut cases = Vec::new();
    for (index, def) in world.blueprints.iter().enumerate() {
        let blueprint = BlueprintId(index as u16);
        assert_eq!(world.blueprints.get(blueprint).key, def.key);
        if let Some(weapon) = def.weapon.filter(covered) {
            cases.push(Case {
                key: def.key.clone(),
                blueprint,
                extra: None,
                weapon,
            });
        }
        for (extra, weapon) in def
            .extra_weapons
            .iter()
            .enumerate()
            .filter(|(_, weapon)| covered(weapon))
        {
            cases.push(Case {
                key: def.key.clone(),
                blueprint,
                extra: Some(extra),
                weapon: *weapon,
            });
        }
    }
    cases
}

/// Where the shooter starts: north of the cluster along the shore, on its side of it, far
/// enough for artillery's least range and near enough for its reach. Aircraft start inland
/// and fly to what they are told to attack.
fn shooter_spot(def: &Blueprint, weapon: &WeaponDef) -> (i32, i32) {
    let range = weapon.range.to_num::<i32>();
    let least = weapon.min_range.to_num::<i32>();
    let distance = (range / 2).max(least + 150).min(range - 60);
    if def.is_air() {
        (6_500, CLUSTER_Y + 1_500)
    } else if matches!(def.movement, MoveClass::Naval | MoveClass::Submarine) || def.on_water {
        // Ships keep to the lake, which ends 9,000 north.
        (4_700, CLUSTER_Y + distance.min(7_300))
    } else {
        (5_140, CLUSTER_Y + distance)
    }
}

/// Every shot the shooter has in flight that can do damage: none of a harmless main weapon's.
fn shots_from(world: &SimWorld, shooter: SimId) -> usize {
    world
        .projectiles
        .iter()
        .filter(|&&(_, entity)| {
            world
                .ecs
                .get::<&Projectile>(entity)
                .is_ok_and(|projectile| {
                    projectile.source == shooter && projectile.damage > Fp::ZERO
                })
        })
        .count()
}

/// The target the weapon being checked has chosen: the main weapon's, or the extra one's.
fn chosen(world: &SimWorld, case: &Case, shooter: SimId) -> Option<SimId> {
    let entity = world.unit_entity(shooter)?;
    match case.extra {
        None => world
            .ecs
            .get::<&Combat>(entity)
            .ok()
            .and_then(|combat| combat.target),
        Some(_) => world
            .ecs
            .get::<&ExtraWeapons>(entity)
            .ok()
            .and_then(|extras| extras.0[0].target),
    }
}

/// Sets the shooter's target, as an attack order does, bypassing the order's own check of what
/// its weapons are for, so that each weapon's own check is what is tested.
fn force_target(world: &mut SimWorld, shooter: SimId, target: SimId) {
    if let Some(entity) = world.unit_entity(shooter) {
        world.ecs.get::<&mut Combat>(entity).unwrap().forced_target = Some(target);
    }
}

/// Whether every aircraft dummy is still in flight.
fn all_flying(world: &SimWorld, dummies: &[(Kind, SimId)]) -> bool {
    dummies
        .iter()
        .filter(|(kind, _)| *kind == Kind::Air)
        .all(|&(_, id)| {
            world.unit_entity(id).is_some_and(|entity| {
                world
                    .ecs
                    .get::<&Flight>(entity)
                    .is_ok_and(|flight| flight.airborne())
            })
        })
}

fn hurt(world: &SimWorld, id: SimId) -> bool {
    health(world, id) < DUMMY_HEALTH
}

/// The world for one case: the shooter, Blue, with only the one weapon, and Red dummies, those
/// of `DUMMIES` that `keep` lets through, that never fire back, take a great deal to destroy,
/// and stay where they are put, aircraft too, which hang in the air. Returns the shooter and
/// each dummy with its kind.
fn set_up(case: &Case, keep: impl Fn(Kind) -> bool) -> (SimWorld, SimId, Vec<(Kind, SimId)>) {
    let mut world = world();
    case.alone(&mut world);
    let def = world.blueprints.get(case.blueprint).clone();
    let shooter = place(
        &mut world,
        &case.key,
        Team::Blue,
        shooter_spot(&def, &case.weapon),
    );
    endless_fuel(&mut world, shooter);
    let mut dummies = Vec::new();
    let mut surface = Vec::new();
    for (kind, key, at) in DUMMIES.into_iter().filter(|&(kind, _, _)| keep(kind)) {
        let id = place(&mut world, key, Team::Red, at);
        let entity = world.unit_entity(id).unwrap();
        world.ecs.get::<&mut Combat>(entity).unwrap().enabled = false;
        let mut health = world.ecs.get::<&mut Health>(entity).unwrap();
        health.max = fp_int(DUMMY_HEALTH);
        health.current = health.max;
        drop(health);
        endless_fuel(&mut world, id);
        if let Ok(mut stats) = world.ecs.get::<&mut UnitStats>(entity) {
            stats.max_speed = Fp::ZERO;
        }
        if let Ok(mut flight) = world.ecs.get::<&mut Flight>(entity) {
            flight.speed = Fp::ZERO;
        }
        if kind == Kind::SurfacedSub {
            surface.push(id);
        }
        dummies.push((kind, id));
    }
    // A submarine with a deck gun surfaces to use it.
    if def.movement == MoveClass::Submarine
        && case.weapon.targets != Targets::Naval
        && case.weapon.kind != WeaponKind::TacticalMissile
    {
        surface.push(shooter);
    }
    // Left idle over land, an aircraft lands, and a landed one is rightly hit by what hits the
    // ground: on patrol, with no speed, it hangs where it is.
    let mut commands = vec![SimCommand::SetSurfaced {
        units: surface,
        surfaced: true,
    }];
    for &(kind, id) in &dummies {
        if kind == Kind::Air {
            let at = world.unit_position(id).unwrap().xy();
            commands.push(SimCommand::Patrol {
                units: vec![id],
                points: vec![at, at + point(0, 50)],
                queued: false,
            });
        }
    }
    world.advance_tick(&commands);
    (world, shooter, dummies)
}

/// Long enough for the slowest aircraft to fly to the cluster and make passes, and for the
/// slowest weapon to fire several times.
const TICKS: u32 = 1_200;

/// With nothing in reach that it is for, a weapon picks nothing and fires nothing: told to attack
/// each of them in turn, which its unit refuses, and then with each forced on it as its target.
#[test]
fn no_weapon_targets_what_it_is_not_for() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for case in cases() {
        if DUMMIES
            .iter()
            .all(|&(kind, _, _)| may_hit(&case.weapon, kind))
        {
            continue;
        }
        checked += 1;
        let name = case.name();
        let (mut world, shooter, dummies) = set_up(&case, |kind| !may_hit(&case.weapon, kind));
        let mobile = !world.blueprints.get(case.blueprint).is_structure();
        let (mut fired, mut picked, mut landed_aircraft) = (0, Vec::new(), false);
        let turn = TICKS / (2 * dummies.len() as u32);
        for tick in 0..TICKS {
            let (index, forced) = (
                (tick / turn) as usize % dummies.len(),
                tick / turn >= dummies.len() as u32,
            );
            let target = dummies[index].1;
            let mut commands = Vec::new();
            if forced {
                force_target(&mut world, shooter, target);
            } else if mobile && tick % turn == 0 {
                commands.push(SimCommand::Attack {
                    units: vec![shooter],
                    target,
                });
            }
            world.advance_tick(&commands);
            landed_aircraft |= !all_flying(&world, &dummies);
            fired = fired.max(shots_from(&world, shooter));
            if let Some(&(kind, _)) = chosen(&world, &case, shooter)
                .and_then(|target| dummies.iter().find(|(_, id)| *id == target))
            {
                if !picked.contains(&kind) {
                    picked.push(kind);
                }
            }
        }
        if landed_aircraft {
            failures.push(format!("{name}: an aircraft landed"));
        }
        if !picked.is_empty() {
            failures.push(format!("{name} picked as its target: {picked:?}"));
        }
        if fired > 0 {
            failures.push(format!(
                "{name} fired {fired} shots with nothing it is for in reach"
            ));
        }
        let hurt: Vec<Kind> = dummies
            .iter()
            .filter(|&&(_, id)| hurt(&world, id))
            .map(|&(kind, _)| kind)
            .collect();
        if !hurt.is_empty() {
            failures.push(format!("{name} hurt {hurt:?}"));
        }
    }
    // 59 since the Strategic Bomber lost its railguns (2026-11-26).
    assert!(checked >= 59, "only {checked} weapons were checked");
    assert!(
        failures.is_empty(),
        "{} of {checked} weapons:\n{failures:#?}",
        failures.len()
    );
}

/// Among one of every kind of unit, packed together so that its blasts, passing shells, flak and
/// torpedoes all have a chance at what they must not hit, a weapon set on each thing it may hit
/// in turn hurts it, and does no damage at all to anything it is not for.
/// Sets the weapon on each thing it may hit in turn, among one of every kind of unit, and
/// returns how many cases that made, and what went wrong in them.
fn check_hits(case: &Case, failures: &mut Vec<String>) -> usize {
    let mut checked = 0;
    let name = case.name();
    let baits: Vec<usize> = DUMMIES
        .iter()
        .enumerate()
        .filter(|(_, (kind, _, _))| BAITS.contains(kind) && may_hit(&case.weapon, *kind))
        .map(|(index, _)| index)
        .collect();
    if baits.is_empty() {
        failures.push(format!("{name} is for none of {BAITS:?}"));
    }
    for bait in baits {
        checked += 1;
        let (mut world, shooter, dummies) = set_up(case, |_| true);
        let (bait_kind, bait_id) = dummies[bait];
        let what = format!("{name}, set on the {bait_kind:?} at {:?}", DUMMIES[bait].2);
        if !world.blueprints.get(case.blueprint).is_structure() {
            world.advance_tick(&[SimCommand::Attack {
                units: vec![shooter],
                target: bait_id,
            }]);
        }
        let (mut picked, mut landed_aircraft, mut since_hit) = (Vec::new(), false, None);
        for _ in 0..TICKS {
            force_target(&mut world, shooter, bait_id);
            world.advance_tick(&[]);
            landed_aircraft |= !all_flying(&world, &dummies);
            if let Some(&(kind, _)) = chosen(&world, case, shooter)
                .and_then(|target| dummies.iter().find(|(_, id)| *id == target))
            {
                if !may_hit(&case.weapon, kind) && !picked.contains(&kind) {
                    picked.push(kind);
                }
            }
            // A few more shots after the first lands, for their blasts to spread.
            if hurt(&world, bait_id) {
                since_hit = Some(since_hit.map_or(0, |ticks: u32| ticks + 1));
            }
            if since_hit.is_some_and(|ticks| ticks > 4 * case.weapon.reload_ticks.max(10)) {
                break;
            }
        }
        let wrongly: Vec<Kind> = dummies
            .iter()
            .filter(|&&(kind, id)| !may_hit(&case.weapon, kind) && hurt(&world, id))
            .map(|&(kind, _)| kind)
            .collect();
        if landed_aircraft {
            failures.push(format!("{what}: an aircraft landed"));
        }
        if since_hit.is_none() {
            failures.push(format!("{what}: it never hurt it"));
        }
        if !picked.is_empty() {
            failures.push(format!("{what}: it picked {picked:?}"));
        }
        if !wrongly.is_empty() {
            failures.push(format!("{what}: it hurt {wrongly:?}, which it is not for"));
        }
    }
    checked
}

#[test]
fn no_weapon_hits_what_it_is_not_for() {
    let mut failures = Vec::new();
    let checked: usize = cases()
        .iter()
        .map(|case| check_hits(case, &mut failures))
        .sum();
    assert!(checked >= 120, "only {checked} cases were checked");
    assert!(
        failures.is_empty(),
        "{} of {checked} cases:\n{failures:#?}",
        failures.len()
    );
}

/// A torpedo given a blast, as none has yet, bursts on the ship or submarine it reaches and hurts
/// only ships and submarines round it: not an aircraft over it, as a steering shot's burst at an
/// aircraft hurts only aircraft, nor a hovercraft beside it.
#[test]
fn a_torpedo_with_a_blast_hurts_only_ships_and_submarines() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for mut case in cases()
        .into_iter()
        .filter(|case| case.weapon.targets == Targets::Naval)
    {
        case.weapon.splash_radius = fp_int(150);
        checked += check_hits(&case, &mut failures);
    }
    assert!(checked >= 10, "only {checked} cases were checked");
    assert!(
        failures.is_empty(),
        "{} of {checked} cases:\n{failures:#?}",
        failures.len()
    );
}

/// The dummies are what they stand for, and stay where they are put, so that the tests above
/// check what they say: each is on the land, on the water, under it, or in the air, as its kind is.
#[test]
fn each_dummy_is_its_kind_and_stays_put() {
    let case = cases()
        .into_iter()
        .find(|case| case.key == "t1_medium_tank")
        .unwrap();
    let (mut world, _, dummies) = set_up(&case, |_| true);
    for _ in 0..TICKS {
        world.advance_tick(&[]);
    }
    let water = world.terrain.water_level();
    for (index, (kind, id)) in dummies.into_iter().enumerate() {
        let entity = world.unit_entity(id).unwrap();
        let at = world.unit_position(id).unwrap();
        let ground = world.terrain.height_at(at.xy());
        let flying = world
            .ecs
            .get::<&Flight>(entity)
            .is_ok_and(|flight| flight.airborne());
        let surfaced = world.surfaced.binary_search(&id).is_ok();
        let radius = world.ecs.get::<&UnitStats>(entity).unwrap().radius;
        let ok = match kind {
            Kind::Land | Kind::Structure => ground > water && !flying,
            Kind::Hover | Kind::WaterStructure | Kind::Ship => ground < water,
            Kind::SurfacedSub => ground < water && surfaced,
            Kind::Sub => ground < water && !surfaced,
            Kind::Seabed => at.z + radius < water,
            Kind::Air => flying,
        };
        assert!(
            ok,
            "the {kind:?} dummy is not one: at {at:?}, ground {ground:?}"
        );
        let put = DUMMIES[index].2;
        let moved = (at.xy() - point(put.0, put.1))
            .length_squared()
            .to_num::<f64>()
            .sqrt();
        assert!(
            moved < 40.0,
            "the {kind:?} dummy moved {moved:.0} from where it was put"
        );
    }
}
