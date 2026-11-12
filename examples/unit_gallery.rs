//! Lines up every unit and structure on flat ground and saves pictures of them, for comparing
//! how units look before and after a change to the meshes or shaders.
//!
//! `cargo run --release --example unit_gallery -- <output folder>`
//!
//! Each row is one kind of unit: land, air, navy, then structures. Blue's units stand in front,
//! with Red's copies behind them.
use std::path::PathBuf;
use std::sync::Arc;

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::render::camera::{Camera, HeightField};
use sim_engine::render::instances::{build_unit_instances, unit_looks, Headings, InstanceBatches};
use sim_engine::render::pipeline::{FrameScene, Renderer};
use sim_engine::sim::blueprints::UnitClass;
use sim_engine::sim::components::{MoveClass, Team};
use sim_engine::sim::config::{SimConfig, TerrainStyle, TERRAIN_CELL_UNITS};
use sim_engine::sim::terrain::{Terrain, HEIGHT_FRAC_BITS};
use sim_engine::sim::world::SimWorld;
use winit::dpi::LogicalSize;
use winit::event_loop::EventLoop;
use winit::window::WindowBuilder;

const MAP: i32 = 5_120;
const MOBILE_SPACING: i32 = 70;
const RED_BEHIND: i32 = 70;
/// Ships and naval structures need water: a strip across the map, between these heights.
const WATER: std::ops::RangeInclusive<i32> = 1_600..=2_100;

/// Flat land 20 above the water, with the strip of water 40 below it.
fn world() -> SimWorld {
    let cells = (MAP / TERRAIN_CELL_UNITS) as usize;
    let samples = cells + 1;
    let height = |y: usize| if WATER.contains(&(y as i32 * TERRAIN_CELL_UNITS)) { -40 } else { 20 };
    let heights: Vec<i16> = (0..samples * samples).map(|index| (height(index / samples) << HEIGHT_FRAC_BITS) as i16).collect();
    let blocked: Vec<bool> = (0..cells * cells).map(|cell| height(cell / cells) < 0 || height(cell / cells + 1) < 0).collect();
    let terrain = Terrain::from_heights(MAP, heights, &blocked, Fp::ZERO);
    let mut world = SimWorld::with_terrain(SimConfig { map_units: MAP, terrain: TerrainStyle::Flat, ..SimConfig::default() }, 1, terrain);
    world.prepare_map();
    world
}

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).expect("output folder"));
    std::fs::create_dir_all(&out).expect("output folder can be made");
    let mut world = world();
    let blueprints = Arc::clone(&world.blueprints);
    let drawn: Vec<_> = blueprints.iter().filter(|unit| unit.class != UnitClass::Ammo).collect();
    let row_of = |unit: &&sim_engine::sim::blueprints::Blueprint| match unit.movement {
        _ if unit.is_air() => 1,
        MoveClass::Naval | MoveClass::Submarine => 2,
        _ => 0,
    };
    // Where each row's middle is, for the camera.
    let mut rows: Vec<(String, [f64; 2], f64)> = Vec::new();
    for (row, name) in ["land", "air", "navy"].into_iter().enumerate() {
        let units: Vec<_> = drawn.iter().filter(|unit| !unit.is_structure() && row_of(unit) == row).collect();
        let y = [1_000, 1_400, 1_820][row];
        for (index, unit) in units.iter().enumerate() {
            let x = 600 + index as i32 * MOBILE_SPACING;
            world.spawn_mobile(unit.id, Team::Blue, Vec2Fp::new(fp_int(x), fp_int(y)));
            world.spawn_mobile(unit.id, Team::Red, Vec2Fp::new(fp_int(x), fp_int(y + RED_BEHIND)));
        }
        let width = units.len() as i32 * MOBILE_SPACING;
        // Aircraft fly a few hundred units up, so the camera stands well above them.
        let altitude = if row == 1 { f64::from(width) * 0.55 + 600.0 } else { f64::from(width) * 0.55 };
        rows.push((name.to_string(), [f64::from(600 + width / 2), f64::from(y + RED_BEHIND / 2)], altitude));
    }
    // Structures, in rows of a set width, Blue's only: they are painted the same way as units.
    let structures: Vec<_> = drawn.iter().filter(|unit| unit.is_structure()).collect();
    let (mut cell_x, mut cell_y, mut tallest) = (30_u32, 120_u32, 0_u32);
    let row_start = cell_y;
    let mut water_x = 62_u32;
    for unit in structures {
        let size = u32::from(unit.footprint);
        if unit.on_water {
            world.spawn_finished_structure(unit.id, Team::Blue, water_x, 92);
            water_x += size + 3;
            continue;
        }
        if cell_x + size > 130 {
            cell_x = 30;
            cell_y += tallest + 3;
            tallest = 0;
        }
        world.spawn_finished_structure(unit.id, Team::Blue, cell_x, cell_y);
        cell_x += size + 3;
        tallest = tallest.max(size);
    }
    let cell = TERRAIN_CELL_UNITS;
    let middle_y = (row_start + cell_y + tallest) as i32 / 2 * cell;
    rows.push(("structures".to_string(), [f64::from(80 * cell), f64::from(middle_y)], f64::from(55 * cell)));
    rows.push(("structures_close".to_string(), [f64::from(55 * cell), f64::from(row_start as i32 * cell + 120)], f64::from(20 * cell)));
    rows.push(("land_close".to_string(), [800.0, 1_035.0], 260.0));
    rows.push(("navy_wide".to_string(), [1_500.0, 1_860.0], 700.0));

    let snapshot = world.snapshot();
    if std::env::var_os("GALLERY_DEBUG").is_some() {
        for unit in &snapshot.units {
            println!("{} {} at {} {} {}", unit.id.0, blueprints.get(sim_engine::sim::blueprints::BlueprintId(unit.blueprint)).key, unit.x >> 12, unit.y >> 12, unit.z >> 12);
        }
    }
    let heights = HeightField::new(
        world.terrain.samples_per_side(),
        world.terrain.heights().to_vec(),
        f64::from(cell),
        1.0 / f64::from(1 << HEIGHT_FRAC_BITS),
    );
    let looks = unit_looks(&blueprints);

    let event_loop = EventLoop::new().expect("event loop starts");
    let window = Arc::new(WindowBuilder::new().with_title("unit gallery").with_inner_size(LogicalSize::new(1440.0, 900.0)).build(&event_loop).expect("window opens"));
    let size = window.inner_size();
    let mut renderer = Renderer::new(Arc::clone(&window), &heights, None);
    renderer.set_water_level(0.0);
    for (name, focus, altitude) in rows {
        let mut camera = Camera::new(f64::from(MAP), [f64::from(size.width), f64::from(size.height)]);
        camera.look_at(focus[0], focus[1], altitude);
        camera.update(0.0, &heights);
        let mut instances = InstanceBatches::default();
        build_unit_instances(&snapshot.units, &snapshot.units, 1.0, camera.eye(), &[], &looks, &[], &mut Headings::new(), &mut instances);
        let scene = FrameScene { camera: &camera, instances: &instances, selection_box: None };
        let path = out.join(format!("{name}.bmp"));
        renderer.capture(&scene, None, &path).expect("capture saves");
        println!("saved {}", path.display());
    }
}
