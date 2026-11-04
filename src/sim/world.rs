use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hasher;
use std::sync::Arc;
use twox_hash::XxHash64;

use super::blueprints::{BlueprintDb, Res};
use super::combat::{entry_hit, firing_solution, probe_terrain, GRAVITY};
use super::commands::SimCommand;
use super::components::{
    Blueprinted, Builder, Combat, Construction, Factory, Footprint, Health, MoveClass, Order, OrderKind, Patrol,
    Position, Projectile, SimId, Team, UnitStats, Velocity, Wreck,
};
use super::config::{SimConfig, TARGET_SEARCH_INTERVAL, TERRAIN_CELL_UNITS, WRECK_MASS_PERMILLE};
use super::flowfield::{nearest_passable_cell, FlowField, FlowSample};
use super::formation::plan_formation;
use super::production::Economy;
use super::spatial::{GridUnit, Layer, SpatialGrid};
use super::terrain::Terrain;
use super::visibility::{Visibility, VISION_REFRESH_TICKS};
use crate::math::fp::{sqrt_wide, wide_mul, Fp, Vec2Fp, Vec3Fp, FP_FRAC_BITS};
use crate::math::rect::RectFp;
use crate::math::rng::SimRng;

/// How far from a goal, in unit radii, a moving unit can settle against a crowd.
const CROWD_SETTLE_RADII: i32 = 30;
/// The gap between formation spots, in halves of the largest unit's radius.
const FORMATION_SPACING_RADII: i32 = 5;
const SLOT_EXTRA_TICKS: i64 = 40;

/// Settings for spawning a unit.
#[derive(Copy, Clone, Debug)]
pub struct UnitSpawn {
    pub position: Vec2Fp,
    pub team: Team,
    pub max_speed: Fp,
    pub radius: Fp,
    pub health: Fp,
    pub range: Fp,
    pub reload_ticks: u32,
    pub projectile_speed: Fp,
    pub projectile_damage: Fp,
    pub projectile_splash: Fp,
    pub is_structure: bool,
    pub vision: Fp,
}

/// How a match ended.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MatchResult {
    Winner(Team),
    Draw,
}

/// Read-only unit state for the renderer. Values are raw `Fp` bits.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct UnitView {
    pub id: SimId,
    pub team: Team,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub radius: i32,
    /// Raw blueprint id, or `u16::MAX` for a unit without one.
    pub blueprint: u16,
    pub health_permille: u16,
    /// 1000 once finished.
    pub built_permille: u16,
    pub is_structure: bool,
}

/// A factory's production state, for the build menu. Only for the viewer's team.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactoryView {
    pub id: SimId,
    pub queue: Vec<u16>,
    pub repeat: bool,
    pub progress_permille: u16,
    /// Where new units go, as raw `Fp` bits: x, y, and ground height.
    pub rally: Option<[i32; 3]>,
}

/// What a step in a unit's plan is.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlanStepKind {
    Move,
    AttackMove,
    Attack,
    Build,
    Support,
    Patrol,
}

/// One place a unit intends to go or work, as raw `Fp` bits.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PlanStep {
    pub kind: PlanStepKind,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// For a build that has not started: the width of its outline, in units.
    pub pending_width: i32,
}

/// A unit's pending orders, in the order it will carry them out. Only for the viewer's team.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitPlanView {
    pub id: SimId,
    pub steps: Vec<PlanStep>,
}

/// Read-only wreck state. Values are raw `Fp` bits.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WreckView {
    pub id: SimId,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub radius: i32,
    pub mass: i32,
}

/// The viewer's economy, in whole units and per-second rates.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct EconomyView {
    pub mass: i64,
    pub energy: i64,
    pub mass_capacity: i64,
    pub energy_capacity: i64,
    pub mass_income: i64,
    pub energy_income: i64,
    pub mass_demand: i64,
    pub energy_demand: i64,
    /// How much of demand is being met, in thousandths.
    pub satisfaction_permille: u16,
}

/// Read-only projectile state for the renderer. Values are raw `Fp` bits.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ProjectileView {
    pub id: SimId,
    pub team: Team,
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// A copy of the state the renderer needs from one tick, sorted by `SimId`.
#[derive(Clone, Debug, Default)]
pub struct SimSnapshot {
    pub tick: u32,
    pub units: Vec<UnitView>,
    pub projectiles: Vec<ProjectileView>,
    pub wrecks: Vec<WreckView>,
    pub factories: Vec<FactoryView>,
    /// Sorted by `SimId`.
    pub plans: Vec<UnitPlanView>,
    pub economy: EconomyView,
    /// Present when fog of war is on: one byte per cell, for the renderer.
    pub fog: Option<Arc<Vec<u8>>>,
    pub match_result: Option<MatchResult>,
}

/// A blast waiting to be applied.
#[derive(Copy, Clone, Debug)]
struct Explosion {
    x: i32,
    y: i32,
    radius: Fp,
    damage: Fp,
}

struct PendingShot {
    source: SimId,
    team: Team,
    from: Vec3Fp,
    to: Vec3Fp,
    speed: Fp,
    damage: Fp,
    splash: Fp,
}

pub struct SimWorld {
    pub ecs: hecs::World,
    pub terrain: Terrain,
    pub spatial_grid: SpatialGrid,
    pub flow_field: FlowField,
    pub rng: SimRng,
    pub config: SimConfig,
    pub current_tick: u32,
    pub next_sim_id: u32,
    /// Sorted by `SimId`: spawns append, despawns retain order. Any loop whose order
    /// can affect results iterates these, never a `hecs` query.
    pub units: Vec<(SimId, hecs::Entity)>,
    pub projectiles: Vec<(SimId, hecs::Entity)>,
    /// Box obstacles added to the terrain, for the renderer.
    pub obstacles: Vec<RectFp>,
    pub blueprints: Arc<BlueprintDb>,
    pub visibility: Visibility,
    pub economies: [Economy; 2],
    pub deposits: Vec<(u32, u32)>,
    /// The structure on each occupied terrain cell, by cell index.
    pub occupancy: BTreeMap<u32, SimId>,
    pub wrecks: Vec<(SimId, hecs::Entity)>,
    pub builders: Vec<(SimId, hecs::Entity)>,
    pub factories: Vec<(SimId, hecs::Entity)>,
    pub structures: Vec<(SimId, hecs::Entity)>,
    pub patrols: Vec<(SimId, hecs::Entity)>,
    pub commanders: [Option<SimId>; 2],
    pub match_result: Option<MatchResult>,
    pub(super) economy_dirty: bool,
    max_mobile_radius: i32,
    max_structure_radius: i32,
    pending_shots: Vec<PendingShot>,
    pending_hits: Vec<(hecs::Entity, Fp)>,
    pending_explosions: Vec<Explosion>,
    dead_units: Vec<SimId>,
    dead_projectiles: Vec<SimId>,
}

