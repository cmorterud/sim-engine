use super::blueprints::{BlueprintId, EnhancementId};
use super::components::{BuildTask, SimId};
use crate::math::fp::Vec2Fp;

/// Player input. Every instance must apply the same commands on the same tick, in the same order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimCommand {
    Move { units: Vec<SimId>, goal: Vec2Fp },
    AttackMove { units: Vec<SimId>, goal: Vec2Fp },
    /// Move towards a unit, re-routing as it travels.
    Follow { units: Vec<SimId>, target: SimId },
    Stop { units: Vec<SimId> },
    /// Fire the commander's Overcharge at one enemy, closing in first if need be.
    Overcharge { unit: SimId, target: SimId },
    /// Close on one enemy and fire only at it.
    Attack { units: Vec<SimId>, target: SimId },
    /// Fire at a spot on the ground, walking into range first, until given another order.
    /// Shells that land on debris destroy it, and the mass in it with it.
    AttackGround { units: Vec<SimId>, point: Vec2Fp },
    /// Repeat a route. Combat units attack-move along it; engineers also reclaim and repair.
    /// Queued, it begins once what is already ordered is done, instead of replacing it.
    Patrol { units: Vec<SimId>, points: Vec<Vec2Fp>, queued: bool },
    /// Go there after finishing what is already ordered. A group shares one route, and ends
    /// up in a formation there.
    QueueMove { units: Vec<SimId>, goal: Vec2Fp, attack: bool },
    /// Travel together at the pace of the slowest, keeping a formation all the way there.
    /// `attack` stops the whole group to fight, and `queued` goes after what is already ordered.
    FormationMove { units: Vec<SimId>, goal: Vec2Fp, attack: bool, queued: bool },
    /// Add a task after what is already ordered, for those that are builders.
    QueueTask { units: Vec<SimId>, task: BuildTask },
    /// Send a move that has not finished somewhere else, for everyone sharing it.
    MoveOrder { units: Vec<SimId>, task: BuildTask, goal: Vec2Fp },
    /// Put a build that has not started somewhere else, for everyone sharing it.
    MoveBuild { units: Vec<SimId>, task: BuildTask, cell_x: u32, cell_y: u32 },
    /// Build a structure with its corner at a terrain cell. `queued` keeps existing tasks.
    Build { units: Vec<SimId>, blueprint: BlueprintId, cell_x: u32, cell_y: u32, queued: bool },
    /// Add build power to a factory, a construction site, or another builder's work.
    Assist { units: Vec<SimId>, target: SimId },
    Reclaim { units: Vec<SimId>, target: SimId },
    Repair { units: Vec<SimId>, target: SimId },
    /// Builders take over an enemy unit (GAME_SPECIFICATION.md §5.3).
    Capture { units: Vec<SimId>, target: SimId },
    /// Add to a factory's queue. An item that is a factory is an upgrade. With an upgrade in
    /// its queue, a factory also takes what the upgrade will let it build, which waits behind it.
    QueueUnit { factory: SimId, blueprint: BlueprintId, count: u32 },
    /// Upgrade in place: a factory adds the upgrade to its queue, and a commander or structure
    /// starts upgrading, the commander standing still until it is done. A new order for a
    /// commander cancels its upgrade, losing what was spent on it.
    Upgrade { units: Vec<SimId> },
    /// Clear a factory's queue, keeping nothing in progress.
    ClearQueue { factory: SimId },
    /// Remove one order from builders' queues, wherever it has got to by now. The order is
    /// named rather than numbered, because the queue may have moved on since it was shown.
    /// A site that was already started stays where it is.
    CancelTask { units: Vec<SimId>, task: BuildTask },
    /// Remove one of an item from a factory's queue: the last, so that work under way is
    /// only lost if it is the only one. Removing an upgrade also removes what depended on it.
    RemoveQueued { factory: SimId, blueprint: BlueprintId },
    SetRepeat { factory: SimId, repeat: bool },
    /// Switch cloaks on or off, for the units that have one.
    SetCloak { units: Vec<SimId>, on: bool },
    /// Bring submarines up onto the water, where they can fire their deck guns and be fired on
    /// like ships, or take them back down, where only torpedoes reach them.
    SetSurfaced { units: Vec<SimId>, surfaced: bool },
    /// Start a field enhancement on each of these that can take it (GAME_SPECIFICATION.md §2.1).
    /// One in the same slot that it does not build on is removed first.
    Enhance { units: Vec<SimId>, enhancement: EnhancementId },
    /// Board a transport: the land units walk to it as it comes to them, and get in as they
    /// reach it, while there is room.
    Load { units: Vec<SimId>, transport: SimId },
    /// Fly to a spot and set down everything on board around it.
    Unload { transports: Vec<SimId>, point: Vec2Fp },
    /// Charge a teleport to a spot the team can sense, for each of these with a teleporter.
    Teleport { units: Vec<SimId>, point: Vec2Fp },
    /// Start counting down to blowing up, or stop counting for those that already are.
    SelfDestruct { units: Vec<SimId> },
    /// Pause or resume factories and construction sites. Anything else is left alone.
    SetPaused { units: Vec<SimId>, paused: bool },
    SetRally { factory: SimId, point: Vec2Fp },
    /// Send aircraft to land on one of their team's air staging facilities, to refuel and be
    /// repaired (GAME_SPECIFICATION.md §10).
    Refit { units: Vec<SimId>, staging: SimId },
}
