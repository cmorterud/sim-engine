//! The scripted opponent (GAME_SPECIFICATION.md §17). It plays by the player's rules: it
//! acts only through `SimCommand`, and sees enemies only through its team's fog of war.
//! It lives outside `SimWorld`, so a match can be replayed from its command log alone.
#![deny(clippy::float_arithmetic)]

use crate::math::fp::{fp_int, wide_mul, Fp, FpWide, Vec2Fp};
use crate::math::rng::SimRng;
use crate::sim::blueprints::{BlueprintId, Res, UnitClass};
use crate::sim::commands::SimCommand;
use crate::sim::components::{Blueprinted, BuildTask, Builder, Construction, Factory, Order, OrderKind, Position, SimId, Team, UnitStats};
use crate::sim::config::TERRAIN_CELL_UNITS;
use crate::sim::scenario::skirmish_start;
use crate::sim::world::{MatchResult, SimWorld};

/// The AI decides once per second. Both teams decide on the same tick, from the same world:
/// a team that always decided later would always be that far behind.
const THINK_INTERVAL: u32 = 10;
const SEARCH_RINGS: i32 = 60;
const COMMANDER_LEASH_UNITS: i32 = 900;
const DEFEND_RADIUS_UNITS: i32 = 1_400;
/// After 35 minutes, the commander goes with full-size waves.
const COMMANDER_PUSH_TICKS: u32 = 21_000;
/// Enemy deposits farther than this from the enemy's start are raided rather than its base.
const RAID_DISTANCE_UNITS: i32 = 2_000;
const FIRST_WAVE: usize = 10;
const LARGEST_WAVE: usize = 50;
/// An army that has not grown for this long attacks with what it has, rather than wait
/// for a wave its economy can no longer field.
const WAVE_PATIENCE_TICKS: u32 = 600;
const MAX_ENGINEERS: usize = 6;
const MAX_FACTORIES: usize = 4;
const MAX_POINT_DEFENSES: usize = 4;
/// How long before trying a deposit again, in case a hidden enemy holds it.
const DEPOSIT_RETRY_TICKS: u32 = 1_500;

struct Roster {
    engineer: BlueprintId,
    bot: BlueprintId,
    artillery: BlueprintId,
    tank: BlueprintId,
    factory_t1: BlueprintId,
    factory_t2: BlueprintId,
    extractor: BlueprintId,
    generator: BlueprintId,
    storage: BlueprintId,
    point_defense: BlueprintId,
}

impl Roster {
    fn new(world: &SimWorld) -> Self {
        let id = |key: &str| world.blueprints.id(key);
        Self {
            engineer: id("t1_engineer"),
            bot: id("t1_light_bot"),
            artillery: id("t1_light_artillery"),
            tank: id("t2_heavy_tank"),
            factory_t1: id("land_factory_t1"),
            factory_t2: id("land_factory_t2"),
            extractor: id("mass_extractor"),
            generator: id("power_generator"),
            storage: id("mass_storage"),
            point_defense: id("t1_point_defense"),
        }
    }
}

/// What the AI's own forces look like this turn.
#[derive(Default)]
struct Forces {
    commander: Option<SimId>,
    idle_builders: Vec<(SimId, Vec2Fp)>,
    engineers: usize,
    factories: Vec<FactoryInfo>,
    idle_combat: Vec<SimId>,
    combat_units: usize,
    extractors: Vec<(u32, u32)>,
    generators: usize,
    generators_building: usize,
    storages: usize,
    point_defenses: usize,
    /// Cells that are spoken for: occupied, or planned by a builder on its way.
    planned: Vec<(u32, u32)>,
    /// What builders are already on their way to start.
    planned_blueprints: Vec<BlueprintId>,
}

struct FactoryInfo {
    id: SimId,
    blueprint: BlueprintId,
    cell: (u32, u32),
    finished: bool,
    queue_is_empty: bool,
    upgrading: bool,
}

/// A seen enemy, from the team's own sight.
#[derive(Copy, Clone)]
struct Sighting {
    id: SimId,
    position: Vec2Fp,
    is_structure: bool,
}

