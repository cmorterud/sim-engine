use super::blueprints::{BlueprintId, EnhancementId, Res};
use crate::math::fp::{cos_sin_radians, mul_toward_zero, radians_from_tenths_of_degree, sqrt_wide, wide_mul, Fp, FpWide, Vec2Fp, Vec3Fp};

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

/// A player (GAME_SPECIFICATION.md §17.2): one commander, one economy, and the units they own,
/// on a team. A team is allies, who share sight and fight together; what a player builds is
/// theirs alone. In a 1v1 player 0 is Blue and player 1 Red, so the index of each is its team's.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlayerId(pub u8);

impl PlayerId {
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// How many enemy units a unit has destroyed: those it made the final hit on. Shown on its
/// nameplate, and nothing else: it changes nothing in play, so it is not hashed.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Kills(pub u32);

/// Selects a cost grid. The PoC only spawns `Land` units.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MoveClass {
    Land,
    Hover,
    Naval,
    Seabed,
    Air,
    /// Sails under the water, on the same water as ships.
    Submarine,
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
    /// The player it belongs to, on `team`: whose economy it draws on and adds to.
    pub owner: PlayerId,
    pub move_class: MoveClass,
    pub is_structure: bool,
    /// Sight radius, when fog of war is on.
    pub vision: Fp,
    /// How many units its team had before it. Work spread over several ticks, such as looking
    /// for targets, is staggered by this rather than by `SimId`: the two sides' ids interleave,
    /// so a unit and its mirror image would look on different ticks, and whichever team had the
    /// lower ids won more often.
    pub phase: u32,
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
    /// Whether the forced target is one this unit found on an attack-move and is chasing out of
    /// range, rather than one it was told to attack. Its move waits at the front of its queue,
    /// to go on with once the target is destroyed.
    pub chasing: bool,
    /// A target the ground stops this unit's shots at, and the tick until which it is passed
    /// over, so that the unit looks elsewhere or walks on instead of shelling a hill.
    pub blocked: Option<(SimId, u32)>,
    /// Set by a ground attack: fire at this spot, and at nothing else, until given another order.
    pub ground_target: Option<Vec2Fp>,
    /// The ground stops the shot at the ordered target or spot from here, so the unit walks
    /// closer instead of firing into it.
    pub line_blocked: bool,
    /// While the ground hides what it was ordered to shoot: where the unit last was when it
    /// had moved on, and that tick. One that stays put a while fires into the ground anyway,
    /// until it moves again or has a clear shot.
    pub closing: Option<([i32; 2], u32)>,
    /// False while a structure is under construction.
    pub enabled: bool,
}

/// How long a unit walking closer to an ordered target, for a clear shot, may go without getting
/// closer before it fires anyway.
pub const CLOSING_PATIENCE_TICKS: u32 = 20;
/// In `Combat::closing`, for a unit with no way to walk closer.
pub const NO_CLOSER: u32 = u32::MAX;

impl Combat {
    /// Notes where the unit is, as raw `Fp` bits, while the ground hides what it was ordered to
    /// shoot, and returns whether it is still on its way to a clear shot: it has moved a unit
    /// within the last `CLOSING_PATIENCE_TICKS`, and has not found it can get no closer.
    pub fn still_closing(&mut self, at: [i32; 2], tick: u32) -> bool {
        let one = 1i64 << crate::math::fp::FP_FRAC_BITS;
        let moved = |from: [i32; 2]| {
            let (dx, dy) = (i64::from(at[0] - from[0]), i64::from(at[1] - from[1]));
            dx * dx + dy * dy > one * one
        };
        match self.closing {
            Some((_, NO_CLOSER)) => false,
            Some((from, since)) if !moved(from) => tick < since + CLOSING_PATIENCE_TICKS,
            _ => {
                self.closing = Some((at, tick));
                true
            }
        }
    }
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

/// An air transport: the land units it carries, in the order they boarded, and where it is
/// to set them down, with the route it was given to get there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Transport {
    pub cargo: Vec<SimId>,
    pub unload: Option<(Vec2Fp, u32)>,
}

