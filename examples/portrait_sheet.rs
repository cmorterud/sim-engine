//! Renders a portrait of every unit and structure, for the interface's buttons, and packs them
//! into `assets/portraits.png`, with their keys, tile by tile, in `assets/portraits.txt`.
//!
//! `cargo run --release --example portrait_sheet [-- --review <folder>] [--team red|blue] [--out <sheet.png>]
//!  [--only <key>,<key>...] [--uniform] [--tile <pixels>]`
//!
//! With `--out`, the sheet is written there, with its index beside it (`<sheet>.txt`), and the
//! game's own portraits are left alone, which is how to compare looks, or see the other team's
//! colors, without changing the game. `--only` draws just those units, in that order. Each
//! portrait is framed to fill its tile, which hides how big units are beside one another; with
//! `--uniform`, every unit is seen from the same distance, the largest's, and framed the same,
//! so that their proportions can be judged. `--tile` sets a tile's side, 96 pixels by default.
//!
//! Run it again after changing a unit's model. Each unit is drawn alone, close up, from the
//! interface's usual angle, then the same view is drawn without it: what differs is the unit,
//! and everything else is left see-through. Its shadow is kept, faintly, to ground it. Ships are
//! drawn with the water left out, floating over the seabed, so that none of the hull is hidden.
use std::path::Path;
use std::sync::Arc;

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::render::camera::{Camera, HeightField};
use sim_engine::render::instances::{build_unit_instances, unit_looks, InstanceBatches, Motion};
use sim_engine::render::pipeline::{FrameScene, Renderer};
use sim_engine::sim::blueprints::UnitClass;
use sim_engine::sim::components::{MoveClass, Team};
use sim_engine::sim::config::{SimConfig, TerrainStyle, TERRAIN_CELL_UNITS};
use sim_engine::sim::terrain::{Terrain, HEIGHT_FRAC_BITS};
use sim_engine::sim::world::SimWorld;
use winit::dpi::LogicalSize;
use winit::event_loop::EventLoop;
use winit::window::WindowBuilder;

const MAP: i32 = 6_400;
/// The side of a portrait, in pixels.
const TILE: usize = 96;
/// Portraits a row of the sheet.
const COLUMNS: usize = 12;
/// How steeply a ship's portrait looks down on it, in degrees.
const SHIP_TILT_DEGREES: f64 = 50.0;
/// Ships and naval structures need water: a strip across the map, between these heights.
const WATER: std::ops::RangeInclusive<i32> = 1_600..=2_400;

fn ground_height(_x: f64, y: f64) -> f64 {
    if WATER.contains(&(y as i32)) {
        -40.0
    } else {
        20.0
    }
}

