//! The scripted opponent (GAME_SPECIFICATION.md §17). It plays by the player's rules: it
//! acts only through `SimCommand`, and sees enemies only through its team's fog of war.
//! It lives outside `SimWorld`, so a match can be replayed from its command log alone.
#![deny(clippy::float_arithmetic)]

use crate::math::fp::{fp_int, wide_mul, Fp, FpWide, Vec2Fp};
use crate::math::rng::SimRng;
use crate::sim::blueprints::{BlueprintId, FactoryKind, Res, UnitClass};
use crate::sim::commands::SimCommand;
use crate::sim::components::{Blueprinted, BuildTask, Builder, Construction, Factory, Order, OrderKind, Position, SimId, Team, UnitStats, Upgrading};
use crate::sim::config::TERRAIN_CELL_UNITS;
use crate::sim::scenario::MapSource;
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
const MAX_ENGINEERS: usize = 10;
const MAX_FACTORIES: usize = 6;
/// Mass income, a second, at which the AI takes each step up the tech tree.
const T2_INCOME: i64 = 8;
const T3_INCOME: i64 = 30;
/// Mass income at which it upgrades its other T1 factories to T2.
const SECOND_T2_INCOME: i64 = 20;
/// Mass income at which it starts upgrading extractors to T2, and to T3.
const EXTRACTOR_T2_INCOME: i64 = 10;
const EXTRACTOR_T3_INCOME: i64 = 40;
/// Mass income at which new generators are T2, and T3, when a builder can make them.
const GENERATOR_T2_INCOME: i64 = 15;
const GENERATOR_T3_INCOME: i64 = 45;
/// How many higher-tier engineers it keeps, once its factories can build them.
const T2_ENGINEERS: usize = 3;
/// A structure being built that costs at least this much mass is worth sending idle builders to.
const BIG_SITE_MASS: i32 = 500;
const T3_ENGINEERS: usize = 2;
/// How far from home the AI goes to build on a hydrocarbon vent.
const VENT_REACH_UNITS: i32 = 2_500;
const MAX_POINT_DEFENSES: usize = 4;
/// How long before trying a deposit again, in case a hidden enemy holds it.
const DEPOSIT_RETRY_TICKS: u32 = 1_500;

struct Roster {
    /// Engineers by tier, T1 first.
    engineers: [BlueprintId; 3],
    bot: BlueprintId,
    artillery: BlueprintId,
    tank: BlueprintId,
    amphibious: BlueprintId,
    missile_launcher: BlueprintId,
    heavy_artillery: BlueprintId,
    factory_t1: BlueprintId,
    /// Support factories, at T2 and T3.
    support_factories: [BlueprintId; 2],
    extractor: BlueprintId,
    /// Generators by tier, T1 first.
    generators: [BlueprintId; 3],
    storage: BlueprintId,
    point_defense: BlueprintId,
    hydrocarbon: BlueprintId,
}

impl Roster {
    fn new(world: &SimWorld) -> Self {
        let id = |key: &str| world.blueprints.id(key);
        Self {
            engineers: [id("t1_engineer"), id("t2_engineer"), id("t3_engineer")],
            bot: id("t1_light_bot"),
            artillery: id("t1_light_artillery"),
            tank: id("t2_heavy_tank"),
            amphibious: id("t2_amphibious_tank"),
            missile_launcher: id("t2_mobile_missile_launcher"),
            heavy_artillery: id("t3_mobile_artillery"),
            factory_t1: id("land_factory_t1"),
            support_factories: [id("land_factory_t2_support"), id("land_factory_t3_support")],
            extractor: id("mass_extractor"),
            generators: [id("power_generator"), id("power_generator_t2"), id("power_generator_t3")],
            storage: id("mass_storage"),
            point_defense: id("t1_point_defense"),
            hydrocarbon: id("hydrocarbon_plant"),
        }
    }

    fn is_generator(&self, blueprint: BlueprintId) -> bool {
        self.generators.contains(&blueprint)
    }
}

