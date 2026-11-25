//! The commanders' field enhancements, Orbital Redeploy, and self-destruct
//! (GAME_SPECIFICATION.md §2.1).

use std::hash::Hasher;

use super::blueprints::{BlueprintId, EnhancementEffects, EnhancementId, Res, Slot, TeleporterDef, TICKS_PER_SECOND};
use super::components::{
    Builder, Combat, Construction, Enhancements, Enhancing, Health, Intel, Order, Position, SelfDestructing, Shield, SimId, Teleporting, UnitStats, Upgrading,
    Velocity, Veterancy,
};
use super::visibility::Sense;
use super::world::{SimWorld, RANK_HEALTH_PERMILLE};
use crate::math::fp::{sqrt_wide, Fp, Vec2Fp, Vec3Fp};

/// Ctrl+K gives five seconds to change one's mind.
pub const SELF_DESTRUCT_TICKS: u32 = 5 * TICKS_PER_SECOND as u32;

/// One of the inspiration's map units is one of our terrain cells, and its teleport costs are
/// worked out in its units.
const UNITS_PER_ORIGINAL_UNIT: i64 = super::config::TERRAIN_CELL_UNITS as i64;

/// `distance` to the power 2.1, for a whole number of the inspiration's units, as its
/// teleport costs use. Worked out in whole numbers, so that every machine agrees: the tenth
/// root comes from a search, to 1/2048.
pub fn distance_cost_power(distance: i64) -> i64 {
    let distance = distance.clamp(0, 1023);
    if distance == 0 {
        return 0;
    }
    // The largest r, in 2048ths, with r^10 <= distance: (r / 2048)^10 <= distance.
    let target = (distance as u128) << 110;
    let (mut low, mut high) = (2048u128, 4096u128);
    while high - low > 1 {
        let middle = (low + high) / 2;
        let power = (0..9).fold(middle, |product, _| product * middle);
        if power <= target {
            low = middle;
        } else {
            high = middle;
        }
    }
    (distance * distance * low as i64) / 2048
}

/// What a teleport over this distance costs, and how long it charges, in the inspiration's
/// terms: a flat cost plus half the distance to the power 2.1, and a delay plus a
/// hundred-thousandth of it in seconds, each up to a limit.
pub fn teleport_cost(teleporter: &TeleporterDef, distance: Fp) -> (Res, u32) {
    let original = i64::from(distance.to_bits()) / (i64::from(Fp::ONE.to_bits()) * UNITS_PER_ORIGINAL_UNIT);
    let power = distance_cost_power(original);
    let energy = (teleporter.flat_energy + Res::from_num(power / 2)).min(teleporter.max_energy);
    // A hundred-thousandth of a second is a ten-thousandth of a tick.
    let ticks = (i64::from(teleporter.min_ticks) + power / 10_000).min(i64::from(teleporter.max_ticks)) as u32;
    (energy, ticks.max(1))
}

impl SimWorld {
    /// The combined effects of a unit's enhancements.
    pub fn enhancement_effects(&self, entity: hecs::Entity) -> EnhancementEffects {
        let mut total = EnhancementEffects::default();
        let Ok(installed) = self.ecs.get::<&Enhancements>(entity) else {
            return total;
        };
        for &id in installed.0.iter().flatten() {
            let effects = &self.blueprints.enhancement(id).effects;
            total.build_power = total.build_power.max(effects.build_power);
            total.build_tier = total.build_tier.max(effects.build_tier);
            total.health += effects.health;
            total.regen_per_tick += effects.regen_per_tick;
            total.weapon_range = effects.weapon_range.or(total.weapon_range);
            total.weapon_damage = effects.weapon_damage.or(total.weapon_damage);
            total.weapon_reload_ticks = effects.weapon_reload_ticks.or(total.weapon_reload_ticks);
            total.weapon_splash = effects.weapon_splash.or(total.weapon_splash);
            total.vision = total.vision.max(effects.vision);
            total.deep_scan = total.deep_scan.max(effects.deep_scan);
            total.jammer = effects.jammer.or(total.jammer);
            total.jammer_upkeep = effects.jammer_upkeep.or(total.jammer_upkeep);
            total.shield = effects.shield.or(total.shield);
            total.cloak_upkeep = effects.cloak_upkeep.or(total.cloak_upkeep);
            total.stealth_upkeep = effects.stealth_upkeep.or(total.stealth_upkeep);
            total.teleporter = effects.teleporter.or(total.teleporter);
            total.mass_income += effects.mass_income;
            total.energy_income += effects.energy_income;
        }
        total
    }

