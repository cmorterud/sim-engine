//! Exotic weaponry (GAME_SPECIFICATION.md §12): stuns from EMP weapons, tractor beams that lift
//! a unit and crush it, and storms that a unit leaves where it is destroyed.

use std::hash::Hasher;

use super::blueprints::StormDef;
use super::components::{Blueprinted, Combat, Health, Held, MoveClass, Position, SimId, Stunned, Team, TractorBeam, UnitStats};
use super::spatial::Layer;
use super::world::{SimWorld, StormView, UnitView, BLOCKED_TARGET_TICKS};
use crate::math::fp::{fp_int, mul_toward_zero, sqrt_wide, Fp, Vec2Fp, Vec3Fp};

/// How often a storm looks again for the nearest enemy, in ticks.
const STORM_RETARGET_TICKS: u32 = 10;
/// How far a storm looks for enemies, in storm radii.
const STORM_SEARCH_RADII: i32 = 8;
/// How far above the ground a tractor beam holds what it lifts.
const TRACTOR_LIFT_UNITS: i32 = 10;
/// From this tier on, a unit is too big for a tractor beam to lift.
const EXPERIMENTAL_TIER: u8 = 4;

/// A storm roaming the battlefield where a unit died. It hurts every enemy of `team` beneath it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Storm {
    /// The side of the unit that left it.
    pub team: Team,
    pub position: Vec2Fp,
    /// Which way it is drifting, as a unit vector.
    pub heading: Vec2Fp,
    pub radius: Fp,
    pub damage_per_tick: Fp,
    pub ticks_left: u32,
    pub speed: Fp,
    age: u32,
}

impl Storm {
    pub fn new(team: Team, position: Vec2Fp, def: StormDef) -> Self {
        Storm { team, position, heading: Vec2Fp::ZERO, radius: def.radius, damage_per_tick: def.damage_per_tick, ticks_left: def.ticks, speed: def.speed, age: 0 }
    }

    pub fn hash_state(&self, hasher: &mut impl Hasher) {
        hasher.write(&[self.team as u8]);
        for value in [self.position.x, self.position.y, self.heading.x, self.heading.y, self.radius, self.damage_per_tick, self.speed] {
            hasher.write(&value.to_bits().to_le_bytes());
        }
        hasher.write(&self.ticks_left.to_le_bytes());
        hasher.write(&self.age.to_le_bytes());
    }
}

impl SimWorld {
    /// Runs at the start of the cleanup, before this tick's damage: stuns wear off, tractor
    /// beams take hold, draw in and crush what they hold, and storms roam and strike.
    pub(super) fn step_exotic(&mut self) {
        if !self.disabled.is_empty() {
            self.step_disabled();
        }
        if !self.tractors.is_empty() {
            self.release_lost_holds();
        }
        if !self.pending_grabs.is_empty() {
            self.take_hold();
        }
        if !self.storms.is_empty() {
            self.step_storms();
        }
    }

    pub(super) fn is_disabled(&self, id: SimId) -> bool {
        !self.disabled.is_empty() && self.disabled.binary_search_by_key(&id, |unit| unit.0).is_ok()
    }

    fn mark_disabled(&mut self, id: SimId, entity: hecs::Entity) {
        if let Err(at) = self.disabled.binary_search_by_key(&id, |unit| unit.0) {
            self.disabled.insert(at, (id, entity));
        }
    }

    /// Stuns wear off. A held unit is drawn in toward the maw of the unit holding it, arriving as
    /// its time runs out, and is crushed then. One whose holder has died is let go, and falls.
    fn step_disabled(&mut self) {
        let mut freed: Vec<SimId> = Vec::new();
        for index in 0..self.disabled.len() {
            let (id, entity) = self.disabled[index];
            let mut still = false;
            if let Ok(mut stunned) = self.ecs.get::<&mut Stunned>(entity) {
                stunned.ticks_left = stunned.ticks_left.saturating_sub(1);
                still = stunned.ticks_left > 0;
            }
            if !still {
                let _ = self.ecs.remove_one::<Stunned>(entity);
            }
            let Some(held) = self.ecs.get::<&Held>(entity).ok().map(|held| *held) else {
                if !still {
                    freed.push(id);
                }
                continue;
            };
            let holder = self.find_unit(held.by).map(|(_, holder)| holder);
            let Some(holder) = holder else {
                self.let_go(entity);
                if !still {
                    freed.push(id);
                }
                continue;
            };
            let ticks_left = held.ticks_left.saturating_sub(1);
            let maw = self.ecs.get::<&Position>(holder).expect("units have positions").0;
            let reach = self.ecs.get::<&UnitStats>(holder).expect("units have stats").radius;
            let radius = self.ecs.get::<&UnitStats>(entity).expect("units have stats").radius;
            let here = self.ecs.get::<&Position>(entity).expect("units have positions").0.xy();
            let offset = maw.xy() - here;
            let distance = sqrt_wide(offset.length_squared());
            let gap = distance - reach - radius;
            let mut next = here;
            if gap > Fp::ZERO && distance > Fp::ZERO {
                let step = gap / Fp::from_num(ticks_left + 1);
                let direction = offset.normalized();
                next = here + direction.scaled(step);
            }
            let lifted = self.terrain.height_at(next) + radius + fp_int(TRACTOR_LIFT_UNITS);
            self.ecs.get::<&mut Position>(entity).expect("units have positions").0 = Vec3Fp::new(next.x, next.y, lifted);
            if ticks_left == 0 {
                // Crushed, with the credit to the unit that held it.
                self.pending_hits.push((entity, Fp::MAX, Some(held.by)));
                self.let_go(entity);
                if !still {
                    freed.push(id);
                }
            } else {
                self.ecs.get::<&mut Held>(entity).expect("just read").ticks_left = ticks_left;
            }
        }
        if !freed.is_empty() {
            self.disabled.retain(|unit| freed.binary_search(&unit.0).is_err());
        }
    }

