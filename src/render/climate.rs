//! Climates: each map's colors for its ground, rock, sand, water and sky, from a table here keyed
//! by the map's name, so that map files, and the simulation's checksums, are untouched. The
//! shaders read the chosen climate from a uniform buffer, and the minimap draws with the same
//! colors.
//!
//! Colors are in linear light, as the shaders work in.

use bytemuck::{Pod, Zeroable};

/// The colors of one climate, laid out as the shaders' `Climate` struct: each a `vec4`, the
/// last component unused except where said.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub struct Climate {
    pub grass_low: [f32; 4],
    pub grass_high: [f32; 4],
    /// Grass on dry, sunny slopes.
    pub grass_dry: [f32; 4],
    /// Grass where the ground is damp.
    pub grass_lush: [f32; 4],
    /// Bare earth, and in its last component how much more of the ground is bare than in the
    /// temperate climate, from 0 to 1.
    pub dirt: [f32; 4],
    pub dirt_damp: [f32; 4],
    pub rock: [f32; 4],
    pub rock_warm: [f32; 4],
    pub sand: [f32; 4],
    pub pebble: [f32; 4],
    /// The light from the sky, from the ground below, and from the sun.
    pub sky: [f32; 4],
    pub ground_bounce: [f32; 4],
    pub sun: [f32; 4],
    /// How much of each color water takes away per unit of depth.
    pub absorption: [f32; 4],
    /// Deep water's own color.
    pub water_deep: [f32; 4],
    /// The sky the water reflects, at the horizon and overhead.
    pub sky_horizon: [f32; 4],
    pub sky_zenith: [f32; 4],
    /// The haze far off in low views, and in its last component how thick it is.
    pub haze: [f32; 4],
}

const fn rgb(r: f32, g: f32, b: f32) -> [f32; 4] {
    [r, g, b, 0.0]
}

/// Green fields and blue water: how every map looked before climates.
pub const TEMPERATE: Climate = Climate {
    grass_low: rgb(0.10, 0.17, 0.09),
    grass_high: rgb(0.24, 0.27, 0.14),
    grass_dry: rgb(0.31, 0.29, 0.15),
    grass_lush: rgb(0.07, 0.15, 0.07),
    dirt: rgb(0.25, 0.21, 0.14),
    dirt_damp: rgb(0.15, 0.12, 0.08),
    rock: rgb(0.30, 0.28, 0.26),
    rock_warm: rgb(0.33, 0.29, 0.24),
    sand: rgb(0.44, 0.40, 0.29),
    pebble: rgb(0.29, 0.27, 0.24),
    sky: rgb(0.42, 0.45, 0.52),
    ground_bounce: rgb(0.20, 0.19, 0.16),
    sun: rgb(0.80, 0.77, 0.70),
    absorption: rgb(0.16, 0.055, 0.035),
    water_deep: rgb(0.02, 0.085, 0.15),
    sky_horizon: rgb(0.60, 0.68, 0.76),
    sky_zenith: rgb(0.22, 0.38, 0.62),
    haze: [0.62, 0.68, 0.76, 1.0],
};

/// Dry scrub on red earth and ochre rock, under a hot, pale sky.
pub const ARID: Climate = Climate {
    grass_low: rgb(0.19, 0.18, 0.08),
    grass_high: rgb(0.31, 0.27, 0.14),
    grass_dry: rgb(0.40, 0.33, 0.18),
    grass_lush: rgb(0.12, 0.15, 0.06),
    dirt: [0.36, 0.23, 0.13, 0.2],
    dirt_damp: rgb(0.21, 0.13, 0.08),
    rock: rgb(0.40, 0.27, 0.18),
    rock_warm: rgb(0.46, 0.29, 0.17),
    sand: rgb(0.53, 0.41, 0.26),
    pebble: rgb(0.35, 0.27, 0.20),
    sky: rgb(0.46, 0.46, 0.48),
    ground_bounce: rgb(0.27, 0.21, 0.14),
    sun: rgb(0.86, 0.79, 0.66),
    absorption: rgb(0.14, 0.06, 0.045),
    water_deep: rgb(0.03, 0.09, 0.13),
    sky_horizon: rgb(0.72, 0.70, 0.66),
    sky_zenith: rgb(0.30, 0.44, 0.62),
    haze: [0.74, 0.68, 0.58, 1.2],
};