pub struct AiBrain {
    team: Team,
    home: Vec2Fp,
    enemy_home: Vec2Fp,
    /// +1 for Blue, -1 for Red, whose plan is Blue's turned about the map's center.
    side: i32,
    rng: SimRng,
    wave_size: usize,
    /// The most idle combat units seen since the last wave, and when that was first reached.
    gathered: (usize, u32),
    deposit_attempts: Vec<((u32, u32), u32)>,
    roster: Option<Roster>,
    rally_set_on: Vec<SimId>,
    unit_counter: u32,
}

impl AiBrain {
    pub fn new(team: Team, seed: u64) -> Self {
        Self {
            team,
            home: skirmish_start(team),
            enemy_home: skirmish_start(team.enemy()),
            side: if team == Team::Blue { 1 } else { -1 },
            // Both teams draw the same numbers, so that on a mirrored map they make mirrored choices.
            rng: SimRng::new(seed ^ 0xA5A5_5A5A),
            wave_size: FIRST_WAVE,
            gathered: (0, 0),
            deposit_attempts: Vec::new(),
            roster: None,
            rally_set_on: Vec::new(),
            unit_counter: 0,
        }
    }

    pub const fn team(&self) -> Team {
        self.team
    }

    /// Decides this tick's orders. Returns nothing on ticks when it is not this team's turn.
    pub fn think(&mut self, world: &SimWorld) -> Vec<SimCommand> {
        if !world.current_tick.is_multiple_of(THINK_INTERVAL) || world.match_result.is_some() {
            return Vec::new();
        }
        if self.roster.is_none() {
            self.roster = Some(Roster::new(world));
        }
        let forces = self.survey(world);
        let sightings = visible_enemies(world, self.team);
        let mut commands = Vec::new();
        self.command_builders(world, &forces, &mut commands);
        self.command_factories(world, &forces, &mut commands);
        self.command_army(world, &forces, &sightings, &mut commands);
        commands
    }

    fn roster(&self) -> &Roster {
        self.roster.as_ref().expect("roster is set before use")
    }

    fn survey(&self, world: &SimWorld) -> Forces {
        let roster = self.roster();
        let mut forces = Forces::default();
        for &(id, entity) in &world.units {
            let stats = world.ecs.get::<&UnitStats>(entity).expect("units have stats");
            if stats.team != self.team {
                continue;
            }
            let Ok(blueprint) = world.ecs.get::<&Blueprinted>(entity).map(|unit| unit.0) else {
                continue;
            };
            let position = world.ecs.get::<&Position>(entity).expect("units have positions").0.xy();
            let order = *world.ecs.get::<&Order>(entity).expect("units have orders");
            let finished = world.ecs.get::<&Construction>(entity).is_err();
            let unit = world.blueprints.get(blueprint);
            let cell = cell_of(position);
            match unit.class {
                UnitClass::Commander | UnitClass::Engineer => {
                    if unit.class == UnitClass::Commander {
                        forces.commander = Some(id);
                    } else {
                        forces.engineers += 1;
                    }
                    let builder = world.ecs.get::<&Builder>(entity).expect("builders have builder parts");
                    if builder.tasks.is_empty() && order.kind == OrderKind::Idle {
                        forces.idle_builders.push((id, position));
                    }
                    for task in &builder.tasks {
                        if let BuildTask::Build { blueprint, cell_x, cell_y, site: None } = *task {
                            forces.planned.push((cell_x, cell_y));
                            forces.planned_blueprints.push(blueprint);
                        }
                    }
                }
                UnitClass::Bot | UnitClass::Artillery | UnitClass::Tank => {
                    forces.combat_units += 1;
                    if order.kind == OrderKind::Idle {
                        forces.idle_combat.push(id);
                    }
                }
                // Only land factories: the AI builds no missile silos, and must never queue in one.
                UnitClass::Factory if blueprint == roster.factory_t1 || blueprint == roster.factory_t2 => {
                    let factory = world.ecs.get::<&Factory>(entity).expect("factories have factory parts");
                    let half = i32::from(unit.footprint) / 2;
                    forces.factories.push(FactoryInfo {
                        id,
                        blueprint,
                        cell: ((cell.0 as i32 - half) as u32, (cell.1 as i32 - half) as u32),
                        finished,
                        queue_is_empty: factory.queue.is_empty(),
                        upgrading: factory.queue.first().is_some_and(|item| world.blueprints.get(*item).is_factory()),
                    });
                }
                // The AI builds neither silos' missiles nor support commanders yet.
                UnitClass::Factory | UnitClass::Ammo | UnitClass::SupportCommander => {}
                UnitClass::Economy | UnitClass::Defense => {
                    if blueprint == roster.extractor {
                        forces.extractors.push(cell);
                    } else if blueprint == roster.generator {
                        forces.generators += 1;
                        forces.generators_building += usize::from(!finished);
                    } else if blueprint == roster.storage {
                        forces.storages += 1;
                    } else if blueprint == roster.point_defense {
                        forces.point_defenses += 1;
                    }
                }
            }
        }
        forces
    }