    /// Lets go of a held unit, which drops to the ground, and frees the beam that held it.
    fn let_go(&mut self, entity: hecs::Entity) {
        let Ok(held) = self.ecs.remove_one::<Held>(entity) else {
            return;
        };
        if let Some((_, holder)) = self.find_unit(held.by) {
            if let Ok(mut beam) = self.ecs.get::<&mut TractorBeam>(holder) {
                beam.holding = None;
            }
        }
        let stats = *self.ecs.get::<&UnitStats>(entity).expect("units have stats");
        let mut position = self.ecs.get::<&mut Position>(entity).expect("units have positions");
        let ground = self.terrain.height_at(position.0.xy());
        let floor = if stats.move_class == MoveClass::Hover { ground.max(self.terrain.water_level()) } else { ground };
        position.0.z = floor + stats.radius;
    }

    /// A beam whose unit has died some other way is free again.
    fn release_lost_holds(&mut self) {
        for index in 0..self.tractors.len() {
            let (id, entity) = self.tractors[index];
            let holding = self.ecs.get::<&TractorBeam>(entity).expect("tractor list only holds beams").holding;
            let Some(target) = holding else {
                continue;
            };
            let still_held = self.find_unit(target).is_some_and(|(_, held)| self.ecs.get::<&Held>(held).is_ok_and(|held| held.by == id));
            if !still_held {
                self.ecs.get::<&mut TractorBeam>(entity).expect("tractor list only holds beams").holding = None;
            }
        }
    }

    /// Beams that fired this tick take hold of their targets, in `SimId` order. Commanders and
    /// experimentals are too big to lift, and a unit already held cannot be taken.
    fn take_hold(&mut self) {
        let tick = self.current_tick;
        for (holder_id, target_id) in std::mem::take(&mut self.pending_grabs) {
            let (Some((_, holder)), Some((_, target))) = (self.find_unit(holder_id), self.find_unit(target_id)) else {
                continue;
            };
            let Ok(beam) = self.ecs.get::<&TractorBeam>(holder).map(|beam| *beam) else {
                continue;
            };
            if beam.holding.is_some() || self.ecs.get::<&Held>(target).is_ok() {
                continue;
            }
            let tier = self.ecs.get::<&Blueprinted>(target).map_or(0, |unit| self.blueprints.get(unit.0).tier);
            let stats = *self.ecs.get::<&UnitStats>(target).expect("units have stats");
            let liftable = !stats.is_structure
                && matches!(stats.move_class, MoveClass::Land | MoveClass::Hover | MoveClass::Seabed)
                && !self.commanders.contains(&Some(target_id))
                && tier < EXPERIMENTAL_TIER;
            if !liftable {
                let mut combat = self.ecs.get::<&mut Combat>(holder).expect("units have combat");
                combat.blocked = Some((target_id, tick + BLOCKED_TARGET_TICKS));
                combat.target = None;
                continue;
            }
            self.ecs.insert_one(target, Held { by: holder_id, ticks_left: beam.crush_ticks }).expect("target is live");
            self.ecs.get::<&mut TractorBeam>(holder).expect("just read").holding = Some(target_id);
            self.mark_disabled(target_id, target);
        }
    }