fn world() -> SimWorld {
    let cells = (MAP / TERRAIN_CELL_UNITS) as usize;
    let samples = cells + 1;
    let cell = f64::from(TERRAIN_CELL_UNITS);
    let sample_height = |index: usize| ground_height((index % samples) as f64 * cell, (index / samples) as f64 * cell);
    let heights: Vec<i16> = (0..samples * samples).map(|index| (sample_height(index) * f64::from(1 << HEIGHT_FRAC_BITS)).round() as i16).collect();
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

/// How much of a pixel is the unit, and its color, from the view with it and the view without:
/// the same color is ground, the same color darkened evenly is its shadow, and anything else is
/// the unit itself.
fn matte(with: &[u8], without: &[u8]) -> [u8; 4] {
    let difference = (0..3).map(|channel| (i32::from(with[channel]) - i32::from(without[channel])).abs()).max().unwrap_or(0);
    if difference < 6 {
        return [0; 4];
    }
    let ratios: Vec<f32> = (0..3).map(|channel| (f32::from(with[channel]) + 4.0) / (f32::from(without[channel]) + 4.0)).collect();
    let mean = ratios.iter().sum::<f32>() / 3.0;
    let even = ratios.iter().all(|ratio| (ratio - mean).abs() < 0.06);
    if even && mean < 0.97 {
        // A shadow: a faint dark patch.
        return [0, 0, 0, ((1.0 - mean) * 160.0).clamp(0.0, 110.0) as u8];
    }
    // Fully solid soon past the threshold, so that a hull close to the ground's color does not
    // come out see-through, mottled by the pattern of the grass behind it.
    let alpha = (difference as f32 / 12.0).clamp(0.0, 1.0);
    [with[0], with[1], with[2], (alpha * 255.0) as u8]
}

/// Cuts the unit out of a frame and shrinks it into a tile, centered, with a little room around.
fn portrait(with: &[u8], without: &[u8], width: usize, height: usize, tile_side: usize) -> Vec<u8> {
    let (cut, extent) = cut_out(with, without, width, height);
    extent.map_or_else(|| vec![0; tile_side * tile_side * 4], |extent| tile_from(&cut, width, extent, framed_side(extent), tile_side))
}

/// The unit cut out of a frame drawn with it and without it, and its extent in the frame,
/// `[left, top, right, bottom]`, if any of it shows.
fn cut_out(with: &[u8], without: &[u8], width: usize, height: usize) -> (Vec<[u8; 4]>, Option<[usize; 4]>) {
    let mut cut: Vec<[u8; 4]> = with.as_chunks::<4>().0.iter().zip(without.as_chunks::<4>().0).map(|(a, b)| matte(a, b)).collect();
    // Water ripples a little differently around a hull, which leaves specks: drop whatever has
    // too few neighbors to be part of the unit.
    for _ in 0..2 {
        let before = cut.clone();
        for y in 1..height - 1 {
            for x in 1..width - 1 {
                if before[y * width + x][3] == 0 {
                    continue;
                }
                let neighbors = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)]
                    .iter()
                    .filter(|(dx, dy)| before[(y as isize + dy) as usize * width + (x as isize + dx) as usize][3] > 0)
                    .count();
                if neighbors < 5 {
                    cut[y * width + x] = [0; 4];
                }
            }
        }
    }
    // The unit's extent: solid pixels only, so a faint shadow does not shift the framing.
    let (mut left, mut top, mut right, mut bottom) = (width, height, 0, 0);
    for y in 0..height {
        for x in 0..width {
            let pixel = cut[y * width + x];
            if pixel[3] > 128 && (pixel[0] > 0 || pixel[1] > 0 || pixel[2] > 0) {
                left = left.min(x);
                right = right.max(x);
                top = top.min(y);
                bottom = bottom.max(y);
            }
        }
    }
    let extent = (right >= left).then_some([left, top, right, bottom]);
    (cut, extent)
}

/// The side of the square a unit of `extent` is framed in, with a margin, in frame pixels.
fn framed_side(extent: [usize; 4]) -> f32 {
    let [left, top, right, bottom] = extent;
    ((right - left).max(bottom - top) as f32 * 1.12).max(8.0)
}

