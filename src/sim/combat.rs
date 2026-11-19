//! Ballistics math (SPECIFICATION.md §4.5). `SimWorld` applies the results.

use crate::math::fp::{Fp, Vec3Fp, FP_FRAC_BITS};

use super::config::{GRAVITY_RAW, TERRAIN_CELL_UNITS};
use super::components::Missile;
use super::spatial::GridEntry;
use super::terrain::Terrain;
use crate::math::fp::{sqrt_wide, Vec2Fp};

pub const GRAVITY: Fp = Fp::from_bits(GRAVITY_RAW);

/// A firing solution: the launch velocity and the number of ticks to reach the aim point.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FiringSolution {
    pub velocity: Vec3Fp,
    pub flight_ticks: u32,
}

/// Solves the ballistic arc from `from` to `to`, with no trigonometry. Under
/// `P += V; Vz -= g`, the height after `t` ticks is `Vz * t - g * t * (t - 1) / 2`.
pub fn firing_solution(from: Vec3Fp, to: Vec3Fp, horizontal_speed: Fp) -> FiringSolution {
    let delta = to - from;
    let distance = sqrt_wide(delta.xy().length_squared()).to_bits() as i64;
    let speed = i64::from(horizontal_speed.to_bits()).max(1);
    let ticks = ((distance + speed - 1) / speed).max(1);
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

/// Where to aim a shell so that it meets a target moving at `velocity` a tick (GAME_SPECIFICATION.md
/// §7.1, Leading Shots): where the target will be once the shell has flown there, refined twice,
/// since a farther point takes longer to reach. Only the ground position leads; the height keeps
/// the target's height over the surface beneath it, given by `surface`.
pub fn lead_point(from: Vec3Fp, to: Vec3Fp, velocity: Vec3Fp, horizontal_speed: Fp, surface: impl Fn(Vec2Fp) -> Fp) -> Vec3Fp {
    if velocity.x == Fp::ZERO && velocity.y == Fp::ZERO {
        return to;
    }
    let ahead = |value: Fp, ticks: u32| Fp::from_bits((i64::from(value.to_bits()) * i64::from(ticks)).clamp(i64::from(i32::MIN / 2), i64::from(i32::MAX / 2)) as i32);
    let mut aim = to;
    for _ in 0..2 {
        let ticks = firing_solution(from, aim, horizontal_speed).flight_ticks;
        aim = Vec3Fp::new(to.x + ahead(velocity.x, ticks), to.y + ahead(velocity.y, ticks), to.z);
    }
    let height = to.z - surface(to.xy());
    Vec3Fp::new(aim.x, aim.y, surface(aim.xy()) + height)
}

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
        // Each tick it turns, then moves along its new heading.
        step = Vec2Fp::new(step.x * turn.x - step.y * turn.y, step.x * turn.y + step.y * turn.x);
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
    let (ax, ay, az) = (mx - ((dx * t) >> FP_FRAC_BITS), my - ((dy * t) >> FP_FRAC_BITS), mz - ((dz * t) >> FP_FRAC_BITS));
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
    let away = [
        i128::from(start.x.to_bits()) - i128::from(center[0]),
        i128::from(start.y.to_bits()) - i128::from(center[1]),
        i128::from(start.z.to_bits()) - i128::from(center[2]),
    ];
    let step = [i128::from(step.x.to_bits()), i128::from(step.y.to_bits()), i128::from(step.z.to_bits())];
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
    let span = step.x.to_bits().abs().max(step.y.to_bits().abs());
    let interval = TERRAIN_CELL_UNITS << FP_FRAC_BITS;
    let samples = (span / interval + 1).max(1);
    let one = 1i32 << FP_FRAC_BITS;
    let mut highest_ground = Fp::MIN;
    for sample in 1..=samples {
        let t = one * sample / samples;
        let along = |value: Fp| Fp::from_bits(((i64::from(value.to_bits()) * i64::from(t)) >> FP_FRAC_BITS) as i32);
        let at = Vec2Fp::new(start.x + along(step.x), start.y + along(step.y));
        let ground = terrain.height_at(at);
        highest_ground = highest_ground.max(ground);
        if start.z + along(step.z) <= ground {
            return TerrainProbe { hit: Some(t), highest_ground };
        }
    }
    TerrainProbe { hit: None, highest_ground }
}

/// Whether a shell fired from `from` at `to` gets there before the ground stops it. It flies
/// the same arc as a real shell. Ground in the last step does not count, because a target on a
/// slope is hit before its shell lands.
pub fn shot_is_clear(terrain: &Terrain, from: Vec3Fp, to: Vec3Fp, horizontal_speed: Fp) -> bool {
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
            let part = |value: Fp| Fp::from_bits(((i64::from(value.to_bits()) * i64::from(along)) >> FP_FRAC_BITS) as i32);
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
        assert_eq!(lead_point(from, to, Vec3Fp::ZERO, fp_int(60), flat), to, "a target standing still is aimed at");
        // Moving sideways at 6 a tick: 300 at 60 is 5 ticks, and the aim point's own distance
        // still rounds up to 6 ticks, so it aims 36 ahead.
        let aim = lead_point(from, to, v(0, 6, 0), fp_int(60), flat);
        assert_eq!((aim.x, aim.z), (to.x, to.z));
        assert_eq!(aim.y, fp_int(36));
        // The height follows the ground under the aim point.
        let slope = |at: Vec2Fp| at.y / 2;
        assert_eq!(lead_point(from, v(300, 0, 5), v(0, 6, 0), fp_int(60), slope).z, fp_int(23));
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
    fn stationary_projectile_inside_a_sphere_hits() {
        let hit = segment_hit(v(0, 0, 0), Vec3Fp::ZERO, 0, 0, 0, fp_int(1).to_bits());
        assert_eq!(hit, Some(0));
    }
}
