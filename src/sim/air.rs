//! Aircraft (GAME_SPECIFICATION.md §10). An aircraft flies at its own height over land and
//! water, straight at where it is going, with no pathfinding and nothing to bump into. In flight
//! it never stops, so it turns in arcs and makes passes over its target. With nothing to do it
//! lands. It burns fuel as it flies, and refuels and is repaired on an airfield.

use super::components::{rotate_toward, Combat, Flight, Order, OrderKind, Position, SimId, Turning, UnitStats, Velocity};
use super::config::GRAVITY_RAW;
use super::spatial::GridEntry;
use super::terrain::Terrain;
use super::world::SimWorld;
use crate::math::fp::{FP_FRAC_BITS, fp_int, mul_toward_zero, sqrt_wide, wide_mul, Fp, FpWide, Vec2Fp, Vec3Fp};

/// Bombs fall at the inspiration's gravity, 4.9 map units a second squared, which is ten times
/// what shells use here.
pub const BOMB_GRAVITY: Fp = Fp::from_bits(GRAVITY_RAW * 10);

/// Pi, in raw `Fp` bits.
const PI_RAW: i32 = 12_868;

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

/// How far an aircraft flying at `speed` goes while it slows to a stop at the rate it lands at,
/// from top speed to none in `LANDING_TICKS`: speed² · LANDING_TICKS / (2 · top).
fn stopping_distance(speed: Fp, top: Fp) -> Fp {
    if top <= Fp::ZERO {
        return Fp::ZERO;
    }
    let raw = wide_mul(speed, speed).to_bits() * i64::from(LANDING_TICKS) / 2 / i64::from(top.to_bits());
    Fp::from_bits(raw.clamp(0, i64::from(i32::MAX)) as i32)
}

/// The speed an aircraft slowing at its landing rate has with `distance` to go to a stop:
/// the inverse of `stopping_distance`.
fn speed_to_stop_in(distance: Fp, top: Fp) -> Fp {
    sqrt_wide(FpWide::from_bits(wide_mul(top, distance).to_bits().max(0) * 2 / i64::from(LANDING_TICKS)))
}

/// Between two aircraft on the ground, past their radii, in units.
const GROUND_GAP_UNITS: i32 = 4;
/// How far out, in spots, a landing aircraft looks for a free one.
const SPOT_SEARCH_RINGS: i32 = 6;

/// A spot to land on at or near `wanted`: the nearest free one of a grid three radii apart
/// about it, laid along the way the aircraft faces so that it turns with the map, nearer the
/// front first and then nearer its right. Free means clear of every other aircraft's spot by
/// both radii and a gap, and `fits`. None if there is none within reach.
fn free_spot(id: SimId, wanted: Vec2Fp, facing: Vec2Fp, radius: Fp, taken: &[(SimId, Vec2Fp, Fp)], fits: impl Fn(Vec2Fp) -> bool) -> Option<Vec2Fp> {
    let free = |point: Vec2Fp| {
        taken.iter().all(|&(other, at, other_radius)| {
            let contact = radius + other_radius + fp_int(GROUND_GAP_UNITS);
            other == id || point.distance_squared(&at) >= wide_mul(contact, contact)
        })
    };
    let spacing = radius.saturating_mul(fp_int(3));
    let right = Vec2Fp::new(facing.y, -facing.x);
    let mut offsets: Vec<(i32, i32)> = (-SPOT_SEARCH_RINGS..=SPOT_SEARCH_RINGS).flat_map(|along| (-SPOT_SEARCH_RINGS..=SPOT_SEARCH_RINGS).map(move |across| (along, across))).collect();
    offsets.sort_unstable_by_key(|&(along, across)| (along * along + across * across, -along, -across));
    offsets.into_iter().find_map(|(along, across)| {
        // Rounded toward zero, so that mirror images find mirror-image spots.
        let (ahead, aside) = (spacing.saturating_mul(fp_int(along)), spacing.saturating_mul(fp_int(across)));
        let point = Vec2Fp::new(
            wanted.x + mul_toward_zero(facing.x, ahead) + mul_toward_zero(right.x, aside),
            wanted.y + mul_toward_zero(facing.y, ahead) + mul_toward_zero(right.y, aside),
        );
        (fits(point) && free(point)).then_some(point)
    })
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

/// Ticks a hovering gunship drifts one way across its line to its target before it drifts back,
/// and the share of its top speed it drifts at; and the same for its drift in and out along that
/// line, slower and out of step, so that a group over its work is a loose, bobbing blob rather
/// than a still knot (the design owner, 2026-11-27).
pub const BOB_ACROSS_TICKS: u32 = 20;
pub const BOB_ACROSS_SHARE: i32 = 8;
pub const BOB_ALONG_TICKS: u32 = 30;
pub const BOB_ALONG_SHARE: i32 = 16;

/// How a gunship hovering since `since` drifts this tick, facing `facing` (towards its target):
/// across, then along, each a half swing first, so that it rocks about where it stopped. It
/// depends only on how long it has hovered and which way it faces, so mirror images drift in
/// mirror image, and gunships that stopped on different ticks drift out of step.
fn bob(since: u32, tick: u32, facing: Vec2Fp, top_speed: Fp) -> Vec2Fp {
    let hovered = tick.wrapping_sub(since);
    let sign = |period: u32| if ((hovered + period / 2) / period).is_multiple_of(2) { Fp::ONE } else { -Fp::ONE };
    let across = Vec2Fp::new(-facing.y, facing.x).scaled(top_speed * sign(BOB_ACROSS_TICKS) / BOB_ACROSS_SHARE);
    let along = facing.scaled(top_speed * sign(BOB_ALONG_TICKS) / BOB_ALONG_SHARE);
    Vec2Fp::new(across.x + along.x, across.y + along.y)
}

/// Below this share of a full tank, a patrolling aircraft goes to refuel.
const REFUEL_SHARE_PERCENT: u32 = 20;
/// Ticks of fuel gained each tick on the ground, the inspiration's recharge rate of 5, and on an
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
    /// Its four pads' corners, in the order aircraft take them (`pad_order`).
    pads: [(i32, i32); 4],
    slots: u8,
    used: u8,
    repair: Fp,
    energy: super::blueprints::Res,
}