    /// The highest tier of structure a builder can build: its own, or its fabrication pack's.
    pub fn build_tier(&self, entity: hecs::Entity) -> u8 {
        let own = self.ecs.get::<&super::components::Blueprinted>(entity).map_or(0, |unit| self.blueprints.get(unit.0).tier);
        own.max(self.enhancement_effects(entity).build_tier)
    }

    /// Whether a builder can build this: it is on its list, and not above its tier.
    pub fn can_build(&self, entity: hecs::Entity, blueprint: BlueprintId) -> bool {
        self.on_build_list(entity, blueprint) && self.blueprints.get(blueprint).tier <= self.build_tier(entity)
    }

    /// The tier it will build once the fabrication pack it is being fitted with is done.
    pub fn build_tier_ahead(&self, entity: hecs::Entity) -> u8 {
        let pending = self.ecs.get::<&Enhancing>(entity).map_or(0, |under_way| self.blueprints.enhancement(under_way.enhancement).effects.build_tier);
        self.build_tier(entity).max(pending)
    }

    /// Whether a builder could build this once its fabrication pack under way is done, but not
    /// yet. Such a build waits behind the suite, and goes if the suite is cancelled.
    pub fn can_build_after_suite(&self, entity: hecs::Entity, blueprint: BlueprintId) -> bool {
        self.on_build_list(entity, blueprint) && !self.can_build(entity, blueprint) && self.blueprints.get(blueprint).tier <= self.build_tier_ahead(entity)
    }

    fn on_build_list(&self, entity: hecs::Entity, blueprint: BlueprintId) -> bool {
        self.ecs.get::<&super::components::Blueprinted>(entity).is_ok_and(|unit| self.blueprints.get(unit.0).builds.contains(&blueprint))
    }

    /// Starts an enhancement, if the unit can take it. What is in its slot is removed first,
    /// unless the new one builds on it, in which case it stays until the new one is done.
    pub(super) fn start_enhancement(&mut self, id: SimId, enhancement: EnhancementId) {
        let Some((_, entity)) = self.find_unit(id) else {
            return;
        };
        let Ok(blueprint) = self.ecs.get::<&super::components::Blueprinted>(entity).map(|unit| unit.0) else {
            return;
        };
        let def = self.blueprints.enhancement(enhancement);
        let (slot, requires) = (def.slot, def.requires);
        if !def.units.contains(&blueprint) || self.ecs.get::<&Construction>(entity).is_ok() {
            return;
        }
        if self.ecs.get::<&Enhancing>(entity).is_ok_and(|under_way| under_way.enhancement == enhancement) {
            return;
        }
        let current = self.ecs.get::<&Enhancements>(entity).map_or(None, |installed| installed.0[slot.index()]);
        // One that is there already, or that what is there builds on, cannot be taken again:
        // the T2 suite after the T3 one would be a step back.
        if self.blueprints.slot_has(current, enhancement) || requires.is_some_and(|needed| current != Some(needed)) {
            return;
        }
        // It drops what it was doing, and stands still.
        self.clear_unit_plans(&[id]);
        self.set_order(id, Order::IDLE);
        if current.is_some() && current != requires {
            self.set_enhancement(id, entity, slot, None);
        }
        self.ecs.insert_one(entity, Enhancing { enhancement, progress: Res::ZERO }).expect("unit is live");
        let at = self.enhancing.partition_point(|unit| unit.0 < id);
        self.enhancing.insert(at, (id, entity));
    }

    pub(super) fn finish_enhancement(&mut self, id: SimId, entity: hecs::Entity) {
        let Ok(under_way) = self.ecs.remove_one::<Enhancing>(entity) else {
            return;
        };
        self.enhancing.retain(|unit| unit.0 != id);
        let slot = self.blueprints.enhancement(under_way.enhancement).slot;
        self.set_enhancement(id, entity, slot, Some(under_way.enhancement));
    }

