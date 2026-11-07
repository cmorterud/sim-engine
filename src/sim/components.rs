use super::blueprints::{BlueprintId, Res};
use crate::math::fp::{cos_sin_radians, radians_from_tenths_of_degree, sqrt_wide, wide_mul, Fp, FpWide, Vec2Fp, Vec3Fp};

/// Assigned from a counter on spawn and never reused. The stable identity for
/// ordering, hashing, and interpolation. `hecs::Entity` is only a storage handle.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SimId(pub u32);

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Team {
    Blue,
    Red,
}

impl Team {
    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn enemy(self) -> Self {
        match self {
            Team::Blue => Team::Red,
            Team::Red => Team::Blue,
        }
    }
}

/// Selects a cost grid. The PoC only spawns `Land` units.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MoveClass {
    Land,
    Hover,
    Naval,
    Seabed,
    Air,
}

#[derive(Copy, Clone, Debug)]
pub struct Position(pub Vec3Fp);

#[derive(Copy, Clone, Debug)]
pub struct Velocity(pub Vec3Fp);

#[derive(Copy, Clone, Debug)]
pub struct UnitStats {
    pub max_speed: Fp,
    pub radius: Fp,
    pub team: Team,
    pub move_class: MoveClass,
    pub is_structure: bool,
    /// Sight radius, when fog of war is on.
    pub vision: Fp,
}

#[derive(Copy, Clone, Debug)]
pub struct Health {
    pub current: Fp,
    pub max: Fp,
}

#[derive(Copy, Clone, Debug)]
pub struct Combat {
    pub range: Fp,
    pub reload_ticks: u32,
    pub ticks_until_ready: u32,
    pub projectile_speed: Fp,
    pub projectile_damage: Fp,
    pub projectile_splash: Fp,
    pub target: Option<SimId>,
    /// Set by an Attack order. Other enemies are ignored until it dies.
    pub forced_target: Option<SimId>,
    /// A target the ground stops this unit's shots at, and the tick until which it is passed
    /// over, so that the unit looks elsewhere or walks on instead of shelling a hill.
    pub blocked: Option<(SimId, u32)>,
    /// Set by a ground attack: fire at this spot, and at nothing else, until given another order.
    pub ground_target: Option<Vec2Fp>,
    /// False while a structure is under construction.
    pub enabled: bool,
}

#[derive(Copy, Clone, Debug)]
pub struct Projectile {
    pub velocity: Vec3Fp,
    pub damage: Fp,
    pub source: SimId,
    pub source_team: Team,
    pub lifetime_ticks: u32,
    /// Blast radius on impact. Zero means a direct hit on one unit.
    pub splash_radius: Fp,
    /// With friendly fire on, ticks until this shell can hit allies. It flies clear of
    /// friendly lines to its aim point, and only a round that overshoots is a stray.
    pub allied_safe_ticks: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OrderKind {
    Idle,
    /// Keeps moving while firing at anything in range.
    Move,
    /// Stops to fire when a target is in range.
    AttackMove,
    /// Closes on `Combat::forced_target` and stops once it is in range.
    Attack,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Order {
    pub kind: OrderKind,
    /// Flow field route. Meaningless when `kind` is `Idle`.
    pub route: u32,
    /// This unit's spot in its group's arrival formation.
    pub slot: Option<Vec2Fp>,
    /// How long it may spend walking to its spot, so that a blocked spot cannot trap it.
    pub slot_ticks: u32,
}

impl Order {
    pub const IDLE: Self = Self { kind: OrderKind::Idle, route: 0, slot: None, slot_ticks: 0 };
}

/// Marks a unit made from a blueprint. PoC stress-test units have none.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Blueprinted(pub BlueprintId);

/// Present while a structure is being built. Finished structures lose this component.
#[derive(Copy, Clone, Debug)]
pub struct Construction {
    /// From 0 to 1.
    pub progress: Res,
}

/// The terrain cells a structure occupies.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Footprint {
    pub cell_x: u32,
    pub cell_y: u32,
    pub size: u32,
}

/// Bonuses from touching neighbors (GAME_SPECIFICATION.md §4), in thousandths.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Adjacency {
    pub mass_output_bonus: u32,
    pub energy_cost_discount: u32,
    pub mass_cost_discount: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BuildTask {
    Build { blueprint: BlueprintId, cell_x: u32, cell_y: u32, site: Option<SimId> },
    Assist { target: SimId },
    Reclaim { target: SimId },
    Repair { target: SimId },
    /// Walk somewhere as part of a queue. Until `started`, this task holds the route's reference.
    Walk { order: Order, started: bool },
}

impl BuildTask {
    /// Whether two tasks are the same order. A build is the same order before and after
    /// its site appears.
    pub fn is_same_order(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Self::Build { blueprint, cell_x, cell_y, .. }, Self::Build { blueprint: other_blueprint, cell_x: other_x, cell_y: other_y, .. }) => {
                blueprint == other_blueprint && cell_x == other_x && cell_y == other_y
            }
            (Self::Walk { order, .. }, Self::Walk { order: other_order, .. }) => order.route == other_order.route,
            _ => self == other,
        }
    }
}

