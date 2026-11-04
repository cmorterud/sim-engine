//! The flow economy, construction, factories, reclaim, and adjacency
//! (GAME_SPECIFICATION.md §3-6). Everything here runs once per tick in `SimId` order.

use std::collections::BTreeMap;
use std::hash::Hasher;

use super::blueprints::{BlueprintId, Res, TICKS_PER_SECOND};
use super::commands::SimCommand;
use super::components::{
    Adjacency, Blueprinted, BuildTask, Builder, Combat, Construction, Factory, Footprint, Health, Order, OrderKind, Patrol,
    Position, SimId, Team, UnitStats, Wreck,
};
use super::config::{PATROL_SEARCH_INTERVAL, PATROL_SEARCH_RADIUS_UNITS, RECLAIM_MASS_PER_POWER, TERRAIN_CELL_UNITS};
use super::flowfield::nearest_passable_cell;
use super::spatial::{GridEntry, Layer};
use super::world::{EconomyView, PlanStep, PlanStepKind, SimWorld, UnitPlanView, UnitSpawn};
use crate::math::fp::{fp_int, wide_mul, Fp, Vec2Fp};

const MEX_STORAGE_BONUS: u32 = 125;
const MEX_STORAGE_BONUS_MAX: u32 = 500;
const FACTORY_ENERGY_DISCOUNT: u32 = 63;
const FACTORY_ENERGY_DISCOUNT_MAX: u32 = 250;
const FACTORY_MASS_DISCOUNT: u32 = 38;
const FACTORY_MASS_DISCOUNT_MAX: u32 = 150;
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
    /// This tick's mass from reclaim.
    pub mass_reclaimed: Res,
    pub mass_demand: Res,
    pub energy_demand: Res,
    /// The share of demand that was met, from 0 to 1.
    pub satisfaction: Res,
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
            self.mass_demand,
            self.energy_demand,
            self.satisfaction,
        ] {
            hasher.write(&value.to_bits().to_le_bytes());
        }
    }

    pub fn view(&self) -> EconomyView {
        let per_second = |value: Res| (value * TICKS_PER_SECOND).to_num::<i64>();
        EconomyView {
            mass: self.mass.to_num(),
            energy: self.energy.to_num(),
            mass_capacity: self.mass_capacity.to_num(),
            energy_capacity: self.energy_capacity.to_num(),
            mass_income: per_second(self.mass_income + self.mass_reclaimed),
            energy_income: per_second(self.energy_income),
            mass_demand: per_second(self.mass_demand),
            energy_demand: per_second(self.energy_demand),
            satisfaction_permille: (self.satisfaction.to_bits() * 1000 / Res::ONE.to_bits()).clamp(0, 1000) as u16,
        }
    }
}

/// What a unit of build power is being spent on.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum WorkKind {
    Construct,
    FactoryItem,
    Repair,
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

