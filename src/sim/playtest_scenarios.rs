//! Playtest scenarios (README, Playtest scenarios): the balance harness's fights, to play by
//! hand, from `data/scenarios.toml`. The game builds each from that file, and the page that lists
//! them (`crate::scenario_page`) is written from it too, so the page shows what the game plays.

use serde::Deserialize;

use crate::math::fp::{fp_int, Vec2Fp};

use super::blueprints::{BlueprintDb, Res};
use super::commands::SimCommand;
use super::components::{SimId, Team};
use super::config::{SimConfig, TerrainStyle};
use super::grounds::Ground;
use super::scenario::Scenario;
use super::world::SimWorld;

/// The scenarios, as the game is built with them.
const SOURCE: &str = include_str!("../../data/scenarios.toml");
/// The ground's size, the duel ground's.
pub const MAP_UNITS: i32 = 5_120;
/// Both sides' energy and its storage, for shields and artillery shots: the hungriest shield's
/// upkeep for most of an hour, and few enough digits to fit the resource bar.
const ENERGY: i64 = 5_000_000;
/// Both sides' materiel and its storage.
const MATERIEL: i64 = 1_000;
/// A block's width, when the file does not give one.
const COLUMNS: u32 = 5;
/// A block's units are this many radii apart.
const SPACING_RADII: i32 = 3;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaytestScenario {
    pub id: String,
    pub title: String,
    /// What the scenario shows.
    pub shows: String,
    /// What to try in it.
    #[serde(rename = "try")]
    pub to_try: String,
    /// Where it is fought: flat land unless it says (`land`, `water`, or `shore`), the balance
    /// harness's grounds (`crate::sim::grounds`).
    #[serde(default)]
    pub ground: Ground,
    /// The tier whose prices weigh each side, as the harness weighs a fight at its tier; the
    /// highest tier among its units unless it says.
    pub tier: Option<u8>,
    /// The balance target it plays, a dotted path into data/balance_targets.toml, which the page
    /// shows with its value.
    pub target: Option<String>,
    /// Blue's cost as a multiple of Red's, where the target asks for more than an equal cost: 1
    /// unless it says. A test holds each scenario's sides to it.
    pub blue_over_red: Option<f64>,
    /// The player's groups.
    pub blue: Vec<Group>,
    pub red: Vec<Group>,
}

/// A block of one unit, or one structure.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub unit: String,
    #[serde(default = "one")]
    pub count: u32,
    /// For units: where the block's first row is centred, in world units.
    pub at: Option<[i32; 2]>,
    /// For a structure: the cell of its south-west corner.
    pub cell: Option<[u32; 2]>,
    pub columns: Option<u32>,
    /// Red's only: "attack" attack-moves at the player's first group; "hold", the default,
    /// stays put.
    pub order: Option<String>,
}

const fn one() -> u32 {
    1
}

#[derive(Deserialize)]
struct File {
    scenario: Vec<PlaytestScenario>,
}

/// Every scenario, in the file's order. Panics if the file is malformed, which a test checks.
pub fn all() -> Vec<PlaytestScenario> {
    toml::from_str::<File>(SOURCE).expect("data/scenarios.toml parses").scenario
}

pub fn find(id: &str) -> Option<PlaytestScenario> {
    all().into_iter().find(|scenario| scenario.id == id)
}

impl Group {
    pub fn attacks(&self) -> bool {
        self.order.as_deref() == Some("attack")
    }
}

impl PlaytestScenario {
    /// The tier whose prices weigh its sides.
    pub fn tier(&self, db: &BlueprintDb) -> u8 {
        self.tier.unwrap_or_else(|| self.blue.iter().chain(&self.red).filter_map(|group| db.find(&group.unit)).map(|id| db.get(id).tier).max().unwrap_or(1))
    }

