//! Turns a right-click into orders, depending on what is selected and what is under the cursor,
//! and checks structure placement. Works from snapshots, so that it respects fog of war.

use super::selection::{nearest_unit, order_command};
use crate::math::fp::{Fp, Vec2Fp};
use crate::render::camera::{Camera, HeightField};
use crate::sim::blueprints::{BlueprintDb, BlueprintId, UnitClass, WeaponKind};
use crate::sim::commands::SimCommand;
use crate::sim::components::{BuildTask, SimId, Team};
use crate::sim::config::TERRAIN_CELL_UNITS;
use crate::sim::world::{SimSnapshot, UnitView, WreckMotion, WreckView};

const RAW_TO_WORLD: f64 = 1.0 / 4096.0;
/// How close, in pixels, a click must be to a wreck to reclaim it.
const WRECK_CLICK_PX: f64 = 16.0;

/// How the selection breaks down.
struct SelectionParts {
    builders: Vec<SimId>,
    armed: Vec<SimId>,
    mobile: Vec<SimId>,
    factories: Vec<SimId>,
}

fn sort_selection(selected: &[SimId], units: &[UnitView], blueprints: &BlueprintDb) -> SelectionParts {
    let mut parts = SelectionParts { builders: Vec::new(), armed: Vec::new(), mobile: Vec::new(), factories: Vec::new() };
    for id in selected {
        let Ok(index) = units.binary_search_by_key(id, |unit| unit.id) else {
            continue;
        };
        let unit = &units[index];
        if unit.blueprint == u16::MAX {
            parts.mobile.push(*id);
            parts.armed.push(*id);
            continue;
        }
        let blueprint = blueprints.get(BlueprintId(unit.blueprint));
        // A missile launcher fires only on order, so a right-click on an enemy is one.
        if blueprint.weapon.is_some_and(|weapon| !matches!(weapon.kind, WeaponKind::Shell | WeaponKind::Flak)) {
            parts.armed.push(*id);
        }
        // A factory that moves, the missile submarine, which builds its own missiles, is a
        // ship to orders: a right-click moves it, as a rally point would mean nothing.
        if blueprint.is_factory() && blueprint.is_structure() {
            parts.factories.push(*id);
            continue;
        }
        if blueprint.is_structure() {
            continue;
        }
        parts.mobile.push(*id);
        if blueprint.weapon.is_some() {
            parts.armed.push(*id);
        }
        // Its build power builds its missiles; it does not help with work elsewhere.
        if blueprint.build_power > Fp::ZERO && !blueprint.is_factory() {
            parts.builders.push(*id);
        }
    }
    parts
}

/// Everything a right-click needs to know.
pub struct RightClick<'a> {
    pub selected: &'a [SimId],
    pub units: &'a [UnitView],
    pub wrecks: &'a [WreckView],
    pub blueprints: &'a BlueprintDb,
    pub team: Team,
    pub camera: &'a Camera,
    pub cursor: [f64; 2],
    pub ground: Option<[f64; 2]>,
    pub attack_move: bool,
    /// Enemy contacts on radar, which can be attacked as if seen.
    pub contacts: &'a [crate::sim::world::ContactView],
    /// Submarines on the surface, which guns reach as well as torpedoes.
    pub surfaced: &'a [SimId],
    /// The water's height, if the map has any: a wreck on the sea floor is picked from the slick
    /// on the surface over it, as well as where it lies.
    pub water: Option<f64>,
}

/// What a target is, which decides what weapons reach it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TargetKind {
    /// On land, or a structure.
    Ground,
    Air,
    /// A ship, or a submarine on the surface: guns and torpedoes.
    Surface,
    /// A submarine under the water: torpedoes.
    Submerged,
}

/// What an enemy unit in sight is.
pub fn unit_target_kind(unit: &UnitView, blueprints: &BlueprintDb, surfaced: &[SimId]) -> TargetKind {
    if unit.blueprint == u16::MAX {
        return TargetKind::Ground;
    }
    let def = blueprints.get(BlueprintId(unit.blueprint));
    match def.movement {
        _ if def.is_air() => TargetKind::Air,
        crate::sim::components::MoveClass::Submarine if surfaced.binary_search(&unit.id).is_err() => TargetKind::Submerged,
        crate::sim::components::MoveClass::Submarine | crate::sim::components::MoveClass::Naval if !def.is_structure() => TargetKind::Surface,
        _ => TargetKind::Ground,
    }
}

/// What a radar contact is.
pub fn contact_target_kind(contact: &crate::sim::world::ContactView) -> TargetKind {
    if contact.is_air {
        TargetKind::Air
    } else if contact.is_sub {
        TargetKind::Submerged
    } else if contact.is_naval {
        TargetKind::Surface
    } else {
        TargetKind::Ground
    }
}

/// Whether any of a unit's weapons reaches this kind of target.
pub fn weapons_reach(def: &crate::sim::blueprints::Blueprint, kind: TargetKind) -> bool {
    use crate::sim::blueprints::Targets;
    def.weapon.iter().chain(&def.extra_weapons).any(|weapon| match kind {
        TargetKind::Air => matches!(weapon.targets, Targets::Air | Targets::Both),
        TargetKind::Submerged => weapon.targets == Targets::Naval,
        TargetKind::Surface => matches!(weapon.targets, Targets::Ground | Targets::Both | Targets::Naval),
        TargetKind::Ground => matches!(weapon.targets, Targets::Ground | Targets::Both),
    })
}

/// Whether a unit fires missiles only on order, from a stock it builds and stows: the tactical
/// and strategic missile launchers and the missile submarine. Cruise missiles that pick their own
/// targets, as a ship's do, are not.
pub fn launches_on_order(def: &crate::sim::blueprints::Blueprint) -> bool {
    def.max_stock > 0 && def.weapon.is_some_and(|weapon| matches!(weapon.kind, WeaponKind::TacticalMissile | WeaponKind::StrategicMissile) && !weapon.automatic)
}

/// Whether this unit fires missiles only on order.
pub fn is_launcher(unit: &UnitView, blueprints: &BlueprintDb) -> bool {
    unit.blueprint != u16::MAX && launches_on_order(blueprints.get(BlueprintId(unit.blueprint)))
}

/// Whether a unit could fire at a spot, `radius` short of it: anything that can move goes into
/// range, so only a structure's reach counts. A launcher that cannot reach is left out of an
/// order rather than told to wait on a target it will never fire at.
fn reaches_from_where_it_stands(unit: &UnitView, blueprints: &BlueprintDb, point: [f64; 2], radius: f64) -> bool {
    if unit.blueprint == u16::MAX || !unit.is_structure {
        return true;
    }
    let raw = |value: i32| f64::from(value) / 4096.0;
    let range = blueprints.get(BlueprintId(unit.blueprint)).weapon.map_or(0.0, |weapon| weapon.range.to_num::<f64>());
    (raw(unit.x) - point[0]).hypot(raw(unit.y) - point[1]) <= range + radius
}

/// Drops the selected missile launchers that stand where they cannot reach the spot.
fn without_launchers_out_of_reach(ids: &mut Vec<SimId>, click: &RightClick<'_>, point: [f64; 2], radius: f64) {
    ids.retain(|id| {
        click.units.binary_search_by_key(id, |unit| unit.id).ok().is_none_or(|index| {
            let unit = &click.units[index];
            !is_launcher(unit, click.blueprints) || reaches_from_where_it_stands(unit, click.blueprints, point, radius)
        })
    });
}

/// The enemy under the cursor, in sight or on radar, and what it is: a unit, or a contact's
/// position when it names none (a decoy, which looks the same).
pub enum Hovered {
    Unit(SimId, TargetKind),
    Contact(Option<SimId>, [f64; 2], TargetKind),
}

pub fn hovered_enemy(click: &RightClick<'_>) -> Option<Hovered> {
    let reach = crate::input::selection::TARGET_CLICK_RADIUS_PX;
    if let Some(id) = crate::input::selection::nearest_unit_within(click.units, click.camera, click.team.enemy(), click.cursor, reach) {
        let unit = &click.units[click.units.binary_search_by_key(&id, |unit| unit.id).ok()?];
        return Some(Hovered::Unit(id, unit_target_kind(unit, click.blueprints, click.surfaced)));
    }
    let raw = |value: i32| f64::from(value) / 4096.0;
    click
        .contacts
        .iter()
        .filter_map(|contact| {
            let on_screen = click.camera.world_to_screen([raw(contact.x), raw(contact.y), raw(contact.z)])?;
            let distance = (on_screen[0] - click.cursor[0]).hypot(on_screen[1] - click.cursor[1]);
            (distance <= reach).then_some((distance, contact))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, contact)| Hovered::Contact(contact.unit, [raw(contact.x), raw(contact.y)], contact_target_kind(contact)))
}

/// Whether any selected unit has a weapon that reaches this kind of target.
pub fn selection_reaches(click: &RightClick<'_>, kind: TargetKind) -> bool {
    click.selected.iter().any(|id| {
        click.units.binary_search_by_key(id, |unit| unit.id).ok().is_some_and(|index| {
            let unit = &click.units[index];
            unit.blueprint != u16::MAX && weapons_reach(click.blueprints.get(BlueprintId(unit.blueprint)), kind)
        })
    })
}

/// The selected structures with guns, which fire at what they choose rather than on order, that
/// reach `target` and can hit its kind.
fn defenses_in_reach(click: &RightClick<'_>, target: &UnitView, kind: TargetKind) -> Vec<SimId> {
    let raw = |value: i32| f64::from(value) / 4096.0;
    click
        .selected
        .iter()
        .filter_map(|id| click.units.binary_search_by_key(id, |unit| unit.id).ok().map(|index| &click.units[index]))
        .filter(|unit| unit.blueprint != u16::MAX)
        .filter(|unit| {
            let def = click.blueprints.get(BlueprintId(unit.blueprint));
            let guns = def.weapon.is_some_and(|weapon| matches!(weapon.kind, WeaponKind::Shell | WeaponKind::Flak));
            let reach = def.weapon.map_or(0.0, |weapon| weapon.range.to_num::<f64>()) + raw(target.radius);
            let apart = (raw(unit.x) - raw(target.x)).hypot(raw(unit.y) - raw(target.y));
            def.is_structure() && !def.is_factory() && guns && weapons_reach(def, kind) && apart <= reach
        })
        .map(|unit| unit.id)
        .collect()
}