/// Which way a unit's body faces, and how it turns. A unit only moves forward along its facing.
/// With a turn radius, it turns like a car: only while moving, in an arc no tighter than the
/// radius, slowing but not stopping in a tight turn. A goal inside the circle it would turn in
/// cannot be reached that way, so it pivots slowly on the spot instead. Without a radius, it
/// turns on the spot, like a walker.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Turning {
    /// A unit vector.
    pub facing: Vec2Fp,
    /// The most it can turn in a tick, in radians.
    pub max_turn: Fp,
    /// The tightest arc it can turn in, in units. Zero turns on the spot.
    pub radius: Fp,
}

/// Turns a unit vector towards another by at most `turn` radians, the short way round.
/// Exactly behind has no shorter side, so it turns the positive way.
pub fn rotate_toward(facing: Vec2Fp, toward: Vec2Fp, turn: Fp) -> Vec2Fp {
    let dot = facing.x * toward.x + facing.y * toward.y;
    let (cos, sin) = cos_sin_radians(FpWide::from_bits(i64::from(turn.to_bits()) << 12));
    if dot >= cos {
        return toward;
    }
    let left = facing.x * toward.y - facing.y * toward.x >= Fp::ZERO;
    let sin = if left { sin } else { -sin };
    Vec2Fp::new(facing.x * cos - facing.y * sin, facing.x * sin + facing.y * cos).normalized()
}

/// A weapon that has to swivel to its target before it can fire.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Turret {
    /// A unit vector.
    pub facing: Vec2Fp,
    /// The most it can turn in a tick, in radians.
    pub max_turn: Fp,
}

/// About cos(6 degrees): a turret pointing this close to its target can fire.
pub const AIMED: Fp = Fp::from_bits(4_074);

impl Turret {
    /// From a turn rate in degrees per second.
    pub fn new(facing: Vec2Fp, degrees_per_second: u32) -> Self {
        let max_turn = radians_from_tenths_of_degree(degrees_per_second.min(900));
        Self { facing, max_turn: Fp::from_bits((max_turn.to_bits() >> 12) as i32) }
    }

    /// Swivels towards a direction, given as a unit vector, and returns whether it now points close enough to fire.
    pub fn aim(&mut self, toward: Vec2Fp) -> bool {
        self.facing = rotate_toward(self.facing, toward, self.max_turn);
        self.facing.x * toward.x + self.facing.y * toward.y >= AIMED
    }
}

/// In a tight turn, a vehicle slows to this share of its speed.
const TURNING_SPEED_SHARE: Fp = Fp::from_bits(2_048);
/// A unit pivoting on the spot turns at this share of its turn rate.
const PIVOT_SHARE: Fp = Fp::from_bits(2_048);
/// About cos(20 degrees): closer to its goal than this, a unit is already facing it.
const ROUGHLY_FACING: Fp = Fp::from_bits(3_849);

impl Turning {
    /// From a turn rate in degrees per second, and a turn radius in units. At ten ticks a
    /// second, the rate is the turn per tick in tenths of a degree.
    pub fn new(facing: Vec2Fp, degrees_per_second: u32, radius: Fp) -> Self {
        let max_turn = radians_from_tenths_of_degree(degrees_per_second.min(900));
        Self { facing, max_turn: Fp::from_bits((max_turn.to_bits() >> 12) as i32), radius }
    }

