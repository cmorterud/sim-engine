//! Explosions, missile trails, and ships' wakes: short-lived effects the renderer makes from the
//! snapshots alone. The simulation never sees them.
//!
//! Each is a puff that grows, changes color, and shrinks away over its life, drawn as an opaque
//! low-poly shape with the units. They follow the simulation's clock, so they stop while paused.

use std::collections::VecDeque;

use super::instances::{merge_by_id, painted, InstanceBatches, ShotLook, ShotStyle, UnitLook, RAW_TO_WORLD};
use super::meshes::Shape;
use crate::sim::components::{SimId, WreckKind};
use crate::sim::world::{ProjectileKind, ProjectileView, SimSnapshot, UnitView};

/// The most puffs kept at once. The oldest go first, so that a huge battle cannot slow the frame.
const MAX_PUFFS: usize = 6_000;
/// How far apart a missile's trail puffs are, as a share of the puff's size.
const TRAIL_SPACING: f64 = 0.6;
/// How opaque smoke is when fresh. It thins away as it ages.
const SMOKE_OPACITY: f32 = 0.55;
/// How opaque dense smoke is, a nuclear cloud's: nearly solid, until near the end of its life.
const DENSE_SMOKE_OPACITY: f32 = 0.92;
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
/// The white of a blast's first instant, before it cools to fire.
const FLASH_WHITE: [f32; 3] = [1.0, 0.98, 0.9];
/// A blast's lobes of fire: orange, cooling to a dull red.
const LOBE_HOT: [f32; 3] = [1.0, 0.66, 0.22];
const LOBE_COOL: [f32; 3] = [0.62, 0.14, 0.04];
/// How wide a torpedo's column of water is thrown, in units.
const TORPEDO_SPOUT: f32 = 16.0;
/// Fireballs this wide or wider billow in lobes about a white-hot core; smaller ones are one ball.
const LOBED_FIREBALL: f32 = 4.0;
/// Earth thrown up by a shell, and the dust it settles into.
const DIRT: [f32; 3] = [0.36, 0.28, 0.19];
const DUST: [f32; 3] = [0.58, 0.50, 0.38];
/// A flak burst's smoke.
const FLAK_BLACK: [f32; 3] = [0.10, 0.10, 0.11];
/// A nuclear blast's stem, dust carried up from the ground.
const STEM: [f32; 3] = [0.55, 0.46, 0.38];
/// Bits that keep a missile's exhaust, and a shell's smoke, apart from other trails by id.
const EXHAUST_ID: u32 = 1 << 30;
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
    /// Smoke that stays thick most of its life and thins only at the end, as a nuclear cloud
    /// does, rather than thinning all the way, as a shell's smoke does.
    dense: bool,
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
    /// Laser beams lately fired, each from its muzzle to where it struck.
    beams: VecDeque<LaserBeam>,
    /// What each shell in flight, or just landed, looks like, by projectile, sorted: worked out
    /// once, when it is first seen, from the weapon that fired it (`ShotStyle::resolve`).
    shots: Vec<(SimId, ShotStyle)>,
}

/// A laser's beam, which the simulation fires and lands within one tick: drawn for a moment
/// after, thinning out, so that it is seen.
#[derive(Copy, Clone, Debug)]
struct LaserBeam {
    from: [f64; 3],
    to: [f64; 3],
    born: f64,
    color: [f32; 3],
}