/// A tile `tile_side` pixels square of a square `side` frame pixels across, centered on the
/// middle of `extent` in a cut-out frame `width` pixels wide.
fn tile_from(cut: &[[u8; 4]], width: usize, extent: [usize; 4], side: f32, tile_side: usize) -> Vec<u8> {
    let height = cut.len() / width;
    let [left, top, right, bottom] = extent;
    let mut tile = vec![0u8; tile_side * tile_side * 4];
    let (middle_x, middle_y) = ((left + right) as f32 / 2.0, (top + bottom) as f32 / 2.0);
    let step = side / tile_side as f32;
    // Each tile pixel averages the frame pixels under it, weighting color by coverage.
    for ty in 0..tile_side {
        for tx in 0..tile_side {
            let (x0, y0) = (middle_x - side / 2.0 + tx as f32 * step, middle_y - side / 2.0 + ty as f32 * step);
            let mut sum = [0f32; 4];
            let mut count = 0f32;
            let samples = (step.ceil() as usize).max(1);
            for sy in 0..samples {
                for sx in 0..samples {
                    let (x, y) = ((x0 + (sx as f32 + 0.5) * step / samples as f32) as isize, (y0 + (sy as f32 + 0.5) * step / samples as f32) as isize);
                    count += 1.0;
                    if x < 0 || y < 0 || x as usize >= width || y as usize >= height {
                        continue;
                    }
                    let pixel = cut[y as usize * width + x as usize];
                    let alpha = f32::from(pixel[3]) / 255.0;
                    for channel in 0..3 {
                        sum[channel] += f32::from(pixel[channel]) * alpha;
                    }
                    sum[3] += alpha;
                }
            }
            let at = (ty * tile_side + tx) * 4;
            if sum[3] > 0.0 {
                for channel in 0..3 {
                    tile[at + channel] = (sum[channel] / sum[3]).round() as u8;
                }
                tile[at + 3] = (sum[3] / count * 255.0).round() as u8;
            }
        }
    }
    tile
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let option = |name: &str| arguments.iter().position(|argument| argument == name).and_then(|at| arguments.get(at + 1)).cloned();
    let team = if option("--team").as_deref() == Some("red") { Team::Red } else { Team::Blue };
    let uniform = arguments.iter().any(|argument| argument == "--uniform");
    let tile_side: usize = option("--tile").and_then(|value| value.parse().ok()).unwrap_or(TILE);
    let mut world = world();
    let blueprints = Arc::clone(&world.blueprints);
    let drawn: Vec<_> = match option("--only") {
        Some(keys) => keys.split(',').map(|key| blueprints.get(blueprints.find(key).unwrap_or_else(|| panic!("no unit {key}")))).collect(),
        None => blueprints.iter().filter(|unit| unit.class != UnitClass::Ammo).collect(),
    };
    // Everyone apart, so that each can be drawn alone: land and air units in rows on land,
    // ships on the water, structures beyond.
    let (mut land_x, mut land_y, mut sea_x) = (300, 400, 300);
    let (mut cell_x, mut cell_y) = (12_u32, 140_u32);
    let mut sea_cell_x = 12_u32;
    for unit in &drawn {
        if unit.is_structure() {
            let size = u32::from(unit.footprint);
            if unit.on_water {
                world.spawn_finished_structure(unit.id, team, sea_cell_x, 100);
                sea_cell_x += size + 10;
            } else {
                if cell_x + size > 300 {
                    cell_x = 12;
                    cell_y += 18;
                }
                // Extractors and plants stand on a deposit or vent, so lay one under them.
                if unit.requires_deposit {
                    world.deposits.push((cell_x, cell_y));
                }
                if unit.requires_vent {
                    world.vents.push((cell_x + size / 2, cell_y + size / 2));
                }
                if world.spawn_finished_structure(unit.id, team, cell_x, cell_y).is_none() {
                    eprintln!("{} could not be built", unit.key);
                }
                cell_x += size + 10;
            }
        } else if matches!(unit.movement, MoveClass::Naval | MoveClass::Submarine) {
            world.spawn_mobile(unit.id, team, Vec2Fp::new(fp_int(sea_x), fp_int(1_900)));
            sea_x += 260;
        } else {
            world.spawn_mobile(unit.id, team, Vec2Fp::new(fp_int(land_x), fp_int(land_y)));
            land_x += 260;
            if land_x > MAP - 300 {
                land_x = 300;
                land_y += 300;
            }
        }
    }
    let mut snapshot = world.snapshot();
    let raw = |value: f64| (value * 4096.0) as i32;
    let ships: Vec<u16> = drawn.iter().filter(|unit| !unit.is_structure() && matches!(unit.movement, MoveClass::Naval | MoveClass::Submarine)).map(|unit| unit.id.0).collect();
    for unit in &mut snapshot.units {
        // Turned three quarters towards the camera, which looks along +Y. Ships show more of
        // their side, bow to the right, since what tells one from another is its profile:
        // its turrets, funnels, domes, and towers.
        unit.facing = if ships.contains(&unit.blueprint) { [raw(0.85), raw(-0.53)] } else { [raw(0.55), raw(-0.83)] };
        unit.aim = unit.facing;
    }
    let cell = TERRAIN_CELL_UNITS;
    let heights = HeightField::new(world.terrain.samples_per_side(), world.terrain.heights().to_vec(), f64::from(cell), 1.0 / f64::from(1 << HEIGHT_FRAC_BITS));
    let looks = unit_looks(&blueprints);

    let event_loop = EventLoop::new().expect("event loop starts");
    let window = Arc::new(WindowBuilder::new().with_title("portraits").with_inner_size(LogicalSize::new(900.0, 900.0)).build(&event_loop).expect("window opens"));
    let size = window.inner_size();
    let mut renderer = Renderer::new(Arc::clone(&window), &heights, None);
    renderer.set_water_level(0.0);

    // How far the camera stands from each unit: from a big one further than from a small one, so
    // that each fills its portrait, or with `--uniform`, from all as far as from the largest.
    let stand_off = |radius: f64| radius * 6.0 + 30.0;
    let farthest = snapshot.units.iter().map(|view| stand_off(f64::from(view.radius) / 4096.0)).fold(0.0, f64::max);
    let mut keys: Vec<String> = Vec::new();
    let mut tiles: Vec<Vec<u8>> = Vec::new();
    let mut kinds: Vec<&'static str> = Vec::new();
    let mut cut_outs: Vec<(Vec<[u8; 4]>, Option<[usize; 4]>, usize)> = Vec::new();
    for unit in drawn {
        let Some(view) = snapshot.units.iter().find(|view| view.blueprint == unit.id.0).cloned() else {
            eprintln!("{} could not be placed", unit.key);
            continue;
        };
        let (x, y, z, radius) = (f64::from(view.x) / 4096.0, f64::from(view.y) / 4096.0, f64::from(view.z) / 4096.0, f64::from(view.radius) / 4096.0);
        let above = (z - ground_height(x, y)).max(0.0);
        let mut camera = Camera::new(f64::from(MAP), [f64::from(size.width), f64::from(size.height)]);
        let altitude = above + if uniform { farthest } else { stand_off(radius) };
        camera.look_at(x, y, altitude);
        camera.update(0.0, &heights);
        // A ship is seen from lower down, more from the side, so that its profile shows.
        if ships.contains(&unit.id.0) {
            camera.orbit(0.0, SHIP_TILT_DEGREES - camera.pitch_degrees());
            camera.update(0.0, &heights);
        }
        if above > 0.0 {
            let beyond = above / camera.pitch_degrees().to_radians().tan();
            camera.look_at(x, y + beyond, altitude);
            camera.update(0.0, &heights);
        }
        // The water would be drawn over a hull's lower half, so ships float over the dry seabed.
        let on_water = unit.on_water || matches!(unit.movement, MoveClass::Naval | MoveClass::Submarine);
        renderer.set_water_level(if on_water { -1_000.0 } else { 0.0 });
        let mut frame = |units: &[sim_engine::sim::world::UnitView]| {
            let mut instances = InstanceBatches::default();
            build_unit_instances(units, units, 1.0, camera.eye(), &[], &looks, &[], &mut Motion::new(), &mut instances);
            renderer.capture_rgba(&FrameScene { camera: &camera, instances: &instances, selection_box: None, time: 0.0 }, None)
        };
        let (width, height, with) = frame(std::slice::from_ref(&view));
        let (_, _, without) = frame(&[]);
        if uniform {
            let (cut, extent) = cut_out(&with, &without, width as usize, height as usize);
            cut_outs.push((cut, extent, width as usize));
        } else {
            tiles.push(portrait(&with, &without, width as usize, height as usize, tile_side));
        }
        keys.push(unit.key.clone());
        kinds.push(if unit.is_structure() {
            "structures"
        } else if unit.is_air() {
            "air"
        } else if on_water {
            "naval"
        } else {
            "land"
        });
        println!("{}", unit.key);
    }

    // Seen from one distance, every unit is framed alike too, by the largest of them.
    if uniform {
        let side = cut_outs.iter().filter_map(|(_, extent, _)| extent.map(framed_side)).fold(8.0, f32::max);
        tiles = cut_outs.iter().map(|(cut, extent, width)| extent.map_or_else(|| vec![0; tile_side * tile_side * 4], |extent| tile_from(cut, *width, extent, side, tile_side))).collect();
    }
    let rows = tiles.len().div_ceil(COLUMNS);
    let (sheet_width, sheet_height) = (COLUMNS * tile_side, rows * tile_side);
    let mut sheet = vec![0u8; sheet_width * sheet_height * 4];
    for (index, tile) in tiles.iter().enumerate() {
        let (column, row) = (index % COLUMNS, index / COLUMNS);
        for y in 0..tile_side {
            let from = &tile[y * tile_side * 4..(y + 1) * tile_side * 4];
            let at = ((row * tile_side + y) * sheet_width + column * tile_side) * 4;
            sheet[at..at + tile_side * 4].copy_from_slice(from);
        }
    }
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    let sheet_path = option("--out").map_or_else(|| assets.join("portraits.png"), std::path::PathBuf::from);
    let file = std::fs::File::create(&sheet_path).expect("sheet can be written");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), sheet_width as u32, sheet_height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Best);
    encoder.write_header().expect("header writes").write_image_data(&sheet).expect("sheet writes");
    let index = format!("{tile_side} {COLUMNS}\n{}\n", keys.join("\n"));
    std::fs::write(if option("--out").is_some() { sheet_path.with_extension("txt") } else { assets.join("portraits.txt") }, index).expect("index writes");
    println!("{} portraits", keys.len());
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--review") {
        let folder = std::path::PathBuf::from(args.next().expect("a folder for the review sheets"));
        std::fs::create_dir_all(&folder).expect("the review folder can be made");
        for kind in ["land", "air", "naval", "structures"] {
            let chosen: Vec<&Vec<u8>> = tiles.iter().zip(&kinds).filter(|(_, tile_kind)| **tile_kind == kind).map(|(tile, _)| tile).collect();
            let path = folder.join(format!("{kind}.png"));
            write_review(&path, &chosen, tile_side);
            println!("saved {}", path.display());
        }
    }
}

