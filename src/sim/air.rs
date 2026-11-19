//! Aircraft (GAME_SPECIFICATION.md §10). An aircraft flies at its own height over land and
//! water, straight at where it is going, with no pathfinding and nothing to bump into. In flight
//! it never stops, so it turns in arcs and makes passes over its target. With nothing to do it
//! lands. It burns fuel as it flies, and refuels and is repaired on an airfield.

use super::components::{rotate_toward, Combat, Flight, Order, OrderKind, Position, SimId, Turning, UnitStats, Velocity};
use super::config::GRAVITY_RAW;
use super::spatial::GridEntry;
use super::terrain::Terrain;
use super::world::SimWorld;
use crate::math::fp::{fp_int, wide_mul, Fp, Vec2Fp, Vec3Fp};

/// Bombs fall at FAF's gravity, 4.9 map units a second squared, which is ten times
/// what shells use here.
pub const BOMB_GRAVITY: Fp = Fp::from_bits(GRAVITY_RAW * 10);

/// Ticks of straight flight a bomber leaves itself, after turning round, to settle on the line
/// to its target before it must let go.
const BOMB_RUN_UP_TICKS: i32 = 10;
/// This close to the end of a move, in ticks of flight, an aircraft has arrived.
const ARRIVE_TICKS: i32 = 3;
/// Ticks to slow to a stop and come down, or to take off again.
const LANDING_TICKS: i32 = 30;

/// The height of an aircraft flying over a point.
pub fn flying_height(terrain: &Terrain, at: Vec2Fp, elevation: Fp) -> Fp {
    terrain.height_at(at).max(terrain.water_level()) + elevation
}

/// How many ticks a bomb takes to fall this far.
pub fn bomb_fall_ticks(fall: Fp) -> i64 {
    // After n ticks a bomb has fallen g * n * (n - 1) / 2.
    let fall = i64::from(fall.to_bits().max(0));
    ((2 * fall / i64::from(BOMB_GRAVITY.to_bits())) as u64).isqrt() as i64 + 1
}

/// Where a target moving at `velocity` a tick will be once a bomb has fallen `fall` onto it.
pub fn bomb_lead(target: Vec2Fp, velocity: [i32; 2], fall: Fp) -> Vec2Fp {
    let ticks = bomb_fall_ticks(fall);
    let ahead = |at: Fp, step: i32| Fp::from_bits((i64::from(at.to_bits()) + i64::from(step) * ticks).clamp(0, i64::from(i32::MAX)) as i32);
    Vec2Fp::new(ahead(target.x, velocity[0]), ahead(target.y, velocity[1]))
}

/// How far ahead of the aircraft a bomb dropped from `fall` up lands.
pub fn bomb_throw(fall: Fp, speed: Fp) -> Fp {
    Fp::from_bits((i64::from(speed.to_bits()) * bomb_fall_ticks(fall)).min(i64::from(i32::MAX)) as i32)
}

/// Whether a bomb dropped now would land on a target moving at `velocity` a tick, given where the
/// aircraft is and how it is flying. The bomb keeps the aircraft's speed as it falls, so it is let
/// go short of the target; it lands where the target will be by then, so a moving one is led.
pub fn bomb_lands_on(from: Vec3Fp, facing: Vec2Fp, speed: Fp, target: Vec3Fp, velocity: [i32; 2], reach: Fp) -> bool {
    let ticks = bomb_fall_ticks(from.z - target.z);
    let drop = i64::from(speed.to_bits()) * ticks;
    let ahead = bomb_lead(target.xy(), velocity, from.z - target.z);
    let offset = ahead - from.xy();
    let along = (i64::from(facing.x.to_bits()) * i64::from(offset.x.to_bits()) + i64::from(facing.y.to_bits()) * i64::from(offset.y.to_bits())) >> 12;
    let across = (i64::from(facing.x.to_bits()) * i64::from(offset.y.to_bits()) - i64::from(facing.y.to_bits()) * i64::from(offset.x.to_bits())) >> 12;
    let short = drop - along;
    let reach = i64::from(reach.to_bits());
    along > 0 && short * short + across * across <= reach * reach
}