/// A land unit on its way to board a transport, by the route it was given to follow it. Any
/// other order, which replaces the route, calls it off.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Boarding {
    pub transport: SimId,
    pub route: u32,
}

/// A unit inside a transport, off the map until it is set down. `lists` records which of the
/// world's lists it was in, so that it goes back into the same ones.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Carried {
    pub transport: SimId,
    pub lists: u32,
}

/// A unit just finished in a factory, driving out of its bay: straight to `exit`, through the
/// factory's open end, before it heads for `rally` or follows an order given meanwhile.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LeavingFactory {
    pub exit: Vec2Fp,
    pub rally: Option<Vec2Fp>,
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
    /// The way straight to its route's goal is clear, so it walks straight there rather than
    /// along the route's eight directions and through the middles of its sectors' edges.
    pub direct: bool,
}

impl Order {
    pub const IDLE: Self = Self { kind: OrderKind::Idle, route: 0, slot: None, slot_ticks: 0, direct: false };
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

/// Bonuses from touching neighbors (GAME_SPECIFICATION.md §4), in hundred-thousandths, as in
/// `blueprints::ADJACENCY_SCALE`. Discounts are positive, and never reach the whole cost.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Adjacency {
    pub mass_output_bonus: i32,
    pub energy_output_bonus: i32,
    /// Off what it builds or upgrades to.
    pub energy_cost_discount: i32,
    pub mass_cost_discount: i32,
    /// Off what it needs to keep running.
    pub upkeep_discount: i32,
    /// For artillery: off what each shot costs, and off the time between shots.
    pub weapon_energy_discount: i32,
    pub reload_discount: i32,
}

impl Adjacency {
    /// A value less a discount.
    pub fn less(value: super::blueprints::Res, discount: i32) -> super::blueprints::Res {
        value * (super::blueprints::ADJACENCY_SCALE - i64::from(discount)) / super::blueprints::ADJACENCY_SCALE
    }

    /// A value with a bonus on top.
    pub fn more(value: super::blueprints::Res, bonus: i32) -> super::blueprints::Res {
        value + value * i64::from(bonus) / super::blueprints::ADJACENCY_SCALE
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BuildTask {
    Build { blueprint: BlueprintId, cell_x: u32, cell_y: u32, site: Option<SimId> },
    Assist { target: SimId },
    Reclaim { target: SimId },
    Repair { target: SimId },
    /// Take over an enemy unit (GAME_SPECIFICATION.md §5.3).
    Capture { target: SimId },
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

/// An aircraft (GAME_SPECIFICATION.md §10). In flight it never stops: it flies the way its
/// `Turning` faces, at its own height over land or water, in arcs. With nothing to do it lands,
/// where there is open ground, and circles where there is none. Nothing on the ground blocks it,
/// and it pushes nothing aside.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Flight {
    /// How high above the ground or the water it flies.
    pub elevation: Fp,
    /// How high it is now: `elevation` in flight, and zero once landed.
    pub altitude: Fp,
    /// How fast it is flying now: its top speed in flight, and zero once landed.
    pub speed: Fp,
    /// Ticks of flight left, and on a full tank. With none left, it flies low and slow.
    pub fuel: u32,
    pub max_fuel: u32,
    /// How many of an airfield's slots it takes: one for T1, two for T2, four for T3.
    pub size: u8,
    /// The airfield it is on its way to, or on, to refuel and be repaired.
    pub refit: Option<SimId>,
    /// On the pad, and which of the four pads.
    pub docked: bool,
    pub pad: u8,
    /// Hovers over what it attacks, like a gunship, instead of making passes.
    pub hovers: bool,
    /// Having flown over its target, how far it flies on before it turns back for another pass.
    pub break_off: Fp,
    /// Flying on, away from its target.
    pub breaking_off: bool,
    /// How its heading turned over the last tick, as the cosine and sine of the angle: for
    /// anti-air guns and flak, which aim along a steady turn.
    pub turn: Vec2Fp,
}

impl Flight {
    /// Off the ground. A landed aircraft is hit by what hits ground units, and not by anti-air.
    pub fn airborne(&self) -> bool {
        self.altitude > Fp::ZERO
    }

    pub fn out_of_fuel(&self) -> bool {
        self.fuel == 0
    }

    /// The height it flies at above the ground: its elevation, or half that out of fuel.
    pub fn cruising_height(&self) -> Fp {
        if self.out_of_fuel() { self.elevation / 2 } else { self.elevation }
    }
}

/// A weapon that fires in volleys (`WeaponDef::salvo`): where it is in its volley. A unit's main
/// weapon carries it as a component, an extra weapon in its own `salvo`. Between shots the weapon
/// waits on its usual `ticks_until_ready`, which `fire` sets.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Salvo {
    pub def: super::blueprints::SalvoDef,
    /// Shots fired so far in this volley: 0 between volleys.
    pub fired: u32,
    /// Ticks until the next volley may start, from this one's first shot.
    pub next_volley: u32,
}

impl Salvo {
    pub const fn new(def: super::blueprints::SalvoDef) -> Self {
        Self { def, fired: 0, next_volley: 0 }
    }

