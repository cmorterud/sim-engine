//! Phase 5: selection and orders end to end, from screen coordinates to moving units.
//! Everything except the operating system delivering the mouse events is covered here.

use sim_engine::driver::{CommandSource, SimDriver};
use sim_engine::input::selection::{order_command, Selection};
use sim_engine::render::camera::{Camera, HeightField};
use sim_engine::sim::components::Team;
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::scenario;
use sim_engine::sim::world::SimSnapshot;
use std::sync::Arc;

const VIEWPORT: [f64; 2] = [1600.0, 1000.0];

#[test]
fn drag_select_and_right_click_moves_only_the_selected_units() {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, ..SimConfig::default() };
    let mut scenario = scenario::battle(4, 100, config);
    // No opening orders: the only movement should come from the player's command.
    scenario.opening_commands.clear();
    let samples = scenario.world.terrain.samples_per_side();
    let heights = HeightField::new(samples, scenario.world.terrain.heights().to_vec(), 20.0, 1.0 / 16.0);
    let mut camera = Camera::new(5_120.0, VIEWPORT);
    camera.look_at(2_560.0, 2_560.0, 150.0);

    let driver = SimDriver::start(scenario, CommandSource::Player, false);
    let mut latest: Arc<SimSnapshot> = loop {
        if let Some(snapshot) = driver.try_snapshot() {
            break snapshot;
        }
    };
    let start = Arc::clone(&latest);

    // Drag a box around the left half of the screen, where the Blue block stands.
    let mut selection = Selection::default();
    selection.move_cursor([0.0, 0.0]);
    selection.begin_drag();
    selection.move_cursor([VIEWPORT[0] / 2.0, VIEWPORT[1]]);
    assert!(selection.drag_box().is_some());
    selection.end_drag(&start.units, &camera, Team::Blue);
    let selected = selection.selected().to_vec();
    assert!(selected.len() >= 90, "only {} of 100 Blue units selected", selected.len());
    for id in &selected {
        let unit = start.units.iter().find(|unit| unit.id == *id).unwrap();
        assert_eq!(unit.team, Team::Blue);
    }

    // Right-click on open ground to the north.
    let target = camera.world_to_screen([2_480.0, 2_640.0, 0.0]).unwrap();
    let ground = camera.screen_to_ground(target, &heights).unwrap();
    assert!((ground[0] - 2_480.0).abs() < 1.0 && (ground[1] - 2_640.0).abs() < 1.0, "clicked {ground:?}");
    driver.send(order_command(&selected, ground, false));

    // Run until the group reaches the point it was sent to.
    let goal = [ground[0] * 4096.0, ground[1] * 4096.0];
    let distance = |x: i32, y: i32| ((f64::from(x) - goal[0]).hypot(f64::from(y) - goal[1])) / 4096.0;
    let spread = 30.0;
    let mut arrived = 0;
    for _ in 0..2_000_000 {
        if let Some(snapshot) = driver.try_snapshot() {
            latest = snapshot;
            arrived = latest
                .units
                .iter()
                .filter(|unit| selected.binary_search(&unit.id).is_ok())
                .filter(|unit| distance(unit.x, unit.y) < spread)
                .count();
            if arrived * 10 >= selected.len() * 9 || latest.tick > 1_000 {
                break;
            }
        } else {
            std::thread::yield_now();
        }
    }
    assert!(arrived * 10 >= selected.len() * 9, "only {arrived} of {} units arrived by tick {}", selected.len(), latest.tick);

    // Red was neither selected nor ordered, so it can only have been nudged by contact.
    for unit in latest.units.iter().filter(|unit| unit.team == Team::Red) {
        let before = start.units.iter().find(|other| other.id == unit.id).unwrap();
        let moved = (f64::from(unit.x - before.x)).hypot(f64::from(unit.y - before.y)) / 4096.0;
        assert!(moved < 10.0, "unselected unit {:?} moved {moved} units", unit.id);
    }
    driver.finish();
}
