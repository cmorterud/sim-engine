//! Draws a bundled map from above, as a PPM image, for designing maps: height as shade, steep
//! ground brown, water blue, trees green, rocks grey, deposits cyan, Blue's start blue and Red's red.
//!
//! `cargo run --release --example map_picture -- highland highland.ppm`
//!
//! `skirmish` draws the generated skirmish map, from the seed the game starts with unless a
//! third argument gives another. `generate` draws a map made from settings, given after the
//! output file as its kind, size in km, players, and map seed, with the usual for the rest:
//!
//! `cargo run --release --example map_picture -- generate lake.ppm central_lake 20 8 3`
use sim_engine::math::fp::{Fp, Vec2Fp};
use sim_engine::sim::components::WreckKind;
use sim_engine::sim::config::TERRAIN_CELL_UNITS;
use sim_engine::sim::scenario::MapData;

/// The seed the game starts the skirmish map from, as in main.rs.
const DEFAULT_SEED: u64 = 0x5EED_2026;

/// The generated skirmish map, in the form a bundled map takes: its ground, its walls as blocked
/// cells, its deposits, and its starts.
fn skirmish_map(seed: u64) -> MapData {
    let world = sim_engine::sim::scenario::skirmish(seed).world;
    let map_units = world.config.map_units;
    let cells = (map_units / TERRAIN_CELL_UNITS) as usize;
    let center = |index: usize| Fp::from_num(index as i32 * TERRAIN_CELL_UNITS + TERRAIN_CELL_UNITS / 2);
    let blocked = (0..cells * cells)
        .map(|index| {
            let point = Vec2Fp::new(center(index % cells), center(index / cells));
            !world.terrain.is_passable(point) || world.obstacles.iter().any(|wall| wall.contains(point))
        })
        .collect();
    MapData {
        name: "skirmish".to_string(),
        map_units,
        heights: world.terrain.heights().to_vec(),
        blocked,
        water_level: Fp::from_num(-1000),
        deposits: world.deposits.clone(),
        vents: Vec::new(),
        starts: world.starts,
        layouts: Vec::new(),
        scenery: Vec::new(),
    }
}

fn main() {
    let name = std::env::args().nth(1).expect("map name");
    let out = std::env::args().nth(2).expect("output file");
    let map = if name == "skirmish" {
        skirmish_map(std::env::args().nth(3).and_then(|seed| seed.parse().ok()).unwrap_or(DEFAULT_SEED))
    } else if name == "generate" {
        let number = |index: usize| std::env::args().nth(index).and_then(|text| text.parse().ok());
        let choices = sim_engine::sim::maps::Choices {
            kind: std::env::args().nth(3).expect("a kind of map"),
            size_km: number(4),
            players: number(5),
            seed: std::env::args().nth(6).and_then(|text| text.parse().ok()),
            ..Default::default()
        };
        sim_engine::sim::maps::generate(&sim_engine::sim::maps::MapSettings::chosen(&choices).expect("settings the generator makes"))
    } else {
        sim_engine::sim::maps::bundled(&name).expect("a bundled map")
    };
    let cells = (map.map_units / TERRAIN_CELL_UNITS) as usize;
    let samples = cells + 1;
    let (low, high) = (*map.heights.iter().min().unwrap() as f32, *map.heights.iter().max().unwrap() as f32);
    let water = map.water_level.to_num::<f32>() * 16.0;
    let mut pixels = vec![[0u8; 3]; cells * cells];
    for y in 0..cells {
        for x in 0..cells {
            let h = map.heights[y * samples + x] as f32;
            let t = (h - low) / (high - low).max(1.0);
            let mut c = [(60.0 + 120.0 * t) as u8, (80.0 + 110.0 * t) as u8, (50.0 + 70.0 * t) as u8];
            if map.blocked[y * cells + x] { c = [90, 70, 60]; }
            if h < water { c = [30, 70, 120]; }
            pixels[y * cells + x] = c;
        }
    }
    for &(kind, p) in &map.scenery {
        let (x, y) = ((p.x.to_num::<i32>() / TERRAIN_CELL_UNITS) as usize, (p.y.to_num::<i32>() / TERRAIN_CELL_UNITS) as usize);
        if x < cells && y < cells { pixels[y * cells + x] = if kind == WreckKind::Tree { [20, 110, 30] } else { [160, 160, 150] }; }
    }
    for &(x, y) in &map.deposits {
        for dy in 0..3 { for dx in 0..3 { let (px, py) = (x as usize + dx - 1, y as usize + dy - 1); if px < cells && py < cells { pixels[py * cells + px] = [80, 255, 220]; } } }
    }
    for (index, s) in map.starts.iter().enumerate() {
        let (sx, sy) = (s.x.to_num::<i32>() / TERRAIN_CELL_UNITS, s.y.to_num::<i32>() / TERRAIN_CELL_UNITS);
        for dy in -6..=6 { for dx in -6..=6 { let (px, py) = (sx + dx, sy + dy); if (0..cells as i32).contains(&px) && (0..cells as i32).contains(&py) { pixels[py as usize * cells + px as usize] = if index == 0 { [60, 120, 255] } else { [255, 70, 60] }; } } }
    }
    // Team starts, where the map has them, in yellow.
    for s in map.layouts.iter().flatten() {
        let (sx, sy) = (s.x.to_num::<i32>() / TERRAIN_CELL_UNITS, s.y.to_num::<i32>() / TERRAIN_CELL_UNITS);
        for dy in -4..=4 { for dx in -4..=4 { let (px, py) = (sx + dx, sy + dy); if (0..cells as i32).contains(&px) && (0..cells as i32).contains(&py) { pixels[py as usize * cells + px as usize] = [250, 220, 40]; } } }
    }
    let mut data = format!("P6 {cells} {cells} 255\n").into_bytes();
    for p in pixels { data.extend_from_slice(&p); }
    std::fs::write(out, data).unwrap();
}