    /// A tick passes, before the weapon may fire. A volley whose reload is up is over, even with
    /// shots left in it, as when it had nothing to shoot at for a while: the next shot starts a
    /// new one.
    pub fn tick(&mut self) {
        self.next_volley = self.next_volley.saturating_sub(1);
        if self.next_volley == 0 {
            self.fired = 0;
        }
    }

    /// It fires a shot, with this reload from the start of one volley to the next. Returns the
    /// ticks until it may fire again: to the volley's next shot, or, after its last, to the next
    /// volley.
    pub fn fire(&mut self, reload: u32) -> u32 {
        if self.fired == 0 {
            self.next_volley = reload;
        }
        let at = self.def.offset(self.fired);
        self.fired += 1;
        if self.fired >= self.def.shots {
            self.fired = 0;
            self.next_volley
        } else {
            self.def.offset(self.fired) - at
        }
    }
}

/// A unit's weapons beyond its main one, such as a battleship's other guns or a bomber's anti-air
/// guns. Each picks its own targets, among what it can hit, reloads on its own, and has its own turret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtraWeapons(pub Vec<ExtraWeapon>);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ExtraWeapon {
    pub range: Fp,
    pub reload_ticks: u32,
    pub ticks_until_ready: u32,
    pub projectile_speed: Fp,
    pub damage: Fp,
    pub splash: Fp,
    pub hits_air: bool,
    pub hits_ground: bool,
    /// Flak, which bursts where it was aimed, at aircraft.
    pub flak: bool,
    /// Missiles that steer themselves to aircraft; otherwise its shots fly straight.
    pub homing: bool,
    /// A torpedo: ships and submarines only.
    pub torpedo: bool,
    /// Cruise missiles, which pick their own targets, fly at this height, and need none built.
    pub missile_height: Option<Fp>,
    pub target: Option<SimId>,
    pub turret: Option<Turret>,
    /// How far from where it aims its shots land, as `Scatter`. Fixed from the blueprint.
    pub spread: Fp,
    /// What it hits is stunned this many ticks.
    pub stun_ticks: u32,
    /// Fires in volleys, and where it is in its volley.
    pub salvo: Option<Salvo>,
    /// Fires with the main weapon, as one broadside (`WeaponDef::broadside`).
    pub broadside: bool,
    /// Whether it pointed at what it aims at, as of the last tick: a broadside waits for it.
    pub aimed: bool,
    /// A broadside gun is due to fire once each time the main weapon fires, and only then.
    pub due: bool,
}

