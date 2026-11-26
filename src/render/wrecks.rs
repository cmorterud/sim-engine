//! Wrecks as they are drawn (GAME_SPECIFICATION.md §6.1): each the charred shell of what it was,
//! if that was seen, moving smoothly between snapshots while it falls or sinks, and tipped over
//! as it goes: a downed aircraft dives nose first along the way it falls, rolling as it comes
//! down, and debris in the water settles bow down and listing to one side, as it then lies on
//! the sea floor. Over each wreck under the water a slick of oil spreads on the surface, which
//! shows where it lies, since the water hides the floor, and which a right-click on it reclaims.

use super::effects::Effects;
use super::instances::{merge_by_id, painted, scenery_instance, wreck_instance, wreck_model_instances, InstanceBatches, Tilt, UnitLook, RAW_TO_WORLD};
use super::meshes::Shape;
use crate::sim::world::{WreckMotion, WreckView};

/// How far debris in the water tips bow down at most, and lists to one side, in radians, once it
/// is twice its radius under the surface: about 10 to 21 degrees down, and up to 17 either way.
const SUNK_PITCH: f32 = 0.18;
const SUNK_PITCH_SPREAD: f32 = 0.19;
const SUNK_LIST: f32 = 0.3;
/// How steeply a falling aircraft dives, as a share of how steeply it is falling, the most it
/// dives, in radians, and how fast it rolls as it comes down, in radians for each unit it falls.
const DIVE_SHARE: f32 = 0.85;
const MOST_DIVE: f32 = 1.2;
const FALLING_ROLL: f32 = 0.03;
/// How wide a slick of oil spreads over a wreck on the floor, as a share of its radius.
const SLICK_SPREAD: f32 = 4.0;

/// A number from 0 to 1 that depends only on `seed`.
fn noise(seed: u64) -> f32 {
    let mut value = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^= value >> 31;
    ((value >> 40) as f32) / (1u64 << 24) as f32
}

/// How far a wreck is under the water, from 0 with its underside at the surface to 1 once it is
/// twice its radius down, or `None` if it is not in the water at all.
fn sunk_share(wreck: &WreckView, middle: f64, radius: f64, water: Option<f64>) -> Option<f32> {
    let water = water?;
    let underside = middle - radius * 0.5;
    let in_water = wreck.motion == WreckMotion::Sinking || (wreck.motion == WreckMotion::Still && middle < water);
    in_water.then(|| ((water - underside) / (radius * 2.0)).clamp(0.0, 1.0) as f32)
}

/// How a wreck is tipped over: diving and rolling as it falls, bow down and listing in the
/// water, level on the ground. `facing` is the way it points, `velocity` how far it went in the
/// last tick, in units.
pub fn tilt(wreck: &WreckView, middle: [f64; 3], radius: f64, facing: [f32; 2], velocity: [f64; 3], water: Option<f64>) -> Option<Tilt> {
    let forward = [facing[0], facing[1], 0.0];
    let right = [facing[1], -facing[0], 0.0];
    let seed = u64::from(wreck.id.0);
    let (pitch, roll) = match wreck.motion {
        WreckMotion::Falling => {
            let across = velocity[0].hypot(velocity[1]);
            let dive = ((-velocity[2]).atan2(across.max(0.01)) as f32).clamp(0.0, MOST_DIVE / DIVE_SHARE) * DIVE_SHARE;
            let spin = if noise(seed ^ 0x5) < 0.5 { -1.0 } else { 1.0 };
            (dive, noise(seed ^ 0x7) * std::f32::consts::TAU + spin * middle[2] as f32 * FALLING_ROLL)
        }
        _ => {
            let share = sunk_share(wreck, middle[2], radius, water)?;
            (share * (SUNK_PITCH + SUNK_PITCH_SPREAD * noise(seed ^ 0x11)), share * SUNK_LIST * (noise(seed ^ 0x13) * 2.0 - 1.0))
        }
    };
    if pitch == 0.0 && roll == 0.0 {
        return None;
    }
    // Rolled about the way it points, then pitched nose down about its side.
    Some(Tilt::about(middle, forward, roll).then(&Tilt::about(middle, right, -pitch)))
}

/// Draws every wreck in `current`: scenery as trees and rocks, and debris as the shell of what it
/// was, where `effects` saw it made, or a heap of debris, with a slick over it if it is under the
/// water. `alpha` is how far the frame is from `previous` to `current`.
#[allow(clippy::too_many_arguments)]
pub fn build_wreck_instances(previous: &[WreckView], current: &[WreckView], alpha: f64, eye: [f64; 3], effects: &Effects, looks: &[UnitLook], water: Option<f64>, time: f64, out: &mut InstanceBatches) {
    merge_by_id(previous, current, |wreck| wreck.id, |before, wreck| {
        let now = [wreck.x, wreck.y, wreck.z].map(|value| f64::from(value) * RAW_TO_WORLD);
        let then = before.map_or(now, |before| [before.x, before.y, before.z].map(|value| f64::from(value) * RAW_TO_WORLD));
        let radius = f64::from(wreck.radius) * RAW_TO_WORLD;
        if wreck.kind.is_scenery() {
            let (shape, instance) = scenery_instance(wreck.kind, now, radius, eye);
            out.push(shape, instance);
            return;
        }
        let middle: [f64; 3] = std::array::from_fn(|axis| then[axis] + (now[axis] - then[axis]) * alpha);
        let velocity: [f64; 3] = std::array::from_fn(|axis| now[axis] - then[axis]);
        let origin = effects.wreck_origin(wreck.id).filter(|(blueprint, _)| usize::from(*blueprint) < looks.len());
        let facing = origin.map_or([0.0, 1.0], |(_, facing)| facing);
        let tilt = tilt(wreck, middle, radius, facing, velocity, water);
        match origin {
            Some((blueprint, facing)) => wreck_model_instances(middle, radius, &looks[usize::from(blueprint)], facing, eye, tilt.as_ref(), out),
            None => {
                let mut instance = wreck_instance(middle, radius, eye);
                if let Some(tilt) = &tilt {
                    tilt.apply(&mut instance, eye);
                }
                out.push(Shape::Wreck, instance);
            }
        }
        if let Some(share) = sunk_share(wreck, middle[2], radius, water).filter(|&share| share > 0.0) {
            out.push(Shape::Scar, slick(wreck, middle, radius, share, water.unwrap_or(middle[2]), time, eye));
        }
    });
}