    /// Cancels an enhancement under way, losing what was spent on it. Structures queued for the
    /// tier it would have unlocked go with it.
    pub(super) fn cancel_enhancement(&mut self, id: SimId, entity: hecs::Entity) {
        if self.ecs.remove_one::<Enhancing>(entity).is_ok() {
            self.enhancing.retain(|unit| unit.0 != id);
            let tier = self.build_tier(entity);
            let blueprints = &self.blueprints;
            if let Ok(mut builder) = self.ecs.get::<&mut super::components::Builder>(entity) {
                builder.tasks.retain(|task| !matches!(task, super::components::BuildTask::Build { blueprint, site: None, .. } if blueprints.get(*blueprint).tier > tier));
            }
        }
        if self.ecs.remove_one::<Teleporting>(entity).is_ok() {
            self.teleporting.retain(|unit| unit.0 != id);
        }
    }

    /// Puts an enhancement in a slot, or empties it, at once, and updates the unit to match.
    /// For tests and scenarios, which start units already enhanced.
    pub fn install_enhancement(&mut self, id: SimId, enhancement: EnhancementId) {
        if let Some((_, entity)) = self.find_unit(id) {
            let slot = self.blueprints.enhancement(enhancement).slot;
            self.set_enhancement(id, entity, slot, Some(enhancement));
        }
    }

    fn set_enhancement(&mut self, id: SimId, entity: hecs::Entity, slot: Slot, value: Option<EnhancementId>) {
        let mut installed = self.ecs.get::<&Enhancements>(entity).map_or(Enhancements::default(), |installed| *installed);
        installed.0[slot.index()] = value;
        let listed = self.enhanced.binary_search_by_key(&id, |unit| unit.0);
        if installed.0.iter().all(Option::is_none) {
            let _ = self.ecs.remove_one::<Enhancements>(entity);
            if let Ok(index) = listed {
                self.enhanced.remove(index);
            }
        } else {
            self.ecs.insert_one(entity, installed).expect("unit is live");
            if let Err(index) = listed {
                self.enhanced.insert(index, (id, entity));
            }
        }
        self.refresh_enhanced_stats(id, entity);
    }

    /// Sets what enhancements change from the unit's blueprint and its enhancements together.
    fn refresh_enhanced_stats(&mut self, id: SimId, entity: hecs::Entity) {
        let blueprint = self.blueprints.get(self.ecs.get::<&super::components::Blueprinted>(entity).expect("enhanced units have blueprints").0);
        let (base_power, base_health, weapon, base_vision) = (blueprint.build_power, blueprint.health, blueprint.weapon, blueprint.vision);
        let effects = self.enhancement_effects(entity);
        if let Ok(mut builder) = self.ecs.get::<&mut Builder>(entity) {
            builder.power = effects.build_power.unwrap_or(base_power);
        }
        if let (Ok(mut combat), Some(weapon)) = (self.ecs.get::<&mut Combat>(entity), weapon) {
            combat.range = effects.weapon_range.unwrap_or(weapon.range);
            combat.projectile_damage = effects.weapon_damage.unwrap_or(weapon.damage);
            combat.reload_ticks = effects.weapon_reload_ticks.unwrap_or(weapon.reload_ticks);
            combat.projectile_splash = effects.weapon_splash.unwrap_or(weapon.splash_radius);
        }
        self.ecs.get::<&mut UnitStats>(entity).expect("units have stats").vision = effects.vision.unwrap_or(base_vision);
        // Veterancy adds its share on top.
        let rank = self.ecs.get::<&Veterancy>(entity).map_or(0, |veterancy| veterancy.rank);
        let max = Fp::from_bits((i64::from((base_health + effects.health).to_bits()) * (1_000 + RANK_HEALTH_PERMILLE * i64::from(rank)) / 1_000) as i32);
        {
            let mut health = self.ecs.get::<&mut Health>(entity).expect("units have health");
            let gained = (max - health.max).max(Fp::ZERO);
            health.max = max;
            health.current = (health.current + gained).min(max);
        }
        let listed = self.shields.binary_search_by_key(&id, |unit| unit.0);
        match effects.shield {
            Some(def) => {
                let same = self.ecs.get::<&Shield>(entity).is_ok_and(|shield| shield.radius == def.radius && shield.height == def.height && shield.max_health == def.health && shield.upkeep == def.upkeep);
                if !same {
                    let shield = Shield {
                        radius: def.radius,
                        height: def.height,
                        health: def.health,
                        max_health: def.health,
                        regen_per_tick: def.regen_per_tick,
                        recharging: 0,
                        recharge_ticks: def.recharge_ticks,
                        upkeep: def.upkeep,
                    };
                    self.ecs.insert_one(entity, shield).expect("unit is live");
                }
                if let Err(index) = listed {
                    self.shields.insert(index, (id, entity));
                }
            }
            None => {
                let _ = self.ecs.remove_one::<Shield>(entity);
                if let Ok(index) = listed {
                    self.shields.remove(index);
                }
            }
        }
        let needs_intel = effects.cloak_upkeep.is_some() || effects.stealth_upkeep.is_some() || effects.jammer.is_some() || effects.deep_scan.is_some();
        if needs_intel && self.ecs.get::<&Intel>(entity).is_err() {
            let intel = Intel { running: true, cloak_on: false, cloaked: false, stealthed: false, jamming: false };
            self.ecs.insert_one(entity, intel).expect("unit is live");
            let at = self.intel.partition_point(|unit| unit.0 < id);
            self.intel.insert(at, (id, entity));
        }
        let own_cloak = blueprint_cloak(self, entity);
        if let Ok(mut intel) = self.ecs.get::<&mut Intel>(entity) {
            if effects.cloak_upkeep.is_none() && !own_cloak {
                intel.cloak_on = false;
                intel.cloaked = false;
            }
            if effects.stealth_upkeep.is_none() {
                intel.stealthed = false;
            }
            if effects.jammer.is_none() {
                intel.jamming = false;
            }
        }
        self.economy_dirty = true;
    }