impl ExtraWeapon {
    /// Ticks until it may fire again after the shot it just fired.
    pub fn fired(&mut self) -> u32 {
        match self.salvo.as_mut() {
            Some(salvo) => salvo.fire(self.reload_ticks),
            None => self.reload_ticks,
        }
    }
}

/// An artillery piece's weapon (GAME_SPECIFICATION.md §7): it cannot hit what is too close,
/// its shells scatter, and a shot may cost energy, which the team must have when it fires.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Artillery {
    pub min_range: Fp,
    pub spread: Fp,
    pub energy_per_shot: super::blueprints::Res,
}

/// A gun fixed to the nose, a fighter's (`WeaponDef::arc`): it fires only at what is within its
/// arc of straight ahead, the cosine of the arc's half-angle, so the aircraft must face its target.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct NoseGun(pub Fp);

/// Artillery that sets up before it fires (`WeaponDef::setup_ticks`). It is packed up when
/// built, and any movement of its own packs it up again.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    pub ticks: u32,
    /// Ticks still to stand still before it can fire.
    pub remaining: u32,
}

impl Setup {
    pub fn packed(ticks: u32) -> Self {
        Self { ticks, remaining: ticks }
    }

    /// One tick, moving or not: returns whether it is set up and may fire.
    pub fn tick(&mut self, moving: bool) -> bool {
        self.remaining = if moving { self.ticks } else { self.remaining.saturating_sub(1) };
        self.remaining == 0
    }
}

/// A main weapon, other than artillery's, whose shots land up to this far from where they are
/// aimed: the inspiration's FiringRandomness, at 20 units to its one. Artillery keeps its spread in
/// `Artillery`. Fixed from the blueprint, so not hashed.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Scatter(pub Fp);

/// A weapon that can aim at aircraft. A weapon without it only hits what is on the ground.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AntiAir {
    /// Whether it can also hit what is on the ground.
    pub also_ground: bool,
    /// Its shots are flak (`WeaponKind::Flak`), which burst where they were aimed.
    pub flak: bool,
    /// Its shots are missiles that steer themselves to their aircraft. Otherwise they fly
    /// straight to where it will be.
    pub homing: bool,
}

/// A shell at an aircraft that does not steer, an anti-air gun's or flak's, flying straight to
/// where it was aimed as its lifetime runs out: flak bursts there, and a gun's shell hits the
/// aircraft there, if it is.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Fused;

/// A laser's beam (`WeaponKind::Laser`): a shot that crosses to its target in a straight line
/// within the tick it is fired and is spent. `from` is the muzzle it left, for drawing the beam
/// from there to where it struck; it plays no part in the game.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Beam {
    pub from: Vec3Fp,
}

/// A torpedo weapon: it hits ships and submarines, and nothing else.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Torpedo;

/// A bomber's weapon: it drops its bomb as it flies over the target, and the bomb falls.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BombBay;

/// A shot at an aircraft, or a torpedo at a ship or submarine, that steers itself to it, and
/// hits only it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Homing {
    pub target: SimId,
    /// How far it runs this tick. A torpedo leaves at a third of its top speed and gathers speed to
    /// it over two seconds, as the inspiration's do (2026-11-25); a shot at an aircraft flies at its
    /// top speed from the first.
    pub speed: Fp,
    pub top_speed: Fp,
    /// A torpedo, which can only hit what is on or under the water.
    pub underwater: bool,
    /// A decoy an anti-torpedo charge dropped into the water, which a torpedo turns to and strikes
    /// instead of its target, harmlessly (2026-11-25).
    pub lure: Option<Vec3Fp>,
}

/// A falling bomb. Bombs fall at the inspiration's gravity, which is stronger than that of shells.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Falling;

