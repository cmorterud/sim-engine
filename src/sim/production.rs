//! The flow economy, construction, factories, reclaim, and adjacency
//! (GAME_SPECIFICATION.md §3-6). Everything here runs once per tick in `SimId` order.

use std::collections::BTreeMap;
use std::hash::Hasher;

use super::blueprints::{BlueprintId, Res, Targets, UnitClass, WeaponKind, ADJACENCY_SCALE, TICKS_PER_SECOND};
use super::commands::SimCommand;
use super::components::{
    Adjacency, AntiAir, BeingCaptured, BombBay, Flight, Blueprinted, BuildTask, Builder, Intel, Launcher, Upgrading, MissileDefense, DischargeOrder, DischargeWeapon, Paused, Shield, Turning, Turret, Veterancy, Combat, Construction, Factory, Footprint, Health, Order, OrderKind, OrderQueue, Patrol,
    Position, SimId, Team, UnitStats, Wreck, WreckKind,
};
use super::config::{
    PATROL_SEARCH_INTERVAL, PATROL_SEARCH_RADIUS_UNITS, RECLAIM_ENERGY_PER_POWER, RECLAIM_MASS_PER_POWER, ROCK_MASS_PER_RADIUS, TERRAIN_CELL_UNITS, TREE_ENERGY, TREE_MASS, TREE_RADIUS_UNITS,
};
use super::spatial::{GridEntry, Layer};
use super::world::{BuildQueueView, EconomyView, OrderStopView, PlanStep, UnitRatesView, PlanStepKind, SimWorld, UnitPlanView, UnitSpawn, WorkLinkView};
use crate::math::fp::{fp_int, wide_mul, Fp, FpWide, Vec2Fp};

/// A new site starts with this share of its health, so that it exists to be shot at.
const SITE_STARTING_HEALTH_PERMILLE: i32 = 10;

/// One team's resources. Everything is per tick, except for stored amounts and capacities.
#[derive(Copy, Clone, Debug, Default)]
pub struct Economy {
    pub mass: Res,
    pub energy: Res,
    pub mass_capacity: Res,
    pub energy_capacity: Res,
    /// Storage that comes with the scenario rather than from any structure.
    pub base_mass_capacity: Res,
    pub base_energy_capacity: Res,
    /// Production from finished structures and commanders, updated when they change.
    pub mass_income: Res,
    pub energy_income: Res,
    /// This tick's mass and energy from reclaim.
    pub mass_reclaimed: Res,
    pub energy_reclaimed: Res,
    pub mass_demand: Res,
    pub energy_demand: Res,
    /// The share of demand that was met, from 0 to 1.
    pub satisfaction: Res,
    /// Mass extractors' share of `mass_income`, and the energy they need to make it, per tick.
    pub extractor_income: Res,
    pub extractor_upkeep: Res,
    /// The share of that energy paid this tick, from 0 to 1. Extractors make that share of
    /// their mass, so an energy stall slows them all alike.
    pub extractor_efficiency: Res,
    /// Everything reclaimed so far, for the economy panel. Nothing in the simulation reads it,
    /// so it is left out of the state hash.
    pub mass_reclaimed_total: Res,
    pub energy_reclaimed_total: Res,
}

impl Economy {
    pub fn hash_state(&self, hasher: &mut impl Hasher) {
        for value in [
            self.mass,
            self.energy,
            self.mass_capacity,
            self.energy_capacity,
            self.base_mass_capacity,
            self.base_energy_capacity,
            self.mass_income,
            self.energy_income,
            self.mass_reclaimed,
            self.energy_reclaimed,
            self.mass_demand,
            self.energy_demand,
            self.satisfaction,
            self.extractor_income,
            self.extractor_upkeep,
            self.extractor_efficiency,
        ] {
            hasher.write(&value.to_bits().to_le_bytes());
        }
    }

    /// The mass the extractors are not making this tick for want of energy.
    pub fn extractor_shortfall(&self) -> Res {
        self.extractor_income - self.extractor_income * self.extractor_efficiency
    }

    pub fn view(&self) -> EconomyView {
        let per_second = |value: Res| (value * TICKS_PER_SECOND).to_num::<i64>();
        EconomyView {
            mass: self.mass.to_num(),
            energy: self.energy.to_num(),
            mass_capacity: self.mass_capacity.to_num(),
            energy_capacity: self.energy_capacity.to_num(),
            mass_income: per_second(self.mass_income - self.extractor_shortfall() + self.mass_reclaimed),
            energy_income: per_second(self.energy_income + self.energy_reclaimed),
            mass_demand: per_second(self.mass_demand),
            energy_demand: per_second(self.energy_demand),
            satisfaction_permille: (self.satisfaction.to_bits() * 1000 / Res::ONE.to_bits()).clamp(0, 1000) as u16,
            mass_reclaim: per_second(self.mass_reclaimed),
            energy_reclaim: per_second(self.energy_reclaimed),
            mass_reclaimed: self.mass_reclaimed_total.to_num(),
            energy_reclaimed: self.energy_reclaimed_total.to_num(),
        }
    }
}

/// A builder that worked on something this tick.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WorkLink {
    pub builder: SimId,
    pub target: SimId,
    pub reclaim: bool,
}

/// What a unit of build power is being spent on.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum WorkKind {
    Construct,
    FactoryItem,
    Repair,
    Upgrade,
    Enhance,
    Capture,
}

/// The result of looking at a builder's first task.
enum TaskStep {
    Done,
    Work { target: SimId, kind: WorkKind },
    Reclaim { target: SimId, entity: hecs::Entity },
    MoveTo(Vec2Fp),
    Wait,
}

/// One item's cost per tick at full speed.
struct WorkItem {
    target: SimId,
    entity: hecs::Entity,
    kind: WorkKind,
    team: Team,
    /// The share of the job finished per tick at full speed.
    step: Res,
    mass: Res,
    energy: Res,
}

fn fp_to_res(value: Fp) -> Res {
    Res::from_bits(i64::from(value.to_bits()) << 12)
}

/// A factory's exits, best first: its two open ends, east and west, the one facing `toward`
/// first, then its closed sides, for when both ends are blocked. When two face it equally,
/// the one clockwise of it wins. Both measures turn with the map, so a factory on a mirrored
/// map picks the mirrored exit.
fn exit_order(center: Vec2Fp, toward: Vec2Fp) -> [(i32, i32); 4] {
    let (dx, dy) = (toward.x - center.x, toward.y - center.y);
    let mut exits = [(0, -1), (1, 0), (0, 1), (-1, 0)];
    // With unit sides, the dot and cross products need no multiplication.
    let pick = |x: i32, y: i32, along_x: Fp, along_y: Fp| match (x, y) {
        (1, _) => along_x,
        (-1, _) => -along_x,
        (_, 1) => along_y,
        _ => -along_y,
    };
    exits.sort_by_key(|&(x, y)| (x == 0, std::cmp::Reverse(pick(x, y, dx, dy)), std::cmp::Reverse(pick(x, y, dy, -dx))));
    exits
}

pub(super) fn cell_center(cell_x: u32, cell_y: u32, size: u32) -> Vec2Fp {
    let half_cells = |cell: u32| fp_int(cell as i32 * TERRAIN_CELL_UNITS) + fp_int(size as i32 * TERRAIN_CELL_UNITS) / 2;
    Vec2Fp::new(half_cells(cell_x), half_cells(cell_y))
}

impl SimWorld {
    fn cell_index(&self, cell_x: u32, cell_y: u32) -> u32 {
        cell_y * self.terrain.cells_per_side() as u32 + cell_x
    }

    pub fn blueprint_spawn(&self, blueprint: BlueprintId, team: Team, position: Vec2Fp) -> UnitSpawn {
        let unit = self.blueprints.get(blueprint);
        let weapon = unit.weapon;
        UnitSpawn {
            position,
            team,
            max_speed: unit.speed,
            radius: unit.radius,
            health: unit.health,
            range: weapon.map_or(Fp::ZERO, |weapon| weapon.range),
            reload_ticks: weapon.map_or(0, |weapon| weapon.reload_ticks),
            projectile_speed: weapon.map_or(Fp::ZERO, |weapon| weapon.projectile_speed),
            projectile_damage: weapon.map_or(Fp::ZERO, |weapon| weapon.damage),
            projectile_splash: weapon.map_or(Fp::ZERO, |weapon| weapon.splash_radius),
            is_structure: unit.is_structure(),
            vision: unit.vision,
        }
    }

    /// Spawns a finished mobile unit.
    pub fn spawn_mobile(&mut self, blueprint: BlueprintId, team: Team, position: Vec2Fp) -> SimId {
        self.spawn_mobile_inner(blueprint, team, position, false)
    }

    /// `in_bay`: spawned in a factory's bay, where it could not otherwise be, to drive out.
    fn spawn_mobile_inner(&mut self, blueprint: BlueprintId, team: Team, position: Vec2Fp, in_bay: bool) -> SimId {
        let spawn = self.blueprint_spawn(blueprint, team, position);
        let move_class = self.blueprints.get(blueprint).move_class();
        let id = if in_bay { self.spawn_unit_anywhere(spawn, move_class) } else { self.spawn_unit_as(spawn, move_class) };
        let entity = self.find_unit(id).expect("just spawned").1;
        match move_class {
            super::components::MoveClass::Submarine => self.submarines.push((id, entity)),
            super::components::MoveClass::Seabed => self.seabed_walkers.push((id, entity)),
            _ => {}
        }
        self.attach_blueprint_parts(id, entity, blueprint);
        if self.blueprints.get(blueprint).is_commander() {
            self.commanders[team.index()] = Some(id);
        }
        self.economy_dirty = true;
        id
    }

    /// Sets both teams' starting resources, once their commanders exist.
    pub fn start_economy(&mut self, mass: i64, energy: i64) {
        self.recompute_economy();
        for economy in &mut self.economies {
            economy.mass = Res::from_num(mass).min(economy.mass_capacity);
            economy.energy = Res::from_num(energy).min(economy.energy_capacity);
        }
    }

    /// Whether a structure could start at a cell right now.
    pub fn can_place(&self, blueprint: BlueprintId, cell_x: u32, cell_y: u32) -> bool {
        let unit = self.blueprints.get(blueprint);
        let size = u32::from(unit.footprint);
        if size == 0 {
            return false;
        }
        if unit.requires_deposit && !self.deposits.contains(&(cell_x, cell_y)) {
            return false;
        }
        if unit.requires_vent && !self.vents.contains(&(cell_x + size / 2, cell_y + size / 2)) {
            return false;
        }
        for y in cell_y..cell_y + size {
            for x in cell_x..cell_x + size {
                // A structure on the water needs deep water under all of it, and one on land, land.
                let open = if unit.on_water { self.terrain.cell_is_navigable(x, y) } else { self.terrain.cell_is_open(x, y) };
                if !open {
                    return false;
                }
                // Deposits are saved for extractors, and vents for geothermal plants.
                if !unit.requires_deposit && self.deposits.contains(&(x, y)) {
                    return false;
                }
                if !unit.requires_vent && self.vents.contains(&(x, y)) {
                    return false;
                }
            }
        }
        true
    }

    /// Adds a tree or a rock to the map. A tree is always the same size, and a rock holds
    /// mass in proportion to its radius.
    pub fn spawn_scenery(&mut self, kind: WreckKind, position: Vec2Fp, rock_radius: Fp) -> SimId {
        let (mass, energy, radius) = match kind {
            WreckKind::Tree => (Res::from_num(TREE_MASS), Res::from_num(TREE_ENERGY), fp_int(TREE_RADIUS_UNITS)),
            WreckKind::Rock => (fp_to_res(rock_radius) * ROCK_MASS_PER_RADIUS, Res::ZERO, rock_radius),
            WreckKind::Debris => panic!("debris comes from destroyed units"),
        };
        let id = self.next_id();
        let entity = self.ecs.spawn((id, Wreck { mass, energy, position, radius, kind, health: Fp::ZERO }));
        self.wrecks.push((id, entity));
        id
    }

    /// Trees and rocks on a new structure's ground are cleared, and the team gets what they
    /// held, as though its builder had reclaimed them first. Debris stays where it is.
    fn clear_scenery(&mut self, team: Team, cell_x: u32, cell_y: u32, size: u32) {
        let low = cell_center(cell_x, cell_y, 1) - Vec2Fp::new(fp_int(TERRAIN_CELL_UNITS / 2), fp_int(TERRAIN_CELL_UNITS / 2));
        let high = low + Vec2Fp::new(fp_int(TERRAIN_CELL_UNITS * size as i32), fp_int(TERRAIN_CELL_UNITS * size as i32));
        let mut cleared = Vec::new();
        for &(id, entity) in &self.wrecks {
            let wreck = *self.ecs.get::<&Wreck>(entity).expect("wreck list only holds wrecks");
            let inside = |value: Fp, low: Fp, high: Fp| value + wreck.radius > low && value - wreck.radius < high;
            if wreck.kind.is_scenery() && inside(wreck.position.x, low.x, high.x) && inside(wreck.position.y, low.y, high.y) {
                cleared.push((id, entity, wreck));
            }
        }
        let economy = &mut self.economies[team.index()];
        for &(_, _, wreck) in &cleared {
            economy.mass = (economy.mass + wreck.mass).min(economy.mass_capacity);
            economy.energy = (economy.energy + wreck.energy).min(economy.energy_capacity);
        }
        for (id, entity, _) in cleared {
            self.ecs.despawn(entity).expect("wreck list only holds live wrecks");
            self.wrecks.retain(|wreck| wreck.0 != id);
        }
    }

    /// Starts a structure at a cell. It occupies the ground right away.
    pub fn spawn_site(&mut self, blueprint: BlueprintId, team: Team, cell_x: u32, cell_y: u32) -> Option<SimId> {
        self.place_structure(blueprint, team, cell_x, cell_y, false)
    }

    /// Places a finished structure, for scenarios and tests.
    pub fn spawn_finished_structure(&mut self, blueprint: BlueprintId, team: Team, cell_x: u32, cell_y: u32) -> Option<SimId> {
        self.place_structure(blueprint, team, cell_x, cell_y, true)
    }