    /// Turns towards `desired` as far as one tick allows, and returns the velocity the unit
    /// can actually have. `goal_distance` is how far away it is going.
    pub fn steer(&mut self, desired: Vec2Fp, goal_distance: Fp) -> Vec2Fp {
        if desired == Vec2Fp::ZERO {
            return desired;
        }
        let speed = sqrt_wide(desired.length_squared());
        let toward = desired.normalized();
        let facing = self.facing;
        let dot = facing.x * toward.x + facing.y * toward.y;
        // Exactly behind has no shorter side, so it turns the positive way.
        let left = facing.x * toward.y - facing.y * toward.x >= Fp::ZERO;
        let (forward, turn) = if self.radius == Fp::ZERO {
            (speed * dot.max(Fp::ZERO), self.max_turn)
        } else if dot < ROUGHLY_FACING && self.goal_is_inside_turn(toward, goal_distance, left) {
            (Fp::ZERO, self.max_turn * PIVOT_SHARE)
        } else {
            let forward = speed * dot.clamp(TURNING_SPEED_SHARE, Fp::ONE);
            (forward, self.max_turn.min(forward / self.radius))
        };
        self.facing = rotate_toward(facing, toward, turn);
        let ahead = if self.radius == Fp::ZERO { forward.min(speed * (self.facing.x * toward.x + self.facing.y * toward.y).max(Fp::ZERO)) } else { forward };
        Vec2Fp::new(self.facing.x * ahead, self.facing.y * ahead)
    }

    /// Whether the goal is inside the circle this unit would turn in on that side, where
    /// driving on would only go round it.
    fn goal_is_inside_turn(&self, toward: Vec2Fp, goal_distance: Fp, left: bool) -> bool {
        let radius = self.radius;
        if goal_distance >= radius * 2 {
            return false;
        }
        let goal = Vec2Fp::new(toward.x * goal_distance, toward.y * goal_distance);
        let (side_x, side_y) = if left { (-self.facing.y, self.facing.x) } else { (self.facing.y, -self.facing.x) };
        // Inside the circle of this radius centred to that side: |goal - side * radius| < radius.
        goal.length_squared() < wide_mul(radius * 2, goal.x * side_x + goal.y * side_y)
    }
}

/// The commander's Overcharge weapon, ready to fire again after its own reload.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OverchargeWeapon {
    pub energy: Res,
    pub damage: Fp,
    pub structure_damage: Fp,
    pub reload_ticks: u32,
    pub ticks_until_ready: u32,
}

/// An order to Overcharge one enemy, as soon as it is in range and there is the energy.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OverchargeOrder {
    pub target: SimId,
}

/// Marks an Overcharge shell. It does at most this much to a structure.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Overcharged {
    pub structure_damage: Fp,
}

/// Experience from destroying enemies, and the rank it has earned, from 1 to 5. Units with
/// none have neither component.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Veterancy {
    /// The mass of the enemies this unit has destroyed.
    pub experience: Res,
    pub rank: u8,
}

/// A shield generator's dome: a sphere around the generator's base that stops enemy shells
/// coming in from outside, while shells from inside fly out freely (GAME_SPECIFICATION.md §8.1).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Shield {
    pub radius: Fp,
    pub health: Fp,
    pub max_health: Fp,
    pub regen_per_tick: Fp,
    /// Ticks until a collapsed dome comes back at full strength. Zero while it is up.
    pub recharging: u32,
    pub recharge_ticks: u32,
    /// Energy per tick while up, before any discount from neighbors.
    pub upkeep: Res,
}

impl Shield {
    pub const fn is_up(&self) -> bool {
        self.recharging == 0
    }

    /// Knocks the dome down, to come back at full strength after its recharge time.
    pub fn collapse(&mut self) {
        self.health = Fp::ZERO;
        self.recharging = self.recharge_ticks.max(1);
    }
}

/// A weapon that fires missiles instead of shells (GAME_SPECIFICATION.md §8).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Launcher {
    /// Nuclear: fired only on order, using up a missile that its silo built.
    pub strategic: bool,
    pub cruise_height: Fp,
    /// Missiles built and waiting. Only strategic launchers use it.
    pub stock: u32,
}

