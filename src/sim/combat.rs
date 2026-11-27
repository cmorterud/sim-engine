//! Ballistics math (SPECIFICATION.md §4.5). `SimWorld` applies the results.

use crate::math::fp::{Fp, Vec3Fp, FP_FRAC_BITS};

use super::config::{GRAVITY_RAW, TERRAIN_CELL_UNITS};
use super::components::Missile;
use super::spatial::GridEntry;
use super::terrain::Terrain;
use crate::math::fp::{mul_toward_zero, sqrt_wide, Vec2Fp};

pub const GRAVITY: Fp = Fp::from_bits(GRAVITY_RAW);

/// The speed a beam weapon's shots are given, in units a tick: further than any map is across, so
/// that a laser's beam reaches whatever it is fired at within the tick (`WeaponKind::Laser`).
pub const BEAM_SPEED: Fp = Fp::from_bits(100_000 << FP_FRAC_BITS);

/// Whether a shot at this speed is a beam: it crosses to its target in a straight line within
/// the tick it is fired, with no arc, no lead, and no scatter.
pub fn is_beam(speed: Fp) -> bool {
    speed >= BEAM_SPEED
}

/// A firing solution: the launch velocity and the number of ticks to reach the aim point.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FiringSolution {
    pub velocity: Vec3Fp,
    pub flight_ticks: u32,
}

/// Solves the ballistic arc from `from` to `to`, with no trigonometry. Under
/// `P += V; Vz -= g`, the height after `t` ticks is `Vz * t - g * t * (t - 1) / 2`.
pub fn firing_solution(from: Vec3Fp, to: Vec3Fp, horizontal_speed: Fp) -> FiringSolution {
    firing_solution_within(from, to, horizontal_speed, 1)
}

/// As `firing_solution`, but taking no fewer than `least` ticks, so that a shell can arrive just
/// as a target coming on does, flying a little slower than it could.
pub fn firing_solution_within(from: Vec3Fp, to: Vec3Fp, horizontal_speed: Fp, least: u32) -> FiringSolution {
    let delta = to - from;
    let distance = sqrt_wide(delta.xy().length_squared()).to_bits() as i64;
    let speed = i64::from(horizontal_speed.to_bits()).max(1);
    let ticks = ((distance + speed - 1) / speed).max(i64::from(least)).max(1);
    let divide = |raw: i32| Fp::from_bits((i64::from(raw) / ticks) as i32);
    let arc = (i64::from(GRAVITY_RAW) * (ticks - 1)) / 2;
    FiringSolution {
        velocity: Vec3Fp::new(
            divide(delta.x.to_bits()),
            divide(delta.y.to_bits()),
            divide(delta.z.to_bits()) + Fp::from_bits(arc as i32),
        ),
        flight_ticks: ticks as u32,
    }
}

/// How high an artillery shell's arc rises above the line from gun to aim, as a share of the
/// distance across: about a 58 degree launch, a high lob.
pub const ARC_HEIGHT_SHARE: (i64, i64) = (2, 5);
/// And no higher than this, in units, so that the longest-ranged shells stay in view.
pub const ARC_HEIGHT_MAX_UNITS: i64 = 3_000;

/// An artillery shell's flight (GAME_SPECIFICATION.md §7, Artillery): as long as a flat shell's
/// at the same speed across the ground, so that it leads its target the same, but up in a high
/// arc, under a gravity of its own, and down on where it was aimed. Returns the solution and that
/// gravity, the fall a tick.
pub fn lobbed_solution(from: Vec3Fp, to: Vec3Fp, horizontal_speed: Fp, least: u32) -> (FiringSolution, Fp) {
    let delta = to - from;
    let distance = sqrt_wide(delta.xy().length_squared()).to_bits() as i64;
    let speed = i64::from(horizontal_speed.to_bits()).max(1);
    let ticks = ((distance + speed - 1) / speed).max(i64::from(least)).max(1);
    let max_height = ARC_HEIGHT_MAX_UNITS << crate::math::fp::FP_FRAC_BITS;
    let height = (distance * ARC_HEIGHT_SHARE.0 / ARC_HEIGHT_SHARE.1).min(max_height);
    // A rise of `height` at the middle of a flight of `ticks`: g t^2 / 8.
    let gravity = (height * 8 / (ticks * ticks)).max(i64::from(GRAVITY_RAW));
    let divide = |raw: i32| Fp::from_bits((i64::from(raw) / ticks) as i32);
    // Moving first and then falling each tick, it drops g (t - 1) t / 2 in t ticks.
    let rise = gravity * (ticks - 1) / 2;
    let solution = FiringSolution {
        velocity: Vec3Fp::new(divide(delta.x.to_bits()), divide(delta.y.to_bits()), divide(delta.z.to_bits()) + Fp::from_bits(rise as i32)),
        flight_ticks: ticks as u32,
    };
    (solution, Fp::from_bits(gravity as i32))
}