    fn place_structure(&mut self, blueprint: BlueprintId, team: Team, cell_x: u32, cell_y: u32, finished: bool) -> Option<SimId> {
        if !self.can_place(blueprint, cell_x, cell_y) {
            return None;
        }
        let size = u32::from(self.blueprints.get(blueprint).footprint);
        self.clear_scenery(team, cell_x, cell_y, size);
        let center = cell_center(cell_x, cell_y, size);
        let spawn = self.blueprint_spawn(blueprint, team, center);
        // A structure on the water floats on it, as ships do.
        let class = if self.blueprints.get(blueprint).on_water { super::components::MoveClass::Naval } else { super::components::MoveClass::Land };
        let id = self.spawn_unit_as(spawn, class);
        let entity = self.find_unit(id).expect("just spawned").1;
        self.attach_blueprint_parts(id, entity, blueprint);
        self.ecs
            .insert(entity, (Footprint { cell_x, cell_y, size }, Adjacency::default()))
            .expect("entity was just spawned");
        self.structures.push((id, entity));
        self.terrain.set_structure_cells(cell_x, cell_y, size, true);
        self.flow_field.invalidate_cells(cell_x, cell_y, size);
        for y in cell_y..cell_y + size {
            for x in cell_x..cell_x + size {
                let cell = self.cell_index(x, y);
                self.occupancy.insert(cell, id);
            }
        }
        if finished {
            self.economy_dirty = true;
        } else {
            let mut health = self.ecs.get::<&mut Health>(entity).expect("units have health");
            health.current = (health.max * SITE_STARTING_HEALTH_PERMILLE / 1000).max(Fp::ONE);
            drop(health);
            self.ecs.get::<&mut Combat>(entity).expect("units have combat").enabled = false;
            self.ecs.insert_one(entity, Construction { progress: Res::ZERO }).expect("entity was just spawned");
        }
        Some(id)
    }

    fn attach_blueprint_parts(&mut self, id: SimId, entity: hecs::Entity, blueprint: BlueprintId) {
        let unit = self.blueprints.get(blueprint);
        let (power, range, is_factory) = (unit.build_power, unit.build_range, unit.is_factory());
        self.ecs.insert_one(entity, Blueprinted(blueprint)).expect("entity was just spawned");
        // Each side starts out facing the middle of the map, as it is drawn.
        let half = Fp::from_bits(2_896);
        let team = self.ecs.get::<&UnitStats>(entity).expect("units have stats").team;
        let facing = match team {
            Team::Blue => Vec2Fp::new(half, half),
            Team::Red => Vec2Fp::new(-half, -half),
        };
        if let (Some(rate), false) = (unit.turn_rate, unit.is_structure()) {
            let radius = fp_int(unit.turn_radius as i32);
            self.ecs.insert_one(entity, Turning::new(facing, rate, radius)).expect("entity was just spawned");
        }
        if let Some(discharge) = unit.discharge {
            let weapon = DischargeWeapon { def: discharge, ticks_until_ready: 0 };
            self.ecs.insert_one(entity, weapon).expect("entity was just spawned");
        }
        if let Some(flight) = unit.flight {
            self.ecs.insert_one(entity, Turning::new(facing, flight.turn_rate, Fp::ZERO)).expect("entity was just spawned");
            let airborne = Flight {
                elevation: flight.elevation,
                altitude: flight.elevation,
                speed: unit.speed,
                fuel: flight.fuel_ticks,
                max_fuel: flight.fuel_ticks,
                size: match unit.tier {
                    0 | 1 => 1,
                    2 => 2,
                    _ => 4,
                },
                refit: None,
                docked: false,
                pad: 0,
                hovers: flight.hovers,
                break_off: flight.break_off,
                breaking_off: false,
            };
            self.ecs.insert_one(entity, airborne).expect("entity was just spawned");
            let mut position = self.ecs.get::<&mut Position>(entity).expect("units have positions");
            position.0.z = super::air::flying_height(&self.terrain, position.0.xy(), flight.elevation);
            drop(position);
            self.aircraft.push((id, entity));
        }
        match unit.weapon.map(|weapon| weapon.targets) {
            Some(Targets::Air) => self.ecs.insert_one(entity, AntiAir { also_ground: false }).expect("entity was just spawned"),
            Some(Targets::Both) => self.ecs.insert_one(entity, AntiAir { also_ground: true }).expect("entity was just spawned"),
            Some(Targets::Naval) => self.ecs.insert_one(entity, super::components::Torpedo).expect("entity was just spawned"),
            Some(Targets::Ground) | None => {}
        }
        if unit.weapon.is_some_and(|weapon| weapon.kind == WeaponKind::Bomb) {
            self.ecs.insert_one(entity, BombBay).expect("entity was just spawned");
        }
        if !unit.extra_weapons.is_empty() {
            let extras = unit
                .extra_weapons
                .iter()
                .map(|weapon| super::components::ExtraWeapon {
                    range: weapon.range,
                    reload_ticks: weapon.reload_ticks,
                    ticks_until_ready: 0,
                    projectile_speed: weapon.projectile_speed,
                    damage: weapon.damage,
                    splash: weapon.splash_radius,
                    hits_air: matches!(weapon.targets, Targets::Air | Targets::Both),
                    hits_ground: matches!(weapon.targets, Targets::Ground | Targets::Both),
                    torpedo: weapon.targets == Targets::Naval,
                    missile_height: (weapon.kind == WeaponKind::TacticalMissile).then_some(weapon.cruise_height),
                    target: None,
                    turret: weapon.turn_rate.map(|rate| Turret::new(facing, rate)),
                    spread: weapon.spread,
                    stun_ticks: weapon.stun_ticks,
                })
                .collect();
            self.ecs.insert_one(entity, super::components::ExtraWeapons(extras)).expect("entity was just spawned");
        }
        if let Some(weapon) = unit.weapon.filter(|weapon| !weapon.artillery && weapon.spread > Fp::ZERO) {
            self.ecs.insert_one(entity, super::components::Scatter(weapon.spread)).expect("entity was just spawned");
        }
        if let Some(weapon) = unit.weapon.filter(|weapon| weapon.artillery) {
            let artillery = super::components::Artillery { min_range: weapon.min_range, spread: weapon.spread, energy_per_shot: weapon.energy_per_shot };
            self.ecs.insert_one(entity, artillery).expect("entity was just spawned");
        }
        if let Some(weapon) = unit.weapon.filter(|weapon| weapon.stun_ticks > 0) {
            self.ecs.insert_one(entity, super::components::StunWeapon(weapon.stun_ticks)).expect("entity was just spawned");
        }
        if let Some(weapon) = unit.weapon.filter(|weapon| weapon.kind == WeaponKind::Tractor) {
            let beam = super::components::TractorBeam { crush_ticks: weapon.crush_ticks.max(1), holding: None };
            self.ecs.insert_one(entity, beam).expect("entity was just spawned");
            self.tractors.push((id, entity));
        }
        if let Some(rate) = unit.weapon.and_then(|weapon| weapon.turn_rate) {
            self.ecs.insert_one(entity, Turret::new(facing, rate)).expect("entity was just spawned");
        }
        if let Some(weapon) = unit.weapon.filter(|weapon| matches!(weapon.kind, WeaponKind::TacticalMissile | WeaponKind::StrategicMissile)) {
            let launcher = Launcher {
                strategic: weapon.kind == WeaponKind::StrategicMissile,
                cruise_height: weapon.cruise_height,
                stock: 0,
                inner_radius: weapon.inner_radius,
                inner_damage: weapon.inner_damage,
                automatic: weapon.automatic,
            };
            self.ecs.insert_one(entity, launcher).expect("entity was just spawned");
        }
        if unit.has_intel() {
            // Without upkeep, it works from the start. With upkeep, once it is first paid for.
            let intel = Intel { running: unit.upkeep == Res::ZERO, cloak_on: false, cloaked: false, stealthed: false, jamming: false };
            self.ecs.insert_one(entity, intel).expect("entity was just spawned");
            self.intel.push((id, entity));
        }
        if let Some(def) = unit.missile_defense {
            let defense = MissileDefense { strategic: def.strategic, range: def.range, reload_ticks: def.reload_ticks, ticks_until_ready: 0, stock: 0 };
            self.ecs.insert_one(entity, defense).expect("entity was just spawned");
            self.missile_defenses.push((id, entity));
        }
        if let Some(def) = unit.torpedo_defense {
            let defense = super::components::TorpedoDefense { range: def.range, reload_ticks: def.reload_ticks, ticks_until_ready: 0 };
            self.ecs.insert_one(entity, defense).expect("entity was just spawned");
            self.torpedo_defenses.push((id, entity));
        }
        if let Some(def) = unit.shield {
            let shield = Shield {
                radius: def.radius,
                health: def.health,
                max_health: def.health,
                regen_per_tick: def.regen_per_tick,
                recharging: 0,
                recharge_ticks: def.recharge_ticks,
                upkeep: def.upkeep,
            };
            self.ecs.insert_one(entity, shield).expect("entity was just spawned");
            self.shields.push((id, entity));
        }
        if unit.airfield.is_some() {
            self.airfields.push((id, entity));
        }
        if unit.transport.is_some() {
            self.ecs.insert_one(entity, super::components::Transport::default()).expect("entity was just spawned");
            self.transports.push((id, entity));
        }
        if is_factory {
            let factory = Factory { power, queue: Vec::new(), repeat: false, progress: Res::ZERO, rally: None };
            self.ecs.insert_one(entity, factory).expect("entity was just spawned");
            self.factories.push((id, entity));
        } else if power > Fp::ZERO && (!unit.is_structure() || unit.assists) {
            // A structure's build power only drives its own upgrades, but for an engineering
            // station's, which helps whatever is in reach.
            self.ecs.insert_one(entity, Builder { power, range, tasks: Vec::new() }).expect("entity was just spawned");
            self.builders.push((id, entity));
        }
    }

    pub(super) fn clear_footprint(&mut self, footprint: Footprint) {
        let Footprint { cell_x, cell_y, size } = footprint;
        self.terrain.set_structure_cells(cell_x, cell_y, size, false);
        self.flow_field.invalidate_cells(cell_x, cell_y, size);
        for y in cell_y..cell_y + size {
            for x in cell_x..cell_x + size {
                let cell = self.cell_index(x, y);
                self.occupancy.remove(&cell);
            }
        }
    }

