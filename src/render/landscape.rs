//! Fixed views of a map's land and sea, for comparing how they look before and after a change
//! to the terrain or water shaders (`--landscape`). The views are picked from the heights alone,
//! so that the same map gives the same pictures on every run and every machine.

use super::camera::HeightField;

/// One picture to take: where the camera looks, from how high, and whether it tilts down to
/// its lowest angle to look across the ground instead of down on it.
#[derive(Clone, Debug, PartialEq)]
pub struct LandscapeView {
    /// The picture's file name, without the extension.
    pub name: String,
    pub focus: [f64; 2],
    pub altitude: f64,
    pub low_angle: bool,
}

/// How high the camera is for each place's pictures: close, near, and middle distance.
const ALTITUDES: [f64; 3] = [150.0, 500.0, 1_500.0];
/// How far apart the samples are that are looked through for each kind of place.
const SEARCH_STRIDE: usize = 4;

/// The views of a map: its shore, its land, and its deepest water at each of `ALTITUDES`, a
/// low look across the shore, its far corner up close, where the shaders' coordinates are
/// largest, and the whole map from high up. Places a map does not have, as water on a dry
/// map, are left out.
pub fn landscape_views(heights: &HeightField, water_level: f64) -> Vec<LandscapeView> {
    let samples = heights.samples();
    let cell = heights.cell_units();
    let size = (samples - 1) as f64 * cell;
    let middle = [size / 2.0, size / 2.0];
    let height = |x: usize, y: usize| f64::from(heights.heights()[y * samples + x]) * heights.units_per_step();
    let at = |x: usize, y: usize| [x as f64 * cell, y as f64 * cell];
    let from_middle = |point: [f64; 2]| (point[0] - middle[0]).hypot(point[1] - middle[1]);

    // Away from the map's edges, which the camera cannot centre on.
    let margin = (samples / 10).max(1);
    let inner = || (margin..samples.saturating_sub(margin)).step_by(SEARCH_STRIDE);
    let mut shore: Option<[f64; 2]> = None;
    let mut land: Option<[f64; 2]> = None;
    let mut deep: Option<([f64; 2], f64)> = None;
    let nearer = |best: Option<[f64; 2]>, point: [f64; 2]| best.is_none_or(|best| from_middle(point) < from_middle(best));
    for y in inner() {
        for x in inner() {
            let here = height(x, y);
            let point = at(x, y);
            // The shore: ground at the water's edge, with water on one side and land on the other.
            let east = height((x + SEARCH_STRIDE).min(samples - 1), y);
            if here < water_level && east > water_level && nearer(shore, point) {
                shore = Some(point);
            }
            if here > water_level + 20.0 && nearer(land, point) {
                land = Some(point);
            }
            if here < water_level && deep.is_none_or(|(_, lowest)| here < lowest) {
                deep = Some((point, here));
            }
        }
    }

    let mut views = Vec::new();
    for (place, point) in [("shore", shore), ("land", land), ("deep", deep.map(|(point, _)| point))] {
        let Some(point) = point else { continue };
        for altitude in ALTITUDES {
            views.push(LandscapeView { name: format!("{place}_{altitude:04.0}"), focus: point, altitude, low_angle: false });
        }
    }
    if let Some(point) = shore.or(land) {
        views.push(LandscapeView { name: "across_0300".into(), focus: point, altitude: 300.0, low_angle: true });
    }
    let corner = size * 0.9;
    views.push(LandscapeView { name: "corner_0150".into(), focus: [corner, corner], altitude: 150.0, low_angle: false });
    views.push(LandscapeView { name: "whole".into(), focus: middle, altitude: f64::INFINITY, low_angle: false });
    views
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bowl: low in the middle, rising to the edges, 20 units a cell, one unit a step.
    fn bowl(samples: usize) -> HeightField {
        let middle = (samples / 2) as f64;
        let heights = (0..samples * samples)
            .map(|index| {
                let (x, y) = ((index % samples) as f64, (index / samples) as f64);
                ((x - middle).hypot(y - middle) * 2.0 - 60.0) as i16
            })
            .collect();
        HeightField::new(samples, heights, 20.0, 1.0)
    }

    #[test]
    fn a_map_with_water_gets_shore_land_and_deep_views() {
        let views = landscape_views(&bowl(129), 0.0);
        for name in ["shore_0150", "land_0500", "deep_1500", "across_0300", "corner_0150", "whole"] {
            assert!(views.iter().any(|view| view.name == name), "{name} is missing from {views:?}");
        }
        let deep = views.iter().find(|view| view.name == "deep_0150").unwrap();
        assert!((deep.focus[0] - 64.0 * 20.0).abs() <= 4.0 * 20.0, "the deepest water is the bowl's middle: {deep:?}");
    }

    #[test]
    fn a_dry_map_gets_no_water_views() {
        let views = landscape_views(&bowl(129), -1_000.0);
        assert!(views.iter().all(|view| !view.name.starts_with("shore") && !view.name.starts_with("deep")));
        assert!(views.iter().any(|view| view.name == "land_0150"));
    }

    #[test]
    fn the_same_heights_give_the_same_views() {
        assert_eq!(landscape_views(&bowl(65), 0.0), landscape_views(&bowl(65), 0.0));
    }
}