/// Where to aim a shell so that it meets a target moving at `velocity` a tick (GAME_SPECIFICATION.md
/// §7.1, Leading Shots): the first point along the target's course that the shell can reach by
/// the time the target gets there. The shell moves first on the tick it is fired, after the
/// target has, so by its `t`th move the target has moved `t - 1` times. Only the ground position
/// leads; the height keeps the target's height over the surface beneath it, given by `surface`.
/// A target the shell could never catch within `LEAD_MAX_TICKS` is aimed at where it will be then.
/// Returns the point and the ticks the shell should take to get there, which, against a target
/// coming on, can be a little more than it needs, so that it does not arrive early.
pub fn lead_point(from: Vec3Fp, to: Vec3Fp, velocity: Vec3Fp, horizontal_speed: Fp, surface: impl Fn(Vec2Fp) -> Fp) -> (Vec3Fp, u32) {
    if velocity.x == Fp::ZERO && velocity.y == Fp::ZERO {
        return (to, 1);
    }
    let raw = |value: Fp| i128::from(value.to_bits());
    let speed = raw(horizontal_speed).max(1);
    let (vx, vy) = (raw(velocity.x), raw(velocity.y));
    let (x, y) = (raw(to.x) - raw(from.x), raw(to.y) - raw(from.y));
    let reach = |t: i128| {
        let (ax, ay) = (x + vx * (t - 1), y + vy * (t - 1));
        ax * ax + ay * ay <= speed * speed * t * t
    };
    // The shell closes at most its speed plus the target's a tick, so none sooner than this.
    let closing = speed + i128::from(sqrt_wide(velocity.xy().length_squared()).to_bits());
    let soonest = (i128::from(sqrt_wide((to.xy() - from.xy()).length_squared()).to_bits()) / closing).max(1);
    let ticks = (soonest..=LEAD_MAX_TICKS).find(|&t| reach(t)).unwrap_or(LEAD_MAX_TICKS);
    let clamp = |value: i128| Fp::from_bits(value.clamp(i128::from(i32::MIN / 2), i128::from(i32::MAX / 2)) as i32);
    let aim = Vec2Fp::new(clamp(raw(to.x) + vx * (ticks - 1)), clamp(raw(to.y) + vy * (ticks - 1)));
    let height = to.z - surface(to.xy());
    (Vec3Fp::new(aim.x, aim.y, surface(aim) + height), ticks as u32)
}

/// The longest flight a shell leads its target for: past the slowest artillery's longest.
const LEAD_MAX_TICKS: i128 = 600;

/// Where a shell flying straight at `speed` a tick, in three dimensions, from `from` meets a
/// target now at `to`, moving at `velocity` a tick across the ground and turning by `turn` (the
/// cosine and sine of the angle) each tick, as an aircraft in a steady turn does, and how many
/// ticks the shell takes. The shell moves first this tick, after the target has, so by its last
/// move the target has moved one tick fewer. If it can never catch it within `most` ticks, where
/// the target will be by then.
pub fn intercept(from: Vec3Fp, to: Vec3Fp, velocity: Vec3Fp, turn: Vec2Fp, speed: Fp, most: u32) -> (u32, Vec3Fp) {
    let raw = |value: Fp| i128::from(value.to_bits());
    let speed = raw(speed).max(1);
    let (mut at, mut step) = (to, velocity.xy());
    for ticks in 1..=most {
        let (x, y, z) = (raw(at.x) - raw(from.x), raw(at.y) - raw(from.y), raw(at.z) - raw(from.z));
        let t = i128::from(ticks);
        if x * x + y * y + z * z <= speed * speed * t * t || ticks == most {
            return (ticks, at);
        }
        // Each tick it turns, then moves along its new heading. Rounded toward zero, so that a
        // shot at an aircraft and at its mirror image lead alike: `*` rounds toward negative
        // infinity, and the step of one is the other's negated while the turn is the same.
        let times = mul_toward_zero;
        step = Vec2Fp::new(times(step.x, turn.x) - times(step.y, turn.y), times(step.x, turn.y) + times(step.y, turn.x));
        at = Vec3Fp::new(at.x + step.x, at.y + step.y, at.z);
    }
    (most, at)
}