/// A defense that destroys enemy missiles in flight.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MissileDefense {
    /// Stops nuclear missiles, one interceptor each. Otherwise stops cruise missiles.
    pub strategic: bool,
    pub range: Fp,
    pub reload_ticks: u32,
    pub ticks_until_ready: u32,
    /// Interceptors built and waiting. Only strategic defenses use it.
    pub stock: u32,
}

/// A projectile that flies itself to a spot: up to its cruise height above the ground, along
/// the ground at that height, and down onto the spot.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Missile {
    pub target: Vec2Fp,
    /// Units per tick, along the ground.
    pub speed: Fp,
    pub cruise_height: Fp,
    pub strategic: bool,
}

/// An upgrade in place, to the blueprint the unit's own one upgrades to. A commander stands
/// still meanwhile. Factories upgrade through their queue instead.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Upgrading {
    pub into: BlueprintId,
    /// From 0 to 1.
    pub progress: Res,
}

/// Marks a factory or a construction site that the player has paused. It spends nothing and
/// makes no progress until resumed, and keeps its queue and progress meanwhile.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Paused;

/// Movement orders waiting their turn, for a unit that is not a builder. Builders keep theirs
/// among their tasks. Each order holds a reference to its route until it starts.
#[derive(Clone, Debug, Default)]
pub struct OrderQueue(pub Vec<Order>);

/// An engineer or commander. Works through its tasks in order.
#[derive(Clone, Debug)]
pub struct Builder {
    pub power: Fp,
    pub range: Fp,
    pub tasks: Vec<BuildTask>,
}

#[derive(Clone, Debug)]
pub struct Factory {
    pub power: Fp,
    /// The front item is in production. An item that is a factory is an upgrade.
    pub queue: Vec<BlueprintId>,
    pub repeat: bool,
    /// Progress on the front item, from 0 to 1.
    pub progress: Res,
    pub rally: Option<Vec2Fp>,
}

/// A repeating route through waypoints.
#[derive(Clone, Debug)]
pub struct Patrol {
    pub points: Vec<Vec2Fp>,
    pub next: u32,
}

/// What builders can reclaim: battlefield debris, or the trees and rocks that come with the map.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WreckKind {
    Debris,
    /// Mostly energy, and quick to reclaim.
    Tree,
    /// Pure mass.
    Rock,
}

impl WreckKind {
    /// Trees and rocks are part of the map, so they show through fog of war, like the terrain.
    pub const fn is_scenery(self) -> bool {
        !matches!(self, Self::Debris)
    }
}

/// Something builders can reclaim, for mass and energy.
#[derive(Copy, Clone, Debug)]
pub struct Wreck {
    pub mass: Res,
    pub energy: Res,
    pub position: Vec2Fp,
    pub radius: Fp,
    pub kind: WreckKind,
    /// What it takes to destroy debris. Shells that land on it wear it down, and it loses mass
    /// and energy in proportion. Zero for trees and rocks, which shells leave alone.
    pub health: Fp,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::fp::fp_int;

    fn east() -> Vec2Fp {
        Vec2Fp::new(Fp::ONE, Fp::ZERO)
    }

    fn far() -> Fp {
        fp_int(10_000)
    }

    /// Drives a unit at 4 units a tick towards a goal, and returns where it went.
    fn drive(turning: &mut Turning, goal: Vec2Fp, ticks: usize) -> Vec<Vec2Fp> {
        let mut here = Vec2Fp::ZERO;
        let mut path = Vec::new();
        for _ in 0..ticks {
            let away = goal - here;
            let distance = sqrt_wide(away.length_squared());
            if distance <= fp_int(4) {
                break;
            }
            let desired = Vec2Fp::new(away.x * 4 / distance, away.y * 4 / distance);
            let velocity = turning.steer(desired, distance);
            here = here + velocity;
            path.push(here);
        }
        path
    }

    #[test]
    fn a_unit_facing_its_goal_goes_at_full_speed() {
        for radius in [Fp::ZERO, fp_int(40)] {
            let mut turning = Turning::new(east(), 90, radius);
            assert_eq!(turning.steer(Vec2Fp::new(fp_int(4), Fp::ZERO), far()), Vec2Fp::new(fp_int(4), Fp::ZERO));
        }
    }