    fn command_builders(&mut self, world: &SimWorld, forces: &Forces, commands: &mut Vec<SimCommand>) {
        let mut planned = forces.planned.clone();
        let mut new_blueprints = forces.planned_blueprints.clone();
        for &(id, position) in &forces.idle_builders {
            let is_commander = forces.commander == Some(id);
            let plan = self.next_structure(world, forces, position, is_commander, &planned, &new_blueprints);
            if let Some((blueprint, cell)) = plan {
                planned.push(cell);
                new_blueprints.push(blueprint);
                if blueprint == self.roster().extractor {
                    self.deposit_attempts.push((cell, world.current_tick));
                }
                commands.push(SimCommand::Build {
                    units: vec![id],
                    blueprint,
                    cell_x: cell.0,
                    cell_y: cell.1,
                    queued: false,
                });
                continue;
            }
            if is_commander {
                continue;
            }
            if let Some(wreck) = nearest_visible_wreck(world, self.team, position, self.home, self.enemy_home) {
                commands.push(SimCommand::Reclaim { units: vec![id], target: wreck });
            } else if let Some(factory) = forces.factories.iter().find(|factory| factory.finished && !factory.queue_is_empty) {
                commands.push(SimCommand::Assist { units: vec![id], target: factory.id });
            }
        }
    }

    /// Chooses what an idle builder should start next, in priority order.
    fn next_structure(
        &self,
        world: &SimWorld,
        forces: &Forces,
        position: Vec2Fp,
        is_commander: bool,
        planned: &[(u32, u32)],
        planned_blueprints: &[BlueprintId],
    ) -> Option<(BlueprintId, (u32, u32))> {
        let roster = self.roster();
        let economy = &world.economies[self.team.index()];
        let per_second = |value: Res| (value * 10).to_num::<i64>();
        let (mass_income, energy_income) = (per_second(economy.mass_income), per_second(economy.energy_income));
        let count_planned = |blueprint: BlueprintId| planned_blueprints.iter().filter(|item| **item == blueprint).count();

        let factories_total = forces.factories.len() + count_planned(roster.factory_t1);
        if factories_total == 0 {
            return self.site_near_home(world, roster.factory_t1, 1, planned).map(|cell| (roster.factory_t1, cell));
        }

        // Mass limits everything early on, so extractors come before all else unless
        // energy has actually run short. Generators then keep pace with mass income.
        let energy_is_low = economy.energy < economy.energy_capacity * 2 / 5 && economy.energy_demand > economy.energy_income;
        let generators_coming = forces.generators_building + count_planned(roster.generator);
        let energy_lags = energy_income < mass_income * 12;
        if energy_is_low && generators_coming < 3 {
            if let Some(cell) = self.generator_site(world, forces, planned) {
                return Some((roster.generator, cell));
            }
        }
        if let Some(cell) = self.nearest_deposit(world, forces, position, is_commander, planned) {
            return Some((roster.extractor, cell));
        }
        if energy_lags && generators_coming < 2 {
            if let Some(cell) = self.generator_site(world, forces, planned) {
                return Some((roster.generator, cell));
            }
        }

        let factories_wanted = (1 + mass_income as usize / 10).min(MAX_FACTORIES);
        if factories_total < factories_wanted && economy.mass > Res::from_num(150) {
            if let Some(cell) = self.site_near_home(world, roster.factory_t1, 1, planned) {
                return Some((roster.factory_t1, cell));
            }
        }

        if economy.mass > economy.mass_capacity * 7 / 10 {
            if forces.storages < forces.extractors.len() * 2 {
                if let Some(cell) = self.storage_site(world, forces, planned) {
                    return Some((roster.storage, cell));
                }
            }
            if forces.point_defenses + count_planned(roster.point_defense) < MAX_POINT_DEFENSES {
                if let Some(cell) = self.site_towards_enemy(world, roster.point_defense, planned) {
                    return Some((roster.point_defense, cell));
                }
            }
        }
        None
    }