/// A segment's closest approach to a sphere around `(x, y, z)`. Returns how far along
/// the segment that happens, as raw `Fp` from 0 to 1, if it is within `radius`.
pub fn segment_hit(start: Vec3Fp, step: Vec3Fp, x: i32, y: i32, z: i32, radius: i32) -> Option<i32> {
    let (sx, sy, sz) = (i64::from(start.x.to_bits()), i64::from(start.y.to_bits()), i64::from(start.z.to_bits()));
    let (dx, dy, dz) = (i64::from(step.x.to_bits()), i64::from(step.y.to_bits()), i64::from(step.z.to_bits()));
    let (mx, my, mz) = (i64::from(x) - sx, i64::from(y) - sy, i64::from(z) - sz);
    let along = mx * dx + my * dy + mz * dz;
    let length_squared = dx * dx + dy * dy + dz * dz;
    let one = 1i64 << FP_FRAC_BITS;
    let t = if length_squared == 0 || along <= 0 {
        0
    } else if along >= length_squared {
        one
    } else {
        // i128 keeps this exact even for very fast shells over long segments.
        ((i128::from(along) << FP_FRAC_BITS) / i128::from(length_squared)) as i64
    };
    // Divided rather than shifted, rounding toward zero, so that a shot and its mirror image
    // come equally close: shifted, the one flying the negative way along an axis came a raw bit
    // further each time, and could hit where its image missed.
    let part = |d: i64| d * t / (1 << FP_FRAC_BITS);
    let (ax, ay, az) = (mx - part(dx), my - part(dy), mz - part(dz));
    let radius = i64::from(radius);
    (ax * ax + ay * ay + az * az <= radius * radius).then_some(t as i32)
}

/// A missile's velocity for the next tick: towards its target at its speed, at its cruise
/// height above the ground, and over the last three cruise heights, down a straight line to just
/// below the target, so that it lands on it. It climbs no steeper than 45 degrees and dives no
/// steeper than about 63, and once over the target it drops straight down.
pub fn missile_velocity(terrain: &Terrain, from: Vec3Fp, missile: &Missile) -> Vec3Fp {
    let here = from.xy();
    let away = missile.target - here;
    let distance = sqrt_wide(away.length_squared());
    let along = missile.speed.min(distance);
    let flat = if distance > Fp::ZERO {
        let direction = away.normalized();
        direction.scaled(along)
    } else {
        Vec2Fp::ZERO
    };
    let next = here + flat;
    let left = distance - along;
    let dive = missile.cruise_height * 3;
    let landing = terrain.height_at(missile.target) - Fp::ONE;
    let cruise = terrain.height_at(here).max(terrain.height_at(next)) + missile.cruise_height;
    let wanted = if left >= dive || dive == Fp::ZERO { cruise } else { landing + (cruise.max(landing) - landing) * (left / dive) };
    let climb = missile.speed;
    Vec3Fp::new(flat.x, flat.y, (wanted - from.z).clamp(-climb * 2, climb))
}

/// Where a segment first enters a sphere around `center`, as raw `Fp` from 0 to 1. Only a
/// segment that starts outside can enter, which is what lets shells fired from inside a shield
/// dome fly out. Exact, in integers: the smaller root of |start + t * step - center| = radius.
pub fn sphere_entry(start: Vec3Fp, step: Vec3Fp, center: [i32; 3], radius: i32) -> Option<i32> {
    dome_entry(start, step, center, radius, radius)
}