/// An artillery shell in its high arc, falling by this much a tick, a gravity of its own
/// (`combat::lobbed_solution`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Lobbed(pub Fp);

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
    let turn = |a: Fp, b: Fp| mul_toward_zero(a, b);
    Vec2Fp::new(turn(facing.x, cos) - turn(facing.y, sin), turn(facing.x, sin) + turn(facing.y, cos)).normalized()
}

/// A weapon that has to swivel to its target before it can fire.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Turret {
    /// A unit vector.
    pub facing: Vec2Fp,
    /// The most it can turn in a tick, in radians.
    pub max_turn: Fp,
    /// Where it sits on the hull, from the unit's middle, as `[right, forward]`: it aims and
    /// fires from there. Fixed from the blueprint, so not hashed.
    pub mount: [Fp; 2],
    /// It faces the stern while it has nothing to aim at, as a ship's aft guns do.
    pub aft: bool,
}

/// About cos(6 degrees): a turret pointing this close to its target can fire.
pub const AIMED: Fp = Fp::from_bits(4_074);

impl Turret {
    /// From a turn rate in degrees per second.
    pub fn new(facing: Vec2Fp, degrees_per_second: u32) -> Self {
        let max_turn = radians_from_tenths_of_degree(degrees_per_second.min(900));
        Self { facing, max_turn: Fp::from_bits((max_turn.to_bits() >> 12) as i32), mount: [Fp::ZERO; 2], aft: false }
    }

    /// A turret as a blueprint's weapon has it, on a unit facing `facing`: where it sits, and
    /// facing the way it rests.
    pub fn mounted(facing: Vec2Fp, degrees_per_second: u32, mount: [Fp; 2], aft: bool) -> Self {
        let rest = if aft { Vec2Fp::new(-facing.x, -facing.y) } else { facing };
        Self { mount, aft, ..Self::new(rest, degrees_per_second) }
    }

    /// Where it sits on a unit at `(x, y)` facing `front`, as raw `Fp` bits.
    pub fn position(&self, (x, y): (i32, i32), front: Vec2Fp) -> (i32, i32) {
        let [right, forward] = self.mount;
        if right == Fp::ZERO && forward == Fp::ZERO {
            return (x, y);
        }
        // To the right of a unit facing (fx, fy) is (fy, -fx).
        let dx = mul_toward_zero(right, front.y) + mul_toward_zero(forward, front.x);
        let dy = mul_toward_zero(forward, front.y) - mul_toward_zero(right, front.x);
        (x.saturating_add(dx.to_bits()), y.saturating_add(dy.to_bits()))
    }

    /// Swivels back to where it rests on a unit facing `front`, the bow or the stern.
    pub fn stand_down(&mut self, front: Vec2Fp) {
        let rest = if self.aft { Vec2Fp::new(-front.x, -front.y) } else { front };
        self.aim(rest);
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
        self.facing.scaled(ahead)
    }

    /// Whether the goal is inside the circle this unit would turn in on that side, where
    /// driving on would only go round it.
    fn goal_is_inside_turn(&self, toward: Vec2Fp, goal_distance: Fp, left: bool) -> bool {
        let radius = self.radius;
        if goal_distance >= radius * 2 {
            return false;
        }
        let goal = toward.scaled(goal_distance);
        let (side_x, side_y) = if left { (-self.facing.y, self.facing.x) } else { (self.facing.y, -self.facing.x) };
        // Inside the circle of this radius centred to that side: |goal - side * radius| < radius.
        goal.length_squared() < wide_mul(radius * 2, goal.x * side_x + goal.y * side_y)
    }
}

/// The commander's Discharge weapon, ready to fire again after its own reload.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DischargeWeapon {
    pub def: super::blueprints::DischargeDef,
    pub ticks_until_ready: u32,
}

/// An order to Discharge one enemy, as soon as it is in range and there is the energy.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DischargeOrder {
    pub target: SimId,
}

/// Marks a Discharge shell, whose damage and cost are worked out where it lands.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Discharged {
    pub def: super::blueprints::DischargeDef,
}

