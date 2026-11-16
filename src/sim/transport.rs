//! Air transports (GAME_SPECIFICATION.md §10): land units board one as it comes to them, ride
//! inside it off the map, and are set down together where it is sent. A unit inside is out of
//! the world's unit list, so nothing can see, target, or order it, and it neither moves nor
//! fires. It rides at its transport's position, and goes down with it.

use super::blueprints::{Blueprint, TransportDef, UnitClass};
use super::components::{Boarding, Builder, Carried, LeavingFactory, MoveClass, Order, OrderKind, OrderQueue, Patrol, Position, SimId, Transport, UnitStats, Velocity};
use super::config::TERRAIN_CELL_UNITS;
use super::world::SimWorld;
use crate::math::fp::{fp_int, wide_mul, Fp, Vec2Fp, Vec3Fp};

/// How far, besides the two units' radii, a unit may be from its transport to board it.
const BOARDING_REACH_UNITS: i32 = 40;

/// How near its unloading spot a transport must come to set its cargo down.
const UNLOAD_REACH_UNITS: i32 = 80;

/// How many cells out from the unloading spot cargo is set down, at most.
const UNLOAD_SPREAD_CELLS: i32 = 12;

/// Which of a transport's sizes a unit takes: T1 land units are small, T2 medium, and T3 and
/// field rigs large. Aircraft, ships, structures, and commanders cannot be carried.
pub fn transport_size(unit: &Blueprint) -> Option<usize> {
    let carriable = !unit.is_structure() && !unit.is_air() && unit.class != UnitClass::Commander && unit.class != UnitClass::Ammo && unit.speed > Fp::ZERO;
    let afloat = matches!(unit.movement, MoveClass::Naval | MoveClass::Submarine);
    if !carriable || afloat {
        return None;
    }
    Some(if unit.class == UnitClass::FieldRig { 2 } else { usize::from(unit.tier.clamp(1, 3)) - 1 })
}

/// Whether a load of `counts` units of each size, and one more of `size`, fits: each small unit
/// takes a `slots[0]`th of the room, each medium a `slots[1]`th, and each large a `slots[2]`th.
pub fn fits(def: TransportDef, counts: [u32; 3], size: usize) -> bool {
    let mut counts = counts.map(u64::from);
    counts[size] += 1;
    let [small, medium, large] = def.slots.map(u64::from);
    counts[0] * medium * large + counts[1] * small * large + counts[2] * small * medium <= small * medium * large
}

impl SimWorld {
    /// The world's lists a carried unit leaves, and goes back into when set down.
    fn carried_unit_lists(&mut self) -> [&mut Vec<(SimId, hecs::Entity)>; 18] {
        [
            &mut self.builders,
            &mut self.patrols,
            &mut self.order_queues,
            &mut self.veterans,
            &mut self.shields,
            &mut self.missile_defenses,
            &mut self.upgrading,
            &mut self.intel,
            &mut self.enhanced,
            &mut self.enhancing,
            &mut self.teleporting,
            &mut self.self_destructing,
            &mut self.being_captured,
            &mut self.submarines,
            &mut self.torpedo_defenses,
            &mut self.seabed_walkers,
            &mut self.disabled,
            &mut self.tractors,
        ]
    }

    /// How many of each size a transport is carrying.
    fn cargo_counts(&self, transport: hecs::Entity) -> [u32; 3] {
        let mut counts = [0; 3];
        if let Ok(cargo) = self.ecs.get::<&Transport>(transport) {
            for &id in &cargo.cargo {
                let Ok(index) = self.carried.binary_search_by_key(&id, |unit| unit.0) else {
                    continue;
                };
                let entity = self.carried[index].1;
                if let Some(size) = self.ecs.get::<&super::components::Blueprinted>(entity).ok().and_then(|unit| transport_size(self.blueprints.get(unit.0))) {
                    counts[size] += 1;
                }
            }
        }
        counts
    }

    fn transport_entity(&self, id: SimId) -> Option<hecs::Entity> {
        let index = self.transports.binary_search_by_key(&id, |unit| unit.0).ok()?;
        Some(self.transports[index].1)
    }