    /// Applies this tick's stuns, after its damage, to what survived it. Aircraft are not
    /// affected. A new stun only ever lengthens one already running.
    pub(super) fn apply_stuns(&mut self) {
        for (entity, ticks) in std::mem::take(&mut self.pending_stuns) {
            let Ok(id) = self.ecs.get::<&SimId>(entity).map(|id| *id) else {
                continue;
            };
            let alive = self.ecs.get::<&Health>(entity).is_ok_and(|health| health.current > Fp::ZERO);
            let flying = self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.move_class == MoveClass::Air);
            if !alive || flying {
                continue;
            }
            if let Ok(mut stunned) = self.ecs.get::<&mut Stunned>(entity) {
                stunned.ticks_left = stunned.ticks_left.max(ticks);
                continue;
            }
            self.ecs.insert_one(entity, Stunned { ticks_left: ticks }).expect("entity is live");
            self.mark_disabled(id, entity);
        }
    }

    /// Each storm drifts toward the nearest enemy it can find, or wanders if there is none, and
    /// strikes every enemy beneath it. It blows itself out when its time runs out.
    fn step_storms(&mut self) {
        let max_coordinate = Fp::from_bits(self.terrain.max_coordinate_raw());
        for index in 0..self.storms.len() {
            let mut storm = self.storms[index];
            let enemy = storm.team.enemy();
            let (x, y) = (storm.position.x.to_bits(), storm.position.y.to_bits());
            if storm.age.is_multiple_of(STORM_RETARGET_TICKS) {
                let search = storm.radius.to_bits().saturating_mul(STORM_SEARCH_RADII);
                let mut nearest: Option<(i64, SimId, i32, i32)> = None;
                let mut consider = |other: &super::spatial::GridEntry| {
                    let (dx, dy) = (i64::from(other.x - x), i64::from(other.y - y));
                    let candidate = (dx * dx + dy * dy, other.id, other.x, other.y);
                    if candidate.0 <= i64::from(search) * i64::from(search) && nearest.is_none_or(|best| (candidate.0, candidate.1) < (best.0, best.1)) {
                        nearest = Some(candidate);
                    }
                };
                self.spatial_grid.for_each_candidate(Some(enemy), Layer::Mobile, x, y, search, &mut consider);
                self.spatial_grid.for_each_candidate(Some(enemy), Layer::Structure, x, y, search, &mut consider);
                let toward = nearest.map(|(_, _, to_x, to_y)| Vec2Fp::new(Fp::from_bits(to_x - x), Fp::from_bits(to_y - y)).normalized());
                storm.heading = match toward {
                    Some(heading) if heading != Vec2Fp::ZERO => heading,
                    Some(_) => Vec2Fp::ZERO,
                    None => {
                        let wander = Vec2Fp::new(self.rng.range_fp(-Fp::ONE, Fp::ONE), self.rng.range_fp(-Fp::ONE, Fp::ONE)).normalized();
                        if wander == Vec2Fp::ZERO { Vec2Fp::new(Fp::ONE, Fp::ZERO) } else { wander }
                    }
                };
            }
            storm.age += 1;
            let clamp = |value: Fp| value.clamp(Fp::ZERO, max_coordinate);
            storm.position = Vec2Fp::new(clamp(storm.position.x + mul_toward_zero(storm.heading.x, storm.speed)), clamp(storm.position.y + mul_toward_zero(storm.heading.y, storm.speed)));

            let (x, y) = (storm.position.x.to_bits(), storm.position.y.to_bits());
            let radius = i64::from(storm.radius.to_bits());
            let (max_mobile, max_structure) = (self.max_mobile_radius(), self.max_structure_radius());
            let hits = &mut self.pending_hits;
            let mut strike = |other: &super::spatial::GridEntry| {
                let (dx, dy) = (i64::from(other.x - x), i64::from(other.y - y));
                let reach = radius + i64::from(other.radius);
                if dx * dx + dy * dy <= reach * reach {
                    hits.push((other.entity, storm.damage_per_tick, None));
                }
            };
            let search = storm.radius.to_bits();
            self.spatial_grid.for_each_candidate(Some(enemy), Layer::Mobile, x, y, search + max_mobile, &mut strike);
            self.spatial_grid.for_each_candidate(Some(enemy), Layer::Structure, x, y, search + max_structure, &mut strike);
            storm.ticks_left = storm.ticks_left.saturating_sub(1);
            self.storms[index] = storm;
        }
        self.storms.retain(|storm| storm.ticks_left > 0);
    }

    /// What the snapshot shows of §12: the stunned units among those shown, the beams holding
    /// them, and the storms the viewer can see.
    pub(super) fn exotic_views(&self, units: &[UnitView], seen: &impl Fn(Team, i32, i32) -> bool) -> (Vec<SimId>, Vec<[[i32; 3]; 2]>, Vec<StormView>) {
        let mut stunned = Vec::new();
        let mut beams = Vec::new();
        let shown = |id: SimId| units.binary_search_by_key(&id, |unit| unit.id).ok().map(|index| &units[index]);
        for &(id, entity) in &self.disabled {
            let Some(unit) = shown(id) else {
                continue;
            };
            if self.ecs.get::<&Stunned>(entity).is_ok() {
                stunned.push(id);
            }
            if let Some(holder) = self.ecs.get::<&Held>(entity).ok().and_then(|held| shown(held.by)) {
                beams.push([[holder.x, holder.y, holder.z], [unit.x, unit.y, unit.z]]);
            }
        }
        let storms = self
            .storms
            .iter()
            .filter(|storm| seen(storm.team, storm.position.x.to_bits(), storm.position.y.to_bits()))
            .map(|storm| StormView { x: storm.position.x.to_bits(), y: storm.position.y.to_bits(), radius: storm.radius.to_bits() })
            .collect();
        (stunned, beams, storms)
    }
}
