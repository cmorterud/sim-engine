//! Explosions, missile trails, and ships' wakes: short-lived effects the renderer makes from the
//! snapshots alone. The simulation never sees them.
//!
//! Each is a puff that grows, changes color, and shrinks away over its life, drawn as an opaque
//! low-poly shape with the units. They follow the simulation's clock, so they stop while paused.

use std::collections::VecDeque;

use super::instances::{merge_by_id, painted, InstanceBatches, UnitLook, RAW_TO_WORLD};
use super::meshes::Shape;
use crate::sim::components::{SimId, WreckKind};
use crate::sim::world::{ProjectileKind, SimSnapshot, UnitView};

/// The most puffs kept at once. The oldest go first, so that a huge battle cannot slow the frame.
const MAX_PUFFS: usize = 6_000;
/// How far apart a missile's trail puffs are, as a share of the puff's size.
const TRAIL_SPACING: f64 = 0.6;
/// How opaque smoke is when fresh. It thins away as it ages.
const SMOKE_OPACITY: f32 = 0.55;
/// How far apart a ship's wake puffs are, as a share of the ship's size.
const WAKE_SPACING: f64 = 0.35;
/// How far apart a ship's bow spray is thrown, as a share of its radius, and the bit that
/// keeps where the spray last went apart from where the wake did, which share the ship's id.
const BOW_SPRAY_SPACING: f64 = 0.5;
const BOW_SPRAY_ID: u32 = 1 << 31;

const FIRE_HOT: [f32; 3] = [1.0, 0.92, 0.55];
const FIRE_COOL: [f32; 3] = [0.85, 0.28, 0.05];
const SMOKE_DARK: [f32; 3] = [0.24, 0.23, 0.22];
const SMOKE_PALE: [f32; 3] = [0.45, 0.44, 0.42];
const TRAIL_FRESH: [f32; 3] = [0.92, 0.90, 0.86];
const TRAIL_OLD: [f32; 3] = [0.62, 0.62, 0.60];
const FOAM: [f32; 3] = [0.92, 0.96, 1.0];
const FOAM_OLD: [f32; 3] = [0.55, 0.68, 0.78];
/// The most craters and scorch marks kept at once, the oldest going first.
const MAX_SCARS: usize = 1_200;
/// How many of the newest scars a new one is checked against, to fold it into one it lands on.
const SCAR_NEIGHBOURS: usize = 200;
/// How long a scar lasts, in seconds, the last part of it fading away.
const SCAR_LIFE: f64 = 240.0;
const SCAR_FADE: f64 = 90.0;
/// How quickly a scar appears, under the fireball that makes it.
const SCAR_APPEAR: f64 = 0.3;
/// How high over the ground a blast can be and still mark it.
const SCAR_REACH: f64 = 4.0;
/// The most track marks kept at once, apart from the scars, so that driving never wipes out
/// the marks of a battle.
const MAX_TRACKS: usize = 2_000;
/// How long track marks last, in seconds, the last part of it fading away.
const TRACK_LIFE: f64 = 8.0;
const TRACK_FADE: f64 = 5.0;
/// How far apart a vehicle's track marks are laid, as a share of its width.
const TRACK_SPACING: f64 = 0.8;

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

/// How fast a fireball swells, and how wide it then gets at its widest, as a share of its size
/// at the start: (1 + 0.8 t)(1 - t³) is largest, 1.237, at t = 0.428.
const FIREBALL_GROWTH: f32 = 0.8;
const FIREBALL_PEAK: f32 = 1.237;

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

/// A mark a battle leaves on the ground: a crater where something landed, or a scorch mark
/// where a unit died. Drawn only: the ground itself never changes.
#[derive(Copy, Clone, Debug, PartialEq)]
struct Scar {
    at: [f64; 3],
    radius: f32,
    crater: bool,
    born: f64,
    /// From 0 to 1, varying its shape and which way it lies.
    seed: f32,
}

