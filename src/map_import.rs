//! Loads a map made for *Forged Alliance* from a player's own files, for a 1v1 on two of its
//! starts (GAME_SPECIFICATION.md §20: maps from other games are never shipped, only loaded).
//!
//! A map is a folder holding a binary `.scmap`, with the heightmap, water, and props, and a
//! `_save.lua`, with the markers: mass deposits and the armies' starts. The reader is written for
//! this project from the files' layout. It turns everything into whole numbers here, outside the
//! simulation, so that every instance builds the same world from the same files.

use std::path::{Path, PathBuf};

use crate::math::fp::{Fp, Vec2Fp, FP_FRAC_BITS};
use crate::sim::components::WreckKind;
use crate::sim::config::{MAX_MAP_UNITS, SECTOR_UNITS, TERRAIN_CELL_UNITS};
use crate::sim::scenario::MapData;
use crate::sim::terrain::HEIGHT_FRAC_BITS;

/// One of the original's units is one of our terrain cells: its 10 km map, 512 across, is our
/// 10 km skirmish map, 10,240 across.
pub const UNITS_PER_ORIGINAL_UNIT: i32 = TERRAIN_CELL_UNITS;
/// Land steeper than this, in height per unit of ground, stops land units. About the original's
/// limit: a cell is blocked if its corners differ by more than this.
const MAX_SLOPE: f64 = 0.75;

/// What was loaded, with notes on what was left out, for the player.
pub struct Imported {
    pub map: MapData,
    pub notes: Vec<String>,
}

/// The heightmap, water, and props from a `.scmap`.
struct Scmap {
    size: usize,
    height_scale: f64,
    heights: Vec<u16>,
    water: Option<f64>,
    props: Vec<(String, [f32; 3])>,
}

/// A marker from the `_save.lua`: its name, type, and position.
#[derive(Debug, PartialEq)]
struct Marker {
    name: String,
    kind: String,
    position: [f64; 3],
}

/// Loads the map in a folder, or the folder of a `.scmap`, with `starts` naming the two armies
/// that play, from 1: Blue's, then Red's.
pub fn import(path: &Path, starts: (usize, usize)) -> Result<Imported, String> {
    if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("zip")) {
        return Err(format!("{} is a zip: unzip it, and give the folder inside", path.display()));
    }
    let folder = if path.is_dir() { path.to_path_buf() } else { path.parent().map_or_else(PathBuf::new, Path::to_path_buf) };
    let find = |suffix: &str| -> Result<PathBuf, String> {
        let entries = std::fs::read_dir(&folder).map_err(|error| format!("cannot read {}: {error}", folder.display()))?;
        let mut found: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|file| file.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.to_ascii_lowercase().ends_with(suffix)))
            .collect();
        found.sort();
        found.into_iter().next().ok_or_else(|| format!("no *{suffix} in {}", folder.display()))
    };
    let scmap_path = find(".scmap")?;
    let save_path = find("_save.lua")?;
    let scmap = parse_scmap(&std::fs::read(&scmap_path).map_err(|error| format!("cannot read {}: {error}", scmap_path.display()))?)?;
    let save = std::fs::read_to_string(&save_path).map_err(|error| format!("cannot read {}: {error}", save_path.display()))?;
    let name = scmap_path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("map").to_string();
    convert(&name, &scmap, &parse_markers(&save), starts)
}

/// Reads a little-endian file front to back.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8], String> {
        let end = self.at.checked_add(count).filter(|&end| end <= self.bytes.len()).ok_or("the .scmap ends early")?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().expect("four bytes")))
    }

    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().expect("four bytes")))
    }

    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn string(&mut self) -> Result<String, String> {
        let rest = &self.bytes[self.at..];
        let end = rest.iter().position(|&byte| byte == 0).ok_or("a name in the .scmap has no end")?;
        let text = String::from_utf8_lossy(&rest[..end]).into_owned();
        self.at += end + 1;
        Ok(text)
    }
}