impl SimWorld {
    pub fn new(config: SimConfig, seed: u64) -> Self {
        let mut rng = SimRng::new(seed);
        let terrain = Terrain::generate(config.map_units, config.terrain, &mut rng);
        let spatial_grid = SpatialGrid::new(config.map_units);
        let flow_field = FlowField::new(&terrain);
        let visibility = Visibility::new(config.map_units);
        Self {
            ecs: hecs::World::new(),
            terrain,
            spatial_grid,
            flow_field,
            rng,
            config,
            current_tick: 0,
            next_sim_id: 0,
            units: Vec::new(),
            projectiles: Vec::new(),
            obstacles: Vec::new(),
            blueprints: Arc::new(BlueprintDb::standard()),
            visibility,
            economies: [Economy::default(), Economy::default()],
            deposits: Vec::new(),
            occupancy: BTreeMap::new(),
            wrecks: Vec::new(),
            builders: Vec::new(),
            factories: Vec::new(),
            structures: Vec::new(),
            patrols: Vec::new(),
            commanders: [None, None],
            match_result: None,
            economy_dirty: false,
            max_mobile_radius: 0,
            max_structure_radius: 0,
            pending_shots: Vec::new(),
            pending_hits: Vec::new(),
            pending_explosions: Vec::new(),
            dead_units: Vec::new(),
            dead_projectiles: Vec::new(),
        }
    }

    /// Does the one-time map preparation now, so that it cannot land on the first tick.
    /// Call after adding obstacles. Safe to skip: it would otherwise run on first use.
    pub fn prepare_map(&mut self) {
        self.flow_field.prepare(&self.terrain);
    }

    pub fn add_box_obstacle(&mut self, rect: RectFp) {
        self.terrain.add_box_obstacle(rect);
        self.flow_field.invalidate();
        self.obstacles.push(rect);
    }

    pub(super) fn next_id(&mut self) -> SimId {
        let id = SimId(self.next_sim_id);
        self.next_sim_id += 1;
        id
    }

    pub fn spawn_unit(&mut self, spawn: UnitSpawn) -> SimId {
        assert!(self.terrain.in_bounds(spawn.position), "unit spawned outside the map");
        assert!(self.terrain.is_passable(spawn.position), "unit spawned in an impassable cell");
        let id = self.next_id();
        let ground = self.terrain.height_at(spawn.position);
        let entity = self.ecs.spawn((
            id,
            Position(Vec3Fp::new(spawn.position.x, spawn.position.y, ground + spawn.radius)),
            Velocity(Vec3Fp::ZERO),
            UnitStats {
                max_speed: spawn.max_speed,
                radius: spawn.radius,
                team: spawn.team,
                move_class: MoveClass::Land,
                is_structure: spawn.is_structure,
                vision: spawn.vision,
            },
            Health { current: spawn.health, max: spawn.health },
            Combat {
                range: spawn.range,
                reload_ticks: spawn.reload_ticks,
                ticks_until_ready: 0,
                projectile_speed: spawn.projectile_speed,
                projectile_damage: spawn.projectile_damage,
                projectile_splash: spawn.projectile_splash,
                target: None,
                forced_target: None,
                enabled: true,
            },
            Order::IDLE,
        ));
        self.units.push((id, entity));
        let largest = if spawn.is_structure { &mut self.max_structure_radius } else { &mut self.max_mobile_radius };
        *largest = (*largest).max(spawn.radius.to_bits());
        id
    }

    /// Spawns a projectile directly. Units use this through firing; tests and benchmarks can call it.
    pub fn spawn_projectile(&mut self, position: Vec3Fp, projectile: Projectile) -> SimId {
        let id = self.next_id();
        let entity = self.ecs.spawn((id, Position(position), projectile));
        self.projectiles.push((id, entity));
        id
    }

    pub(super) fn find_unit(&self, id: SimId) -> Option<(usize, hecs::Entity)> {
        let index = self.units.binary_search_by_key(&id, |unit| unit.0).ok()?;
        Some((index, self.units[index].1))
    }

    pub fn unit_entity(&self, id: SimId) -> Option<hecs::Entity> {
        self.find_unit(id).map(|(_, entity)| entity)
    }

    pub fn unit_position(&self, id: SimId) -> Option<Vec3Fp> {
        let (_, entity) = self.find_unit(id)?;
        Some(self.ecs.get::<&Position>(entity).ok()?.0)
    }

    pub fn unit_health(&self, id: SimId) -> Option<Fp> {
        let (_, entity) = self.find_unit(id)?;
        Some(self.ecs.get::<&Health>(entity).ok()?.current)
    }

    pub fn unit_order(&self, id: SimId) -> Option<Order> {
        let (_, entity) = self.find_unit(id)?;
        Some(*self.ecs.get::<&Order>(entity).ok()?)
    }

    /// Advances the world by exactly one 100 ms deterministic tick. Commands are applied
    /// in slice order, so callers must supply the same order on every instance.
    pub fn advance_tick(&mut self, commands: &[SimCommand]) {
        self.step_commands(commands);
        self.rebuild_grid();
        self.step_movement();
        self.rebuild_grid();
        self.step_visibility();
        self.step_combat();
        self.step_projectiles();
        self.step_production();
        self.step_cleanup();
        self.current_tick += 1;
    }

    pub fn step_commands(&mut self, commands: &[SimCommand]) {
        for command in commands {
            match command {
                SimCommand::Move { units, goal } => {
                    self.clear_unit_plans(units);
                    self.assign_route(units, OrderKind::Move, *goal, None);
                }
                SimCommand::AttackMove { units, goal } => {
                    self.clear_unit_plans(units);
                    self.assign_route(units, OrderKind::AttackMove, *goal, None);
                }
                SimCommand::Follow { units, target } => {
                    if let Some(position) = self.unit_position(*target) {
                        let followers: Vec<SimId> = units.iter().copied().filter(|id| id != target).collect();
                        self.clear_unit_plans(&followers);
                        self.assign_route(&followers, OrderKind::Move, position.xy(), Some(*target));
                    }
                }
                SimCommand::Stop { units } => {
                    self.clear_unit_plans(units);
                    for &id in units {
                        self.set_order(id, Order::IDLE);
                    }
                }
                SimCommand::Attack { units, target } => self.command_attack(units, *target),
                other => self.apply_production_command(other),
            }
        }
    }