/// The slick of oil over a wreck under the water: spreading from it as it sinks, at its widest
/// once it is down, and turning slowly as it drifts. Drawn after the water, by the scar shader's
/// fifth kind, with how strongly it shows, and a number that varies its shape.
fn slick(wreck: &WreckView, middle: [f64; 3], radius: f64, share: f32, water: f64, time: f64, eye: [f64; 3]) -> super::instances::UnitInstanceRaw {
    let seed = noise(u64::from(wreck.id.0) ^ 0x51);
    let width = (radius as f32 * SLICK_SPREAD * (0.4 + 0.6 * share)).max(6.0);
    let angle = seed * std::f32::consts::TAU + (time * 0.01) as f32;
    let look = [4.0, share.sqrt(), seed];
    painted([middle[0], middle[1], water], [width, width * (0.8 + 0.3 * seed), 1.0], [angle.cos(), angle.sin()], look, look, look, 0.0, eye)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::components::{SimId, WreckKind};

    fn view(motion: WreckMotion, z: f64) -> WreckView {
        let raw = |value: f64| (value * 4096.0) as i32;
        WreckView { id: SimId(7), x: raw(100.0), y: raw(100.0), z: raw(z), radius: raw(10.0), mass: 50, energy: 0, kind: WreckKind::Debris, motion }
    }

    /// Where the nose of a wreck pointing north ends up.
    fn nose(tilt: Option<Tilt>) -> [f32; 3] {
        tilt.map_or([0.0, 1.0, 0.0], |tilt| tilt.turn([0.0, 1.0, 0.0]))
    }

    /// A falling aircraft dives along the way it falls: level while it still goes on mostly
    /// across, steeper as it drops, but never straight down.
    #[test]
    fn a_falling_wreck_dives_along_the_way_it_falls() {
        let falling = view(WreckMotion::Falling, 200.0);
        let at = [100.0, 100.0, 200.0];
        let gentle = nose(tilt(&falling, at, 10.0, [0.0, 1.0], [0.0, 30.0, -2.0], None));
        let steep = nose(tilt(&falling, at, 10.0, [0.0, 1.0], [0.0, 5.0, -20.0], None));
        assert!(gentle[2] < 0.0 && gentle[2] > -0.15, "nearly level, going on fast: {gentle:?}");
        assert!(steep[2] < -0.75, "steeply down, falling fast: {steep:?}");
        assert!(steep[1] > 0.0, "but still ahead: {steep:?}");
    }

    /// In the water a wreck tips bow down as it sinks, and lies so on the floor; on land it lies
    /// level, and so does one that has only just touched the water.
    #[test]
    fn a_sinking_wreck_goes_bow_down_and_lies_so_on_the_floor() {
        let water = Some(100.0);
        let tipped = |motion, z: f64| nose(tilt(&view(motion, z), [100.0, 100.0, z], 10.0, [0.0, 1.0], [0.0; 3], water))[2];
        assert_eq!(tipped(WreckMotion::Sinking, 105.0), 0.0, "afloat, still level");
        let partly = tipped(WreckMotion::Sinking, 95.0);
        let deep = tipped(WreckMotion::Sinking, 60.0);
        assert!(partly < 0.0 && deep < partly, "further down, further over: {partly}, {deep}");
        assert_eq!(tipped(WreckMotion::Still, 55.0), deep, "on the floor, as it was going down");
        assert_eq!(tipped(WreckMotion::Still, 105.0), 0.0, "on land, level");
        assert!(tilt(&view(WreckMotion::Still, 55.0), [100.0, 100.0, 55.0], 10.0, [0.0, 1.0], [0.0; 3], None).is_none(), "with no water, nothing is sunk");
    }

    /// Debris under the water has a slick over it, on the surface, wider than it; on land, none.
    #[test]
    fn a_wreck_under_the_water_has_a_slick_over_it() {
        let effects = Effects::new();
        let draw = |wreck: WreckView| {
            let mut out = InstanceBatches::default();
            build_wreck_instances(&[wreck], &[wreck], 0.5, [0.0, 0.0, 500.0], &effects, &[], Some(100.0), 0.0, &mut out);
            out
        };
        let sunk = draw(view(WreckMotion::Still, 55.0));
        assert_eq!(sunk.shape(Shape::Wreck).len(), 1);
        let slick = sunk.shape(Shape::Scar);
        assert_eq!(slick.len(), 1, "a slick");
        assert_eq!(slick[0].color[0], 4.0, "of the fifth kind of scar");
        assert!((f64::from(slick[0].model_matrix[3][2]) - (100.0 - 500.0)).abs() < 1e-3, "on the water");
        assert!(slick[0].model_matrix[0][..2].iter().map(|v| v * v).sum::<f32>().sqrt() > 20.0, "wider than the wreck");
        assert!(draw(view(WreckMotion::Still, 105.0)).shape(Shape::Scar).is_empty(), "none on land");
    }
}
