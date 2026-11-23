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
use sim_engine::render::effects::Effects;
use sim_engine::render::instances::{build_unit_instances, unit_looks, InstanceBatches, Motion};
use sim_engine::render::links;
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

/// How high the ground is at a point: flat land 20 above the water, a strip of water 40 below it,
/// and beyond the structures, a hilly stretch for looking at the ground itself.
fn ground_height(x: f64, y: f64) -> f64 {
    if WATER.contains(&(y as i32)) {
        return -40.0;
    }
    let smooth = |from: f64, to: f64, value: f64| {
        let t = ((value - from) / (to - from)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let hills = smooth(HILLS_START, HILLS_START + 300.0, y);
    if hills == 0.0 {
        return 20.0;
    }
    // Rolling hills.
    let rolling = 14.0 * (x / 170.0).sin() * (y / 210.0).cos() + 6.0 * (x / 90.0 + y / 130.0).sin();
    // A plateau with cliffs all round.
    let plateau = 55.0 * smooth(2_700.0, 2_730.0, x) * (1.0 - smooth(3_670.0, 3_700.0, x)) * smooth(3_700.0, 3_730.0, y) * (1.0 - smooth(4_370.0, 4_400.0, y));
    // A gully running north to south.
    let gully = -18.0 * (-((x - 2_000.0) / 50.0).powi(2)).exp();
    // A lake with a gentle shore.
    let from_lake = (x - 1_300.0).hypot(y - 4_000.0);
    let lake = -45.0 * (1.0 - smooth(330.0, 510.0, from_lake));
    20.0 + hills * (rolling + plateau + gully + lake)
}

/// Where the hilly stretch begins, north of the structures.
const HILLS_START: f64 = 3_300.0;
/// Where a battle has been fought, on the hills, to see the marks it leaves.
const SCARS: [f64; 2] = [2_300.0, 3_560.0];

/// The marks of a battle around `SCARS`: shells that landed, and units that died and left wrecks.
fn battlefield() -> Effects {
    use sim_engine::sim::components::{SimId, WreckKind};
    use sim_engine::sim::world::{ProjectileKind, ProjectileView, SimSnapshot, UnitView, WreckView};
    let raw = |value: f64| (value * 4096.0) as i32;
    let spot = |index: u32, spread: f64| {
        let angle = f64::from(index) * 2.399;
        let reach = spread * (f64::from(index) / 40.0).sqrt();
        [SCARS[0] + angle.cos() * reach, SCARS[1] + angle.sin() * reach]
    };
    let mut before = SimSnapshot { tick: 1, ..SimSnapshot::default() };
    let mut after = SimSnapshot { tick: 2, ..SimSnapshot::default() };
    for index in 0..40 {
        let [x, y] = spot(index, 130.0);
        let kind = if index % 13 == 0 { ProjectileKind::TacticalMissile } else { ProjectileKind::Shell };
        before.projectiles.push(ProjectileView { id: SimId(1_000 + index), team: Team::Red, x: raw(x), y: raw(y), z: raw(ground_height(x, y) + 1.0), kind, source: SimId(1), from: None, splash: 0 });
    }
    for index in 0..6 {
        let [x, y] = spot(index * 7 + 3, 110.0);
        let (z, radius) = (ground_height(x, y), 4.0 + f64::from(index));
        before.units.push(UnitView {
            id: SimId(2_000 + index),
            team: Team::Blue,
            x: raw(x),
            y: raw(y),
            z: raw(z + radius),
            radius: raw(radius),
            blueprint: u16::MAX,
            health_permille: 0,
            built_permille: 1000,
            is_structure: false,
            facing: [0, 0],
            aim: [0, 0], extra_aims: [[0; 2]; 3],
            working: false,
            assembling: None,
            upgrade: None,
            remembered: false,
            ally: false,
        });
        after.wrecks.push(WreckView { id: SimId(3_000 + index), x: raw(x), y: raw(y), z: raw(z), radius: raw(radius), mass: 1, energy: 0, kind: WreckKind::Debris });
    }
    let mut effects = Effects::new();
    effects.note_events(&before, &after, 0.0, &ground_height, Some(0.0));
    effects
}

fn world() -> SimWorld {
    let cells = (MAP / TERRAIN_CELL_UNITS) as usize;
    let samples = cells + 1;
    let cell = f64::from(TERRAIN_CELL_UNITS);
    let sample_height = |index: usize| ground_height((index % samples) as f64 * cell, (index / samples) as f64 * cell);
    let heights: Vec<i16> = (0..samples * samples).map(|index| (sample_height(index) * f64::from(1 << HEIGHT_FRAC_BITS)).round() as i16).collect();
    // Cells with any corner under water are water.
    let blocked: Vec<bool> = (0..cells * cells)
        .map(|index| {
            let (x, y) = (index % cells, index / cells);
            [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)].iter().any(|&(x, y)| sample_height(y * samples + x) < 0.0)
        })
        .collect();
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
    // Where each row's middle is, for the camera, and how high above the ground what it shows flies.
    let mut rows: Vec<(String, [f64; 2], f64, f64)> = Vec::new();
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
        rows.push((name.to_string(), [f64::from(600 + width / 2), f64::from(y + RED_BEHIND / 2)], altitude, 0.0));
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
    // A factory with generators touching it, wired to it for their adjacency bonus, and an
    // engineer building another generator beside them.
    let factory = world.spawn_finished_structure(blueprints.id("land_factory_t1"), Team::Blue, 150, 122).expect("room for the factory");
    let generators: Vec<_> = [(154, 122), (154, 124), (149, 123), (151, 126)]
        .into_iter()
        .map(|(x, y)| world.spawn_finished_structure(blueprints.id("power_generator"), Team::Blue, x, y).expect("room for a generator"))
        .collect();
    let site = world.spawn_finished_structure(blueprints.id("power_generator"), Team::Blue, 158, 125).expect("room for the site");
    let engineer = world.spawn_mobile(blueprints.id("t1_engineer"), Team::Blue, Vec2Fp::new(fp_int(3_230), fp_int(2_440)));
    let cell = TERRAIN_CELL_UNITS;
    let middle_y = (row_start + cell_y + tallest) as i32 / 2 * cell;
    rows.push(("adjacency".to_string(), [3_080.0, 2_480.0], 260.0, 0.0));
    rows.push(("structures".to_string(), [f64::from(80 * cell), f64::from(middle_y)], f64::from(55 * cell), 0.0));
    rows.push(("structures_close".to_string(), [f64::from(55 * cell), f64::from(row_start as i32 * cell + 120)], f64::from(20 * cell), 0.0));
    rows.push(("land_close".to_string(), [800.0, 1_035.0], 260.0, 0.0));
    rows.push(("navy_wide".to_string(), [1_500.0, 1_860.0], 700.0, 0.0));
    // The ground itself: hills, a plateau with cliffs, a gully, and a lake.
    rows.push(("terrain".to_string(), [2_400.0, 3_900.0], 1_300.0, 0.0));
    rows.push(("terrain_cliff".to_string(), [2_950.0, 3_620.0], 320.0, 0.0));
    rows.push(("terrain_shore".to_string(), [1_700.0, 3_850.0], 300.0, 0.0));
    rows.push(("terrain_ground".to_string(), [2_300.0, 3_500.0], 90.0, 0.0));
    rows.push(("scars".to_string(), [SCARS[0], SCARS[1] + 40.0], 260.0, 0.0));
    rows.push(("smoke".to_string(), [SCARS[0], SCARS[1] + 40.0], 260.0, 0.0));

    let mut snapshot = world.snapshot();
    // Adjacency is reported at ground height under each structure's middle, as the simulation does.
    let center = |id| {
        let unit = snapshot.units.iter().find(|unit| unit.id == id).expect("spawned");
        let ground = world.terrain.height_at(Vec2Fp::new(Fp::from_bits(unit.x), Fp::from_bits(unit.y)));
        [unit.x, unit.y, ground.to_bits()]
    };
    snapshot.adjacency = generators.iter().map(|&generator| (center(generator), center(factory))).collect();
    let [x, y, z] = center(site);
    snapshot.work_links.push(sim_engine::sim::world::WorkLinkView { builder: engineer, target: [x, y, z + 5 * 4096], reclaim: false });
    if let Some(unit) = snapshot.units.iter_mut().find(|unit| unit.id == site) {
        unit.built_permille = 400;
    }
    // Each factory shows the first thing it builds in its bay, two thirds done, and gets a close-up.
    for unit in &mut snapshot.units {
        let made = blueprints.get(sim_engine::sim::blueprints::BlueprintId(unit.blueprint));
        if made.is_factory() {
            unit.assembling = made.builds.iter().find(|item| !blueprints.get(**item).is_factory()).map(|item| (item.0, 660));
            let (x, y, radius) = (f64::from(unit.x) / 4096.0, f64::from(unit.y) / 4096.0, f64::from(unit.radius) / 4096.0);
            if unit.team == Team::Blue && rows.iter().all(|row| row.0 != format!("{}_bay", made.key)) {
                rows.push((format!("{}_bay", made.key), [x, y], radius * 5.0, 0.0));
            }
        }
    }
    // `GALLERY_FOCUS=<blueprint key>` adds close-ups of Blue's copy of that unit, near and nearer.
    if let Ok(key) = std::env::var("GALLERY_FOCUS") {
        let id = blueprints.id(&key).0;
        let unit = snapshot.units.iter().find(|unit| unit.blueprint == id && unit.team == Team::Blue).expect("GALLERY_FOCUS names a unit in the gallery");
        let (x, y, radius) = (f64::from(unit.x) / 4096.0, f64::from(unit.y) / 4096.0, f64::from(unit.radius) / 4096.0);
        // Aircraft fly well above the ground, so the camera stands above them and aims through them.
        let above = (f64::from(unit.z) / 4096.0 - 20.0).max(0.0);
        rows.push((format!("{key}_close"), [x, y], above + radius * 9.0, above));
        rows.push((format!("{key}_closer"), [x, y], above + radius * 4.5, above));
    }
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
    // A battle fought a minute ago, so that only its marks are left.
    let mut battle = battlefield();

    let event_loop = EventLoop::new().expect("event loop starts");
    let window = Arc::new(WindowBuilder::new().with_title("unit gallery").with_inner_size(LogicalSize::new(1440.0, 900.0)).build(&event_loop).expect("window opens"));
    let size = window.inner_size();
    let mut renderer = Renderer::new(Arc::clone(&window), &heights, None);
    renderer.set_water_level(0.0);
    for (name, focus, altitude, above) in rows {
        let mut camera = Camera::new(f64::from(MAP), [f64::from(size.width), f64::from(size.height)]);
        camera.look_at(focus[0], focus[1], altitude);
        camera.update(0.0, &heights);
        // The camera looks along +Y and down, so to see something `above` the ground at the
        // middle of the view, it aims at the ground that far beyond it along the line of sight.
        if above > 0.0 {
            let beyond = above / camera.pitch_degrees().to_radians().tan();
            camera.look_at(focus[0], focus[1] + beyond, altitude);
            camera.update(0.0, &heights);
        }
        let mut instances = InstanceBatches::default();
        build_unit_instances(&snapshot.units, &snapshot.units, 1.0, camera.eye(), &[], &looks, &[], &mut Motion::new(), &mut instances);
        links::build_wiring_instances(&snapshot.adjacency, &snapshot.units, 0.3, camera.eye(), &mut instances);
        links::build_work_beam_instances(&links::work_beams(&snapshot, &snapshot, 1.0), 0.3, camera.eye(), &mut instances);
        // The battle a moment after it ended, smoke still rising, or a minute after, only marks left.
        // Drawing forgets what has burned out, so the smoke view has a battle of its own.
        if name == "smoke" {
            battlefield().build(1.5, camera.eye(), &mut instances);
        } else {
            battle.build(60.0, camera.eye(), &mut instances);
        }
        let scene = FrameScene { camera: &camera, instances: &instances, selection_box: None, time: 0.0 };
        let path = out.join(format!("{name}.bmp"));
        renderer.capture(&scene, None, &path).expect("capture saves");
        println!("saved {}", path.display());
    }
}