/// Where a segment first enters a dome around `center`, as `sphere_entry` does a sphere: an
/// upright ellipsoid `radius` across and `height` high, a sphere when the two are equal, so that a
/// flattened shield stays beneath aircraft and their fire must come down through it (2026-11-24).
/// Heights stretched by radius / height make it a sphere of its radius; the stretch rounds toward
/// zero, the same for a shot and its mirror image, which differ only across.
pub fn dome_entry(start: Vec3Fp, step: Vec3Fp, center: [i32; 3], radius: i32, height: i32) -> Option<i32> {
    let stretch = |value: i128| if height == radius { value } else { value * i128::from(radius) / i128::from(height.max(1)) };
    let away = [
        i128::from(start.x.to_bits()) - i128::from(center[0]),
        i128::from(start.y.to_bits()) - i128::from(center[1]),
        stretch(i128::from(start.z.to_bits()) - i128::from(center[2])),
    ];
    let step = [i128::from(step.x.to_bits()), i128::from(step.y.to_bits()), stretch(i128::from(step.z.to_bits()))];
    let dot = |a: [i128; 3], b: [i128; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let outside = dot(away, away) - i128::from(radius) * i128::from(radius);
    let toward = dot(away, step);
    let length_squared = dot(step, step);
    if outside <= 0 || toward >= 0 || length_squared == 0 {
        return None;
    }
    let discriminant = toward * toward - length_squared * outside;
    if discriminant < 0 {
        return None;
    }
    let along = -toward - discriminant.isqrt();
    (along <= length_squared).then(|| ((along << FP_FRAC_BITS) / length_squared) as i32)
}

/// Whether a grid entry can be hit by this segment, and how far along. Only used
/// for entries that have already passed the team and source filters.
pub fn entry_hit(entry: &GridEntry, start: Vec3Fp, step: Vec3Fp) -> Option<i32> {
    segment_hit(start, step, entry.x, entry.y, entry.z, entry.radius)
}

/// The result of sampling terrain under a projectile's step.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TerrainProbe {
    /// Where along the segment the projectile first reaches the ground, as raw `Fp` from 0 to 1.
    pub hit: Option<i32>,
    /// The highest ground sampled, for skipping unit checks on shells high above.
    pub highest_ground: Fp,
}

/// Samples the height map along a segment at intervals of at most one terrain cell.
pub fn probe_terrain(terrain: &Terrain, start: Vec3Fp, step: Vec3Fp) -> TerrainProbe {
    probe_ground(terrain, start, step, false)
}

/// As `probe_terrain`, but over the sea the water's surface counts as the ground: a bomb bursts
/// on the water rather than sinking to the sea floor. Where it meets the surface is worked out
/// exactly, not at the sample, so the burst is on the water and not a step below it.
pub fn probe_surface(terrain: &Terrain, start: Vec3Fp, step: Vec3Fp) -> TerrainProbe {
    probe_ground(terrain, start, step, true)
}

fn probe_ground(terrain: &Terrain, start: Vec3Fp, step: Vec3Fp, water_is_ground: bool) -> TerrainProbe {
    let span = step.x.to_bits().abs().max(step.y.to_bits().abs());
    let interval = TERRAIN_CELL_UNITS << FP_FRAC_BITS;
    let samples = (span / interval + 1).max(1);
    let one = 1i32 << FP_FRAC_BITS;
    let mut highest_ground = Fp::MIN;
    for sample in 1..=samples {
        let t = one * sample / samples;
        let along = |value: Fp| Fp::from_bits(((i64::from(value.to_bits()) * i64::from(t)) / (1 << FP_FRAC_BITS)) as i32);
        let at = Vec2Fp::new(start.x + along(step.x), start.y + along(step.y));
        let floor = terrain.height_at(at);
        let water = terrain.water_level();
        let ground = if water_is_ground { floor.max(water) } else { floor };
        highest_ground = highest_ground.max(ground);
        if start.z + along(step.z) <= ground {
            // On the water, the step is cut where it crosses the surface, if that is sooner.
            if water_is_ground && floor < water && step.z < Fp::ZERO && start.z > water {
                let crossing = ((i64::from((start.z - water).to_bits()) << FP_FRAC_BITS) / i64::from(-step.z.to_bits())) as i32;
                return TerrainProbe { hit: Some(crossing.min(t)), highest_ground };
            }
            return TerrainProbe { hit: Some(t), highest_ground };
        }
    }
    TerrainProbe { hit: None, highest_ground }
}

/// Whether a shell fired from `from` at `to` gets there before the ground stops it. It flies
/// the same arc as a real shell. Ground in the last step does not count, because a target on a
/// slope is hit before its shell lands.
pub fn shot_is_clear(terrain: &Terrain, from: Vec3Fp, to: Vec3Fp, horizontal_speed: Fp) -> bool {
    // A beam goes straight there, so the ground must be clear of that line all the way.
    if is_beam(horizontal_speed) {
        return probe_terrain(terrain, from, to - from).hit.is_none();
    }
    let solution = firing_solution(from, to, horizontal_speed);
    let (mut position, mut velocity) = (from, solution.velocity);
    for _ in 1..solution.flight_ticks {
        if probe_terrain(terrain, position, velocity).hit.is_some() {
            return false;
        }
        position = position + velocity;
        velocity.z -= GRAVITY;
    }
    true
}