    /// A unit's radar and deep scan ranges: its blueprint's, or an enhancement's deep scan.
    pub(super) fn sensor_ranges(&self, entity: hecs::Entity) -> (Fp, Fp) {
        let unit = self.blueprints.get(self.ecs.get::<&super::components::Blueprinted>(entity).expect("sensor units have blueprints").0);
        let deep_scan = if self.enhanced.is_empty() { None } else { self.enhancement_effects(entity).deep_scan };
        (unit.radar, deep_scan.unwrap_or(unit.deep_scan))
    }

    /// What a unit pays to stay cloaked: from its blueprint, or else from an enhancement.
    pub(super) fn cloak_upkeep(&self, entity: hecs::Entity) -> Option<Res> {
        let own = self.ecs.get::<&super::components::Blueprinted>(entity).ok().and_then(|unit| self.blueprints.get(unit.0).cloak_upkeep);
        own.or_else(|| self.enhancement_effects(entity).cloak_upkeep)
    }

    /// Enhanced units heal on their own, a little every tick.
    pub(super) fn regenerate_enhanced(&mut self) {
        for index in 0..self.enhanced.len() {
            let entity = self.enhanced[index].1;
            let regen = self.enhancement_effects(entity).regen_per_tick;
            if regen == Fp::ZERO {
                continue;
            }
            let mut health = self.ecs.get::<&mut Health>(entity).expect("enhanced units are live");
            if health.current > Fp::ZERO {
                health.current = (health.current + regen).min(health.max);
            }
        }
    }

    /// Starts each unit with a teleporter charging toward a spot its team can sense: in sight,
    /// or on radar or deep scan. It must be open ground.
    pub(super) fn command_teleport(&mut self, units: &[SimId], point: Vec2Fp) {
        if !self.terrain.in_bounds(point) || !self.terrain.is_passable(point) {
            return;
        }
        for &id in units {
            let Some((_, entity)) = self.find_unit(id) else {
                continue;
            };
            let Some(teleporter) = self.enhancement_effects(entity).teleporter else {
                continue;
            };
            if self.ecs.get::<&Construction>(entity).is_ok() {
                continue;
            }
            let team = self.ecs.get::<&UnitStats>(entity).expect("units have stats").team;
            let (x, y) = (point.x.to_bits(), point.y.to_bits());
            let senses = !self.config.fog_of_war
                || self.visibility.is_visible(team, x, y)
                || self.visibility.senses(Sense::Radar, team, x, y)
                || self.visibility.senses(Sense::DeepScan, team, x, y);
            if !senses {
                continue;
            }
            let here = self.ecs.get::<&Position>(entity).expect("units have positions").0.xy();
            let distance = sqrt_wide((point - here).length_squared());
            let (energy, ticks) = teleport_cost(&teleporter, distance);
            self.clear_unit_plans(&[id]);
            self.set_order(id, Order::IDLE);
            let charge = Teleporting { destination: point, ticks_left: ticks, total_ticks: ticks, energy_per_tick: energy / i64::from(ticks) };
            self.ecs.insert_one(entity, charge).expect("unit is live");
            let at = self.teleporting.partition_point(|unit| unit.0 < id);
            self.teleporting.insert(at, (id, entity));
        }
    }