fn cell_center(cell_x: u32, cell_y: u32, size: u32) -> Vec2Fp {
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
        let spawn = self.blueprint_spawn(blueprint, team, position);
        let id = self.spawn_unit(spawn);
        let entity = self.find_unit(id).expect("just spawned").1;
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
        for y in cell_y..cell_y + size {
            for x in cell_x..cell_x + size {
                if !self.terrain.cell_is_open(x, y) {
                    return false;
                }
                // Deposits are saved for extractors.
                if !unit.requires_deposit && self.deposits.contains(&(x, y)) {
                    return false;
                }
            }
        }
        true
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
        let center = cell_center(cell_x, cell_y, size);
        let spawn = self.blueprint_spawn(blueprint, team, center);
        let id = self.spawn_unit(spawn);
        let entity = self.find_unit(id).expect("just spawned").1;
        self.attach_blueprint_parts(id, entity, blueprint);
        self.ecs
            .insert(entity, (Footprint { cell_x, cell_y, size }, Adjacency::default()))
            .expect("entity was just spawned");
        self.structures.push((id, entity));
        self.terrain.set_structure_cells(cell_x, cell_y, size, true);
        self.flow_field.invalidate();
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
        if is_factory {
            let factory = Factory { power, queue: Vec::new(), repeat: false, progress: Res::ZERO, rally: None };
            self.ecs.insert_one(entity, factory).expect("entity was just spawned");
            self.factories.push((id, entity));
        } else if power > Fp::ZERO {
            self.ecs.insert_one(entity, Builder { power, range, tasks: Vec::new() }).expect("entity was just spawned");
            self.builders.push((id, entity));
        }
    }

    pub(super) fn clear_footprint(&mut self, footprint: Footprint) {
        let Footprint { cell_x, cell_y, size } = footprint;
        self.terrain.set_structure_cells(cell_x, cell_y, size, false);
        self.flow_field.invalidate();
        for y in cell_y..cell_y + size {
            for x in cell_x..cell_x + size {
                let cell = self.cell_index(x, y);
                self.occupancy.remove(&cell);
            }
        }
    }

    pub(super) fn apply_production_command(&mut self, command: &SimCommand) {
        match command {
            SimCommand::Patrol { units, points } => {
                if points.is_empty() {
                    return;
                }
                self.clear_unit_plans(units);
                for &id in units {
                    if self.find_unit(id).is_none() {
                        continue;
                    }
                    let (_, entity) = self.find_unit(id).expect("just checked");
                    self.set_order(id, Order::IDLE);
                    self.ecs.insert_one(entity, Patrol { points: points.clone(), next: 0 }).expect("unit is live");
                    // Keep the list sorted, since it is usually a small, recent change.
                    let at = self.patrols.partition_point(|patrol| patrol.0 < id);
                    self.patrols.insert(at, (id, entity));
                }
            }
            SimCommand::Build { units, blueprint, cell_x, cell_y, queued } => {
                let builders = self.units_that_can_build(units, *blueprint);
                if !*queued {
                    self.clear_unit_plans(&builders);
                    for &id in &builders {
                        self.set_order(id, Order::IDLE);
                    }
                }
                let task = BuildTask::Build { blueprint: *blueprint, cell_x: *cell_x, cell_y: *cell_y, site: None };
                self.push_task(&builders, task);
            }
            SimCommand::Assist { units, target } => self.replace_tasks(units, BuildTask::Assist { target: *target }),
            SimCommand::Reclaim { units, target } => self.replace_tasks(units, BuildTask::Reclaim { target: *target }),
            SimCommand::Repair { units, target } => self.replace_tasks(units, BuildTask::Repair { target: *target }),
            SimCommand::QueueUnit { factory, blueprint, count } => {
                let Some(entity) = self.factory_entity(*factory) else {
                    return;
                };
                let current = self.ecs.get::<&Blueprinted>(entity).expect("factories have blueprints").0;
                let unit = self.blueprints.get(current);
                if unit.builds.contains(blueprint) || unit.upgrades_to == Some(*blueprint) {
                    let mut factory = self.ecs.get::<&mut Factory>(entity).expect("factory list only holds factories");
                    factory.queue.extend(std::iter::repeat_n(*blueprint, (*count).min(100) as usize));
                }
            }
            SimCommand::ClearQueue { factory } => {
                if let Some(entity) = self.factory_entity(*factory) {
                    let mut factory = self.ecs.get::<&mut Factory>(entity).expect("factory list only holds factories");
                    factory.queue.clear();
                    factory.progress = Res::ZERO;
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
            | SimCommand::AttackMove { .. }
            | SimCommand::Follow { .. }
            | SimCommand::Stop { .. }
            | SimCommand::Attack { .. } => {}
        }
    }

    fn factory_entity(&self, id: SimId) -> Option<hecs::Entity> {
        self.factories.binary_search_by_key(&id, |factory| factory.0).ok().map(|index| self.factories[index].1)
    }

    fn units_that_can_build(&self, units: &[SimId], blueprint: BlueprintId) -> Vec<SimId> {
        units
            .iter()
            .copied()
            .filter(|id| {
                self.find_unit(*id).is_some_and(|(_, entity)| {
                    self.ecs.get::<&Builder>(entity).is_ok()
                        && self.ecs.get::<&Blueprinted>(entity).is_ok_and(|unit| self.blueprints.get(unit.0).builds.contains(&blueprint))
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
        if self.builders.is_empty() && self.factories.is_empty() && self.structures.is_empty() && self.patrols.is_empty() {
            return;
        }
        if self.economy_dirty {
            self.recompute_economy();
        }
        self.step_patrols();

        for economy in &mut self.economies {
            economy.mass_reclaimed = Res::ZERO;
        }
        let mut power_by_target: BTreeMap<(SimId, WorkKind), Fp> = BTreeMap::new();
        for index in 0..self.builders.len() {
            let (id, entity) = self.builders[index];
            self.step_builder(id, entity, &mut power_by_target);
        }
        for &(id, entity) in &self.factories {
            let has_item = self.ecs.get::<&Factory>(entity).is_ok_and(|factory| !factory.queue.is_empty());
            if has_item && self.ecs.get::<&Construction>(entity).is_err() {
                let power = self.ecs.get::<&Factory>(entity).expect("factory list only holds factories").power;
                *power_by_target.entry((id, WorkKind::FactoryItem)).or_insert(Fp::ZERO) += power;
            }
        }

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
            let mass_supply = economy.mass + economy.mass_income + economy.mass_reclaimed;
            let energy_supply = economy.energy + economy.energy_income;
            let share = |supply: Res, wanted: Res| {
                if wanted <= supply || wanted == Res::ZERO {
                    Res::ONE
                } else {
                    (supply / wanted).clamp(Res::ZERO, Res::ONE)
                }
            };
            satisfaction[team_index] = share(mass_supply, mass_demand).min(share(energy_supply, energy_demand));
            economy.mass_demand = mass_demand;
            economy.energy_demand = energy_demand;
            economy.satisfaction = satisfaction[team_index];
        }

        let mut spent = [(Res::ZERO, Res::ZERO); 2];
        for item in &items {
            let team = item.team.index();
            let share = satisfaction[team];
            spent[team].0 += item.mass * share;
            spent[team].1 += item.energy * share;
            self.apply_work(item, item.step * share);
        }
        for (team_index, economy) in self.economies.iter_mut().enumerate() {
            let mass = economy.mass + economy.mass_income + economy.mass_reclaimed - spent[team_index].0;
            let energy = economy.energy + economy.energy_income - spent[team_index].1;
            economy.mass = mass.clamp(Res::ZERO, economy.mass_capacity);
            economy.energy = energy.clamp(Res::ZERO, economy.energy_capacity);
        }
    }

    fn step_builder(&mut self, id: SimId, entity: hecs::Entity, power_by_target: &mut BTreeMap<(SimId, WorkKind), Fp>) {
        let (task, power, range) = {
            let builder = self.ecs.get::<&Builder>(entity).expect("builder list only holds builders");
            let Some(&task) = builder.tasks.first() else {
                return;
            };
            (task, builder.power, builder.range)
        };
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
            }
            TaskStep::Reclaim { target, entity: wreck_entity } => {
                self.halt(id, entity);
                let wanted = fp_to_res(power) * RECLAIM_MASS_PER_POWER / TICKS_PER_SECOND;
                let mut wreck = self.ecs.get::<&mut Wreck>(wreck_entity).expect("task checked the wreck");
                let taken = wanted.min(wreck.mass);
                wreck.mass -= taken;
                let finished = wreck.mass <= Res::ZERO;
                drop(wreck);
                self.economies[team.index()].mass_reclaimed += taken;
                if finished {
                    self.ecs.despawn(wreck_entity).expect("task checked the wreck");
                    self.wrecks.retain(|wreck| wreck.0 != target);
                }
            }
            TaskStep::MoveTo(goal) => {
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
                if health.current >= health.max {
                    return TaskStep::Done;
                }
                TaskStep::Work { target, kind: WorkKind::Repair }
            }
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
        let blueprint = match kind {
            WorkKind::FactoryItem => {
                let factory = self.ecs.get::<&Factory>(entity).ok()?;
                *factory.queue.first()?
            }
            WorkKind::Construct | WorkKind::Repair => self.ecs.get::<&Blueprinted>(entity).ok()?.0,
        };
        let unit = self.blueprints.get(blueprint);
        let step = fp_to_res(power) / (unit.build_time * TICKS_PER_SECOND);
        let (mass_discount, energy_discount) = if kind == WorkKind::FactoryItem {
            (adjacency.mass_cost_discount, adjacency.energy_cost_discount)
        } else {
            (0, 0)
        };
        let mass = unit.mass * step * (1000 - i64::from(mass_discount)) / 1000;
        let energy = unit.energy * step * (1000 - i64::from(energy_discount)) / 1000;
        Some(WorkItem { entity, kind, team, step, mass, energy })
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
            WorkKind::FactoryItem => {
                let mut factory = self.ecs.get::<&mut Factory>(item.entity).expect("work item checked the factory");
                factory.progress += amount;
                if factory.progress < Res::ONE {
                    return;
                }
                factory.progress = Res::ZERO;
                let finished = factory.queue.remove(0);
                if factory.repeat {
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

    fn finish_factory_item(&mut self, entity: hecs::Entity, team: Team, finished: BlueprintId, rally: Option<Vec2Fp>) {
        let unit = self.blueprints.get(finished);
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
        let exits = [Vec2Fp::new(Fp::ZERO, -reach), Vec2Fp::new(reach, Fp::ZERO), Vec2Fp::new(Fp::ZERO, reach), Vec2Fp::new(-reach, Fp::ZERO)];
        let exit = exits
            .iter()
            .map(|offset| center + *offset)
            .find(|point| self.terrain.in_bounds(*point) && self.terrain.is_passable(*point))
            .or_else(|| {
                nearest_passable_cell(&self.terrain, center).map(|(x, y)| cell_center(x as u32, y as u32, 1))
            });
        let Some(exit) = exit else {
            return;
        };
        let id = self.spawn_mobile(finished, team, exit);
        if let Some(point) = rally {
            self.assign_route(&[id], OrderKind::Move, point, None);
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
        }
        let mut adjacency_updates = Vec::with_capacity(self.structures.len());
        for &(id, entity) in &self.structures {
            if self.ecs.get::<&Construction>(entity).is_ok() {
                continue;
            }
            let blueprint = self.ecs.get::<&Blueprinted>(entity).expect("structures have blueprints").0;
            let footprint = *self.ecs.get::<&Footprint>(entity).expect("structures have footprints");
            let team = self.ecs.get::<&UnitStats>(entity).expect("units have stats").team;
            let bonus = self.adjacency_of(id, blueprint, footprint, team);
            adjacency_updates.push((entity, bonus));
            let unit = self.blueprints.get(blueprint);
            let economy = &mut self.economies[team.index()];
            economy.mass_capacity += unit.mass_storage;
            economy.energy_capacity += unit.energy_storage;
            economy.mass_income += unit.mass_income + unit.mass_income * i64::from(bonus.mass_output_bonus) / 1000;
            economy.energy_income += unit.energy_income;
        }
        for (entity, bonus) in adjacency_updates {
            *self.ecs.get::<&mut Adjacency>(entity).expect("structures have adjacency") = bonus;
        }
        for team in [Team::Blue, Team::Red] {
            let Some((_, entity)) = self.commanders[team.index()].and_then(|id| self.find_unit(id)) else {
                continue;
            };
            let blueprint = self.ecs.get::<&Blueprinted>(entity).expect("commanders have blueprints").0;
            let unit = self.blueprints.get(blueprint);
            let economy = &mut self.economies[team.index()];
            economy.mass_capacity += unit.mass_storage;
            economy.energy_capacity += unit.energy_storage;
            economy.mass_income += unit.mass_income;
            economy.energy_income += unit.energy_income;
        }
        for economy in &mut self.economies {
            economy.mass = economy.mass.min(economy.mass_capacity);
            economy.energy = economy.energy.min(economy.energy_capacity);
        }
    }

    /// Finds the finished, friendly structures touching a footprint's edges.
    fn adjacency_of(&self, id: SimId, blueprint: BlueprintId, footprint: Footprint, team: Team) -> Adjacency {
        let unit = self.blueprints.get(blueprint);
        let (mass_storage, power) = (self.blueprints.id("mass_storage"), self.blueprints.id("power_generator"));
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
        let (mut storages, mut generators) = (0u32, 0u32);
        for neighbor in neighbors {
            let Some((_, entity)) = self.find_unit(neighbor) else {
                continue;
            };
            let same_team = self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team == team);
            if !same_team || self.ecs.get::<&Construction>(entity).is_ok() {
                continue;
            }
            let other = self.ecs.get::<&Blueprinted>(entity).expect("structures have blueprints").0;
            storages += u32::from(other == mass_storage);
            generators += u32::from(other == power);
        }
        let mut adjacency = Adjacency::default();
        if unit.requires_deposit {
            adjacency.mass_output_bonus = (storages * MEX_STORAGE_BONUS).min(MEX_STORAGE_BONUS_MAX);
        }
        if unit.is_factory() {
            adjacency.energy_cost_discount = (generators * FACTORY_ENERGY_DISCOUNT).min(FACTORY_ENERGY_DISCOUNT_MAX);
            adjacency.mass_cost_discount = (storages * FACTORY_MASS_DISCOUNT).min(FACTORY_MASS_DISCOUNT_MAX);
        }
        adjacency
    }

    /// Sends idle patrollers to their next waypoint. Idle engineers look for work first.
    fn step_patrols(&mut self) {
        for index in 0..self.patrols.len() {
            let (id, entity) = self.patrols[index];
            let is_idle = self.ecs.get::<&Order>(entity).expect("units have orders").kind == OrderKind::Idle;
            let tasks = self.ecs.get::<&Builder>(entity).map_or(0, |builder| builder.tasks.len());
            let is_builder = self.ecs.get::<&Builder>(entity).is_ok();
            if tasks > 0 {
                continue;
            }
            if is_builder && id.0.wrapping_add(self.current_tick) % PATROL_SEARCH_INTERVAL == 0 {
                if let Some(task) = self.find_patrol_work(entity) {
                    self.set_order(id, Order::IDLE);
                    self.ecs.get::<&mut Builder>(entity).expect("just checked").tasks.push(task);
                    continue;
                }
            }
            if !is_idle {
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
                if let Some(goal) = self.flow_field.goal(order.route) {
                    let kind = match order.kind {
                        OrderKind::AttackMove => PlanStepKind::AttackMove,
                        OrderKind::Attack => PlanStepKind::Attack,
                        OrderKind::Move | OrderKind::Idle => PlanStepKind::Move,
                    };
                    steps.push(self.plan_step(kind, goal, 0));
                }
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
                let width = if site.is_none() { size as i32 * TERRAIN_CELL_UNITS } else { 0 };
                Some(self.plan_step(PlanStepKind::Build, cell_center(cell_x, cell_y, size), width))
            }
            BuildTask::Assist { target } | BuildTask::Repair { target } => {
                Some(self.plan_step(PlanStepKind::Support, unit_point(target)?, 0))
            }
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
            for value in [adjacency.mass_output_bonus, adjacency.energy_cost_discount, adjacency.mass_cost_discount] {
                hasher.write(&value.to_le_bytes());
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