    fn nearest_deposit(&self, world: &SimWorld, forces: &Forces, position: Vec2Fp, is_commander: bool, planned: &[(u32, u32)]) -> Option<(u32, u32)> {
        let leash = fp_int(COMMANDER_LEASH_UNITS);
        let mut best: Option<(FpWide, (u32, u32))> = None;
        for &cell in &world.deposits {
            let center = cell_center(cell);
            // Stay on our side of the map, where an engineer has a chance of surviving.
            if center.distance_squared(&self.home) > center.distance_squared(&self.enemy_home) {
                continue;
            }
            if is_commander && center.distance_squared(&self.home) > wide_mul(leash, leash) {
                continue;
            }
            if forces.extractors.contains(&cell) || planned.contains(&cell) {
                continue;
            }
            let recently_tried = self.deposit_attempts.iter().any(|&(tried, tick)| {
                tried == cell && world.current_tick.saturating_sub(tick) < DEPOSIT_RETRY_TICKS
            });
            if recently_tried {
                continue;
            }
            let distance = center.distance_squared(&position);
            if best.is_none_or(|(closest, _)| distance < closest) {
                best = Some((distance, cell));
            }
        }
        best.map(|(_, cell)| cell)
    }

    /// Generators go along a factory's east and west sides, for the discount. The
    /// north and south sides stay clear, because that is where units come out.
    fn generator_site(&self, world: &SimWorld, forces: &Forces, planned: &[(u32, u32)]) -> Option<(u32, u32)> {
        let generator = self.roster().generator;
        let size = footprint(world, generator);
        for factory in forces.factories.iter().filter(|factory| factory.finished) {
            let (x, y) = self.flip_site(world, cell_i32(factory.cell), footprint(world, factory.blueprint));
            for offset in 0..3 {
                for cell_x in [x - 1, x + 3] {
                    let Some(cell) = self.world_site(world, (cell_x, y + offset), size) else {
                        continue;
                    };
                    if world.can_place(generator, cell.0, cell.1) && !planned.contains(&cell) {
                        return Some(cell);
                    }
                }
            }
        }
        self.site_near_home(world, generator, 1, planned)
    }