impl Staging {
    fn pad(&self, pad: u8) -> Vec2Fp {
        let offset = fp_int(PAD_OFFSET_UNITS);
        let (x, y) = self.pads[usize::from(pad % 4)];
        Vec2Fp::new(self.center.x + offset * x, self.center.y + offset * y)
    }
}

/// The order an airfield's pads are taken in: nearest the map's middle first, and of two as
/// near, the one clockwise of the other about the middle. Both turn with the map, so an airfield
/// and its mirror image fill mirror-image pads. They were taken south-west, south-east,
/// north-west, north-east on every airfield, so that the first aircraft down on one side landed
/// on the pad nearest its enemy and its mirror image on the pad furthest from its own.
fn pad_order(center: Vec2Fp, middle: Vec2Fp) -> [(i32, i32); 4] {
    let mut pads = [(-1, -1), (1, -1), (-1, 1), (1, 1)];
    let (dx, dy) = (i64::from((center.x - middle.x).to_bits()), i64::from((center.y - middle.y).to_bits()));
    let offset = i64::from(fp_int(PAD_OFFSET_UNITS).to_bits());
    pads.sort_by_key(|&(x, y)| {
        let (px, py) = (dx + offset * i64::from(x), dy + offset * i64::from(y));
        // Clockwise about the middle: the cross product of where the airfield is and where the
        // pad is, from the middle, which keeps its sign under a half turn.
        (px * px + py * py, -(dx * py - dy * px).signum())
    });
    pads
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
                let pads = pad_order(center, Vec2Fp::new(fp_int(self.terrain.map_units() / 2), fp_int(self.terrain.map_units() / 2)));
                Some(Staging { id, team, center, pads, slots: def.slots, used: 0, repair: def.repair_per_tick, energy: def.energy_per_tick })
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
        let outer = terrain.outer_margin_raw();
        let center = Vec2Fp::new(Fp::from_bits(max / 2), Fp::from_bits(max / 2));
        let tick = self.current_tick;
        let locate = |target: SimId| -> Option<GridEntry> { units.binary_search_by_key(&target, |unit| unit.0).ok().map(|index| *grid.entry(index)) };
        // The spots aircraft have taken on the ground, to land on or resting on, so that no two
        // land on one another. Those taken this tick are added as they are.
        let mut taken: Vec<(SimId, Vec2Fp, Fp)> = self
            .aircraft
            .iter()
            .filter_map(|&(id, entity)| {
                let spot = self.ecs.get::<&Flight>(entity).ok()?.spot?;
                Some((id, spot, self.ecs.get::<&UnitStats>(entity).ok()?.radius))
            })
            .collect();
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
            Option<&super::components::OrderQueue>,
            Option<&super::components::Torpedo>,
        )>();
        let mut view = query.view();
        for &(id, entity) in &self.aircraft {
            let (position, velocity, stats, order, combat, turning, flight, health, patrol, bomb_bay, queue, torpedo) = view.get_mut(entity).expect("aircraft are live units");
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
                let economy = &mut economies[stats.owner.index()];
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
            // What it was told to attack comes first: what its guns fire at on the way does not
            // turn it, nor what they fire at on the way to a spot it was told to bomb.
            let chasing = if order.kind == OrderKind::Move {
                None
            } else if combat.forced_target.is_some() {
                combat.forced_target
            } else if combat.ground_target.is_some() && order.kind == OrderKind::Attack {
                None
            } else {
                combat.target
            };
            // A bomber with a gun, after an aircraft, flies as a fighter: its bomb is for the
            // ground, so it makes no bombing passes at a plane.
            let bombing = bomb_bay.is_some() && !chasing.and_then(locate).is_some_and(|entry| entry.is_air);
            // A torpedo bomber makes a bomber's runs too, letting its torpedoes go along its
            // heading (the design owner, 2026-11-27), though its torpedoes home on their ship, so
            // it heads for the ship itself and lets go anywhere within its reach.
            let torpedo_runs = torpedo.is_some() && bomb_bay.is_none();
            // A bomber heads for where the target will be when its bomb lands.
            let target = chasing.and_then(locate).map(|entry| {
                let at = Vec2Fp::new(Fp::from_bits(entry.x), Fp::from_bits(entry.y));
                if bombing { bomb_lead(at, entry.velocity, flight.elevation) } else { at }
            });
            let ground_target = combat.ground_target.filter(|_| order.kind == OrderKind::Attack);
            let mut heading_for = if staging.is_some() { None } else { target.or(ground_target) };
            let attacking = heading_for.is_some();
            let close = Fp::from_bits(stats.max_speed.to_bits().saturating_mul(ARRIVE_TICKS));
            // Where a move it has just finished was to: its spot to land on.
            let mut arrived_at = None;
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
                // A move is to its own spot, in its group's grid or the goal itself. The last one,
                // which it lands at, is over once it is close enough to slow down onto it; one on a
                // patrol, or with more to come, once it is there. An attack-move is over at its
                // goal, as it always was: it is a sweep, and fighting it lands only where it can.
                let spot = if ends { order.slot.unwrap_or(goal) } else { goal };
                let last = order.kind == OrderKind::Move && !patrolling && queue.is_none_or(|queue| queue.0.is_empty());
                let reach = if last { stopping_distance(stats.max_speed, stats.max_speed) + close } else { close };
                if ends && here.distance_squared(&spot) <= wide_mul(reach, reach) {
                    flow.release(order.route);
                    *order = Order::IDLE;
                    arrived_at = Some(spot);
                } else {
                    heading_for = Some(spot);
                }
            }

            // A gunship stops over what it attacks, well within range, turns to face it, and drifts
            // back and forth there (`bob`).
            let hold = flight.hovers
                && attacking
                && heading_for.is_some_and(|point| {
                    let reach = combat.range * 3 / 4;
                    here.distance_squared(&point) <= wide_mul(reach, reach)
                });
            // It notes when it came to a stop over its work, which sets how it drifts there.
            if !hold {
                flight.hovering_since = u32::MAX;
            } else if flight.hovering_since == u32::MAX {
                flight.hovering_since = tick;
            }
            // With nothing to do, it lands, if there is open ground below to land on. Patrols
            // and trips to refuel keep it in the air. It lands on the map, just inside the edge
            // if that is where it is: only where it is over the map and would still be once it
            // has glided to a stop, half its speed for the ticks it takes to slow; past the edge,
            // or heading out over it, it turns back in first.
            let glide = turning.facing.scaled(flight.speed.saturating_mul(fp_int(LANDING_TICKS / 2 + 1)));
            let near_edge = !terrain.in_bounds(here) || !terrain.in_bounds(here + glide);
            let idle = heading_for.is_none() && !patrolling && flight.refit.is_none();
            // It lands on a spot of its own, on open ground, off any structure (such as the
            // facility it has just left) and clear of the other aircraft down or coming down:
            // where it finished its move, or else where it would come to a stop flying straight
            // on. In the air, aircraft fly through one another as their ways cross; on the
            // ground they never overlap (the design owner, 2026-11-27). One already resting
            // keeps where it is.
            if !idle {
                flight.spot = None;
            } else if flight.spot.is_none() {
                let fits = |point: Vec2Fp| {
                    let on_map = terrain.in_bounds(point);
                    // The cell as units count it, so that mirror-image spots on a cell's edge
                    // are in mirror-image cells.
                    let (cell_x, cell_y) = terrain.unit_cell_of(point);
                    on_map && terrain.is_passable(point) && !occupancy.contains_key(&((cell_y * terrain.cells_per_side() + cell_x) as u32))
                };
                flight.spot = if !flight.airborne() && flight.speed == Fp::ZERO && fits(here) {
                    Some(here)
                } else {
                    let ahead = turning.facing.scaled(stopping_distance(flight.speed, stats.max_speed));
                    let wanted = arrived_at.unwrap_or(Vec2Fp::new(here.x + ahead.x, here.y + ahead.y));
                    free_spot(id, wanted, turning.facing, stats.radius, &taken, fits)
                };
                if let Some(spot) = flight.spot {
                    taken.push((id, spot, stats.radius));
                }
            }
            // On its way to its spot, it slows as it nears it so as to stop on it, and comes
            // down once it is slowing.
            let mut final_speed = None;
            if let (true, Some(spot)) = (idle, flight.spot) {
                if spot != here {
                    heading_for = Some(spot);
                    let distance = sqrt_wide(here.distance_squared(&spot));
                    final_speed = Some((speed_to_stop_in(distance, stats.max_speed).max(stats.max_speed / LANDING_TICKS), distance));
                }
            }
            let landing = idle && flight.spot == Some(here);
            // Out of fuel, it flies at half its height and speed.
            let (top_speed, height) = (if flight.out_of_fuel() { stats.max_speed / 2 } else { stats.max_speed }, flight.cruising_height());
            // A fighter in range behind an aircraft it attacks eases off to the speed its target
            // flies away from it, to stay on its tail rather than overshoot and loop round; never
            // below half its own, and it still turns no faster than it can.
            let tailing = chasing.filter(|_| !bombing && !flight.hovers).and_then(locate).filter(|entry| entry.is_air).and_then(|entry| {
                let offset = Vec2Fp::new(Fp::from_bits(entry.x), Fp::from_bits(entry.y)) - here;
                let facing = turning.facing;
                let reach = combat.range * 3 / 4;
                let behind = facing.x * offset.x + facing.y * offset.y > Fp::ZERO && offset.length_squared() <= wide_mul(reach, reach);
                behind.then(|| {
                    let away = facing.x * Fp::from_bits(entry.velocity[0]) + facing.y * Fp::from_bits(entry.velocity[1]);
                    away.clamp(flight.min_speed.min(top_speed), top_speed)
                })
            });
            // A point inside its turn at full speed it reaches by easing off just enough to turn
            // onto it, rather than fly on and loop round, as long as that is no slower than its
            // slowest (`min_speed`): tighter still, it flies on at full speed. A turn it can make at
            // full speed it makes at full speed. In a dogfight too (the design owner, 2026-11-26).
            // Not while a bomber breaks off, which flies on on purpose.
            let floor = flight.min_speed.min(top_speed);
            let turning_in = heading_for.filter(|_| floor < top_speed && !flight.hovers && !flight.breaking_off && !hold && !landing).and_then(|point| {
                let offset = point - here;
                let facing = turning.facing;
                (offset != Vec2Fp::ZERO && inside_turn(facing, offset, turn_radius(top_speed, turning.max_turn))).then_some(())?;
                let across = (facing.x * offset.y - facing.y * offset.x).abs();
                // The circle through it that leaves along its heading, d² / 2·across.
                let fits = Fp::from_bits(i32::try_from((wide_mul(offset.x, offset.x) + wide_mul(offset.y, offset.y)).to_bits() / (2 * i64::from(across.to_bits().max(1)))).unwrap_or(i32::MAX));
                let speed = fits.saturating_mul(turning.max_turn);
                (speed >= floor).then_some(speed.min(top_speed))
            });
            let down = |value: Fp, full: Fp, floor: Fp| (value - full / LANDING_TICKS).max(floor);
            let up = |value: Fp, full: Fp, ceiling: Fp| if value > ceiling { down(value, full, ceiling) } else { (value + full / LANDING_TICKS).min(ceiling) };
            let coming_down = final_speed.is_some_and(|(speed, _)| speed < top_speed);
            if landing {
                // On its spot, it has stopped, and settles onto it.
                flight.speed = Fp::ZERO;
                flight.altitude = down(flight.altitude, flight.elevation, Fp::ZERO);
            } else if hold {
                flight.speed = down(flight.speed, stats.max_speed, Fp::ZERO);
                flight.altitude = up(flight.altitude, flight.elevation, height);
            } else {
                let slowing = final_speed.map_or(top_speed, |(speed, _)| speed);
                flight.speed = up(flight.speed, stats.max_speed, tailing.unwrap_or(top_speed).min(turning_in.unwrap_or(top_speed)).min(slowing));
                flight.altitude = if coming_down { down(flight.altitude, flight.elevation, Fp::ZERO) } else { up(flight.altitude, flight.elevation, height) };
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
            // A torpedo bomber lets go from as far out as its reach, so it throws nothing: it
            // needs only room to turn round and settle on the line.
            let throw = if bombing { Some(bomb_throw(flight.elevation, stats.max_speed)) } else { torpedo_runs.then_some(Fp::ZERO) };
            let break_off = match throw {
                Some(throw) => {
                    // Room to line up again, however tight its turn: a target moving across its
                    // path needs a straight run at it.
                    let run_up = stats.max_speed.saturating_mul(fp_int(BOMB_RUN_UP_TICKS));
                    flight.break_off.max(throw + radius.saturating_mul(fp_int(2)).max(run_up) + run_up)
                }
                None => flight.break_off,
            };
            // And further still while it would be back where it lets go before its bomb is ready,
            // round half its turn and back less the throw, rather than pass over unready and go
            // round again: a bomber that turns quicker than it reloads wastes no passes.
            let unready_reach = throw.and_then(|throw| {
                let half_turn = Fp::from_bits(PI_RAW).saturating_div(turning.max_turn).to_num::<i32>();
                // With a run-up to spare, as when lining up: it is ready before it gets there.
                let spare = i32::try_from(combat.ticks_until_ready).unwrap_or(i32::MAX).saturating_add(BOMB_RUN_UP_TICKS).saturating_sub(half_turn);
                (spare > 0).then(|| throw.saturating_add(stats.max_speed.saturating_mul(fp_int(spare))))
            });
            let facing = turning.facing;
            turning.facing = match heading_for {
                Some(point) => {
                    let offset = point - here;
                    let distance_squared = here.distance_squared(&point);
                    if !attacking || flight.hovers {
                        flight.breaking_off = false;
                    } else if flight.breaking_off {
                        let out = unready_reach.map_or(break_off, |reach| reach.max(break_off));
                        // At the outer margin past the map's edge it turns back whatever its run.
                        let at_limit = [here.x, here.y].iter().any(|&value| value.to_bits() <= 1 - outer || value.to_bits() >= max + outer);
                        // And once it has flown twice that, however near the target still is: an
                        // aircraft on its tail stays near, and once nothing pushed aircraft apart
                        // in flight a fighter-bomber broke off from the interceptor behind it for
                        // good (2026-11-27).
                        flight.breaking_off_ticks = flight.breaking_off_ticks.saturating_add(1);
                        let flown = i64::from(speed.to_bits()) * i64::from(flight.breaking_off_ticks);
                        let long_enough = flown >= 2 * i64::from(out.to_bits());
                        flight.breaking_off = distance_squared < wide_mul(out, out) && !at_limit && !long_enough;
                    } else {
                        flight.breaking_off_ticks = 0;
                        // Once over the target and past it, fly on for a run-up to the next pass.
                        let behind = facing.x * offset.x + facing.y * offset.y <= Fp::ZERO;
                        let half = break_off / 2;
                        flight.breaking_off = behind && distance_squared < wide_mul(half, half);
                    }
                    // A gunship can turn on the spot, so nothing is out of its reach. Easing off
                    // to turn in, it turns as tight as its speed allows.
                    let unreachable = !flight.hovers && inside_turn(facing, offset, if turning_in.is_some() || final_speed.is_some() { turn_radius(speed, turning.max_turn) } else { radius });
                    if flight.breaking_off || offset == Vec2Fp::ZERO || unreachable {
                        facing
                    } else {
                        rotate_toward(facing, offset.normalized(), turning.max_turn)
                    }
                }
                // On its spot, it stays facing the way it came in.
                None if landing => {
                    flight.breaking_off = false;
                    facing
                }
                // Over water or cliffs, or waiting for a pad, it circles where it is, unless that
                // would take it off the map, and past the edge it turns back onto it.
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
            // The last stretch onto its spot it covers exactly.
            let step = match (final_speed, flight.spot) {
                (Some((_, distance)), Some(spot)) if speed >= distance => spot - here,
                _ => turning.facing.scaled(speed),
            };
            let step = if hold {
                let drift = bob(flight.hovering_since, tick, turning.facing, top_speed);
                Vec2Fp::new(step.x + drift.x, step.y + drift.y)
            } else {
                step
            };
            // It may fly on past the map's edge, as after a pass, and turns back of itself; the
            // outer margin only stops a runaway. Kept a raw bit further in on the low side, so
            // that the limits are mirror images: the map runs from 0 to a raw bit short of its
            // size, and mirrored matches parted where one side was held a bit further out.
            let next = Vec2Fp::new(Fp::from_bits((here.x + step.x).to_bits().clamp(1 - outer, max + outer)), Fp::from_bits((here.y + step.y).to_bits().clamp(1 - outer, max + outer)));
            // Landed, it rests on the ground like any other unit.
            position.0 = Vec3Fp::new(next.x, next.y, flying_height(terrain, next, flight.altitude.max(stats.radius)));
            velocity.0 = Vec3Fp::new(step.x, step.y, Fp::ZERO);
        }
        self.separate_holding_gunships();
    }

    /// Gunships holding over their work keep a little apart, a loose blob rather than one on top
    /// of another; no other aircraft does in flight (the design owner, 2026-11-27). Any two
    /// closer than their radii and a margin are pushed apart, a third of the overlap a tick, so
    /// that they spread without jittering. In order along x, so only near neighbors are compared.
    /// Divisions round toward zero, so mirror-image pairs move in mirror image. Two exactly
    /// together part side by side, the lower id to the right of the way it faces, which turns
    /// with the map as a compass direction does not.
    fn separate_holding_gunships(&mut self) {
        const MARGIN_RAW: i64 = 4 << 12;
        let mut flying: Vec<(i64, i64, i64, SimId, hecs::Entity, Vec2Fp)> = self
            .aircraft
            .iter()
            .filter_map(|&(id, entity)| {
                let flight = self.ecs.get::<&Flight>(entity).ok()?;
                if flight.altitude <= Fp::ZERO || flight.hovering_since == u32::MAX {
                    return None;
                }
                let position = self.ecs.get::<&Position>(entity).ok()?.0;
                let radius = self.ecs.get::<&UnitStats>(entity).ok()?.radius;
                let facing = self.ecs.get::<&Turning>(entity).ok()?.facing;
                Some((i64::from(position.x.to_bits()), i64::from(position.y.to_bits()), i64::from(radius.to_bits()), id, entity, facing))
            })
            .collect();
        if flying.len() < 2 {
            return;
        }
        flying.sort_unstable_by_key(|&(x, _, _, id, _, _)| (x, id));
        let widest = flying.iter().map(|aircraft| aircraft.2).max().unwrap_or(0);
        let mut pushes = vec![(0_i64, 0_i64); flying.len()];
        for first in 0..flying.len() {
            let (x, y, radius, id, _, facing) = flying[first];
            for second in first + 1..flying.len() {
                let (other_x, other_y, other_radius, other_id, _, other_facing) = flying[second];
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
                    // Exactly together: the lower id to the right of the way it faces.
                    let (sign, facing) = if id < other_id { (1, facing) } else { (-1, other_facing) };
                    let (fx, fy) = (i64::from(facing.x.to_bits()), i64::from(facing.y.to_bits()));
                    let one = 1_i64 << FP_FRAC_BITS;
                    (sign * fy * overlap / one / 6, -sign * fx * overlap / one / 6)
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
        let outer = i64::from(self.terrain.outer_margin_raw());
        for (index, &(_, _, _, _, entity, _)) in flying.iter().enumerate() {
            let (push_x, push_y) = pushes[index];
            if push_x == 0 && push_y == 0 {
                continue;
            }
            let mut position = self.ecs.get::<&mut Position>(entity).expect("aircraft are live units");
            let x = (i64::from(position.0.x.to_bits()) + push_x).clamp(1 - outer, max + outer) as i32;
            let y = (i64::from(position.0.y.to_bits()) + push_y).clamp(1 - outer, max + outer) as i32;
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