/// Experience from destroying enemies, and the rank it has earned, from 1 to 5. Units with
/// none have neither component.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Veterancy {
    /// The mass of the enemies this unit has destroyed.
    pub experience: Res,
    pub rank: u8,
}

/// The damage enemies have done to a unit, by who did it, so that the experience for destroying
/// it is shared out as the inspiration does: each by its share of the damage. Units nobody has hit
/// have none.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct VetDamage {
    /// Each attacker and the damage it has done, in the order they first hit.
    pub by: Vec<(SimId, Fp)>,
    pub total: Fp,
}

/// A shield generator's dome: a sphere around the generator's base that stops enemy shells
/// coming in from outside, while shells from inside fly out freely (GAME_SPECIFICATION.md §8.1).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Shield {
    pub radius: Fp,
    /// How high the dome rises at its middle (`ShieldDef::height`).
    pub height: Fp,
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
    /// Nuclear: one missile for each order. Otherwise it fires at its target until told to stop.
    pub strategic: bool,
    pub cruise_height: Fp,
    /// Missiles built and waiting. It fires only these, and only on order.
    pub stock: u32,
    /// A nuclear blast's inner ring. Zero for none.
    pub inner_radius: Fp,
    pub inner_damage: Fp,
    /// Picks its own targets, and needs no missiles built, as a ship's cruise missiles do.
    pub automatic: bool,
}

/// The most tactical missile defense mounts one unit carries.
pub const MAX_DEFENSE_MOUNTS: usize = 4;

/// A defense that destroys enemy missiles in flight.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MissileDefense {
    /// Stops nuclear missiles, one interceptor each. Otherwise stops cruise missiles.
    pub strategic: bool,
    pub range: Fp,
    /// Each mount's.
    pub reload_ticks: u32,
    /// Each mount is a defense of its own, with its own reload: these are the first `mounts`.
    pub mounts: u8,
    pub ticks_until_ready: [u32; MAX_DEFENSE_MOUNTS],
    /// Interceptors built and waiting. Only strategic defenses use it.
    pub stock: u32,
}

/// Anti-torpedo charges, which shoot down enemy torpedoes in range, one each reload.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TorpedoDefense {
    pub range: Fp,
    pub reload_ticks: u32,
    pub ticks_until_ready: u32,
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
    /// A nuclear blast's inner ring, on top of the projectile's own damage and splash. Zero for none.
    pub inner_radius: Fp,
    pub inner_damage: Fp,
}

/// A nuclear missile's flight (2026-11-25): a slow lift out of its silo, then, at one constant
/// speed along its path, straight up to `apex`, a quarter turn to level, level flight there to
/// over its target, a quarter turn down, and straight down onto the ground there. Nothing stops
/// it on the way, shields and units included: only a strategic interceptor.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct NuclearFlight {
    /// Where it starts, inside its silo.
    pub start: Vec3Fp,
    /// The ground it strikes.
    pub target: Vec3Fp,
    /// The height of its level flight.
    pub apex: Fp,
    /// The radius of its two turns.
    pub turn: Fp,
    /// How far it has come along its path.
    pub travelled: Fp,
    /// Its speed along its path, after the lift.
    pub speed: Fp,
}

/// How far a nuclear missile rises out of its silo before it speeds up, and how fast.
pub const NUCLEAR_LIFT_UNITS: i32 = 60;
pub const NUCLEAR_LIFT_SPEED: Fp = Fp::from_bits(3 << crate::math::fp::FP_FRAC_BITS);
/// The radius of a nuclear missile's turns, at most: less over a target closer than two of it.
pub const NUCLEAR_TURN_UNITS: i32 = 300;

/// A right angle, in radians, with 24 fractional bits.
const HALF_PI_BITS: i64 = 26_353_589;