fn parse_scmap(bytes: &[u8]) -> Result<Scmap, String> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.take(4)? != b"Map\x1a" {
        return Err("not a .scmap: it does not start with the map signature".into());
    }
    let (_version, _signature, _two) = (reader.i32()?, reader.i32()?, reader.i32()?);
    let (_width, _height) = (reader.f32()?, reader.f32()?);
    reader.take(4 + 2)?;
    let preview = usize::try_from(reader.i32()?).map_err(|_| "a bad preview size")?;
    reader.take(preview)?;
    let minor = reader.i32()?;
    if minor < 56 {
        return Err(format!("map version {minor} is older than Forged Alliance's, and not supported"));
    }
    let (width, height) = (reader.i32()?, reader.i32()?);
    if width != height || width <= 0 {
        return Err(format!("only square maps are supported, not {width} by {height}"));
    }
    let size = width as usize;
    let height_scale = f64::from(reader.f32()?);
    let samples = (size + 1) * (size + 1);
    let heights = reader.take(samples * 2)?.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect();

    // Then the look of the map, which is not used, and its water, which is.
    reader.byte()?;
    for _ in 0..3 {
        reader.string()?;
    }
    let cubemaps = reader.i32()?;
    if !(0..=64).contains(&cubemaps) {
        return Err(format!("map version {minor} has a layout this reader does not know"));
    }
    for _ in 0..cubemaps * 2 {
        reader.string()?;
    }
    // Lighting: a multiplier, sun direction, ambience, color, shadow, specular, bloom, and fog.
    reader.take(23 * 4)?;
    let water = match reader.byte()? {
        0 => None,
        1 => Some(f64::from(reader.f32()?)),
        _ => return Err(format!("map version {minor} has a layout this reader does not know")),
    };

    // Props are near the end: a blueprint path, then position, three rows of rotation, and scale.
    let mut props = Vec::new();
    let mut at = reader.at;
    while let Some(found) = find(&bytes[at..], b"/env/") {
        let start = at + found;
        let Some(length) = bytes[start..].iter().position(|&byte| byte == 0) else {
            break;
        };
        let path = String::from_utf8_lossy(&bytes[start..start + length]).into_owned();
        at = start + length + 1;
        if path.to_ascii_lowercase().ends_with("_prop.bp") && at + 60 <= bytes.len() {
            let float = |index: usize| f32::from_le_bytes(bytes[at + index * 4..at + index * 4 + 4].try_into().expect("four bytes"));
            props.push((path, [float(0), float(1), float(2)]));
            at += 60;
        }
    }
    Ok(Scmap { size, height_scale, heights, water, props })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// Every marker with a type and a position, from the Lua the editor writes.
fn parse_markers(save: &str) -> Vec<Marker> {
    let mut markers = Vec::new();
    let (mut name, mut kind, mut position): (Option<String>, Option<String>, Option<[f64; 3]>) = (None, None, None);
    for line in save.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("['") {
            if let Some((marker, tail)) = rest.split_once("']") {
                if tail.trim() == "= {" {
                    (name, kind, position) = (Some(marker.to_string()), None, None);
                    continue;
                }
                let value = tail.trim().trim_start_matches('=').trim();
                if marker == "type" {
                    kind = value.strip_prefix("STRING( '").and_then(|text| text.split('\'').next()).map(str::to_string);
                } else if marker == "position" {
                    let numbers: Vec<f64> = value
                        .trim_start_matches("VECTOR3(")
                        .trim_end_matches(',')
                        .trim_end_matches(')')
                        .split(',')
                        .filter_map(|number| number.trim().parse().ok())
                        .collect();
                    position = <[f64; 3]>::try_from(numbers).ok();
                }
            }
        } else if line.starts_with('}') {
            if let (Some(name), Some(kind), Some(position)) = (name.take(), kind.take(), position.take()) {
                markers.push(Marker { name, kind, position });
            }
        }
    }
    markers
}

