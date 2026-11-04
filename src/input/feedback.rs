//! Shows that a click was received: a brief marker on the map and a line of text saying
//! what was ordered. A click that shows nothing never reached the game.

use crate::sim::blueprints::BlueprintDb;
use crate::sim::commands::SimCommand;
use crate::sim::components::SimId;

/// How long a marker stays on the map, in seconds.
pub const MARKER_SECONDS: f64 = 0.6;
/// How long the text line stays on screen, in seconds.
pub const LABEL_SECONDS: f64 = 1.6;
/// Rapid clicks drop the oldest markers beyond this count.
const MAX_MARKERS: usize = 16;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MarkerKind {
    Move,
    AttackMove,
    /// On a target unit, and follows it.
    Attack,
    /// Assist, repair, or reclaim.
    Support,
    Rally,
    Patrol,
    Build,
    /// The click arrived, but produced no order.
    Rejected,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Marker {
    pub kind: MarkerKind,
    /// World position of the click.
    pub position: [f64; 3],
    /// A unit to follow while it stays visible.
    pub target: Option<SimId>,
    pub age: f64,
}

impl Marker {
    /// From 0 when new to 1 when gone.
    pub fn progress(&self) -> f64 {
        (self.age / MARKER_SECONDS).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub kind: MarkerKind,
    pub text: String,
    pub age: f64,
}

/// The markers and text line currently on screen.
#[derive(Default)]
pub struct OrderFeedback {
    markers: Vec<Marker>,
    label: Option<Label>,
}

fn unit_count(units: &[SimId]) -> String {
    match units.len() {
        1 => "1 unit".to_string(),
        count => format!("{count} units"),
    }
}

/// What a command looks like to the player: its marker, its text, and any unit to follow.
pub fn describe(command: &SimCommand, blueprints: &BlueprintDb) -> Option<(MarkerKind, String, Option<SimId>)> {
    Some(match command {
        SimCommand::Move { units, .. } => (MarkerKind::Move, format!("Move: {}", unit_count(units)), None),
        SimCommand::AttackMove { units, .. } => (MarkerKind::AttackMove, format!("Attack-move: {}", unit_count(units)), None),
        SimCommand::Attack { units, target } => (MarkerKind::Attack, format!("Attack: {}", unit_count(units)), Some(*target)),
        SimCommand::Follow { units, target } => (MarkerKind::Support, format!("Follow: {}", unit_count(units)), Some(*target)),
        SimCommand::Assist { units, target } => (MarkerKind::Support, format!("Assist: {}", unit_count(units)), Some(*target)),
        SimCommand::Repair { units, target } => (MarkerKind::Support, format!("Repair: {}", unit_count(units)), Some(*target)),
        SimCommand::Reclaim { units, .. } => (MarkerKind::Support, format!("Reclaim: {}", unit_count(units)), None),
        SimCommand::SetRally { .. } => (MarkerKind::Rally, "Rally point set".to_string(), None),
        SimCommand::Patrol { units, points } => {
            (MarkerKind::Patrol, format!("Patrol: {} points, {}", points.len(), unit_count(units)), None)
        }
        SimCommand::Build { blueprint, .. } => (MarkerKind::Build, format!("Build: {}", blueprints.get(*blueprint).name), None),
        SimCommand::Stop { .. } | SimCommand::QueueUnit { .. } | SimCommand::ClearQueue { .. } | SimCommand::SetRepeat { .. } => return None,
    })
}

impl OrderFeedback {
    pub fn markers(&self) -> &[Marker] {
        &self.markers
    }

    pub const fn label(&self) -> Option<&Label> {
        self.label.as_ref()
    }

    /// Ages everything by `dt` seconds and removes what has expired.
    pub fn update(&mut self, dt: f64) {
        for marker in &mut self.markers {
            marker.age += dt;
        }
        self.markers.retain(|marker| marker.age < MARKER_SECONDS);
        if let Some(label) = &mut self.label {
            label.age += dt;
            if label.age >= LABEL_SECONDS {
                self.label = None;
            }
        }
    }

    pub fn add_marker(&mut self, kind: MarkerKind, position: [f64; 3], target: Option<SimId>) {
        if self.markers.len() == MAX_MARKERS {
            self.markers.remove(0);
        }
        self.markers.push(Marker { kind, position, target, age: 0.0 });
    }

    pub fn set_label(&mut self, kind: MarkerKind, text: String) {
        self.label = Some(Label { kind, text, age: 0.0 });
    }

    /// Reports the outcome of a click. With no commands, it shows why nothing happened.
    pub fn show_orders(&mut self, commands: &[SimCommand], position: Option<[f64; 3]>, blueprints: &BlueprintDb, nothing_reason: &str) {
        let described: Vec<_> = commands.iter().filter_map(|command| describe(command, blueprints)).collect();
        let Some((first_kind, _, first_target)) = described.first() else {
            self.show_rejected(position, nothing_reason);
            return;
        };
        if let Some(position) = position {
            self.add_marker(*first_kind, position, *first_target);
        }
        let text: Vec<&str> = described.iter().map(|(_, text, _)| text.as_str()).collect();
        self.set_label(*first_kind, text.join(", "));
    }

    /// Reports a click that arrived but could not do anything.
    pub fn show_rejected(&mut self, position: Option<[f64; 3]>, reason: &str) {
        if let Some(position) = position {
            self.add_marker(MarkerKind::Rejected, position, None);
        }
        self.set_label(MarkerKind::Rejected, reason.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::fp::Vec2Fp;

    const AT: [f64; 3] = [10.0, 20.0, 1.0];

    fn db() -> BlueprintDb {
        BlueprintDb::standard()
    }

    fn units(count: u32) -> Vec<SimId> {
        (0..count).map(SimId).collect()
    }

    #[test]
    fn each_order_gets_its_own_marker_and_text() {
        let db = db();
        let goal = Vec2Fp::ZERO;
        let cases = [
            (SimCommand::Move { units: units(12), goal }, MarkerKind::Move, "Move: 12 units"),
            (SimCommand::AttackMove { units: units(1), goal }, MarkerKind::AttackMove, "Attack-move: 1 unit"),
            (SimCommand::Attack { units: units(3), target: SimId(9) }, MarkerKind::Attack, "Attack: 3 units"),
            (SimCommand::Assist { units: units(2), target: SimId(9) }, MarkerKind::Support, "Assist: 2 units"),
            (SimCommand::Reclaim { units: units(2), target: SimId(9) }, MarkerKind::Support, "Reclaim: 2 units"),
            (SimCommand::SetRally { factory: SimId(1), point: goal }, MarkerKind::Rally, "Rally point set"),
        ];
        for (command, kind, text) in cases {
            let mut feedback = OrderFeedback::default();
            feedback.show_orders(&[command], Some(AT), &db, "unused");
            assert_eq!(feedback.markers()[0].kind, kind);
            assert_eq!(feedback.label().unwrap().text, text);
        }
    }

    #[test]
    fn attack_markers_follow_their_target() {
        let db = db();
        let mut feedback = OrderFeedback::default();
        feedback.show_orders(&[SimCommand::Attack { units: units(1), target: SimId(9) }], Some(AT), &db, "unused");
        assert_eq!(feedback.markers()[0].target, Some(SimId(9)));
    }

    #[test]
    fn a_click_with_no_orders_says_why() {
        let db = db();
        let mut feedback = OrderFeedback::default();
        feedback.show_orders(&[], Some(AT), &db, "No units selected");
        assert_eq!(feedback.markers()[0].kind, MarkerKind::Rejected);
        assert_eq!(feedback.label().unwrap().text, "No units selected");
    }

    #[test]
    fn mixed_orders_list_everything_that_happened() {
        let db = db();
        let goal = Vec2Fp::ZERO;
        let mut feedback = OrderFeedback::default();
        let commands = [SimCommand::Move { units: units(2), goal }, SimCommand::SetRally { factory: SimId(5), point: goal }];
        feedback.show_orders(&commands, Some(AT), &db, "unused");
        assert_eq!(feedback.label().unwrap().text, "Move: 2 units, Rally point set");
    }

    #[test]
    fn markers_and_text_expire_on_their_own_schedules() {
        let db = db();
        let mut feedback = OrderFeedback::default();
        feedback.show_orders(&[SimCommand::Move { units: units(1), goal: Vec2Fp::ZERO }], Some(AT), &db, "unused");
        feedback.update(MARKER_SECONDS * 0.5);
        assert!((feedback.markers()[0].progress() - 0.5).abs() < 1e-9);
        feedback.update(MARKER_SECONDS);
        assert!(feedback.markers().is_empty(), "the marker should be gone");
        assert!(feedback.label().is_some(), "the text lasts longer than the marker");
        feedback.update(LABEL_SECONDS);
        assert!(feedback.label().is_none());
    }

    #[test]
    fn rapid_clicks_cannot_grow_without_limit() {
        let mut feedback = OrderFeedback::default();
        for _ in 0..100 {
            feedback.add_marker(MarkerKind::Move, AT, None);
        }
        assert_eq!(feedback.markers().len(), MAX_MARKERS);
    }
}