/// Works out the orders for a right-click: attack what is under the cursor, help with
/// friendly work, reclaim debris, or otherwise move there.
pub fn right_click_orders(click: &RightClick<'_>) -> Vec<SimCommand> {
    let parts = sort_selection(click.selected, click.units, click.blueprints);
    let mut commands = Vec::new();

    // Attack-move always means attack-move, even with the cursor on a unit.
    if click.attack_move {
        if let (Some(ground), false) = (click.ground, parts.mobile.is_empty()) {
            commands.push(order_command(&parts.mobile, ground, true));
        }
        return commands;
    }

    // On an enemy, armed units attack, and builders that cannot fight capture it.
    let enemy = crate::input::selection::nearest_unit_within(click.units, click.camera, click.team.enemy(), click.cursor, crate::input::selection::TARGET_CLICK_RADIUS_PX);
    // A contact on radar, with nothing in sight there: attacked as if seen. A decoy, which looks
    // the same, is gone to, to find nothing.
    if enemy.is_none() && !parts.armed.is_empty() {
        if let Some(Hovered::Contact(unit, at, _)) = hovered_enemy(click) {
            let mut armed = parts.armed.clone();
            without_launchers_out_of_reach(&mut armed, click, at, 0.0);
            match unit {
                Some(target) if !armed.is_empty() => commands.push(SimCommand::Attack { units: armed, target }),
                Some(_) => {}
                // Missile launchers fire at a decoy's spot, as they would at what it looks like.
                None => {
                    if !parts.mobile.is_empty() {
                        commands.push(order_command(&parts.mobile, at, true));
                    }
                    let point = Vec2Fp::new(Fp::from_num(at[0]), Fp::from_num(at[1]));
                    let launchers: Vec<SimId> = armed
                        .iter()
                        .copied()
                        .filter(|id| !parts.mobile.contains(id) && click.units.binary_search_by_key(id, |unit| unit.id).ok().is_some_and(|index| is_launcher(&click.units[index], click.blueprints)))
                        .collect();
                    if !launchers.is_empty() {
                        commands.push(SimCommand::AttackGround { units: launchers, point });
                    }
                }
            }
            return commands;
        }
    }
    if let Some(target) = enemy {
        let unarmed: Vec<SimId> = parts.builders.iter().copied().filter(|id| !parts.armed.contains(id)).collect();
        let index = click.units.binary_search_by_key(&target, |unit| unit.id).expect("found in this list");
        // Defenses with guns, which pick their own targets, focus on this one when it is in their
        // reach and their guns can hit it; told to attack what they cannot reach, they would wait
        // on it and let what comes close go by.
        let kind = unit_target_kind(&click.units[index], click.blueprints, click.surfaced);
        let mut armed = parts.armed.clone();
        let raw = |value: i32| f64::from(value) / 4096.0;
        let aimed = &click.units[index];
        without_launchers_out_of_reach(&mut armed, click, [raw(aimed.x), raw(aimed.y)], raw(aimed.radius));
        armed.extend(defenses_in_reach(click, &click.units[index], kind));
        armed.sort_unstable();
        armed.dedup();
        if !armed.is_empty() {
            commands.push(SimCommand::Attack { units: armed, target });
        }
        if !unarmed.is_empty() && capturable(&click.units[index], click.blueprints) {
            commands.push(SimCommand::Capture { units: unarmed, target });
        }
        if !commands.is_empty() {
            return commands;
        }
    }

    let friend = nearest_unit(click.units, click.camera, click.team, click.cursor).filter(|id| !click.selected.contains(id));
    // Aircraft sent to a finished airfield land on it to refuel and be repaired.
    if let Some(target) = friend {
        let index = click.units.binary_search_by_key(&target, |unit| unit.id).expect("found in this list");
        let unit = &click.units[index];
        let staging = unit.blueprint != u16::MAX && unit.built_permille == 1000 && click.blueprints.get(BlueprintId(unit.blueprint)).airfield.is_some();
        let aircraft: Vec<SimId> = parts
            .mobile
            .iter()
            .copied()
            .filter(|id| {
                click.units.binary_search_by_key(id, |unit| unit.id).ok().is_some_and(|index| {
                    let unit = &click.units[index];
                    unit.blueprint != u16::MAX && click.blueprints.get(BlueprintId(unit.blueprint)).is_air()
                })
            })
            .collect();
        if staging && !aircraft.is_empty() {
            commands.push(SimCommand::Refit { units: aircraft, staging: target });
            return commands;
        }
        // Land units sent to a finished transport board it.
        let transport = unit.blueprint != u16::MAX && unit.built_permille == 1000 && click.blueprints.get(BlueprintId(unit.blueprint)).transport.is_some();
        let riders: Vec<SimId> = parts
            .mobile
            .iter()
            .copied()
            .filter(|id| {
                click.units.binary_search_by_key(id, |unit| unit.id).ok().is_some_and(|index| {
                    let unit = &click.units[index];
                    unit.blueprint != u16::MAX && crate::sim::transport::transport_size(click.blueprints.get(BlueprintId(unit.blueprint))).is_some()
                })
            })
            .collect();
        if transport && !riders.is_empty() {
            commands.push(SimCommand::Load { units: riders, transport: target });
            return commands;
        }
    }
    // Selected factories right-clicked onto another factory help it with its queue.
    let mut helping = Vec::new();
    if let Some(target) = friend {
        let index = click.units.binary_search_by_key(&target, |unit| unit.id).expect("found in this list");
        let unit = &click.units[index];
        let is_factory = unit.blueprint != u16::MAX && click.blueprints.get(BlueprintId(unit.blueprint)).is_factory();
        let factories: Vec<SimId> = parts.factories.iter().copied().filter(|&id| id != target).collect();
        if is_factory && !factories.is_empty() {
            helping.push(SimCommand::Assist { units: factories, target });
            if parts.builders.is_empty() && parts.mobile.is_empty() {
                return helping;
            }
        }
    }
    commands.extend(helping);
    if let (Some(target), false) = (friend, parts.builders.is_empty()) {
        let index = click.units.binary_search_by_key(&target, |unit| unit.id).expect("found in this list");
        let unit = &click.units[index];
        let is_factory = unit.blueprint != u16::MAX && click.blueprints.get(BlueprintId(unit.blueprint)).is_factory();
        if unit.built_permille < 1000 || is_factory {
            commands.push(SimCommand::Assist { units: parts.builders.clone(), target });
            return commands;
        }
        if unit.health_permille < 1000 {
            commands.push(SimCommand::Repair { units: parts.builders.clone(), target });
            return commands;
        }
    }

    if !parts.builders.is_empty() {
        if let Some(target) = wreck_under_cursor(click) {
            commands.push(SimCommand::Reclaim { units: parts.builders.clone(), target });
            return commands;
        }
    }

    if let Some(ground) = click.ground {
        if !parts.mobile.is_empty() {
            commands.push(order_command(&parts.mobile, ground, click.attack_move));
        }
        let point = Vec2Fp::new(Fp::from_num(ground[0]), Fp::from_num(ground[1]));
        for &factory in &parts.factories {
            commands.push(SimCommand::SetRally { factory, point });
        }
    }
    commands
}

/// A patrol for each unit, from where it starts it to the points and back round: from where it
/// stands, or, queued, from where it will be once the orders ahead of it are done, as with any
/// other queued order.
pub fn patrol_commands(units: &[SimId], snapshot: &crate::sim::world::SimSnapshot, points: &[Vec2Fp], queued: bool) -> Vec<SimCommand> {
    let raw = |x: i32, y: i32| Vec2Fp::new(Fp::from_bits(x), Fp::from_bits(y));
    units
        .iter()
        .filter_map(|&id| {
            let unit = snapshot.units.iter().find(|unit| unit.id == id)?;
            let last_step = queued.then(|| snapshot.plans.iter().find(|plan| plan.id == id).and_then(|plan| plan.steps.last())).flatten();
            let start = last_step.map_or(raw(unit.x, unit.y), |step| raw(step.x, step.y));
            Some(SimCommand::Patrol { units: vec![id], points: std::iter::once(start).chain(points.iter().copied()).collect(), queued })
        })
        .collect()
}

/// The same orders, to carry out after what units are already doing. An attack follows its
/// target until its turn comes, whether the target is in sight or a contact on radar. A rally
/// point or a follow order has no end to wait for, so those stay as they are.
pub fn queued_commands(commands: Vec<SimCommand>) -> Vec<SimCommand> {
    commands
        .into_iter()
        .map(|command| match command {
            SimCommand::Move { units, goal } => SimCommand::QueueMove { units, goal, attack: false },
            SimCommand::AttackMove { units, goal } => SimCommand::QueueMove { units, goal, attack: true },
            SimCommand::Attack { units, target } => SimCommand::QueueAttack { units, target },
            SimCommand::Assist { units, target } => SimCommand::QueueTask { units, task: BuildTask::Assist { target } },
            SimCommand::Repair { units, target } => SimCommand::QueueTask { units, task: BuildTask::Repair { target } },
            SimCommand::Capture { units, target } => SimCommand::QueueTask { units, task: BuildTask::Capture { target } },
            SimCommand::Reclaim { units, target } => SimCommand::QueueTask { units, task: BuildTask::Reclaim { target } },
            other => other,
        })
        .collect()
}

/// The same moves, made formation moves: the group travels at the pace of its slowest unit and
/// keeps its shape on the way. Anything that is not a move stays as it is.
pub fn formation_commands(commands: Vec<SimCommand>) -> Vec<SimCommand> {
    commands
        .into_iter()
        .map(|command| match command {
            SimCommand::Move { units, goal } => SimCommand::FormationMove { units, goal, attack: false, queued: false },
            SimCommand::AttackMove { units, goal } => SimCommand::FormationMove { units, goal, attack: true, queued: false },
            SimCommand::QueueMove { units, goal, attack } => SimCommand::FormationMove { units, goal, attack, queued: true },
            other => other,
        })
        .collect()
}