/// Dark basalt and ash with sparse grass, black sand and a smoky sky.
pub const VOLCANIC: Climate = Climate {
    grass_low: rgb(0.075, 0.085, 0.06),
    grass_high: rgb(0.13, 0.13, 0.10),
    grass_dry: rgb(0.18, 0.16, 0.12),
    grass_lush: rgb(0.055, 0.08, 0.05),
    dirt: [0.13, 0.12, 0.11, 0.45],
    dirt_damp: rgb(0.07, 0.065, 0.06),
    rock: rgb(0.14, 0.135, 0.13),
    rock_warm: rgb(0.21, 0.14, 0.11),
    sand: rgb(0.15, 0.14, 0.13),
    pebble: rgb(0.10, 0.095, 0.09),
    sky: rgb(0.42, 0.42, 0.45),
    ground_bounce: rgb(0.18, 0.16, 0.14),
    sun: rgb(0.84, 0.74, 0.62),
    absorption: rgb(0.18, 0.07, 0.05),
    water_deep: rgb(0.015, 0.05, 0.08),
    sky_horizon: rgb(0.56, 0.56, 0.58),
    sky_zenith: rgb(0.24, 0.30, 0.42),
    haze: [0.52, 0.50, 0.50, 1.3],
};

/// Bright green islands, white sand and clear turquoise water.
pub const TROPICAL: Climate = Climate {
    grass_low: rgb(0.06, 0.17, 0.06),
    grass_high: rgb(0.16, 0.26, 0.08),
    grass_dry: rgb(0.30, 0.30, 0.12),
    grass_lush: rgb(0.04, 0.14, 0.05),
    dirt: rgb(0.28, 0.20, 0.12),
    dirt_damp: rgb(0.15, 0.11, 0.07),
    rock: rgb(0.32, 0.30, 0.27),
    rock_warm: rgb(0.36, 0.31, 0.25),
    sand: rgb(0.60, 0.54, 0.40),
    pebble: rgb(0.40, 0.36, 0.30),
    sky: rgb(0.42, 0.47, 0.55),
    ground_bounce: rgb(0.20, 0.20, 0.15),
    sun: rgb(0.85, 0.81, 0.72),
    absorption: rgb(0.22, 0.045, 0.04),
    water_deep: rgb(0.01, 0.09, 0.15),
    sky_horizon: rgb(0.62, 0.72, 0.80),
    sky_zenith: rgb(0.20, 0.42, 0.70),
    haze: [0.64, 0.72, 0.80, 0.9],
};

impl Climate {
    /// The climate of the map by this name. Maps not in the table are temperate.
    pub fn for_map(name: &str) -> Self {
        match name {
            "saddleback" => ARID,
            "twin_passes" => VOLCANIC,
            "scattered_isles" => TROPICAL,
            _ => TEMPERATE,
        }
    }
}

impl Default for Climate {
    fn default() -> Self {
        TEMPERATE
    }
}

/// A linear color as the screen shows it (sRGB), from 0 to 255 a channel.
pub fn to_screen(color: [f32; 4]) -> [u8; 3] {
    std::array::from_fn(|channel| {
        let linear = color[channel].clamp(0.0, 1.0);
        let encoded = if linear <= 0.003_130_8 { linear * 12.92 } else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 };
        (encoded * 255.0).round() as u8
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_is_as_many_vec4s_as_the_shader_has() {
        let shader = include_str!("shaders.wgsl");
        let start = shader.find("struct Climate {").expect("the shaders have a Climate struct");
        let body = &shader[start..start + shader[start..].find("};").expect("it ends")];
        let fields = body.lines().filter(|line| line.trim_end().ends_with(": vec4<f32>,")).count();
        assert_eq!(std::mem::size_of::<Climate>(), fields * 16);
    }

    #[test]
    fn the_bundled_maps_each_have_a_climate_and_others_are_temperate() {
        assert_eq!(Climate::for_map("saddleback"), ARID);
        assert_eq!(Climate::for_map("twin_passes"), VOLCANIC);
        assert_eq!(Climate::for_map("scattered_isles"), TROPICAL);
        assert_eq!(Climate::for_map("highland"), TEMPERATE);
        assert_eq!(Climate::for_map("generated"), TEMPERATE);
    }

    #[test]
    fn colors_reach_the_screen_brighter_than_they_are_in_linear_light() {
        assert_eq!(to_screen([0.0, 1.0, 0.216, 0.0]), [0, 255, 128]);
    }
}