    /// Land units set off for a transport, which comes to where they are. Each gets in once it
    /// reaches the transport, while there is room.
    pub(super) fn command_load(&mut self, units: &[SimId], transport: SimId) {
        let Some(transport_entity) = self.transport_entity(transport) else {
            return;
        };
        if self.ecs.get::<&super::components::Construction>(transport_entity).is_ok() {
            return;
        }
        let team = self.ecs.get::<&UnitStats>(transport_entity).expect("units have stats").team;
        let busy = |world: &Self, id: SimId| {
            [&world.upgrading, &world.enhancing, &world.teleporting, &world.self_destructing, &world.being_captured, &world.disabled]
                .iter()
                .any(|list| list.binary_search_by_key(&id, |unit| unit.0).is_ok())
        };
        let riders: Vec<SimId> = units
            .iter()
            .copied()
            .filter(|&id| {
                self.find_unit(id).is_some_and(|(_, entity)| {
                    self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team == team)
                        && self.ecs.get::<&super::components::Blueprinted>(entity).is_ok_and(|unit| transport_size(self.blueprints.get(unit.0)).is_some())
                        && self.ecs.get::<&super::components::Construction>(entity).is_err()
                })
            })
            .filter(|&id| !busy(self, id))
            .collect();
        if riders.is_empty() {
            return;
        }
        let Some(transport_at) = self.unit_position(transport) else {
            return;
        };
        self.clear_unit_plans(&riders);
        self.assign_route(&riders, OrderKind::Move, transport_at.xy(), Some(transport));
        // The transport comes to the middle of them, and settles there.
        let mut sum = (0_i64, 0_i64);
        for &id in &riders {
            let (_, entity) = self.find_unit(id).expect("just checked");
            let at = self.ecs.get::<&Position>(entity).expect("units have positions").0;
            sum.0 += i64::from(at.x.to_bits());
            sum.1 += i64::from(at.y.to_bits());
            let route = self.ecs.get::<&Order>(entity).expect("units have orders").route;
            let _ = self.ecs.insert_one(entity, Boarding { transport, route });
            if let Err(at) = self.boarding.binary_search_by_key(&id, |unit| unit.0) {
                self.boarding.insert(at, (id, entity));
            }
        }
        let count = riders.len() as i64;
        let middle = Vec2Fp::new(Fp::from_bits((sum.0 / count) as i32), Fp::from_bits((sum.1 / count) as i32));
        self.assign_route(&[transport], OrderKind::Move, middle, None);
    }

    /// Transports fly to a spot and set down everything they carry around it.
    pub(super) fn command_unload(&mut self, transports: &[SimId], point: Vec2Fp) {
        for &id in transports {
            let Some(entity) = self.transport_entity(id) else {
                continue;
            };
            if self.ecs.get::<&Transport>(entity).is_ok_and(|transport| transport.cargo.is_empty()) {
                continue;
            }
            self.assign_route(&[id], OrderKind::Move, point, None);
            let route = self.ecs.get::<&Order>(entity).expect("units have orders").route;
            self.ecs.get::<&mut Transport>(entity).expect("transport list only holds transports").unload = Some((point, route));
        }
    }

    /// Boards units that have reached their transports, sets cargo down where transports have
    /// arrived, and carries what is on board along with each transport.
    pub(super) fn step_transports(&mut self) {
        if self.transports.is_empty() {
            return;
        }
        let reach = |a: Fp, b: Fp, extra: i32| a + b + fp_int(extra);

        // Boarding.
        for index in (0..self.boarding.len()).rev() {
            let (id, entity) = self.boarding[index];
            let boarding = *self.ecs.get::<&Boarding>(entity).expect("boarding list only holds boarders");
            let order = *self.ecs.get::<&Order>(entity).expect("units have orders");
            let here = self.ecs.get::<&Position>(entity).expect("units have positions").0.xy();
            let radius = self.ecs.get::<&UnitStats>(entity).expect("units have stats").radius;
            let transport = self.transport_entity(boarding.transport);
            let near = transport.is_some_and(|transport| {
                let there = self.ecs.get::<&Position>(transport).expect("units have positions").0.xy();
                let transport_radius = self.ecs.get::<&UnitStats>(transport).expect("units have stats").radius;
                let within = reach(radius, transport_radius, BOARDING_REACH_UNITS);
                here.distance_squared(&there) <= wide_mul(within, within)
            });
            let size = self.ecs.get::<&super::components::Blueprinted>(entity).ok().and_then(|unit| transport_size(self.blueprints.get(unit.0)));
            let room = match (transport, size) {
                (Some(transport), Some(size)) => {
                    let def = self.ecs.get::<&super::components::Blueprinted>(transport).ok().and_then(|unit| self.blueprints.get(unit.0).transport);
                    def.is_some_and(|def| fits(def, self.cargo_counts(transport), size))
                }
                _ => false,
            };
            let still_going = transport.is_some() && order.kind != OrderKind::Idle && order.route == boarding.route;
            if near && room {
                self.boarding.remove(index);
                let _ = self.ecs.remove_one::<Boarding>(entity);
                self.stow(id, entity, boarding.transport);
            } else if !still_going || (near && !room) {
                // Called off, out of room, or the transport is gone.
                self.boarding.remove(index);
                let _ = self.ecs.remove_one::<Boarding>(entity);
                if near && !room {
                    self.set_order(id, Order::IDLE);
                }
            }
        }

        // Unloading, and carrying.
        for index in 0..self.transports.len() {
            let (_, entity) = self.transports[index];
            let Some((point, route)) = self.ecs.get::<&Transport>(entity).expect("transport list only holds transports").unload else {
                self.carry_along(entity);
                continue;
            };
            let order = *self.ecs.get::<&Order>(entity).expect("units have orders");
            if order.kind != OrderKind::Idle && order.route != route {
                // Sent somewhere else instead.
                self.ecs.get::<&mut Transport>(entity).expect("transport list only holds transports").unload = None;
                self.carry_along(entity);
                continue;
            }
            let here = self.ecs.get::<&Position>(entity).expect("units have positions").0.xy();
            let within = fp_int(UNLOAD_REACH_UNITS);
            if here.distance_squared(&point) > wide_mul(within, within) {
                self.carry_along(entity);
                continue;
            }
            let cargo = {
                let mut transport = self.ecs.get::<&mut Transport>(entity).expect("transport list only holds transports");
                transport.unload = None;
                std::mem::take(&mut transport.cargo)
            };
            // Red searches the other way round, so mirror-image drops stay mirror images.
            let mirror = self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team == super::components::Team::Red);
            let mut taken: Vec<(usize, usize)> = Vec::new();
            for carried in cargo {
                self.unstow(carried, point, mirror, &mut taken);
            }
        }
    }

    /// Keeps what a transport carries at its position, so that it is set down, or left as
    /// wreckage, where the transport is.
    fn carry_along(&mut self, transport: hecs::Entity) {
        let Ok(cargo) = self.ecs.get::<&Transport>(transport).map(|transport| transport.cargo.clone()) else {
            return;
        };
        if cargo.is_empty() {
            return;
        }
        let at = self.ecs.get::<&Position>(transport).expect("units have positions").0;
        for id in cargo {
            if let Ok(index) = self.carried.binary_search_by_key(&id, |unit| unit.0) {
                let entity = self.carried[index].1;
                self.ecs.get::<&mut Position>(entity).expect("units have positions").0 = at;
            }
        }
    }

    /// Takes a unit off the map and into a transport.
    fn stow(&mut self, id: SimId, entity: hecs::Entity, transport: SimId) {
        self.set_order(id, Order::IDLE);
        self.release_unit_plans(entity);
        if let Ok(mut builder) = self.ecs.get::<&mut Builder>(entity) {
            builder.tasks.clear();
        }
        if let Ok(mut queue) = self.ecs.get::<&mut OrderQueue>(entity) {
            queue.0.clear();
        }
        let _ = self.ecs.remove_one::<Patrol>(entity);
        let _ = self.ecs.remove_one::<LeavingFactory>(entity);
        self.ecs.get::<&mut Velocity>(entity).expect("units have velocities").0 = Vec3Fp::ZERO;
        let mut lists = 0_u32;
        for (bit, list) in self.carried_unit_lists().into_iter().enumerate() {
            if let Ok(at) = list.binary_search_by_key(&id, |unit| unit.0) {
                list.remove(at);
                lists |= 1 << bit;
            }
        }
        if let Ok(at) = self.units.binary_search_by_key(&id, |unit| unit.0) {
            self.units.remove(at);
        }
        self.ecs.insert_one(entity, Carried { transport, lists }).expect("unit is live");
        let at = self.carried.binary_search_by_key(&id, |unit| unit.0).unwrap_err();
        self.carried.insert(at, (id, entity));
        if let Some(transport) = self.transport_entity(transport) {
            self.ecs.get::<&mut Transport>(transport).expect("transport list only holds transports").cargo.push(id);
        }
    }

    /// Puts a carried unit back on the map, on the nearest cell to `point` that it can stand on
    /// and that nothing set down with it has taken. Cells equally near are tried in a fixed order,
    /// turned about the center for `mirror`, Red's, so that both sides break the ties alike.
    fn unstow(&mut self, id: SimId, point: Vec2Fp, mirror: bool, taken: &mut Vec<(usize, usize)>) {
        let Ok(index) = self.carried.binary_search_by_key(&id, |unit| unit.0) else {
            return;
        };
        let entity = self.carried[index].1;
        let class = self.ecs.get::<&UnitStats>(entity).expect("units have stats").move_class;
        let radius = self.ecs.get::<&UnitStats>(entity).expect("units have stats").radius;
        let (center_x, center_y) = self.terrain.cell_of(point);
        let cells = self.terrain.cells_per_side() as i32;
        let cell_size = fp_int(TERRAIN_CELL_UNITS);
        let mut spot = None;
        'rings: for ring in 0..=UNLOAD_SPREAD_CELLS {
            for dy in -ring..=ring {
                for dx in -ring..=ring {
                    if dx.abs() != ring && dy.abs() != ring {
                        continue;
                    }
                    let turn = if mirror { -1 } else { 1 };
                    let (x, y) = (center_x as i32 + dx * turn, center_y as i32 + dy * turn);
                    if !(0..cells).contains(&x) || !(0..cells).contains(&y) || taken.contains(&(x as usize, y as usize)) {
                        continue;
                    }
                    let at = Vec2Fp::new(cell_size * x + cell_size / 2, cell_size * y + cell_size / 2);
                    if self.terrain.passable_for(class, at) {
                        spot = Some((x as usize, y as usize, at));
                        break 'rings;
                    }
                }
            }
        }
        let Some((x, y, at)) = spot else {
            // Nowhere to stand within reach: it stays on board.
            if let Some(transport) = self.ecs.get::<&Carried>(entity).ok().map(|carried| carried.transport).and_then(|transport| self.transport_entity(transport)) {
                self.ecs.get::<&mut Transport>(transport).expect("transport list only holds transports").cargo.push(id);
            }
            return;
        };
        taken.push((x, y));
        let ground = match class {
            MoveClass::Hover => self.terrain.height_at(at).max(self.terrain.water_level()),
            _ => self.terrain.height_at(at),
        };
        self.ecs.get::<&mut Position>(entity).expect("units have positions").0 = Vec3Fp::new(at.x, at.y, ground + radius);
        let carried = self.ecs.remove_one::<Carried>(entity).expect("carried list only holds carried units");
        self.carried.remove(index);
        let at = self.units.binary_search_by_key(&id, |unit| unit.0).unwrap_err();
        self.units.insert(at, (id, entity));
        for (bit, list) in self.carried_unit_lists().into_iter().enumerate() {
            if carried.lists & (1 << bit) != 0 {
                if let Err(at) = list.binary_search_by_key(&id, |unit| unit.0) {
                    list.insert(at, (id, entity));
                }
            }
        }
    }

    /// Destroys a unit inside a transport that has gone down, leaving its wreckage there.
    pub(super) fn destroy_carried(&mut self, id: SimId) {
        let Ok(index) = self.carried.binary_search_by_key(&id, |unit| unit.0) else {
            return;
        };
        let (_, entity) = self.carried.remove(index);
        self.remove_carried_unit(entity);
    }
}