impl NuclearFlight {
    pub fn new(start: Vec3Fp, target: Vec3Fp, apex: Fp, speed: Fp) -> Self {
        let across = (target.xy() - start.xy()).length_squared();
        let distance = crate::math::fp::sqrt_wide(across);
        let lift_top = start.z + crate::math::fp::fp_int(NUCLEAR_LIFT_UNITS);
        // It climbs at least a turn above the top of its lift and the ground it strikes.
        let turn = crate::math::fp::fp_int(NUCLEAR_TURN_UNITS).min(distance / 2);
        let apex = apex.max(lift_top + turn).max(target.z + turn);
        Self { start, target, apex, turn, travelled: Fp::ZERO, speed }
    }

    fn lift_top(&self) -> Fp {
        self.start.z + crate::math::fp::fp_int(NUCLEAR_LIFT_UNITS)
    }

    fn quarter_turn(&self) -> Fp {
        Fp::from_bits(((i64::from(self.turn.to_bits()) * HALF_PI_BITS) >> 24) as i32)
    }

    fn across(&self) -> Fp {
        crate::math::fp::sqrt_wide((self.target.xy() - self.start.xy()).length_squared())
    }

    /// The lengths of its legs: the lift, the climb, the first turn, level flight, the second
    /// turn, and the dive.
    fn legs(&self) -> [Fp; 6] {
        let lift = crate::math::fp::fp_int(NUCLEAR_LIFT_UNITS);
        let climb = (self.apex - self.turn - self.lift_top()).max(Fp::ZERO);
        let arc = self.quarter_turn();
        let level = (self.across() - self.turn * 2).max(Fp::ZERO);
        let dive = (self.apex - self.turn - self.target.z).max(Fp::ZERO);
        [lift, climb, arc, level, arc, dive]
    }

    /// The whole length of its path.
    pub fn length(&self) -> Fp {
        self.legs().iter().fold(Fp::ZERO, |sum, leg| sum + *leg)
    }

    /// How far it goes this tick: slowly while it rises out of its silo, then at its speed.
    pub fn step(&self) -> Fp {
        if self.travelled < crate::math::fp::fp_int(NUCLEAR_LIFT_UNITS) { NUCLEAR_LIFT_SPEED } else { self.speed }
    }

    /// Where it is, this far along its path.
    pub fn position_at(&self, travelled: Fp) -> Vec3Fp {
        let [lift, climb, arc, level, _, _] = self.legs();
        let way = (self.target.xy() - self.start.xy()).normalized();
        let flat = |along: Fp, z: Fp| {
            let at = self.start.xy() + way.scaled(along);
            Vec3Fp::new(at.x, at.y, z)
        };
        let angle = |along: Fp| {
            if self.turn <= Fp::ZERO {
                return crate::math::fp::FpWide::ZERO;
            }
            let bits = (i64::from(along.to_bits()) << 24) / i64::from(self.turn.to_bits());
            crate::math::fp::FpWide::from_bits(bits.clamp(0, HALF_PI_BITS))
        };
        let mut left = travelled.max(Fp::ZERO);
        if left <= lift {
            return Vec3Fp::new(self.start.x, self.start.y, self.start.z + left);
        }
        left -= lift;
        let bottom = self.apex - self.turn;
        if left <= climb {
            return Vec3Fp::new(self.start.x, self.start.y, self.lift_top() + left);
        }
        left -= climb;
        if left <= arc {
            // Round a center a turn out from the climb and a turn below the apex.
            let (cos, sin) = crate::math::fp::cos_sin_radians(angle(left));
            return flat(self.turn - self.turn * cos, bottom + self.turn * sin);
        }
        left -= arc;
        if left <= level {
            return flat(self.turn + left, self.apex);
        }
        left -= level;
        let over = self.across();
        if left <= arc {
            let (cos, sin) = crate::math::fp::cos_sin_radians(angle(left));
            return flat(over - self.turn + self.turn * sin, bottom + self.turn * cos);
        }
        left -= arc;
        Vec3Fp::new(self.target.x, self.target.y, (bottom - left).max(self.target.z))
    }
}

