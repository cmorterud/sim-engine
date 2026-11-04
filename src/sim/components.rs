use crate::math::fp::{Fp, Vec3Fp};

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
    pub target: Option<SimId>,
}

#[derive(Copy, Clone, Debug)]
pub struct Projectile {
    pub velocity: Vec3Fp,
    pub damage: Fp,
    pub source: SimId,
    pub source_team: Team,
    pub lifetime_ticks: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OrderKind {
    Idle,
    /// Keeps moving while firing at anything in range.
    Move,
    /// Stops to fire when a target is in range.
    AttackMove,
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
