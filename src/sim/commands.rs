use super::blueprints::BlueprintId;
use super::components::SimId;
use crate::math::fp::Vec2Fp;

/// Player input. Every instance must apply the same commands on the same tick, in the same order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimCommand {
    Move { units: Vec<SimId>, goal: Vec2Fp },
    AttackMove { units: Vec<SimId>, goal: Vec2Fp },
    /// Move towards a unit, re-routing as it travels.
    Follow { units: Vec<SimId>, target: SimId },
    Stop { units: Vec<SimId> },
    /// Close on one enemy and fire only at it.
    Attack { units: Vec<SimId>, target: SimId },
    /// Repeat a route. Combat units attack-move along it; engineers also reclaim and repair.
    Patrol { units: Vec<SimId>, points: Vec<Vec2Fp> },
    /// Build a structure with its corner at a terrain cell. `queued` keeps existing tasks.
    Build { units: Vec<SimId>, blueprint: BlueprintId, cell_x: u32, cell_y: u32, queued: bool },
    /// Add build power to a factory, a construction site, or another builder's work.
    Assist { units: Vec<SimId>, target: SimId },
    Reclaim { units: Vec<SimId>, target: SimId },
    Repair { units: Vec<SimId>, target: SimId },
    /// Add to a factory's queue. An item that is a factory is an upgrade.
    QueueUnit { factory: SimId, blueprint: BlueprintId, count: u32 },
    /// Clear a factory's queue, keeping nothing in progress.
    ClearQueue { factory: SimId },
    SetRepeat { factory: SimId, repeat: bool },
    SetRally { factory: SimId, point: Vec2Fp },
}