fn convert(name: &str, scmap: &Scmap, markers: &[Marker], starts: (usize, usize)) -> Result<Imported, String> {
    let mut notes = Vec::new();
    let size = scmap.size;
    let map_units = i32::try_from(size).ok().and_then(|size| size.checked_mul(UNITS_PER_ORIGINAL_UNIT)).ok_or("the map is too large")?;
    if map_units % SECTOR_UNITS != 0 || map_units > MAX_MAP_UNITS {
        return Err(format!("a map {size} across is not a size this game supports: it must be a multiple of {}, up to {}", SECTOR_UNITS / UNITS_PER_ORIGINAL_UNIT, MAX_MAP_UNITS / UNITS_PER_ORIGINAL_UNIT));
    }
    let samples = size + 1;
    let corner = |x: usize, y: usize| scmap.heights[y * samples + x];

    // Heights, from the lowest point up, in sixteenths of our units.
    let lowest = scmap.heights.iter().copied().min().unwrap_or(0);
    let to_raw = |height: u16| (f64::from(height - lowest) * scmap.height_scale * f64::from(UNITS_PER_ORIGINAL_UNIT) * f64::from(1 << HEIGHT_FRAC_BITS)).round();
    let highest = to_raw(scmap.heights.iter().copied().max().unwrap_or(0));
    if highest > f64::from(i16::MAX) {
        return Err(format!(
            "its hills are too tall: they rise {:.0} of the original's units, and this game holds up to {:.0}",
            highest / f64::from(UNITS_PER_ORIGINAL_UNIT << HEIGHT_FRAC_BITS),
            f64::from(i16::MAX) / f64::from(UNITS_PER_ORIGINAL_UNIT << HEIGHT_FRAC_BITS)
        ));
    }
    let heights: Vec<i16> = scmap.heights.iter().map(|&height| to_raw(height) as i16).collect();

    // A cell is blocked if it is too steep, or wholly under water.
    let steep = (MAX_SLOPE / scmap.height_scale) as u16;
    let water_height = scmap.water.map(|level| ((level / scmap.height_scale).round() as i64).clamp(0, i64::from(u16::MAX)) as u16);
    let mut blocked = vec![false; size * size];
    let (mut too_steep, mut under_water) = (0, 0);
    for y in 0..size {
        for x in 0..size {
            let corners = [corner(x, y), corner(x + 1, y), corner(x, y + 1), corner(x + 1, y + 1)];
            let (low, high) = (corners.iter().min().copied().unwrap_or(0), corners.iter().max().copied().unwrap_or(0));
            if water_height.is_some_and(|water| high < water) {
                blocked[y * size + x] = true;
                under_water += 1;
            } else if high - low > steep {
                blocked[y * size + x] = true;
                too_steep += 1;
            }
        }
    }
    notes.push(format!("{too_steep} of {} cells too steep to cross ({:.1}%)", size * size, 100.0 * f64::from(too_steep) / (size * size) as f64));
    let water_level = match water_height {
        Some(water) if water > lowest => {
            notes.push(format!("{under_water} cells under water, impassable until naval units (§10)"));
            Fp::from_num(f64::from(water - lowest) * scmap.height_scale * f64::from(UNITS_PER_ORIGINAL_UNIT))
        }
        _ => Fp::from_bits(-(1000 << FP_FRAC_BITS)),
    };

    let to_point = |x: f64, z: f64| {
        let limit = f64::from(map_units) - 0.001;
        let scale = f64::from(UNITS_PER_ORIGINAL_UNIT);
        Vec2Fp::new(Fp::from_num((x * scale).clamp(0.0, limit)), Fp::from_num((z * scale).clamp(0.0, limit)))
    };
    let cell_of = |x: f64, z: f64| ((x.floor().max(0.0) as usize).min(size - 1), (z.floor().max(0.0) as usize).min(size - 1));

    let mut deposits: Vec<(u32, u32)> = Vec::new();
    let mut deposits_blocked = 0;
    for marker in markers.iter().filter(|marker| marker.kind == "Mass") {
        let (x, y) = cell_of(marker.position[0], marker.position[2]);
        if !deposits.contains(&(x as u32, y as u32)) {
            deposits.push((x as u32, y as u32));
            deposits_blocked += usize::from(blocked[y * size + x]);
        }
    }
    notes.push(format!("{} mass deposits", deposits.len()));
    if deposits_blocked > 0 {
        notes.push(format!("{deposits_blocked} of them on ground too steep to build on"));
    }
    let mut vents: Vec<(u32, u32)> = Vec::new();
    let mut vents_blocked = 0;
    for marker in markers.iter().filter(|marker| marker.kind == "Hydrocarbon") {
        let (x, y) = cell_of(marker.position[0], marker.position[2]);
        if !vents.contains(&(x as u32, y as u32)) {
            vents.push((x as u32, y as u32));
            // A plant covers the vent's cell and the ring around it.
            let ground = (y.saturating_sub(1)..=(y + 1).min(size - 1)).any(|row| (x.saturating_sub(1)..=(x + 1).min(size - 1)).any(|column| blocked[row * size + column]));
            vents_blocked += usize::from(ground);
        }
    }
    if !vents.is_empty() {
        notes.push(format!("{} hydrocarbon vents", vents.len()));
    }
    if vents_blocked > 0 {
        notes.push(format!("{vents_blocked} of them on ground too steep to build on"));
    }

    let army = |number: usize| {
        let wanted = format!("ARMY_{number}");
        markers.iter().find(|marker| marker.name == wanted).map(|marker| to_point(marker.position[0], marker.position[2])).ok_or_else(|| format!("the map has no start for army {number}"))
    };
    if starts.0 == starts.1 {
        return Err("the two starts must differ".into());
    }
    let armies = markers.iter().filter(|marker| marker.name.starts_with("ARMY_")).count();
    let chosen = [army(starts.0)?, army(starts.1)?];
    notes.push(format!("starts {} (Blue) and {} (Red), of the map's {armies}", starts.0, starts.1));

    let mut scenery = Vec::new();
    let (mut skipped, mut drowned) = (0, 0);
    for (path, position) in &scmap.props {
        // Land units can never reach what lies under water, so it is left out until naval units.
        let (x, y) = cell_of(f64::from(position[0]), f64::from(position[2]));
        let corners = [corner(x, y), corner(x + 1, y), corner(x, y + 1), corner(x + 1, y + 1)];
        if water_height.is_some_and(|water| corners.iter().all(|&height| height < water)) {
            drowned += 1;
            continue;
        }
        let path = path.to_ascii_lowercase();
        let kind = if path.contains("/trees/") {
            WreckKind::Tree
        } else if path.contains("/rocks/") {
            WreckKind::Rock
        } else {
            skipped += 1;
            continue;
        };
        scenery.push((kind, to_point(f64::from(position[0]), f64::from(position[2]))));
    }
    let trees = scenery.iter().filter(|(kind, _)| *kind == WreckKind::Tree).count();
    notes.push(format!("{trees} trees and {} rocks, with this game's reclaim values", scenery.len() - trees));
    if skipped > 0 {
        notes.push(format!("{skipped} other props left out"));
    }
    if drowned > 0 {
        notes.push(format!("{drowned} props under water left out"));
    }

    let map = MapData { name: name.to_string(), map_units, heights, blocked, water_level, deposits, vents, starts: chosen, scenery };
    Ok(Imported { map, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small `.scmap`: a flat map with a ridge, water optional, and two props.
    fn scmap_bytes(size: usize, water: Option<f32>) -> Vec<u8> {
        let mut bytes = b"Map\x1a".to_vec();
        for value in [2i32, 0xBEEF_FEEDu32 as i32, 2] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend((size as f32).to_le_bytes());
        bytes.extend((size as f32).to_le_bytes());
        bytes.extend([0u8; 6]);
        bytes.extend(4i32.to_le_bytes());
        bytes.extend(b"DDS ");
        bytes.extend(56i32.to_le_bytes());
        bytes.extend((size as i32).to_le_bytes());
        bytes.extend((size as i32).to_le_bytes());
        bytes.extend((1.0f32 / 128.0).to_le_bytes());
        for y in 0..=size {
            for x in 0..=size {
                // 10 units high everywhere, with a wall 20 high down the middle column.
                let height: u16 = if x == size / 2 && y > 2 { 30 * 128 } else { 10 * 128 };
                bytes.extend(height.to_le_bytes());
            }
        }
        bytes.push(0);
        for name in ["TTerrain", "/textures/background.dds", "/textures/sky.dds"] {
            bytes.extend(name.as_bytes());
            bytes.push(0);
        }
        bytes.extend(1i32.to_le_bytes());
        for name in ["<default>", "/textures/env.dds"] {
            bytes.extend(name.as_bytes());
            bytes.push(0);
        }
        bytes.extend([0u8; 23 * 4]);
        match water {
            None => bytes.push(0),
            Some(level) => {
                bytes.push(1);
                bytes.extend(level.to_le_bytes());
            }
        }
        bytes.extend([7u8; 40]);
        for (path, x, z) in [("/env/evergreen/props/trees/pine01_prop.bp", 3.5f32, 4.5f32), ("/env/desert/props/rocks/des_boulder01_prop.bp", 10.25, 2.0)] {
            bytes.extend(path.as_bytes());
            bytes.push(0);
            for value in [x, 12.0, z, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0] {
                bytes.extend(value.to_le_bytes());
            }
        }
        bytes
    }

    const SAVE: &str = "
    Markers = {
        ['Mass 00'] = {
          ['size'] = FLOAT( 1.00 ),
          ['type'] = STRING( 'Mass' ),
          ['position'] = VECTOR3( 6.50, 10.0, 7.50 ),
        },
        ['Hydrocarbon 00'] = {
          ['type'] = STRING( 'Hydrocarbon' ),
          ['position'] = VECTOR3( 20.50, 10.0, 20.50 ),
        },
        ['ARMY_1'] = {
          ['type'] = STRING( 'Blank Marker' ),
          ['position'] = VECTOR3( 8.50, 10.0, 56.50 ),
        },
        ['ARMY_2'] = {
          ['type'] = STRING( 'Blank Marker' ),
          ['position'] = VECTOR3( 55.50, 10.0, 7.50 ),
        },
    },
    ['ARMY_1'] = {
      personality = '',
      Economy = {mass = 0, energy = 0},
    },
    ";

    #[test]
    fn markers_come_out_with_their_types_and_positions() {
        let markers = parse_markers(SAVE);
        assert_eq!(markers.len(), 4, "{markers:?}");
        assert_eq!(markers[0], Marker { name: "Mass 00".into(), kind: "Mass".into(), position: [6.5, 10.0, 7.5] });
        assert_eq!(markers[2].name, "ARMY_1");
    }

    #[test]
    fn a_scmap_gives_its_heights_water_and_props() {
        let scmap = parse_scmap(&scmap_bytes(64, None)).unwrap();
        assert_eq!((scmap.size, scmap.heights.len(), scmap.water), (64, 65 * 65, None));
        assert_eq!(scmap.props.len(), 2);
        assert_eq!(scmap.props[1].1, [10.25, 12.0, 2.0]);
        assert_eq!(parse_scmap(&scmap_bytes(64, Some(15.0))).unwrap().water, Some(15.0));
        assert!(parse_scmap(b"nope").is_err());
    }

    #[test]
    fn a_map_becomes_terrain_deposits_starts_and_scenery_in_our_units() {
        let scmap = parse_scmap(&scmap_bytes(64, None)).unwrap();
        let imported = convert("test", &scmap, &parse_markers(SAVE), (1, 2)).unwrap();
        let map = &imported.map;
        assert_eq!(map.map_units, 64 * 20);
        assert_eq!(map.heights.len(), 65 * 65);
        // The lowest ground is 0, and the wall 20 of the original's units, 400 of ours, above it.
        assert_eq!(map.heights.iter().copied().min(), Some(0));
        assert_eq!(map.heights.iter().copied().max(), Some(400 << HEIGHT_FRAC_BITS));
        // The cells either side of the wall are too steep; flat ground is not.
        assert!(map.blocked[10 * 64 + 31] && map.blocked[10 * 64 + 32]);
        assert!(!map.blocked[10 * 64 + 10]);
        assert_eq!(map.deposits, [(6, 7)]);
        assert_eq!(map.starts, [Vec2Fp::new(Fp::from_num(170), Fp::from_num(1_130)), Vec2Fp::new(Fp::from_num(1_110), Fp::from_num(150))]);
        assert_eq!(map.scenery, [(WreckKind::Tree, Vec2Fp::new(Fp::from_num(70), Fp::from_num(90))), (WreckKind::Rock, Vec2Fp::new(Fp::from_num(205), Fp::from_num(40)))]);
        assert_eq!(map.vents, [(20, 20)]);
        assert!(imported.notes.iter().any(|note| note.contains("1 hydrocarbon vents")));
        assert!(convert("test", &scmap, &parse_markers(SAVE), (1, 3)).is_err(), "there is no army 3");
    }

    #[test]
    fn ground_under_water_is_blocked() {
        let scmap = parse_scmap(&scmap_bytes(64, Some(15.0))).unwrap();
        let map = convert("test", &scmap, &parse_markers(SAVE), (1, 2)).unwrap().map;
        assert!(map.blocked[10 * 64 + 10], "the flat ground at 10 is under water at 15");
        assert_eq!(map.water_level, Fp::from_num(100), "5 of the original's units over the lowest ground");
    }

    #[test]
    fn a_zip_is_refused_with_advice() {
        let error = import(Path::new("/tmp/some_map.zip"), (1, 2)).err().unwrap();
        assert!(error.contains("unzip"));
    }
}