    pub(super) fn apply_production_command(&mut self, command: &SimCommand) {
        match command {
            SimCommand::Patrol { units, points, queued } => {
                if points.is_empty() {
                    return;
                }
                if !*queued {
                    self.clear_unit_plans(units);
                }
                for &id in units {
                    if self.find_unit(id).is_none() {
                        continue;
                    }
                    let (_, entity) = self.find_unit(id).expect("just checked");
                    if *queued {
                        // A patrol never ends by itself, so one queued after another replaces it.
                        self.end_patrol(id, entity);
                    } else {
                        self.set_order(id, Order::IDLE);
                    }
                    self.ecs.insert_one(entity, Patrol { points: points.clone(), next: 0 }).expect("unit is live");
                    // Keep the list sorted, since it is usually a small, recent change.
                    let at = self.patrols.partition_point(|patrol| patrol.0 < id);
                    self.patrols.insert(at, (id, entity));
                }
            }
            SimCommand::Build { units, blueprint, cell_x, cell_y, queued } => {
                let builders = self.units_that_can_build(units, *blueprint);
                // A commander being fitted with an fabrication pack can already queue what the
                // suite unlocks. That waits behind the suite, so it is queued whatever the order
                // says, and never cancels the suite.
                let after_suite: Vec<SimId> = units
                    .iter()
                    .copied()
                    .filter(|id| {
                        self.find_unit(*id).is_some_and(|(_, entity)| {
                            let team = self.ecs.get::<&UnitStats>(entity).expect("units have stats").team;
                            self.ecs.get::<&Builder>(entity).is_ok() && self.can_build_after_suite(entity, *blueprint) && self.support_allowed(team, *blueprint)
                        })
                    })
                    .collect();
                self.push_task(&after_suite, BuildTask::Build { blueprint: *blueprint, cell_x: *cell_x, cell_y: *cell_y, site: None });
                if !*queued {
                    self.clear_unit_plans(&builders);
                    for &id in &builders {
                        self.set_order(id, Order::IDLE);
                    }
                }
                let task = BuildTask::Build { blueprint: *blueprint, cell_x: *cell_x, cell_y: *cell_y, site: None };
                if *queued {
                    for &id in &builders {
                        self.adopt_current_order(id);
                    }
                }
                self.push_task(&builders, task);
                // The other mobile builders in the selection, which cannot build it, drop what they
                // were doing and help the first that can: told to build something new, a selection
                // builds that, and nothing else goes on being built.
                if !*queued {
                    if let Some(&lead) = builders.first() {
                        let helpers: Vec<SimId> = units
                            .iter()
                            .copied()
                            .filter(|id| !builders.contains(id) && !after_suite.contains(id))
                            .filter(|id| {
                                self.find_unit(*id).is_some_and(|(_, entity)| {
                                    self.ecs.get::<&Builder>(entity).is_ok() && self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| !stats.is_structure)
                                })
                            })
                            .collect();
                        self.replace_tasks(&helpers, BuildTask::Assist { target: lead });
                    }
                }
            }
            SimCommand::MoveBuild { units, task, cell_x, cell_y } => {
                let BuildTask::Build { blueprint, .. } = *task else {
                    return;
                };
                for &id in units {
                    let Some((_, entity)) = self.find_unit(id) else {
                        continue;
                    };
                    let Ok(mut builder) = self.ecs.get::<&mut Builder>(entity) else {
                        continue;
                    };
                    // A site that has been started stays where it is.
                    let at = builder.tasks.iter().position(|other| other.is_same_order(task) && matches!(other, BuildTask::Build { site: None, .. }));
                    let Some(at) = at else {
                        continue;
                    };
                    builder.tasks[at] = BuildTask::Build { blueprint, cell_x: *cell_x, cell_y: *cell_y, site: None };
                    drop(builder);
                    // It may be walking to the old spot.
                    if at == 0 {
                        self.set_order(id, Order::IDLE);
                    }
                }
            }
            SimCommand::QueueTask { units, task } => {
                let builders: Vec<SimId> = units
                    .iter()
                    .copied()
                    .filter(|id| self.find_unit(*id).is_some_and(|(_, entity)| self.ecs.get::<&Builder>(entity).is_ok()))
                    .collect();
                for &id in &builders {
                    let (_, entity) = self.find_unit(id).expect("just checked");
                    self.end_patrol(id, entity);
                    self.adopt_current_order(id);
                }
                self.push_task(&builders, *task);
            }
            SimCommand::Assist { units, target } => self.replace_tasks(units, BuildTask::Assist { target: *target }),
            SimCommand::Reclaim { units, target } => self.replace_tasks(units, BuildTask::Reclaim { target: *target }),
            SimCommand::Repair { units, target } => self.replace_tasks(units, BuildTask::Repair { target: *target }),
            SimCommand::Capture { units, target } => self.replace_tasks(units, BuildTask::Capture { target: *target }),
            SimCommand::QueueUnit { factory, blueprint, count } => {
                let Some(entity) = self.factory_entity(*factory) else {
                    return;
                };
                let current = self.ecs.get::<&Blueprinted>(entity).expect("factories have blueprints").0;
                let unit = self.blueprints.get(current);
                let is_upgrade = unit.upgrades_to == Some(*blueprint);
                let mut factory = self.ecs.get::<&mut Factory>(entity).expect("factory list only holds factories");
                // Always added at the end, so it can only come out after the upgrade has finished.
                let after_upgrade = unit
                    .upgrades_to
                    .is_some_and(|upgrade| factory.queue.contains(&upgrade) && self.blueprints.get(upgrade).builds.contains(blueprint));
                if unit.builds.contains(blueprint) || is_upgrade || after_upgrade {
                    if is_upgrade {
                        // A factory only upgrades once, however many times it is asked.
                        if !factory.queue.contains(blueprint) {
                            factory.queue.push(*blueprint);
                        }
                    } else {
                        factory.queue.extend(std::iter::repeat_n(*blueprint, (*count).min(100) as usize));
                    }
                }
            }
            SimCommand::ClearQueue { factory } => {
                if let Some(entity) = self.factory_entity(*factory) {
                    let mut factory = self.ecs.get::<&mut Factory>(entity).expect("factory list only holds factories");
                    factory.queue.clear();
                    factory.progress = Res::ZERO;
                }
            }
            SimCommand::SetPaused { units, paused } => {
                for &id in units {
                    let Some((_, entity)) = self.find_unit(id) else {
                        continue;
                    };
                    let has_upkeep = self.ecs.get::<&Blueprinted>(entity).is_ok_and(|unit| self.blueprints.get(unit.0).upkeep > Res::ZERO);
                    let pausable = self.ecs.get::<&Factory>(entity).is_ok() || self.ecs.get::<&Construction>(entity).is_ok() || self.ecs.get::<&Shield>(entity).is_ok() || has_upkeep;
                    if !pausable {
                        continue;
                    }
                    if *paused {
                        self.ecs.insert_one(entity, Paused).expect("unit is live");
                    } else {
                        let _ = self.ecs.remove_one::<Paused>(entity);
                    }
                    // A paused extractor makes nothing.
                    self.economy_dirty = true;
                }
            }
            SimCommand::CancelTask { units, task } => {
                for &id in units {
                    let Some((_, entity)) = self.find_unit(id) else {
                        continue;
                    };
                    let Ok(mut builder) = self.ecs.get::<&mut Builder>(entity) else {
                        self.cancel_queued_walk(id, entity, task);
                        continue;
                    };
                    let Some(index) = builder.tasks.iter().position(|other| other.is_same_order(task)) else {
                        continue;
                    };
                    let removed = builder.tasks.remove(index);
                    drop(builder);
                    if let BuildTask::Walk { order, started: false } = removed {
                        self.flow_field.release(order.route);
                    }
                    // It may be walking to that order. The next tick starts it on whatever is left.
                    if index == 0 {
                        self.set_order(id, Order::IDLE);
                    }
                }
            }
            SimCommand::RemoveQueued { factory, blueprint } => {
                if let Some(entity) = self.factory_entity(*factory) {
                    let mut factory = self.ecs.get::<&mut Factory>(entity).expect("factory list only holds factories");
                    if let Some(index) = factory.queue.iter().rposition(|item| item == blueprint) {
                        factory.queue.remove(index);
                        if index == 0 {
                            factory.progress = Res::ZERO;
                        }
                        // Without its upgrade, a factory cannot build what was waiting for it.
                        if self.blueprints.get(*blueprint).is_factory() {
                            let current = self.ecs.get::<&Blueprinted>(entity).expect("factories have blueprints").0;
                            let builds = &self.blueprints.get(current).builds;
                            factory.queue.retain(|item| builds.contains(item));
                        }
                    }
                }
            }
            SimCommand::SetCloak { units, on } => {
                for &id in units {
                    let Some((_, entity)) = self.find_unit(id) else {
                        continue;
                    };
                    if let Ok(mut intel) = self.ecs.get::<&mut Intel>(entity) {
                        intel.cloak_on = *on;
                        intel.cloaked &= *on;
                    }
                }
            }
            SimCommand::SetSurfaced { units, surfaced } => {
                for &id in units {
                    let is_sub = self.find_unit(id).is_some_and(|(_, entity)| self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.move_class == super::components::MoveClass::Submarine));
                    if !is_sub {
                        continue;
                    }
                    match (self.surfaced.binary_search(&id), *surfaced) {
                        (Err(at), true) => self.surfaced.insert(at, id),
                        (Ok(at), false) => {
                            self.surfaced.remove(at);
                        }
                        _ => {}
                    }
                    // Up onto the water, or back down below it, at once, as movement would put it.
                    let (_, entity) = self.find_unit(id).expect("just checked");
                    let radius = self.ecs.get::<&UnitStats>(entity).expect("units have stats").radius;
                    let water = self.terrain.water_level();
                    let height = if *surfaced { water + radius } else { water - radius };
                    self.ecs.get::<&mut super::components::Position>(entity).expect("units have positions").0.z = height;
                }
            }
            SimCommand::Upgrade { units } => {
                for &id in units {
                    self.start_upgrade(id);
                }
            }
            SimCommand::SetRepeat { factory, repeat } => {
                if let Some(entity) = self.factory_entity(*factory) {
                    self.ecs.get::<&mut Factory>(entity).expect("factory list only holds factories").repeat = *repeat;
                }
            }
            SimCommand::SetRally { factory, point } => {
                if let Some(entity) = self.factory_entity(*factory) {
                    self.ecs.get::<&mut Factory>(entity).expect("factory list only holds factories").rally = Some(*point);
                }
            }
            SimCommand::Move { .. }
            | SimCommand::Enhance { .. }
            | SimCommand::Teleport { .. }
            | SimCommand::SelfDestruct { .. }
            | SimCommand::Refit { .. }
            | SimCommand::Load { .. }
            | SimCommand::Unload { .. }
            | SimCommand::AttackMove { .. }
            | SimCommand::QueueMove { .. }
            | SimCommand::FormationMove { .. }
            | SimCommand::AttackGround { .. }
            | SimCommand::Discharge { .. }
            | SimCommand::MoveOrder { .. }
            | SimCommand::Follow { .. }
            | SimCommand::Stop { .. }
            | SimCommand::Attack { .. } => {}
        }
    }

    /// Keeps a builder's current walk as the first thing in its queue, so that what is queued
    /// next waits for it, even if the next job happens to be within reach already.
    /// Starts a unit upgrading to what its blueprint upgrades to, if it can and is not already.
    fn start_upgrade(&mut self, id: SimId) {
        let Some((_, entity)) = self.find_unit(id) else {
            return;
        };
        let Ok(current) = self.ecs.get::<&Blueprinted>(entity).map(|unit| unit.0) else {
            return;
        };
        let Some(into) = self.blueprints.get(current).upgrades_to else {
            return;
        };
        if self.ecs.get::<&Factory>(entity).is_ok() {
            self.apply_production_command(&SimCommand::QueueUnit { factory: id, blueprint: into, count: 1 });
            return;
        }
        if self.ecs.get::<&Construction>(entity).is_ok() || self.ecs.get::<&Upgrading>(entity).is_ok() {
            return;
        }
        // A commander drops what it was doing, and stands still.
        if self.ecs.get::<&Builder>(entity).is_ok() {
            self.clear_unit_plans(&[id]);
            self.set_order(id, Order::IDLE);
        }
        self.ecs.insert_one(entity, Upgrading { into, progress: Res::ZERO }).expect("unit is live");
        let at = self.upgrading.partition_point(|unit| unit.0 < id);
        self.upgrading.insert(at, (id, entity));
    }

    /// The build power a unit puts into its own upgrade.
    fn upgrade_power(&self, entity: hecs::Entity) -> Fp {
        match self.ecs.get::<&Builder>(entity) {
            Ok(builder) => builder.power,
            Err(_) => self.ecs.get::<&Blueprinted>(entity).map_or(Fp::ZERO, |unit| self.blueprints.get(unit.0).build_power),
        }
    }

    /// Takes on the upgraded blueprint, at the same share of health.
    fn finish_upgrade(&mut self, id: SimId, entity: hecs::Entity, into: BlueprintId) {
        let unit = self.blueprints.get(into);
        let (power, max_health, vision) = (unit.build_power, unit.health, unit.vision);
        self.ecs.get::<&mut Blueprinted>(entity).expect("upgrades have blueprints").0 = into;
        if let Ok(mut builder) = self.ecs.get::<&mut Builder>(entity) {
            builder.power = power;
        }
        self.ecs.get::<&mut UnitStats>(entity).expect("units have stats").vision = vision;
        let mut health = self.ecs.get::<&mut Health>(entity).expect("units have health");
        let share = i64::from(health.current.to_bits()) * i64::from(max_health.to_bits()) / i64::from(health.max.to_bits().max(1));
        health.current = Fp::from_bits(share as i32);
        health.max = max_health;
        drop(health);
        self.ecs.remove_one::<Upgrading>(entity).expect("it was upgrading");
        self.upgrading.retain(|unit| unit.0 != id);
        self.economy_dirty = true;
    }

    pub(super) fn adopt_current_order(&mut self, id: SimId) {
        let Some((_, entity)) = self.find_unit(id) else {
            return;
        };
        let order = *self.ecs.get::<&Order>(entity).expect("units have orders");
        if let Ok(mut builder) = self.ecs.get::<&mut Builder>(entity) {
            if builder.tasks.is_empty() && matches!(order.kind, OrderKind::Move | OrderKind::AttackMove) {
                builder.tasks.push(BuildTask::Walk { order, started: true });
            }
        }
    }

    /// Removes a walk from a unit that is not a builder, whether waiting or under way.
    fn cancel_queued_walk(&mut self, id: SimId, entity: hecs::Entity, task: &BuildTask) {
        let BuildTask::Walk { order: cancelled, .. } = task else {
            return;
        };
        let removed = self.ecs.get::<&mut OrderQueue>(entity).ok().and_then(|mut queue| {
            let index = queue.0.iter().position(|order| order.route == cancelled.route)?;
            queue.0.remove(index);
            Some(queue.0.is_empty())
        });
        if let Some(empty) = removed {
            self.flow_field.release(cancelled.route);
            if empty {
                self.ecs.remove_one::<OrderQueue>(entity).expect("just checked");
                self.order_queues.retain(|queue| queue.0 != id);
            }
            return;
        }
        let current = *self.ecs.get::<&Order>(entity).expect("units have orders");
        if current.kind != OrderKind::Idle && current.route == cancelled.route {
            self.set_order(id, Order::IDLE);
        }
    }

    fn factory_entity(&self, id: SimId) -> Option<hecs::Entity> {
        self.factories.binary_search_by_key(&id, |factory| factory.0).ok().map(|index| self.factories[index].1)
    }

    /// The highest tier of finished headquarters of a kind the team has, and at least 1, since
    /// any factory builds T1.
    pub fn headquarters_tier(&self, team: Team, kind: super::blueprints::FactoryKind) -> u8 {
        self.factories
            .iter()
            .filter(|&&(_, entity)| self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team == team) && self.ecs.get::<&Construction>(entity).is_err())
            .filter_map(|&(_, entity)| {
                let unit = self.blueprints.get(self.ecs.get::<&Blueprinted>(entity).ok()?.0);
                (unit.is_headquarters() && unit.factory_kind == Some(kind)).then_some(unit.tier)
            })
            .max()
            .unwrap_or(1)
    }

    /// Whether the team may have this: anything but a support factory, which needs a
    /// headquarters of its kind of at least its tier.
    pub fn support_allowed(&self, team: Team, blueprint: BlueprintId) -> bool {
        let unit = self.blueprints.get(blueprint);
        match (unit.support, unit.factory_kind) {
            (true, Some(kind)) => self.headquarters_tier(team, kind) >= unit.tier,
            _ => true,
        }
    }

    /// Whether a factory may work on an item now. A support factory builds nothing above its
    /// headquarters' tier, and upgrades only to a tier its headquarters has reached.
    fn factory_may_build(&self, entity: hecs::Entity, item: BlueprintId) -> bool {
        let Ok(blueprint) = self.ecs.get::<&Blueprinted>(entity).map(|unit| unit.0) else {
            return false;
        };
        let unit = self.blueprints.get(blueprint);
        let (true, Some(kind)) = (unit.support, unit.factory_kind) else {
            return true;
        };
        let team = self.ecs.get::<&UnitStats>(entity).expect("units have stats").team;
        self.blueprints.get(item).tier <= self.headquarters_tier(team, kind)
    }

    fn units_that_can_build(&self, units: &[SimId], blueprint: BlueprintId) -> Vec<SimId> {
        units
            .iter()
            .copied()
            .filter(|id| {
                self.find_unit(*id).is_some_and(|(_, entity)| {
                    let team = self.ecs.get::<&UnitStats>(entity).expect("units have stats").team;
                    self.ecs.get::<&Builder>(entity).is_ok() && self.can_build(entity, blueprint) && self.support_allowed(team, blueprint)
                })
            })
            .collect()
    }

    fn push_task(&mut self, units: &[SimId], task: BuildTask) {
        for &id in units {
            if let Some((_, entity)) = self.find_unit(id) {
                if let Ok(mut builder) = self.ecs.get::<&mut Builder>(entity) {
                    builder.tasks.push(task);
                }
            }
        }
    }

    fn replace_tasks(&mut self, units: &[SimId], task: BuildTask) {
        let builders: Vec<SimId> = units
            .iter()
            .copied()
            .filter(|id| self.find_unit(*id).is_some_and(|(_, entity)| self.ecs.get::<&Builder>(entity).is_ok()))
            .collect();
        self.clear_unit_plans(&builders);
        for &id in &builders {
            self.set_order(id, Order::IDLE);
        }
        self.push_task(&builders, task);
    }

    fn within(&self, here: Vec2Fp, there: Vec2Fp, range: Fp) -> bool {
        here.distance_squared(&there) <= wide_mul(range, range)
    }

    /// Runs one tick of the economy and everything that spends from it.
    pub fn step_production(&mut self) {
        self.work_links.clear();
        self.factory_spending.clear();
        if !self.self_destructing.is_empty() {
            self.step_self_destructs();
        }
        // Before anything else: the last builder dying still takes its income with it.
        if self.economy_dirty {
            self.recompute_economy();
        }
        if self.builders.is_empty() && self.factories.is_empty() && self.structures.is_empty() && self.patrols.is_empty() {
            return;
        }
        self.step_patrols();
        self.step_stations();

        for economy in &mut self.economies {
            economy.mass_reclaimed = Res::ZERO;
            economy.energy_reclaimed = Res::ZERO;
        }
        // Shields are paid for first, then intel, out of what is stored and coming in, and
        // building gets the rest.
        let upkeep = self.step_shields();
        let upkeep = self.step_intel_upkeep(upkeep);
        let upkeep = if self.teleporting.is_empty() { upkeep } else { self.step_teleports(upkeep) };
        let upkeep = self.pay_extractors(upkeep);
        let mut power_by_target: BTreeMap<(SimId, WorkKind), Fp> = BTreeMap::new();
        for index in 0..self.builders.len() {
            let (id, entity) = self.builders[index];
            self.step_builder(id, entity, &mut power_by_target);
        }
        for &(id, entity) in &self.upgrading {
            *power_by_target.entry((id, WorkKind::Upgrade)).or_insert(Fp::ZERO) += self.upgrade_power(entity);
        }
        for &(id, entity) in &self.enhancing {
            *power_by_target.entry((id, WorkKind::Enhance)).or_insert(Fp::ZERO) += self.upgrade_power(entity);
        }
        for &(id, entity) in &self.factories {
            let has_item = self.ecs.get::<&Factory>(entity).is_ok_and(|factory| !factory.queue.is_empty());
            if has_item && self.ecs.get::<&Construction>(entity).is_err() {
                let power = self.ecs.get::<&Factory>(entity).expect("factory list only holds factories").power;
                *power_by_target.entry((id, WorkKind::FactoryItem)).or_insert(Fp::ZERO) += power;
            }
        }

        // Paused factories and sites take nothing, and nobody works on them meanwhile.
        let is_paused = |world: &Self, target: SimId| world.find_unit(target).is_some_and(|(_, entity)| world.ecs.get::<&Paused>(entity).is_ok());
        power_by_target.retain(|&(target, kind), _| kind == WorkKind::Repair || !is_paused(self, target));
        let mut links = std::mem::take(&mut self.work_links);
        links.retain(|link| link.reclaim || !is_paused(self, link.target));
        self.work_links = links;

        // Work out what everything costs at full speed.
        let mut items = Vec::with_capacity(power_by_target.len());
        for (&(target, kind), &power) in &power_by_target {
            if let Some(item) = self.work_item(target, kind, power) {
                items.push(item);
            }
        }
        let mut demand = [(Res::ZERO, Res::ZERO); 2];
        for item in &items {
            demand[item.team.index()].0 += item.mass;
            demand[item.team.index()].1 += item.energy;
        }

        // When demand exceeds supply, everything slows by the same share. A team is
        // limited by whichever resource is scarcer, so both are spent in step.
        let mut satisfaction = [Res::ONE; 2];
        for (team_index, economy) in self.economies.iter_mut().enumerate() {
            let (mass_demand, energy_demand) = demand[team_index];
            let mass_supply = economy.mass + economy.mass_income - economy.extractor_shortfall() + economy.mass_reclaimed;
            let energy_supply = economy.energy + economy.energy_income + economy.energy_reclaimed - upkeep[team_index];
            let share = |supply: Res, wanted: Res| {
                if wanted <= supply || wanted == Res::ZERO {
                    Res::ONE
                } else {
                    (supply / wanted).clamp(Res::ZERO, Res::ONE)
                }
            };
            satisfaction[team_index] = share(mass_supply, mass_demand).min(share(energy_supply, energy_demand));
            economy.mass_demand = mass_demand;
            economy.energy_demand = energy_demand + upkeep[team_index];
            economy.satisfaction = satisfaction[team_index];
        }

        let mut spent = [(Res::ZERO, Res::ZERO); 2];
        for item in &items {
            let team = item.team.index();
            let share = satisfaction[team];
            spent[team].0 += item.mass * share;
            spent[team].1 += item.energy * share;
            if item.kind == WorkKind::FactoryItem {
                self.factory_spending.push((item.target, item.mass * share, item.energy * share));
            }
            self.apply_work(item, item.step * share);
        }
        if !self.being_captured.is_empty() {
            self.drop_stopped_captures();
        }
        for (team_index, economy) in self.economies.iter_mut().enumerate() {
            let mass = economy.mass + economy.mass_income - economy.extractor_shortfall() + economy.mass_reclaimed - spent[team_index].0;
            let energy = economy.energy + economy.energy_income + economy.energy_reclaimed - spent[team_index].1 - upkeep[team_index];
            economy.mass = mass.clamp(Res::ZERO, economy.mass_capacity);
            economy.energy = energy.clamp(Res::ZERO, economy.energy_capacity);
        }
    }

    /// Pays the extractors' energy, after shields, intel, and teleports, out of what is stored and
    /// coming in. What cannot be paid slows every extractor of the team by the same share.
    /// Returns what each team has paid, everything before included.
    fn pay_extractors(&mut self, mut paid: [Res; 2]) -> [Res; 2] {
        for (team, economy) in self.economies.iter_mut().enumerate() {
            if economy.extractor_upkeep == Res::ZERO {
                economy.extractor_efficiency = Res::ONE;
                continue;
            }
            let available = (economy.energy + economy.energy_income + economy.energy_reclaimed - paid[team]).max(Res::ZERO);
            economy.extractor_efficiency = (available / economy.extractor_upkeep).min(Res::ONE);
            paid[team] += economy.extractor_upkeep * economy.extractor_efficiency;
        }
        paid
    }

    /// Counts down collapsed domes, and for those that are up, takes their upkeep and lets them
    /// heal. A team that cannot pay for a dome loses it, so its shields fail one by one, in
    /// `SimId` order, as its energy runs out. Paused generators and sites neither pay nor heal.
    /// Returns what each team paid this tick.
    fn step_shields(&mut self) -> [Res; 2] {
        let mut paid = [Res::ZERO; 2];
        for &(_, entity) in &self.shields {
            if self.ecs.get::<&Construction>(entity).is_ok() || self.ecs.get::<&Paused>(entity).is_ok() {
                continue;
            }
            let team = self.ecs.get::<&UnitStats>(entity).expect("units have stats").team.index();
            let discount = self.ecs.get::<&Adjacency>(entity).map_or(0, |adjacency| adjacency.upkeep_discount);
            let mut shield = self.ecs.get::<&mut Shield>(entity).expect("shield list only holds shields");
            if !shield.is_up() {
                shield.recharging -= 1;
                if shield.is_up() {
                    shield.health = shield.max_health;
                }
                continue;
            }
            let cost = Adjacency::less(shield.upkeep, discount);
            let economy = &self.economies[team];
            if economy.energy + economy.energy_income - paid[team] < cost {
                shield.collapse();
                continue;
            }
            paid[team] += cost;
            shield.health = (shield.health + shield.regen_per_tick).min(shield.max_health);
        }
        paid
    }

    /// Pays for intel, in `SimId` order, after shields: a structure's sensors, field, and
    /// jamming run while paid for, and a cloak that cannot be paid for switches off. Paused and
    /// unfinished structures neither pay nor run. Returns what each team has paid, shields included.
    fn step_intel_upkeep(&mut self, mut paid: [Res; 2]) -> [Res; 2] {
        for &(id, entity) in &self.intel {
            let blueprint = self.blueprints.get(self.ecs.get::<&Blueprinted>(entity).expect("intel units have blueprints").0);
            let (upkeep, cloak_upkeep) = (blueprint.upkeep, self.cloak_upkeep(entity).unwrap_or(Res::ZERO));
            let effects = if self.enhanced.is_empty() { None } else { Some(self.enhancement_effects(entity)) };
            let (stealth_upkeep, jammer_upkeep) = effects.map_or((None, None), |effects| (effects.stealth_upkeep, effects.jammer.and(effects.jammer_upkeep.or(Some(Res::ZERO)))));
            let team = self.ecs.get::<&UnitStats>(entity).expect("units have stats").team.index();
            let switched_off = self.ecs.get::<&Construction>(entity).is_ok() || self.ecs.get::<&Paused>(entity).is_ok();
            let discount = self.ecs.get::<&Adjacency>(entity).map_or(0, |adjacency| adjacency.upkeep_discount);
            let mut intel = self.ecs.get::<&mut Intel>(entity).expect("intel list only holds intel units");
            // Stunned, or held in a tractor beam, everything is off, and nothing is paid for.
            if !self.disabled.is_empty() && self.disabled.binary_search_by_key(&id, |unit| unit.0).is_ok() {
                intel.running &= upkeep == Res::ZERO;
                (intel.cloaked, intel.stealthed, intel.jamming) = (false, false, false);
                continue;
            }
            let economy = &self.economies[team];
            let mut afford = |cost: Res| {
                let ok = economy.energy + economy.energy_income - paid[team] >= cost;
                if ok {
                    paid[team] += cost;
                }
                ok
            };
            if upkeep > Res::ZERO {
                intel.running = !switched_off && afford(upkeep * i64::from(1000 - discount) / 1000);
            }
            if intel.cloak_on {
                intel.cloaked = afford(cloak_upkeep);
                intel.cloak_on = intel.cloaked;
            }
            if let Some(cost) = stealth_upkeep {
                intel.stealthed = afford(cost);
            }
            if let Some(cost) = jammer_upkeep {
                intel.jamming = afford(cost);
            }
        }
        paid
    }

    fn step_builder(&mut self, id: SimId, entity: hecs::Entity, power_by_target: &mut BTreeMap<(SimId, WorkKind), Fp>) {
        // Upgrading takes all of its build power, and its other work waits, as does a teleport.
        if self.is_busy_in_place(entity) {
            return;
        }
        let (task, power, range) = {
            let builder = self.ecs.get::<&Builder>(entity).expect("builder list only holds builders");
            let Some(&task) = builder.tasks.first() else {
                return;
            };
            (task, builder.power, builder.range)
        };
        // Its way to this task turned out to be cut off, so the task cannot be done.
        if !matches!(task, BuildTask::Walk { .. }) && self.unreachable.binary_search(&id).is_ok() {
            self.ecs.get::<&mut Builder>(entity).expect("builder list only holds builders").tasks.remove(0);
            return;
        }
        if let BuildTask::Walk { order, started } = task {
            let idle = self.ecs.get::<&Order>(entity).expect("units have orders").kind == OrderKind::Idle;
            if idle {
                let mut builder = self.ecs.get::<&mut Builder>(entity).expect("builder list only holds builders");
                if started {
                    builder.tasks.remove(0);
                } else {
                    builder.tasks[0] = BuildTask::Walk { order, started: true };
                    drop(builder);
                    self.set_order(id, order);
                }
            }
            return;
        }
        let here = self.ecs.get::<&Position>(entity).expect("units have positions").0.xy();
        let team = self.ecs.get::<&UnitStats>(entity).expect("units have stats").team;

        match self.look_at_task(task, entity, here, team, range) {
            TaskStep::Done => {
                self.ecs.get::<&mut Builder>(entity).expect("builder list only holds builders").tasks.remove(0);
                self.set_order(id, Order::IDLE);
            }
            TaskStep::Work { target, kind } => {
                self.halt(id, entity);
                *power_by_target.entry((target, kind)).or_insert(Fp::ZERO) += power;
                self.work_links.push(WorkLink { builder: id, target, reclaim: false });
            }
            TaskStep::Reclaim { target, entity: wreck_entity } => {
                self.halt(id, entity);
                self.work_links.push(WorkLink { builder: id, target, reclaim: true });
                let per_tick = |rate: i64| fp_to_res(power) * rate / TICKS_PER_SECOND;
                let mut wreck = self.ecs.get::<&mut Wreck>(wreck_entity).expect("task checked the wreck");
                let mass = per_tick(RECLAIM_MASS_PER_POWER).min(wreck.mass);
                let energy = per_tick(RECLAIM_ENERGY_PER_POWER).min(wreck.energy);
                wreck.mass -= mass;
                wreck.energy -= energy;
                let finished = wreck.mass <= Res::ZERO && wreck.energy <= Res::ZERO;
                drop(wreck);
                let economy = &mut self.economies[team.index()];
                economy.mass_reclaimed += mass;
                economy.energy_reclaimed += energy;
                economy.mass_reclaimed_total += mass;
                economy.energy_reclaimed_total += energy;
                if finished {
                    self.ecs.despawn(wreck_entity).expect("task checked the wreck");
                    self.wrecks.retain(|wreck| wreck.0 != target);
                }
            }
            TaskStep::MoveTo(goal) => {
                // A field workshop cannot go to work out of its reach, so it lets it go.
                if self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.is_structure) {
                    self.ecs.get::<&mut Builder>(entity).expect("builder list only holds builders").tasks.remove(0);
                    return;
                }
                let idle = self.ecs.get::<&Order>(entity).expect("units have orders").kind == OrderKind::Idle;
                if idle {
                    self.assign_route(&[id], OrderKind::Move, goal, None);
                }
            }
            TaskStep::Wait => self.halt(id, entity),
        }
    }

    fn halt(&mut self, id: SimId, entity: hecs::Entity) {
        if self.ecs.get::<&Order>(entity).expect("units have orders").kind != OrderKind::Idle {
            self.set_order(id, Order::IDLE);
        }
    }

    fn look_at_task(&mut self, task: BuildTask, entity: hecs::Entity, here: Vec2Fp, team: Team, range: Fp) -> TaskStep {
        match task {
            BuildTask::Build { blueprint, cell_x, cell_y, site } => {
                let size = u32::from(self.blueprints.get(blueprint).footprint);
                let center = cell_center(cell_x, cell_y, size);
                let reach = range + self.blueprints.get(blueprint).radius;
                if !self.within(here, center, reach) {
                    return TaskStep::MoveTo(center);
                }
                if let Some(site) = site {
                    return match self.find_unit(site) {
                        Some((_, site_entity)) if self.ecs.get::<&Construction>(site_entity).is_ok() => {
                            TaskStep::Work { target: site, kind: WorkKind::Construct }
                        }
                        _ => TaskStep::Done,
                    };
                }
                // Join a site another builder already started here, or start a new one.
                let cell = self.cell_index(cell_x, cell_y);
                let found = match self.occupancy.get(&cell).copied() {
                    Some(occupant) => {
                        let (_, occupant_entity) = self.find_unit(occupant).expect("occupancy only holds live units");
                        let joinable = self.ecs.get::<&Construction>(occupant_entity).is_ok()
                            && self.ecs.get::<&Blueprinted>(occupant_entity).is_ok_and(|unit| unit.0 == blueprint)
                            && self.ecs.get::<&UnitStats>(occupant_entity).is_ok_and(|stats| stats.team == team);
                        joinable.then_some(occupant)
                    }
                    // A support factory whose headquarters is gone by now is not started.
                    None if !self.support_allowed(team, blueprint) => None,
                    None => self.spawn_site(blueprint, team, cell_x, cell_y),
                };
                let Some(found) = found else {
                    return TaskStep::Done;
                };
                let mut builder = self.ecs.get::<&mut Builder>(entity).expect("builder list only holds builders");
                builder.tasks[0] = BuildTask::Build { blueprint, cell_x, cell_y, site: Some(found) };
                TaskStep::Wait
            }
            BuildTask::Assist { target } => {
                let Some((_, target_entity)) = self.find_unit(target) else {
                    return TaskStep::Done;
                };
                let target_position = self.ecs.get::<&Position>(target_entity).expect("units have positions").0.xy();
                let target_radius = self.ecs.get::<&UnitStats>(target_entity).expect("units have stats").radius;
                if !self.within(here, target_position, range + target_radius) {
                    return TaskStep::MoveTo(target_position);
                }
                if self.ecs.get::<&Construction>(target_entity).is_ok() {
                    return TaskStep::Work { target, kind: WorkKind::Construct };
                }
                if self.ecs.get::<&Upgrading>(target_entity).is_ok() {
                    return TaskStep::Work { target, kind: WorkKind::Upgrade };
                }
                if self.ecs.get::<&super::components::Enhancing>(target_entity).is_ok() {
                    return TaskStep::Work { target, kind: WorkKind::Enhance };
                }
                if let Ok(factory) = self.ecs.get::<&Factory>(target_entity) {
                    return if factory.queue.is_empty() {
                        TaskStep::Wait
                    } else {
                        TaskStep::Work { target, kind: WorkKind::FactoryItem }
                    };
                }
                // Assisting a builder means working on whatever it is working on.
                if let Ok(other) = self.ecs.get::<&Builder>(target_entity) {
                    if let Some(BuildTask::Build { site: Some(site), .. }) = other.tasks.first() {
                        return TaskStep::Work { target: *site, kind: WorkKind::Construct };
                    }
                    return TaskStep::Wait;
                }
                TaskStep::Done
            }
            BuildTask::Repair { target } => {
                let Some((_, target_entity)) = self.find_unit(target) else {
                    return TaskStep::Done;
                };
                let target_position = self.ecs.get::<&Position>(target_entity).expect("units have positions").0.xy();
                let target_radius = self.ecs.get::<&UnitStats>(target_entity).expect("units have stats").radius;
                if !self.within(here, target_position, range + target_radius) {
                    return TaskStep::MoveTo(target_position);
                }
                let health = *self.ecs.get::<&Health>(target_entity).expect("units have health");
                // A unit captured away from its team is not repaired by its old one.
                let theirs = self.ecs.get::<&UnitStats>(target_entity).expect("units have stats").team != team;
                if health.current >= health.max || theirs {
                    return TaskStep::Done;
                }
                TaskStep::Work { target, kind: WorkKind::Repair }
            }
            BuildTask::Capture { target } => {
                let Some((_, target_entity)) = self.find_unit(target) else {
                    return TaskStep::Done;
                };
                if !self.capturable(target_entity, team) {
                    return TaskStep::Done;
                }
                let target_position = self.ecs.get::<&Position>(target_entity).expect("units have positions").0.xy();
                let target_radius = self.ecs.get::<&UnitStats>(target_entity).expect("units have stats").radius;
                if !self.within(here, target_position, range + target_radius) {
                    return TaskStep::MoveTo(target_position);
                }
                TaskStep::Work { target, kind: WorkKind::Capture }
            }
            BuildTask::Walk { .. } => unreachable!("walks are handled before looking at a task"),
            BuildTask::Reclaim { target } => {
                let Ok(index) = self.wrecks.binary_search_by_key(&target, |wreck| wreck.0) else {
                    return TaskStep::Done;
                };
                let wreck_entity = self.wrecks[index].1;
                let wreck = *self.ecs.get::<&Wreck>(wreck_entity).expect("wreck list only holds wrecks");
                if !self.within(here, wreck.position, range + wreck.radius) {
                    return TaskStep::MoveTo(wreck.position);
                }
                TaskStep::Reclaim { target, entity: wreck_entity }
            }
        }
    }

    fn work_item(&self, target: SimId, kind: WorkKind, power: Fp) -> Option<WorkItem> {
        let (_, entity) = self.find_unit(target)?;
        let team = self.ecs.get::<&UnitStats>(entity).ok()?.team;
        let adjacency = self.ecs.get::<&Adjacency>(entity).map_or(Adjacency::default(), |adjacency| *adjacency);
        // A capture takes half the target's build time, and costs its energy, to the capturers.
        if kind == WorkKind::Capture {
            let unit = self.blueprints.get(self.ecs.get::<&Blueprinted>(entity).ok()?.0);
            let step = fp_to_res(power) * 2 / (unit.build_time * TICKS_PER_SECOND);
            return Some(WorkItem { target, entity, kind, team: team.enemy(), step, mass: Res::ZERO, energy: unit.energy * step });
        }
        if kind == WorkKind::Enhance {
            let enhancement = self.blueprints.enhancement(self.ecs.get::<&super::components::Enhancing>(entity).ok()?.enhancement);
            let step = fp_to_res(power) / (enhancement.build_time * TICKS_PER_SECOND);
            return Some(WorkItem { target, entity, kind, team, step, mass: enhancement.mass * step, energy: enhancement.energy * step });
        }
        let blueprint = match kind {
            WorkKind::Enhance | WorkKind::Capture => unreachable!("worked out above"),
            WorkKind::FactoryItem => {
                let factory = self.ecs.get::<&Factory>(entity).ok()?;
                *factory.queue.first()?
            }
            WorkKind::Construct | WorkKind::Repair => self.ecs.get::<&Blueprinted>(entity).ok()?.0,
            WorkKind::Upgrade => self.ecs.get::<&Upgrading>(entity).ok()?.into,
        };
        let unit = self.blueprints.get(blueprint);
        // A full silo builds no more, and takes nothing, until it fires.
        if kind == WorkKind::FactoryItem && unit.class == UnitClass::Ammo && self.silo_is_full(entity) {
            return None;
        }
        // A support factory waits, at no cost, for a headquarters of the item's tier.
        if kind == WorkKind::FactoryItem && !self.factory_may_build(entity, blueprint) {
            return None;
        }
        let step = fp_to_res(power) / (unit.build_time * TICKS_PER_SECOND);
        // What a structure builds, or upgrades to, costs less beside the right neighbors.
        let (mass_discount, energy_discount) = if matches!(kind, WorkKind::FactoryItem | WorkKind::Upgrade) {
            (adjacency.mass_cost_discount, adjacency.energy_cost_discount)
        } else {
            (0, 0)
        };
        let mass = Adjacency::less(unit.mass * step, mass_discount);
        let energy = Adjacency::less(unit.energy * step, energy_discount);
        Some(WorkItem { target, entity, kind, team, step, mass, energy })
    }

    fn apply_work(&mut self, item: &WorkItem, amount: Res) {
        match item.kind {
            WorkKind::Construct => {
                let Ok(mut construction) = self.ecs.get::<&mut Construction>(item.entity) else {
                    return;
                };
                construction.progress += amount;
                let finished = construction.progress >= Res::ONE;
                drop(construction);
                self.heal(item.entity, amount);
                if finished {
                    self.ecs.remove_one::<Construction>(item.entity).expect("just checked");
                    self.ecs.get::<&mut Combat>(item.entity).expect("units have combat").enabled = true;
                    let mut health = self.ecs.get::<&mut Health>(item.entity).expect("units have health");
                    // Damage taken while under construction stays; the rest of the health is earned.
                    health.current = health.current.min(health.max);
                    drop(health);
                    self.economy_dirty = true;
                }
            }
            WorkKind::Repair => self.heal(item.entity, amount),
            WorkKind::Capture => {
                let progress = self.ecs.get::<&BeingCaptured>(item.entity).map_or(Res::ZERO, |capture| capture.progress) + amount;
                if progress >= Res::ONE {
                    self.capture_unit(item.target, item.entity, item.team);
                } else {
                    self.ecs.insert_one(item.entity, BeingCaptured { progress }).expect("work item checked the unit");
                    if let Err(at) = self.being_captured.binary_search_by_key(&item.target, |unit| unit.0) {
                        self.being_captured.insert(at, (item.target, item.entity));
                    }
                    self.captured_this_tick.push(item.target);
                }
            }
            WorkKind::Enhance => {
                let Ok(mut under_way) = self.ecs.get::<&mut super::components::Enhancing>(item.entity) else {
                    return;
                };
                under_way.progress += amount;
                let finished = under_way.progress >= Res::ONE;
                drop(under_way);
                if finished {
                    self.finish_enhancement(item.target, item.entity);
                }
            }
            WorkKind::Upgrade => {
                let Ok(mut upgrading) = self.ecs.get::<&mut Upgrading>(item.entity) else {
                    return;
                };
                upgrading.progress += amount;
                let (finished, into) = (upgrading.progress >= Res::ONE, upgrading.into);
                drop(upgrading);
                if finished {
                    self.finish_upgrade(item.target, item.entity, into);
                }
            }
            WorkKind::FactoryItem => {
                let mut factory = self.ecs.get::<&mut Factory>(item.entity).expect("work item checked the factory");
                factory.progress += amount;
                if factory.progress < Res::ONE {
                    return;
                }
                factory.progress = Res::ZERO;
                let finished = factory.queue.remove(0);
                // Repeat loops units, not upgrades, which only happen once.
                if factory.repeat && !self.blueprints.get(finished).is_factory() {
                    factory.queue.push(finished);
                }
                let rally = factory.rally;
                drop(factory);
                self.finish_factory_item(item.entity, item.team, finished, rally);
            }
        }
    }

    fn heal(&mut self, entity: hecs::Entity, share: Res) {
        let mut health = self.ecs.get::<&mut Health>(entity).expect("units have health");
        let gain = Fp::from_bits(((i64::from(health.max.to_bits()) * share.to_bits()) >> 24) as i32);
        health.current = health.current.saturating_add(gain).min(health.max);
    }

    /// Whether a team's builders can capture this unit: an enemy's, finished, and not a
    /// commander or field rig, which cannot be captured.
    fn capturable(&self, entity: hecs::Entity, team: Team) -> bool {
        let theirs = self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team != team);
        let finished = self.ecs.get::<&Construction>(entity).is_err();
        // Nor can an aircraft in flight be reached.
        let commander = self.ecs.get::<&Blueprinted>(entity).is_ok_and(|unit| {
            let class = self.blueprints.get(unit.0).class;
            matches!(class, UnitClass::Commander | UnitClass::FieldRig | UnitClass::Air)
        });
        theirs && finished && !commander
    }

    /// A capture nobody worked on this tick starts over.
    fn drop_stopped_captures(&mut self) {
        let worked = std::mem::take(&mut self.captured_this_tick);
        let (kept, stopped): (Vec<_>, Vec<_>) = self.being_captured.iter().partition(|unit| worked.contains(&unit.0));
        for (_, entity) in stopped {
            let _ = self.ecs.remove_one::<BeingCaptured>(entity);
        }
        self.being_captured = kept;
    }

    /// Hands a unit to the team that captured it, as it is, with nothing to do.
    fn capture_unit(&mut self, id: SimId, entity: hecs::Entity, team: Team) {
        self.clear_unit_plans(&[id]);
        self.set_order(id, Order::IDLE);
        let _ = self.ecs.remove_one::<BeingCaptured>(entity);
        self.being_captured.retain(|unit| unit.0 != id);
        let _ = self.ecs.remove_one::<Paused>(entity);
        if self.ecs.remove_one::<super::components::SelfDestructing>(entity).is_ok() {
            self.self_destructing.retain(|unit| unit.0 != id);
        }
        if let Ok(mut factory) = self.ecs.get::<&mut Factory>(entity) {
            factory.queue.clear();
            factory.repeat = false;
            factory.progress = Res::ZERO;
            factory.rally = None;
        }
        self.ecs.get::<&mut UnitStats>(entity).expect("units have stats").team = team;
        // Nobody keeps shooting at it: its new side would be shooting its own.
        for &(_, other) in &self.units {
            if let Ok(mut combat) = self.ecs.get::<&mut Combat>(other) {
                if combat.target == Some(id) {
                    combat.target = None;
                }
                if combat.forced_target == Some(id) {
                    combat.forced_target = None;
                }
            }
        }
        self.economy_dirty = true;
    }

    fn silo_is_full(&self, entity: hecs::Entity) -> bool {
        let Ok(blueprint) = self.ecs.get::<&Blueprinted>(entity) else {
            return false;
        };
        let limit = self.blueprints.get(blueprint.0).max_stock;
        let launcher = self.ecs.get::<&Launcher>(entity).ok().map(|launcher| launcher.stock);
        let stock = launcher.or_else(|| self.ecs.get::<&MissileDefense>(entity).ok().map(|defense| defense.stock)).unwrap_or(0);
        limit > 0 && stock >= limit
    }

    fn finish_factory_item(&mut self, entity: hecs::Entity, team: Team, finished: BlueprintId, rally: Option<Vec2Fp>) {
        let unit = self.blueprints.get(finished);
        // A silo keeps the missiles it builds, until it fires them.
        if unit.class == UnitClass::Ammo {
            if let Ok(mut launcher) = self.ecs.get::<&mut Launcher>(entity) {
                launcher.stock += 1;
            } else if let Ok(mut defense) = self.ecs.get::<&mut MissileDefense>(entity) {
                defense.stock += 1;
            }
            return;
        }
        if unit.is_factory() {
            // An upgrade: the same structure takes on its new stats, at the same share of health.
            let (power, max_health, vision) = (unit.build_power, unit.health, unit.vision);
            self.ecs.get::<&mut Blueprinted>(entity).expect("factories have blueprints").0 = finished;
            self.ecs.get::<&mut Factory>(entity).expect("work item checked the factory").power = power;
            self.ecs.get::<&mut UnitStats>(entity).expect("units have stats").vision = vision;
            let mut health = self.ecs.get::<&mut Health>(entity).expect("units have health");
            let share = i64::from(health.current.to_bits()) * i64::from(max_health.to_bits()) / i64::from(health.max.to_bits());
            health.current = Fp::from_bits(share as i32);
            health.max = max_health;
            return;
        }
        let center = self.ecs.get::<&Position>(entity).expect("units have positions").0.xy();
        let factory_radius = self.ecs.get::<&UnitStats>(entity).expect("units have stats").radius;
        let reach = factory_radius + unit.radius + fp_int(6);
        let exits = exit_order(center, rally.unwrap_or_else(|| {
            let middle = fp_int(self.terrain.map_units() / 2);
            Vec2Fp::new(middle, middle)
        }));
        let exit = exits
            .iter()
            .map(|&(x, y)| center + Vec2Fp::new(reach * x, reach * y))
            .find(|point| self.terrain.in_bounds(*point) && self.terrain.passable_for(unit.move_class(), *point))
            .or_else(|| {
                super::flowfield::nearest_passable_cell_for(&self.terrain, unit.move_class(), center).map(|(x, y)| cell_center(x as u32, y as u32, 1))
            });
        let Some(exit) = exit else {
            return;
        };
        // It is built in the bay, so it starts there. An aircraft lifts off and flies; anything
        // else drives out of the open end first (`LeavingFactory`), then heads for the rally.
        let flies = unit.move_class() == super::components::MoveClass::Air;
        let id = self.spawn_mobile_inner(finished, team, center, true);
        if flies {
            // With no rally point, it clears the pad for the next.
            self.assign_route(&[id], OrderKind::Move, rally.unwrap_or(exit), None);
        } else {
            let entity = self.find_unit(id).expect("just spawned").1;
            self.ecs.insert_one(entity, super::components::LeavingFactory { exit, rally }).expect("just spawned");
        }
    }

    /// Recalculates storage, income, and adjacency after a structure appears or disappears.
    fn recompute_economy(&mut self) {
        self.economy_dirty = false;
        for economy in &mut self.economies {
            economy.mass_capacity = economy.base_mass_capacity;
            economy.energy_capacity = economy.base_energy_capacity;
            economy.mass_income = Res::ZERO;
            economy.energy_income = Res::ZERO;
            economy.extractor_income = Res::ZERO;
            economy.extractor_upkeep = Res::ZERO;
        }
        let mut adjacency_updates = Vec::with_capacity(self.structures.len());
        let mut links = Vec::new();
        for &(id, entity) in &self.structures {
            if self.ecs.get::<&Construction>(entity).is_ok() {
                continue;
            }
            let blueprint = self.ecs.get::<&Blueprinted>(entity).expect("structures have blueprints").0;
            let footprint = *self.ecs.get::<&Footprint>(entity).expect("structures have footprints");
            let team = self.ecs.get::<&UnitStats>(entity).expect("units have stats").team;
            let bonus = self.adjacency_of(id, blueprint, footprint, team, &mut links);
            adjacency_updates.push((entity, bonus));
            let unit = self.blueprints.get(blueprint);
            // An extractor makes mass for energy, and a paused one makes nothing and needs nothing.
            let extractor = unit.mass_income > Res::ZERO && unit.upkeep > Res::ZERO;
            let paused = extractor && self.ecs.get::<&Paused>(entity).is_ok();
            let economy = &mut self.economies[team.index()];
            economy.mass_capacity += unit.mass_storage;
            economy.energy_capacity += unit.energy_storage;
            if paused {
                continue;
            }
            let mass = Adjacency::more(unit.mass_income, bonus.mass_output_bonus);
            economy.mass_income += mass;
            economy.energy_income += Adjacency::more(unit.energy_income, bonus.energy_output_bonus);
            if extractor {
                economy.extractor_income += mass;
                economy.extractor_upkeep += Adjacency::less(unit.upkeep, bonus.upkeep_discount);
            }
        }
        links.sort_unstable();
        self.adjacency_links = links;
        for (entity, bonus) in adjacency_updates {
            *self.ecs.get::<&mut Adjacency>(entity).expect("structures have adjacency") = bonus;
        }
        // Commanders and field rigs make and store resources of their own.
        for &(_, entity) in &self.builders {
            let Ok(blueprint) = self.ecs.get::<&Blueprinted>(entity).map(|unit| unit.0) else {
                continue;
            };
            let unit = self.blueprints.get(blueprint);
            let stats = *self.ecs.get::<&UnitStats>(entity).expect("units have stats");
            let earns = unit.mass_income > Res::ZERO || unit.energy_income > Res::ZERO || unit.mass_storage > Res::ZERO || unit.energy_storage > Res::ZERO;
            if stats.is_structure || !(earns || self.ecs.get::<&super::components::Enhancements>(entity).is_ok()) || self.ecs.get::<&Construction>(entity).is_ok() {
                continue;
            }
            let effects = self.enhancement_effects(entity);
            let economy = &mut self.economies[stats.team.index()];
            economy.mass_capacity += unit.mass_storage;
            economy.energy_capacity += unit.energy_storage;
            economy.mass_income += unit.mass_income + effects.mass_income;
            economy.energy_income += unit.energy_income + effects.energy_income;
        }
        for economy in &mut self.economies {
            economy.mass = economy.mass.min(economy.mass_capacity);
            economy.energy = economy.energy.min(economy.energy_capacity);
        }
    }

    /// Finds the finished, friendly structures touching a footprint's edges.
    /// Also adds to `links` each neighbor that gives this structure a bonus it uses, as (giver,
    /// receiver), for drawing.
    fn adjacency_of(&self, id: SimId, blueprint: BlueprintId, footprint: Footprint, team: Team, links: &mut Vec<(SimId, SimId)>) -> Adjacency {
        let unit = self.blueprints.get(blueprint);
        let Footprint { cell_x, cell_y, size } = footprint;
        let mut neighbors: Vec<SimId> = Vec::new();
        for offset in 0..size {
            let around = [
                (cell_x.wrapping_sub(1), cell_y + offset),
                (cell_x + size, cell_y + offset),
                (cell_x + offset, cell_y.wrapping_sub(1)),
                (cell_x + offset, cell_y + size),
            ];
            for (x, y) in around {
                let cells = self.terrain.cells_per_side() as u32;
                if x >= cells || y >= cells {
                    continue;
                }
                if let Some(&neighbor) = self.occupancy.get(&self.cell_index(x, y)) {
                    if neighbor != id && !neighbors.contains(&neighbor) {
                        neighbors.push(neighbor);
                    }
                }
            }
        }
        // Each finished neighbor of the team gives its bonuses for this structure's size, and they add up.
        let size = unit.adjacency_size;
        let artillery = unit.weapon.is_some_and(|weapon| weapon.artillery);
        let mut adjacency = Adjacency::default();
        for neighbor in neighbors {
            let Some((_, entity)) = self.find_unit(neighbor) else {
                continue;
            };
            let same_team = self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team == team);
            if !same_team || self.ecs.get::<&Construction>(entity).is_ok() {
                continue;
            }
            let other = self.blueprints.get(self.ecs.get::<&Blueprinted>(entity).expect("structures have blueprints").0);
            if let Some(giver) = other.adjacency_giver {
                let bonus = giver.bonus();
                // Only a bonus this structure has a use for counts as a link worth showing.
                let builds = unit.is_factory() || unit.build_power > Fp::ZERO;
                let upkeep = unit.upkeep > Res::ZERO || unit.shield.is_some();
                let used = (builds && (bonus.energy_active[size] != 0 || bonus.mass_active[size] != 0))
                    || (upkeep && bonus.upkeep[size] != 0)
                    || (unit.mass_income > Res::ZERO && bonus.mass_output[size] != 0)
                    || (unit.energy_income > Res::ZERO && bonus.energy_output[size] != 0)
                    || (artillery && (bonus.weapon_energy[size] != 0 || bonus.rate_of_fire[size] != 0));
                if used {
                    links.push((neighbor, id));
                }
                adjacency.energy_cost_discount += bonus.energy_active[size];
                adjacency.mass_cost_discount += bonus.mass_active[size];
                adjacency.upkeep_discount += bonus.upkeep[size];
                adjacency.mass_output_bonus += bonus.mass_output[size];
                adjacency.energy_output_bonus += bonus.energy_output[size];
                // Weapon energy and rate of fire only matter to artillery, as in FAF.
                if artillery {
                    adjacency.weapon_energy_discount += bonus.weapon_energy[size];
                    adjacency.reload_discount += bonus.rate_of_fire[size];
                }
            }
        }
        // A discount can never make something free.
        let most = (ADJACENCY_SCALE * 9 / 10) as i32;
        for discount in [&mut adjacency.energy_cost_discount, &mut adjacency.mass_cost_discount, &mut adjacency.upkeep_discount, &mut adjacency.weapon_energy_discount, &mut adjacency.reload_discount] {
            *discount = (*discount).min(most);
        }
        adjacency
    }

    /// Sends idle patrollers to their next waypoint. Idle engineers look for work first.
    fn step_patrols(&mut self) {
        for index in 0..self.patrols.len() {
            let (id, entity) = self.patrols[index];
            let is_idle = self.ecs.get::<&Order>(entity).expect("units have orders").kind == OrderKind::Idle;
            // An aircraft gone to refuel comes back to its patrol once it is done.
            if self.ecs.get::<&Flight>(entity).is_ok_and(|flight| flight.refit.is_some()) {
                continue;
            }
            let tasks = self.ecs.get::<&Builder>(entity).map_or(0, |builder| builder.tasks.len());
            let is_builder = self.ecs.get::<&Builder>(entity).is_ok();
            if tasks > 0 {
                continue;
            }
            let phase = self.ecs.get::<&UnitStats>(entity).map_or(0, |stats| stats.phase);
            if is_builder && phase.wrapping_add(self.current_tick).is_multiple_of(PATROL_SEARCH_INTERVAL) {
                if let Some(task) = self.find_patrol_work(entity) {
                    self.set_order(id, Order::IDLE);
                    self.ecs.get::<&mut Builder>(entity).expect("just checked").tasks.push(task);
                    continue;
                }
            }
            // A queued patrol waits for the moves ahead of it.
            if !is_idle || self.ecs.get::<&OrderQueue>(entity).is_ok_and(|queue| !queue.0.is_empty()) {
                continue;
            }
            let mut patrol = self.ecs.get::<&mut Patrol>(entity).expect("patrol list only holds patrols");
            let goal = patrol.points[patrol.next as usize];
            patrol.next = (patrol.next + 1) % patrol.points.len() as u32;
            drop(patrol);
            let kind = if is_builder { OrderKind::Move } else { OrderKind::AttackMove };
            self.assign_route(&[id], kind, goal, None);
        }
    }

    /// Engineering stations look for work within their reach every so often while they have
    /// none, and give up assisting a factory once it has nothing left to make.
    fn step_stations(&mut self) {
        for index in 0..self.builders.len() {
            let (_, entity) = self.builders[index];
            let Ok(stats) = self.ecs.get::<&UnitStats>(entity).map(|stats| *stats) else {
                continue;
            };
            if !stats.is_structure || self.ecs.get::<&Construction>(entity).is_ok() || self.ecs.get::<&Paused>(entity).is_ok() {
                continue;
            }
            if !stats.phase.wrapping_add(self.current_tick).is_multiple_of(PATROL_SEARCH_INTERVAL) {
                continue;
            }
            let current = self.ecs.get::<&Builder>(entity).ok().and_then(|builder| builder.tasks.first().copied());
            if let Some(BuildTask::Assist { target }) = current {
                let idle_factory = self.find_unit(target).is_some_and(|(_, target)| {
                    self.ecs.get::<&Factory>(target).is_ok_and(|factory| factory.queue.is_empty()) || self.ecs.get::<&Paused>(target).is_ok()
                });
                if idle_factory {
                    self.ecs.get::<&mut Builder>(entity).expect("builder list only holds builders").tasks.clear();
                }
            }
            let has_work = self.ecs.get::<&Builder>(entity).is_ok_and(|builder| !builder.tasks.is_empty());
            if !has_work {
                if let Some(task) = self.find_station_work(entity) {
                    self.ecs.get::<&mut Builder>(entity).expect("builder list only holds builders").tasks.push(task);
                }
            }
        }
    }

    /// What a field workshop helps with: the nearest friendly construction site, factory
    /// at work, or upgrade within its reach, or failing that, the nearest damaged friend. Ties go
    /// to the lowest id.
    fn find_station_work(&self, entity: hecs::Entity) -> Option<BuildTask> {
        let here = self.ecs.get::<&Position>(entity).ok()?.0.xy();
        let team = self.ecs.get::<&UnitStats>(entity).ok()?.team;
        let range = self.ecs.get::<&Builder>(entity).ok()?.range;
        let mut best_build: Option<(FpWide, SimId, BuildTask)> = None;
        let mut best_repair: Option<(FpWide, SimId, BuildTask)> = None;
        let mut consider = |other: &GridEntry| {
            if other.entity == entity {
                return;
            }
            let position = Vec2Fp::new(Fp::from_bits(other.x), Fp::from_bits(other.y));
            let reach = range + Fp::from_bits(other.radius);
            let distance = here.distance_squared(&position);
            if distance > wide_mul(reach, reach) {
                return;
            }
            let paused = self.ecs.get::<&Paused>(other.entity).is_ok();
            let building = !paused
                && (self.ecs.get::<&Construction>(other.entity).is_ok()
                    || self.ecs.get::<&Upgrading>(other.entity).is_ok()
                    || self.ecs.get::<&Factory>(other.entity).is_ok_and(|factory| !factory.queue.is_empty()));
            let damaged = self.ecs.get::<&Health>(other.entity).is_ok_and(|health| health.current < health.max) && self.ecs.get::<&Construction>(other.entity).is_err();
            let better = |best: &Option<(FpWide, SimId, BuildTask)>| best.as_ref().is_none_or(|&(closest, id, _)| (distance, other.id) < (closest, id));
            if building && better(&best_build) {
                best_build = Some((distance, other.id, BuildTask::Assist { target: other.id }));
            } else if damaged && better(&best_repair) {
                best_repair = Some((distance, other.id, BuildTask::Repair { target: other.id }));
            }
        };
        let (x, y) = (here.x.to_bits(), here.y.to_bits());
        let search = range + Fp::from_bits(self.max_structure_radius().max(self.max_mobile_radius()));
        self.spatial_grid.for_each_candidate(Some(team), Layer::Structure, x, y, search.to_bits(), &mut consider);
        self.spatial_grid.for_each_candidate(Some(team), Layer::Mobile, x, y, search.to_bits(), &mut consider);
        best_build.or(best_repair).map(|(_, _, task)| task)
    }

    /// The nearest wreck, construction site, or damaged friend. Ties go to the lowest id.
    fn find_patrol_work(&self, entity: hecs::Entity) -> Option<BuildTask> {
        let here = self.ecs.get::<&Position>(entity).ok()?.0.xy();
        let team = self.ecs.get::<&UnitStats>(entity).ok()?.team;
        let radius = fp_int(PATROL_SEARCH_RADIUS_UNITS);
        let limit = wide_mul(radius, radius);
        let mut best = None;
        for &(id, wreck_entity) in &self.wrecks {
            let wreck = self.ecs.get::<&Wreck>(wreck_entity).ok()?;
            let distance = here.distance_squared(&wreck.position);
            if distance <= limit && best.is_none_or(|(closest, _)| distance < closest) {
                best = Some((distance, BuildTask::Reclaim { target: id }));
            }
        }
        if let Some((_, task)) = best {
            return Some(task);
        }
        let mut consider = |other: &GridEntry| {
            let position = Vec2Fp::new(Fp::from_bits(other.x), Fp::from_bits(other.y));
            let distance = here.distance_squared(&position);
            if distance > limit || other.entity == entity {
                return;
            }
            let needs_work = self.ecs.get::<&Construction>(other.entity).is_ok();
            let damaged = self.ecs.get::<&Health>(other.entity).is_ok_and(|health| health.current < health.max);
            if (needs_work || damaged) && best.is_none_or(|(closest, _)| distance < closest) {
                let task = if needs_work { BuildTask::Assist { target: other.id } } else { BuildTask::Repair { target: other.id } };
                best = Some((distance, task));
            }
        };
        let (x, y) = (here.x.to_bits(), here.y.to_bits());
        self.spatial_grid.for_each_candidate(Some(team), Layer::Structure, x, y, radius.to_bits(), &mut consider);
        self.spatial_grid.for_each_candidate(Some(team), Layer::Mobile, x, y, radius.to_bits(), &mut consider);
        best.map(|(_, task)| task)
    }

    /// The pending orders of one team's units. A plan never reveals what the team cannot
    /// see: a step at a hidden position, such as a fogged attack target, is left out.
    pub(super) fn plans_for(&self, team: Team) -> Vec<UnitPlanView> {
        let mut plans = Vec::new();
        for &(id, entity) in &self.units {
            let stats = self.ecs.get::<&UnitStats>(entity).expect("units have stats");
            if stats.team != team || stats.is_structure {
                continue;
            }
            let mut steps = Vec::new();
            if let Ok(builder) = self.ecs.get::<&Builder>(entity) {
                for task in &builder.tasks {
                    if let Some(step) = self.task_step(task) {
                        steps.push(step);
                    }
                }
            }
            let order = *self.ecs.get::<&Order>(entity).expect("units have orders");
            // A builder's route only leads to its first task, which is already listed.
            if steps.is_empty() && order.kind != OrderKind::Idle {
                steps.extend(self.order_step(&order));
            }
            if let Ok(queue) = self.ecs.get::<&OrderQueue>(entity) {
                steps.extend(queue.0.iter().filter_map(|order| self.order_step(order)));
            }
            if let Ok(patrol) = self.ecs.get::<&Patrol>(entity) {
                let count = patrol.points.len() as u32;
                for offset in 0..count {
                    let point = patrol.points[((patrol.next + offset) % count) as usize];
                    steps.push(self.plan_step(PlanStepKind::Patrol, point, 0));
                }
            }
            steps.retain(|step| {
                !self.config.fog_of_war || step.kind != PlanStepKind::Attack || self.visibility.is_visible(team, step.x, step.y)
            });
            if !steps.is_empty() {
                plans.push(UnitPlanView { id, steps });
            }
        }
        plans
    }

    /// The viewer's paused factories and sites, in `SimId` order.
    pub(super) fn paused_for(&self, team: Team) -> Vec<SimId> {
        self.units
            .iter()
            .filter(|&&(_, entity)| self.ecs.get::<&Paused>(entity).is_ok() && self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team == team))
            .map(|&(id, _)| id)
            .collect()
    }

    /// The viewer's units with orders waiting, in `SimId` order. A unit that is not a builder
    /// shows its current walk first, so that it can be removed like any other.
    pub(super) fn build_queues_for(&self, team: Team) -> Vec<BuildQueueView> {
        let on_team = |entity: hecs::Entity| self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team == team);
        let mut queues: Vec<BuildQueueView> = self
            .builders
            .iter()
            .filter(|(_, entity)| on_team(*entity))
            .filter_map(|&(id, entity)| {
                let builder = self.ecs.get::<&Builder>(entity).ok()?;
                (!builder.tasks.is_empty()).then(|| BuildQueueView { id, tasks: builder.tasks.clone() })
            })
            .collect();
        for &(id, entity) in self.order_queues.iter().filter(|(_, entity)| on_team(*entity)) {
            let current = *self.ecs.get::<&Order>(entity).expect("units have orders");
            let waiting = self.ecs.get::<&OrderQueue>(entity).expect("queue list only holds queues");
            let walking = (current.kind != OrderKind::Idle).then_some(BuildTask::Walk { order: current, started: true });
            let tasks = walking.into_iter().chain(waiting.0.iter().map(|order| BuildTask::Walk { order: *order, started: false })).collect();
            queues.push(BuildQueueView { id, tasks });
        }
        queues.sort_unstable_by_key(|queue| queue.id);
        queues
    }

    /// The viewer's orders that can still be moved. Patrols repeat on their own, attacks and
    /// follow orders go wherever their target goes, and a site that has started stays put.
    pub(super) fn order_stops_for(&self, team: Team) -> Vec<OrderStopView> {
        let mut stops = Vec::new();
        for &(id, entity) in &self.units {
            let stats = self.ecs.get::<&UnitStats>(entity).expect("units have stats");
            if stats.team != team || stats.is_structure || self.ecs.get::<&Patrol>(entity).is_ok() {
                continue;
            }
            let mut add = |task: BuildTask| {
                if let Some(step) = self.task_step(&task) {
                    stops.push(OrderStopView { unit: id, task, position: [step.x, step.y, step.z] });
                }
            };
            let tasks = self.ecs.get::<&Builder>(entity).map_or(0, |builder| builder.tasks.len());
            if let Ok(builder) = self.ecs.get::<&Builder>(entity) {
                for task in &builder.tasks {
                    if matches!(task, BuildTask::Walk { .. } | BuildTask::Build { site: None, .. }) {
                        add(*task);
                    }
                }
            }
            let order = *self.ecs.get::<&Order>(entity).expect("units have orders");
            // A builder's route only leads to its first task, which is already listed.
            if tasks == 0 && matches!(order.kind, OrderKind::Move | OrderKind::AttackMove) && !self.flow_field.is_following(order.route) {
                add(BuildTask::Walk { order, started: true });
            }
            if let Ok(queue) = self.ecs.get::<&OrderQueue>(entity) {
                for order in &queue.0 {
                    add(BuildTask::Walk { order: *order, started: false });
                }
            }
        }
        stops
    }

    /// What the viewer's structures and commander make and spend, in tenths per second.
    pub(super) fn rates_for(&self, team: Team) -> Vec<UnitRatesView> {
        let tenths_per_second = |per_tick: Res| (per_tick * TICKS_PER_SECOND * 10).round().to_num::<i32>();
        let commander = self.commanders[team.index()].and_then(|id| self.find_unit(id).map(|(_, entity)| (id, entity)));
        let mut rates: Vec<UnitRatesView> = self
            .structures
            .iter()
            .copied()
            .chain(commander)
            .filter(|&(_, entity)| self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team == team) && self.ecs.get::<&Construction>(entity).is_err())
            .filter_map(|(id, entity)| {
                let unit = self.blueprints.get(self.ecs.get::<&Blueprinted>(entity).ok()?.0);
                let adjacency = self.ecs.get::<&Adjacency>(entity).map_or(Adjacency::default(), |adjacency| *adjacency);
                let mut mass = Adjacency::more(unit.mass_income, adjacency.mass_output_bonus);
                let spent = self.factory_spending.iter().find(|spending| spending.0 == id);
                // An extractor makes, and pays for, the share of its mass its team can power.
                let extractor_use = if unit.mass_income > Res::ZERO && unit.upkeep > Res::ZERO {
                    let efficiency = self.economies[team.index()].extractor_efficiency;
                    if self.ecs.get::<&Paused>(entity).is_ok() {
                        mass = Res::ZERO;
                        Res::ZERO
                    } else {
                        mass *= efficiency;
                        Adjacency::less(unit.upkeep, adjacency.upkeep_discount) * efficiency
                    }
                } else {
                    Res::ZERO
                };
                // A dome that is up costs its upkeep, less the discount from neighbors.
                let upkeep = self.ecs.get::<&Shield>(entity).ok().filter(|shield| shield.is_up() && self.ecs.get::<&Paused>(entity).is_err()).map_or(Res::ZERO, |shield| {
                    Adjacency::less(shield.upkeep, adjacency.upkeep_discount)
                });
                // So does running intel, and a cloak.
                let intel = self.ecs.get::<&Intel>(entity).ok().map_or(Res::ZERO, |intel| {
                    let running = if intel.running { Adjacency::less(unit.upkeep, adjacency.upkeep_discount) } else { Res::ZERO };
                    running + if intel.cloaked { unit.cloak_upkeep.unwrap_or(Res::ZERO) } else { Res::ZERO }
                });
                Some(UnitRatesView {
                    id,
                    mass_income: tenths_per_second(mass),
                    energy_income: tenths_per_second(Adjacency::more(unit.energy_income, adjacency.energy_output_bonus)),
                    mass_use: spent.map_or(0, |spending| tenths_per_second(spending.1)),
                    energy_use: spent.map_or(0, |spending| tenths_per_second(spending.2)) + tenths_per_second(upkeep + intel + extractor_use),
                })
            })
            .collect();
        rates.sort_unstable_by_key(|rates| rates.id);
        rates
    }

    /// This tick's work, limited to what `seen` allows. A team with nothing to spend is
    /// not building anything, however hard its builders are trying.
    pub(super) fn work_link_views(&self, seen: impl Fn(Team, i32, i32) -> bool) -> Vec<WorkLinkView> {
        if self.match_result.is_some() {
            return Vec::new();
        }
        self.work_links
            .iter()
            .filter_map(|link| {
                let (_, builder) = self.find_unit(link.builder)?;
                let team = self.ecs.get::<&UnitStats>(builder).ok()?.team;
                if !link.reclaim && self.economies[team.index()].satisfaction == Res::ZERO {
                    return None;
                }
                let (place, radius) = if link.reclaim {
                    let index = self.wrecks.binary_search_by_key(&link.target, |wreck| wreck.0).ok()?;
                    let wreck = self.ecs.get::<&Wreck>(self.wrecks[index].1).ok()?;
                    (wreck.position, wreck.radius)
                } else {
                    let (_, target) = self.find_unit(link.target)?;
                    let radius = self.ecs.get::<&UnitStats>(target).ok()?.radius;
                    (self.ecs.get::<&Position>(target).ok()?.0.xy(), radius)
                };
                let here = self.ecs.get::<&Position>(builder).ok()?.0;
                let (x, y) = (place.x.to_bits(), place.y.to_bits());
                if !seen(team, here.x.to_bits(), here.y.to_bits()) || !seen(team, x, y) {
                    return None;
                }
                let z = (self.terrain.height_at(place) + radius / 2).to_bits();
                Some(WorkLinkView { builder: link.builder, target: [x, y, z], reclaim: link.reclaim })
            })
            .collect()
    }

    fn order_step(&self, order: &Order) -> Option<PlanStep> {
        let goal = order.slot.or_else(|| self.flow_field.goal(order.route))?;
        let kind = match order.kind {
            OrderKind::AttackMove => PlanStepKind::AttackMove,
            // An attack on a unit follows it. One that does not is aimed at the ground.
            OrderKind::Attack if self.flow_field.is_following(order.route) => PlanStepKind::Attack,
            OrderKind::Attack => PlanStepKind::AttackGround,
            OrderKind::Move | OrderKind::Idle => PlanStepKind::Move,
        };
        Some(self.plan_step(kind, goal, 0))
    }

    fn plan_step(&self, kind: PlanStepKind, point: Vec2Fp, pending_width: i32) -> PlanStep {
        PlanStep {
            kind,
            x: point.x.to_bits(),
            y: point.y.to_bits(),
            z: self.terrain.height_at(point).to_bits(),
            pending_width,
        }
    }

    fn task_step(&self, task: &BuildTask) -> Option<PlanStep> {
        let unit_point = |target: SimId| {
            let (_, entity) = self.find_unit(target)?;
            Some(self.ecs.get::<&Position>(entity).ok()?.0.xy())
        };
        match *task {
            BuildTask::Build { blueprint, cell_x, cell_y, site } => {
                let size = u32::from(self.blueprints.get(blueprint).footprint);
                let width = if site.is_none() { fp_int(size as i32 * TERRAIN_CELL_UNITS).to_bits() } else { 0 };
                Some(self.plan_step(PlanStepKind::Build, cell_center(cell_x, cell_y, size), width))
            }
            BuildTask::Assist { target } | BuildTask::Repair { target } | BuildTask::Capture { target } => {
                Some(self.plan_step(PlanStepKind::Support, unit_point(target)?, 0))
            }
            BuildTask::Walk { order, .. } => self.order_step(&order),
            BuildTask::Reclaim { target } => {
                let index = self.wrecks.binary_search_by_key(&target, |wreck| wreck.0).ok()?;
                let position = self.ecs.get::<&Wreck>(self.wrecks[index].1).ok()?.position;
                Some(self.plan_step(PlanStepKind::Support, position, 0))
            }
        }
    }

    /// Hashes the parts only some units have. A tag byte keeps "absent" distinct from zero.
    pub(super) fn hash_unit_extras(&self, entity: hecs::Entity, hasher: &mut impl Hasher) {
        if let Ok(blueprint) = self.ecs.get::<&Blueprinted>(entity) {
            hasher.write(&[1]);
            hasher.write(&blueprint.0 .0.to_le_bytes());
        }
        if let Ok(construction) = self.ecs.get::<&Construction>(entity) {
            hasher.write(&[2]);
            hasher.write(&construction.progress.to_bits().to_le_bytes());
        }
        if let Ok(footprint) = self.ecs.get::<&Footprint>(entity) {
            hasher.write(&[3]);
            for value in [footprint.cell_x, footprint.cell_y, footprint.size] {
                hasher.write(&value.to_le_bytes());
            }
        }
        if let Ok(adjacency) = self.ecs.get::<&Adjacency>(entity) {
            hasher.write(&[4]);
            for value in [adjacency.mass_output_bonus, adjacency.energy_output_bonus, adjacency.energy_cost_discount, adjacency.mass_cost_discount, adjacency.upkeep_discount] {
                hasher.write(&value.to_le_bytes());
            }
            // Only for structures that have them, so that checksums from before artillery still hold.
            if adjacency.weapon_energy_discount != 0 || adjacency.reload_discount != 0 {
                hasher.write(&[1]);
                hasher.write(&adjacency.weapon_energy_discount.to_le_bytes());
                hasher.write(&adjacency.reload_discount.to_le_bytes());
            }
        }
        if let Ok(builder) = self.ecs.get::<&Builder>(entity) {
            hasher.write(&[5]);
            hasher.write(&builder.power.to_bits().to_le_bytes());
            hasher.write(&builder.range.to_bits().to_le_bytes());
            for task in &builder.tasks {
                let (kind, first, second, third, fourth) = match *task {
                    BuildTask::Build { blueprint, cell_x, cell_y, site } => {
                        (0u8, u32::from(blueprint.0), cell_x, cell_y, site.map_or(u32::MAX, |site| site.0))
                    }
                    BuildTask::Assist { target } => (1, target.0, 0, 0, 0),
                    BuildTask::Reclaim { target } => (2, target.0, 0, 0, 0),
                    BuildTask::Repair { target } => (3, target.0, 0, 0, 0),
                    BuildTask::Capture { target } => (5, target.0, 0, 0, 0),
                    BuildTask::Walk { order, started } => {
                        hash_slot(hasher, order.slot);
                        (4, order.route, u32::from(started), order.slot_ticks, order.kind as u32)
                    }
                };
                hasher.write(&[kind]);
                for value in [first, second, third, fourth] {
                    hasher.write(&value.to_le_bytes());
                }
            }
        }
        if let Ok(factory) = self.ecs.get::<&Factory>(entity) {
            hasher.write(&[6, u8::from(factory.repeat)]);
            hasher.write(&factory.power.to_bits().to_le_bytes());
            hasher.write(&factory.progress.to_bits().to_le_bytes());
            for item in &factory.queue {
                hasher.write(&item.0.to_le_bytes());
            }
            if let Some(rally) = factory.rally {
                hasher.write(&rally.x.to_bits().to_le_bytes());
                hasher.write(&rally.y.to_bits().to_le_bytes());
            }
        }
        if self.ecs.get::<&Paused>(entity).is_ok() {
            hasher.write(&[11]);
        }
        if let Ok(weapon) = self.ecs.get::<&DischargeWeapon>(entity) {
            hasher.write(&[12]);
            hasher.write(&weapon.ticks_until_ready.to_le_bytes());
        }
        if let Ok(order) = self.ecs.get::<&DischargeOrder>(entity) {
            hasher.write(&[13]);
            hasher.write(&order.target.0.to_le_bytes());
        }
        if let Ok(damage) = self.ecs.get::<&super::components::VetDamage>(entity) {
            hasher.write(&[40]);
            hasher.write(&damage.total.to_bits().to_le_bytes());
            for (attacker, dealt) in &damage.by {
                hasher.write(&attacker.0.to_le_bytes());
                hasher.write(&dealt.to_bits().to_le_bytes());
            }
        }
        if let Ok(veterancy) = self.ecs.get::<&Veterancy>(entity) {
            hasher.write(&[14, veterancy.rank]);
            hasher.write(&veterancy.experience.to_bits().to_le_bytes());
        }
        if let Ok(turret) = self.ecs.get::<&Turret>(entity) {
            hasher.write(&[10]);
            hasher.write(&turret.facing.x.to_bits().to_le_bytes());
            hasher.write(&turret.facing.y.to_bits().to_le_bytes());
        }
        if let Ok(turning) = self.ecs.get::<&Turning>(entity) {
            hasher.write(&[9]);
            hasher.write(&turning.facing.x.to_bits().to_le_bytes());
            hasher.write(&turning.facing.y.to_bits().to_le_bytes());
        }
        if let Ok(queue) = self.ecs.get::<&OrderQueue>(entity) {
            hasher.write(&[8]);
            for order in &queue.0 {
                hasher.write(&[order.kind as u8]);
                hasher.write(&order.route.to_le_bytes());
                hasher.write(&order.slot_ticks.to_le_bytes());
                hash_slot(hasher, order.slot);
            }
        }
        if let Ok(intel) = self.ecs.get::<&Intel>(entity) {
            hasher.write(&[19, u8::from(intel.running), u8::from(intel.cloak_on), u8::from(intel.cloaked)]);
            if intel.stealthed || intel.jamming {
                hasher.write(&[1, u8::from(intel.jamming)]);
            }
        }
        self.hash_enhancement_state(entity, hasher);
        if let Ok(capture) = self.ecs.get::<&BeingCaptured>(entity) {
            hasher.write(&[24]);
            hasher.write(&capture.progress.to_bits().to_le_bytes());
        }
        if let Ok(flight) = self.ecs.get::<&Flight>(entity) {
            hasher.write(&[25, u8::from(flight.breaking_off), u8::from(flight.docked), flight.pad, flight.size, u8::from(flight.hovers)]);
            for value in [flight.elevation, flight.altitude, flight.speed] {
                hasher.write(&value.to_bits().to_le_bytes());
            }
            for value in [flight.fuel, flight.max_fuel, flight.refit.map_or(u32::MAX, |staging| staging.0)] {
                hasher.write(&value.to_le_bytes());
            }
            hasher.write(&flight.break_off.to_bits().to_le_bytes());
        }
        if let Ok(anti_air) = self.ecs.get::<&AntiAir>(entity) {
            hasher.write(&[26, u8::from(anti_air.also_ground)]);
        }
        if self.ecs.get::<&BombBay>(entity).is_ok() {
            hasher.write(&[27]);
        }
        if self.ecs.get::<&super::components::Torpedo>(entity).is_ok() {
            hasher.write(&[30]);
        }
        if let Ok(extras) = self.ecs.get::<&super::components::ExtraWeapons>(entity) {
            hasher.write(&[29]);
            for weapon in &extras.0 {
                hasher.write(&weapon.ticks_until_ready.to_le_bytes());
                hasher.write(&weapon.target.map_or(u32::MAX, |target| target.0).to_le_bytes());
                if let Some(turret) = weapon.turret {
                    hasher.write(&turret.facing.x.to_bits().to_le_bytes());
                    hasher.write(&turret.facing.y.to_bits().to_le_bytes());
                }
            }
        }
        if let Ok(artillery) = self.ecs.get::<&super::components::Artillery>(entity) {
            hasher.write(&[28]);
            hasher.write(&artillery.min_range.to_bits().to_le_bytes());
            hasher.write(&artillery.spread.to_bits().to_le_bytes());
            hasher.write(&artillery.energy_per_shot.to_bits().to_le_bytes());
        }
        if let Ok(upgrading) = self.ecs.get::<&Upgrading>(entity) {
            hasher.write(&[18]);
            hasher.write(&upgrading.into.0.to_le_bytes());
            hasher.write(&upgrading.progress.to_bits().to_le_bytes());
        }
        if let Ok(launcher) = self.ecs.get::<&Launcher>(entity) {
            hasher.write(&[16, u8::from(launcher.strategic)]);
            hasher.write(&launcher.cruise_height.to_bits().to_le_bytes());
            hasher.write(&launcher.stock.to_le_bytes());
            if launcher.inner_radius > Fp::ZERO {
                hasher.write(&launcher.inner_radius.to_bits().to_le_bytes());
                hasher.write(&launcher.inner_damage.to_bits().to_le_bytes());
            }
            if launcher.automatic {
                hasher.write(&[1]);
            }
        }
        if let Ok(defense) = self.ecs.get::<&MissileDefense>(entity) {
            hasher.write(&[17, u8::from(defense.strategic)]);
            hasher.write(&defense.range.to_bits().to_le_bytes());
            for value in [defense.reload_ticks, defense.ticks_until_ready, defense.stock] {
                hasher.write(&value.to_le_bytes());
            }
        }
        if let Ok(defense) = self.ecs.get::<&super::components::TorpedoDefense>(entity) {
            hasher.write(&[31]);
            hasher.write(&defense.ticks_until_ready.to_le_bytes());
        }
        if let Ok(shield) = self.ecs.get::<&Shield>(entity) {
            hasher.write(&[15]);
            for value in [shield.radius, shield.health, shield.max_health, shield.regen_per_tick] {
                hasher.write(&value.to_bits().to_le_bytes());
            }
            hasher.write(&shield.recharging.to_le_bytes());
            hasher.write(&shield.recharge_ticks.to_le_bytes());
            hasher.write(&shield.upkeep.to_bits().to_le_bytes());
        }
        // §12's status effects and tractor beams, only for the units that have them.
        if let Ok(stunned) = self.ecs.get::<&super::components::Stunned>(entity) {
            hasher.write(&[32]);
            hasher.write(&stunned.ticks_left.to_le_bytes());
        }
        if let Ok(held) = self.ecs.get::<&super::components::Held>(entity) {
            hasher.write(&[33]);
            hasher.write(&held.by.0.to_le_bytes());
            hasher.write(&held.ticks_left.to_le_bytes());
        }
        if let Ok(beam) = self.ecs.get::<&super::components::TractorBeam>(entity) {
            hasher.write(&[34]);
            hasher.write(&beam.crush_ticks.to_le_bytes());
            hasher.write(&beam.holding.map_or(u32::MAX, |id| id.0).to_le_bytes());
        }
        if let Ok(weapon) = self.ecs.get::<&super::components::StunWeapon>(entity) {
            hasher.write(&[35]);
            hasher.write(&weapon.0.to_le_bytes());
        }
        if let Ok(transport) = self.ecs.get::<&super::components::Transport>(entity) {
            hasher.write(&[62]);
            hasher.write(&(transport.cargo.len() as u64).to_le_bytes());
            for id in &transport.cargo {
                hasher.write(&id.0.to_le_bytes());
            }
            if let Some((point, route)) = transport.unload {
                hasher.write(&point.x.to_bits().to_le_bytes());
                hasher.write(&point.y.to_bits().to_le_bytes());
                hasher.write(&route.to_le_bytes());
            }
        }
        if let Ok(boarding) = self.ecs.get::<&super::components::Boarding>(entity) {
            hasher.write(&[63]);
            hasher.write(&boarding.transport.0.to_le_bytes());
            hasher.write(&boarding.route.to_le_bytes());
        }
        if let Ok(carried) = self.ecs.get::<&super::components::Carried>(entity) {
            hasher.write(&[64]);
            hasher.write(&carried.transport.0.to_le_bytes());
            hasher.write(&carried.lists.to_le_bytes());
        }
        if let Ok(leaving) = self.ecs.get::<&super::components::LeavingFactory>(entity) {
            hasher.write(&[61]);
            hasher.write(&leaving.exit.x.to_bits().to_le_bytes());
            hasher.write(&leaving.exit.y.to_bits().to_le_bytes());
            if let Some(rally) = leaving.rally {
                hasher.write(&rally.x.to_bits().to_le_bytes());
                hasher.write(&rally.y.to_bits().to_le_bytes());
            }
        }
        if let Ok(patrol) = self.ecs.get::<&Patrol>(entity) {
            hasher.write(&[7]);
            hasher.write(&patrol.next.to_le_bytes());
            for point in &patrol.points {
                hasher.write(&point.x.to_bits().to_le_bytes());
                hasher.write(&point.y.to_bits().to_le_bytes());
            }
        }
    }
}

fn hash_slot(hasher: &mut impl Hasher, slot: Option<Vec2Fp>) {
    if let Some(slot) = slot {
        hasher.write(&slot.x.to_bits().to_le_bytes());
        hasher.write(&slot.y.to_bits().to_le_bytes());
    }
}