    /// Pays each charging teleport's energy for this tick, after shields and intel, aborting any
    /// that cannot be paid, and jumps those that are done. Returns what each team has paid.
    pub(super) fn step_teleports(&mut self, mut paid: Vec<Res>) -> Vec<Res> {
        let mut aborted = Vec::new();
        let mut arrived = Vec::new();
        let mut paid_by = Vec::new();
        for &(id, entity) in &self.teleporting {
            let team = self.ecs.get::<&UnitStats>(entity).expect("units have stats").owner.index();
            let mut charge = self.ecs.get::<&mut Teleporting>(entity).expect("teleport list only holds teleports");
            let economy = &self.economies[team];
            if economy.energy + economy.energy_income - paid[team] < charge.energy_per_tick {
                aborted.push((id, entity));
                continue;
            }
            paid[team] += charge.energy_per_tick;
            paid_by.push((id, charge.energy_per_tick));
            charge.ticks_left -= 1;
            if charge.ticks_left == 0 {
                arrived.push((id, entity, charge.destination));
            }
        }
        for (id, entity) in aborted {
            self.ecs.remove_one::<Teleporting>(entity).expect("just checked");
            self.teleporting.retain(|unit| unit.0 != id);
        }
        for (id, entity, destination) in arrived {
            self.ecs.remove_one::<Teleporting>(entity).expect("just checked");
            self.teleporting.retain(|unit| unit.0 != id);
            self.jump(id, entity, destination);
        }
        self.upkeep_paid.extend(paid_by);
        paid
    }

    /// Lands a teleport, destroying any enemy unit standing where it lands.
    fn jump(&mut self, id: SimId, entity: hecs::Entity, destination: Vec2Fp) {
        let stats = *self.ecs.get::<&UnitStats>(entity).expect("units have stats");
        for &(_, other) in &self.units {
            let Ok(them) = self.ecs.get::<&UnitStats>(other) else {
                continue;
            };
            if them.team == stats.team || them.is_structure {
                continue;
            }
            let at = self.ecs.get::<&Position>(other).expect("units have positions").0.xy();
            let reach = i64::from((stats.radius + them.radius).to_bits());
            let (dx, dy) = (i64::from((at.x - destination.x).to_bits()), i64::from((at.y - destination.y).to_bits()));
            if dx * dx + dy * dy < reach * reach {
                let health = self.ecs.get::<&Health>(other).expect("units have health").current;
                self.pending_hits.push((other, health.max(Fp::from_bits(1)), Some(id)));
            }
        }
        let ground = self.terrain.height_at(destination);
        self.ecs.get::<&mut Position>(entity).expect("units have positions").0 = Vec3Fp::new(destination.x, destination.y, ground);
        if let Ok(mut velocity) = self.ecs.get::<&mut Velocity>(entity) {
            velocity.0 = Vec3Fp::ZERO;
        }
    }

    /// Starts each unit counting down to blowing up, or stops one that already is.
    pub(super) fn command_self_destruct(&mut self, units: &[SimId]) {
        for &id in units {
            let Some((_, entity)) = self.find_unit(id) else {
                continue;
            };
            let listed = self.self_destructing.binary_search_by_key(&id, |unit| unit.0);
            match listed {
                Ok(index) => {
                    self.ecs.remove_one::<SelfDestructing>(entity).expect("listed");
                    self.self_destructing.remove(index);
                }
                Err(index) => {
                    self.ecs.insert_one(entity, SelfDestructing { ticks_left: SELF_DESTRUCT_TICKS }).expect("unit is live");
                    self.self_destructing.insert(index, (id, entity));
                }
            }
        }
    }

