use std::hash::Hasher;
use twox_hash::XxHash64;

use super::combat::{entry_hit, firing_solution, probe_terrain, GRAVITY};
use super::commands::SimCommand;
use super::components::{
    Combat, Health, MoveClass, Order, OrderKind, Position, Projectile, SimId, Team, UnitStats, Velocity,
};
use super::config::{SimConfig, TARGET_SEARCH_INTERVAL};
use super::flowfield::{FlowField, FlowSample};
use super::spatial::{GridUnit, SpatialGrid};
use super::terrain::Terrain;
use crate::math::fp::{Fp, Vec2Fp, Vec3Fp, FP_FRAC_BITS};
use crate::math::rect::RectFp;
use crate::math::rng::SimRng;

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
}

struct PendingShot {
    source: SimId,
    team: Team,
    from: Vec3Fp,
    to: Vec3Fp,
    speed: Fp,
    damage: Fp,
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
    max_unit_radius: i32,
    pending_shots: Vec<PendingShot>,
    pending_hits: Vec<(hecs::Entity, Fp)>,
    dead_units: Vec<SimId>,
    dead_projectiles: Vec<SimId>,
}

impl SimWorld {
    pub fn new(config: SimConfig, seed: u64) -> Self {
        let mut rng = SimRng::new(seed);
        let terrain = Terrain::generate(config.map_units, config.terrain, &mut rng);
        let spatial_grid = SpatialGrid::new(config.map_units);
        let flow_field = FlowField::new(&terrain);
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
            max_unit_radius: 0,
            pending_shots: Vec::new(),
            pending_hits: Vec::new(),
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

    fn next_id(&mut self) -> SimId {
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
            },
            Health { current: spawn.health, max: spawn.health },
            Combat {
                range: spawn.range,
                reload_ticks: spawn.reload_ticks,
                ticks_until_ready: 0,
                projectile_speed: spawn.projectile_speed,
                projectile_damage: spawn.projectile_damage,
                target: None,
            },
            Order::IDLE,
        ));
        self.units.push((id, entity));
        self.max_unit_radius = self.max_unit_radius.max(spawn.radius.to_bits());
        id
    }

    /// Spawns a projectile directly. Units use this through firing; tests and benchmarks can call it.
    pub fn spawn_projectile(&mut self, position: Vec3Fp, projectile: Projectile) -> SimId {
        let id = self.next_id();
        let entity = self.ecs.spawn((id, Position(position), projectile));
        self.projectiles.push((id, entity));
        id
    }

    fn find_unit(&self, id: SimId) -> Option<(usize, hecs::Entity)> {
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
        self.step_combat();
        self.step_projectiles();
        self.step_cleanup();
        self.current_tick += 1;
    }

    pub fn step_commands(&mut self, commands: &[SimCommand]) {
        for command in commands {
            match command {
                SimCommand::Move { units, goal } => self.assign_route(units, OrderKind::Move, *goal, None),
                SimCommand::AttackMove { units, goal } => self.assign_route(units, OrderKind::AttackMove, *goal, None),
                SimCommand::Follow { units, target } => {
                    if let Some(position) = self.unit_position(*target) {
                        let followers: Vec<SimId> = units.iter().copied().filter(|id| id != target).collect();
                        self.assign_route(&followers, OrderKind::Move, position.xy(), Some(*target));
                    }
                }
                SimCommand::Stop { units } => {
                    for &id in units {
                        self.set_order(id, Order::IDLE);
                    }
                }
            }
        }
    }

    fn set_order(&mut self, id: SimId, order: Order) {
        if let Some((_, entity)) = self.find_unit(id) {
            let mut current = self.ecs.get::<&mut Order>(entity).expect("units have orders");
            if current.kind != OrderKind::Idle {
                self.flow_field.release(current.route);
            }
            *current = order;
        }
    }

    fn assign_route(&mut self, units: &[SimId], kind: OrderKind, goal: Vec2Fp, follow: Option<SimId>) {
        let mut members = Vec::with_capacity(units.len());
        let mut largest_radius = 0;
        for &id in units {
            if let Some((_, entity)) = self.find_unit(id) {
                members.push(id);
                let stats = self.ecs.get::<&UnitStats>(entity).expect("units have stats");
                largest_radius = largest_radius.max(stats.radius.to_bits());
            }
        }
        if members.is_empty() {
            return;
        }
        // Large groups stop further out, because they cannot all fit at the goal.
        let spread = (members.len() as u64).isqrt().max(1) as i64;
        let arrive_radius = Fp::from_bits((i64::from(largest_radius) * spread * 6 / 5) as i32);
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
        for id in members {
            self.set_order(id, Order { kind, route });
        }
    }

    pub fn rebuild_grid(&mut self) {
        let mut query = self.ecs.query::<(&Position, &UnitStats)>();
        let view = query.view();
        self.spatial_grid.rebuild(self.units.iter().map(|&(id, entity)| {
            let (position, stats) = view.get(entity).expect("unit list only holds live units");
            GridUnit {
                id,
                entity,
                team: stats.team,
                x: position.0.x.to_bits(),
                y: position.0.y.to_bits(),
                z: position.0.z.to_bits(),
                radius: stats.radius.to_bits(),
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

        let max_radius = self.max_unit_radius;
        let max_coordinate = terrain.max_coordinate_raw();
        let mut query = self.ecs.query_mut::<(&mut Position, &mut Velocity, &UnitStats, &mut Order, &Combat)>();
        let mut view = query.view();
        for (index, &(_, entity)) in units.iter().enumerate() {
            let (position, velocity, stats, order, combat) = view.get_mut(entity).expect("unit list only holds live units");
            let here = position.0.xy();

            // Steering from the unit's order.
            let mut desired = Vec2Fp::ZERO;
            if order.kind != OrderKind::Idle {
                let goal = flow.goal(order.route).unwrap_or(here);
                let arrive_radius = flow.arrive_radius(order.route).unwrap_or(Fp::ZERO);
                let arrived = here.distance_squared(&goal) <= crate::math::fp::wide_mul(arrive_radius, arrive_radius);
                if arrived {
                    if !flow.is_following(order.route) {
                        flow.release(order.route);
                        *order = Order::IDLE;
                    }
                } else if order.kind != OrderKind::AttackMove || combat.target.is_none() {
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
            grid.for_each_candidate(None, me.x, me.y, me.radius + max_radius, |other| {
                if other.index == me.index {
                    return;
                }
                let (dx, dy) = (i64::from(me.x - other.x), i64::from(me.y - other.y));
                let contact = i64::from(me.radius + other.radius);
                let distance_squared = dx * dx + dy * dy;
                if distance_squared >= contact * contact {
                    return;
                }
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

    pub fn step_combat(&mut self) {
        let tick = self.current_tick;
        let units = &self.units;
        let grid = &self.spatial_grid;
        let shots = &mut self.pending_shots;
        shots.clear();
        let mut query = self.ecs.query_mut::<(&UnitStats, &mut Combat)>();
        let mut view = query.view();
        for (index, &(id, entity)) in units.iter().enumerate() {
            let (stats, combat) = view.get_mut(entity).expect("unit list only holds live units");
            combat.ticks_until_ready = combat.ticks_until_ready.saturating_sub(1);
            let me = grid.entry(index);
            let range = i64::from(combat.range.to_bits());
            let squared_distance = |x: i32, y: i32| {
                let (dx, dy) = (i64::from(x - me.x), i64::from(y - me.y));
                dx * dx + dy * dy
            };

            // Drop a target that has died or moved out of range.
            if let Some(target) = combat.target {
                let still_valid = units
                    .binary_search_by_key(&target, |unit| unit.0)
                    .is_ok_and(|target_index| {
                        let entry = grid.entry(target_index);
                        squared_distance(entry.x, entry.y) <= range * range
                    });
                if !still_valid {
                    combat.target = None;
                }
            }

            // Searches are spread across ticks. Rings grow outward, so units at the
            // front of a battle stop early with the nearest enemy.
            if combat.target.is_none() && id.0.wrapping_add(tick) % TARGET_SEARCH_INTERVAL == 0 {
                let mut ring = 8i64 << FP_FRAC_BITS;
                loop {
                    let radius = ring.min(range);
                    let mut best: Option<(i64, SimId)> = None;
                    grid.for_each_candidate(Some(stats.team.enemy()), me.x, me.y, radius as i32, |other| {
                        let distance = squared_distance(other.x, other.y);
                        if distance <= radius * radius {
                            let candidate = (distance, other.id);
                            if best.is_none_or(|current| candidate < current) {
                                best = Some(candidate);
                            }
                        }
                    });
                    if let Some((_, found)) = best {
                        combat.target = Some(found);
                        break;
                    }
                    if radius >= range {
                        break;
                    }
                    ring *= 2;
                }
            }

            if combat.ticks_until_ready == 0 {
                if let Some(target) = combat.target {
                    let target_index = units
                        .binary_search_by_key(&target, |unit| unit.0)
                        .expect("target was just validated or found");
                    let entry = grid.entry(target_index);
                    let raw = |x: i32, y: i32, z: i32| Vec3Fp::new(Fp::from_bits(x), Fp::from_bits(y), Fp::from_bits(z));
                    shots.push(PendingShot {
                        source: id,
                        team: stats.team,
                        from: raw(me.x, me.y, me.z),
                        to: raw(entry.x, entry.y, entry.z),
                        speed: combat.projectile_speed,
                        damage: combat.projectile_damage,
                    });
                    combat.ticks_until_ready = combat.reload_ticks;
                }
            }
        }

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
                },
            );
        }
    }

    pub fn step_projectiles(&mut self) {
        let terrain = &self.terrain;
        let grid = &self.spatial_grid;
        let hits = &mut self.pending_hits;
        let dead = &mut self.dead_projectiles;
        let friendly_fire = self.config.friendly_fire;
        let max_radius = self.max_unit_radius;
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

            let probe = probe_terrain(terrain, start, step);

            // Shells far above the ground cannot touch a unit, which spares most grid queries.
            let lowest = start.z.min(end.z);
            let reach = Fp::from_bits(max_radius * 2);
            let mut best: Option<(i32, SimId, hecs::Entity)> = None;
            if lowest <= probe.highest_ground + reach {
                let half = (step.x.to_bits().abs() + step.y.to_bits().abs()) / 2;
                let center_x = start.x.to_bits() + step.x.to_bits() / 2;
                let center_y = start.y.to_bits() + step.y.to_bits() / 2;
                let team_filter = if friendly_fire { None } else { Some(projectile.source_team.enemy()) };
                let source = projectile.source;
                grid.for_each_candidate(team_filter, center_x, center_y, half + max_radius, |other| {
                    if other.id == source {
                        return;
                    }
                    if let Some(along) = entry_hit(other, start, step) {
                        let candidate = (along, other.id, other.entity);
                        if best.is_none_or(|current| (candidate.0, candidate.1) < (current.0, current.1)) {
                            best = Some(candidate);
                        }
                    }
                });
            }

            match (best, probe.hit) {
                (Some((along, _, target)), terrain_hit) if terrain_hit.is_none_or(|ground| along <= ground) => {
                    hits.push((target, projectile.damage));
                    dead.push(id);
                }
                (_, Some(_)) => dead.push(id),
                _ => {
                    if projectile.lifetime_ticks == 0 || !terrain.in_bounds(end.xy()) {
                        dead.push(id);
                    }
                }
            }
        }

        self.dead_units.clear();
        if !self.config.invulnerable_units {
            for &(target, damage) in &self.pending_hits {
                let (id, health) = self
                    .ecs
                    .query_one_mut::<(&SimId, &mut Health)>(target)
                    .expect("hit entities are live until cleanup");
                let was_alive = health.current > Fp::ZERO;
                health.current = health.current.saturating_sub(damage);
                if was_alive && health.current <= Fp::ZERO {
                    self.dead_units.push(*id);
                }
            }
        }
    }

    pub fn step_cleanup(&mut self) {
        if !self.dead_units.is_empty() {
            self.dead_units.sort_unstable();
            for index in 0..self.dead_units.len() {
                let id = self.dead_units[index];
                let (_, entity) = self.find_unit(id).expect("dead units are still listed");
                let order = *self.ecs.get::<&Order>(entity).expect("units have orders");
                if order.kind != OrderKind::Idle {
                    self.flow_field.release(order.route);
                }
                self.ecs.despawn(entity).expect("dead units are live until now");
            }
            let dead = &self.dead_units;
            self.units.retain(|unit| dead.binary_search(&unit.0).is_err());
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

    /// Computes a 64-bit checksum of the entire simulation state. Every value that can
    /// affect a future tick is hashed. `Hasher::write_i32` and friends use native
    /// endianness, so all values go through `to_le_bytes`.
    pub fn compute_checksum(&self) -> u64 {
        let mut hasher = XxHash64::with_seed(0);
        hasher.write(&self.current_tick.to_le_bytes());
        hasher.write(&self.next_sim_id.to_le_bytes());
        hasher.write(&self.rng.state().to_le_bytes());
        hasher.write(&self.terrain.content_hash().to_le_bytes());
        hasher.write(&[u8::from(self.config.friendly_fire), u8::from(self.config.invulnerable_units)]);
        hasher.write(&self.max_unit_radius.to_le_bytes());

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
        hasher.write(&[stats.team as u8, stats.move_class as u8]);
        write_fp(hasher, health.current);
        write_fp(hasher, health.max);
        write_fp(hasher, combat.range);
        hasher.write(&combat.reload_ticks.to_le_bytes());
        hasher.write(&combat.ticks_until_ready.to_le_bytes());
        write_fp(hasher, combat.projectile_speed);
        write_fp(hasher, combat.projectile_damage);
        hasher.write(&combat.target.map_or(u32::MAX, |target| target.0).to_le_bytes());
        hasher.write(&[order.kind as u8]);
        hasher.write(&order.route.to_le_bytes());
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
    }

    /// Copies the state the renderer needs. Rendering never touches `SimWorld` directly.
    pub fn snapshot(&self) -> SimSnapshot {
        let mut unit_query = self.ecs.query::<(&Position, &UnitStats)>();
        let unit_view = unit_query.view();
        let units = self
            .units
            .iter()
            .map(|&(id, entity)| {
                let (position, stats) = unit_view.get(entity).expect("unit list only holds live units");
                UnitView {
                    id,
                    team: stats.team,
                    x: position.0.x.to_bits(),
                    y: position.0.y.to_bits(),
                    z: position.0.z.to_bits(),
                    radius: stats.radius.to_bits(),
                }
            })
            .collect();
        let mut projectile_query = self.ecs.query::<(&Position, &Projectile)>();
        let projectile_view = projectile_query.view();
        let projectiles = self
            .projectiles
            .iter()
            .map(|&(id, entity)| {
                let (position, projectile) = projectile_view.get(entity).expect("projectile list only holds live projectiles");
                ProjectileView {
                    id,
                    team: projectile.source_team,
                    x: position.0.x.to_bits(),
                    y: position.0.y.to_bits(),
                    z: position.0.z.to_bits(),
                }
            })
            .collect();
        SimSnapshot { tick: self.current_tick, units, projectiles }
    }
}

fn write_fp(hasher: &mut impl Hasher, value: Fp) {
    hasher.write(&value.to_bits().to_le_bytes());
}

fn write_vec3(hasher: &mut impl Hasher, value: Vec3Fp) {
    write_fp(hasher, value.x);
    write_fp(hasher, value.y);
    write_fp(hasher, value.z);
}
