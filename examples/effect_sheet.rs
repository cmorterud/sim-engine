//! Draws each weapon's shot in flight and its blast at a few moments after it lands, a row a
//! weapon, into one picture, to judge the effects side by side: they are hard to catch in a game,
//! where a battleship, a torpedo, or a nuke seldom fires where the camera is.
//!
//! `cargo run --release --example effect_sheet [-- --out <sheet.png>]`
//!
//! The rows are printed, in order, as they are drawn. Nothing here is the simulation's: each shot
//! is a snapshot made up by hand, given to the renderer's effects as a game would give it, with
//! the unit that fired it in sight, so that it looks as that unit's weapon does. Like the
//! portrait sheet, it opens a window while it draws.
use std::sync::Arc;

use sim_engine::render::camera::{Camera, HeightField};
use sim_engine::render::effects::Effects;
use sim_engine::render::instances::{build_projectile_instances, unit_looks, InstanceBatches};
use sim_engine::render::pipeline::{FrameScene, Renderer};
use sim_engine::sim::blueprints::BlueprintDb;
use sim_engine::sim::components::{SimId, Team};
use sim_engine::sim::terrain::HEIGHT_FRAC_BITS;
use sim_engine::sim::world::{ProjectileKind, ProjectileView, SimSnapshot, UnitView};
use winit::dpi::LogicalSize;
use winit::event_loop::EventLoop;
use winit::window::WindowBuilder;

const MAP: f64 = 16_384.0;
const CELL: f64 = 20.0;
/// Land is this high, and the sea floor this deep, under a sea level of 0 across the map's
/// middle third, from north to south.
const LAND: f64 = 20.0;
const SEA_FLOOR: f64 = -40.0;
/// A tile of the sheet, in pixels.
const TILE: usize = 240;

fn ground(x: f64, _y: f64) -> f64 {
    if (MAP / 3.0..MAP * 2.0 / 3.0).contains(&x) { SEA_FLOOR } else { LAND }
}

/// One row: which unit fires, what kind of projectile, whether it lands in the sea, how high
/// over the ground it bursts, how far off the camera stands for the blast, and when after it
/// lands each blast tile is drawn.
struct Row {
    key: &'static str,
    kind: ProjectileKind,
    at_sea: bool,
    burst_height: f64,
    stand_off: f64,
    moments: [f64; 3],
}

