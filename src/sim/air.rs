//! Aircraft (GAME_SPECIFICATION.md §10). An aircraft flies at its own height over land and
//! water, straight at where it is going, with no pathfinding and nothing to bump into. It never
//! stops, so it turns in arcs, makes passes over its target, and circles when it has nowhere to go.

use super::components::{rotate_toward, Combat, Flight, Order, OrderKind, Position, SimId, Turning, UnitStats, Velocity};
use super::config::GRAVITY_RAW;
use super::spatial::GridEntry;
use super::terrain::Terrain;
use super::world::SimWorld;
use crate::math::fp::{fp_int, wide_mul, Fp, Vec2Fp, Vec3Fp};

/// Bombs fall at the original's gravity, 4.9 map units a second squared, which is ten times
/// what shells use here.
pub const BOMB_GRAVITY: Fp = Fp::from_bits(GRAVITY_RAW * 10);

/// This close to the end of a move, in ticks of flight, an aircraft has arrived.
const ARRIVE_TICKS: i32 = 3;
/// Ticks to slow to a stop and come down, or to take off again.
const LANDING_TICKS: i32 = 30;

/// The height of an aircraft flying over a point.
pub fn flying_height(terrain: &Terrain, at: Vec2Fp, elevation: Fp) -> Fp {
    terrain.height_at(at).max(terrain.water_level()) + elevation
}

/// Whether a bomb dropped now would land on a target, given where the aircraft is and how it is
/// flying. The bomb keeps the aircraft's speed as it falls, so it is let go short of the target.
pub fn bomb_lands_on(from: Vec3Fp, facing: Vec2Fp, speed: Fp, target: Vec3Fp, reach: Fp) -> bool {
    let fall = i64::from((from.z - target.z).to_bits().max(0));
    // After n ticks a bomb has fallen g * n * (n - 1) / 2.
    let ticks = ((2 * fall / i64::from(BOMB_GRAVITY.to_bits())) as u64).isqrt() as i64 + 1;
    let drop = i64::from(speed.to_bits()) * ticks;
    let offset = (target - from).xy();
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

impl SimWorld {
    /// Flies every aircraft for a tick. Positions of other units come from the grid, so the
    /// order aircraft are updated in cannot matter.
    pub(super) fn step_aircraft(&mut self) {
        if self.aircraft.is_empty() {
            return;
        }
        let units = &self.units;
        let grid = &self.spatial_grid;
        let terrain = &self.terrain;
        let flow = &mut self.flow_field;
        let max = terrain.max_coordinate_raw();
        let center = Vec2Fp::new(Fp::from_bits(max / 2), Fp::from_bits(max / 2));
        let locate = |target: SimId| -> Option<GridEntry> { units.binary_search_by_key(&target, |unit| unit.0).ok().map(|index| *grid.entry(index)) };
        let mut query = self.ecs.query_mut::<(&mut Position, &mut Velocity, &UnitStats, &mut Order, &Combat, &mut Turning, &mut Flight)>();
        let mut view = query.view();
        for &(_, entity) in &self.aircraft {
            let (position, velocity, stats, order, combat, turning, flight) = view.get_mut(entity).expect("aircraft are live units");
            let here = position.0.xy();
            let radius = turn_radius(stats.max_speed, turning.max_turn);

            // A target to make passes over, the end of its move, or nowhere.
            let chasing = if order.kind == OrderKind::Move { None } else { combat.target.or(combat.forced_target) };
            let target = chasing.and_then(locate).map(|entry| Vec2Fp::new(Fp::from_bits(entry.x), Fp::from_bits(entry.y)));
            let ground_target = combat.ground_target.filter(|_| order.kind == OrderKind::Attack);
            let mut heading_for = target.or(ground_target);
            let attacking = heading_for.is_some();
            if heading_for.is_none() && order.kind != OrderKind::Idle {
                let goal = flow.goal(order.route).unwrap_or(here);
                let close = Fp::from_bits(stats.max_speed.to_bits().saturating_mul(ARRIVE_TICKS));
                // Following a unit never ends: it keeps making passes over it.
                let ends = order.kind != OrderKind::Attack && !flow.is_following(order.route);
                if ends && here.distance_squared(&goal) <= wide_mul(close, close) {
                    flow.release(order.route);
                    *order = Order::IDLE;
                } else {
                    heading_for = Some(goal);
                }
            }

            // With nothing to do, it lands, if there is open ground below to land on.
            let margin = radius.saturating_mul(fp_int(3));
            let near_edge = [here.x, here.y].iter().any(|&value| value < margin || Fp::from_bits(max) - value < margin);
            let landing = heading_for.is_none() && !near_edge && terrain.is_passable(here);
            let toward_zero = |value: Fp, full: Fp| (value - full / LANDING_TICKS).max(Fp::ZERO);
            let toward_full = |value: Fp, full: Fp| (value + full / LANDING_TICKS).min(full);
            if landing {
                flight.speed = toward_zero(flight.speed, stats.max_speed);
                flight.altitude = toward_zero(flight.altitude, flight.elevation);
            } else {
                flight.speed = toward_full(flight.speed, stats.max_speed);
                flight.altitude = toward_full(flight.altitude, flight.elevation);
            }
            let speed = flight.speed;

            let facing = turning.facing;
            turning.facing = match heading_for {
                Some(point) => {
                    let offset = point - here;
                    let distance_squared = here.distance_squared(&point);
                    if !attacking {
                        flight.breaking_off = false;
                    } else if flight.breaking_off {
                        flight.breaking_off = distance_squared < wide_mul(flight.break_off, flight.break_off);
                    } else {
                        // Once over the target and past it, fly on for a run-up to the next pass.
                        let behind = facing.x * offset.x + facing.y * offset.y <= Fp::ZERO;
                        let half = flight.break_off / 2;
                        flight.breaking_off = behind && distance_squared < wide_mul(half, half);
                    }
                    if flight.breaking_off || offset == Vec2Fp::ZERO || inside_turn(facing, offset, radius) {
                        facing
                    } else {
                        rotate_toward(facing, offset.normalized(), turning.max_turn)
                    }
                }
                // Coming down, it flies straight on as it slows.
                None if landing => {
                    flight.breaking_off = false;
                    facing
                }
                // Over water or cliffs, it circles where it is, unless that would take it off the map.
                None => {
                    flight.breaking_off = false;
                    let toward = if near_edge { (center - here).normalized() } else { Vec2Fp::new(-facing.y, facing.x) };
                    rotate_toward(facing, toward, turning.max_turn)
                }
            };

            let step = Vec2Fp::new(turning.facing.x * speed, turning.facing.y * speed);
            let next = Vec2Fp::new(Fp::from_bits((here.x + step.x).to_bits().clamp(0, max)), Fp::from_bits((here.y + step.y).to_bits().clamp(0, max)));
            // Landed, it rests on the ground like any other unit.
            position.0 = Vec3Fp::new(next.x, next.y, flying_height(terrain, next, flight.altitude.max(stats.radius)));
            velocity.0 = Vec3Fp::new(step.x, step.y, Fp::ZERO);
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
        assert!(bomb_lands_on(at(520), east, fp_int(20), target, fp_int(30)));
        assert!(!bomb_lands_on(at(900), east, fp_int(20), target, fp_int(30)), "too late");
        assert!(!bomb_lands_on(at(200), east, fp_int(20), target, fp_int(30)), "too early");
        let west = Vec2Fp::new(-Fp::ONE, Fp::ZERO);
        assert!(!bomb_lands_on(at(520), west, fp_int(20), target, fp_int(30)), "flying away");
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