    /// Forgets a unit's patrol, build tasks, and forced target. A new order replaces them.
    pub(super) fn clear_unit_plans(&mut self, units: &[SimId]) {
        for &id in units {
            let Some((_, entity)) = self.find_unit(id) else {
                continue;
            };
            if let Ok(mut combat) = self.ecs.get::<&mut Combat>(entity) {
                combat.forced_target = None;
            }
            if let Ok(mut builder) = self.ecs.get::<&mut Builder>(entity) {
                builder.tasks.clear();
            }
            if self.ecs.remove_one::<Patrol>(entity).is_ok() {
                self.patrols.retain(|patrol| patrol.0 != id);
            }
        }
    }

    fn command_attack(&mut self, units: &[SimId], target: SimId) {
        let Some(position) = self.unit_position(target) else {
            return;
        };
        let armed: Vec<SimId> = units
            .iter()
            .copied()
            .filter(|id| *id != target)
            .filter(|id| {
                self.find_unit(*id).is_some_and(|(_, entity)| {
                    self.ecs.get::<&Combat>(entity).is_ok_and(|combat| combat.range > Fp::ZERO)
                })
            })
            .collect();
        self.clear_unit_plans(&armed);
        for &id in &armed {
            let (_, entity) = self.find_unit(id).expect("just checked");
            self.ecs.get::<&mut Combat>(entity).expect("just checked").forced_target = Some(target);
        }
        self.assign_route(&armed, OrderKind::Attack, position.xy(), Some(target));
    }

    pub(super) fn set_order(&mut self, id: SimId, order: Order) {
        if let Some((_, entity)) = self.find_unit(id) {
            let mut current = self.ecs.get::<&mut Order>(entity).expect("units have orders");
            if current.kind != OrderKind::Idle {
                self.flow_field.release(current.route);
            }
            *current = order;
        }
    }

    pub(super) fn assign_route(&mut self, units: &[SimId], kind: OrderKind, goal: Vec2Fp, follow: Option<SimId>) {
        // Keeps the order given, because units take their orders in that order.
        let mut seen = BTreeSet::new();
        let mut members = Vec::with_capacity(units.len());
        let mut walkers = Vec::with_capacity(units.len());
        let mut largest_radius = 0;
        for &id in units {
            if let Some((_, entity)) = self.find_unit(id) {
                if !seen.insert(id) {
                    continue;
                }
                members.push(id);
                let stats = self.ecs.get::<&UnitStats>(entity).expect("units have stats");
                largest_radius = largest_radius.max(stats.radius.to_bits());
                if !stats.is_structure {
                    walkers.push((id, self.ecs.get::<&Position>(entity).expect("units have positions").0.xy(), stats.max_speed));
                }
            }
        }
        if members.is_empty() {
            return;
        }
        // Large groups stop further out, because they cannot all fit at the goal.
        let spread = (members.len() as u64).isqrt().max(1) as i64;
        let mut arrive_radius = Fp::from_bits((i64::from(largest_radius) * spread * 6 / 5) as i32);

        // A group ordered to a place ends up in a formation there.
        let mut slots: Vec<(SimId, Vec2Fp, u32)> = Vec::new();
        if follow.is_none() && matches!(kind, OrderKind::Move | OrderKind::AttackMove) && walkers.len() >= 2 {
            let positions: Vec<(SimId, Vec2Fp)> = walkers.iter().map(|&(id, position, _)| (id, position)).collect();
            let spacing = Fp::from_bits(largest_radius * FORMATION_SPACING_RADII / 2);
            let formation = plan_formation(&positions, goal, spacing);
            arrive_radius = formation.approach_radius;
            for (&(id, _, speed), &slot) in walkers.iter().zip(&formation.slots) {
                let slot = self.nearest_open_point(slot);
                // Time to cross the whole approach twice, plus a margin for crowding.
                let crossing = i64::from(arrive_radius.to_bits()) * 4 / i64::from(speed.to_bits().max(1));
                slots.push((id, slot, (crossing + SLOT_EXTRA_TICKS).min(i64::from(u32::MAX)) as u32));
            }
        }
        let Some(route) = self.flow_field.create_route(
            &self.terrain,
            self.current_tick,
            goal,
            members.len() as u32,
            arrive_radius,
            follow,
        ) else {
            return;
        };
        slots.sort_unstable_by_key(|slot| slot.0);
        for id in members {
            let (slot, slot_ticks) = slots
                .binary_search_by_key(&id, |slot| slot.0)
                .map_or((None, 0), |index| (Some(slots[index].1), slots[index].2));
            self.set_order(id, Order { kind, route, slot, slot_ticks });
        }
    }

    /// The point itself if it can be walked on and is on the map, or else the nearest open cell's center.
    fn nearest_open_point(&self, point: Vec2Fp) -> Vec2Fp {
        let max = self.terrain.max_coordinate_raw();
        let inside = Vec2Fp::new(Fp::from_bits(point.x.to_bits().clamp(0, max)), Fp::from_bits(point.y.to_bits().clamp(0, max)));
        if self.terrain.is_passable(inside) {
            return inside;
        }
        nearest_passable_cell(&self.terrain, inside).map_or(inside, |(x, y)| {
            let center = |cell: usize| Fp::from_bits((cell as i32 * TERRAIN_CELL_UNITS + TERRAIN_CELL_UNITS / 2) << FP_FRAC_BITS);
            Vec2Fp::new(center(x), center(y))
        })
    }

    pub fn rebuild_grid(&mut self) {
        let mut query = self.ecs.query::<(&Position, &UnitStats, &Order)>();
        let view = query.view();
        self.spatial_grid.rebuild(self.units.iter().map(|&(id, entity)| {
            let (position, stats, order) = view.get(entity).expect("unit list only holds live units");
            GridUnit {
                id,
                entity,
                team: stats.team,
                x: position.0.x.to_bits(),
                y: position.0.y.to_bits(),
                z: position.0.z.to_bits(),
                radius: stats.radius.to_bits(),
                is_structure: stats.is_structure,
                is_idle: order.kind == OrderKind::Idle,
            }
        }));
    }

