use super::blueprints::{BlueprintId, Res};
use crate::math::fp::{Fp, Vec2Fp, Vec3Fp};

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
}

impl Order {
    pub const IDLE: Self = Self { kind: OrderKind::Idle, route: 0 };
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
}

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

/// Battlefield debris that engineers can reclaim for mass.
#[derive(Copy, Clone, Debug)]
pub struct Wreck {
    pub mass: Res,
    pub position: Vec2Fp,
    pub radius: Fp,
}