    /// What is wrong with it against the unit data, if anything.
    pub fn problems(&self, db: &BlueprintDb) -> Vec<String> {
        let mut problems = Vec::new();
        if self.blue.is_empty() || self.red.is_empty() {
            problems.push(format!("{}: each side needs something", self.id));
        }
        if !matches!(self.tier, None | Some(1..=3)) {
            problems.push(format!("{}: tier is 1, 2, or 3", self.id));
        }
        if self.blue_over_red.is_some_and(|ratio| ratio <= 0.0) {
            problems.push(format!("{}: blue_over_red is a multiple above 0", self.id));
        }
        let first_at = self.blue.first().and_then(|group| group.at);
        if first_at.is_none() {
            problems.push(format!("{}: Blue's first group must be units, for Red to attack", self.id));
        }
        for (side, groups) in [("blue", &self.blue), ("red", &self.red)] {
            for group in groups {
                let Some(id) = db.find(&group.unit) else {
                    problems.push(format!("{}: no unit {}", self.id, group.unit));
                    continue;
                };
                let structure = db.get(id).is_structure();
                let what = format!("{}: {} {}", self.id, side, group.unit);
                match (structure, group.at, group.cell) {
                    (true, None, Some(_)) if group.count == 1 => {}
                    (true, ..) => problems.push(format!("{what}: a structure takes one cell and no position or count")),
                    (false, Some(_), None) if group.count > 0 => {}
                    (false, ..) => problems.push(format!("{what}: units take a position and a count")),
                }
                // Every unit of a block on ground it can stand on: ships at sea, land units ashore.
                if let (false, Some(_)) = (structure, group.at) {
                    let team = if side == "blue" { Team::Blue } else { Team::Red };
                    let moves = db.get(id).move_class();
                    if block_points(db, group, team).iter().any(|&(x, _)| !self.ground.holds(MAP_UNITS, x, moves)) {
                        problems.push(format!("{what}: some of the block stands where it cannot, on {}", self.ground.name()));
                    }
                }
                match group.order.as_deref() {
                    None | Some("hold") => {}
                    Some("attack") if side == "red" && !structure => {}
                    Some(other) => problems.push(format!("{what}: order {other:?} is for Red's units only, and is attack or hold")),
                }
            }
        }
        problems
    }

    /// The scenario's world, with Red's opening orders, and fog of war as asked.
    pub fn build(&self, seed: u64, fog: bool) -> Scenario {
        let config = SimConfig { map_units: MAP_UNITS, terrain: TerrainStyle::Flat, fog_of_war: fog, ..SimConfig::default() };
        let mut world = self.ground.world(config, seed);
        let mut red_attackers = Vec::new();
        let mut red_start = None;
        for (team, groups) in [(Team::Red, &self.red), (Team::Blue, &self.blue)] {
            for group in groups {
                let blueprint = world.blueprints.id(&group.unit);
                if let Some([cell_x, cell_y]) = group.cell {
                    let id = world.spawn_finished_structure(blueprint, team, cell_x, cell_y).expect("a scenario's structure fits");
                    red_start = red_start.or_else(|| world.unit_position(id).map(|at| at.xy()));
                    continue;
                }
                let ids = spawn_block(&mut world, group, team);
                if team == Team::Red {
                    red_start = red_start.or_else(|| group.at.map(|[x, y]| point(x, y)));
                    if group.attacks() {
                        red_attackers.extend(ids);
                    }
                }
            }
        }
        let blue_start = self.blue.first().and_then(|group| group.at).map_or(point(MAP_UNITS / 4, MAP_UNITS / 2), |[x, y]| point(x, y));
        world.starts = [blue_start, red_start.unwrap_or(point(MAP_UNITS * 3 / 4, MAP_UNITS / 2))];
        world.start_economy(0, 0);
        for economy in &mut world.economies {
            for value in [&mut economy.base_energy_capacity, &mut economy.energy_capacity, &mut economy.energy] {
                *value = Res::from_num(ENERGY);
            }
            // A little materiel, though there is nothing to build: with none stored, the economy
            // counts as stalled, and the resource bar says so.
            for value in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.mass] {
                *value = Res::from_num(MATERIEL);
            }
        }
        world.prepare_map();
        let opening_commands = if red_attackers.is_empty() { Vec::new() } else { vec![SimCommand::AttackMove { units: red_attackers, goal: blue_start }] };
        Scenario { world, opening_commands }
    }
}

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

/// Where a block of a group's units stands, its first row at `at`, the rest behind it: west for
/// Blue, east for Red.
fn block_points(db: &BlueprintDb, group: &Group, team: Team) -> Vec<(i32, i32)> {
    let [x, y] = group.at.expect("units have a position");
    let radius = db.get(db.id(&group.unit)).radius.to_num::<i32>().max(4);
    let spacing = radius * SPACING_RADII;
    let columns = group.columns.unwrap_or(COLUMNS).clamp(1, group.count.max(1)) as i32;
    let back = if team == Team::Blue { -1 } else { 1 };
    (0..group.count as i32)
        .map(|index| {
            let (row, column) = (index / columns, index % columns);
            (x + back * row * spacing, y + (column * 2 - (columns - 1)) * spacing / 2)
        })
        .collect()
}

fn spawn_block(world: &mut SimWorld, group: &Group, team: Team) -> Vec<SimId> {
    let blueprint = world.blueprints.id(&group.unit);
    block_points(&world.blueprints, group, team).into_iter().map(|(x, y)| world.spawn_mobile(blueprint, team, point(x, y))).collect()
}
