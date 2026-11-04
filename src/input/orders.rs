//! Turns a right-click into orders, depending on what is selected and what is under the cursor,
//! and checks structure placement. Works from snapshots, so that it respects fog of war.

use super::selection::{nearest_unit, order_command};
use crate::math::fp::{Fp, Vec2Fp};
use crate::render::camera::Camera;
use crate::sim::blueprints::{BlueprintDb, BlueprintId};
use crate::sim::commands::SimCommand;
use crate::sim::components::{SimId, Team};
use crate::sim::config::TERRAIN_CELL_UNITS;
use crate::sim::world::{UnitView, WreckView};

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
        if blueprint.is_factory() {
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
        if blueprint.build_power > Fp::ZERO {
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

    let enemy = nearest_unit(click.units, click.camera, click.team.enemy(), click.cursor);
    if let (Some(target), false) = (enemy, parts.armed.is_empty()) {
        commands.push(SimCommand::Attack { units: parts.armed.clone(), target });
        return commands;
    }

    let friend = nearest_unit(click.units, click.camera, click.team, click.cursor).filter(|id| !click.selected.contains(id));
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

fn wreck_under_cursor(click: &RightClick<'_>) -> Option<SimId> {
    let mut best: Option<(f64, SimId)> = None;
    for wreck in click.wrecks {
        let world = [f64::from(wreck.x) * RAW_TO_WORLD, f64::from(wreck.y) * RAW_TO_WORLD, f64::from(wreck.z) * RAW_TO_WORLD];
        if let Some(position) = click.camera.world_to_screen(world) {
            let distance = (position[0] - click.cursor[0]).hypot(position[1] - click.cursor[1]);
            if distance <= WRECK_CLICK_PX && best.is_none_or(|(closest, _)| distance < closest) {
                best = Some((distance, wreck.id));
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

/// Everything known to the player about where structures can go.
pub struct PlacementMap<'a> {
    pub cells_per_side: u32,
    /// Sorted (y, x) cells of walls, known from the start.
    pub blocked_cells: &'a [(u32, u32)],
    pub deposits: &'a [(u32, u32)],
    pub blueprints: &'a BlueprintDb,
}

/// A client-side preview of whether a structure fits. The simulation has the final say, and
/// it knows about hidden enemy structures that the player cannot.
pub fn placement_is_valid(map: &PlacementMap<'_>, blueprint: BlueprintId, cell: (u32, u32), units: &[UnitView]) -> bool {
    let unit = map.blueprints.get(blueprint);
    let size = u32::from(unit.footprint);
    if cell.0 + size > map.cells_per_side || cell.1 + size > map.cells_per_side {
        return false;
    }
    if unit.requires_deposit && !map.deposits.contains(&cell) {
        return false;
    }
    let cell_units = f64::from(TERRAIN_CELL_UNITS);
    let structures: Vec<(f64, f64, f64)> = units
        .iter()
        .filter(|other| other.is_structure)
        .map(|other| (f64::from(other.x) * RAW_TO_WORLD, f64::from(other.y) * RAW_TO_WORLD, f64::from(other.radius) * RAW_TO_WORLD))
        .collect();
    for y in cell.1..cell.1 + size {
        for x in cell.0..cell.0 + size {
            if map.blocked_cells.binary_search(&(y, x)).is_ok() {
                return false;
            }
            if !unit.requires_deposit && map.deposits.contains(&(x, y)) {
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
        })
    }

    #[test]
    fn attack_move_stays_an_attack_move_on_top_of_a_unit() {
        let db = db();
        let units = [
            view(1, Team::Blue, db.id("t1_light_bot"), &db, 2_000.0, 1_950.0),
            view(2, Team::Red, db.id("t1_light_bot"), &db, 2_000.0, 2_050.0),
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
        });
        assert!(orders.is_empty());
    }

    #[test]
    fn right_clicking_an_enemy_attacks_it_with_armed_units_only() {
        let db = db();
        let units = [
            view(1, Team::Blue, db.id("t1_light_bot"), &db, 2_000.0, 1_950.0),
            view(2, Team::Blue, db.id("t1_engineer"), &db, 2_010.0, 1_950.0),
            view(3, Team::Red, db.id("t1_light_bot"), &db, 2_000.0, 2_050.0),
        ];
        let orders = click_at(&[SimId(1), SimId(2)], &units, &[], &db, &camera(), [2_000.0, 2_050.0]);
        assert_eq!(orders, [SimCommand::Attack { units: vec![SimId(1)], target: SimId(3) }]);
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
        let wrecks = [WreckView { id: SimId(9), x: raw(1_900.0), y: raw(2_050.0), z: raw(8.0), radius: raw(8.0), mass: 10 }];
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
            view(1, Team::Blue, db.id("t1_light_bot"), &db, 2_000.0, 1_950.0),
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
    fn hidden_enemies_cannot_be_targeted() {
        let db = db();
        // The enemy is simply absent from the snapshot, so the click becomes a move.
        let units = [view(1, Team::Blue, db.id("t1_light_bot"), &db, 2_000.0, 1_950.0)];
        let orders = click_at(&[SimId(1)], &units, &[], &db, &camera(), [2_000.0, 2_050.0]);
        assert!(matches!(orders[0], SimCommand::Move { .. }));
    }

    #[test]
    fn placement_preview_checks_deposits_walls_and_structures() {
        let db = db();
        let (generator, extractor, factory) = (db.id("power_generator"), db.id("mass_extractor"), db.id("land_factory_t1"));
        let blocked = [(10, 10)];
        let deposits = [(20, 20)];
        let map = PlacementMap { cells_per_side: 512, blocked_cells: &blocked, deposits: &deposits, blueprints: &db };
        let existing = [view(1, Team::Blue, factory, &db, 1_030.0, 1_030.0)];
        assert!(placement_is_valid(&map, generator, (100, 100), &existing));
        assert!(!placement_is_valid(&map, generator, (10, 10), &existing), "on a wall");
        assert!(!placement_is_valid(&map, generator, (51, 51), &existing), "on a structure");
        assert!(!placement_is_valid(&map, generator, (20, 20), &existing), "on a deposit");
        assert!(placement_is_valid(&map, extractor, (20, 20), &existing));
        assert!(!placement_is_valid(&map, extractor, (21, 20), &existing), "off a deposit");
        assert!(!placement_is_valid(&map, factory, (510, 100), &existing), "off the map");
    }

    #[test]
    fn placement_cell_centers_the_footprint_on_the_cursor() {
        assert_eq!(placement_cell([1_010.0, 1_010.0], 1), (50, 50));
        assert_eq!(placement_cell([1_030.0, 1_030.0], 3), (50, 50));
    }
}