/// How long a laser's beam shows, in seconds, how wide it starts, in units, and how far its
/// team's color is lifted towards white.
const LASER_LIFE: f64 = 0.28;
const LASER_WIDTH: f32 = 1.6;
const LASER_WHITEN: f32 = 0.55;
const MAX_BEAMS: usize = 512;

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
        let life = self.fireball(at, size, time, seed);
        self.smoke(at, size, life, if size < 4.0 { 1 } else { 3 }, time, seed);
    }

    /// Fire that swells to `size` across, and how long it burns. A big blast billows: a white-hot
    /// core, soon gone, and four lobes of orange fire about it, rising a little as they cool,
    /// which together reach as wide as the blast; it was once one pale ball, which from above
    /// was a flat yellow disc. A small one is a single ball.
    fn fireball(&mut self, at: [f64; 3], size: f32, time: f64, seed: u64) -> f64 {
        let life = 0.25 + f64::from(size).sqrt() * 0.08;
        if size < LOBED_FIREBALL {
            self.add(Puff { shape: Shape::Fireball, at, rise: 0.0, born: time, life, size: size / FIREBALL_PEAK, growth: FIREBALL_GROWTH, height_share: 1.0, color: FIRE_HOT, fade_to: FIRE_COOL, dense: false });
            return life;
        }
        self.add(Puff { shape: Shape::Fireball, at, rise: 0.0, born: time, life: life * 0.6, size: size * 0.5 / FIREBALL_PEAK, growth: FIREBALL_GROWTH, height_share: 1.0, color: FLASH_WHITE, fade_to: FIRE_HOT, dense: false });
        for index in 0..4 {
            let angle = (index as f64 + noise(seed ^ (index * 17 + 1)) * 0.6) / 4.0 * std::f64::consts::TAU;
            let reach = f64::from(size) * 0.22 * (0.7 + 0.6 * noise(seed ^ (index * 17 + 2)));
            let spot = [at[0] + angle.cos() * reach, at[1] + angle.sin() * reach, at[2] + f64::from(size) * 0.08 * noise(seed ^ (index * 17 + 3))];
            let lobe_life = life * (0.8 + 0.4 * noise(seed ^ (index * 17 + 4)));
            self.add(Puff {
                shape: Shape::Fireball,
                at: spot,
                rise: f64::from(size) * 0.15,
                born: time + 0.03 * index as f64,
                life: lobe_life,
                size: size * 0.55 / FIREBALL_PEAK,
                growth: FIREBALL_GROWTH,
                height_share: 1.0,
                color: LOBE_HOT,
                fade_to: LOBE_COOL,
                dense: false,
            });
        }
        life
    }

    /// A column thrown up where a shell lands: earth on the ground, spray on the water.
    fn fountain(&mut self, at: [f64; 3], size: f32, columns: u64, on_water: bool, time: f64, seed: u64) {
        let (color, fade_to) = if on_water { (FOAM, FOAM_OLD) } else { (DIRT, DUST) };
        for index in 0..columns {
            let angle = noise(seed ^ (index * 11 + 5)) * std::f64::consts::TAU;
            let reach = f64::from(size) * 0.2 * noise(seed ^ (index * 11 + 6));
            let spot = [at[0] + angle.cos() * reach, at[1] + angle.sin() * reach, at[2]];
            let rise = f64::from(size) * (1.4 + 0.6 * noise(seed ^ (index * 11 + 7)));
            let life = 0.7 + 0.4 * noise(seed ^ (index * 11 + 8));
            self.add(Puff { shape: Shape::Smoke, at: spot, rise, born: time, life, size: size * 0.32, growth: 0.5, height_share: 1.6, color, fade_to, dense: false });
        }
    }

    /// A puff of dust, or of spray on the water, kicked up where a small round lands.
    fn kick_up(&mut self, at: [f64; 3], size: f32, on_water: bool, time: f64) {
        let (color, fade_to) = if on_water { (FOAM, FOAM_OLD) } else { (DUST, SMOKE_PALE) };
        self.add(Puff { shape: Shape::Smoke, at, rise: f64::from(size) * 0.8, born: time, life: 0.6, size: size * 0.6, growth: 0.8, height_share: 1.0, color, fade_to, dense: false });
    }

    /// A ring of dust, or of foam, running out over the ground from a big blast.
    fn ring(&mut self, at: [f64; 3], size: f32, life: f64, on_water: bool, time: f64) {
        let (color, fade_to) = if on_water { (FOAM, FOAM_OLD) } else { (DUST, DUST) };
        self.add(Puff { shape: Shape::Wake, at: [at[0], at[1], at[2] + 0.3], rise: 0.0, born: time, life, size: size * 0.6, growth: 1.8, height_share: 0.05, color, fade_to, dense: false });
    }

    /// Where a shell lands, by what fired it: `size` across, as wide as its blast. A round with
    /// no blast is a spark and a puff of dust; artillery and bombs throw up earth, or spray, the
    /// heaviest with a ring of dust; a battleship's round is an artillery round with less of a
    /// bang; flak bursts into black smoke; a torpedo throws up a column of water.
    fn burst(&mut self, at: [f64; 3], style: ShotStyle, size: f32, on_water: bool, time: f64, seed: u64) {
        match style.look {
            ShotLook::Tracer => {
                self.fireball(at, size, time, seed);
                self.kick_up(at, size, on_water, time);
            }
            ShotLook::Cannon => {
                let life = self.fireball(at, size, time, seed);
                self.smoke(at, size, life, 1, time, seed);
                self.kick_up(at, size * 0.8, on_water, time);
            }
            ShotLook::Artillery | ShotLook::Bomb => {
                let life = self.fireball(at, size, time, seed);
                self.smoke(at, size, life, 2, time, seed);
                let heavy = style.look == ShotLook::Bomb || style.scale >= 1.3;
                self.fountain(at, size, if heavy { 4 } else { 3 }, on_water, time, seed);
                if heavy {
                    self.ring(at, size, 0.8, on_water, time);
                }
            }
            ShotLook::HeavyGun => {
                let life = self.fireball(at, size, time, seed);
                self.smoke(at, size, life, 1, time, seed);
                if on_water {
                    self.fountain(at, size, 2, true, time, seed);
                }
            }
            // A small flash, and black smoke as wide as the blast.
            ShotLook::Flak => {
                self.add(Puff { shape: Shape::Fireball, at, rise: 0.0, born: time, life: 0.15, size: size * 0.3, growth: 0.4, height_share: 1.0, color: FIRE_HOT, fade_to: FIRE_COOL, dense: false });
                for index in 0..3 {
                    let angle = noise(seed ^ (index * 13 + 1)) * std::f64::consts::TAU;
                    let reach = f64::from(size) * 0.25 * noise(seed ^ (index * 13 + 2));
                    let spot = [at[0] + angle.cos() * reach, at[1] + angle.sin() * reach, at[2] + reach * 0.3];
                    let life = 1.4 + 0.4 * noise(seed ^ (index * 13 + 3));
                    self.add(Puff { shape: Shape::Smoke, at: spot, rise: 0.6, born: time, life, size: size * 0.45, growth: 0.8, height_share: 1.0, color: FLAK_BLACK, fade_to: SMOKE_DARK, dense: false });
                }
            }
            // A column of water as tall as a ship, whatever the torpedo's blast.
            ShotLook::Torpedo => {
                let size = size.max(TORPEDO_SPOUT);
                self.fireball(at, size * 0.4, time, seed);
                self.fountain(at, size, 3, on_water, time, seed);
                if on_water {
                    self.add(Puff { shape: Shape::Smoke, at, rise: f64::from(size) * 1.5, born: time, life: 1.3, size: size * 0.5, growth: 0.6, height_share: 3.0, color: FOAM, fade_to: FOAM_OLD, dense: false });
                }
                self.ring(at, size, 1.2, on_water, time);
            }
        }
    }

    /// A tactical missile's blast: its fireball and smoke, and a ring of dust or foam.
    fn missile_burst(&mut self, at: [f64; 3], size: f32, on_water: bool, time: f64, seed: u64) {
        let life = self.fireball(at, size, time, seed);
        self.smoke(at, size, life, 3, time, seed);
        self.ring(at, size, 0.9, on_water, time);
    }

    /// A nuclear blast, `size` across: a white flash and fire as wide as the blast, a shock ring
    /// running out over the ground, a skirt of dust, and a mushroom cloud: a column of dust rising
    /// from the ground into a broad cap that billows out as it climbs, lit orange from below at
    /// first and greying over some ten seconds. Its smoke is dense, thinning only at the end.
    fn mushroom(&mut self, at: [f64; 3], size: f32, time: f64, seed: u64) {
        let across = f64::from(size);
        let dense = |shape, at, rise, born, life, size, growth, height_share, color, fade_to| Puff { shape, at, rise, born, life, size, growth, height_share, color, fade_to, dense: true };
        self.fireball(at, size, time, seed);
        self.add(Puff { shape: Shape::Fireball, at, rise: 0.0, born: time, life: 0.4, size: size * 0.6, growth: 0.5, height_share: 1.0, color: FLASH_WHITE, fade_to: FIRE_HOT, dense: false });
        self.add(Puff { shape: Shape::Wake, at: [at[0], at[1], at[2] + 0.5], rise: 0.0, born: time, life: 2.0, size: size * 0.5, growth: 3.0, height_share: 0.02, color: [0.85, 0.82, 0.75], fade_to: DUST, dense: false });
        // The skirt: dust rolling out low round the foot of the column.
        for index in 0..6 {
            let angle = (f64::from(index) + 0.5) / 6.0 * std::f64::consts::TAU;
            let spot = [at[0] + angle.cos() * across * 0.22, at[1] + angle.sin() * across * 0.22, at[2] + across * 0.04];
            self.add(dense(Shape::Smoke, spot, across * 0.01, time + 0.4, 8.0, size * 0.24, 0.6, 0.45, DUST, SMOKE_PALE));
        }
        // The column, from the ground to the cap, each puff a little after the one below it.
        let cap = at[2] + across * 0.75;
        for index in 0..8 {
            let height = across * (0.05 + 0.09 * f64::from(index));
            self.add(dense(Shape::Smoke, [at[0], at[1], at[2] + height], across * 0.03, time + 0.2 + 0.12 * f64::from(index), 12.0, size * 0.21, 0.4, 1.3, STEM, SMOKE_PALE));
        }
        // The fireball climbing ahead of the column into the cap.
        self.add(Puff { shape: Shape::Fireball, at: [at[0], at[1], at[2] + across * 0.15], rise: across * 0.18, born: time + 0.2, life: 3.5, size: size * 0.28, growth: 0.5, height_share: 1.0, color: FIRE_HOT, fade_to: FIRE_COOL, dense: false });
        // The cap: a ring of cloud rolling outward round its rim, and a dome over it.
        for index in 0..10 {
            let angle = f64::from(index) / 10.0 * std::f64::consts::TAU;
            let spot = [at[0] + angle.cos() * across * 0.22, at[1] + angle.sin() * across * 0.22, cap];
            self.add(dense(Shape::Smoke, spot, across * 0.04, time + 0.7, 12.0, size * 0.28, 0.8, 0.6, FIRE_COOL, SMOKE_PALE));
        }
        for index in 0..4 {
            let angle = (f64::from(index) + 0.5) / 4.0 * std::f64::consts::TAU;
            let spot = [at[0] + angle.cos() * across * 0.1, at[1] + angle.sin() * across * 0.1, cap + across * 0.08];
            self.add(dense(Shape::Smoke, spot, across * 0.04, time + 0.8, 12.5, size * 0.3, 0.7, 0.7, FIRE_COOL, SMOKE_PALE));
        }
    }

    /// Smoke after a fireball that burns for `life`: `clouds` of it, rising and drifting off.
    fn smoke(&mut self, at: [f64; 3], size: f32, life: f64, clouds: u64, time: f64, seed: u64) {
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
                dense: false,
            });
        }
    }

    /// What a shell looks like: as worked out when it was first seen, or, for one not yet seen
    /// (`note_events` comes first each frame), a guess from its blast.
    pub fn shot(&self, projectile: &ProjectileView) -> ShotStyle {
        self.shots.binary_search_by_key(&projectile.id, |shot| shot.0).map_or_else(|_| ShotStyle::guess(projectile, false), |index| self.shots[index].1)
    }

    /// Works out what each shell new in `current` looks like, from the weapons of the unit that
    /// fired it, if it is in sight.
    fn note_shots(&mut self, current: &SimSnapshot, water: Option<f64>, looks: &[UnitLook]) {
        let mut fresh: Vec<(SimId, ShotStyle)> = Vec::new();
        for projectile in current.projectiles.iter().chain(&current.landed) {
            if projectile.kind != ProjectileKind::Shell || self.shots.binary_search_by_key(&projectile.id, |shot| shot.0).is_ok() || fresh.iter().any(|shot| shot.0 == projectile.id) {
                continue;
            }
            let shooter = current.units.binary_search_by_key(&projectile.source, |unit| unit.id).ok().and_then(|index| looks.get(usize::from(current.units[index].blueprint)));
            let under_water = water.is_some_and(|water| f64::from(projectile.z) * RAW_TO_WORLD < water);
            fresh.push((projectile.id, ShotStyle::resolve(projectile, shooter, under_water)));
        }
        if !fresh.is_empty() {
            self.shots.extend(fresh);
            self.shots.sort_unstable_by_key(|shot| shot.0);
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
    /// leaves a mark, and `water` the water's height, under which nothing is marked. `looks`
    /// says where each unit's laser turrets are drawn, for drawing a beam from the right one.
    pub fn note_events(&mut self, previous: &SimSnapshot, current: &SimSnapshot, time: f64, ground: &dyn Fn(f64, f64) -> f64, water: Option<f64>, looks: &[UnitLook]) {
        // The ground under a point, if a blast there would mark it.
        let marks = |at: [f64; 3], reach: f64| {
            let height = ground(at[0], at[1]);
            (at[2] - height <= reach && water.is_none_or(|water| height >= water)).then_some([at[0], at[1], height])
        };
        if self.seen_tick == Some(current.tick) {
            return;
        }
        self.seen_tick = Some(current.tick);
        self.note_shots(current, water, looks);
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
                ProjectileKind::Shell | ProjectileKind::Beam => 2.5,
                ProjectileKind::Discharge => 8.0,
                ProjectileKind::TacticalMissile => 14.0,
                ProjectileKind::StrategicMissile => 220.0,
            };
            // The fireball is as wide as the blast that does the damage, so that a bomb or a
            // heavy shell shows how far it reaches.
            let blast = (f64::from(projectile.splash) * RAW_TO_WORLD * 2.0) as f32;
            let size = least.max(blast);
            let at = raw_point(projectile.x, projectile.y, projectile.z);
            let seed = u64::from(projectile.id.0);
            // On the water, or just over it, a shell throws up spray rather than earth.
            let on_water = water.is_some_and(|water| ground(at[0], at[1]) < water && at[2] <= water + SCAR_REACH);
            let style = self.shot(projectile);
            match projectile.kind {
                ProjectileKind::Shell => self.burst(at, style, size, on_water, time, seed),
                ProjectileKind::TacticalMissile => self.missile_burst(at, size, on_water, time, seed),
                ProjectileKind::StrategicMissile => self.mushroom(at, size, time, seed),
                ProjectileKind::Discharge | ProjectileKind::Beam => self.explode(at, size, time, seed),
            }
            // Shells that land leave a crater, and a strategic missile scorches a wide circle. Flak
            // bursts in the air, and a torpedo in the water.
            let marking = projectile.kind != ProjectileKind::Shell || !matches!(style.look, ShotLook::Flak | ShotLook::Torpedo);
            if let Some(spot) = marks(at, SCAR_REACH).filter(|_| marking) {
                let strategic = projectile.kind == ProjectileKind::StrategicMissile;
                let radius = if strategic { size * 0.6 } else { (least * 1.4).max(blast * 0.55) };
                self.scar(spot, radius, !strategic, time, seed);
            }
        });
        // A shell's look is kept while it flies, and for the frames that draw it on to where it
        // struck.
        let known = |projectiles: &[ProjectileView], id: SimId| projectiles.binary_search_by_key(&id, |projectile| projectile.id).is_ok();
        self.shots.retain(|shot| known(&current.projectiles, shot.0) || known(&current.landed, shot.0));
        // A laser's beam is fired and lands within a tick, so it is never seen in flight: it is
        // drawn from its muzzle to where it struck, with a flash there.
        for struck in current.landed.iter().filter(|struck| struck.kind == ProjectileKind::Beam) {
            let Some(from) = struck.from else { continue };
            let to = raw_point(struck.x, struck.y, struck.z);
            let team = match struck.team {
                crate::sim::components::Team::Blue => super::instances::BLUE,
                crate::sim::components::Team::Red => super::instances::RED,
            };
            if self.beams.len() >= MAX_BEAMS {
                self.beams.pop_front();
            }
            // From the lens of the turret that fired it, if its unit is in sight.
            let from = raw_point(from[0], from[1], from[2]);
            let shooter = current.units.binary_search_by_key(&struck.source, |unit| unit.id).ok().map(|index| &current.units[index]);
            let from = shooter.and_then(|unit| looks.get(usize::from(unit.blueprint)).map(|look| super::instances::laser_muzzle(look, unit, from, to))).unwrap_or(from);
            self.beams.push_back(LaserBeam { from, to, born: arrival, color: team.map(|channel| channel + (1.0 - channel) * LASER_WHITEN) });
            self.explode(to, 3.0, arrival, u64::from(struck.id.0));
        }
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
            let now = [projectile.x, projectile.y, projectile.z];
            let then = before.map_or(now, |before| [before.x, before.y, before.z]);
            let at = interpolate(then, now);
            // Its other trail, an exhaust or a shell's smoke, is kept apart from the first by id.
            let second = SimId(projectile.id.0 ^ EXHAUST_ID);
            match projectile.kind {
                ProjectileKind::TacticalMissile | ProjectileKind::StrategicMissile => {
                    let strategic = projectile.kind == ProjectileKind::StrategicMissile;
                    // A strategic missile's flame is larger, so that it glows (2026-11-25).
                    let (size, length, flame) = if strategic { (7.0, 20.0, 7.0) } else { (3.0, 9.0, 1.6) };
                    let puff = Puff { shape: Shape::Smoke, at, rise: 1.5, born: time, life: 1.6, size, growth: 1.4, height_share: 1.0, color: TRAIL_FRESH, fade_to: TRAIL_OLD, dense: false };
                    lay(self, projectile.id, at, f64::from(size) * TRAIL_SPACING, puff);
                    // A flame at its tail, brief, the way it has come.
                    let way: [f64; 3] = std::array::from_fn(|axis| f64::from(now[axis] - then[axis]));
                    let distance = way[0].hypot(way[1]).hypot(way[2]);
                    if distance > 0.0 {
                        let tail: [f64; 3] = std::array::from_fn(|axis| at[axis] - way[axis] / distance * length * 0.5);
                        let fire = Puff { shape: Shape::Fireball, at: tail, rise: 0.0, born: time, life: 0.06, size: flame, growth: 0.0, height_share: 1.0, color: FIRE_HOT, fade_to: FIRE_COOL, dense: false };
                        lay(self, second, tail, f64::from(flame) * 0.8, fire);
                    }
                }
                ProjectileKind::Shell => match self.shot(projectile) {
                    // Artillery rounds and bombs leave a thin wisp of smoke, so that they can be followed.
                    style @ ShotStyle { look: ShotLook::Artillery | ShotLook::HeavyGun | ShotLook::Bomb, .. } => {
                        let size = 1.6 * style.scale;
                        let wisp = Puff { shape: Shape::Smoke, at, rise: 0.5, born: time, life: 0.45, size, growth: 1.0, height_share: 1.0, color: TRAIL_FRESH, fade_to: TRAIL_OLD, dense: false };
                        lay(self, second, at, f64::from(size) * 5.0, wisp);
                    }
                    // A torpedo under the water shows by bubbles on the surface over it.
                    ShotStyle { look: ShotLook::Torpedo, .. } => {
                        if let Some(water) = water.filter(|water| at[2] < *water) {
                            let surface = [at[0], at[1], water + 0.2];
                            let bubbles = Puff { shape: Shape::Wake, at: surface, rise: 0.0, born: time, life: 1.4, size: 1.4, growth: 1.2, height_share: 0.2, color: FOAM, fade_to: FOAM_OLD, dense: false };
                            lay(self, second, surface, 4.0, bubbles);
                        }
                    }
                    _ => {}
                },
                ProjectileKind::Discharge | ProjectileKind::Beam => {}
            }
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
                let puff = Puff { shape: Shape::Wake, at: stern, rise: 0.0, born: time, life: 2.8, size, growth: 2.2, height_share: 0.08, color: FOAM, fade_to: FOAM_OLD, dense: false };
                lay(self, unit.id, stern, radius * 2.0 * WAKE_SPACING, puff);
                // And spray thrown up at the bow, brief and small, kept apart from the wake.
                let bow = [center[0] + dx / length * radius * 0.85, center[1] + dy / length * radius * 0.85, water + 0.6];
                let spray = Puff { shape: Shape::Wake, at: bow, rise: 1.5, born: time, life: 0.7, size: size * 0.45, growth: 1.6, height_share: 0.3, color: FOAM, fade_to: FOAM, dense: false };
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
        self.beams.retain(|beam| time - beam.born < LASER_LIFE && beam.born - time < 10.0);
        for beam in &self.beams {
            let age = ((time - beam.born) / LASER_LIFE).clamp(0.0, 1.0) as f32;
            // Not yet due: its snapshot is still ahead of what is drawn.
            if time < beam.born {
                continue;
            }
            let width = LASER_WIDTH * (1.0 - age * age);
            if let Some(rod) = super::links::stretched(beam.from, beam.to, width, width, beam.color, beam.color, eye) {
                out.push(Shape::Beam, rod);
            }
        }
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
                let opacity = if puff.dense { DENSE_SMOKE_OPACITY * (1.0 - age.powi(4)) } else { SMOKE_OPACITY * (1.0 - age.powf(1.5)) };
                instance.hull[3] = (opacity * 255.0).round().clamp(0.0, 255.0) as u8;
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
        effects.note_events(&before, &after, 0.1, &|_, _| 0.0, None, &[]);
        assert_eq!(count(&mut effects, 0.15, Shape::Fireball), 0, "not before it gets there");
        let mut out = InstanceBatches::default();
        effects.build(0.21, [0.0; 3], &mut out);
        let fireballs = out.shape(Shape::Fireball);
        assert_eq!(fireballs.len(), 1);
        assert_eq!(fireballs[0].model_matrix[3][0], 155.0, "where it struck, not 55 short");
        let mut drawn = InstanceBatches::default();
        crate::render::instances::build_projectile_instances(&before.projectiles, &after.projectiles, &after.landed, 0.5, [0.0; 3], &|projectile| effects.shot(projectile), None, &mut drawn);
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
        effects.note_events(&before, &after, 0.0, &|_, _| 0.0, None, &[]);
        effects.note_events(&before, &after, 0.01, &|_, _| 0.0, None, &[]);
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
        effects.note_events(&before, &snapshot(2), 0.0, &|_, _| 0.0, None, &[]);
        let width = widest_fireball(&mut effects);
        assert!(width >= 20.0, "a blast reaching 12 is about 24 across, not {width}");
        let crater = scars(&mut effects, 1.0)[0];
        assert!(crater.model_matrix[0][0].hypot(crater.model_matrix[0][1]) >= 24.0, "and leaves a crater as wide");
    }

    /// How wide a blast's fire gets at its widest, across the ground, all its lobes together,
    /// with `effects` holding one blast.
    fn widest_fireball(effects: &mut Effects) -> f32 {
        (0..400)
            .map(|step| {
                let mut out = InstanceBatches::default();
                effects.build(f64::from(step) * 0.005, [0.0; 3], &mut out);
                let mut low = [f32::MAX; 2];
                let mut high = [f32::MIN; 2];
                for fireball in out.shape(Shape::Fireball) {
                    let across = |row: usize| fireball.model_matrix[row][0].hypot(fireball.model_matrix[row][1]);
                    let half = (across(0) + across(1)) / 4.0;
                    for axis in 0..2 {
                        low[axis] = low[axis].min(fireball.model_matrix[3][axis] - half);
                        high[axis] = high[axis].max(fireball.model_matrix[3][axis] + half);
                    }
                }
                (0..2).map(|axis| high[axis] - low[axis]).fold(0.0, f32::max)
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
                    effects.note_events(&before, &after, 0.0, &|_, _| 0.0, None, &[]);
                } else {
                    let mut before = before;
                    let mut bomb = shell(5, ProjectileKind::Shell);
                    bomb.splash = radius.to_bits();
                    before.projectiles = vec![bomb];
                    effects.note_events(&before, &after, 0.0, &|_, _| 0.0, None, &[]);
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
        effects.note_events(&before, &after, 0.0, &|_, _| 0.0, None, &[]);
        let mut out = InstanceBatches::default();
        effects.build(0.1, [0.0; 3], &mut out);
        let fire = out.shape(Shape::Fireball);
        assert!(!fire.is_empty(), "the unit that died blows up");
        assert!(fire.iter().all(|fireball| (fireball.model_matrix[3][0] - 50.0).abs() < 20.0), "where it died, and nowhere near the other");
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
        effects.note_events(&before, &after, 0.0, &|_, _| 0.0, None, &[]);
        assert_eq!(effects.wreck_origin(SimId(9)), Some((7, [1.0, 0.0])));
        assert_eq!(effects.wreck_origin(SimId(10)), None, "no unit died there");
        let mut later = snapshot(3);
        later.wrecks = vec![stranger];
        effects.note_events(&after, &later, 1.0, &|_, _| 0.0, None, &[]);
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
        effects.note_events(&before, &after, 0.0, &|_, _| 3.0, None, &[]);
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
        effects.note_events(&before, &snapshot(2), 0.0, &|_, _| 3.0, None, &[]);
        assert_eq!(scars(&mut effects, 1.0).len(), 1);
    }

    #[test]
    fn nothing_is_marked_under_water() {
        let mut before = snapshot(1);
        before.projectiles = vec![shell(5, ProjectileKind::Shell)];
        let mut effects = Effects::new();
        effects.note_events(&before, &snapshot(2), 0.0, &|_, _| -10.0, Some(0.0), &[]);
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

    /// Lands a shell fired by a unit of `key`, as the weapon fires it, onto ground at height 0 or,
    /// with `water`, into the water at that height over a sea floor under it.
    fn land(key: &str, water: Option<f64>) -> Effects {
        let db = crate::sim::blueprints::BlueprintDb::standard();
        let looks = crate::render::instances::unit_looks(&db);
        let mut gun = unit(1);
        gun.blueprint = db.id(key).0;
        let weapon = db.get(db.id(key)).weapon.expect("it has a weapon");
        let mut round = shell(5, ProjectileKind::Shell);
        round.splash = weapon.splash_radius.to_bits();
        round.z = 4096;
        let mut before = snapshot(1);
        before.units = vec![gun];
        before.projectiles = vec![round];
        let mut after = snapshot(2);
        after.units = vec![gun];
        let floor = if water.is_some() { -20.0 } else { 0.0 };
        let mut effects = Effects::new();
        // The shell is first seen, then lands.
        effects.note_events(&snapshot(0), &before, 0.0, &|_, _| floor, water, &looks);
        effects.note_events(&before, &after, 0.1, &|_, _| floor, water, &looks);
        effects
    }

    /// A battleship's round is an artillery round with less of a bang: as wide as its own, smaller
    /// blast, with fewer puffs than a T2 artillery shell's and no earth thrown up. T3 artillery
    /// throws up more than T2 does, with a ring of dust.
    #[test]
    fn a_battleships_round_lands_with_less_of_a_bang_than_artillery() {
        let (mut battleship, mut artillery, siege) = (land("t3_battleship", None), land("t2_artillery", None), land("t3_artillery", None));
        assert!(battleship.len() < artillery.len(), "{} puffs from a battleship, {} from T2 artillery", battleship.len(), artillery.len());
        assert!(siege.len() > artillery.len(), "{} puffs from T3 artillery, {} from T2", siege.len(), artillery.len());
        let (battleship_width, artillery_width) = (widest_fireball(&mut battleship), widest_fireball(&mut artillery));
        assert!((battleship_width - 80.0).abs() < 80.0 * 0.3, "a battleship's blast reaches 40: {battleship_width} across");
        assert!(artillery_width > battleship_width, "T2 artillery's blast reaches 70: {artillery_width} across");
        assert_eq!(count(&mut battleship, 0.15, Shape::Wake), 0, "no ring");
        assert_eq!(count(&mut land("t3_artillery", None), 0.15, Shape::Wake), 1, "T3 artillery's ring of dust");
    }

    /// Flak bursts in the air into black smoke, and leaves no crater; a torpedo throws up a column
    /// of water and leaves none either; a shell on the water throws up spray, not earth.
    #[test]
    fn flak_bursts_black_torpedoes_and_shells_on_the_water_throw_up_spray() {
        let mut flak = land("t2_flak", None);
        assert!(scars(&mut flak, 1.0).is_empty(), "no crater from flak");
        let mut out = InstanceBatches::default();
        flak.build(0.3, [0.0; 3], &mut out);
        assert!(out.shape(Shape::Smoke).iter().any(|puff| puff.color[0] < 0.2), "black smoke");
        let mut torpedo = land("t1_torpedo_launcher", Some(5.0));
        assert!(scars(&mut torpedo, 1.0).is_empty(), "no crater from a torpedo");
        let spray = |effects: &mut Effects| {
            let mut out = InstanceBatches::default();
            effects.build(0.3, [0.0; 3], &mut out);
            out.shape(Shape::Smoke).iter().map(|puff| puff.color[2]).collect::<Vec<f32>>()
        };
        assert!(spray(&mut torpedo).iter().any(|blue| *blue > 0.8), "a white column of water");
        let wet = spray(&mut land("t2_artillery", Some(5.0)));
        assert!(wet.iter().any(|blue| *blue > 0.8) && wet.iter().all(|blue| *blue > 0.3), "spray, not earth: {wet:?}");
        let dry = spray(&mut land("t2_artillery", None));
        assert!(dry.iter().any(|blue| *blue < 0.3), "earth on the ground: {dry:?}");
    }

    /// A nuclear blast is a mushroom cloud: far more puffs than a tactical missile's, a stem and
    /// cap still standing after eight seconds, long after the missile's smoke is gone.
    #[test]
    fn a_nuke_lands_as_a_mushroom_cloud() {
        let blast = |kind: ProjectileKind| {
            let mut before = snapshot(1);
            before.projectiles = vec![shell(5, kind)];
            let mut effects = Effects::new();
            effects.note_events(&before, &snapshot(2), 0.0, &|_, _| 0.0, None, &[]);
            effects
        };
        let (mut nuke, mut tactical) = (blast(ProjectileKind::StrategicMissile), blast(ProjectileKind::TacticalMissile));
        assert!(nuke.len() >= tactical.len() * 2, "{} puffs from a nuke, {} from a tactical missile", nuke.len(), tactical.len());
        assert!(count(&mut nuke, 8.0, Shape::Smoke) >= 8, "the cloud still stands");
        assert_eq!(count(&mut tactical, 8.0, Shape::Smoke), 0, "the missile's smoke is long gone");
    }

    /// A missile's tail burns as it flies, and an artillery round leaves a wisp of smoke.
    #[test]
    fn missiles_burn_at_the_tail_and_artillery_rounds_leave_a_wisp() {
        let mut before = snapshot(1);
        before.projectiles = vec![shell(5, ProjectileKind::TacticalMissile)];
        let mut after = snapshot(2);
        let mut moved = shell(5, ProjectileKind::TacticalMissile);
        moved.x += 4096 * 30;
        after.projectiles = vec![moved];
        let mut effects = Effects::new();
        effects.trail(&before, &after, 0.0, &[], None, 0.0);
        effects.trail(&before, &after, 1.0, &[], None, 0.05);
        assert!(count(&mut effects, 0.05, Shape::Fireball) > 0, "a flame at its tail");
        let mut effects = land("t2_artillery", None);
        effects.shots = vec![(SimId(7), ShotStyle { look: ShotLook::Artillery, scale: 1.0 })];
        let (mut before, mut after) = (snapshot(1), snapshot(2));
        before.projectiles = vec![shell(7, ProjectileKind::Shell)];
        let mut moved = shell(7, ProjectileKind::Shell);
        moved.x += 4096 * 60;
        after.projectiles = vec![moved];
        let smoke = count(&mut effects, 0.2, Shape::Smoke);
        effects.trail(&before, &after, 0.0, &[], None, 0.2);
        effects.trail(&before, &after, 1.0, &[], None, 0.25);
        assert!(count(&mut effects, 0.25, Shape::Smoke) > smoke, "a wisp behind the round");
    }

    #[test]
    fn puffs_grow_then_shrink_away() {
        let puff = Puff { shape: Shape::Smoke, at: [0.0; 3], rise: 1.0, born: 0.0, life: 1.0, size: 2.0, growth: 1.0, height_share: 1.0, color: SMOKE_DARK, fade_to: SMOKE_PALE, dense: false };
        let size = |time: f64| puff.at_time(time).map(|(_, size, _)| size);
        assert!(size(0.5).expect("alive") > size(0.0).expect("alive"));
        assert!(size(0.99).expect("alive") < size(0.5).expect("alive"));
        assert_eq!(size(1.0), None);
        assert_eq!(puff.at_time(0.5).expect("alive").0[2], 0.5, "it rises");
    }
}