/// The tightest circle an aircraft can fly, in units.
fn turn_radius(speed: Fp, max_turn: Fp) -> Fp {
    if max_turn <= Fp::ZERO {
        return Fp::MAX;
    }
    speed.saturating_div(max_turn)
}

/// Whether a point lies inside one of the two circles the aircraft would fly turning hard left
/// or right. It can never reach such a point by turning towards it, only by flying on first.
fn inside_turn(facing: Vec2Fp, offset: Vec2Fp, radius: Fp) -> bool {
    let along = facing.x * offset.x + facing.y * offset.y;
    let across = (facing.x * offset.y - facing.y * offset.x).abs();
    let side = across - radius;
    wide_mul(along, along) + wide_mul(side, side) < wide_mul(radius, radius)
}

/// Below this share of a full tank, a patrolling aircraft goes to refuel.
const REFUEL_SHARE_PERCENT: u32 = 20;
/// Ticks of fuel gained each tick on the ground, FAF's recharge rate of 5, and on an
/// airfield's pad, 50 times that.
const LANDED_REFUEL_PER_TICK: u32 = 5;
const DOCKED_REFUEL_PER_TICK: u32 = LANDED_REFUEL_PER_TICK * 50;
/// How far from its middle an airfield's four pads are, in units.
const PAD_OFFSET_UNITS: i32 = 20;

/// An airfield, as the aircraft see it this tick.
struct Staging {
    id: SimId,
    team: super::components::Team,
    center: Vec2Fp,
    slots: u8,
    used: u8,
    repair: Fp,
    energy: super::blueprints::Res,
}

impl Staging {
    fn pad(&self, pad: u8) -> Vec2Fp {
        let offset = fp_int(PAD_OFFSET_UNITS);
        let (x, y) = [(-1, -1), (1, -1), (-1, 1), (1, 1)][usize::from(pad % 4)];
        Vec2Fp::new(self.center.x + offset * x, self.center.y + offset * y)
    }
}