/// Portraits of one kind of unit, twice their size on a dark ground in rows of eight, for
/// comparing models before and after a change as the buttons would show them.
fn write_review(path: &Path, tiles: &[&Vec<u8>], tile_side: usize) {
    const PER_ROW: usize = 8;
    const SCALE: usize = 2;
    const GROUND: f32 = 34.0;
    let side = tile_side * SCALE;
    let rows = tiles.len().div_ceil(PER_ROW).max(1);
    let (width, height) = (PER_ROW * side, rows * side);
    let mut picture = vec![0u8; width * height * 4];
    for pixel in picture.as_chunks_mut::<4>().0 {
        *pixel = [GROUND as u8, GROUND as u8, GROUND as u8, 255];
    }
    for (index, tile) in tiles.iter().enumerate() {
        let (left, top) = ((index % PER_ROW) * side, (index / PER_ROW) * side);
        for y in 0..side {
            for x in 0..side {
                let from = ((y / SCALE) * tile_side + x / SCALE) * 4;
                let alpha = f32::from(tile[from + 3]) / 255.0;
                let at = ((top + y) * width + left + x) * 4;
                for channel in 0..3 {
                    picture[at + channel] = (f32::from(tile[from + channel]) * alpha + GROUND * (1.0 - alpha)).round() as u8;
                }
            }
        }
    }
    let file = std::fs::File::create(path).expect("the review sheet can be written");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().expect("header writes").write_image_data(&picture).expect("sheet writes");
}