    pub fn step_movement(&mut self) {
        let tick = self.current_tick;
        let units = &self.units;
        let grid = &self.spatial_grid;
        let terrain = &self.terrain;
        let flow = &mut self.flow_field;
        flow.update_moving_goals(terrain, tick, |target| {
            let index = units.binary_search_by_key(&target, |unit| unit.0).ok()?;
            let entry = grid.entry(index);
            Some(Vec2Fp::new(Fp::from_bits(entry.x), Fp::from_bits(entry.y)))
        });
        flow.build_pending(terrain, tick);

        let max_radius = self.max_mobile_radius;
        let max_coordinate = terrain.max_coordinate_raw();
        let mut query = self.ecs.query_mut::<(&mut Position, &mut Velocity, &UnitStats, &mut Order, &Combat)>();
        let mut view = query.view();
        for (index, &(_, entity)) in units.iter().enumerate() {
            let (position, velocity, stats, order, combat) = view.get_mut(entity).expect("unit list only holds live units");
            if stats.is_structure {
                continue;
            }
            let here = position.0.xy();

            // A structure was placed on top of this unit: walk straight out, through it if need be.
            if !terrain.is_passable(here) {
                if let Some((cell_x, cell_y)) = nearest_passable_cell(terrain, here) {
                    let half_cell = TERRAIN_CELL_UNITS << FP_FRAC_BITS >> 1;
                    let cell_size = TERRAIN_CELL_UNITS << FP_FRAC_BITS;
                    let exit = Vec2Fp::new(
                        Fp::from_bits(cell_x as i32 * cell_size + half_cell),
                        Fp::from_bits(cell_y as i32 * cell_size + half_cell),
                    );
                    let direction = (exit - here).normalized();
                    let speed = stats.max_speed.max(Fp::ONE);
                    let next = Vec2Fp::new(here.x + direction.x * speed, here.y + direction.y * speed);
                    position.0 = Vec3Fp::new(next.x, next.y, terrain.height_at(next) + stats.radius);
                }
                continue;
            }

            // Steering from the unit's order.
            let mut desired = Vec2Fp::ZERO;
            if order.kind != OrderKind::Idle {
                let goal = flow.goal(order.route).unwrap_or(here);
                let arrive_radius = flow.arrive_radius(order.route).unwrap_or(Fp::ZERO);
                let arrived = here.distance_squared(&goal) <= wide_mul(arrive_radius, arrive_radius);
                let may_walk = order.kind == OrderKind::Move || combat.target.is_none();
                if let Some(slot) = order.slot {
                    // Close enough to the group's goal to leave the route and walk to its own spot.
                    if arrived {
                        let close_enough = stats.radius / 2;
                        if order.slot_ticks == 0 || here.distance_squared(&slot) <= wide_mul(close_enough, close_enough) {
                            flow.release(order.route);
                            *order = Order::IDLE;
                        } else if may_walk {
                            order.slot_ticks -= 1;
                            let direction = (slot - here).normalized();
                            let remaining = sqrt_wide(here.distance_squared(&slot));
                            let speed = stats.max_speed.min(remaining);
                            desired = Vec2Fp::new(direction.x * speed, direction.y * speed);
                        }
                    } else if may_walk {
                        if let FlowSample::Direction(direction) = flow.sample(order.route, tick, here, terrain) {
                            desired = Vec2Fp::new(direction.x * stats.max_speed, direction.y * stats.max_speed);
                        }
                    }
                } else if arrived {
                    if !flow.is_following(order.route) {
                        flow.release(order.route);
                        *order = Order::IDLE;
                    }
                } else if may_walk {
                    if let FlowSample::Direction(direction) = flow.sample(order.route, tick, here, terrain) {
                        desired = Vec2Fp::new(direction.x * stats.max_speed, direction.y * stats.max_speed);
                    }
                }
            }

            // Ease velocity towards the steering direction to round off the 8-way flow directions.
            let half = |from: Fp, to: Fp| Fp::from_bits(from.to_bits() + (to.to_bits() - from.to_bits()) / 2);
            velocity.0 = Vec3Fp::new(half(velocity.0.x, desired.x), half(velocity.0.y, desired.y), Fp::ZERO);

            // Local separation. Reads positions from the grid, so results cannot depend on update order.
            let me = grid.entry(index);
            let (mut push_x, mut push_y) = (0i64, 0i64);
            let mut touches_settled_friend = false;
            grid.for_each_candidate(None, Layer::Mobile, me.x, me.y, me.radius + max_radius, |other| {
                if other.index == me.index {
                    return;
                }
                let (dx, dy) = (i64::from(me.x - other.x), i64::from(me.y - other.y));
                let contact = i64::from(me.radius + other.radius);
                let distance_squared = dx * dx + dy * dy;
                if distance_squared >= contact * contact {
                    return;
                }
                touches_settled_friend |= other.is_idle && other.team == me.team;
                let distance = (distance_squared as u64).isqrt() as i64;
                let overlap = contact - distance;
                if distance == 0 {
                    // Exactly overlapping units push apart along a direction from their ids.
                    let pair = (me.id.0.min(other.id.0) ^ me.id.0.max(other.id.0)) as usize % 8;
                    let (nx, ny) = [(1, 0), (0, 1), (1, 1), (1, -1), (2, 1), (1, 2), (2, -1), (1, -2)][pair];
                    let sign = if me.id > other.id { 1 } else { -1 };
                    push_x += sign * nx * overlap / 2;
                    push_y += sign * ny * overlap / 2;
                } else {
                    push_x += dx * overlap / distance;
                    push_y += dy * overlap / distance;
                }
            });
            // A crowd can cover a shared goal, such as a rally point, so that later units can
            // never reach it. A unit that bumps into a settled friend nearby has arrived too.
            // Units with a spot of their own have a time limit instead.
            if touches_settled_friend
                && order.slot.is_none()
                && matches!(order.kind, OrderKind::Move | OrderKind::AttackMove)
                && !flow.is_following(order.route)
            {
                let settle_radius = stats.radius * CROWD_SETTLE_RADII;
                let goal = flow.goal(order.route).unwrap_or(here);
                if here.distance_squared(&goal) <= wide_mul(settle_radius, settle_radius) {
                    flow.release(order.route);
                    *order = Order::IDLE;
                }
            }

            // Soft and capped, so dense crowds spread out instead of jittering. The cap keeps up
            // with the unit's own speed, so that fast units cannot simply overpower separation.
            let cap = (i64::from(me.radius) / 2).max(i64::from(stats.max_speed.to_bits()));
            let push_x = (push_x / 3).clamp(-cap, cap) as i32;
            let push_y = (push_y / 3).clamp(-cap, cap) as i32;

            let step_x = velocity.0.x.to_bits() + push_x;
            let step_y = velocity.0.y.to_bits() + push_y;
            if step_x == 0 && step_y == 0 {
                continue;
            }

            // One axis at a time against the cost grid, so units slide along walls.
            let mut next = here;
            let candidate_x = Vec2Fp::new(Fp::from_bits((here.x.to_bits() + step_x).clamp(0, max_coordinate)), next.y);
            if terrain.is_passable(candidate_x) {
                next = candidate_x;
            }
            let candidate_y = Vec2Fp::new(next.x, Fp::from_bits((here.y.to_bits() + step_y).clamp(0, max_coordinate)));
            if terrain.is_passable(candidate_y) {
                next = candidate_y;
            }
            if next != here {
                position.0 = Vec3Fp::new(next.x, next.y, terrain.height_at(next) + stats.radius);
            }
        }
    }