    #[test]
    fn a_walker_turns_on_the_spot_to_go_back() {
        // 180 degrees a second is 18 degrees a tick, so turning right round takes ten ticks.
        let mut turning = Turning::new(east(), 180, Fp::ZERO);
        let back = Vec2Fp::new(fp_int(-4), Fp::ZERO);
        let mut ticks = 0;
        let mut first_moves = Vec::new();
        while turning.facing.x > Fp::from_num(-0.999) {
            first_moves.push(turning.steer(back, far()));
            ticks += 1;
            assert!(ticks < 20, "it never turned round");
        }
        assert!((10..=11).contains(&ticks), "took {ticks} ticks");
        for velocity in &first_moves[..4] {
            assert_eq!(*velocity, Vec2Fp::ZERO, "a walker facing away stays where it is while it turns");
        }
    }

    #[test]
    fn a_vehicle_keeps_moving_while_it_turns_and_never_turns_tighter_than_its_radius() {
        let radius = fp_int(40);
        let mut turning = Turning::new(east(), 360, radius);
        // A goal far behind, so that it has to come all the way round.
        let path = drive(&mut turning, Vec2Fp::new(fp_int(-2_000), Fp::ZERO), 1_000);
        assert!(path.len() > 20);
        let mut previous = Vec2Fp::ZERO;
        let mut facing = east();
        for (tick, spot) in path.iter().enumerate().take(60) {
            let step = *spot - previous;
            let travelled = sqrt_wide(step.length_squared());
            assert!(travelled > Fp::ZERO, "a vehicle cannot stand still to turn, but did at tick {tick}");
            let heading = step.normalized();
            let turned = facing.x * heading.x + facing.y * heading.y;
            // Turning by an angle no larger than travelled / radius: cos is at least 1 - angle^2 / 2.
            let angle = travelled / radius;
            assert!(turned >= Fp::ONE - angle * angle / 2 - Fp::from_bits(40), "too tight a turn at tick {tick}");
            facing = heading;
            previous = *spot;
        }
        // The U-turn swings out to the side, about one radius, instead of pivoting.
        let widest = path.iter().map(|spot| spot.y.abs()).max().unwrap();
        assert!(widest >= fp_int(60) && widest <= fp_int(120), "it swung {widest} to the side");
        assert!(sqrt_wide((*path.last().unwrap() - Vec2Fp::new(fp_int(-2_000), Fp::ZERO)).length_squared()) <= fp_int(8), "it never got there");
    }

    #[test]
    fn a_vehicle_reaches_a_goal_inside_its_turning_circle_instead_of_circling_it() {
        let mut turning = Turning::new(east(), 360, fp_int(40));
        // Just beside it, closer than it could ever turn to.
        let goal = Vec2Fp::new(Fp::ZERO, fp_int(30));
        let path = drive(&mut turning, goal, 200);
        assert!(path.len() < 120, "it took {} ticks, so it may be going round in circles", path.len());
    }

    #[test]
    fn a_turret_swivels_at_its_own_rate_and_is_only_aimed_when_on_target() {
        // 90 degrees a second is 9 degrees a tick, so half a turn takes 20 ticks.
        let mut turret = Turret::new(east(), 90);
        let behind = Vec2Fp::new(-Fp::ONE, Fp::ZERO);
        let mut ticks = 0;
        while !turret.aim(behind) {
            ticks += 1;
            assert!(ticks < 40, "it never came round");
        }
        assert!((19..=21).contains(&ticks), "took {ticks} ticks");
        assert!(turret.aim(behind), "it stays on target");
    }

    #[test]
    fn standing_still_keeps_the_facing() {
        let mut turning = Turning::new(east(), 180, fp_int(40));
        assert_eq!(turning.steer(Vec2Fp::ZERO, far()), Vec2Fp::ZERO);
        assert_eq!(turning.facing, east());
    }

    #[test]
    fn facing_stays_a_unit_vector() {
        let mut turning = Turning::new(east(), 90, fp_int(30));
        drive(&mut turning, Vec2Fp::new(fp_int(-500), fp_int(300)), 300);
        let length = sqrt_wide(turning.facing.length_squared());
        assert!((length - Fp::ONE).abs() <= Fp::from_bits(8), "facing is no longer a unit vector: {length}");
    }
}