/// An upgrade in place, to the blueprint the unit's own one upgrades to. A commander stands
/// still meanwhile. Factories upgrade through their queue instead.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Upgrading {
    pub into: BlueprintId,
    /// From 0 to 1.
    pub progress: Res,
}

/// What a unit does for intel: its sensors, stealth field, jamming, and cloak, all from its
/// blueprint (GAME_SPECIFICATION.md §9).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Intel {
    /// Built, not paused, and paid for this tick, so that its sensors, field, and jamming work.
    /// Always for a unit with no upkeep, such as the commander's built-in deep scan array.
    pub running: bool,
    /// The player has switched its cloak on.
    pub cloak_on: bool,
    /// The cloak is on and paid for this tick, so it hides the unit.
    pub cloaked: bool,
    /// A personal stealth enhancement is paid for this tick, so the unit is off enemy radar.
    pub stealthed: bool,
    /// A jammer enhancement is paid for this tick, so its decoys show on enemy radar.
    pub jamming: bool,
}

/// How far along an enemy capture of this unit is. Gone if nobody works on it for a tick, so a
/// capture that stops starts over.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BeingCaptured {
    /// From 0 to 1.
    pub progress: Res,
}

/// The field enhancements a unit has, one for each slot (GAME_SPECIFICATION.md §2.1).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct Enhancements(pub [Option<EnhancementId>; 3]);

/// An enhancement under way. The unit stands still and puts all its build power into it,
/// and a new order cancels it, losing what was spent.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Enhancing {
    pub enhancement: EnhancementId,
    /// From 0 to 1.
    pub progress: Res,
}

/// A teleport charging up. It costs energy every tick, aborts if that cannot be paid, and
/// jumps when the charge is done. The unit stands still meanwhile, and a new order cancels it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Teleporting {
    pub destination: Vec2Fp,
    pub ticks_left: u32,
    pub total_ticks: u32,
    pub energy_per_tick: Res,
}

/// Counting down to blowing itself up, leaving no debris.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SelfDestructing {
    pub ticks_left: u32,
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
    /// Another factory this one helps: with nothing of its own to build, it takes the next item
    /// that factory has not started, that it can build itself, and builds it.
    pub assisting: Option<SimId>,
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

/// Debris still on its way down (GAME_SPECIFICATION.md §6.1): an aircraft's, falling with the
/// way it was flying until it strikes the ground or the water, or anything's in the water,
/// sinking to the sea floor. Wrecks at rest have none, and lie half their radius over the ground.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Settling {
    /// The height of its middle, as a wreck at rest is reported half its radius up.
    pub z: Fp,
    /// How far it moves in a tick.
    pub velocity: Vec3Fp,
    /// In the water, sinking, rather than falling through the air. Reclaim reaches it there as
    /// it does on the floor; in the air nothing does.
    pub wet: bool,
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

/// Stunned by an EMP (GAME_SPECIFICATION.md §12): it cannot move or fire, its sensors, cloak,
/// stealth and jamming are off, and its shield dome is down, until the time runs out.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Stunned {
    pub ticks_left: u32,
}

/// Lifted by a tractor beam (GAME_SPECIFICATION.md §12): held helpless, and drawn in to the unit
/// holding it, which crushes it when the time runs out. Let go if that unit dies.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub by: SimId,
    pub ticks_left: u32,
}

/// A tractor beam: it holds one unit at a time, and crushes each after `crush_ticks`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TractorBeam {
    pub crush_ticks: u32,
    pub holding: Option<SimId>,
}

/// A unit whose main weapon stuns what it hits, for this many ticks.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StunWeapon(pub u32);

/// A shot that stuns what it hits, or what its blast reaches, for this many ticks.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Stunning(pub u32);