/// What the AI's own forces look like this turn.
#[derive(Default)]
struct Forces {
    commander: Option<SimId>,
    idle_builders: Vec<IdleBuilder>,
    engineers: usize,
    /// Engineers of each tier, T1 first.
    engineers_by_tier: [usize; 3],
    factories: Vec<FactoryInfo>,
    /// The team's best finished land headquarters' tier, 1 for none.
    headquarters_tier: u8,
    idle_combat: Vec<SimId>,
    combat_units: usize,
    extractors: Vec<(u32, u32)>,
    /// Finished extractors that could upgrade, with their tier and whether they are upgrading.
    extractor_sites: Vec<ExtractorInfo>,
    /// Structures still being built that are worth helping with, and where.
    big_sites: Vec<(SimId, Vec2Fp)>,
    /// Whether any engineer can build T2, or T3, structures.
    builder_tier: u8,
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
    tier: u8,
    support: bool,
}

#[derive(Copy, Clone)]
struct IdleBuilder {
    id: SimId,
    entity: hecs::Entity,
    position: Vec2Fp,
}

#[derive(Copy, Clone)]
struct ExtractorInfo {
    id: SimId,
    position: Vec2Fp,
    tier: u8,
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
    /// A brain for one team, with both teams' starts from the map.
    pub fn new(team: Team, seed: u64, starts: [Vec2Fp; 2]) -> Self {
        Self {
            team,
            home: starts[team.index()],
            enemy_home: starts[team.enemy().index()],
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
        self.command_upgrades(world, &forces, &mut commands);
        self.command_army(world, &forces, &sightings, &mut commands);
        commands
    }

    fn roster(&self) -> &Roster {
        self.roster.as_ref().expect("roster is set before use")
    }

    fn survey(&self, world: &SimWorld) -> Forces {
        let roster = self.roster();
        let mut forces = Forces { headquarters_tier: world.headquarters_tier(self.team, FactoryKind::Land), ..Forces::default() };
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
            if !finished && unit.is_structure() && unit.mass >= Res::from_num(BIG_SITE_MASS) {
                forces.big_sites.push((id, position));
            }
            match unit.class {
                UnitClass::Commander | UnitClass::Engineer => {
                    if unit.class == UnitClass::Commander {
                        forces.commander = Some(id);
                    } else {
                        forces.engineers += 1;
                        forces.engineers_by_tier[usize::from(unit.tier.clamp(1, 3) - 1)] += 1;
                        forces.builder_tier = forces.builder_tier.max(unit.tier);
                    }
                    let builder = world.ecs.get::<&Builder>(entity).expect("builders have builder parts");
                    let upgrading = world.ecs.get::<&crate::sim::components::Enhancing>(entity).is_ok();
                    if builder.tasks.is_empty() && order.kind == OrderKind::Idle && !upgrading {
                        forces.idle_builders.push(IdleBuilder { id, entity, position });
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
                UnitClass::Factory if unit.factory_kind == Some(FactoryKind::Land) => {
                    let factory = world.ecs.get::<&Factory>(entity).expect("factories have factory parts");
                    let half = i32::from(unit.footprint) / 2;
                    forces.factories.push(FactoryInfo {
                        id,
                        blueprint,
                        cell: ((cell.0 as i32 - half) as u32, (cell.1 as i32 - half) as u32),
                        finished,
                        queue_is_empty: factory.queue.is_empty(),
                        upgrading: factory.queue.first().is_some_and(|item| world.blueprints.get(*item).is_factory()),
                        tier: unit.tier,
                        support: unit.support,
                    });
                }
                // The AI builds neither silos' missiles, support commanders nor aircraft yet.
                UnitClass::Factory | UnitClass::Ammo | UnitClass::SupportCommander | UnitClass::Air => {}
                UnitClass::Economy | UnitClass::Defense => {
                    if unit.requires_deposit {
                        forces.extractors.push(cell);
                        if finished {
                            let upgrading = world.ecs.get::<&Upgrading>(entity).is_ok();
                            forces.extractor_sites.push(ExtractorInfo { id, position, tier: unit.tier, upgrading });
                        }
                    } else if roster.is_generator(blueprint) {
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
        for &IdleBuilder { id, entity, position } in &forces.idle_builders {
            let is_commander = forces.commander == Some(id);
            let plan = self.next_structure(world, forces, entity, position, is_commander, &planned, &new_blueprints);
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
            // An extractor upgrading near home is the best thing to help with: it pays for itself.
            let upgrade = forces
                .extractor_sites
                .iter()
                .filter(|extractor| extractor.upgrading)
                .min_by_key(|extractor| (extractor.position.distance_squared(&position), extractor.id));
            let site = forces.big_sites.iter().min_by_key(|(site, at)| (at.distance_squared(&position), *site));
            if let Some(&(site, _)) = site {
                commands.push(SimCommand::Assist { units: vec![id], target: site });
            } else if let Some(wreck) = nearest_visible_wreck(world, self.team, position, self.home, self.enemy_home) {
                commands.push(SimCommand::Reclaim { units: vec![id], target: wreck });
            } else if let Some(extractor) = upgrade {
                commands.push(SimCommand::Assist { units: vec![id], target: extractor.id });
            } else if let Some(factory) = forces.factories.iter().find(|factory| factory.finished && !factory.queue_is_empty) {
                commands.push(SimCommand::Assist { units: vec![id], target: factory.id });
            }
        }
    }

    /// Chooses what an idle builder should start next, in priority order.
    #[allow(clippy::too_many_arguments)]
    fn next_structure(
        &self,
        world: &SimWorld,
        forces: &Forces,
        builder: hecs::Entity,
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
        // The best generator this builder can make for the income, and the factory: a support
        // factory once there is a headquarters of its tier, which costs far less.
        // With income for a bigger generator, and an engineer that can build one, small
        // builders leave generators to it, and help instead.
        let wanted_tier = (0..3).rev().find(|&tier| [0, GENERATOR_T2_INCOME, GENERATOR_T3_INCOME][tier] <= mass_income).unwrap_or(0);
        let wanted_tier = wanted_tier.min(usize::from(forces.builder_tier.max(1)) - 1);
        let generator = (0..=wanted_tier).rev().map(|tier| roster.generators[tier]).find(|&blueprint| world.can_build(builder, blueprint));
        let generator_tier = generator.map_or(0, |blueprint| world.blueprints.get(blueprint).tier as usize - 1);
        let generator = generator.filter(|_| generator_tier == wanted_tier);
        let factory = (0..2)
            .rev()
            .map(|tier| roster.support_factories[tier])
            .find(|&blueprint| world.can_build(builder, blueprint) && world.support_allowed(self.team, blueprint))
            .unwrap_or(roster.factory_t1);
        let factories_planned = count_planned(roster.factory_t1) + roster.support_factories.iter().map(|&blueprint| count_planned(blueprint)).sum::<usize>();
        let generators_planned: usize = roster.generators.iter().map(|&blueprint| count_planned(blueprint)).sum();

        let factories_total = forces.factories.len() + factories_planned;
        if factories_total == 0 {
            return self.site_near_home(world, roster.factory_t1, 1, planned).map(|cell| (roster.factory_t1, cell));
        }

        // Extractors cost upkeep, so energy limits everything early on: while it is short, every
        // idle builder makes generators, and a new extractor waits. Generators keep ahead of
        // mass income, two at a time, before each new extractor.
        let energy_is_low = economy.energy < economy.energy_capacity * 2 / 5 && economy.energy_demand > economy.energy_income;
        let generators_coming = forces.generators_building + generators_planned;
        let energy_lags = energy_income < mass_income * 12;
        // A free vent near home is the cheapest energy there is.
        if energy_is_low || energy_lags {
            if let Some(cell) = self.vent_site(world, planned) {
                return Some((roster.hydrocarbon, cell));
            }
        }
        if energy_is_low {
            return generator.and_then(|generator| self.generator_site(world, forces, generator, planned).map(|cell| (generator, cell)));
        }
        if let Some(generator) = generator.filter(|_| energy_lags && generators_coming < 2) {
            if let Some(cell) = self.generator_site(world, forces, generator, planned) {
                return Some((generator, cell));
            }
        }
        if let Some(cell) = self.nearest_deposit(world, forces, position, is_commander, planned) {
            return Some((roster.extractor, cell));
        }

        let factories_wanted = (1 + mass_income as usize / 10).min(MAX_FACTORIES);
        if factories_total < factories_wanted && economy.mass > Res::from_num(150) {
            if let Some(cell) = self.site_near_home(world, factory, 1, planned) {
                return Some((factory, cell));
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

    /// The corner cell for a hydrocarbon plant over the free vent nearest home, if one is
    /// within reach of the base.
    fn vent_site(&self, world: &SimWorld, planned: &[(u32, u32)]) -> Option<(u32, u32)> {
        let plant = self.roster().hydrocarbon;
        let half = u32::from(world.blueprints.get(plant).footprint) / 2;
        let reach = wide_mul(fp_int(VENT_REACH_UNITS), fp_int(VENT_REACH_UNITS));
        world
            .vents
            .iter()
            .filter(|vent| vent.0 >= half && vent.1 >= half)
            .map(|vent| (vent.0 - half, vent.1 - half))
            .filter(|&cell| !planned.contains(&cell) && world.can_place(plant, cell.0, cell.1))
            .map(|cell| (cell_center(cell).distance_squared(&self.home), cell))
            .filter(|&(distance, _)| distance <= reach)
            .min()
            .map(|(_, cell)| cell)
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
    /// Larger generators go wherever there is room near home.
    fn generator_site(&self, world: &SimWorld, forces: &Forces, generator: BlueprintId, planned: &[(u32, u32)]) -> Option<(u32, u32)> {
        let size = footprint(world, generator);
        if size > 1 {
            return self.site_near_home(world, generator, 1, planned);
        }
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
        let economy = world.economies[self.team.index()];
        let mass_income = (economy.mass_income * 10).to_num::<i64>();
        let energy_income = (economy.energy_income * 10).to_num::<i64>();
        let mut upgrade_under_way = forces.factories.iter().any(|factory| factory.upgrading);
        let mut engineers = forces.engineers;
        let mut by_tier = forces.engineers_by_tier;
        let engineers_wanted = MAX_ENGINEERS.min(2 + mass_income as usize / 5);
        let rally = self.rally_point();
        let headquarters = forces.headquarters_tier;
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
            // A support factory builds nothing above its headquarters' tier.
            let tier = if factory.support { factory.tier.min(headquarters) } else { factory.tier };
            let upgrade = world.blueprints.get(factory.blueprint).upgrades_to;
            // One upgrade at a time: the first T1 factory to T2, that one to T3, then the rest.
            let upgrade_wanted = upgrade.is_some_and(|into| {
                let into = world.blueprints.get(into);
                let allowed = world.support_allowed(self.team, into.id) && (!into.support || headquarters >= into.tier);
                let income = match (into.tier, factory.tier < headquarters || factory.support) {
                    (2, false) => T2_INCOME,
                    (2, true) => SECOND_T2_INCOME,
                    _ => T3_INCOME,
                };
                // An upgrade draws a great deal of energy, so it waits for an economy that can feed it.
                let energy_ready = economy.satisfaction >= Res::ONE && energy_income >= mass_income * 10;
                allowed && energy_ready && mass_income >= income
            });
            let roster = self.roster();
            let engineer_tier = usize::from(tier.clamp(1, 3)) - 1;
            let item = if engineers < engineers_wanted {
                engineers += 1;
                by_tier[engineer_tier] += 1;
                roster.engineers[engineer_tier]
            } else if tier == 2 && by_tier[1] + by_tier[2] < T2_ENGINEERS {
                by_tier[1] += 1;
                roster.engineers[1]
            } else if tier == 3 && by_tier[2] < T3_ENGINEERS {
                by_tier[2] += 1;
                roster.engineers[2]
            } else if upgrade_wanted && !upgrade_under_way {
                upgrade_under_way = true;
                upgrade.expect("just checked")
            } else {
                self.unit_counter += 1;
                let roster = self.roster();
                let turn = self.unit_counter % 6;
                match tier {
                    1 => {
                        if self.unit_counter.is_multiple_of(4) { roster.artillery } else { roster.bot }
                    }
                    2 => [roster.tank, roster.tank, roster.bot, roster.amphibious, roster.tank, roster.missile_launcher][turn as usize],
                    _ => [roster.tank, roster.heavy_artillery, roster.tank, roster.amphibious, roster.missile_launcher, roster.tank][turn as usize],
                }
            };
            commands.push(SimCommand::QueueUnit { factory: factory.id, blueprint: item, count: 1 });
        }
    }

    /// Extractors near home upgrade first, a few at a time as income grows, unless energy is short.
    fn command_upgrades(&mut self, world: &SimWorld, forces: &Forces, commands: &mut Vec<SimCommand>) {
        let economy = world.economies[self.team.index()];
        let mass_income = (economy.mass_income * 10).to_num::<i64>();
        // Upgrades pay for themselves, so they go ahead while factories spend all the mass, but
        // not into a stall, which would slow everything.
        if mass_income < EXTRACTOR_T2_INCOME || economy.satisfaction < Res::ONE * 9 / 10 {
            return;
        }
        let energy_is_low = economy.energy < economy.energy_capacity * 2 / 5 && economy.energy_demand > economy.energy_income;
        if energy_is_low {
            return;
        }
        let under_way = forces.extractor_sites.iter().filter(|extractor| extractor.upgrading).count();
        if under_way > mass_income as usize / 25 {
            return;
        }
        let top = if mass_income >= EXTRACTOR_T3_INCOME { 2 } else { 1 };
        let next = forces
            .extractor_sites
            .iter()
            .filter(|extractor| !extractor.upgrading && extractor.tier <= top)
            .min_by_key(|extractor| (extractor.tier, extractor.position.distance_squared(&self.home), extractor.id));
        if let Some(extractor) = next {
            commands.push(SimCommand::Upgrade { units: vec![extractor.id] });
        }
    }

    /// A little way from home toward the enemy, on each axis.
    fn rally_point(&self) -> Vec2Fp {
        let toward = |home: Fp, enemy: Fp| fp_int(500 * if enemy > home { 1 } else if enemy < home { -1 } else { self.side });
        Vec2Fp::new(self.home.x + toward(self.home.x, self.enemy_home.x), self.home.y + toward(self.home.y, self.enemy_home.y))
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
pub fn play_match(seed: u64, max_ticks: u32, observe: impl FnMut(&SimWorld)) -> MatchRecord {
    play_match_on(&MapSource::Skirmish, seed, max_ticks, observe)
}

/// Plays a full AI-vs-AI match on a map, calling `observe` after each tick.
pub fn play_match_on(map: &MapSource, seed: u64, max_ticks: u32, mut observe: impl FnMut(&SimWorld)) -> MatchRecord {
    let mut world = map.scenario(seed).world;
    let mut brains = [AiBrain::new(Team::Blue, seed, world.starts), AiBrain::new(Team::Red, seed, world.starts)];
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
    replay_match_on(&MapSource::Skirmish, seed, log)
}

/// Replays a recorded match on a map with no AI, and returns its checksums.
pub fn replay_match_on(map: &MapSource, seed: u64, log: &CommandLog) -> Vec<u64> {
    let mut world = map.scenario(seed).world;
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
