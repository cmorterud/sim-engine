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
}