/// Discharge the enemy under the cursor with each selected unit that has it.
pub fn discharge_orders(click: &RightClick<'_>) -> Vec<SimCommand> {
    let Some(target) = crate::input::selection::nearest_unit_within(click.units, click.camera, click.team.enemy(), click.cursor, crate::input::selection::TARGET_CLICK_RADIUS_PX) else {
        return Vec::new();
    };
    click
        .selected
        .iter()
        .filter_map(|id| click.units.binary_search_by_key(id, |unit| unit.id).ok().map(|index| &click.units[index]))
        .filter(|unit| unit.blueprint != u16::MAX && click.blueprints.get(BlueprintId(unit.blueprint)).discharge.is_some())
        .map(|unit| SimCommand::Discharge { unit: unit.id, target })
        .collect()
}

/// Whether a unit the player can see could be captured: finished, and not a commander, a
/// field rig, or an aircraft.
fn capturable(unit: &UnitView, blueprints: &BlueprintDb) -> bool {
    unit.blueprint != u16::MAX && unit.built_permille == 1000 && {
        let class = blueprints.get(BlueprintId(unit.blueprint)).class;
        !matches!(class, UnitClass::Commander | UnitClass::FieldRig | UnitClass::Air)
    }
}

/// The selected builders capture the enemy under the cursor.
pub fn capture_orders(click: &RightClick<'_>) -> Vec<SimCommand> {
    let Some(target) = crate::input::selection::nearest_unit_within(click.units, click.camera, click.team.enemy(), click.cursor, crate::input::selection::TARGET_CLICK_RADIUS_PX) else {
        return Vec::new();
    };
    let builders = sort_selection(click.selected, click.units, click.blueprints).builders;
    let index = click.units.binary_search_by_key(&target, |unit| unit.id).expect("found in this list");
    if builders.is_empty() || !capturable(&click.units[index], click.blueprints) {
        return Vec::new();
    }
    vec![SimCommand::Capture { units: builders, target }]
}

/// The selected units that can walk.
pub fn mobile_units(selected: &[SimId], units: &[UnitView], blueprints: &BlueprintDb) -> Vec<SimId> {
    sort_selection(selected, units, blueprints).mobile
}

/// A plain move to the ground, whatever is under the cursor.
pub fn move_orders(click: &RightClick<'_>) -> Vec<SimCommand> {
    let mobile = mobile_units(click.selected, click.units, click.blueprints);
    match click.ground {
        Some(ground) if !mobile.is_empty() => vec![order_command(&mobile, ground, false)],
        _ => Vec::new(),
    }
}

/// Assist the friendly unit under the cursor. Builders lend their build power if it has work
/// that can use it. Everything else follows it, and fires at what comes into range.
pub fn assist_orders(click: &RightClick<'_>) -> Vec<SimCommand> {
    let Some(target) = nearest_unit(click.units, click.camera, click.team, click.cursor) else {
        return Vec::new();
    };
    let index = click.units.binary_search_by_key(&target, |unit| unit.id).expect("found in this list");
    let unit = &click.units[index];
    let has_build_work = unit.built_permille < 1000
        || (unit.blueprint != u16::MAX && {
            let blueprint = click.blueprints.get(BlueprintId(unit.blueprint));
            blueprint.is_factory() || (!blueprint.is_structure() && blueprint.build_power > Fp::ZERO)
        });
    let parts = sort_selection(click.selected, click.units, click.blueprints);
    let helpers: Vec<SimId> = if has_build_work { parts.builders.iter().copied().filter(|id| *id != target).collect() } else { Vec::new() };
    let followers: Vec<SimId> = parts.mobile.iter().copied().filter(|id| *id != target && !helpers.contains(id)).collect();
    let mut commands = Vec::new();
    if !helpers.is_empty() {
        commands.push(SimCommand::Assist { units: helpers, target });
    }
    if !followers.is_empty() {
        commands.push(SimCommand::Follow { units: followers, target });
    }
    commands
}

/// Launch: the selected missile launchers fire at the enemy under the cursor, in sight or on
/// radar, or at the ground there. Those standing out of reach are left out; a missile submarine
/// sails into range first. An enemy no launcher's missile can hit, such as an aircraft, is
/// passed over for the ground beneath it. Nothing if no launcher selected can fire there.
pub fn launch_orders(click: &RightClick<'_>) -> Vec<SimCommand> {
    let launchers: Vec<&UnitView> = click
        .selected
        .iter()
        .filter_map(|id| click.units.binary_search_by_key(id, |unit| unit.id).ok().map(|index| &click.units[index]))
        .filter(|unit| unit.team == click.team && is_launcher(unit, click.blueprints))
        .collect();
    if launchers.is_empty() {
        return Vec::new();
    }
    let raw = |value: i32| f64::from(value) / 4096.0;
    let hittable = |kind: TargetKind| launchers.iter().any(|unit| weapons_reach(click.blueprints.get(BlueprintId(unit.blueprint)), kind));
    let aim = match hovered_enemy(click) {
        Some(Hovered::Unit(id, kind)) if hittable(kind) => {
            let unit = &click.units[click.units.binary_search_by_key(&id, |unit| unit.id).expect("found in this list")];
            Some((Some(id), [raw(unit.x), raw(unit.y)], raw(unit.radius)))
        }
        Some(Hovered::Contact(id, at, kind)) if hittable(kind) => Some((id, at, 0.0)),
        _ => None,
    };
    let Some((target, point, radius)) = aim.or_else(|| click.ground.map(|ground| (None, ground, 0.0))) else {
        return Vec::new();
    };
    let able: Vec<SimId> = launchers.iter().filter(|unit| reaches_from_where_it_stands(unit, click.blueprints, point, radius)).map(|unit| unit.id).collect();
    if able.is_empty() {
        return Vec::new();
    }
    match target {
        Some(target) => vec![SimCommand::Attack { units: able, target }],
        None => vec![SimCommand::AttackGround { units: able, point: Vec2Fp::new(Fp::from_num(point[0]), Fp::from_num(point[1])) }],
    }
}

/// What a right-click on the enemy under the cursor would be, for the pointer to show.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PointerTarget {
    /// An attack: the red target.
    Attack,
    /// An attack in which missile launchers fire: the red target too.
    Launch,
    /// Nothing selected can reach it or hit its kind.
    OutOfReach,
}

/// What a right-click would do to the enemy under the cursor, worked out from the very orders
/// it would give, so that the pointer never promises an attack that the click does not make.
/// `None` with no enemy there, or when the click captures it instead. A decoy shows the same
/// as what it looks like.
pub fn right_click_target(click: &RightClick<'_>) -> Option<PointerTarget> {
    let hovered = hovered_enemy(click)?;
    let (decoy, kind) = match hovered {
        Hovered::Unit(_, kind) => (false, kind),
        Hovered::Contact(unit, _, kind) => (unit.is_none(), kind),
    };
    // Of those ordered to attack, the ones that can hit what it is, and whether any launches.
    let hitting = |units: &[SimId]| -> Option<bool> {
        let able: Vec<&UnitView> = units
            .iter()
            .filter_map(|id| click.units.binary_search_by_key(id, |unit| unit.id).ok().map(|index| &click.units[index]))
            .filter(|unit| unit.blueprint == u16::MAX || weapons_reach(click.blueprints.get(BlueprintId(unit.blueprint)), kind))
            .collect();
        (!able.is_empty()).then(|| able.iter().any(|unit| is_launcher(unit, click.blueprints)))
    };
    let orders = right_click_orders(click);
    if let Some(units) = orders.iter().find_map(|order| match order {
        SimCommand::Attack { units, .. } => Some(units),
        SimCommand::AttackGround { units, .. } if decoy => Some(units),
        _ => None,
    }) {
        return Some(match hitting(units) {
            Some(true) => PointerTarget::Launch,
            Some(false) => PointerTarget::Attack,
            None => PointerTarget::OutOfReach,
        });
    }
    if decoy && orders.iter().any(|order| matches!(order, SimCommand::AttackMove { units, .. } if !units.is_empty())) {
        return Some(PointerTarget::Attack);
    }
    if orders.iter().any(|order| matches!(order, SimCommand::Capture { .. })) {
        return None;
    }
    Some(PointerTarget::OutOfReach)
}

/// Fire at the ground under the cursor with everything selected that has a gun, structures
/// included. On debris, it aims at the middle of it, which is what the shells must land on.
pub fn attack_ground_orders(click: &RightClick<'_>) -> Vec<SimCommand> {
    let armed: Vec<SimId> = click
        .selected
        .iter()
        .filter_map(|id| click.units.binary_search_by_key(id, |unit| unit.id).ok().map(|index| &click.units[index]))
        .filter(|unit| unit.team == click.team && (unit.blueprint == u16::MAX || click.blueprints.get(BlueprintId(unit.blueprint)).weapon.is_some()))
        .map(|unit| unit.id)
        .collect();
    let debris = wreck_under_cursor(click)
        .and_then(|id| click.wrecks.iter().find(|wreck| wreck.id == id))
        .filter(|wreck| !wreck.kind.is_scenery())
        .map(|wreck| Vec2Fp::new(Fp::from_bits(wreck.x), Fp::from_bits(wreck.y)));
    let point = debris.or_else(|| click.ground.map(|ground| Vec2Fp::new(Fp::from_num(ground[0]), Fp::from_num(ground[1]))));
    match point {
        Some(point) if !armed.is_empty() => vec![SimCommand::AttackGround { units: armed, point }],
        _ => Vec::new(),
    }
}