    /// Updates what each team can see. Units take turns, which spreads out the cost.
    pub fn step_visibility(&mut self) {
        if !self.config.fog_of_war {
            return;
        }
        let tick = self.current_tick;
        self.visibility.set_tick(tick);
        let mut query = self.ecs.query::<&UnitStats>();
        let view = query.view();
        for (index, &(id, entity)) in self.units.iter().enumerate() {
            // Everyone looks on the first ticks, so that a new match starts with sight.
            if tick >= VISION_REFRESH_TICKS && id.0.wrapping_add(tick) % VISION_REFRESH_TICKS != 0 {
                continue;
            }
            let stats = view.get(entity).expect("unit list only holds live units");
            let entry = self.spatial_grid.entry(index);
            self.visibility.stamp_disc(stats.team, entry.x, entry.y, stats.vision);
        }
    }

    pub fn step_combat(&mut self) {
        let tick = self.current_tick;
        let units = &self.units;
        let grid = &self.spatial_grid;
        let visibility = &self.visibility;
        let fog = self.config.fog_of_war;
        let shots = &mut self.pending_shots;
        let (max_mobile, max_structure) = (self.max_mobile_radius, self.max_structure_radius);
        shots.clear();
        let mut finished_attacks: Vec<SimId> = Vec::new();
        let mut query = self.ecs.query_mut::<(&UnitStats, &mut Combat)>();
        let mut view = query.view();
        for (index, &(id, entity)) in units.iter().enumerate() {
            let (stats, combat) = view.get_mut(entity).expect("unit list only holds live units");
            combat.ticks_until_ready = combat.ticks_until_ready.saturating_sub(1);
            if !combat.enabled || combat.range == Fp::ZERO {
                continue;
            }
            let me = grid.entry(index);
            let range = i64::from(combat.range.to_bits());
            let squared_distance = |x: i32, y: i32| {
                let (dx, dy) = (i64::from(x - me.x), i64::from(y - me.y));
                dx * dx + dy * dy
            };
            // Range reaches the edge of a target, so that large structures can be hit from their range.
            let can_engage = |entry: &super::spatial::GridEntry| {
                let reach = range + i64::from(entry.radius);
                squared_distance(entry.x, entry.y) <= reach * reach
                    && (!fog || visibility.is_visible(stats.team, entry.x, entry.y))
            };
            let locate = |target: SimId| units.binary_search_by_key(&target, |unit| unit.0).ok().map(|i| grid.entry(i));

            if let Some(forced) = combat.forced_target {
                match locate(forced) {
                    Some(entry) => combat.target = can_engage(entry).then_some(forced),
                    None => {
                        combat.forced_target = None;
                        combat.target = None;
                        finished_attacks.push(id);
                    }
                }
            } else {
                // Drop a target that has died, moved out of range, or gone out of sight.
                if let Some(target) = combat.target {
                    if !locate(target).is_some_and(can_engage) {
                        combat.target = None;
                    }
                }

                // Searches are spread across ticks. Rings grow outward, so units at the
                // front of a battle stop early with the nearest enemy. Mobile units come first.
                if combat.target.is_none() && id.0.wrapping_add(tick) % TARGET_SEARCH_INTERVAL == 0 {
                    let mut ring = (8i64 << FP_FRAC_BITS).max(range / 8);
                    loop {
                        let radius = ring.min(range);
                        let mut best: Option<(bool, i64, SimId)> = None;
                        let mut consider = |other: &super::spatial::GridEntry| {
                            let distance = squared_distance(other.x, other.y);
                            let reach = radius + i64::from(other.radius);
                            if distance <= reach * reach && (!fog || visibility.is_visible(stats.team, other.x, other.y)) {
                                let candidate = (other.is_structure, distance, other.id);
                                if best.is_none_or(|current| candidate < current) {
                                    best = Some(candidate);
                                }
                            }
                        };
                        let enemy = Some(stats.team.enemy());
                        grid.for_each_candidate(enemy, Layer::Mobile, me.x, me.y, radius as i32 + max_mobile, &mut consider);
                        if max_structure > 0 {
                            grid.for_each_candidate(enemy, Layer::Structure, me.x, me.y, radius as i32 + max_structure, &mut consider);
                        }
                        if let Some((_, _, found)) = best {
                            combat.target = Some(found);
                            break;
                        }
                        if radius >= range {
                            break;
                        }
                        ring *= 2;
                    }
                }
            }

            if combat.ticks_until_ready == 0 {
                if let Some(target) = combat.target {
                    let entry = locate(target).expect("target was just validated or found");
                    let raw = |x: i32, y: i32, z: i32| Vec3Fp::new(Fp::from_bits(x), Fp::from_bits(y), Fp::from_bits(z));
                    shots.push(PendingShot {
                        source: id,
                        team: stats.team,
                        from: raw(me.x, me.y, me.z),
                        to: raw(entry.x, entry.y, entry.z),
                        speed: combat.projectile_speed,
                        damage: combat.projectile_damage,
                        splash: combat.projectile_splash,
                    });
                    combat.ticks_until_ready = combat.reload_ticks;
                }
            }
        }

        for id in finished_attacks {
            self.set_order(id, Order::IDLE);
        }
        let friendly_fire = self.config.friendly_fire;
        for shot in std::mem::take(&mut self.pending_shots) {
            let solution = firing_solution(shot.from, shot.to, shot.speed);
            self.spawn_projectile(
                shot.from,
                Projectile {
                    velocity: solution.velocity,
                    damage: shot.damage,
                    source: shot.source,
                    source_team: shot.team,
                    // Long enough to fly past a dodging target and reach the ground.
                    lifetime_ticks: solution.flight_ticks + solution.flight_ticks / 2 + 10,
                    splash_radius: shot.splash,
                    allied_safe_ticks: if friendly_fire { solution.flight_ticks } else { 0 },
                },
            );
        }
    }