fn raw(value: f64) -> i32 {
    (value * 4096.0).round() as i32
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let out = arguments.iter().position(|argument| argument == "--out").and_then(|at| arguments.get(at + 1)).cloned().unwrap_or_else(|| "target/effect_sheet.png".to_string());
    let soon = [0.05, 0.3, 1.0];
    let rows = [
        Row { key: "t1_medium_tank", kind: ProjectileKind::Shell, at_sea: false, burst_height: 0.0, stand_off: 70.0, moments: soon },
        Row { key: "t2_point_defense", kind: ProjectileKind::Shell, at_sea: false, burst_height: 0.0, stand_off: 160.0, moments: soon },
        Row { key: "t1_light_artillery", kind: ProjectileKind::Shell, at_sea: false, burst_height: 0.0, stand_off: 150.0, moments: soon },
        Row { key: "t2_artillery", kind: ProjectileKind::Shell, at_sea: false, burst_height: 0.0, stand_off: 300.0, moments: soon },
        Row { key: "t3_artillery", kind: ProjectileKind::Shell, at_sea: false, burst_height: 0.0, stand_off: 340.0, moments: soon },
        Row { key: "t3_battleship", kind: ProjectileKind::Shell, at_sea: false, burst_height: 0.0, stand_off: 300.0, moments: soon },
        Row { key: "t3_battleship", kind: ProjectileKind::Shell, at_sea: true, burst_height: 0.0, stand_off: 300.0, moments: soon },
        Row { key: "t2_flak", kind: ProjectileKind::Shell, at_sea: false, burst_height: 120.0, stand_off: 320.0, moments: soon },
        Row { key: "t1_bomber", kind: ProjectileKind::Shell, at_sea: false, burst_height: 0.0, stand_off: 200.0, moments: soon },
        Row { key: "t1_torpedo_launcher", kind: ProjectileKind::Shell, at_sea: true, burst_height: 0.0, stand_off: 110.0, moments: soon },
        Row { key: "tactical_missile_launcher", kind: ProjectileKind::TacticalMissile, at_sea: false, burst_height: 0.0, stand_off: 150.0, moments: soon },
        Row { key: "strategic_missile_launcher", kind: ProjectileKind::StrategicMissile, at_sea: false, burst_height: 0.0, stand_off: 2_000.0, moments: [0.5, 3.0, 8.0] },
    ];

    let db = BlueprintDb::standard();
    let looks = unit_looks(&db);
    let cells = (MAP / CELL) as usize;
    let samples = cells + 1;
    let heights: Vec<i16> = (0..samples * samples).map(|index| (ground((index % samples) as f64 * CELL, (index / samples) as f64 * CELL) * f64::from(1 << HEIGHT_FRAC_BITS)) as i16).collect();
    let field = HeightField::new(samples, heights, CELL, 1.0 / f64::from(1 << HEIGHT_FRAC_BITS));
    let event_loop = EventLoop::new().expect("event loop starts");
    let window = Arc::new(WindowBuilder::new().with_title("effects").with_inner_size(LogicalSize::new(720.0, 720.0)).build(&event_loop).expect("window opens"));
    let size = window.inner_size();
    let mut renderer = Renderer::new(Arc::clone(&window), &field, None);
    renderer.set_water_level(0.0);
    let water = Some(0.0);

    let mut tiles: Vec<Vec<u8>> = Vec::new();
    for row in &rows {
        let blueprint = db.get(db.id(row.key));
        let splash = blueprint.weapon.map_or(0, |weapon| weapon.splash_radius.to_bits());
        let (x, y) = (if row.at_sea { MAP * 0.5 } else { MAP * 0.15 }, MAP * 0.5);
        let floor = ground(x, y).max(if row.at_sea { 0.0 } else { f64::MIN });
        let shooter = UnitView {
            id: SimId(1),
            team: Team::Blue,
            x: raw(x - 400.0),
            y: raw(y),
            z: raw(LAND),
            radius: raw(8.0),
            blueprint: blueprint.id.0,
            health_permille: 1000,
            built_permille: 1000,
            is_structure: blueprint.is_structure(),
            facing: [4096, 0],
            aim: [4096, 0],
            extra_aims: [[0; 2]; 3], pitches: [0; 4],
            working: false,
            assembling: None,
            upgrade: None,
            remembered: false,
            ally: false,
            owner: None,
        };
        // In flight: from the west, coming down at the target, over two ticks.
        let shot = |id: u32, along: f64, height: f64| ProjectileView {
            id: SimId(id),
            team: Team::Blue,
            x: raw(x - along),
            y: raw(y),
            z: raw(floor + row.burst_height + height),
            kind: row.kind,
            source: SimId(1),
            from: None,
            splash,
        };
        let snapshot = |tick: u32, projectiles: Vec<ProjectileView>, landed: Vec<ProjectileView>| SimSnapshot { tick, units: vec![shooter], projectiles, landed, ..SimSnapshot::default() };
        let (first, second) = (snapshot(1, vec![shot(5, 60.0, 20.0)], Vec::new()), snapshot(2, vec![shot(5, 30.0, 10.0)], Vec::new()));
        let mut flying = Effects::new();
        flying.note_events(&snapshot(0, Vec::new(), Vec::new()), &first, 0.1, &ground, water, &looks);
        flying.note_events(&first, &second, 0.2, &ground, water, &looks);
        for step in 0..=10 {
            let alpha = f64::from(step) / 10.0;
            flying.trail(&first, &second, alpha, &looks, water, 0.1 + alpha * 0.1);
        }
        // Blasts are seen at 45 degrees, as close up in a game, so that what rises shows; a nuclear
        // cloud from low down, nearly side on, and looking at the middle of the cloud, so that
        // its column and cap show.
        let nuclear = row.kind == ProjectileKind::StrategicMissile;
        let draw = |renderer: &mut Renderer, effects: &mut Effects, look: [f64; 3], altitude: f64, time: f64, projectiles: bool| {
            let mut camera = Camera::new(MAP, [f64::from(size.width), f64::from(size.height)]);
            camera.look_at(look[0], look[1], look[2] + altitude);
            camera.update(0.0, &field);
            if !projectiles {
                camera.orbit(0.0, if nuclear { 20.0 } else { 45.0 } - camera.pitch_degrees());
                camera.update(0.0, &field);
            }
            let beyond = (look[2] - ground(look[0], look[1]).max(0.0)).max(0.0) / camera.pitch_degrees().to_radians().tan();
            camera.look_at(look[0], look[1] + beyond, look[2] + altitude);
            camera.update(0.0, &field);
            let mut instances = InstanceBatches::default();
            if projectiles {
                build_projectile_instances(&first.projectiles, &second.projectiles, &[], 1.0, camera.eye(), &|projectile| effects.shot(projectile), &|_| None, water, &mut instances);
            }
            effects.build(time, camera.eye(), &mut instances);
            let (width, height, pixels) = renderer.capture_rgba(&FrameScene { camera: &camera, instances: &instances, selection_box: None, time: time as f32 }, None);
            shrink(&pixels, width as usize, height as usize)
        };
        let flight_at = [x - 30.0, y, floor + row.burst_height + 10.0];
        let flight_view = if row.kind == ProjectileKind::StrategicMissile { 160.0 } else { 70.0 };
        tiles.push(draw(&mut renderer, &mut flying, flight_at, flight_view, 0.2, true));
        // Then it lands, at tick 3, 0.3 s in, and its blast is drawn as it goes.
        let mut landing = Effects::new();
        let third = snapshot(3, Vec::new(), vec![shot(5, 0.0, 0.0)]);
        landing.note_events(&snapshot(0, Vec::new(), Vec::new()), &second, 0.1, &ground, water, &looks);
        landing.note_events(&second, &third, 0.3, &ground, water, &looks);
        let blast_at = [x, y, floor + row.burst_height + if nuclear { 700.0 } else { 0.0 }];
        for moment in row.moments {
            tiles.push(draw(&mut renderer, &mut landing, blast_at, row.stand_off, 0.3 + moment, false));
        }
        println!("{}{}: in flight, then {:?} s after it lands", row.key, if row.at_sea { " (at sea)" } else { "" }, row.moments);
    }

    let columns = 4;
    let (width, height) = (columns * TILE, rows.len() * TILE);
    let mut sheet = vec![0u8; width * height * 4];
    for (index, tile) in tiles.iter().enumerate() {
        let (left, top) = ((index % columns) * TILE, (index / columns) * TILE);
        for row in 0..TILE {
            let at = ((top + row) * width + left) * 4;
            sheet[at..at + TILE * 4].copy_from_slice(&tile[row * TILE * 4..(row + 1) * TILE * 4]);
        }
    }
    let file = std::fs::File::create(&out).expect("the sheet can be written");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().expect("header writes").write_image_data(&sheet).expect("sheet writes");
    println!("saved {out}");
}

/// The middle square of a capture, scaled down to a tile by averaging.
fn shrink(pixels: &[u8], width: usize, height: usize) -> Vec<u8> {
    let side = width.min(height);
    let (left, top) = ((width - side) / 2, (height - side) / 2);
    let mut tile = vec![0u8; TILE * TILE * 4];
    for ty in 0..TILE {
        for tx in 0..TILE {
            let (x0, x1) = (left + tx * side / TILE, left + ((tx + 1) * side / TILE).max(tx * side / TILE + 1));
            let (y0, y1) = (top + ty * side / TILE, top + ((ty + 1) * side / TILE).max(ty * side / TILE + 1));
            let mut sum = [0u32; 3];
            let mut count = 0;
            for y in y0..y1.min(height) {
                for x in x0..x1.min(width) {
                    let at = (y * width + x) * 4;
                    for channel in 0..3 {
                        sum[channel] += u32::from(pixels[at + channel]);
                    }
                    count += 1;
                }
            }
            let at = (ty * TILE + tx) * 4;
            for channel in 0..3 {
                tile[at + channel] = (sum[channel] / count.max(1)) as u8;
            }
            tile[at + 3] = 255;
        }
    }
    tile
}