fn wreck_under_cursor(click: &RightClick<'_>) -> Option<SimId> {
    let mut best: Option<(f64, SimId)> = None;
    // Debris still falling through the air cannot be reclaimed yet, nor shot at on the ground.
    for wreck in click.wrecks.iter().filter(|wreck| wreck.motion != WreckMotion::Falling) {
        let world = [f64::from(wreck.x) * RAW_TO_WORLD, f64::from(wreck.y) * RAW_TO_WORLD, f64::from(wreck.z) * RAW_TO_WORLD];
        // Under the water, also where the slick over it is, on the surface.
        let surface = click.water.filter(|&water| world[2] < water).map(|water| [world[0], world[1], water]);
        for spot in std::iter::once(world).chain(surface) {
            if let Some(position) = click.camera.world_to_screen(spot) {
                let distance = (position[0] - click.cursor[0]).hypot(position[1] - click.cursor[1]);
                if distance <= WRECK_CLICK_PX && best.is_none_or(|(closest, _)| distance < closest) {
                    best = Some((distance, wreck.id));
                }
            }
        }
    }
    best.map(|(_, id)| id)
}

/// The corner cell for a structure centered on a ground point.
pub fn placement_cell(ground: [f64; 2], footprint: u8) -> (u32, u32) {
    let cell = f64::from(TERRAIN_CELL_UNITS);
    let offset = f64::from(footprint) * 0.5;
    let corner = |value: f64| (value / cell - offset + 0.5).floor().max(0.0) as u32;
    (corner(ground[0]), corner(ground[1]))
}

/// How far from the cursor, in points on screen, an extractor will snap to a free deposit.
const DEPOSIT_SNAP_POINTS: f64 = 40.0;

/// The middle of a cell on the ground, as a world position.
pub fn deposit_position(cell: (u32, u32), heights: &HeightField) -> [f64; 3] {
    let cell_units = f64::from(TERRAIN_CELL_UNITS);
    let (x, y) = ((f64::from(cell.0) + 0.5) * cell_units, (f64::from(cell.1) + 0.5) * cell_units);
    [x, y, heights.height_at(x, y)]
}

/// The deposits a new extractor could go on right now.
/// For a geothermal plant, the corner cells of the free vents it could go over instead.
pub fn free_deposits(map: &PlacementMap<'_>, blueprint: BlueprintId, units: &[UnitView], pending: &[PendingBuild]) -> Vec<(u32, u32)> {
    let unit = map.blueprints.get(blueprint);
    let sites: Vec<(u32, u32)> = if unit.requires_vent {
        let half = u32::from(unit.footprint) / 2;
        map.vents.iter().filter(|vent| vent.0 >= half && vent.1 >= half).map(|vent| (vent.0 - half, vent.1 - half)).collect()
    } else {
        map.deposits.to_vec()
    };
    sites.into_iter().filter(|cell| placement_is_valid(map, blueprint, *cell, units, pending)).collect()
}

/// Where a site's marker is drawn: the middle of the structure's footprint.
pub fn site_position(cell: (u32, u32), footprint: u8, heights: &HeightField) -> [f64; 3] {
    let half = u32::from(footprint) / 2;
    deposit_position((cell.0 + half, cell.1 + half), heights)
}

/// Where a structure would go for this cursor position. Structures that need a deposit snap to
/// the nearest free one on screen, so that the reach is the same however far out the camera is.
#[allow(clippy::too_many_arguments)]
pub fn placement_target(
    map: &PlacementMap<'_>,
    blueprint: BlueprintId,
    cursor: [f64; 2],
    ground: Option<[f64; 2]>,
    camera: &Camera,
    heights: &HeightField,
    units: &[UnitView],
    pending: &[PendingBuild],
    pixels_per_point: f64,
) -> Option<(u32, u32)> {
    let unit = map.blueprints.get(blueprint);
    if unit.requires_deposit || unit.requires_vent {
        let reach = DEPOSIT_SNAP_POINTS * pixels_per_point;
        let mut best: Option<(f64, (u32, u32))> = None;
        for cell in free_deposits(map, blueprint, units, pending) {
            if let Some(screen) = camera.world_to_screen(site_position(cell, unit.footprint, heights)) {
                let distance = (screen[0] - cursor[0]).hypot(screen[1] - cursor[1]);
                if distance <= reach && best.is_none_or(|(closest, _)| distance < closest) {
                    best = Some((distance, cell));
                }
            }
        }
        if let Some((_, cell)) = best {
            return Some(cell);
        }
    }
    ground.map(|ground| placement_cell(ground, unit.footprint))
}

/// The longest row a single Shift-drag can lay out.
const MAX_LINE_BUILDS: i64 = 64;

/// The corner cells for a row of structures between two cells, side by side with no gaps,
/// in order from the start. A row ends on the last spot that fits before passing `end`.
pub fn line_cells(start: (u32, u32), end: (u32, u32), footprint: u8) -> Vec<(u32, u32)> {
    let size = i64::from(footprint.max(1));
    let (dx, dy) = (i64::from(end.0) - i64::from(start.0), i64::from(end.1) - i64::from(start.1));
    let steps = (dx.abs().max(dy.abs()) / size).min(MAX_LINE_BUILDS - 1);
    // Each step advances one footprint along the longer axis, which keeps neighbors from overlapping.
    let reach = dx.abs().max(dy.abs()).max(1);
    (0..=steps)
        .map(|step| {
            let along = step * size;
            let round_div = |value: i64| (2 * value * along + reach).div_euclid(2 * reach);
            ((i64::from(start.0) + round_div(dx)) as u32, (i64::from(start.1) + round_div(dy)) as u32)
        })
        .collect()
}

/// A build that is queued but not started, in world units.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PendingBuild {
    pub center: [f64; 3],
    pub width: f64,
}

/// Every queued build in a snapshot's plans, whichever builder holds it. Several builders
/// queued on the same spot count once.
pub fn pending_builds(snapshot: &SimSnapshot) -> Vec<PendingBuild> {
    let mut builds: Vec<PendingBuild> = Vec::new();
    for step in snapshot.plans.iter().flat_map(|plan| &plan.steps) {
        if step.pending_width > 0 {
            let build = PendingBuild {
                center: [f64::from(step.x) * RAW_TO_WORLD, f64::from(step.y) * RAW_TO_WORLD, f64::from(step.z) * RAW_TO_WORLD],
                width: f64::from(step.pending_width) * RAW_TO_WORLD,
            };
            if !builds.contains(&build) {
                builds.push(build);
            }
        }
    }
    builds
}

/// Everything known to the player about where structures can go.
pub struct PlacementMap<'a> {
    pub cells_per_side: u32,
    /// Sorted (y, x) cells of walls, known from the start.
    pub blocked_cells: &'a [(u32, u32)],
    pub deposits: &'a [(u32, u32)],
    /// Geothermal vents, by cell.
    pub vents: &'a [(u32, u32)],
    /// Sorted (y, x) cells of water deep enough for ships, where naval structures go.
    pub water_cells: &'a [(u32, u32)],
    pub blueprints: &'a BlueprintDb,
}