    pub fn step_projectiles(&mut self) {
        let terrain = &self.terrain;
        let grid = &self.spatial_grid;
        let hits = &mut self.pending_hits;
        let explosions = &mut self.pending_explosions;
        let dead = &mut self.dead_projectiles;
        let friendly_fire = self.config.friendly_fire;
        let (max_mobile, max_structure) = (self.max_mobile_radius, self.max_structure_radius);
        hits.clear();
        dead.clear();
        let mut query = self.ecs.query_mut::<(&mut Position, &mut Projectile)>();
        let mut view = query.view();
        for &(id, entity) in &self.projectiles {
            let (position, projectile) = view.get_mut(entity).expect("projectile list only holds live projectiles");
            let start = position.0;
            let step = projectile.velocity;
            let end = start + step;
            position.0 = end;
            projectile.velocity.z -= GRAVITY;
            projectile.lifetime_ticks = projectile.lifetime_ticks.saturating_sub(1);
            let allies_are_safe = projectile.allied_safe_ticks > 0;
            projectile.allied_safe_ticks = projectile.allied_safe_ticks.saturating_sub(1);

            let probe = probe_terrain(terrain, start, step);

            // Shells far above the ground cannot touch a unit, which spares most grid queries.
            let lowest = start.z.min(end.z);
            let reach = Fp::from_bits(max_mobile.max(max_structure) * 2);
            let mut best: Option<(i32, SimId, hecs::Entity)> = None;
            if lowest <= probe.highest_ground + reach {
                let half = (step.x.to_bits().abs() + step.y.to_bits().abs()) / 2;
                let center_x = start.x.to_bits() + step.x.to_bits() / 2;
                let center_y = start.y.to_bits() + step.y.to_bits() / 2;
                let team_filter = if friendly_fire && !allies_are_safe { None } else { Some(projectile.source_team.enemy()) };
                let source = projectile.source;
                let mut consider = |other: &super::spatial::GridEntry| {
                    if other.id == source {
                        return;
                    }
                    if let Some(along) = entry_hit(other, start, step) {
                        let candidate = (along, other.id, other.entity);
                        if best.is_none_or(|current| (candidate.0, candidate.1) < (current.0, current.1)) {
                            best = Some(candidate);
                        }
                    }
                };
                grid.for_each_candidate(team_filter, Layer::Mobile, center_x, center_y, half + max_mobile, &mut consider);
                if max_structure > 0 {
                    grid.for_each_candidate(team_filter, Layer::Structure, center_x, center_y, half + max_structure, &mut consider);
                }
            }

            // Splash shells detonate where they stop, and damage everything nearby.
            let splash = projectile.splash_radius > Fp::ZERO;
            let blast = Explosion { x: end.x.to_bits(), y: end.y.to_bits(), radius: projectile.splash_radius, damage: projectile.damage };

            match (best, probe.hit) {
                (Some((along, _, target)), terrain_hit) if terrain_hit.is_none_or(|ground| along <= ground) => {
                    if splash {
                        explosions.push(blast);
                    } else {
                        hits.push((target, projectile.damage));
                    }
                    dead.push(id);
                }
                (_, Some(_)) => {
                    if splash {
                        explosions.push(blast);
                    }
                    dead.push(id);
                }
                _ => {
                    if projectile.lifetime_ticks == 0 || !terrain.in_bounds(end.xy()) {
                        dead.push(id);
                    }
                }
            }
        }

    }

    /// Queues a hit on every unit within a blast, friend or foe.
    fn apply_explosion(&mut self, blast: Explosion) {
        let (grid, hits) = (&self.spatial_grid, &mut self.pending_hits);
        let radius = i64::from(blast.radius.to_bits());
        let mut consider = |other: &super::spatial::GridEntry| {
            let (dx, dy) = (i64::from(other.x - blast.x), i64::from(other.y - blast.y));
            let reach = radius + i64::from(other.radius);
            if dx * dx + dy * dy <= reach * reach {
                hits.push((other.entity, blast.damage));
            }
        };
        let search = blast.radius.to_bits();
        grid.for_each_candidate(None, Layer::Mobile, blast.x, blast.y, search + self.max_mobile_radius, &mut consider);
        grid.for_each_candidate(None, Layer::Structure, blast.x, blast.y, search + self.max_structure_radius, &mut consider);
    }

    /// Resolves this tick's damage, including blasts from units that die here and the
    /// chain reactions they cause, then removes everything that died.
    pub fn step_cleanup(&mut self) {
        self.dead_units.clear();
        if !self.config.invulnerable_units {
            loop {
                for blast in std::mem::take(&mut self.pending_explosions) {
                    self.apply_explosion(blast);
                }
                if self.pending_hits.is_empty() {
                    break;
                }
                for (target, damage) in std::mem::take(&mut self.pending_hits) {
                    let (id, position, health, blueprint) = self
                        .ecs
                        .query_one_mut::<(&SimId, &Position, &mut Health, Option<&Blueprinted>)>(target)
                        .expect("hit entities are live until the end of this step");
                    let was_alive = health.current > Fp::ZERO;
                    health.current = health.current.saturating_sub(damage);
                    if was_alive && health.current <= Fp::ZERO {
                        let (id, x, y, blueprint) =
                            (*id, position.0.x.to_bits(), position.0.y.to_bits(), blueprint.map(|b| b.0));
                        self.dead_units.push(id);
                        if let Some(blast) = blueprint.and_then(|b| self.blueprints.get(b).death_explosion) {
                            self.pending_explosions.push(Explosion { x, y, radius: blast.radius, damage: blast.damage });
                        }
                    }
                }
            }
        }
        self.pending_explosions.clear();
        self.pending_hits.clear();
        self.resolve_match_end();

        if !self.dead_units.is_empty() {
            self.dead_units.sort_unstable();
            self.dead_units.dedup();
            for index in 0..self.dead_units.len() {
                let id = self.dead_units[index];
                let (_, entity) = self.find_unit(id).expect("dead units are still listed");
                self.remove_unit(entity);
            }
            let dead = &self.dead_units;
            let alive = |unit: &(SimId, hecs::Entity)| dead.binary_search(&unit.0).is_err();
            self.units.retain(alive);
            self.builders.retain(alive);
            self.factories.retain(alive);
            self.structures.retain(alive);
            self.patrols.retain(alive);
            self.dead_units.clear();
        }
        if !self.dead_projectiles.is_empty() {
            let dead = &self.dead_projectiles;
            let ecs = &mut self.ecs;
            // Both lists are sorted by SimId, so one forward pass finds every match.
            let mut next_dead = 0;
            self.projectiles.retain(|&(id, entity)| {
                if dead.get(next_dead) == Some(&id) {
                    next_dead += 1;
                    ecs.despawn(entity).expect("dead projectiles are live until now");
                    false
                } else {
                    true
                }
            });
            self.dead_projectiles.clear();
        }
    }