    /// Storage goes on an extractor's edges, for its output bonus.
    fn storage_site(&self, world: &SimWorld, forces: &Forces, planned: &[(u32, u32)]) -> Option<(u32, u32)> {
        let storage = self.roster().storage;
        let size = footprint(world, storage);
        for &extractor in &forces.extractors {
            let (x, y) = self.flip_site(world, cell_i32(extractor), 1);
            for site in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                let Some(cell) = self.world_site(world, site, size) else {
                    continue;
                };
                if world.can_place(storage, cell.0, cell.1) && !planned.contains(&cell) {
                    return Some(cell);
                }
            }
        }
        None
    }

    fn site_near_home(&self, world: &SimWorld, blueprint: BlueprintId, gap: i32, planned: &[(u32, u32)]) -> Option<(u32, u32)> {
        let home = self.plan_position(world, self.home);
        self.search_site(world, blueprint, gap, cell_i32(cell_of(home)), planned)
    }

    /// On the plan, the enemy is always up and to the right.
    fn site_towards_enemy(&self, world: &SimWorld, blueprint: BlueprintId, planned: &[(u32, u32)]) -> Option<(u32, u32)> {
        let home = self.plan_position(world, self.home);
        let spot = Vec2Fp::new(home.x + fp_int(400), home.y + fp_int(400));
        self.search_site(world, blueprint, 1, cell_i32(cell_of(spot)), planned)
    }

    /// Scans square rings around a cell of the plan for a spot with a clear gap around it,
    /// so that the base never walls itself in.
    fn search_site(&self, world: &SimWorld, blueprint: BlueprintId, gap: i32, center: (i32, i32), planned: &[(u32, u32)]) -> Option<(u32, u32)> {
        let size = footprint(world, blueprint);
        for ring in 4..SEARCH_RINGS {
            for dy in -ring..=ring {
                for dx in -ring..=ring {
                    if dx.abs() != ring && dy.abs() != ring {
                        continue;
                    }
                    let Some(cell) = self.world_site(world, (center.0 + dx, center.1 + dy), size) else {
                        continue;
                    };
                    let (x, y) = cell_i32(cell);
                    if x < gap || y < gap {
                        continue;
                    }
                    let clear = (y - gap..y + size + gap).all(|row| {
                        (x - gap..x + size + gap).all(|column| {
                            let spot = (column as u32, row as u32);
                            world.terrain.cell_is_open(spot.0, spot.1) && !planned.contains(&spot)
                        })
                    });
                    if clear && world.can_place(blueprint, cell.0, cell.1) {
                        return Some(cell);
                    }
                }
            }
        }
        None
    }

    /// The AI lays out its base on a plan drawn from Blue's side of the map. Red's plan is
    /// Blue's turned 180 degrees about the map's center, with each footprint turned too, so
    /// that on a mirrored map both teams get the same layout. Turning is its own inverse.
    fn flip_site(&self, world: &SimWorld, cell: (i32, i32), size: i32) -> (i32, i32) {
        if self.side > 0 {
            return cell;
        }
        let last = world.terrain.map_units() / TERRAIN_CELL_UNITS - 1;
        (last - cell.0 - size + 1, last - cell.1 - size + 1)
    }

    /// Where a site of the plan lands on the map, if it is on the map at all.
    fn world_site(&self, world: &SimWorld, cell: (i32, i32), size: i32) -> Option<(u32, u32)> {
        let (x, y) = self.flip_site(world, cell, size);
        (x >= 0 && y >= 0).then_some((x as u32, y as u32))
    }

    /// A map position, as seen on the plan.
    fn plan_position(&self, world: &SimWorld, position: Vec2Fp) -> Vec2Fp {
        if self.side > 0 {
            return position;
        }
        let map = fp_int(world.terrain.map_units());
        Vec2Fp::new(map - position.x, map - position.y)
    }

    fn command_factories(&mut self, world: &SimWorld, forces: &Forces, commands: &mut Vec<SimCommand>) {
        let roster = self.roster();
        let (engineer, bot, artillery, tank, factory_t1, factory_t2) =
            (roster.engineer, roster.bot, roster.artillery, roster.tank, roster.factory_t1, roster.factory_t2);
        let economy = world.economies[self.team.index()];
        let mass_income = (economy.mass_income * 10).to_num::<i64>();
        let mut upgrade_under_way = forces.factories.iter().any(|factory| factory.upgrading);
        let mut engineers = forces.engineers;
        let rally = self.rally_point();
        for factory in forces.factories.iter().filter(|factory| factory.finished) {
            if !self.rally_set_on.contains(&factory.id) {
                self.rally_set_on.push(factory.id);
                commands.push(SimCommand::SetRally { factory: factory.id, point: rally });
            }
            if !factory.queue_is_empty {
                continue;
            }
            // Do not add to the queue during a stall, since that only spreads resources thinner.
            if economy.satisfaction < Res::ONE / 2 {
                continue;
            }
            let item = if engineers < MAX_ENGINEERS.min(2 + mass_income as usize / 5) {
                engineers += 1;
                engineer
            } else if factory.blueprint == factory_t1 && !upgrade_under_way && mass_income >= 8 {
                upgrade_under_way = true;
                factory_t2
            } else {
                self.unit_counter += 1;
                if factory.blueprint == factory_t2 {
                    if self.unit_counter.is_multiple_of(3) { bot } else { tank }
                } else if self.unit_counter.is_multiple_of(4) {
                    artillery
                } else {
                    bot
                }
            };
            commands.push(SimCommand::QueueUnit { factory: factory.id, blueprint: item, count: 1 });
        }
    }

    fn rally_point(&self) -> Vec2Fp {
        let forward = fp_int(500 * self.side);
        Vec2Fp::new(self.home.x + forward, self.home.y + forward)
    }

    fn command_army(&mut self, world: &SimWorld, forces: &Forces, sightings: &[Sighting], commands: &mut Vec<SimCommand>) {
        if forces.idle_combat.is_empty() {
            return;
        }
        // Enemies near home come first, whatever the size of the army.
        let defend = fp_int(DEFEND_RADIUS_UNITS);
        let threat = sightings
            .iter()
            .filter(|enemy| enemy.position.distance_squared(&self.home) <= wide_mul(defend, defend))
            .min_by_key(|enemy| (enemy.position.distance_squared(&self.home), enemy.id));
        if let Some(enemy) = threat {
            commands.push(SimCommand::AttackMove { units: forces.idle_combat.clone(), goal: enemy.position });
            return;
        }
        let gathered = forces.idle_combat.len();
        if gathered > self.gathered.0 {
            self.gathered = (gathered, world.current_tick);
        }
        let stopped_growing = world.current_tick - self.gathered.1 >= WAVE_PATIENCE_TICKS;
        if gathered < self.wave_size && !(stopped_growing && gathered >= FIRST_WAVE) {
            return;
        }
        // Send a wave at what can be seen, then the enemy's base, then search their deposits.
        let commander = world.commanders[self.team.enemy().index()];
        let target = sightings
            .iter()
            .find(|enemy| Some(enemy.id) == commander)
            .or_else(|| sightings.iter().find(|enemy| enemy.is_structure))
            .or_else(|| sightings.first())
            .map(|enemy| enemy.position);
        let full_strength = gathered >= LARGEST_WAVE;
        let goal = target.unwrap_or_else(|| self.search_point(world, full_strength));
        // Not in formation: waves at the slowest unit's pace arrived late enough that, in 200
        // AI-vs-AI matches, 6 went past the time limit, against none with each at its own speed.
        let mut wave = forces.idle_combat.clone();
        // A long, even game is one a defended base keeps winning. The commander joins a
        // full-size wave, which either breaks the enemy's base or loses the commander.
        if full_strength && world.current_tick >= COMMANDER_PUSH_TICKS {
            wave.extend(forces.commander);
        }
        commands.push(SimCommand::AttackMove { units: wave, goal });
        if gathered >= self.wave_size {
            self.wave_size = (self.wave_size + self.wave_size / 4 + 1).min(LARGEST_WAVE);
        }
        self.gathered = (0, world.current_tick);
    }

    /// Where to look when no enemy is in sight. The enemy's start is public knowledge, as on
    /// any RTS map. Smaller waves mostly raid the enemy's outlying deposits instead.
    fn search_point(&mut self, world: &SimWorld, full_strength: bool) -> Vec2Fp {
        // A full-size wave goes for the base, to finish a lead that raids have opened up.
        if full_strength || self.rng.below(4) == 0 {
            return self.enemy_home;
        }
        // Outlying extractors are worth more than the base: they are far from its defenses,
        // and every one lost widens a lead that would otherwise stay even.
        let outlying = fp_int(RAID_DISTANCE_UNITS);
        let enemy_side: Vec<(u32, u32)> = world
            .deposits
            .iter()
            .copied()
            .filter(|&cell| {
                let center = cell_center(cell);
                let from_enemy = center.distance_squared(&self.enemy_home);
                from_enemy < center.distance_squared(&self.home) && from_enemy > wide_mul(outlying, outlying)
            })
            .collect();
        match enemy_side.len() as u32 {
            0 => self.enemy_home,
            count => cell_center(enemy_side[self.rng.below(count) as usize]),
        }
    }
}