impl SimWorld {
    /// Sends aircraft to land on an airfield of their team, and stay until they are
    /// refuelled and repaired. It ends whatever else they were doing, patrols included.
    pub(super) fn command_refit(&mut self, units: &[SimId], staging: SimId) {
        let Some((_, staging_entity)) = self.find_unit(staging) else {
            return;
        };
        if self.airfields.binary_search_by_key(&staging, |unit| unit.0).is_err() || self.ecs.get::<&super::components::Construction>(staging_entity).is_ok() {
            return;
        }
        let team = self.ecs.get::<&UnitStats>(staging_entity).expect("units have stats").team;
        let aircraft: Vec<SimId> = units
            .iter()
            .copied()
            .filter(|id| self.find_unit(*id).is_some_and(|(_, entity)| self.ecs.get::<&Flight>(entity).is_ok() && self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team == team)))
            .collect();
        self.clear_unit_plans(&aircraft);
        for id in aircraft {
            self.set_order(id, Order::IDLE);
            let (_, entity) = self.find_unit(id).expect("just checked");
            self.ecs.get::<&mut Flight>(entity).expect("just checked").refit = Some(staging);
        }
    }

    /// The finished airfields, with how many of their slots are taken.
    fn stagings(&self) -> Vec<Staging> {
        let mut stagings: Vec<Staging> = self
            .airfields
            .iter()
            .filter(|&&(_, entity)| self.ecs.get::<&super::components::Construction>(entity).is_err())
            .filter_map(|&(id, entity)| {
                let def = self.blueprints.get(self.ecs.get::<&super::components::Blueprinted>(entity).ok()?.0).airfield?;
                let team = self.ecs.get::<&UnitStats>(entity).ok()?.team;
                let center = self.ecs.get::<&Position>(entity).ok()?.0.xy();
                Some(Staging { id, team, center, slots: def.slots, used: 0, repair: def.repair_per_tick, energy: def.energy_per_tick })
            })
            .collect();
        for &(_, entity) in &self.aircraft {
            let flight = *self.ecs.get::<&Flight>(entity).expect("aircraft fly");
            if let (true, Some(staging)) = (flight.docked, flight.refit) {
                if let Some(spot) = stagings.iter_mut().find(|spot| spot.id == staging) {
                    spot.used += flight.size;
                }
            }
        }
        stagings
    }

    /// Flies every aircraft for a tick. Positions of other units come from the grid, so the
    /// order aircraft are updated in cannot matter.
    pub(super) fn step_aircraft(&mut self) {
        if self.aircraft.is_empty() {
            return;
        }
        let mut stagings = if self.airfields.is_empty() { Vec::new() } else { self.stagings() };
        let units = &self.units;
        let grid = &self.spatial_grid;
        let terrain = &self.terrain;
        let flow = &mut self.flow_field;
        let economies = &mut self.economies;
        let occupancy = &self.occupancy;
        let max = terrain.max_coordinate_raw();
        let center = Vec2Fp::new(Fp::from_bits(max / 2), Fp::from_bits(max / 2));
        let locate = |target: SimId| -> Option<GridEntry> { units.binary_search_by_key(&target, |unit| unit.0).ok().map(|index| *grid.entry(index)) };
        let mut query = self.ecs.query_mut::<(
            &mut Position,
            &mut Velocity,
            &UnitStats,
            &mut Order,
            &Combat,
            &mut Turning,
            &mut Flight,
            &mut super::components::Health,
            Option<&super::components::Patrol>,
            Option<&super::components::BombBay>,
        )>();
        let mut view = query.view();
        for &(_, entity) in &self.aircraft {
            let (position, velocity, stats, order, combat, turning, flight, health, patrol, bomb_bay) = view.get_mut(entity).expect("aircraft are live units");
            let here = position.0.xy();
            let radius = turn_radius(stats.max_speed, turning.max_turn);
            let patrolling = patrol.is_some();

            // On patrol and low on fuel, it goes to the nearest airfield of its team.
            if patrolling && flight.refit.is_none() && flight.fuel * 100 < flight.max_fuel * REFUEL_SHARE_PERCENT {
                let nearest = stagings.iter().filter(|spot| spot.team == stats.team).min_by_key(|spot| (here.distance_squared(&spot.center), spot.id));
                if let Some(spot) = nearest {
                    flight.refit = Some(spot.id);
                    if order.kind != OrderKind::Idle {
                        flow.release(order.route);
                        *order = Order::IDLE;
                    }
                }
            }
            // A facility that is gone, or no longer its team's, leaves it where it is.
            let staging = flight.refit.and_then(|id| stagings.iter().position(|spot| spot.id == id && spot.team == stats.team));
            if flight.refit.is_some() && staging.is_none() {
                (flight.refit, flight.docked) = (None, false);
            }

            // On the pad, it refuels and is repaired, and leaves once it is done.
            if let (Some(index), true) = (staging, flight.docked) {
                let spot = &stagings[index];
                flight.fuel = (flight.fuel + DOCKED_REFUEL_PER_TICK).min(flight.max_fuel);
                let economy = &mut economies[stats.team.index()];
                if health.current < health.max && economy.energy >= spot.energy {
                    economy.energy -= spot.energy;
                    health.current = (health.current + spot.repair).min(health.max);
                }
                let pad = spot.pad(flight.pad);
                position.0 = Vec3Fp::new(pad.x, pad.y, terrain.height_at(pad) + stats.radius);
                velocity.0 = Vec3Fp::ZERO;
                (flight.speed, flight.altitude) = (Fp::ZERO, Fp::ZERO);
                if flight.fuel == flight.max_fuel && health.current == health.max {
                    (flight.refit, flight.docked) = (None, false);
                }
                continue;
            }

            // A target to make passes over, a facility to land on, the end of its move, or nowhere.
            let chasing = if order.kind == OrderKind::Move { None } else { combat.target.or(combat.forced_target) };
            // A bomber heads for where the target will be when its bomb lands.
            let target = chasing.and_then(locate).map(|entry| {
                let at = Vec2Fp::new(Fp::from_bits(entry.x), Fp::from_bits(entry.y));
                if bomb_bay.is_some() { bomb_lead(at, entry.velocity, flight.elevation) } else { at }
            });
            let ground_target = combat.ground_target.filter(|_| order.kind == OrderKind::Attack);
            let mut heading_for = if staging.is_some() { None } else { target.or(ground_target) };
            let attacking = heading_for.is_some();
            let close = Fp::from_bits(stats.max_speed.to_bits().saturating_mul(ARRIVE_TICKS));
            if let Some(index) = staging {
                let spot = &mut stagings[index];
                if here.distance_squared(&spot.center) > wide_mul(close, close) {
                    heading_for = Some(spot.center);
                } else if spot.used + flight.size <= spot.slots {
                    // Lands on the first pad free, and waits overhead while they are all taken.
                    flight.docked = true;
                    flight.pad = spot.used;
                    spot.used += flight.size;
                    let pad = spot.pad(flight.pad);
                    position.0 = Vec3Fp::new(pad.x, pad.y, terrain.height_at(pad) + stats.radius);
                    velocity.0 = Vec3Fp::ZERO;
                    (flight.speed, flight.altitude) = (Fp::ZERO, Fp::ZERO);
                    continue;
                }
            } else if heading_for.is_none() && order.kind != OrderKind::Idle {
                let goal = flow.goal(order.route).unwrap_or(here);
                // Following a unit never ends: it keeps making passes over it.
                let ends = order.kind != OrderKind::Attack && !flow.is_following(order.route);
                if ends && here.distance_squared(&goal) <= wide_mul(close, close) {
                    flow.release(order.route);
                    *order = Order::IDLE;
                } else {
                    heading_for = Some(goal);
                }
            }

            // A gunship stops over what it attacks, well within range, and turns to face it.
            let hold = flight.hovers
                && attacking
                && heading_for.is_some_and(|point| {
                    let reach = combat.range * 3 / 4;
                    here.distance_squared(&point) <= wide_mul(reach, reach)
                });
            // With nothing to do, it lands, if there is open ground below to land on. Patrols
            // and trips to refuel keep it in the air.
            let margin = radius.saturating_mul(fp_int(3));
            let near_edge = [here.x, here.y].iter().any(|&value| value < margin || Fp::from_bits(max) - value < margin);
            let idle = heading_for.is_none() && !patrolling && flight.refit.is_none();
            let landing = idle && !near_edge && terrain.is_passable(here);
            // Over a structure, such as the facility it has just left, it flies on to land beyond it.
            let (cell_x, cell_y) = terrain.cell_of(here);
            let over_structure = idle && occupancy.contains_key(&((cell_y * terrain.cells_per_side() + cell_x) as u32));
            // Out of fuel, it flies at half its height and speed.
            let (top_speed, height) = if flight.out_of_fuel() { (stats.max_speed / 2, flight.elevation / 2) } else { (stats.max_speed, flight.elevation) };
            let down = |value: Fp, full: Fp, floor: Fp| (value - full / LANDING_TICKS).max(floor);
            let up = |value: Fp, full: Fp, ceiling: Fp| if value > ceiling { down(value, full, ceiling) } else { (value + full / LANDING_TICKS).min(ceiling) };
            if landing {
                flight.speed = down(flight.speed, stats.max_speed, Fp::ZERO);
                flight.altitude = down(flight.altitude, flight.elevation, Fp::ZERO);
            } else if hold {
                flight.speed = down(flight.speed, stats.max_speed, Fp::ZERO);
                flight.altitude = up(flight.altitude, flight.elevation, height);
            } else {
                flight.speed = up(flight.speed, stats.max_speed, top_speed);
                flight.altitude = up(flight.altitude, flight.elevation, height);
            }
            // Flying burns fuel, and on the ground it slowly comes back.
            if flight.airborne() {
                flight.fuel = flight.fuel.saturating_sub(1);
            } else {
                flight.fuel = (flight.fuel + LANDED_REFUEL_PER_TICK).min(flight.max_fuel);
            }
            let speed = flight.speed;

            // A bomber lets go well short of its target, its bomb's throw ahead, so it flies on
            // far enough past to turn round and line up for that again: twice round its turn,
            // then the throw, then a run-up to settle on the line.
            let break_off = if bomb_bay.is_some() {
                let throw = bomb_throw(flight.elevation, stats.max_speed);
                flight.break_off.max(throw + radius.saturating_mul(fp_int(2)) + stats.max_speed.saturating_mul(fp_int(BOMB_RUN_UP_TICKS)))
            } else {
                flight.break_off
            };
            let facing = turning.facing;
            turning.facing = match heading_for {
                Some(point) => {
                    let offset = point - here;
                    let distance_squared = here.distance_squared(&point);
                    if !attacking || flight.hovers {
                        flight.breaking_off = false;
                    } else if flight.breaking_off {
                        flight.breaking_off = distance_squared < wide_mul(break_off, break_off);
                    } else {
                        // Once over the target and past it, fly on for a run-up to the next pass.
                        let behind = facing.x * offset.x + facing.y * offset.y <= Fp::ZERO;
                        let half = break_off / 2;
                        flight.breaking_off = behind && distance_squared < wide_mul(half, half);
                    }
                    // A gunship can turn on the spot, so nothing is out of its reach.
                    let unreachable = !flight.hovers && inside_turn(facing, offset, radius);
                    if flight.breaking_off || offset == Vec2Fp::ZERO || unreachable {
                        facing
                    } else {
                        rotate_toward(facing, offset.normalized(), turning.max_turn)
                    }
                }
                // Coming down, it flies straight on as it slows, as it does to clear a structure.
                None if landing || (over_structure && !near_edge) => {
                    flight.breaking_off = false;
                    facing
                }
                // Over water or cliffs, or waiting for a pad, it circles where it is, unless that
                // would take it off the map.
                None => {
                    flight.breaking_off = false;
                    let toward = if near_edge { (center - here).normalized() } else { Vec2Fp::new(-facing.y, facing.x) };
                    rotate_toward(facing, toward, turning.max_turn)
                }
            };

            // How far it has been turning a tick, over the last few, for aiming at it along its
            // turn: aircraft that follow or circle turn on some ticks and not others.
            let now = turning.facing;
            let turned = Vec2Fp::new(facing.x * now.x + facing.y * now.y, facing.x * now.y - facing.y * now.x);
            flight.turn = Vec2Fp::new((flight.turn.x * 3 + turned.x) / 4, (flight.turn.y * 3 + turned.y) / 4);
            let step = turning.facing.scaled(speed);
            let next = Vec2Fp::new(Fp::from_bits((here.x + step.x).to_bits().clamp(0, max)), Fp::from_bits((here.y + step.y).to_bits().clamp(0, max)));
            // Landed, it rests on the ground like any other unit.
            position.0 = Vec3Fp::new(next.x, next.y, flying_height(terrain, next, flight.altitude.max(stats.radius)));
            velocity.0 = Vec3Fp::new(step.x, step.y, Fp::ZERO);
        }
        self.separate_aircraft();
    }

    /// Aircraft in flight keep a little apart, so that two sent the same way do not fly as one:
    /// any two closer than their radii and a margin are pushed apart, a third of the overlap a
    /// tick, so that they spread without jittering. In order along x, so only near neighbors are
    /// compared. Divisions round toward zero, so mirror-image pairs move in mirror image.
    fn separate_aircraft(&mut self) {
        const MARGIN_RAW: i64 = 4 << 12;
        let mut flying: Vec<(i64, i64, i64, SimId, hecs::Entity)> = self
            .aircraft
            .iter()
            .filter_map(|&(id, entity)| {
                let flight = self.ecs.get::<&Flight>(entity).ok()?;
                if flight.altitude <= Fp::ZERO {
                    return None;
                }
                let position = self.ecs.get::<&Position>(entity).ok()?.0;
                let radius = self.ecs.get::<&UnitStats>(entity).ok()?.radius;
                Some((i64::from(position.x.to_bits()), i64::from(position.y.to_bits()), i64::from(radius.to_bits()), id, entity))
            })
            .collect();
        if flying.len() < 2 {
            return;
        }
        flying.sort_unstable_by_key(|&(x, _, _, id, _)| (x, id));
        let widest = flying.iter().map(|aircraft| aircraft.2).max().unwrap_or(0);
        let mut pushes = vec![(0_i64, 0_i64); flying.len()];
        for first in 0..flying.len() {
            let (x, y, radius, id, _) = flying[first];
            for second in first + 1..flying.len() {
                let (other_x, other_y, other_radius, other_id, _) = flying[second];
                if other_x - x > radius + widest + MARGIN_RAW {
                    break;
                }
                let contact = radius + other_radius + MARGIN_RAW;
                let (dx, dy) = (x - other_x, y - other_y);
                let distance_squared = dx * dx + dy * dy;
                if distance_squared >= contact * contact {
                    continue;
                }
                let distance = (distance_squared as u64).isqrt() as i64;
                let overlap = contact - distance;
                let (push_x, push_y) = if distance == 0 {
                    // Exactly together: apart along a direction from their ids.
                    let sign = if id < other_id { 1 } else { -1 };
                    (sign * overlap / 6, 0)
                } else {
                    (dx * overlap / distance / 6, dy * overlap / distance / 6)
                };
                pushes[first].0 += push_x;
                pushes[first].1 += push_y;
                pushes[second].0 -= push_x;
                pushes[second].1 -= push_y;
            }
        }
        let max = i64::from(self.terrain.max_coordinate_raw());
        for (index, &(_, _, _, _, entity)) in flying.iter().enumerate() {
            let (push_x, push_y) = pushes[index];
            if push_x == 0 && push_y == 0 {
                continue;
            }
            let mut position = self.ecs.get::<&mut Position>(entity).expect("aircraft are live units");
            let x = (i64::from(position.0.x.to_bits()) + push_x).clamp(0, max) as i32;
            let y = (i64::from(position.0.y.to_bits()) + push_y).clamp(0, max) as i32;
            position.0 = Vec3Fp::new(Fp::from_bits(x), Fp::from_bits(y), position.0.z);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bomb_is_let_go_short_of_its_target() {
        let east = Vec2Fp::new(Fp::ONE, Fp::ZERO);
        let target = Vec3Fp::new(fp_int(1_000), fp_int(0), fp_int(0));
        let at = |x: i32| Vec3Fp::new(fp_int(x), fp_int(0), fp_int(280));
        // From 280 up, a bomb falls for 24 ticks, so at 20 a tick it lands 480 on.
        let still = [0, 0];
        assert!(bomb_lands_on(at(520), east, fp_int(20), target, still, fp_int(30)));
        assert!(!bomb_lands_on(at(900), east, fp_int(20), target, still, fp_int(30)), "too late");
        assert!(!bomb_lands_on(at(200), east, fp_int(20), target, still, fp_int(30)), "too early");
        // A target driving away at 5 a tick is 120 further on when the bomb lands: let go later.
        let away = [fp_int(5).to_bits(), 0];
        assert!(bomb_lands_on(at(640), east, fp_int(20), target, away, fp_int(30)), "led");
        assert!(!bomb_lands_on(at(520), east, fp_int(20), target, away, fp_int(30)), "where it is now is too soon");
        let west = Vec2Fp::new(-Fp::ONE, Fp::ZERO);
        assert!(!bomb_lands_on(at(520), west, fp_int(20), target, still, fp_int(30)), "flying away");
    }

    #[test]
    fn points_inside_the_turning_circles_cannot_be_turned_to() {
        let north = Vec2Fp::new(Fp::ZERO, Fp::ONE);
        let radius = fp_int(100);
        assert!(inside_turn(north, Vec2Fp::new(fp_int(100), fp_int(0)), radius));
        assert!(inside_turn(north, Vec2Fp::new(fp_int(-50), fp_int(20)), radius));
        assert!(!inside_turn(north, Vec2Fp::new(fp_int(0), fp_int(300)), radius));
        assert!(!inside_turn(north, Vec2Fp::new(fp_int(250), fp_int(0)), radius));
    }
}