    /// A team whose commander died is eliminated, and everything it owns goes with it.
    fn resolve_match_end(&mut self) {
        if self.match_result.is_some() || self.dead_units.is_empty() {
            return;
        }
        let lost = |team: Team| self.commanders[team.index()].is_some_and(|commander| self.dead_units.contains(&commander));
        let (blue_lost, red_lost) = (lost(Team::Blue), lost(Team::Red));
        self.match_result = match (blue_lost, red_lost) {
            (true, true) => Some(MatchResult::Draw),
            (true, false) => Some(MatchResult::Winner(Team::Red)),
            (false, true) => Some(MatchResult::Winner(Team::Blue)),
            (false, false) => None,
        };
        for (team, eliminated) in [(Team::Blue, blue_lost), (Team::Red, red_lost)] {
            if !eliminated {
                continue;
            }
            for index in 0..self.units.len() {
                let (id, entity) = self.units[index];
                let on_team = self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team == team);
                if on_team && !self.dead_units.contains(&id) {
                    self.dead_units.push(id);
                }
            }
        }
    }

    /// Despawns a dead unit, leaving a wreck if it had any mass in it.
    fn remove_unit(&mut self, entity: hecs::Entity) {
        let order = *self.ecs.get::<&Order>(entity).expect("units have orders");
        if order.kind != OrderKind::Idle {
            self.flow_field.release(order.route);
        }
        let position = self.ecs.get::<&Position>(entity).expect("units have positions").0;
        let radius = self.ecs.get::<&UnitStats>(entity).expect("units have stats").radius;
        if let Ok(footprint) = self.ecs.get::<&Footprint>(entity).map(|footprint| *footprint) {
            self.clear_footprint(footprint);
            self.economy_dirty = true;
        }
        let wreck_mass = self.ecs.get::<&Blueprinted>(entity).ok().map(|blueprint| {
            let built = self.ecs.get::<&Construction>(entity).map_or(Res::ONE, |construction| construction.progress);
            self.blueprints.get(blueprint.0).mass * WRECK_MASS_PERMILLE / 1000 * built
        });
        self.ecs.despawn(entity).expect("dead units are live until now");
        if let Some(mass) = wreck_mass.filter(|mass| *mass > Res::ZERO) {
            let wreck_id = self.next_id();
            let wreck = self.ecs.spawn((wreck_id, Wreck { mass, position: position.xy(), radius }));
            self.wrecks.push((wreck_id, wreck));
        }
    }

    /// Computes a 64-bit checksum of the entire simulation state. Every value that can
    /// affect a future tick is hashed. `Hasher::write_i32` and friends use native
    /// endianness, so all values go through `to_le_bytes`.
    pub fn compute_checksum(&self) -> u64 {
        let mut hasher = XxHash64::with_seed(0);
        hasher.write(&self.current_tick.to_le_bytes());
        hasher.write(&self.next_sim_id.to_le_bytes());
        hasher.write(&self.rng.state().to_le_bytes());
        hasher.write(&self.terrain.content_hash().to_le_bytes());
        hasher.write(&[
            u8::from(self.config.friendly_fire),
            u8::from(self.config.invulnerable_units),
            u8::from(self.config.fog_of_war),
        ]);
        hasher.write(&self.max_mobile_radius.to_le_bytes());
        hasher.write(&self.max_structure_radius.to_le_bytes());
        hasher.write(&self.blueprints.content_hash().to_le_bytes());
        for economy in &self.economies {
            economy.hash_state(&mut hasher);
        }
        for &(x, y) in &self.deposits {
            hasher.write(&x.to_le_bytes());
            hasher.write(&y.to_le_bytes());
        }
        for (&cell, id) in &self.occupancy {
            hasher.write(&cell.to_le_bytes());
            hasher.write(&id.0.to_le_bytes());
        }
        for commander in &self.commanders {
            hasher.write(&commander.map_or(u32::MAX, |id| id.0).to_le_bytes());
        }
        hasher.write(&[match self.match_result {
            None => 0,
            Some(MatchResult::Draw) => 1,
            Some(MatchResult::Winner(team)) => 2 + team as u8,
        }]);
        if self.config.fog_of_war {
            self.visibility.hash_state(&mut hasher);
        }

        hasher.write(&(self.units.len() as u64).to_le_bytes());
        for &(id, entity) in &self.units {
            hasher.write(&id.0.to_le_bytes());
            self.hash_unit(entity, &mut hasher);
        }
        hasher.write(&(self.projectiles.len() as u64).to_le_bytes());
        for &(id, entity) in &self.projectiles {
            hasher.write(&id.0.to_le_bytes());
            self.hash_projectile(entity, &mut hasher);
        }
        hasher.write(&(self.wrecks.len() as u64).to_le_bytes());
        for &(id, entity) in &self.wrecks {
            hasher.write(&id.0.to_le_bytes());
            let wreck = self.ecs.get::<&Wreck>(entity).expect("wreck list only holds live wrecks");
            hasher.write(&wreck.mass.to_bits().to_le_bytes());
            write_fp(&mut hasher, wreck.position.x);
            write_fp(&mut hasher, wreck.position.y);
        }
        self.flow_field.hash_state(&mut hasher);
        hasher.finish()
    }

    fn hash_unit(&self, entity: hecs::Entity, hasher: &mut impl Hasher) {
        let mut query = self
            .ecs
            .query_one::<(&Position, &Velocity, &UnitStats, &Health, &Combat, &Order)>(entity)
            .expect("unit list only holds live units");
        let (position, velocity, stats, health, combat, order) = query.get().expect("units have every component");
        write_vec3(hasher, position.0);
        write_vec3(hasher, velocity.0);
        write_fp(hasher, stats.max_speed);
        write_fp(hasher, stats.radius);
        hasher.write(&[stats.team as u8, stats.move_class as u8, u8::from(stats.is_structure)]);
        write_fp(hasher, stats.vision);
        write_fp(hasher, health.current);
        write_fp(hasher, health.max);
        write_fp(hasher, combat.range);
        hasher.write(&combat.reload_ticks.to_le_bytes());
        hasher.write(&combat.ticks_until_ready.to_le_bytes());
        write_fp(hasher, combat.projectile_speed);
        write_fp(hasher, combat.projectile_damage);
        write_fp(hasher, combat.projectile_splash);
        hasher.write(&combat.target.map_or(u32::MAX, |target| target.0).to_le_bytes());
        hasher.write(&combat.forced_target.map_or(u32::MAX, |target| target.0).to_le_bytes());
        hasher.write(&[u8::from(combat.enabled), order.kind as u8]);
        hasher.write(&order.route.to_le_bytes());
        if let Some(slot) = order.slot {
            write_fp(hasher, slot.x);
            write_fp(hasher, slot.y);
        }
        hasher.write(&order.slot_ticks.to_le_bytes());
        drop(query);
        self.hash_unit_extras(entity, hasher);
    }

    fn hash_projectile(&self, entity: hecs::Entity, hasher: &mut impl Hasher) {
        let mut query = self
            .ecs
            .query_one::<(&Position, &Projectile)>(entity)
            .expect("projectile list only holds live projectiles");
        let (position, projectile) = query.get().expect("projectiles have every component");
        write_vec3(hasher, position.0);
        write_vec3(hasher, projectile.velocity);
        write_fp(hasher, projectile.damage);
        hasher.write(&projectile.source.0.to_le_bytes());
        hasher.write(&[projectile.source_team as u8]);
        hasher.write(&projectile.lifetime_ticks.to_le_bytes());
        write_fp(hasher, projectile.splash_radius);
        hasher.write(&projectile.allied_safe_ticks.to_le_bytes());
    }

    /// Copies everything, with no fog of war. For the stress-test scenarios.
    pub fn snapshot(&self) -> SimSnapshot {
        self.snapshot_for(None)
    }

    /// Copies the state the renderer needs, limited to what `viewer` can see.
    /// Rendering never touches `SimWorld` directly.
    pub fn snapshot_for(&self, viewer: Option<Team>) -> SimSnapshot {
        let seen = |team: Team, x: i32, y: i32| match viewer {
            Some(viewer) if self.config.fog_of_war && team != viewer => self.visibility.is_visible(viewer, x, y),
            _ => true,
        };
        let mut unit_query = self.ecs.query::<(&Position, &UnitStats, &Health, Option<&Blueprinted>, Option<&Construction>)>();
        let unit_view = unit_query.view();
        let units = self
            .units
            .iter()
            .filter_map(|&(id, entity)| {
                let (position, stats, health, blueprint, construction) = unit_view.get(entity).expect("unit list only holds live units");
                let (x, y, z) = (position.0.x.to_bits(), position.0.y.to_bits(), position.0.z.to_bits());
                seen(stats.team, x, y).then(|| UnitView {
                    id,
                    team: stats.team,
                    x,
                    y,
                    z,
                    radius: stats.radius.to_bits(),
                    blueprint: blueprint.map_or(u16::MAX, |blueprint| blueprint.0 .0),
                    health_permille: permille(i64::from(health.current.to_bits()), i64::from(health.max.to_bits())),
                    built_permille: construction.map_or(1000, |construction| {
                        (construction.progress.to_bits() * 1000 / Res::ONE.to_bits()).clamp(0, 1000) as u16
                    }),
                    is_structure: stats.is_structure,
                })
            })
            .collect();
        let mut projectile_query = self.ecs.query::<(&Position, &Projectile)>();
        let projectile_view = projectile_query.view();
        let projectiles = self
            .projectiles
            .iter()
            .filter_map(|&(id, entity)| {
                let (position, projectile) = projectile_view.get(entity).expect("projectile list only holds live projectiles");
                let (x, y, z) = (position.0.x.to_bits(), position.0.y.to_bits(), position.0.z.to_bits());
                seen(projectile.source_team, x, y).then_some(ProjectileView { id, team: projectile.source_team, x, y, z })
            })
            .collect();
        let wrecks = self
            .wrecks
            .iter()
            .filter_map(|&(id, entity)| {
                let wreck = self.ecs.get::<&Wreck>(entity).expect("wreck list only holds live wrecks");
                let (x, y) = (wreck.position.x.to_bits(), wreck.position.y.to_bits());
                // Wrecks belong to no one, so use the viewer's own sight for both cases.
                let visible = viewer.is_none_or(|viewer| !self.config.fog_of_war || self.visibility.is_visible(viewer, x, y));
                visible.then(|| WreckView {
                    id,
                    x,
                    y,
                    z: (self.terrain.height_at(wreck.position) + wreck.radius / 2).to_bits(),
                    radius: wreck.radius.to_bits(),
                    mass: wreck.mass.to_num::<i32>(),
                })
            })
            .collect();
        let factories = viewer.map_or_else(Vec::new, |viewer| {
            self.factories
                .iter()
                .filter_map(|&(id, entity)| {
                    let stats = self.ecs.get::<&UnitStats>(entity).expect("factories have stats");
                    if stats.team != viewer {
                        return None;
                    }
                    let factory = self.ecs.get::<&Factory>(entity).expect("factory list only holds factories");
                    Some(FactoryView {
                        id,
                        queue: factory.queue.iter().map(|blueprint| blueprint.0).collect(),
                        repeat: factory.repeat,
                        progress_permille: (factory.progress.to_bits() * 1000 / Res::ONE.to_bits()).clamp(0, 1000) as u16,
                        rally: factory.rally.map(|point| {
                            [point.x.to_bits(), point.y.to_bits(), self.terrain.height_at(point).to_bits()]
                        }),
                    })
                })
                .collect()
        });
        let plans = viewer.map_or_else(Vec::new, |viewer| self.plans_for(viewer));
        let economy = viewer.map_or_else(EconomyView::default, |viewer| self.economies[viewer.index()].view());
        let fog = match viewer {
            Some(viewer) if self.config.fog_of_war => Some(Arc::new(self.visibility.fog_bytes(viewer))),
            _ => None,
        };
        SimSnapshot {
            tick: self.current_tick,
            units,
            projectiles,
            wrecks,
            factories,
            plans,
            economy,
            fog,
            match_result: self.match_result,
        }
    }
}

fn permille(value: i64, max: i64) -> u16 {
    if max <= 0 {
        return 0;
    }
    (value * 1000 / max).clamp(0, 1000) as u16
}

fn write_fp(hasher: &mut impl Hasher, value: Fp) {
    hasher.write(&value.to_bits().to_le_bytes());
}

fn write_vec3(hasher: &mut impl Hasher, value: Vec3Fp) {
    write_fp(hasher, value.x);
    write_fp(hasher, value.y);
    write_fp(hasher, value.z);
}