/// A client-side preview of whether a structure fits. The simulation has the final say, and
/// it knows about hidden enemy structures that the player cannot.
pub fn placement_is_valid(map: &PlacementMap<'_>, blueprint: BlueprintId, cell: (u32, u32), units: &[UnitView], pending: &[PendingBuild]) -> bool {
    let unit = map.blueprints.get(blueprint);
    let size = u32::from(unit.footprint);
    if cell.0 + size > map.cells_per_side || cell.1 + size > map.cells_per_side {
        return false;
    }
    if unit.requires_deposit && !map.deposits.contains(&cell) {
        return false;
    }
    if unit.requires_vent && !map.vents.contains(&(cell.0 + size / 2, cell.1 + size / 2)) {
        return false;
    }
    let cell_units = f64::from(TERRAIN_CELL_UNITS);
    let structures: Vec<(f64, f64, f64)> = units
        .iter()
        .filter(|other| other.is_structure)
        .map(|other| (f64::from(other.x) * RAW_TO_WORLD, f64::from(other.y) * RAW_TO_WORLD, f64::from(other.radius) * RAW_TO_WORLD))
        .chain(pending.iter().map(|build| (build.center[0], build.center[1], build.width * 0.5)))
        .collect();
    for y in cell.1..cell.1 + size {
        for x in cell.0..cell.0 + size {
            let on_water = map.water_cells.binary_search(&(y, x)).is_ok();
            if unit.on_water != on_water || (!unit.on_water && map.blocked_cells.binary_search(&(y, x)).is_ok()) {
                return false;
            }
            if !unit.requires_deposit && map.deposits.contains(&(x, y)) {
                return false;
            }
            if !unit.requires_vent && map.vents.contains(&(x, y)) {
                return false;
            }
            let (center_x, center_y) = ((f64::from(x) + 0.5) * cell_units, (f64::from(y) + 0.5) * cell_units);
            for &(other_x, other_y, radius) in &structures {
                if (center_x - other_x).abs() < radius && (center_y - other_y).abs() < radius {
                    return false;
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> BlueprintDb {
        BlueprintDb::standard()
    }

    fn camera() -> Camera {
        let mut camera = Camera::new(10_240.0, [1600.0, 1000.0]);
        camera.look_at(2_000.0, 2_000.0, 300.0);
        camera
    }

    fn view(id: u32, team: Team, blueprint: BlueprintId, db: &BlueprintDb, x: f64, y: f64) -> UnitView {
        let raw = |value: f64| (value * 4096.0) as i32;
        let unit = db.get(blueprint);
        UnitView {
            id: SimId(id),
            team,
            x: raw(x),
            y: raw(y),
            z: raw(unit.radius.to_num::<f64>()),
            radius: unit.radius.to_bits(),
            blueprint: blueprint.0,
            health_permille: 1000,
            built_permille: 1000,
            is_structure: unit.is_structure(),
            facing: [0, 0], aim: [0, 0], extra_aims: [[0; 2]; 3], pitches: [0; 4],
            working: false,
            assembling: None,
            upgrade: None,
            remembered: false,
            ally: false,
        }
    }

    fn click_at(selected: &[SimId], units: &[UnitView], wrecks: &[WreckView], db: &BlueprintDb, camera: &Camera, world: [f64; 2]) -> Vec<SimCommand> {
        let cursor = camera.world_to_screen([world[0], world[1], 8.0]).unwrap();
        right_click_orders(&RightClick {
            selected,
            units,
            wrecks,
            blueprints: db,
            team: Team::Blue,
            camera,
            cursor,
            ground: Some(world),
            attack_move: false,
            contacts: &[],
            surfaced: &[],
            water: None,
        })
    }

    fn click_for<'a>(selected: &'a [SimId], units: &'a [UnitView], db: &'a BlueprintDb, camera: &'a Camera, world: [f64; 2]) -> RightClick<'a> {
        let cursor = camera.world_to_screen([world[0], world[1], 8.0]).unwrap();
        RightClick { selected, units, wrecks: &[], blueprints: db, team: Team::Blue, camera, cursor, ground: Some(world), attack_move: false, contacts: &[], surfaced: &[], water: None }
    }

    #[test]
    fn discharge_goes_from_the_commander_to_the_enemy_under_the_cursor() {
        let db = db();
        let units = [
            view(1, Team::Blue, db.id("command_rig"), &db, 2_000.0, 1_950.0),
            view(2, Team::Blue, db.id("t1_light_walker"), &db, 2_040.0, 1_950.0),
            view(3, Team::Red, db.id("t2_heavy_tank"), &db, 2_000.0, 2_050.0),
        ];
        let camera = camera();
        let orders = discharge_orders(&click_for(&[SimId(1), SimId(2)], &units, &db, &camera, [2_000.0, 2_050.0]));
        assert_eq!(orders, [SimCommand::Discharge { unit: SimId(1), target: SimId(3) }], "only the commander has Discharge");
        assert!(discharge_orders(&click_for(&[SimId(2)], &units, &db, &camera, [2_000.0, 2_050.0])).is_empty());
        assert!(discharge_orders(&click_for(&[SimId(1)], &units, &db, &camera, [2_000.0, 1_950.0])).is_empty(), "a friend is not a target");
    }

    #[test]
    fn attack_ground_takes_everything_with_a_gun_and_aims_at_the_middle_of_debris() {
        use crate::sim::components::WreckKind;
        let db = db();
        let units = [
            view(1, Team::Blue, db.id("t1_light_artillery"), &db, 2_000.0, 1_950.0),
            view(2, Team::Blue, db.id("t1_engineer"), &db, 2_040.0, 1_950.0),
            view(3, Team::Blue, db.id("t1_point_defense"), &db, 1_960.0, 1_950.0),
            view(4, Team::Red, db.id("t1_light_walker"), &db, 2_300.0, 2_300.0),
        ];
        let raw = |value: f64| (value * 4096.0) as i32;
        let wreck = |id: u32, x: f64, kind: WreckKind| WreckView { id: SimId(id), x: raw(x), y: raw(2_050.0), z: raw(8.0), radius: raw(8.0), mass: 10, energy: 0, kind, motion: WreckMotion::Still };
        let wrecks = [wreck(8, 1_900.0, WreckKind::Debris), wreck(9, 2_100.0, WreckKind::Tree)];
        let camera = camera();
        let selected = [SimId(1), SimId(2), SimId(3), SimId(4)];
        let at = |world: [f64; 2]| {
            let cursor = camera.world_to_screen([world[0], world[1], 8.0]).unwrap();
            attack_ground_orders(&RightClick { selected: &selected, units: &units, wrecks: &wrecks, blueprints: &db, team: Team::Blue, camera: &camera, cursor, ground: Some(world), attack_move: false, contacts: &[], surfaced: &[], water: None })
        };
        let point = |x: f64, y: f64| Vec2Fp::new(Fp::from_num(x), Fp::from_num(y));
        let armed = vec![SimId(1), SimId(3)];
        // Close to debris snaps to its middle. A tree cannot be blasted, so it aims at the ground there.
        assert_eq!(at([1_902.0, 2_052.0]), [SimCommand::AttackGround { units: armed.clone(), point: point(1_900.0, 2_050.0) }]);
        assert_eq!(at([2_102.0, 2_052.0]), [SimCommand::AttackGround { units: armed.clone(), point: point(2_102.0, 2_052.0) }]);
        let engineer_only = attack_ground_orders(&click_for(&[SimId(2)], &units, &db, &camera, [2_000.0, 2_000.0]));
        assert!(engineer_only.is_empty(), "an engineer has no gun");
    }

    #[test]
    fn shift_turns_orders_into_their_queued_forms() {
        let goal = Vec2Fp::new(Fp::from_num(10.0), Fp::from_num(20.0));
        let ids = || vec![SimId(1)];
        let target = SimId(2);
        let queued = queued_commands(
            vec![
                SimCommand::Move { units: ids(), goal },
                SimCommand::AttackMove { units: ids(), goal },
                SimCommand::Attack { units: ids(), target },
                SimCommand::Assist { units: ids(), target },
                SimCommand::Repair { units: ids(), target },
                SimCommand::Reclaim { units: ids(), target },
            ],
        );
        assert_eq!(
            queued,
            [
                SimCommand::QueueMove { units: ids(), goal, attack: false },
                SimCommand::QueueMove { units: ids(), goal, attack: true },
                SimCommand::QueueAttack { units: ids(), target },
                SimCommand::QueueTask { units: ids(), task: BuildTask::Assist { target } },
                SimCommand::QueueTask { units: ids(), task: BuildTask::Repair { target } },
                SimCommand::QueueTask { units: ids(), task: BuildTask::Reclaim { target } },
            ]
        );
    }

    #[test]
    fn orders_with_no_end_to_wait_for_are_not_queued() {
        let goal = Vec2Fp::new(Fp::from_num(10.0), Fp::from_num(20.0));
        let unchanged = vec![SimCommand::SetRally { factory: SimId(1), point: goal }, SimCommand::Follow { units: vec![SimId(2)], target: SimId(1) }];
        assert_eq!(queued_commands(unchanged.clone()), unchanged);
    }

    #[test]
    fn ctrl_makes_moves_formation_moves_and_leaves_other_orders_alone() {
        let goal = Vec2Fp::new(Fp::from_num(10.0), Fp::from_num(20.0));
        let ids = || vec![SimId(1), SimId(2)];
        let orders = vec![
            SimCommand::Move { units: ids(), goal },
            SimCommand::AttackMove { units: ids(), goal },
            SimCommand::QueueMove { units: ids(), goal, attack: false },
            SimCommand::QueueMove { units: ids(), goal, attack: true },
            SimCommand::Attack { units: ids(), target: SimId(9) },
            SimCommand::SetRally { factory: SimId(3), point: goal },
        ];
        assert_eq!(
            formation_commands(orders),
            [
                SimCommand::FormationMove { units: ids(), goal, attack: false, queued: false },
                SimCommand::FormationMove { units: ids(), goal, attack: true, queued: false },
                SimCommand::FormationMove { units: ids(), goal, attack: false, queued: true },
                SimCommand::FormationMove { units: ids(), goal, attack: true, queued: true },
                SimCommand::Attack { units: ids(), target: SimId(9) },
                SimCommand::SetRally { factory: SimId(3), point: goal },
            ]
        );
    }

    #[test]
    fn an_armed_move_goes_to_the_ground_even_on_top_of_a_unit_and_leaves_structures_out() {
        let db = db();
        let units = [
            view(1, Team::Blue, db.id("t1_light_walker"), &db, 2_000.0, 1_950.0),
            view(2, Team::Blue, db.id("land_factory_t1"), &db, 1_900.0, 1_950.0),
            view(3, Team::Red, db.id("t1_light_walker"), &db, 2_000.0, 2_050.0),
        ];
        let camera = camera();
        let goal = Vec2Fp::new(Fp::from_num(2_000.0), Fp::from_num(2_050.0));
        let orders = move_orders(&click_for(&[SimId(1), SimId(2)], &units, &db, &camera, [2_000.0, 2_050.0]));
        assert_eq!(orders, [SimCommand::Move { units: vec![SimId(1)], goal }]);
        assert!(move_orders(&click_for(&[SimId(2)], &units, &db, &camera, [2_000.0, 2_050.0])).is_empty(), "a factory cannot walk");
    }

    #[test]
    fn assisting_a_combat_unit_means_following_it() {
        let db = db();
        let units = [
            view(1, Team::Blue, db.id("t1_light_walker"), &db, 2_000.0, 1_900.0),
            view(2, Team::Blue, db.id("t1_engineer"), &db, 2_040.0, 1_900.0),
            view(3, Team::Blue, db.id("t2_heavy_tank"), &db, 2_000.0, 2_050.0),
        ];
        let camera = camera();
        let orders = assist_orders(&click_for(&[SimId(1), SimId(2)], &units, &db, &camera, [2_000.0, 2_050.0]));
        assert_eq!(orders, [SimCommand::Follow { units: vec![SimId(1), SimId(2)], target: SimId(3) }]);
    }

    #[test]
    fn assisting_a_builder_lends_build_power_and_guards_it_with_the_rest() {
        let db = db();
        let units = [
            view(1, Team::Blue, db.id("t1_light_walker"), &db, 2_000.0, 1_900.0),
            view(2, Team::Blue, db.id("t1_engineer"), &db, 2_040.0, 1_900.0),
            view(3, Team::Blue, db.id("command_rig"), &db, 2_000.0, 2_050.0),
        ];
        let camera = camera();
        let orders = assist_orders(&click_for(&[SimId(1), SimId(2)], &units, &db, &camera, [2_000.0, 2_050.0]));
        assert_eq!(orders, [SimCommand::Assist { units: vec![SimId(2)], target: SimId(3) }, SimCommand::Follow { units: vec![SimId(1)], target: SimId(3) }]);
    }

    #[test]
    fn assisting_needs_a_friendly_unit_and_a_unit_cannot_assist_itself() {
        let db = db();
        let units = [view(1, Team::Blue, db.id("t1_light_walker"), &db, 2_000.0, 1_950.0), view(2, Team::Red, db.id("t2_heavy_tank"), &db, 2_000.0, 2_050.0)];
        let camera = camera();
        assert!(assist_orders(&click_for(&[SimId(1)], &units, &db, &camera, [2_000.0, 2_050.0])).is_empty(), "the enemy is not a friend");
        assert!(assist_orders(&click_for(&[SimId(1)], &units, &db, &camera, [2_000.0, 1_950.0])).is_empty(), "it is the only one selected");
    }

    #[test]
    fn right_clicking_a_transport_with_land_units_selected_boards_them() {
        let db = db();
        let units = [
            view(1, Team::Blue, db.id("t1_light_walker"), &db, 2_000.0, 1_950.0),
            view(2, Team::Blue, db.id("t2_heavy_tank"), &db, 2_030.0, 1_950.0),
            view(3, Team::Blue, db.id("t1_interceptor"), &db, 2_060.0, 1_950.0),
            view(4, Team::Blue, db.id("t1_light_transport"), &db, 2_000.0, 2_050.0),
        ];
        let camera = camera();
        let orders = click_at(&[SimId(1), SimId(2), SimId(3)], &units, &[], &db, &camera, [2_000.0, 2_050.0]);
        assert_eq!(orders, [SimCommand::Load { units: vec![SimId(1), SimId(2)], transport: SimId(4) }], "the interceptor cannot board");
    }

    #[test]
    fn attack_move_stays_an_attack_move_on_top_of_a_unit() {
        let db = db();
        let units = [
            view(1, Team::Blue, db.id("t1_light_walker"), &db, 2_000.0, 1_950.0),
            view(2, Team::Red, db.id("t1_light_walker"), &db, 2_000.0, 2_050.0),
        ];
        let camera = camera();
        let world = [2_000.0, 2_050.0];
        let cursor = camera.world_to_screen([world[0], world[1], 8.0]).unwrap();
        let orders = right_click_orders(&RightClick {
            selected: &[SimId(1)],
            units: &units,
            wrecks: &[],
            blueprints: &db,
            team: Team::Blue,
            camera: &camera,
            cursor,
            ground: Some(world),
            attack_move: true,
            contacts: &[],
            surfaced: &[],
            water: None,
        });
        let goal = Vec2Fp::new(Fp::from_num(world[0]), Fp::from_num(world[1]));
        assert_eq!(orders, [SimCommand::AttackMove { units: vec![SimId(1)], goal }]);
    }

    #[test]
    fn attack_move_with_only_structures_selected_orders_nothing() {
        let db = db();
        let units = [view(1, Team::Blue, db.id("power_generator"), &db, 2_000.0, 1_950.0)];
        let camera = camera();
        let cursor = camera.world_to_screen([2_000.0, 2_050.0, 8.0]).unwrap();
        let orders = right_click_orders(&RightClick {
            selected: &[SimId(1)],
            units: &units,
            wrecks: &[],
            blueprints: &db,
            team: Team::Blue,
            camera: &camera,
            cursor,
            ground: Some([2_000.0, 2_050.0]),
            attack_move: true,
            contacts: &[],
            surfaced: &[],
            water: None,
        });
        assert!(orders.is_empty());
    }

    #[test]
    fn right_clicking_an_enemy_attacks_it_with_armed_units_and_captures_it_with_engineers() {
        let db = db();
        let units = [
            view(1, Team::Blue, db.id("t1_light_walker"), &db, 2_000.0, 1_950.0),
            view(2, Team::Blue, db.id("t1_engineer"), &db, 2_010.0, 1_950.0),
            view(3, Team::Red, db.id("t1_light_walker"), &db, 2_000.0, 2_050.0),
        ];
        let orders = click_at(&[SimId(1), SimId(2)], &units, &[], &db, &camera(), [2_000.0, 2_050.0]);
        assert_eq!(orders, [SimCommand::Attack { units: vec![SimId(1)], target: SimId(3) }, SimCommand::Capture { units: vec![SimId(2)], target: SimId(3) }]);
    }

    /// Zoomed out, an enemy is a symbol a few pixels across: a right-click near it, not only dead
    /// on it, attacks it, and one further off is a move.
    #[test]
    fn a_right_click_near_a_far_off_enemy_attacks_it() {
        let db = db();
        let mut camera = Camera::new(10_240.0, [1600.0, 1000.0]);
        camera.look_at(2_000.0, 2_000.0, 6_000.0);
        let units = [view(1, Team::Blue, db.id("t1_medium_tank"), &db, 1_500.0, 2_000.0), view(2, Team::Red, db.id("t1_medium_tank"), &db, 2_000.0, 2_000.0)];
        let on_screen = camera.world_to_screen([2_000.0, 2_000.0, 9.0]).unwrap();
        let click = |offset: f64| {
            right_click_orders(&RightClick {
                selected: &[SimId(1)],
                units: &units,
                wrecks: &[],
                blueprints: &db,
                team: Team::Blue,
                camera: &camera,
                cursor: [on_screen[0] + offset, on_screen[1]],
                ground: Some([2_000.0, 2_000.0]),
                attack_move: false,
                contacts: &[],
                surfaced: &[],
                water: None,
            })
        };
        assert_eq!(click(18.0), [SimCommand::Attack { units: vec![SimId(1)], target: SimId(2) }], "18 pixels off still attacks");
        assert!(matches!(click(30.0)[..], [SimCommand::Move { .. }]), "30 pixels off is a move: {:?}", click(30.0));
    }

    /// A right-click on a radar contact, with nothing in sight there, attacks the unit it is; on
    /// a decoy, which looks the same, the units go there to find nothing.
    #[test]
    fn a_right_click_on_a_radar_contact_attacks_it() {
        let db = db();
        let camera = camera();
        let units = [view(1, Team::Blue, db.id("t1_medium_tank"), &db, 1_800.0, 2_000.0)];
        let raw = |value: f64| (value * 4096.0) as i32;
        let contact = |unit: Option<SimId>| crate::sim::world::ContactView { unit, x: raw(2_100.0), y: raw(2_000.0), z: raw(9.0), is_air: false, is_naval: false, is_sub: false, is_structure: false };
        let cursor = camera.world_to_screen([2_100.0, 2_000.0, 9.0]).unwrap();
        let click = |contacts: &[crate::sim::world::ContactView]| {
            right_click_orders(&RightClick { selected: &[SimId(1)], units: &units, wrecks: &[], blueprints: &db, team: Team::Blue, camera: &camera, cursor, ground: Some([2_100.0, 2_000.0]), attack_move: false, contacts, surfaced: &[], water: None })
        };
        assert_eq!(click(&[contact(Some(SimId(7)))]), [SimCommand::Attack { units: vec![SimId(1)], target: SimId(7) }]);
        assert!(matches!(click(&[contact(None)])[..], [SimCommand::AttackMove { .. }]), "a decoy is gone to");
        // What the cursor says: a tank reaches a contact on land, not one in the air.
        let hover = RightClick { selected: &[SimId(1)], units: &units, wrecks: &[], blueprints: &db, team: Team::Blue, camera: &camera, cursor, ground: None, attack_move: false, contacts: &[], surfaced: &[], water: None };
        assert!(selection_reaches(&hover, TargetKind::Ground));
        assert!(!selection_reaches(&hover, TargetKind::Air));
    }

    /// With Shift, an attack on a radar contact waits its turn like one on a unit in sight. A
    /// contact is not among the units in sight, and the attack was sent unqueued, so it
    /// replaced the move before it.
    #[test]
    fn a_queued_attack_on_a_radar_contact_waits_its_turn() {
        let db = db();
        let camera = camera();
        let units = [view(1, Team::Blue, db.id("t1_medium_tank"), &db, 1_800.0, 2_000.0)];
        let raw = |value: f64| (value * 4096.0) as i32;
        let contacts = [crate::sim::world::ContactView { unit: Some(SimId(7)), x: raw(2_100.0), y: raw(2_000.0), z: raw(9.0), is_air: false, is_naval: false, is_sub: false, is_structure: false }];
        let cursor = camera.world_to_screen([2_100.0, 2_000.0, 9.0]).unwrap();
        let click = RightClick { selected: &[SimId(1)], units: &units, wrecks: &[], blueprints: &db, team: Team::Blue, camera: &camera, cursor, ground: Some([2_100.0, 2_000.0]), attack_move: false, contacts: &contacts, surfaced: &[], water: None };
        assert_eq!(queued_commands(right_click_orders(&click)), [SimCommand::QueueAttack { units: vec![SimId(1)], target: SimId(7) }]);
    }

    /// Every unit that moves is moved by a right-click on the ground and by a move order, the
    /// missile submarine, which counts as a factory as it builds its missiles, included. It was
    /// given a rally point instead, and could not be moved at all.
    #[test]
    fn every_mobile_unit_moves_on_a_right_click() {
        let db = db();
        let camera = camera();
        let ground = [2_000.0, 2_000.0];
        let cursor = camera.world_to_screen([ground[0], ground[1], 0.0]).unwrap();
        for def in db.iter().filter(|def| !def.is_structure() && def.speed > Fp::ZERO && def.class != crate::sim::blueprints::UnitClass::Ammo) {
            let units = [view(1, Team::Blue, db.id(&def.key), &db, 1_000.0, 1_000.0)];
            let click = RightClick { selected: &[SimId(1)], units: &units, wrecks: &[], blueprints: &db, team: Team::Blue, camera: &camera, cursor, ground: Some(ground), attack_move: false, contacts: &[], surfaced: &[], water: None };
            let orders = right_click_orders(&click);
            assert!(matches!(orders[..], [SimCommand::Move { .. }]), "{} on a right-click: {orders:?}", def.key);
            assert!(matches!(move_orders(&click)[..], [SimCommand::Move { .. }]), "{} on a move order", def.key);
        }
    }

    #[test]
    fn right_clicking_a_factory_with_factories_selected_assists_it() {
        let db = db();
        let units = [
            view(1, Team::Blue, db.id("land_factory_t1"), &db, 1_700.0, 2_050.0),
            view(3, Team::Blue, db.id("land_factory_t1"), &db, 1_900.0, 2_050.0),
        ];
        let camera = camera();
        let orders = click_at(&[SimId(1)], &units, &[], &db, &camera, [1_900.0, 2_050.0]);
        assert_eq!(orders, [SimCommand::Assist { units: vec![SimId(1)], target: SimId(3) }], "not a rally point on it");
    }

    #[test]
    fn right_clicking_a_factory_or_site_with_engineers_assists() {
        let db = db();
        let mut site = view(2, Team::Blue, db.id("power_generator"), &db, 2_100.0, 2_050.0);
        site.built_permille = 400;
        let units = [
            view(1, Team::Blue, db.id("t1_engineer"), &db, 2_000.0, 1_950.0),
            site,
            view(3, Team::Blue, db.id("land_factory_t1"), &db, 1_900.0, 2_050.0),
        ];
        let camera = camera();
        let assist_site = click_at(&[SimId(1)], &units, &[], &db, &camera, [2_100.0, 2_050.0]);
        assert_eq!(assist_site, [SimCommand::Assist { units: vec![SimId(1)], target: SimId(2) }]);
        let assist_factory = click_at(&[SimId(1)], &units, &[], &db, &camera, [1_900.0, 2_050.0]);
        assert_eq!(assist_factory, [SimCommand::Assist { units: vec![SimId(1)], target: SimId(3) }]);
    }

    #[test]
    fn right_clicking_damaged_friend_repairs_and_a_wreck_reclaims() {
        let db = db();
        let mut hurt = view(2, Team::Blue, db.id("power_generator"), &db, 2_100.0, 2_050.0);
        hurt.health_permille = 400;
        let units = [view(1, Team::Blue, db.id("t1_engineer"), &db, 2_000.0, 1_950.0), hurt];
        let raw = |value: f64| (value * 4096.0) as i32;
        let wrecks = [WreckView { id: SimId(9), x: raw(1_900.0), y: raw(2_050.0), z: raw(8.0), radius: raw(8.0), mass: 10, energy: 0, kind: crate::sim::components::WreckKind::Debris, motion: WreckMotion::Still }];
        let camera = camera();
        assert_eq!(
            click_at(&[SimId(1)], &units, &wrecks, &db, &camera, [2_100.0, 2_050.0]),
            [SimCommand::Repair { units: vec![SimId(1)], target: SimId(2) }]
        );
        assert_eq!(
            click_at(&[SimId(1)], &units, &wrecks, &db, &camera, [1_900.0, 2_050.0]),
            [SimCommand::Reclaim { units: vec![SimId(1)], target: SimId(9) }]
        );
    }

    #[test]
    fn right_clicking_ground_moves_units_and_sets_factory_rally_points() {
        let db = db();
        let units = [
            view(1, Team::Blue, db.id("t1_light_walker"), &db, 2_000.0, 1_950.0),
            view(2, Team::Blue, db.id("land_factory_t1"), &db, 1_900.0, 1_900.0),
        ];
        let orders = click_at(&[SimId(1), SimId(2)], &units, &[], &db, &camera(), [2_050.0, 2_080.0]);
        let point = Vec2Fp::new(Fp::from_num(2_050.0), Fp::from_num(2_080.0));
        assert_eq!(
            orders,
            [
                SimCommand::Move { units: vec![SimId(1)], goal: point },
                SimCommand::SetRally { factory: SimId(2), point },
            ]
        );
    }

    #[test]
    fn right_clicking_an_enemy_orders_missile_launchers_to_fire_and_point_defenses_in_reach() {
        let db = db();
        let reach = db.get(db.id("t1_point_defense")).weapon.expect("a gun").range.to_num::<f64>();
        let units = [
            view(1, Team::Blue, db.id("tactical_missile_launcher"), &db, 1_000.0, 1_000.0),
            view(2, Team::Blue, db.id("strategic_missile_launcher"), &db, 1_100.0, 1_000.0),
            // One point defense out of reach of the target, and one in it.
            view(3, Team::Blue, db.id("t1_point_defense"), &db, 1_200.0, 1_000.0),
            view(5, Team::Blue, db.id("t1_point_defense"), &db, 2_000.0, 2_000.0 - reach + 20.0),
            view(4, Team::Red, db.id("power_generator"), &db, 2_000.0, 2_000.0),
        ];
        let mut units = units;
        units.sort_by_key(|unit| unit.id);
        let orders = click_at(&[SimId(1), SimId(2), SimId(3), SimId(5)], &units, &[], &db, &camera(), [2_000.0, 2_000.0]);
        assert_eq!(orders, [SimCommand::Attack { units: vec![SimId(1), SimId(2), SimId(5)], target: SimId(4) }], "the far point defense keeps to its own targets");
    }

    /// Launch (L) fires the selected missile launchers at the enemy under the cursor, or else at
    /// the ground there. Launchers that stand out of reach are left out, a missile submarine
    /// sails into range, and an enemy no missile can hit, such as an aircraft, is passed over for
    /// the ground beneath it.
    #[test]
    fn launch_fires_the_selected_launchers_that_reach_at_an_enemy_or_the_ground() {
        let db = db();
        let mut units = vec![
            view(1, Team::Blue, db.id("tactical_missile_launcher"), &db, 1_000.0, 1_000.0),
            view(2, Team::Blue, db.id("strategic_missile_launcher"), &db, 1_100.0, 1_000.0),
            // 7,000 off: beyond a tactical launcher's 5,120.
            view(3, Team::Blue, db.id("tactical_missile_launcher"), &db, 9_000.0, 2_000.0),
            view(4, Team::Blue, db.id("t1_medium_tank"), &db, 1_500.0, 1_500.0),
            view(5, Team::Red, db.id("power_generator"), &db, 2_000.0, 2_000.0),
            view(6, Team::Red, db.id("t1_interceptor"), &db, 2_300.0, 1_700.0),
            view(7, Team::Blue, db.id("t3_missile_sub"), &db, 30_000.0, 2_000.0),
        ];
        units.sort_by_key(|unit| unit.id);
        let camera = camera();
        let selected = [SimId(1), SimId(2), SimId(3), SimId(4), SimId(7)];
        let at = |world: [f64; 2]| launch_orders(&click_for(&selected, &units, &db, &camera, world));
        assert_eq!(at([2_000.0, 2_000.0]), [SimCommand::Attack { units: vec![SimId(1), SimId(2), SimId(7)], target: SimId(5) }], "the far launcher is left out, and the tank");
        assert_eq!(at([2_400.0, 2_300.0]), [SimCommand::AttackGround { units: vec![SimId(1), SimId(2), SimId(7)], point: Vec2Fp::new(Fp::from_num(2_400.0), Fp::from_num(2_300.0)) }]);
        let under_aircraft = at([2_300.0, 1_700.0]);
        assert!(matches!(under_aircraft.as_slice(), [SimCommand::AttackGround { .. }]), "missiles do not chase aircraft: {under_aircraft:?}");
        assert!(launch_orders(&click_for(&[SimId(3)], &units, &db, &camera, [2_000.0, 2_000.0])).is_empty(), "out of reach");
        assert!(launch_orders(&click_for(&[SimId(4)], &units, &db, &camera, [2_000.0, 2_000.0])).is_empty(), "a tank launches nothing");
    }

    /// The pointer turns into a red target over an enemy only when a right-click there attacks
    /// or launches, worked out from the orders it gives: not when it captures, and not allowed
    /// when nothing selected reaches it or can hit it. A launcher that stands out of reach is left
    /// out of a right-click's attack, rather than told to wait on what it will never fire at.
    #[test]
    fn the_pointer_shows_a_target_only_where_a_right_click_attacks_or_launches() {
        let db = db();
        let mut units = vec![
            view(1, Team::Blue, db.id("tactical_missile_launcher"), &db, 1_000.0, 1_000.0),
            view(3, Team::Blue, db.id("tactical_missile_launcher"), &db, 9_000.0, 2_000.0),
            view(4, Team::Blue, db.id("t1_medium_tank"), &db, 1_500.0, 1_500.0),
            view(5, Team::Red, db.id("power_generator"), &db, 2_000.0, 2_000.0),
            view(6, Team::Red, db.id("t1_interceptor"), &db, 2_300.0, 1_700.0),
            view(8, Team::Blue, db.id("t1_engineer"), &db, 1_600.0, 1_500.0),
        ];
        units.sort_by_key(|unit| unit.id);
        let camera = camera();
        let target = |selected: &[SimId], world: [f64; 2]| right_click_target(&click_for(selected, &units, &db, &camera, world));
        let generator = [2_000.0, 2_000.0];
        assert_eq!(target(&[SimId(4)], generator), Some(PointerTarget::Attack));
        assert_eq!(target(&[SimId(1), SimId(4)], generator), Some(PointerTarget::Launch));
        assert_eq!(target(&[SimId(3)], generator), Some(PointerTarget::OutOfReach), "the far launcher cannot reach");
        assert_eq!(click_at(&[SimId(1), SimId(3)], &units, &[], &db, &camera, generator), [SimCommand::Attack { units: vec![SimId(1)], target: SimId(5) }]);
        assert_eq!(target(&[SimId(4)], [2_300.0, 1_700.0]), Some(PointerTarget::OutOfReach), "a tank cannot hit an aircraft");
        assert_eq!(target(&[SimId(8)], generator), None, "engineers capture it");
        assert_eq!(target(&[SimId(4)], [2_500.0, 2_500.0]), None, "no enemy there");
    }

    #[test]
    fn right_clicking_an_enemy_has_engineers_capture_it_while_armed_units_attack() {
        let db = db();
        let units = [
            view(1, Team::Blue, db.id("t1_engineer"), &db, 1_000.0, 1_000.0),
            view(2, Team::Blue, db.id("command_rig"), &db, 1_050.0, 1_000.0),
            view(3, Team::Red, db.id("power_generator"), &db, 2_000.0, 2_000.0),
            view(4, Team::Red, db.id("command_rig"), &db, 3_000.0, 3_000.0),
        ];
        let at = |selected: &[SimId], cursor: [f64; 2]| click_at(selected, &units, &[], &db, &camera(), cursor);
        assert_eq!(at(&[SimId(1)], [2_000.0, 2_000.0]), [SimCommand::Capture { units: vec![SimId(1)], target: SimId(3) }]);
        assert_eq!(
            at(&[SimId(1), SimId(2)], [2_000.0, 2_000.0]),
            [SimCommand::Attack { units: vec![SimId(2)], target: SimId(3) }, SimCommand::Capture { units: vec![SimId(1)], target: SimId(3) }],
            "the commander fights, and the engineer captures"
        );
        assert!(!at(&[SimId(1)], [3_000.0, 3_000.0]).iter().any(|order| matches!(order, SimCommand::Capture { .. })), "a commander cannot be captured");
    }

    #[test]
    fn hidden_enemies_cannot_be_targeted() {
        let db = db();
        // The enemy is simply absent from the snapshot, so the click becomes a move.
        let units = [view(1, Team::Blue, db.id("t1_light_walker"), &db, 2_000.0, 1_950.0)];
        let orders = click_at(&[SimId(1)], &units, &[], &db, &camera(), [2_000.0, 2_050.0]);
        assert!(matches!(orders[0], SimCommand::Move { .. }));
    }

    #[test]
    fn placement_preview_checks_deposits_walls_and_structures() {
        let db = db();
        let (generator, extractor, factory) = (db.id("power_generator"), db.id("mass_extractor"), db.id("land_factory_t1"));
        let blocked = [(10, 10)];
        let deposits = [(20, 20)];
        let map = PlacementMap { cells_per_side: 512, blocked_cells: &blocked, deposits: &deposits, vents: &[], water_cells: &[], blueprints: &db };
        let existing = [view(1, Team::Blue, factory, &db, 1_030.0, 1_030.0)];
        assert!(placement_is_valid(&map, generator, (100, 100), &existing, &[]));
        assert!(!placement_is_valid(&map, generator, (10, 10), &existing, &[]), "on a wall");
        assert!(!placement_is_valid(&map, generator, (51, 51), &existing, &[]), "on a structure");
        assert!(!placement_is_valid(&map, generator, (20, 20), &existing, &[]), "on a deposit");
        assert!(placement_is_valid(&map, extractor, (20, 20), &existing, &[]));
        assert!(!placement_is_valid(&map, extractor, (21, 20), &existing, &[]), "off a deposit");
        assert!(!placement_is_valid(&map, factory, (510, 100), &existing, &[]), "off the map");
    }

    fn flat_heights() -> HeightField {
        HeightField::new(513, vec![0; 513 * 513], 20.0, 1.0 / 16.0)
    }

    #[test]
    fn extractors_snap_to_the_nearest_free_deposit_at_any_zoom() {
        let db = db();
        let extractor = db.id("mass_extractor");
        let heights = flat_heights();
        let deposits = [(100, 100), (110, 100), (140, 100)];
        let map = PlacementMap { cells_per_side: 512, blocked_cells: &[], deposits: &deposits, vents: &[], water_cells: &[], blueprints: &db };
        let target_at = |camera: &Camera, offset_px: f64| {
            let screen = camera.world_to_screen(deposit_position((100, 100), &heights)).unwrap();
            let cursor = [screen[0] + offset_px, screen[1]];
            placement_target(&map, extractor, cursor, camera.screen_to_ground(cursor, &heights), camera, &heights, &[], &[], 1.0)
        };
        for altitude in [200.0, 1_000.0, 6_000.0] {
            let mut camera = Camera::new(10_240.0, [1600.0, 1000.0]);
            camera.look_at(2_000.0, 2_000.0, altitude);
            assert_eq!(target_at(&camera, -25.0), Some((100, 100)), "25 pixels off at altitude {altitude}");
        }
        // Close in, a cursor far from every deposit does not snap.
        let mut camera = Camera::new(10_240.0, [1600.0, 1000.0]);
        camera.look_at(2_000.0, 2_000.0, 200.0);
        assert!(!deposits.contains(&target_at(&camera, -300.0).unwrap()));
    }

    #[test]
    fn extractors_skip_deposits_that_are_taken_or_already_queued() {
        let db = db();
        let extractor = db.id("mass_extractor");
        let heights = flat_heights();
        let deposits = [(100, 100), (102, 100)];
        let map = PlacementMap { cells_per_side: 512, blocked_cells: &[], deposits: &deposits, vents: &[], water_cells: &[], blueprints: &db };
        let mut camera = Camera::new(10_240.0, [1600.0, 1000.0]);
        camera.look_at(2_000.0, 2_000.0, 1_500.0);
        let cursor = camera.world_to_screen(deposit_position((100, 100), &heights)).unwrap();
        let target = |pending: &[PendingBuild]| placement_target(&map, extractor, cursor, None, &camera, &heights, &[], pending, 1.0);
        assert_eq!(target(&[]), Some((100, 100)));
        let queued = deposit_position((100, 100), &heights);
        assert_eq!(target(&[PendingBuild { center: queued, width: 20.0 }]), Some((102, 100)), "the queued one is taken");
    }

    #[test]
    fn other_structures_do_not_snap_to_deposits() {
        let db = db();
        let generator = db.id("power_generator");
        let heights = flat_heights();
        let deposits = [(100, 100)];
        let map = PlacementMap { cells_per_side: 512, blocked_cells: &[], deposits: &deposits, vents: &[], water_cells: &[], blueprints: &db };
        let mut camera = Camera::new(10_240.0, [1600.0, 1000.0]);
        camera.look_at(2_000.0, 2_000.0, 300.0);
        let target = placement_target(&map, generator, [800.0, 500.0], Some([2_030.0, 2_010.0]), &camera, &heights, &[], &[], 1.0);
        assert_eq!(target, Some(placement_cell([2_030.0, 2_010.0], 1)));
    }

    #[test]
    fn a_line_lays_structures_side_by_side() {
        assert_eq!(line_cells((10, 10), (10, 10), 1), [(10, 10)], "no drag means one building");
        assert_eq!(line_cells((10, 10), (13, 10), 1), [(10, 10), (11, 10), (12, 10), (13, 10)]);
        assert_eq!(line_cells((10, 10), (10, 7), 1), [(10, 10), (10, 9), (10, 8), (10, 7)], "lines can run backwards");
        assert_eq!(line_cells((10, 10), (17, 10), 3), [(10, 10), (13, 10), (16, 10)], "wide buildings stay touching");
        assert_eq!(line_cells((10, 10), (11, 10), 3), [(10, 10)], "too short a drag for a second one");
        assert_eq!(line_cells((10, 10), (13, 13), 1), [(10, 10), (11, 11), (12, 12), (13, 13)], "diagonals work");
    }

    #[test]
    fn no_two_structures_in_a_line_overlap_and_the_row_stays_on_course() {
        for (end, footprint) in [((40, 17), 3u8), ((17, 40), 3), ((35, 22), 1), ((3, 30), 2), ((45, 45), 3)] {
            let cells = line_cells((10, 10), end, footprint);
            for pair in cells.windows(2) {
                let apart = pair[0].0.abs_diff(pair[1].0).max(pair[0].1.abs_diff(pair[1].1));
                assert!(apart >= u32::from(footprint), "{pair:?} overlap with footprint {footprint}");
            }
            let last = cells[cells.len() - 1];
            assert!(last.0.abs_diff(end.0).max(last.1.abs_diff(end.1)) < u32::from(footprint), "stopped short of {end:?} at {last:?}");
        }
    }

    #[test]
    fn a_line_has_a_length_limit() {
        assert_eq!(line_cells((0, 0), (500, 0), 1).len(), MAX_LINE_BUILDS as usize);
    }

    #[test]
    fn queued_builds_block_placement_but_their_neighbors_are_free() {
        let db = db();
        let (generator, factory) = (db.id("power_generator"), db.id("land_factory_t1"));
        let map = PlacementMap { cells_per_side: 512, blocked_cells: &[], deposits: &[], vents: &[], water_cells: &[], blueprints: &db };
        // A queued factory covering cells 50 to 52.
        let pending = [PendingBuild { center: [1_030.0, 1_030.0, 0.0], width: 60.0 }];
        assert!(!placement_is_valid(&map, generator, (51, 51), &[], &pending), "on top of a queued build");
        assert!(!placement_is_valid(&map, factory, (52, 50), &[], &pending), "overlapping one column of it");
        assert!(placement_is_valid(&map, generator, (53, 51), &[], &pending), "touching is how adjacency works");
        assert!(placement_is_valid(&map, generator, (49, 51), &[], &pending), "touching is how adjacency works");
    }

    #[test]
    fn pending_builds_cover_every_plan_and_count_shared_spots_once() {
        use crate::sim::world::{PlanStep, PlanStepKind, UnitPlanView};
        let raw = |value: f64| (value * 4096.0) as i32;
        let step = |x: f64, width: f64| PlanStep { kind: PlanStepKind::Build, x: raw(x), y: raw(50.0), z: 0, pending_width: raw(width) };
        let snapshot = SimSnapshot {
            plans: vec![
                UnitPlanView { id: SimId(1), steps: vec![step(10.0, 0.0), step(100.0, 20.0)] },
                UnitPlanView { id: SimId(2), steps: vec![step(100.0, 20.0), step(200.0, 60.0)] },
            ],
            ..SimSnapshot::default()
        };
        let builds = pending_builds(&snapshot);
        assert_eq!(builds.len(), 2, "a started build has no outline, and a shared spot counts once");
        assert_eq!(builds[0], PendingBuild { center: [100.0, 50.0, 0.0], width: 20.0 });
        assert_eq!(builds[1].width, 60.0);
    }

    #[test]
    fn placement_cell_centers_the_footprint_on_the_cursor() {
        assert_eq!(placement_cell([1_010.0, 1_010.0], 1), (50, 50));
        assert_eq!(placement_cell([1_030.0, 1_030.0], 3), (50, 50));
    }
}