/// Whether a shell fired from `from` at a spot on the ground lands within `reach` of it, across
/// the ground, rather than on a rise in between or on the near side of a slope.
pub fn shot_lands_near(terrain: &Terrain, from: Vec3Fp, to: Vec3Fp, horizontal_speed: Fp, reach: Fp) -> bool {
    let solution = firing_solution(from, to, horizontal_speed);
    let (mut position, mut velocity) = (from, solution.velocity);
    for _ in 0..solution.flight_ticks {
        if let Some(along) = probe_terrain(terrain, position, velocity).hit {
            let part = |value: Fp| Fp::from_bits(((i64::from(value.to_bits()) * i64::from(along)) / (1 << FP_FRAC_BITS)) as i32);
            let landing = Vec2Fp::new(position.x + part(velocity.x), position.y + part(velocity.y));
            return (landing - to.xy()).length_squared() <= reach * reach;
        }
        position = position + velocity;
        velocity.z -= GRAVITY;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::fp::fp_int;

    fn v(x: i32, y: i32, z: i32) -> Vec3Fp {
        Vec3Fp::new(fp_int(x), fp_int(y), fp_int(z))
    }

    /// Runs the same integration as the simulation and returns the landing point.
    fn fly(from: Vec3Fp, solution: FiringSolution) -> Vec3Fp {
        let (mut position, mut velocity) = (from, solution.velocity);
        for _ in 0..solution.flight_ticks {
            position = position + velocity;
            velocity.z -= GRAVITY;
        }
        position
    }

    #[test]
    fn solution_lands_on_aim_point_within_rounding() {
        let cases = [
            (v(0, 0, 0), v(30, 0, 0), Fp::lit("2")),
            (v(0, 0, 5), v(-21, 17, 1), Fp::lit("2")),
            (v(100, 100, 0), v(100, 400, 25), Fp::lit("1.5")),
            (v(0, 0, 0), v(20_000, 15_000, 40), fp_int(30)),
        ];
        for (from, to, speed) in cases {
            let solution = firing_solution(from, to, speed);
            let landing = fly(from, solution);
            // Rounding is bounded by about t / 4096 units per axis, plus one tick of gravity.
            let tolerance = i64::from(solution.flight_ticks + 1);
            for (actual, expected) in [(landing.x, to.x), (landing.y, to.y), (landing.z, to.z)] {
                let error = (i64::from(actual.to_bits()) - i64::from(expected.to_bits())).abs();
                assert!(error <= tolerance * 2, "{from:?} to {to:?}: error {error} raw over {} ticks", solution.flight_ticks);
            }
        }
    }

    #[test]
    fn zero_distance_shot_is_valid() {
        let solution = firing_solution(v(5, 5, 5), v(5, 5, 5), fp_int(2));
        assert_eq!(solution.flight_ticks, 1);
        assert_eq!(solution.velocity, Vec3Fp::ZERO);
    }

    #[test]
    fn a_shell_leads_a_moving_target_by_its_flight_time() {
        let flat = |_: Vec2Fp| Fp::ZERO;
        let (from, to) = (v(0, 0, 0), v(300, 0, 0));
        assert_eq!(lead_point(from, to, Vec3Fp::ZERO, fp_int(60), flat), (to, 1), "a target standing still is aimed at");
        // Moving sideways at 6 a tick: in 5 ticks it has moved 4 times, to (300, 24), just past
        // 300 away; in 6 it has moved 5 times, to (300, 30), well within 360.
        let (aim, ticks) = lead_point(from, to, v(0, 6, 0), fp_int(60), flat);
        assert_eq!((aim.x, aim.y, aim.z, ticks), (to.x, fp_int(30), to.z, 6));
        // The height follows the ground under the aim point.
        let slope = |at: Vec2Fp| at.y / 2;
        assert_eq!(lead_point(from, v(300, 0, 5), v(0, 6, 0), fp_int(60), slope).0.z, fp_int(20));
    }

    /// The shell and the target arrive together on any course, however slow the shell: straight
    /// at the gun, straight away, across, and faster than the shell closes.
    #[test]
    fn a_shell_meets_its_target_on_any_course() {
        let flat = |_: Vec2Fp| Fp::ZERO;
        let (from, to) = (v(0, 0, 0), v(800, 0, 0));
        for speed in [10, 20, 40, 60] {
            for (x, y) in [(-6, 0), (6, 0), (0, 6), (-12, 0), (5, -5)] {
                let (aim, least) = lead_point(from, to, v(x, y, 0), fp_int(speed), flat);
                let ticks = firing_solution_within(from, aim, fp_int(speed), least).flight_ticks as i32;
                assert_eq!(aim, v(800 + x * (ticks - 1), y * (ticks - 1), 0), "speed {speed}, moving ({x}, {y})");
                let solution = firing_solution_within(from, aim, fp_int(speed), least);
                assert!(sqrt_wide(Vec2Fp::new(solution.velocity.x, solution.velocity.y).length_squared()) <= fp_int(speed), "no faster than it can fly");
            }
        }
    }

    #[test]
    fn segment_hits_sphere_between_its_endpoints() {
        // A 50-unit step passes through a radius-1 sphere that neither endpoint touches.
        let hit = segment_hit(v(0, 0, 0), v(50, 0, 0), fp_int(25).to_bits(), 0, 0, fp_int(1).to_bits());
        assert_eq!(hit, Some(2048)); // Halfway along.
    }

    #[test]
    fn segment_misses_sphere_off_to_the_side() {
        let hit = segment_hit(v(0, 0, 0), v(50, 0, 0), fp_int(25).to_bits(), fp_int(2).to_bits(), 0, fp_int(1).to_bits());
        assert_eq!(hit, None);
    }

    #[test]
    fn segment_does_not_reach_sphere_beyond_its_end() {
        let hit = segment_hit(v(0, 0, 0), v(10, 0, 0), fp_int(25).to_bits(), 0, 0, fp_int(1).to_bits());
        assert_eq!(hit, None);
    }

    #[test]
    fn a_segment_enters_a_sphere_where_it_crosses_the_surface() {
        let raw = |value: i32| fp_int(value).to_bits();
        let center = [raw(100), 0, 0];
        // From 0 to 100 along x, into a sphere of radius 30 around x = 100: in at x = 70.
        assert_eq!(sphere_entry(v(0, 0, 0), v(100, 0, 0), center, raw(30)), Some(2_867));
        // Stopping short of it, passing beside it, or going away from it does not enter.
        assert_eq!(sphere_entry(v(0, 0, 0), v(60, 0, 0), center, raw(30)), None);
        assert_eq!(sphere_entry(v(0, 40, 0), v(200, 0, 0), center, raw(30)), None);
        assert_eq!(sphere_entry(v(200, 0, 0), v(100, 0, 0), center, raw(30)), None);
        // Starting inside never enters, whichever way it goes.
        assert_eq!(sphere_entry(v(90, 0, 0), v(100, 0, 0), center, raw(30)), None);
        assert_eq!(sphere_entry(v(90, 0, 0), v(-100, 0, 0), center, raw(30)), None);
    }

    #[test]
    fn a_flattened_dome_is_entered_through_its_lower_top() {
        let raw = |value: i32| fp_int(value).to_bits();
        let center = [raw(100), 0, 0];
        // Straight down from 25 above the middle of a dome 30 across and 10 high: in at 10, halfway
        // down a step of 30. A sphere of radius 30 would have it start inside, and never enter.
        assert_eq!(dome_entry(v(100, 0, 25), v(0, 0, -30), center, raw(30), raw(10)), Some(2_048));
        assert_eq!(dome_entry(v(100, 0, 25), v(0, 0, -30), center, raw(30), raw(30)), None);
        // Across the water it reaches as far as the sphere does.
        assert_eq!(dome_entry(v(0, 0, 0), v(100, 0, 0), center, raw(30), raw(10)), sphere_entry(v(0, 0, 0), v(100, 0, 0), center, raw(30)));
        // Passing over it, above its top, does not enter.
        assert_eq!(dome_entry(v(0, 0, 12), v(200, 0, 0), center, raw(30), raw(10)), None);
    }

    #[test]
    fn stationary_projectile_inside_a_sphere_hits() {
        let hit = segment_hit(v(0, 0, 0), Vec3Fp::ZERO, 0, 0, 0, fp_int(1).to_bits());
        assert_eq!(hit, Some(0));
    }
}