    /// Counts down, and blows up those whose time is up. A commander's reactor goes off as it
    /// would in battle, and nothing is left to reclaim.
    pub(super) fn step_self_destructs(&mut self) {
        for index in 0..self.self_destructing.len() {
            let entity = self.self_destructing[index].1;
            let mut countdown = self.ecs.get::<&mut SelfDestructing>(entity).expect("list only holds those counting down");
            countdown.ticks_left = countdown.ticks_left.saturating_sub(1);
            if countdown.ticks_left == 0 {
                drop(countdown);
                let health = self.ecs.get::<&Health>(entity).expect("units have health").current;
                self.pending_hits.push((entity, health.max(Fp::from_bits(1)), None));
            }
        }
    }

    /// Whether a unit dying now blew itself up, and so leaves no debris.
    pub(super) fn blew_itself_up(&self, entity: hecs::Entity) -> bool {
        self.ecs.get::<&SelfDestructing>(entity).is_ok_and(|countdown| countdown.ticks_left == 0)
    }

    pub(super) fn hash_enhancement_state(&self, entity: hecs::Entity, hasher: &mut impl Hasher) {
        if let Ok(installed) = self.ecs.get::<&Enhancements>(entity) {
            hasher.write(&[20]);
            for slot in installed.0 {
                hasher.write(&slot.map_or(u16::MAX, |id| id.0).to_le_bytes());
            }
        }
        if let Ok(under_way) = self.ecs.get::<&Enhancing>(entity) {
            hasher.write(&[21]);
            hasher.write(&under_way.enhancement.0.to_le_bytes());
            hasher.write(&under_way.progress.to_bits().to_le_bytes());
        }
        if let Ok(charge) = self.ecs.get::<&Teleporting>(entity) {
            hasher.write(&[22]);
            hasher.write(&charge.destination.x.to_bits().to_le_bytes());
            hasher.write(&charge.destination.y.to_bits().to_le_bytes());
            hasher.write(&charge.ticks_left.to_le_bytes());
            hasher.write(&charge.total_ticks.to_le_bytes());
            hasher.write(&charge.energy_per_tick.to_bits().to_le_bytes());
        }
        if let Ok(countdown) = self.ecs.get::<&SelfDestructing>(entity) {
            hasher.write(&[23]);
            hasher.write(&countdown.ticks_left.to_le_bytes());
        }
    }

    /// Whether a unit is busy with something that keeps it standing still: an upgrade, an
    /// enhancement, or a teleport charge.
    pub(super) fn is_busy_in_place(&self, entity: hecs::Entity) -> bool {
        self.ecs.get::<&Upgrading>(entity).is_ok() || self.ecs.get::<&Enhancing>(entity).is_ok() || self.ecs.get::<&Teleporting>(entity).is_ok()
    }
}

fn blueprint_cloak(world: &SimWorld, entity: hecs::Entity) -> bool {
    world.ecs.get::<&super::components::Blueprinted>(entity).is_ok_and(|unit| world.blueprints.get(unit.0).cloak_upkeep.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::float_arithmetic)]
    fn the_distance_power_matches_the_original_formula() {
        for distance in [1i64, 2, 10, 100, 256, 512, 724, 1023] {
            let exact = (distance as f64).powf(2.1);
            let ours = distance_cost_power(distance) as f64;
            // Whole numbers lose a fraction at the shortest distances, worth nothing against the flat cost.
            assert!((ours - exact).abs() < 1.0 || (ours - exact).abs() / exact < 0.002, "{distance}: {ours} against {exact}");
        }
        assert_eq!(distance_cost_power(0), 0);
    }

    #[test]
    fn teleports_cost_more_and_take_longer_the_farther_they_go() {
        let teleporter = TeleporterDef { flat_energy: Res::from_num(100_000), max_energy: Res::from_num(2_500_000), min_ticks: 200, max_ticks: 500 };
        let (near, near_ticks) = teleport_cost(&teleporter, Fp::from_num(100));
        let (far, far_ticks) = teleport_cost(&teleporter, Fp::from_num(10_000));
        assert!(near < far && near_ticks <= far_ticks);
        // 500 of the inspiration's units: 100,000 + 0.5 * 500^2.1, and 20 + 0.00001 * 500^2.1
        // seconds.
        assert_eq!(far.to_num::<i64>() / 1000, 332);
        assert_eq!(far_ticks, 246);
        assert_eq!(teleport_cost(&teleporter, Fp::ZERO), (Res::from_num(100_000), 200));
    }
}
