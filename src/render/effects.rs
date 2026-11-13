//! Explosions, missile trails, and ships' wakes: short-lived effects the renderer makes from the
//! snapshots alone. The simulation never sees them.
//!
//! Each is a puff that grows, changes color, and shrinks away over its life, drawn as an opaque
//! low-poly shape with the units. They follow the simulation's clock, so they stop while paused.

use super::instances::{merge_by_id, painted, InstanceBatches, UnitLook, RAW_TO_WORLD};
use super::meshes::Shape;
use crate::sim::components::{SimId, WreckKind};
use crate::sim::world::{ProjectileKind, SimSnapshot, UnitView};

/// The most puffs kept at once. The oldest go first, so that a huge battle cannot slow the frame.
const MAX_PUFFS: usize = 6_000;
/// How far apart a missile's trail puffs are, as a share of the puff's size.
const TRAIL_SPACING: f64 = 0.6;
/// How far apart a ship's wake puffs are, as a share of the ship's size.
const WAKE_SPACING: f64 = 0.35;

const FIRE_HOT: [f32; 3] = [1.0, 0.92, 0.55];
const FIRE_COOL: [f32; 3] = [0.85, 0.28, 0.05];
const SMOKE_DARK: [f32; 3] = [0.24, 0.23, 0.22];
const SMOKE_PALE: [f32; 3] = [0.45, 0.44, 0.42];
const TRAIL_FRESH: [f32; 3] = [0.92, 0.90, 0.86];
const TRAIL_OLD: [f32; 3] = [0.62, 0.62, 0.60];
const FOAM: [f32; 3] = [0.92, 0.96, 1.0];
const FOAM_OLD: [f32; 3] = [0.55, 0.68, 0.78];

#[derive(Copy, Clone, Debug, PartialEq)]
struct Puff {
    shape: Shape,
    at: [f64; 3],
    /// How fast it rises, in units a second.
    rise: f64,
    born: f64,
    life: f64,
    size: f32,
    /// How much bigger it grows over its life, as a share of its size.
    growth: f32,
    /// How flat it is, as a share of its size, for foam on the water.
    height_share: f32,
    color: [f32; 3],
    fade_to: [f32; 3],
}

impl Puff {
    /// Where it is, how big, and what color, at `time`, or `None` once it has gone.
    fn at_time(&self, time: f64) -> Option<([f64; 3], f32, [f32; 3])> {
        let age = time - self.born;
        if !(0.0..self.life).contains(&age) {
            return None;
        }
        let t = (age / self.life) as f32;
        let size = self.size * (1.0 + self.growth * t) * (1.0 - t * t * t);
        let color = std::array::from_fn(|channel| self.color[channel] + (self.fade_to[channel] - self.color[channel]) * t);
        Some(([self.at[0], self.at[1], self.at[2] + self.rise * age], size, color))
    }
}

/// A number from 0 to 1 that depends only on `seed`, so that effects vary without randomness.
fn noise(seed: u64) -> f64 {
    let mut value = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^= value >> 31;
    (value >> 11) as f64 / (1u64 << 53) as f64
}

fn raw_point(x: i32, y: i32, z: i32) -> [f64; 3] {
    [f64::from(x) * RAW_TO_WORLD, f64::from(y) * RAW_TO_WORLD, f64::from(z) * RAW_TO_WORLD]
}

/// What is going on, kept between frames.
#[derive(Default)]
pub struct Effects {
    puffs: Vec<Puff>,
    /// The latest snapshot already looked at for explosions, by its tick.
    seen_tick: Option<u32>,
    /// Where each missile and ship last left a puff.
    trail_ends: Vec<(SimId, [f64; 3])>,
}