fn footprint(world: &SimWorld, blueprint: BlueprintId) -> i32 {
    i32::from(world.blueprints.get(blueprint).footprint)
}

const fn cell_i32(cell: (u32, u32)) -> (i32, i32) {
    (cell.0 as i32, cell.1 as i32)
}

fn cell_of(position: Vec2Fp) -> (u32, u32) {
    let cell = |value: Fp| (value.to_num::<i32>() / TERRAIN_CELL_UNITS).max(0) as u32;
    (cell(position.x), cell(position.y))
}

fn cell_center(cell: (u32, u32)) -> Vec2Fp {
    let center = |value: u32| fp_int(value as i32 * TERRAIN_CELL_UNITS + TERRAIN_CELL_UNITS / 2);
    Vec2Fp::new(center(cell.0), center(cell.1))
}

/// The enemies a team can see, and nothing else. All the AI's knowledge of enemies comes from here.
fn visible_enemies(world: &SimWorld, team: Team) -> Vec<Sighting> {
    world
        .spatial_grid
        .entries()
        .iter()
        .filter(|entry| entry.team != team && world.visibility.is_visible(team, entry.x, entry.y))
        // A unit may have died since the grid was built.
        .filter(|entry| world.unit_entity(entry.id).is_some())
        .map(|entry| Sighting {
            id: entry.id,
            position: Vec2Fp::new(Fp::from_bits(entry.x), Fp::from_bits(entry.y)),
            is_structure: entry.is_structure,
        })
        .collect()
}