impl Scar {
    /// How strongly it shows at `time`, from 0 to 1.
    fn strength(&self, time: f64) -> f32 {
        let age = time - self.born;
        let appear = (age / SCAR_APPEAR).clamp(0.0, 1.0);
        let fade = ((SCAR_LIFE - age) / SCAR_FADE).clamp(0.0, 1.0);
        (appear * fade) as f32
    }
}

/// A stretch of the marks a tracked vehicle leaves behind it on the ground.
#[derive(Copy, Clone, Debug, PartialEq)]
struct Track {
    at: [f64; 3],
    width: f32,
    length: f32,
    /// Which way the vehicle was heading, as a unit vector.
    heading: [f32; 2],
    born: f64,
}

impl Track {
    /// How strongly it shows at `time`, from 0 to 1.
    fn strength(&self, time: f64) -> f32 {
        (((TRACK_LIFE - (time - self.born)) / TRACK_FADE).clamp(0.0, 1.0)) as f32
    }
}

/// What is going on, kept between frames.
#[derive(Default)]
pub struct Effects {
    // Queues, so that dropping the oldest when full costs nothing however many there are.
    puffs: VecDeque<Puff>,
    scars: VecDeque<Scar>,
    tracks: VecDeque<Track>,
    /// Which unit each wreck seen made was, and which way it faced, by wreck, sorted by wreck:
    /// wrecks are drawn as the burnt-out shell of what they were.
    wreck_origins: Vec<(SimId, u16, [f32; 2])>,
    /// Where each tracked vehicle last left a mark.
    track_ends: Vec<(SimId, [f64; 3])>,
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

    /// Leaves a mark on the ground at `at`. One that lands mostly on a recent mark of the same
    /// kind, at least as big, only freshens it: where shells keep landing, the ground is already
    /// scarred, and stacking marks there only costs drawing time.
    fn scar(&mut self, at: [f64; 3], radius: f32, crater: bool, time: f64, seed: u64) {
        let covered = self.scars.iter_mut().rev().take(SCAR_NEIGHBOURS).find(|scar| {
            scar.crater == crater && scar.radius >= radius * 0.8 && (scar.at[0] - at[0]).hypot(scar.at[1] - at[1]) < f64::from(scar.radius) * 0.6
        });
        if let Some(scar) = covered {
            scar.born = scar.born.max(time - SCAR_APPEAR);
            return;
        }
        if self.scars.len() >= MAX_SCARS {
            self.scars.pop_front();
        }
        self.scars.push_back(Scar { at, radius, crater, born: time, seed: noise(seed ^ 0x5ca7) as f32 });
    }

    fn add(&mut self, puff: Puff) {
        if self.puffs.len() >= MAX_PUFFS {
            self.puffs.pop_front();
        }
        self.puffs.push_back(puff);
    }