impl Effects {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.puffs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.puffs.is_empty()
    }

    fn add(&mut self, puff: Puff) {
        if self.puffs.len() >= MAX_PUFFS {
            self.puffs.remove(0);
        }
        self.puffs.push(puff);
    }

    /// A blast of `size` across at `at`: a fireball, and smoke that rises and drifts off.
    fn explode(&mut self, at: [f64; 3], size: f32, time: f64, seed: u64) {
        let life = 0.25 + f64::from(size).sqrt() * 0.08;
        self.add(Puff { shape: Shape::Fireball, at, rise: 0.0, born: time, life, size, growth: 0.8, height_share: 1.0, color: FIRE_HOT, fade_to: FIRE_COOL });
        let clouds = if size < 4.0 { 1 } else { 3 };
        for index in 0..clouds {
            let angle = noise(seed ^ (index * 7 + 1)) * std::f64::consts::TAU;
            let reach = f64::from(size) * 0.35 * noise(seed ^ (index * 7 + 2));
            let spot = [at[0] + angle.cos() * reach, at[1] + angle.sin() * reach, at[2] + f64::from(size) * 0.2];
            let puff_life = life * 3.0 + noise(seed ^ (index * 7 + 3)) * life;
            self.add(Puff {
                shape: Shape::Smoke,
                at: spot,
                rise: f64::from(size) * 0.6,
                born: time + life * 0.3,
                life: puff_life,
                size: size * 0.7,
                growth: 1.2,
                height_share: 1.0,
                color: SMOKE_DARK,
                fade_to: SMOKE_PALE,
            });
        }
    }

    /// Starts what the latest snapshot shows has happened since the one before: shells and
    /// missiles that have hit, and units that have died. Call every frame; each snapshot is
    /// looked at once.
    pub fn note_events(&mut self, previous: &SimSnapshot, current: &SimSnapshot, time: f64) {
        if self.seen_tick == Some(current.tick) {
            return;
        }
        self.seen_tick = Some(current.tick);
        // A shell gone from the latest snapshot has hit, or run out, where it last was.
        merge_by_id(&current.projectiles, &previous.projectiles, |projectile| projectile.id, |still_there, projectile| {
            if still_there.is_some() {
                return;
            }
            let size = match projectile.kind {
                ProjectileKind::Shell => 2.5,
                ProjectileKind::Overcharge => 8.0,
                ProjectileKind::TacticalMissile => 14.0,
                ProjectileKind::StrategicMissile => 220.0,
            };
            self.explode(raw_point(projectile.x, projectile.y, projectile.z), size, time, u64::from(projectile.id.0));
        });
        // A unit that dies leaves a wreck where it stood. One that only goes out of sight does not.
        let mut gone: Vec<UnitView> = Vec::new();
        merge_by_id(&current.units, &previous.units, |unit| unit.id, |still_there, unit| {
            if still_there.is_none() {
                gone.push(*unit);
            }
        });
        if gone.is_empty() {
            return;
        }
        let mut blasts = Vec::new();
        merge_by_id(&previous.wrecks, &current.wrecks, |wreck| wreck.id, |before, wreck| {
            if before.is_some() || wreck.kind != WreckKind::Debris {
                return;
            }
            let near = |unit: &&UnitView| {
                let (dx, dy) = (f64::from(unit.x - wreck.x), f64::from(unit.y - wreck.y));
                dx.hypot(dy) <= f64::from(unit.radius + wreck.radius) * 1.5
            };
            if let Some(unit) = gone.iter().find(near) {
                blasts.push(*unit);
            }
        });
        for unit in blasts {
            let size = (f64::from(unit.radius) * RAW_TO_WORLD * 2.6) as f32;
            self.explode(raw_point(unit.x, unit.y, unit.z), size, time, u64::from(unit.id.0) << 1);
        }
    }

    /// Lays smoke behind missiles and foam behind ships, as far as each has moved since the last
    /// frame. `water` is the height of the water, if the map has any.
    pub fn trail(&mut self, previous: &SimSnapshot, current: &SimSnapshot, alpha: f64, looks: &[UnitLook], water: Option<f64>, time: f64) {
        let mut ends = std::mem::take(&mut self.trail_ends);
        let mut next_ends = Vec::with_capacity(ends.len());
        ends.sort_unstable_by_key(|end| end.0);
        let mut lay = |effects: &mut Self, id: SimId, at: [f64; 3], spacing: f64, mut puff: Puff| {
            let last = ends.binary_search_by_key(&id, |end| end.0).ok().map_or(at, |index| ends[index].1);
            let distance = (at[0] - last[0]).hypot(at[1] - last[1]).hypot(at[2] - last[2]);
            let mut end = last;
            // Too far means it was out of view, or new: start afresh.
            if distance > spacing * 40.0 {
                end = at;
            } else {
                let steps = (distance / spacing) as usize;
                for step in 1..=steps {
                    let share = step as f64 * spacing / distance;
                    puff.at = std::array::from_fn(|axis| last[axis] + (at[axis] - last[axis]) * share);
                    effects.add(puff);
                    end = puff.at;
                }
            }
            next_ends.push((id, end));
        };
        let interpolate = |from: [i32; 3], to: [i32; 3]| -> [f64; 3] {
            std::array::from_fn(|axis| (f64::from(from[axis]) + (f64::from(to[axis]) - f64::from(from[axis])) * alpha) * RAW_TO_WORLD)
        };
        merge_by_id(&previous.projectiles, &current.projectiles, |projectile| projectile.id, |before, projectile| {
            let size = match projectile.kind {
                ProjectileKind::TacticalMissile => 3.0,
                ProjectileKind::StrategicMissile => 7.0,
                _ => return,
            };
            let now = [projectile.x, projectile.y, projectile.z];
            let at = interpolate(before.map_or(now, |before| [before.x, before.y, before.z]), now);
            let puff = Puff { shape: Shape::Smoke, at, rise: 1.5, born: time, life: 1.6, size, growth: 1.4, height_share: 1.0, color: TRAIL_FRESH, fade_to: TRAIL_OLD };
            lay(self, projectile.id, at, f64::from(size) * TRAIL_SPACING, puff);
        });
        if let Some(water) = water {
            merge_by_id(&previous.units, &current.units, |unit| unit.id, |before, unit| {
                let Some(before) = before else { return };
                if before.x == unit.x && before.y == unit.y || !looks.get(usize::from(unit.blueprint)).is_some_and(|look| look.shape.leaves_wake()) {
                    return;
                }
                let radius = f64::from(unit.radius) * RAW_TO_WORLD;
                let center = interpolate([before.x, before.y, before.z], [unit.x, unit.y, unit.z]);
                // Only on the surface: a submarine below leaves none.
                if center[2] - radius > water + 1.0 || center[2] < water {
                    return;
                }
                let (dx, dy) = (f64::from(unit.x - before.x), f64::from(unit.y - before.y));
                let length = dx.hypot(dy);
                let stern = [center[0] - dx / length * radius * 0.9, center[1] - dy / length * radius * 0.9, water + 0.3];
                let size = (radius * 0.7) as f32;
                let puff = Puff { shape: Shape::Wake, at: stern, rise: 0.0, born: time, life: 2.8, size, growth: 2.2, height_share: 0.08, color: FOAM, fade_to: FOAM_OLD };
                lay(self, unit.id, stern, radius * 2.0 * WAKE_SPACING, puff);
            });
        }
        self.trail_ends = next_ends;
    }

    /// Forgets puffs that have gone, and draws the rest.
    pub fn build(&mut self, time: f64, eye: [f64; 3], out: &mut InstanceBatches) {
        // A clock that went backwards is a new game: start clean.
        self.puffs.retain(|puff| time - puff.born < puff.life && puff.born - time < 10.0);
        for puff in &self.puffs {
            let Some((at, size, color)) = puff.at_time(time) else { continue };
            if size <= 0.0 {
                continue;
            }
            let instance = painted(at, [size, size, size * puff.height_share], [0.0, 1.0], color, color, color, 0.0, eye);
            out.push(puff.shape, instance);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::components::Team;
    use crate::sim::world::{ProjectileView, WreckView};

    fn snapshot(tick: u32) -> SimSnapshot {
        SimSnapshot { tick, ..SimSnapshot::default() }
    }

    fn shell(id: u32, kind: ProjectileKind) -> ProjectileView {
        ProjectileView { id: SimId(id), team: Team::Blue, x: 4096 * 100, y: 0, z: 4096 * 5, kind, source: SimId(1) }
    }

    fn unit(id: u32) -> UnitView {
        UnitView {
            id: SimId(id),
            team: Team::Red,
            x: 4096 * 50,
            y: 4096 * 50,
            z: 4096 * 3,
            radius: 4096 * 3,
            blueprint: u16::MAX,
            health_permille: 100,
            built_permille: 1000,
            is_structure: false,
            facing: [0, 0],
            aim: [0, 0],
            working: false,
        }
    }

    fn count(effects: &mut Effects, time: f64, shape: Shape) -> usize {
        let mut out = InstanceBatches::default();
        effects.build(time, [0.0; 3], &mut out);
        out.shape(shape).len()
    }

    #[test]
    fn a_shell_that_hits_bursts_into_fire_once() {
        let mut before = snapshot(1);
        before.projectiles = vec![shell(5, ProjectileKind::Shell)];
        let after = snapshot(2);
        let mut effects = Effects::new();
        effects.note_events(&before, &after, 0.0);
        effects.note_events(&before, &after, 0.01);
        assert_eq!(count(&mut effects, 0.01, Shape::Fireball), 1, "one hit, however many frames show it");
        assert_eq!(count(&mut effects, 5.0, Shape::Fireball), 0, "and it burns out");
        assert!(effects.is_empty());
    }

    #[test]
    fn a_unit_that_dies_explodes_but_one_out_of_sight_does_not() {
        let mut before = snapshot(1);
        before.units = vec![unit(1), unit(2)];
        let mut after = snapshot(2);
        // Unit 1 left a wreck where it stood. Unit 2 only went out of sight.
        after.wrecks = vec![WreckView { id: SimId(9), x: 4096 * 50, y: 4096 * 50, z: 0, radius: 4096 * 3, mass: 1, energy: 0, kind: WreckKind::Debris }];
        before.units[1].x = 4096 * 900;
        let mut effects = Effects::new();
        effects.note_events(&before, &after, 0.0);
        assert_eq!(count(&mut effects, 0.01, Shape::Fireball), 1);
    }

    #[test]
    fn missiles_leave_smoke_behind_them() {
        let mut before = snapshot(1);
        before.projectiles = vec![shell(5, ProjectileKind::TacticalMissile)];
        let mut after = snapshot(2);
        let mut moved = shell(5, ProjectileKind::TacticalMissile);
        moved.x += 4096 * 30;
        after.projectiles = vec![moved];
        let mut effects = Effects::new();
        effects.trail(&before, &after, 0.0, &[], None, 0.0);
        effects.trail(&before, &after, 1.0, &[], None, 0.1);
        // 30 units at a puff every 1.8.
        assert_eq!(count(&mut effects, 0.1, Shape::Smoke), 16);
    }

    #[test]
    fn puffs_grow_then_shrink_away() {
        let puff = Puff { shape: Shape::Smoke, at: [0.0; 3], rise: 1.0, born: 0.0, life: 1.0, size: 2.0, growth: 1.0, height_share: 1.0, color: SMOKE_DARK, fade_to: SMOKE_PALE };
        let size = |time: f64| puff.at_time(time).map(|(_, size, _)| size);
        assert!(size(0.5).expect("alive") > size(0.0).expect("alive"));
        assert!(size(0.99).expect("alive") < size(0.5).expect("alive"));
        assert_eq!(size(1.0), None);
        assert_eq!(puff.at_time(0.5).expect("alive").0[2], 0.5, "it rises");
    }
}