fn nearest_visible_wreck(world: &SimWorld, team: Team, position: Vec2Fp, home: Vec2Fp, enemy_home: Vec2Fp) -> Option<SimId> {
    let mut best: Option<(FpWide, SimId)> = None;
    for &(id, entity) in &world.wrecks {
        let wreck = world.ecs.get::<&crate::sim::components::Wreck>(entity).ok()?;
        let (x, y) = (wreck.position.x.to_bits(), wreck.position.y.to_bits());
        let safe = wreck.position.distance_squared(&home) <= wreck.position.distance_squared(&enemy_home);
        // Trees and rocks show through fog, for the AI as for the player.
        if safe && (wreck.kind.is_scenery() || world.visibility.is_visible(team, x, y)) {
            let distance = wreck.position.distance_squared(&position);
            if best.is_none_or(|(closest, _)| distance < closest) {
                best = Some((distance, id));
            }
        }
    }
    best.map(|(_, id)| id)
}

/// One tick's commands, for replaying a match without its AI.
pub type CommandLog = Vec<Vec<SimCommand>>;

/// Advances an AI-vs-AI world one tick and returns the commands issued. The single code
/// path behind matches, the windowed test mode, and headless runs.
pub fn advance_ai_vs_ai(world: &mut SimWorld, brains: &mut [AiBrain; 2]) -> Vec<SimCommand> {
    let mut commands = brains[0].think(world);
    commands.extend(brains[1].think(world));
    world.advance_tick(&commands);
    commands
}

/// The outcome of an AI-vs-AI match.
pub struct MatchRecord {
    pub seed: u64,
    pub ticks: u32,
    pub result: Option<MatchResult>,
    pub checksums: Vec<u64>,
    pub log: CommandLog,
    pub units_built_peak: usize,
}

pub const CHECKSUM_INTERVAL: u32 = 100;

/// Plays a full AI-vs-AI skirmish, calling `observe` after each tick.
pub fn play_match(seed: u64, max_ticks: u32, mut observe: impl FnMut(&SimWorld)) -> MatchRecord {
    let mut world = crate::sim::scenario::skirmish(seed).world;
    let mut brains = [AiBrain::new(Team::Blue, seed), AiBrain::new(Team::Red, seed)];
    let mut record = MatchRecord { seed, ticks: 0, result: None, checksums: Vec::new(), log: Vec::new(), units_built_peak: 0 };
    while world.current_tick < max_ticks && world.match_result.is_none() {
        let commands = advance_ai_vs_ai(&mut world, &mut brains);
        record.log.push(commands);
        record.units_built_peak = record.units_built_peak.max(world.units.len());
        if world.current_tick.is_multiple_of(CHECKSUM_INTERVAL) {
            record.checksums.push(world.compute_checksum());
        }
        observe(&world);
    }
    record.ticks = world.current_tick;
    record.result = world.match_result;
    record.checksums.push(world.compute_checksum());
    record
}

/// Replays a recorded match with no AI and returns its checksums, which must match the original's.
pub fn replay_match(seed: u64, log: &CommandLog) -> Vec<u64> {
    let mut world = crate::sim::scenario::skirmish(seed).world;
    let mut checksums = Vec::new();
    for commands in log {
        world.advance_tick(commands);
        if world.current_tick.is_multiple_of(CHECKSUM_INTERVAL) {
            checksums.push(world.compute_checksum());
        }
    }
    checksums.push(world.compute_checksum());
    checksums
}