    /// A blast of `size` across at `at`: a fireball that swells to that width, and smoke that
    /// rises and drifts off.
    fn explode(&mut self, at: [f64; 3], size: f32, time: f64, seed: u64) {
        let life = 0.25 + f64::from(size).sqrt() * 0.08;
        self.add(Puff { shape: Shape::Fireball, at, rise: 0.0, born: time, life, size: size / FIREBALL_PEAK, growth: FIREBALL_GROWTH, height_share: 1.0, color: FIRE_HOT, fade_to: FIRE_COOL });
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

    /// Which unit a wreck was, as a raw blueprint id, and which way it faced, if its making was
    /// seen: wrecks already there, or made out of sight, are not known.
    pub fn wreck_origin(&self, wreck: SimId) -> Option<(u16, [f32; 2])> {
        let index = self.wreck_origins.binary_search_by_key(&wreck, |origin| origin.0).ok()?;
        let (_, blueprint, facing) = self.wreck_origins[index];
        Some((blueprint, facing))
    }

    /// Starts what the latest snapshot shows has happened since the one before: shells and
    /// missiles that have hit, and units that have died. Call every frame; each snapshot is
    /// looked at once. `ground` gives the ground's height at a point, so that what lands on it
    /// leaves a mark, and `water` the water's height, under which nothing is marked.
    pub fn note_events(&mut self, previous: &SimSnapshot, current: &SimSnapshot, time: f64, ground: &dyn Fn(f64, f64) -> f64, water: Option<f64>) {
        // The ground under a point, if a blast there would mark it.
        let marks = |at: [f64; 3], reach: f64| {
            let height = ground(at[0], at[1]);
            (at[2] - height <= reach && water.is_none_or(|water| height >= water)).then_some([at[0], at[1], height])
        };
        if self.seen_tick == Some(current.tick) {
            return;
        }
        self.seen_tick = Some(current.tick);
        // A shell gone from the latest snapshot has hit, or run out: where the simulation says it
        // struck, if the snapshot says, or else where it last was. It is drawn flying on to that
        // point (build_projectile_instances), so it bursts as the snapshot after it would be due.
        let arrival = (f64::from(current.tick) / crate::sim::blueprints::TICKS_PER_SECOND as f64).max(time);
        merge_by_id(&current.projectiles, &previous.projectiles, |projectile| projectile.id, |still_there, last_seen| {
            if still_there.is_some() {
                return;
            }
            let struck = current.landed.binary_search_by_key(&last_seen.id, |landed| landed.id).ok().map(|index| &current.landed[index]);
            let (projectile, time) = match struck {
                Some(struck) => (struck, arrival),
                None => (last_seen, time),
            };
            let least: f32 = match projectile.kind {
                ProjectileKind::Shell => 2.5,
                ProjectileKind::Discharge => 8.0,
                ProjectileKind::TacticalMissile => 14.0,
                ProjectileKind::StrategicMissile => 220.0,
            };
            // The fireball is as wide as the blast that does the damage, so that a bomb or a
            // heavy shell shows how far it reaches.
            let blast = (f64::from(projectile.splash) * RAW_TO_WORLD * 2.0) as f32;
            let size = least.max(blast);
            let at = raw_point(projectile.x, projectile.y, projectile.z);
            self.explode(at, size, time, u64::from(projectile.id.0));
            // Shells that land leave a crater, and a strategic missile scorches a wide circle.
            if let Some(spot) = marks(at, SCAR_REACH) {
                let strategic = projectile.kind == ProjectileKind::StrategicMissile;
                let radius = if strategic { size * 0.6 } else { (least * 1.4).max(blast * 0.55) };
                self.scar(spot, radius, !strategic, time, u64::from(projectile.id.0));
            }
        });
        // A unit that dies leaves a wreck where it stood. One that only goes out of sight does not,
        // nor a remembered structure found gone: it died out of sight, long since.
        let mut gone: Vec<UnitView> = Vec::new();
        merge_by_id(&current.units, &previous.units, |unit| unit.id, |still_there, unit| {
            if still_there.is_none() && !unit.remembered {
                gone.push(*unit);
            }
        });
        // Wrecks that are gone, reclaimed or burnt away, are forgotten.
        self.wreck_origins.retain(|(wreck, _, _)| current.wrecks.binary_search_by_key(wreck, |seen| seen.id).is_ok());
        // Units that blow up as they are destroyed, as wide as the damage they do.
        for (index, blast) in current.blasts.iter().enumerate() {
            let (x, y) = (f64::from(blast.x) * RAW_TO_WORLD, f64::from(blast.y) * RAW_TO_WORLD);
            let height = ground(x, y);
            let at = [x, y, water.map_or(height, |water| height.max(water))];
            let size = (f64::from(blast.radius) * RAW_TO_WORLD * 2.0) as f32;
            let seed = (u64::from(current.tick) << 20) ^ index as u64 ^ 0x5eed;
            self.explode(at, size, time, seed);
            if let Some(spot) = marks(at, SCAR_REACH) {
                self.scar(spot, size * 0.55, false, time, seed);
            }
        }
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
                let facing = [unit.facing[0] as f32, unit.facing[1] as f32];
                let length = facing[0].hypot(facing[1]);
                let facing = if length > 0.0 { [facing[0] / length, facing[1] / length] } else { [0.0, 1.0] };
                if let Err(at) = self.wreck_origins.binary_search_by_key(&wreck.id, |origin| origin.0) {
                    self.wreck_origins.insert(at, (wreck.id, unit.blueprint, facing));
                }
            }
        });
        for unit in blasts {
            let radius = f64::from(unit.radius) * RAW_TO_WORLD;
            let size = (radius * 2.6) as f32;
            let at = raw_point(unit.x, unit.y, unit.z);
            self.explode(at, size, time, u64::from(unit.id.0) << 1);
            // Units that die on the ground scorch it.
            if let Some(spot) = marks(at, radius * 2.0 + SCAR_REACH) {
                self.scar(spot, (radius * 1.6) as f32, false, time, u64::from(unit.id.0) << 1);
            }
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
                // And spray thrown up at the bow, brief and small, kept apart from the wake.
                let bow = [center[0] + dx / length * radius * 0.85, center[1] + dy / length * radius * 0.85, water + 0.6];
                let spray = Puff { shape: Shape::Wake, at: bow, rise: 1.5, born: time, life: 0.7, size: size * 0.45, growth: 1.6, height_share: 0.3, color: FOAM, fade_to: FOAM };
                lay(self, SimId(unit.id.0 ^ BOW_SPRAY_ID), bow, radius * BOW_SPRAY_SPACING, spray);
            });
        }
        self.trail_ends = next_ends;
        self.lay_tracks(previous, current, alpha, looks, water, time);
    }

    /// Lays track marks behind tracked vehicles on the ground, as far as each has moved since
    /// the last frame.
    fn lay_tracks(&mut self, previous: &SimSnapshot, current: &SimSnapshot, alpha: f64, looks: &[UnitLook], water: Option<f64>, time: f64) {
        let mut ends = std::mem::take(&mut self.track_ends);
        ends.sort_unstable_by_key(|end| end.0);
        let mut next_ends = Vec::with_capacity(ends.len());
        merge_by_id(&previous.units, &current.units, |unit| unit.id, |before, unit| {
            let Some(before) = before else { return };
            let tracked = looks.get(usize::from(unit.blueprint)).is_some_and(|look| look.base.is_some_and(Shape::leaves_tracks) || look.shape.leaves_tracks());
            if !tracked || unit.built_permille < 1000 {
                return;
            }
            let at: [f64; 3] = std::array::from_fn(|axis| {
                let (from, to) = ([before.x, before.y, before.z][axis], [unit.x, unit.y, unit.z][axis]);
                (f64::from(from) + (f64::from(to) - f64::from(from)) * alpha) * RAW_TO_WORLD
            });
            if water.is_some_and(|water| at[2] < water) {
                return;
            }
            let width = f64::from(unit.radius) * RAW_TO_WORLD * 1.6;
            let spacing = width * TRACK_SPACING;
            let last = ends.binary_search_by_key(&unit.id, |end| end.0).ok().map_or(at, |index| ends[index].1);
            let (dx, dy) = (at[0] - last[0], at[1] - last[1]);
            let distance = dx.hypot(dy);
            let mut end = last;
            if distance > spacing * 40.0 {
                // Out of view, or new: start afresh.
                end = at;
            } else if distance >= spacing {
                let heading = [(dx / distance) as f32, (dy / distance) as f32];
                let steps = (distance / spacing) as usize;
                for step in 1..=steps {
                    let share = step as f64 * spacing / distance;
                    let spot: [f64; 3] = std::array::from_fn(|axis| last[axis] + (at[axis] - last[axis]) * share);
                    if self.tracks.len() >= MAX_TRACKS {
                        self.tracks.pop_front();
                    }
                    // Each stretch reaches back over the one before, so that they join.
                    self.tracks.push_back(Track { at: spot, width: width as f32, length: (spacing * 1.7) as f32, heading, born: time });
                    end = spot;
                }
            }
            next_ends.push((unit.id, end));
        });
        self.track_ends = next_ends;
    }

    /// Forgets puffs and scars that have gone, and draws the rest.
    pub fn build(&mut self, time: f64, eye: [f64; 3], out: &mut InstanceBatches) {
        // A clock that went backwards is a new game: start clean.
        self.puffs.retain(|puff| time - puff.born < puff.life && puff.born - time < 10.0);
        self.scars.retain(|scar| time - scar.born < SCAR_LIFE && scar.born - time < 10.0);
        self.tracks.retain(|track| time - track.born < TRACK_LIFE && track.born - time < 10.0);
        for track in &self.tracks {
            // The shader reads a scar's look from its color: tracks are its third kind.
            let look = [2.0, track.strength(time), 0.0];
            out.push(Shape::Scar, painted(track.at, [track.width, track.length, 1.0], track.heading, look, look, look, 0.0, eye));
        }
        for scar in &self.scars {
            let strength = scar.strength(time);
            if strength <= 0.0 {
                continue;
            }
            // The shader reads a scar's look from its color: crater or not, strength, and seed.
            let look = [if scar.crater { 1.0 } else { 0.0 }, strength, scar.seed];
            let angle = scar.seed * std::f32::consts::TAU;
            let width = scar.radius * 2.0;
            out.push(Shape::Scar, painted(scar.at, [width, width, 1.0], [angle.cos(), angle.sin()], look, look, look, 0.0, eye));
        }
        for puff in &self.puffs {
            let Some((at, size, color)) = puff.at_time(time) else { continue };
            if size <= 0.0 {
                continue;
            }
            // Each puff lies its own way, stretched and shaded a little differently from the next,
            // from a number that depends only on where and when it began.
            let seed = puff.at[0].to_bits() ^ puff.at[1].to_bits().rotate_left(21) ^ puff.born.to_bits().rotate_left(42);
            let vary = |index: u64| noise(seed ^ index.wrapping_mul(0x9e37_79b9)) as f32;
            let angle = vary(1) * std::f32::consts::TAU;
            let stretch = [0.75 + 0.5 * vary(2), 0.75 + 0.5 * vary(3), 0.65 + 0.45 * vary(4)];
            let shade = 0.88 + 0.24 * vary(5);
            let color = color.map(|channel| (channel * shade).min(1.0));
            let mut instance = painted(at, [size * stretch[0], size * stretch[1], size * puff.height_share * stretch[2]], [angle.cos(), angle.sin()], color, color, color, 0.0, eye);
            // Smoke is see-through, and thins as it ages. The shader reads how opaque it is from
            // the hull color's last byte.
            if puff.shape == Shape::Smoke {
                let age = ((time - puff.born) / puff.life) as f32;
                instance.hull[3] = (SMOKE_OPACITY * (1.0 - age.powf(1.5)) * 255.0).round().clamp(0.0, 255.0) as u8;
            }
            out.push(puff.shape, instance);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::components::Team;
    use crate::render::instances::UnitInstanceRaw;
    use crate::sim::world::{ProjectileView, WreckView};

    fn snapshot(tick: u32) -> SimSnapshot {
        SimSnapshot { tick, ..SimSnapshot::default() }
    }

    fn shell(id: u32, kind: ProjectileKind) -> ProjectileView {
        ProjectileView { id: SimId(id), team: Team::Blue, x: 4096 * 100, y: 0, z: 4096 * 5, kind, source: SimId(1), from: None, splash: 0 }
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
            aim: [0, 0], extra_aims: [[0; 2]; 3],
            working: false,
            assembling: None,
            upgrade: None,
            remembered: false,
            ally: false,
        }
    }

    fn count(effects: &mut Effects, time: f64, shape: Shape) -> usize {
        let mut out = InstanceBatches::default();
        effects.build(time, [0.0; 3], &mut out);
        out.shape(shape).len()
    }

    /// A shell's last position is a tick's flight short of what it hits: its burst used to be
    /// drawn there, so that every shell looked to land short. It bursts where the simulation says
    /// it struck, as the shell, drawn flying on, gets there.
    #[test]
    fn a_shell_bursts_where_it_struck_when_it_gets_there() {
        let mut before = snapshot(1);
        before.projectiles = vec![shell(5, ProjectileKind::Shell)];
        let mut after = snapshot(2);
        let mut struck = shell(5, ProjectileKind::Shell);
        struck.x += 4096 * 55;
        after.landed = vec![struck];
        let mut effects = Effects::new();
        // At tick 1's time, the shell is still on its way to tick 2.
        effects.note_events(&before, &after, 0.1, &|_, _| 0.0, None);
        assert_eq!(count(&mut effects, 0.15, Shape::Fireball), 0, "not before it gets there");
        let mut out = InstanceBatches::default();
        effects.build(0.21, [0.0; 3], &mut out);
        let fireballs = out.shape(Shape::Fireball);
        assert_eq!(fireballs.len(), 1);
        assert_eq!(fireballs[0].model_matrix[3][0], 155.0, "where it struck, not 55 short");
        let mut drawn = InstanceBatches::default();
        crate::render::instances::build_projectile_instances(&before.projectiles, &after.projectiles, &after.landed, 0.5, [0.0; 3], &mut drawn);
        let shells = drawn.shape(Shape::Shell);
        assert_eq!(shells.len(), 1, "drawn on its way to where it struck");
        assert!((shells[0].model_matrix[3][0] - 127.5).abs() < 0.01, "halfway there");
    }

    #[test]
    fn a_shell_that_hits_bursts_into_fire_once() {
        let mut before = snapshot(1);
        before.projectiles = vec![shell(5, ProjectileKind::Shell)];
        let after = snapshot(2);
        let mut effects = Effects::new();
        effects.note_events(&before, &after, 0.0, &|_, _| 0.0, None);
        effects.note_events(&before, &after, 0.01, &|_, _| 0.0, None);
        assert_eq!(count(&mut effects, 0.01, Shape::Fireball), 1, "one hit, however many frames show it");
        assert_eq!(count(&mut effects, 5.0, Shape::Fireball), 0, "and it burns out");
        assert!(effects.is_empty());
    }

    #[test]
    fn a_blast_is_as_wide_as_the_damage_it_does() {
        let mut before = snapshot(1);
        let mut bomb = shell(5, ProjectileKind::Shell);
        bomb.splash = 4096 * 12;
        bomb.z = 4096;
        before.projectiles = vec![bomb];
        let mut effects = Effects::new();
        effects.note_events(&before, &snapshot(2), 0.0, &|_, _| 0.0, None);
        let mut out = InstanceBatches::default();
        effects.build(0.0, [0.0; 3], &mut out);
        let fireball = out.shape(Shape::Fireball)[0];
        let width = fireball.model_matrix[0][0].hypot(fireball.model_matrix[0][1]);
        assert!(width >= 20.0, "a blast reaching 12 is about 24 across, not {width}");
        let crater = scars(&mut effects, 1.0)[0];
        assert!(crater.model_matrix[0][0].hypot(crater.model_matrix[0][1]) >= 24.0, "and leaves a crater as wide");
    }

    /// How wide the fireball gets at its widest, across the ground, with `effects` holding one.
    fn widest_fireball(effects: &mut Effects) -> f32 {
        (0..400)
            .map(|step| {
                let mut out = InstanceBatches::default();
                effects.build(f64::from(step) * 0.005, [0.0; 3], &mut out);
                out.shape(Shape::Fireball).first().map_or(0.0, |fireball| {
                    let across = |row: usize| fireball.model_matrix[row][0].hypot(fireball.model_matrix[row][1]);
                    (across(0) + across(1)) / 2.0
                })
            })
            .fold(0.0, f32::max)
    }

    /// Every blast, of every weapon of every unit and of every unit that blows up as it is
    /// destroyed, swells to about as wide as the damage it does, and no wider.
    #[test]
    fn every_explosion_is_as_wide_as_its_blast() {
        let db = crate::sim::blueprints::BlueprintDb::standard();
        let mut checked = 0;
        for def in db.iter() {
            let splashes = def.weapon.iter().chain(&def.extra_weapons).map(|weapon| (weapon.splash_radius, false));
            let discharge = def.discharge.iter().map(|discharge| (discharge.splash_radius, false));
            let death = def.death_explosion.iter().map(|blast| (blast.radius, true));
            for (radius, dies) in splashes.chain(discharge).chain(death).filter(|(radius, _)| *radius > crate::math::fp::Fp::ZERO) {
                let mut effects = Effects::new();
                let before = snapshot(1);
                let mut after = snapshot(2);
                if dies {
                    after.blasts = vec![crate::sim::world::BlastView { team: Team::Red, x: 0, y: 0, radius: radius.to_bits() }];
                    effects.note_events(&before, &after, 0.0, &|_, _| 0.0, None);
                } else {
                    let mut before = before;
                    let mut bomb = shell(5, ProjectileKind::Shell);
                    bomb.splash = radius.to_bits();
                    before.projectiles = vec![bomb];
                    effects.note_events(&before, &after, 0.0, &|_, _| 0.0, None);
                }
                let across = radius.to_num::<f32>() * 2.0;
                let width = widest_fireball(&mut effects);
                assert!(width >= across * 0.7 && width <= across.max(2.5) * 1.3, "{}: a blast reaching {} swells to {width} across, not about {across}", def.key, radius.to_num::<f32>());
                checked += 1;
            }
        }
        assert!(checked > 20, "only {checked} blasts");
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
        effects.note_events(&before, &after, 0.0, &|_, _| 0.0, None);
        assert_eq!(count(&mut effects, 0.01, Shape::Fireball), 1);
    }

    /// A wreck seen being made remembers the unit it was and which way it faced, until the
    /// wreck is gone; one that only appears is not known.
    #[test]
    fn a_wreck_remembers_what_it_was_until_it_is_gone() {
        let mut before = snapshot(1);
        before.units = vec![unit(1)];
        before.units[0].blueprint = 7;
        before.units[0].facing = [4096, 0];
        let wreck = WreckView { id: SimId(9), x: 4096 * 50, y: 4096 * 50, z: 0, radius: 4096 * 3, mass: 1, energy: 0, kind: WreckKind::Debris };
        let stranger = WreckView { id: SimId(10), x: 4096 * 900, ..wreck };
        let mut after = snapshot(2);
        after.wrecks = vec![wreck, stranger];
        let mut effects = Effects::new();
        effects.note_events(&before, &after, 0.0, &|_, _| 0.0, None);
        assert_eq!(effects.wreck_origin(SimId(9)), Some((7, [1.0, 0.0])));
        assert_eq!(effects.wreck_origin(SimId(10)), None, "no unit died there");
        let mut later = snapshot(3);
        later.wrecks = vec![stranger];
        effects.note_events(&after, &later, 1.0, &|_, _| 0.0, None);
        assert_eq!(effects.wreck_origin(SimId(9)), None, "reclaimed");
    }

    fn scars(effects: &mut Effects, time: f64) -> Vec<UnitInstanceRaw> {
        let mut out = InstanceBatches::default();
        effects.build(time, [0.0; 3], &mut out);
        out.shape(Shape::Scar).to_vec()
    }

    #[test]
    fn shells_that_land_leave_craters_that_fade_but_bursts_overhead_do_not() {
        let mut before = snapshot(1);
        let mut high = shell(6, ProjectileKind::Shell);
        high.z = 4096 * 40;
        before.projectiles = vec![shell(5, ProjectileKind::Shell), high];
        let after = snapshot(2);
        let mut effects = Effects::new();
        effects.note_events(&before, &after, 0.0, &|_, _| 3.0, None);
        let marks = scars(&mut effects, 1.0);
        assert_eq!(marks.len(), 1, "only the shell near the ground marks it");
        assert_eq!(marks[0].color[0], 1.0, "a crater");
        assert_eq!(marks[0].model_matrix[3][2], 3.0, "lying on the ground");
        assert!(scars(&mut effects, SCAR_LIFE - SCAR_FADE * 0.5)[0].color[1] < 0.6, "fading");
        assert!(scars(&mut effects, SCAR_LIFE + 1.0).is_empty(), "gone");
    }

    #[test]
    fn shells_landing_on_a_crater_freshen_it_instead_of_stacking() {
        let mut before = snapshot(1);
        let mut beside = shell(6, ProjectileKind::Shell);
        beside.x += 4096;
        before.projectiles = vec![shell(5, ProjectileKind::Shell), beside];
        let mut effects = Effects::new();
        effects.note_events(&before, &snapshot(2), 0.0, &|_, _| 3.0, None);
        assert_eq!(scars(&mut effects, 1.0).len(), 1);
    }

    #[test]
    fn nothing_is_marked_under_water() {
        let mut before = snapshot(1);
        before.projectiles = vec![shell(5, ProjectileKind::Shell)];
        let mut effects = Effects::new();
        effects.note_events(&before, &snapshot(2), 0.0, &|_, _| -10.0, Some(0.0));
        assert!(scars(&mut effects, 1.0).is_empty());
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

    /// A tracked vehicle on the move leaves marks behind it that fade; one standing still, or
    /// a walker, leaves none.
    #[test]
    fn tracked_vehicles_leave_marks_that_fade() {
        use crate::sim::blueprints::BlueprintDb;
        let db = BlueprintDb::standard();
        let looks = crate::render::instances::unit_looks(&db);
        let marks = |key: &str, moves: i32| {
            let mut effects = Effects::new();
            let mut start = unit(1);
            start.blueprint = db.id(key).0;
            let mut before = snapshot(1);
            before.units = vec![start];
            let mut after = snapshot(2);
            let mut moved = start;
            moved.x += 4096 * moves;
            after.units = vec![moved];
            effects.trail(&before, &before, 0.0, &looks, None, 0.0);
            effects.trail(&before, &after, 1.0, &looks, None, 0.1);
            let scars = |effects: &mut Effects, time: f64| {
                let mut out = InstanceBatches::default();
                effects.build(time, [0.0; 3], &mut out);
                out.shape(Shape::Scar).iter().filter(|instance| instance.color[0] > 1.5).map(|instance| instance.color[1]).collect::<Vec<f32>>()
            };
            (scars(&mut effects, 0.2), scars(&mut effects, TRACK_LIFE - TRACK_FADE * 0.5), scars(&mut effects, TRACK_LIFE + 1.0))
        };
        let (fresh, fading, gone) = marks("t2_heavy_tank", 30);
        assert!(fresh.len() >= 2 && fresh.iter().all(|strength| *strength > 0.99), "fresh marks behind a tank: {fresh:?}");
        assert!(fading.iter().all(|strength| *strength < 0.6), "fading: {fading:?}");
        assert!(gone.is_empty(), "gone");
        assert!(marks("t2_heavy_tank", 0).0.is_empty(), "a tank standing still leaves none");
        assert!(marks("t1_light_walker", 30).0.is_empty(), "a walker leaves none");
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
