//! The scripted opponent (GAME_SPECIFICATION.md §17). It plays by the player's rules: it
//! acts only through `SimCommand`, and sees enemies only through its team's fog of war.
//! It lives outside `SimWorld`, so a match can be replayed from its command log alone.
#![deny(clippy::float_arithmetic)]

use crate::math::fp::{fp_int, wide_mul, Fp, FpWide, Vec2Fp};
use crate::math::rng::SimRng;
use crate::sim::blueprints::{BlueprintId, FactoryKind, Res, UnitClass};
use crate::sim::commands::SimCommand;
use crate::sim::components::{Blueprinted, BuildTask, Builder, Combat, Construction, Factory, Launcher, MoveClass, Order, OrderKind, Position, SimId, Team, UnitStats, Upgrading};
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
/// From here, half an hour in, any wave of at least twice the first counts as full strength, so
/// that it goes for the base and the commander comes along: two even sides trading waves of 20
/// to 40 against each other's defenses otherwise played to the time limit.
const LATE_PUSH_TICKS: u32 = 30_000;
/// From here, near the soak's 40,000-tick limit, there are no more waves: everything idle, the
/// commander too, goes straight for the enemy commander, or failing sight of it, the enemy's
/// start. Even sides otherwise still traded to the limit now and then, a different seed after
/// each change.
const SUDDEN_DEATH_TICKS: u32 = 34_000;
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
const T3_INCOME: i64 = 50;
/// The most energy income a factory upgrade waits for, a second.
const UPGRADE_ENERGY_INCOME: i64 = 1_500;
/// The most mobile shields the AI keeps, since each costs as much energy to run as four T1
/// generators make.
const MAX_MOBILE_SHIELDS: usize = 2;
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
/// Mass income at which the AI builds its first air factory, without a T2 land headquarters,
/// and its second. Its air factory upgrades to T2 and T3 at these.
const AIR_FACTORY_INCOME: i64 = 15;
const SECOND_AIR_FACTORY_INCOME: i64 = 50;
const AIR_T2_INCOME: i64 = 25;
const AIR_T3_INCOME: i64 = 60;
/// Strike aircraft set out in groups that start at this size and grow to the largest.
const FIRST_AIR_WAVE: usize = 6;
const LARGEST_AIR_WAVE: usize = 16;
/// Enemy aircraft seen this recently call for anti-air.
const AIR_THREAT_TICKS: u32 = 3_000;
/// Anti-air structures it keeps: this many, plus one for every 20 mass a second, up to the most.
const BASE_ANTI_AIR: usize = 2;
const MOST_ANTI_AIR: usize = 6;
/// Aircraft it needs before it builds an air staging facility.
const STAGING_AIRCRAFT: usize = 6;
/// On a map with water, mass income for its first naval factory, without a T2 land
/// headquarters, and its second; its naval factories upgrade to T2 and T3 at these.
const NAVAL_FACTORY_INCOME: i64 = 20;
const SECOND_NAVAL_FACTORY_INCOME: i64 = 60;
const NAVAL_T2_INCOME: i64 = 30;
const NAVAL_T3_INCOME: i64 = 70;
/// How far from home it looks for water deep enough for a naval factory.
const NAVAL_REACH_UNITS: i32 = 7_000;
/// Fleets set out at this size, growing to the largest.
const FIRST_FLEET: usize = 4;
const LARGEST_FLEET: usize = 12;
/// A structure being built that costs at least this much mass is worth sending idle builders to.
const BIG_SITE_MASS: i32 = 500;
const T3_ENGINEERS: usize = 2;
/// How far from home the AI goes to build on a hydrocarbon vent.
const VENT_REACH_UNITS: i32 = 2_500;
const MAX_POINT_DEFENSES: usize = 4;
/// How long before trying a deposit again, in case a hidden enemy holds it.
const DEPOSIT_RETRY_TICKS: u32 = 1_500;
/// A radar once the base has this many factories: early warning for little mass.
const RADAR_FACTORIES: usize = 2;
/// Mass income at which the radar upgrades to T2, which costs 150 energy a second to run.
const RADAR_T2_INCOME: i64 = 40;
/// Mass income for the first shield generator, the first tactical missile defense, and the
/// tactical missile launcher. Each needs T2 builders.
const SHIELD_INCOME: i64 = 20;
const MOST_SHIELDS: usize = 4;
const TMD_INCOME: i64 = 20;
const MOST_TMDS: usize = 3;
const TACTICAL_LAUNCHER_INCOME: i64 = 25;
/// Energy income a second at which an AI that overcharges puts up an energy storage.
const OVERCHARGE_STORAGE_INCOME: i64 = 300;
/// The commander takes its T2 engineering suite at this mass income a second, its heavier
/// cannon at the next, and its T3 suite at the last, each only while it is safe at home: it
/// stands still while an enhancement is fitted. Earlier, the suite's 21,000 energy held back
/// the climb to T3 and the navy.
const COMMANDER_T2_SUITE_INCOME: i64 = 40;
const COMMANDER_CANNON_INCOME: i64 = 60;
const COMMANDER_T3_SUITE_INCOME: i64 = 80;
/// Engineering stations, which help build whatever is near, beside the factories.
const STATION_INCOME: i64 = 30;
const MOST_STATIONS: usize = 2;
/// A strategic missile costs 12,000 mass and 1.5 million energy: only a huge economy builds one.
const NUKE_MASS_INCOME: i64 = 100;
const NUKE_ENERGY_INCOME: i64 = 4_000;
/// With an air factory and this mass income a second, a light transport drops a few T1 land
/// units by an outlying enemy extractor, far from the base's defenses, then flies home for more.
const TRANSPORT_INCOME: i64 = 35;
/// How many it carries a drop, and the fewest worth sending.
const DROP_SIZE: usize = 6;
const FEWEST_DROPPED: usize = 4;
/// How long it waits for its passengers to board, and between ordering transports.
const BOARDING_PATIENCE_TICKS: u32 = 600;
const TRANSPORT_RETRY_TICKS: u32 = 1_500;
/// How far short of the target it sets them down, toward home, clear of what guards it.
const DROP_SHORT_UNITS: i32 = 250;
/// The commander overcharges only above this share of its health.
const OVERCHARGE_HEALTH_PERCENT: i32 = 50;
/// A radar blip this far above the ground or the water is an aircraft, which the army leaves alone.
const BLIP_GROUND_HEIGHT: i32 = 30;

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
    air_factory: BlueprintId,
    air_support_factories: [BlueprintId; 2],
    interceptor: BlueprintId,
    air_superiority: BlueprintId,
    /// Strike aircraft by tier: the bomber, the gunship, and the heavy gunship.
    strike_aircraft: [BlueprintId; 3],
    fighter_bomber: BlueprintId,
    /// Mobile anti-air: T1 and T2.
    mobile_anti_air: [BlueprintId; 2],
    /// Anti-air structures by tier.
    anti_air: [BlueprintId; 3],
    air_staging: BlueprintId,
    naval_factory: BlueprintId,
    naval_support_factories: [BlueprintId; 2],
    frigate: BlueprintId,
    submarine: BlueprintId,
    destroyer: BlueprintId,
    cruiser: BlueprintId,
    shield_boat: BlueprintId,
    battleship: BlueprintId,
    battlecruiser: BlueprintId,
    energy_storage: BlueprintId,
    torpedo_boat: BlueprintId,
    medium_tank: BlueprintId,
    gatling_bot: BlueprintId,
    mobile_shield: BlueprintId,
    heavy_assault_bot: BlueprintId,
    armored_assault_bot: BlueprintId,
    /// Radars by tier: the radar and the T2 radar.
    radars: [BlueprintId; 2],
    shield: BlueprintId,
    tactical_launcher: BlueprintId,
    tactical_missile: BlueprintId,
    tactical_defense: BlueprintId,
    engineering_stations: [BlueprintId; 2],
    light_transport: BlueprintId,
    strategic_launcher: BlueprintId,
    strategic_missile: BlueprintId,
    strategic_defense: BlueprintId,
    anti_nuke: BlueprintId,
    /// The commander's enhancements, in the order it takes them.
    t2_suite: crate::sim::blueprints::EnhancementId,
    heavy_cannon: crate::sim::blueprints::EnhancementId,
    t3_suite: crate::sim::blueprints::EnhancementId,
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
            air_factory: id("air_factory"),
            air_support_factories: [id("air_factory_t2_support"), id("air_factory_t3_support")],
            interceptor: id("t1_interceptor"),
            air_superiority: id("t3_air_superiority"),
            strike_aircraft: [id("t1_bomber"), id("t2_gunship"), id("t3_heavy_gunship")],
            fighter_bomber: id("t2_fighter_bomber"),
            mobile_anti_air: [id("t1_mobile_aa"), id("t2_mobile_flak")],
            anti_air: [id("t1_aa_turret"), id("t2_flak"), id("t3_sam")],
            air_staging: id("air_staging"),
            naval_factory: id("naval_factory"),
            naval_support_factories: [id("naval_factory_t2_support"), id("naval_factory_t3_support")],
            frigate: id("t1_frigate"),
            submarine: id("t1_attack_sub"),
            destroyer: id("t2_destroyer"),
            cruiser: id("t2_cruiser"),
            shield_boat: id("t2_shield_boat"),
            battleship: id("t3_battleship"),
            battlecruiser: id("t3_battlecruiser"),
            torpedo_boat: id("t2_torpedo_boat"),
            energy_storage: id("energy_storage"),
            medium_tank: id("t1_medium_tank"),
            gatling_bot: id("t2_gatling_bot"),
            mobile_shield: id("t2_mobile_shield"),
            heavy_assault_bot: id("t3_heavy_assault_bot"),
            armored_assault_bot: id("t3_armored_assault_bot"),
            radars: [id("radar"), id("radar_t2")],
            shield: id("shield_generator"),
            tactical_launcher: id("tactical_missile_launcher"),
            tactical_missile: id("tactical_missile"),
            tactical_defense: id("tactical_missile_defense"),
            engineering_stations: [id("engineering_station"), id("engineering_station_t3")],
            light_transport: id("t1_light_transport"),
            strategic_launcher: id("strategic_missile_launcher"),
            strategic_missile: id("strategic_missile"),
            strategic_defense: id("strategic_missile_defense"),
            anti_nuke: id("anti_nuke"),
            t2_suite: world.blueprints.enhancement_id("advanced_engineering"),
            heavy_cannon: world.blueprints.enhancement_id("heavy_cannon"),
            t3_suite: world.blueprints.enhancement_id("t3_engineering"),
        }
    }

    fn is_generator(&self, blueprint: BlueprintId) -> bool {
        self.generators.contains(&blueprint)
    }

    fn is_fighter(&self, blueprint: BlueprintId) -> bool {
        blueprint == self.interceptor || blueprint == self.air_superiority
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
    /// Other structures left unfinished, which idle builders finish after the big ones.
    small_sites: Vec<(SimId, Vec2Fp)>,
    /// Whether any engineer can build T2, or T3, structures.
    builder_tier: u8,
    air_factories: Vec<FactoryInfo>,
    air_headquarters_tier: u8,
    /// Idle fighters, and idle strike aircraft, and all aircraft.
    idle_fighters: Vec<SimId>,
    idle_strike: Vec<SimId>,
    aircraft: usize,
    anti_air_structures: usize,
    air_stagings: usize,
    naval_factories: Vec<FactoryInfo>,
    naval_headquarters_tier: u8,
    /// Builders at work on anything but a generator.
    busy_builders: Vec<IdleBuilder>,
    /// What the running extractors' upkeep costs a tick.
    extractor_upkeep: Res,
    /// Energy gone, and less coming in than the extractors alone cost.
    energy_crisis: bool,
    /// Idle ships and submarines, and all of them.
    idle_naval: Vec<SimId>,
    naval_units: usize,
    generators: usize,
    generators_building: usize,
    storages: usize,
    energy_storages: usize,
    point_defenses: usize,
    /// Finished radars that could upgrade, and whether they are, and all radars.
    radar_sites: Vec<(SimId, bool)>,
    radars: usize,
    shields: usize,
    tactical_defenses: usize,
    tactical_launchers: Vec<LauncherInfo>,
    stations: usize,
    transports: Vec<TransportInfo>,
    /// Idle T1 land combat units, which fit a light transport.
    idle_light_combat: Vec<(SimId, Vec2Fp)>,
    strategic_launchers: Vec<LauncherInfo>,
    strategic_defenses: Vec<LauncherInfo>,
    /// Cells that are spoken for: planned by a builder on its way, each site's whole footprint.
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
struct LauncherInfo {
    id: SimId,
    position: Vec2Fp,
    finished: bool,
    queue_is_empty: bool,
    stock: u32,
    /// Already firing at something.
    busy: bool,
}

#[derive(Copy, Clone)]
struct TransportInfo {
    id: SimId,
    position: Vec2Fp,
    cargo: usize,
    /// Set down what it carries, or on its way to.
    unloading: bool,
    idle: bool,
}

/// A transport drop under way.
#[derive(Clone)]
struct Drop {
    transport: SimId,
    passengers: Vec<SimId>,
    target: Vec2Fp,
    phase: DropPhase,
    since: u32,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum DropPhase {
    Boarding,
    Flying,
    Returning,
}

#[derive(Copy, Clone)]
struct ExtractorInfo {
    id: SimId,
    position: Vec2Fp,
    tier: u8,
    upgrading: bool,
    paused: bool,
}

/// A seen enemy, from the team's own sight.
#[derive(Copy, Clone)]
struct Sighting {
    id: SimId,
    position: Vec2Fp,
    is_structure: bool,
    is_air: bool,
    /// On or under the water: a ship, a submarine, or a structure on the water.
    is_naval: bool,
    /// Under the water, where only torpedoes reach: a submarine, or an amphibious unit on the
    /// seabed. The land army cannot hit it.
    is_submerged: bool,
}

/// How hard the AI plays. The player's opponent is easy unless asked otherwise. AI-vs-AI
/// matches, the soak among them, always play normally, so that they keep testing all of it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Difficulty {
    /// A small economy and a small army, slow to react and to climb the tech tree, waiting
    /// twelve minutes before it attacks and then in small waves, and without missiles,
    /// overcharge, or its commander in the fight.
    Easy,
    /// Everything it knows.
    Normal,
}

impl Difficulty {
    /// From its name, as given on the command line or in the page address.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "easy" => Some(Self::Easy),
            "normal" => Some(Self::Normal),
            _ => None,
        }
    }

    const fn skill(self) -> Skill {
        match self {
            Self::Easy => Skill {
                think_interval: THINK_INTERVAL * 3,
                income_share: (1, 2),
                max_engineers: 3,
                // None: its engineers stay within the 3, and replacements come at the factory's tier.
                tier_engineers: (0, 0),
                max_factories: 2,
                max_point_defenses: 1,
                first_wave: 6,
                largest_wave: 12,
                max_army: 16,
                max_aircraft: 4,
                max_fleet: 3,
                first_attack_tick: 7_200,
                commander_push_ticks: u32::MAX,
                missiles: false,
                overcharge: false,
                enhancements: false,
                transports: false,
            },
            Self::Normal => Skill {
                think_interval: THINK_INTERVAL,
                income_share: (1, 1),
                max_engineers: MAX_ENGINEERS,
                tier_engineers: (T2_ENGINEERS, T3_ENGINEERS),
                max_factories: MAX_FACTORIES,
                max_point_defenses: MAX_POINT_DEFENSES,
                first_wave: FIRST_WAVE,
                largest_wave: LARGEST_WAVE,
                max_army: usize::MAX,
                max_aircraft: usize::MAX,
                max_fleet: usize::MAX,
                first_attack_tick: 0,
                commander_push_ticks: COMMANDER_PUSH_TICKS,
                missiles: true,
                overcharge: true,
                enhancements: true,
                transports: true,
            },
        }
    }
}

/// What a difficulty changes.
#[derive(Copy, Clone, Debug)]
struct Skill {
    /// How often it decides anything, in ticks.
    think_interval: u32,
    /// The share of its real income it plans as if it had, as a fraction: less makes it slower
    /// to expand and to climb the tech tree.
    income_share: (i64, i64),
    max_engineers: usize,
    /// Higher-tier engineers it keeps on top of `max_engineers`, at T2 or above and at T3.
    tier_engineers: (usize, usize),
    max_factories: usize,
    max_point_defenses: usize,
    first_wave: usize,
    largest_wave: usize,
    /// The most land combat units, aircraft, and ships it keeps.
    max_army: usize,
    max_aircraft: usize,
    max_fleet: usize,
    /// It defends from the start, but sets out to attack only from this tick.
    first_attack_tick: u32,
    /// When its commander starts joining full-size waves.
    commander_push_ticks: u32,
    missiles: bool,
    overcharge: bool,
    /// Whether its commander takes enhancements.
    enhancements: bool,
    /// Whether it drops raiders by transport.
    transports: bool,
}

pub struct AiBrain {
    team: Team,
    skill: Skill,
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
    air_counter: u32,
    air_wave_size: usize,
    /// When it last saw an enemy aircraft.
    enemy_air_seen: Option<u32>,
    fleet_size: usize,
    /// Worked out once per map: deep water near home where a naval factory fits, nearest
    /// first, and the deep water nearest the enemy's start, where fleets go.
    naval_plan: Option<NavalPlan>,
    /// Whether an enemy strategic missile launcher has been seen, which calls for a defense.
    enemy_nuke_seen: bool,
    /// The transport drop under way, and when a transport was last ordered.
    drop: Option<Drop>,
    transport_ordered: Option<u32>,
}

struct NavalPlan {
    sites: Vec<(u32, u32)>,
    enemy_water: Option<Vec2Fp>,
}

impl AiBrain {
    /// A brain for one team, with both teams' starts from the map, playing normally.
    pub fn new(team: Team, seed: u64, starts: [Vec2Fp; 2]) -> Self {
        Self::with_difficulty(team, seed, starts, Difficulty::Normal)
    }

    pub fn with_difficulty(team: Team, seed: u64, starts: [Vec2Fp; 2], difficulty: Difficulty) -> Self {
        let skill = difficulty.skill();
        Self {
            team,
            skill,
            home: starts[team.index()],
            enemy_home: starts[team.enemy().index()],
            side: if team == Team::Blue { 1 } else { -1 },
            // Both teams draw the same numbers, so that on a mirrored map they make mirrored choices.
            rng: SimRng::new(seed ^ 0xA5A5_5A5A),
            wave_size: skill.first_wave,
            gathered: (0, 0),
            deposit_attempts: Vec::new(),
            roster: None,
            rally_set_on: Vec::new(),
            unit_counter: 0,
            air_counter: 0,
            air_wave_size: FIRST_AIR_WAVE,
            enemy_air_seen: None,
            fleet_size: FIRST_FLEET,
            naval_plan: None,
            enemy_nuke_seen: false,
            drop: None,
            transport_ordered: None,
        }
    }

    pub const fn team(&self) -> Team {
        self.team
    }

    /// Decides this tick's orders. Returns nothing on ticks when it is not this team's turn.
    pub fn think(&mut self, world: &SimWorld) -> Vec<SimCommand> {
        if !world.current_tick.is_multiple_of(self.skill.think_interval) || world.match_result.is_some() {
            return Vec::new();
        }
        if self.roster.is_none() {
            self.roster = Some(Roster::new(world));
            self.naval_plan = Some(self.plan_naval(world));
        }
        let mut forces = self.survey(world);
        let sightings = visible_enemies(world, self.team);
        if sightings.iter().any(|enemy| enemy.is_air) {
            self.enemy_air_seen = Some(world.current_tick);
        }
        let launcher = self.roster().strategic_launcher;
        self.enemy_nuke_seen |= sightings.iter().any(|enemy| {
            enemy.is_structure && world.unit_entity(enemy.id).and_then(|entity| world.ecs.get::<&Blueprinted>(entity).ok().map(|unit| unit.0 == launcher)).unwrap_or(false)
        });
        let mut commands = Vec::new();
        self.command_energy(world, &forces, &mut commands);
        self.command_builders(world, &forces, &mut commands);
        self.command_factories(world, &forces, &mut commands);
        self.command_air_factories(world, &forces, &mut commands);
        self.command_naval_factories(world, &forces, &mut commands);
        self.command_upgrades(world, &forces, &mut commands);
        if self.skill.enhancements {
            self.command_enhancements(world, &forces, &sightings, &mut commands);
        }
        if self.skill.transports {
            self.command_drops(world, &mut forces, &sightings, &mut commands);
        }
        self.command_army(world, &forces, &sightings, &mut commands);
        self.command_air(world, &forces, &sightings, &mut commands);
        self.command_fleet(&forces, &sightings, &mut commands);
        if self.skill.missiles {
            self.command_missiles(world, &forces, &sightings, &mut commands);
        }
        if self.skill.overcharge {
            self.command_overcharge(world, &forces, &sightings, &mut commands);
        }
        commands
    }

    /// An income, a second, as the AI plans with it: all of it, or less when it plays easy.
    const fn planned_income(&self, income: i64) -> i64 {
        income * self.skill.income_share.0 / self.skill.income_share.1
    }

    fn roster(&self) -> &Roster {
        self.roster.as_ref().expect("roster is set before use")
    }

    fn survey(&self, world: &SimWorld) -> Forces {
        let roster = self.roster();
        let mut forces = Forces {
            headquarters_tier: world.headquarters_tier(self.team, FactoryKind::Land),
            air_headquarters_tier: world.headquarters_tier(self.team, FactoryKind::Air),
            naval_headquarters_tier: world.headquarters_tier(self.team, FactoryKind::Naval),
            ..Forces::default()
        };
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
            if !finished && unit.is_structure() && unit.mass < Res::from_num(BIG_SITE_MASS) && order.kind == OrderKind::Idle {
                forces.small_sites.push((id, position));
            }
            if !finished && unit.is_structure() && unit.mass >= Res::from_num(BIG_SITE_MASS) {
                forces.big_sites.push((id, position));
            }
            // Ships and submarines are the fleet's, whatever their class.
            if matches!(stats.move_class, MoveClass::Naval | MoveClass::Submarine) && !unit.is_structure() {
                forces.naval_units += 1;
                if order.kind == OrderKind::Idle && finished {
                    forces.idle_naval.push(id);
                }
                continue;
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
                    // In an energy crisis, a builder on anything but a generator is taken off it.
                    let off_generators = !builder.tasks.is_empty()
                        && !builder.tasks.iter().any(|task| matches!(task, BuildTask::Build { blueprint, .. } if roster.is_generator(*blueprint)));
                    if off_generators && !upgrading {
                        forces.busy_builders.push(IdleBuilder { id, entity, position });
                    }
                    for task in &builder.tasks {
                        if let BuildTask::Build { blueprint, cell_x, cell_y, site: None } = *task {
                            forces.planned.extend(footprint_cells(world, blueprint, (cell_x, cell_y)));
                            forces.planned_blueprints.push(blueprint);
                        }
                    }
                }
                UnitClass::Bot | UnitClass::Artillery | UnitClass::Tank => {
                    forces.combat_units += 1;
                    if order.kind == OrderKind::Idle {
                        forces.idle_combat.push(id);
                        if unit.tier == 1 && finished && world.ecs.get::<&crate::sim::components::Boarding>(entity).is_err() {
                            forces.idle_light_combat.push((id, position));
                        }
                    }
                }
                UnitClass::Air if blueprint == roster.light_transport => {
                    forces.aircraft += 1;
                    let (cargo, unloading) = world.ecs.get::<&crate::sim::components::Transport>(entity).map_or((0, false), |transport| (transport.cargo.len(), transport.unload.is_some()));
                    forces.transports.push(TransportInfo { id, position, cargo, unloading, idle: order.kind == OrderKind::Idle && finished });
                }
                // Only land factories: the AI builds no missile silos, and must never queue in one.
                UnitClass::Factory if unit.factory_kind.is_some() => {
                    let factory = world.ecs.get::<&Factory>(entity).expect("factories have factory parts");
                    let half = i32::from(unit.footprint) / 2;
                    let list = match unit.factory_kind {
                        Some(FactoryKind::Air) => &mut forces.air_factories,
                        Some(FactoryKind::Naval) => &mut forces.naval_factories,
                        _ => &mut forces.factories,
                    };
                    list.push(FactoryInfo {
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
                UnitClass::Air => {
                    forces.aircraft += 1;
                    if order.kind == OrderKind::Idle && finished {
                        if roster.is_fighter(blueprint) {
                            forces.idle_fighters.push(id);
                        } else {
                            forces.idle_strike.push(id);
                        }
                    }
                }
                // Of the silos, only the tactical missile launcher. No support commanders yet.
                UnitClass::Factory if blueprint == roster.tactical_launcher => {
                    let factory = world.ecs.get::<&Factory>(entity).expect("launchers have factory parts");
                    forces.tactical_launchers.push(LauncherInfo {
                        id,
                        position,
                        finished,
                        queue_is_empty: factory.queue.is_empty(),
                        stock: world.ecs.get::<&Launcher>(entity).map_or(0, |launcher| launcher.stock),
                        busy: world.ecs.get::<&Combat>(entity).is_ok_and(|combat| combat.forced_target.is_some() || combat.ground_target.is_some()),
                    });
                }
                UnitClass::Factory if blueprint == roster.strategic_launcher || blueprint == roster.strategic_defense => {
                    let factory = world.ecs.get::<&Factory>(entity).expect("launchers have factory parts");
                    let info = LauncherInfo {
                        id,
                        position,
                        finished,
                        queue_is_empty: factory.queue.is_empty(),
                        stock: world.ecs.get::<&Launcher>(entity).map_or(0, |launcher| launcher.stock),
                        busy: world.ecs.get::<&Combat>(entity).is_ok_and(|combat| combat.forced_target.is_some() || combat.ground_target.is_some()),
                    };
                    if blueprint == roster.strategic_launcher {
                        forces.strategic_launchers.push(info);
                    } else {
                        forces.strategic_defenses.push(info);
                    }
                }
                UnitClass::Factory | UnitClass::Ammo | UnitClass::SupportCommander => {}
                UnitClass::Economy | UnitClass::Defense => {
                    if unit.requires_deposit {
                        forces.extractors.push(cell);
                        if finished {
                            let upgrading = world.ecs.get::<&Upgrading>(entity).is_ok();
                            let paused = world.ecs.get::<&crate::sim::components::Paused>(entity).is_ok();
                            if !paused {
                                forces.extractor_upkeep += unit.upkeep;
                            }
                            forces.extractor_sites.push(ExtractorInfo { id, position, tier: unit.tier, upgrading, paused });
                        }
                    } else if roster.is_generator(blueprint) {
                        forces.generators += 1;
                        forces.generators_building += usize::from(!finished);
                    } else if roster.anti_air.contains(&blueprint) {
                        forces.anti_air_structures += 1;
                    } else if blueprint == roster.air_staging {
                        forces.air_stagings += 1;
                    } else if blueprint == roster.storage {
                        forces.storages += 1;
                    } else if blueprint == roster.energy_storage {
                        forces.energy_storages += 1;
                    } else if blueprint == roster.point_defense {
                        forces.point_defenses += 1;
                    } else if roster.radars.contains(&blueprint) {
                        forces.radars += 1;
                        if finished && blueprint == roster.radars[0] {
                            forces.radar_sites.push((id, world.ecs.get::<&Upgrading>(entity).is_ok()));
                        }
                    } else if blueprint == roster.shield {
                        forces.shields += 1;
                    } else if blueprint == roster.tactical_defense {
                        forces.tactical_defenses += 1;
                    } else if roster.engineering_stations.contains(&blueprint) {
                        forces.stations += 1;
                    }
                }
            }
        }
        // Extractors pay their upkeep before anything is built. Once raids take out the
        // generators and less comes in than they cost, a generator under way never finishes.
        let economy = &world.economies[self.team.index()];
        forces.energy_crisis = economy.energy <= economy.energy_capacity / 20 && economy.energy_income <= forces.extractor_upkeep && forces.extractor_upkeep > Res::ZERO;
        if forces.energy_crisis {
            let busy = std::mem::take(&mut forces.busy_builders);
            forces.idle_builders.extend(busy);
        }
        forces
    }

    fn command_builders(&mut self, world: &SimWorld, forces: &Forces, commands: &mut Vec<SimCommand>) {
        let mut planned = forces.planned.clone();
        let mut new_blueprints = forces.planned_blueprints.clone();
        // In an energy crisis, one generator at a time, with every other builder helping: a
        // trickle of energy split between many sites finishes none of them.
        let mut crisis_lead: Option<SimId> = None;
        for &IdleBuilder { id, entity, position } in &forces.idle_builders {
            let is_commander = forces.commander == Some(id);
            if let Some(lead) = crisis_lead {
                commands.push(SimCommand::Assist { units: vec![id], target: lead });
                continue;
            }
            let plan = self.next_structure(world, forces, entity, position, is_commander, &planned, &new_blueprints);
            if let Some((blueprint, cell)) = plan {
                planned.extend(footprint_cells(world, blueprint, cell));
                new_blueprints.push(blueprint);
                if blueprint == self.roster().extractor {
                    self.deposit_attempts.push((cell, world.current_tick));
                }
                if forces.energy_crisis && self.roster().is_generator(blueprint) {
                    crisis_lead = Some(id);
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
            // A big site first, then any other left unfinished: a builder taken off one, as in an
            // energy crisis, otherwise left it half built for good.
            let nearest = |sites: &[(SimId, Vec2Fp)]| sites.iter().min_by_key(|(site, at)| (at.distance_squared(&position), *site)).copied();
            let site = nearest(&forces.big_sites).or_else(|| nearest(&forces.small_sites));
            if let Some((site, _)) = site {
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
        let per_second = |value: Res| self.planned_income((value * 10).to_num::<i64>());
        let (mass_income, energy_income) = (per_second(economy.mass_income), per_second(economy.energy_income));
        let count_planned = |blueprint: BlueprintId| planned_blueprints.iter().filter(|item| **item == blueprint).count();
        // The best generator this builder can make for the income, and the factory: a support
        // factory once there is a headquarters of its tier, which costs far less.
        // With income for a bigger generator, and an engineer that can build one, small
        // builders leave generators to it, and help instead.
        let wanted_tier = (0..3).rev().find(|&tier| [0, GENERATOR_T2_INCOME, GENERATOR_T3_INCOME][tier] <= mass_income).unwrap_or(0);
        let wanted_tier = wanted_tier.min(usize::from(forces.builder_tier.max(1)) - 1);
        // With no energy at all, the cheapest generator, which the trickle can actually finish.
        let wanted_tier = if forces.energy_crisis { 0 } else { wanted_tier };
        let generator = (0..=wanted_tier).rev().map(|tier| roster.generators[tier]).find(|&blueprint| world.can_build(builder, blueprint));
        let generator_tier = generator.map_or(0, |blueprint| world.blueprints.get(blueprint).tier as usize - 1);
        // But while a shortage of energy is slowing everything down, every builder makes the best
        // generator it can: the few that could make bigger ones were busy elsewhere, and energy
        // sat flat for ten minutes while mass doubled.
        let energy_stall = economy.satisfaction < Res::ONE * 9 / 10 && economy.energy < economy.energy_capacity / 5;
        let generator = generator.filter(|_| generator_tier == wanted_tier || energy_stall);
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
        // Aircraft cost far more energy for their mass than anything on the ground.
        let ratio = if forces.air_factories.is_empty() { 12 } else { 20 };
        let energy_lags = energy_income < mass_income * ratio;
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

        let factories_wanted = (1 + mass_income as usize / 10).min(self.skill.max_factories);
        // A second factory goes up even in a stall, so that an upgrade never stops all production.
        if factories_total < factories_wanted && (economy.mass > Res::from_num(150) || factories_total < 2) {
            if let Some(cell) = self.site_near_home(world, factory, 1, planned) {
                return Some((factory, cell));
            }
        }

        // Air comes once the land side has its T2 headquarters, or the income for it.
        let air_factory = (0..2)
            .rev()
            .map(|tier| roster.air_support_factories[tier])
            .find(|&blueprint| world.can_build(builder, blueprint) && world.support_allowed(self.team, blueprint))
            .unwrap_or(roster.air_factory);
        let air_planned = count_planned(roster.air_factory) + roster.air_support_factories.iter().map(|&blueprint| count_planned(blueprint)).sum::<usize>();
        let air_wanted = if mass_income >= SECOND_AIR_FACTORY_INCOME {
            2
        } else {
            usize::from(forces.headquarters_tier >= 2 || mass_income >= AIR_FACTORY_INCOME)
        };
        // The first needs no mass in store: it is cheap, and the AI spends all it makes.
        let air_total = forces.air_factories.len() + air_planned;
        if air_total < air_wanted && world.can_build(builder, air_factory) && (air_total == 0 || economy.mass > Res::from_num(150)) {
            if let Some(cell) = self.site_near_home(world, air_factory, 1, planned) {
                return Some((air_factory, cell));
            }
        }

        // On a map with water, a naval factory once the land side has its T2 headquarters, or the income.
        let naval_factory = (0..2)
            .rev()
            .map(|tier| roster.naval_support_factories[tier])
            .find(|&blueprint| world.can_build(builder, blueprint) && world.support_allowed(self.team, blueprint))
            .unwrap_or(roster.naval_factory);
        let naval_planned = count_planned(roster.naval_factory) + roster.naval_support_factories.iter().map(|&blueprint| count_planned(blueprint)).sum::<usize>();
        let naval_wanted = if mass_income >= SECOND_NAVAL_FACTORY_INCOME {
            2
        } else {
            usize::from(forces.headquarters_tier >= 2 || mass_income >= NAVAL_FACTORY_INCOME)
        };
        let naval_total = forces.naval_factories.len() + naval_planned;
        if naval_total < naval_wanted && world.can_build(builder, naval_factory) && (naval_total == 0 || economy.mass > Res::from_num(150)) {
            if let Some(cell) = self.naval_site(world, naval_factory, planned) {
                return Some((naval_factory, cell));
            }
        }

        // Once enemy aircraft have been seen, anti-air goes up toward the enemy, the best this
        // builder can make.
        if self.air_threat(world) {
            let wanted = (BASE_ANTI_AIR + mass_income as usize / 20).min(MOST_ANTI_AIR);
            let planned_anti_air: usize = roster.anti_air.iter().map(|&blueprint| count_planned(blueprint)).sum();
            let turret = roster.anti_air.iter().rev().copied().find(|&blueprint| world.can_build(builder, blueprint));
            if let Some(turret) = turret.filter(|_| forces.anti_air_structures + planned_anti_air < wanted) {
                if let Some(cell) = self.site_towards_enemy(world, turret, planned) {
                    return Some((turret, cell));
                }
            }
        }
        if forces.aircraft >= STAGING_AIRCRAFT && forces.air_stagings + count_planned(roster.air_staging) == 0 && world.can_build(builder, roster.air_staging) {
            if let Some(cell) = self.site_near_home(world, roster.air_staging, 1, planned) {
                return Some((roster.air_staging, cell));
            }
        }

        // Intel and defenses. A radar warns of what comes, for little; shields, missile defense,
        // and a missile launcher come with T2 builders and the income to run them.
        if factories_total >= RADAR_FACTORIES && forces.radars + count_planned(roster.radars[0]) == 0 && world.can_build(builder, roster.radars[0]) {
            if let Some(cell) = self.site_towards_enemy(world, roster.radars[0], planned) {
                return Some((roster.radars[0], cell));
            }
        }
        if !energy_lags {
            let shields_wanted = if mass_income >= SHIELD_INCOME { (1 + (mass_income - SHIELD_INCOME) as usize / 30).min(MOST_SHIELDS) } else { 0 };
            if forces.shields + count_planned(roster.shield) < shields_wanted && world.can_build(builder, roster.shield) {
                if let Some(cell) = self.site_near_home(world, roster.shield, 1, planned) {
                    return Some((roster.shield, cell));
                }
            }
            let defenses_wanted = if mass_income >= TMD_INCOME { (1 + (mass_income - TMD_INCOME) as usize / 40).min(MOST_TMDS) } else { 0 };
            if forces.tactical_defenses + count_planned(roster.tactical_defense) < defenses_wanted && world.can_build(builder, roster.tactical_defense) {
                if let Some(cell) = self.site_towards_enemy(world, roster.tactical_defense, planned) {
                    return Some((roster.tactical_defense, cell));
                }
            }
            let launchers_wanted = usize::from(mass_income >= TACTICAL_LAUNCHER_INCOME);
            if forces.tactical_launchers.len() + count_planned(roster.tactical_launcher) < launchers_wanted && world.can_build(builder, roster.tactical_launcher) {
                if let Some(cell) = self.site_near_home(world, roster.tactical_launcher, 1, planned) {
                    return Some((roster.tactical_launcher, cell));
                }
            }
            // Engineering stations beside the factories, which help with whatever is near.
            let stations_wanted = if mass_income >= STATION_INCOME { (1 + (mass_income - STATION_INCOME) as usize / 40).min(MOST_STATIONS) } else { 0 };
            if forces.stations + count_planned(roster.engineering_stations[0]) < stations_wanted && world.can_build(builder, roster.engineering_stations[0]) {
                if let Some(cell) = self.station_site(world, forces, planned) {
                    return Some((roster.engineering_stations[0], cell));
                }
            }
            // A strategic missile defense once the enemy has been seen with a launcher.
            if self.enemy_nuke_seen
                && forces.strategic_defenses.len() + count_planned(roster.strategic_defense) == 0
                && world.can_build(builder, roster.strategic_defense)
            {
                if let Some(cell) = self.site_near_home(world, roster.strategic_defense, 1, planned) {
                    return Some((roster.strategic_defense, cell));
                }
            }
            if self.skill.missiles
                && mass_income >= NUKE_MASS_INCOME
                && energy_income >= NUKE_ENERGY_INCOME
                && forces.strategic_launchers.len() + count_planned(roster.strategic_launcher) == 0
                && world.can_build(builder, roster.strategic_launcher)
            {
                if let Some(cell) = self.site_near_home(world, roster.strategic_launcher, 1, planned) {
                    return Some((roster.strategic_launcher, cell));
                }
            }
        }

        // Overcharge needs 7,500 energy in store, more than a commander holds by itself, so an AI
        // that overcharges puts up one energy storage once its income can fill it.
        if self.skill.overcharge
            && energy_income >= OVERCHARGE_STORAGE_INCOME
            && forces.energy_storages + count_planned(roster.energy_storage) == 0
            && world.can_build(builder, roster.energy_storage)
        {
            if let Some(cell) = self.site_near_home(world, roster.energy_storage, 1, planned) {
                return Some((roster.energy_storage, cell));
            }
        }

        if economy.mass > economy.mass_capacity * 7 / 10 {
            if forces.storages < forces.extractors.len() * 2 {
                if let Some(cell) = self.storage_site(world, forces, planned) {
                    return Some((roster.storage, cell));
                }
            }
            if forces.point_defenses + count_planned(roster.point_defense) < self.skill.max_point_defenses {
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
        let mass_income = self.planned_income((economy.mass_income * 10).to_num::<i64>());
        let energy_income = self.planned_income((economy.energy_income * 10).to_num::<i64>());
        let mut upgrade_under_way = forces.factories.iter().any(|factory| factory.upgrading);
        let mut engineers = forces.engineers;
        let mut by_tier = forces.engineers_by_tier;
        // Units already being made count against the cap too, or each busy factory adds one
        // past it. One making an engineer or upgrading counts as well, which only errs small.
        let busy = |factories: &[FactoryInfo]| factories.iter().filter(|factory| factory.finished && !factory.queue_is_empty).count();
        let mut army = forces.combat_units + busy(&forces.factories);
        let engineers_wanted = self.skill.max_engineers.min(2 + mass_income as usize / 5);
        let rally = self.rally_point();
        let headquarters = forces.headquarters_tier;
        // Mobile shields cost a great deal of energy to run, so there are never many, and none
        // are started while energy is short.
        let mobile_shield = self.roster().mobile_shield;
        let mut mobile_shields = world
            .units
            .iter()
            .filter(|&&(_, entity)| {
                world.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team == self.team)
                    && world.ecs.get::<&Blueprinted>(entity).is_ok_and(|unit| unit.0 == mobile_shield)
            })
            .count();
        let shields_affordable = economy.energy >= economy.energy_capacity / 2 && energy_income >= mass_income * 12;
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
            // A factory upgrades only while another goes on making units: by FAF's costs, an
            // upgrade to T3 takes minutes, and the only factory upgrading left the AI no army.
            let others_producing = forces.factories.iter().any(|other| other.id != factory.id && other.finished && !other.upgrading);
            let upgrade_wanted = others_producing && upgrade.is_some_and(|into| {
                let into = world.blueprints.get(into);
                let allowed = world.support_allowed(self.team, into.id) && (!into.support || headquarters >= into.tier);
                let income = match (into.tier, factory.tier < headquarters || factory.support) {
                    (2, false) => T2_INCOME,
                    (2, true) => SECOND_T2_INCOME,
                    _ => T3_INCOME,
                };
                // An upgrade draws a great deal of energy, so it waits for an economy that can feed it.
                // Started into a stall, a factory upgrade drags out everything else for minutes.
                // Ten energy a second for each of mass, up to what an upgrade itself draws: a big
                // economy's mass outgrows its energy, and held its factories at T2 for good. A
                // stall from spending all the mass, with energy to spare, does not hold it back:
                // the AI spends every bit of mass it makes, so that kind of stall never ends.
                let energy_to_spare = economy.satisfaction >= Res::ONE || economy.energy >= economy.energy_capacity / 2;
                let energy_ready = energy_to_spare && energy_income >= (mass_income * 10).min(UPGRADE_ENERGY_INCOME);
                allowed && energy_ready && mass_income >= income
            });
            let roster = self.roster();
            let engineer_tier = usize::from(tier.clamp(1, 3)) - 1;
            let item = if engineers < engineers_wanted {
                engineers += 1;
                by_tier[engineer_tier] += 1;
                roster.engineers[engineer_tier]
            } else if tier == 2 && by_tier[1] + by_tier[2] < self.skill.tier_engineers.0 {
                by_tier[1] += 1;
                roster.engineers[1]
            } else if tier == 3 && by_tier[2] < self.skill.tier_engineers.1 {
                by_tier[2] += 1;
                roster.engineers[2]
            } else if upgrade_wanted && !upgrade_under_way {
                upgrade_under_way = true;
                upgrade.expect("just checked")
            } else {
                if army >= self.skill.max_army {
                    continue;
                }
                army += 1;
                self.unit_counter += 1;
                let air_threat = self.air_threat(world);
                let roster = self.roster();
                let turn = self.unit_counter % 6;
                // Against aircraft, every fourth unit is anti-air.
                if air_threat && self.unit_counter.is_multiple_of(4) {
                    roster.mobile_anti_air[usize::from(tier >= 2)]
                } else {
                match tier {
                    1 => match self.unit_counter % 4 {
                        0 => roster.artillery,
                        2 => roster.medium_tank,
                        _ => roster.bot,
                    },
                    // A mobile shield goes with every six units, to cover the rest, up to two.
                    2 => [roster.tank, roster.gatling_bot, roster.tank, roster.amphibious, roster.mobile_shield, roster.missile_launcher][turn as usize],
                    _ => [roster.heavy_assault_bot, roster.heavy_artillery, roster.armored_assault_bot, roster.tank, roster.mobile_shield, roster.heavy_assault_bot][turn as usize],
                }
                }
            };
            let item = if item == mobile_shield {
                if mobile_shields < MAX_MOBILE_SHIELDS && shields_affordable {
                    mobile_shields += 1;
                    item
                } else if tier >= 3 {
                    self.roster().heavy_assault_bot
                } else {
                    self.roster().tank
                }
            } else {
                item
            };
            commands.push(SimCommand::QueueUnit { factory: factory.id, blueprint: item, count: 1 });
        }
    }

    /// Finds, once, the deep water near home where naval factories fit, and the deep water
    /// nearest the enemy's start. Ties go by the cell as seen on the plan, so that both sides
    /// of a mirrored map choose mirrored spots. Nothing, on a map without water.
    fn plan_naval(&self, world: &SimWorld) -> NavalPlan {
        if !world.terrain.has_water() {
            return NavalPlan { sites: Vec::new(), enemy_water: None };
        }
        let factory = self.roster().naval_factory;
        let size = footprint(world, factory);
        let cells = world.terrain.cells_per_side() as u32;
        let plan_cell = |cell: (u32, u32), size: i32| self.flip_site(world, cell_i32(cell), size);
        let reach = wide_mul(fp_int(NAVAL_REACH_UNITS), fp_int(NAVAL_REACH_UNITS));
        // Each candidate with its distance, its cell on the plan for ties, and its cell.
        type Candidate = (FpWide, (i32, i32), (u32, u32));
        let mut sites: Vec<Candidate> = Vec::new();
        let mut enemy_water: Option<Candidate> = None;
        for y in 0..cells {
            for x in 0..cells {
                if !world.terrain.cell_is_navigable(x, y) {
                    continue;
                }
                let center = cell_center((x, y));
                let to_enemy = center.distance_squared(&self.enemy_home);
                let candidate = (to_enemy, plan_cell((x, y), 1), (x, y));
                if enemy_water.is_none_or(|best| (candidate.0, candidate.1) < (best.0, best.1)) {
                    enemy_water = Some(candidate);
                }
                // A site is named by its corner, so its middle is a cell in from it.
                let middle = cell_center((x + size as u32 / 2, y + size as u32 / 2));
                let from_home = middle.distance_squared(&self.home);
                if from_home <= reach && from_home < middle.distance_squared(&self.enemy_home) && world.can_place(factory, x, y) {
                    sites.push((from_home, plan_cell((x, y), size), (x, y)));
                }
            }
        }
        sites.sort_unstable();
        NavalPlan { sites: sites.into_iter().map(|(_, _, cell)| cell).collect(), enemy_water: enemy_water.map(|(_, _, cell)| cell_center(cell)) }
    }

    /// The nearest site that is still free, with a cell of open water around it so that
    /// factories do not wall each other in.
    fn naval_site(&self, world: &SimWorld, factory: BlueprintId, planned: &[(u32, u32)]) -> Option<(u32, u32)> {
        let size = footprint(world, factory) as u32;
        let plan = self.naval_plan.as_ref()?;
        plan.sites.iter().copied().find(|&(x, y)| {
            let clear = (y.saturating_sub(1)..y + size + 1).all(|row| (x.saturating_sub(1)..x + size + 1).all(|column| world.terrain.cell_is_navigable(column, row) && !planned.contains(&(column, row))));
            clear && world.can_place(factory, x, y)
        })
    }

    /// Naval factories upgrade with the economy, one at a time, and build the fleet.
    fn command_naval_factories(&mut self, world: &SimWorld, forces: &Forces, commands: &mut Vec<SimCommand>) {
        let economy = world.economies[self.team.index()];
        let mass_income = self.planned_income((economy.mass_income * 10).to_num::<i64>());
        let energy_income = self.planned_income((economy.energy_income * 10).to_num::<i64>());
        let mut upgrade_under_way = forces.naval_factories.iter().any(|factory| factory.upgrading);
        let headquarters = forces.naval_headquarters_tier;
        let rally = self.naval_rally();
        let mut fleet = forces.naval_units + forces.naval_factories.iter().filter(|factory| factory.finished && !factory.queue_is_empty).count();
        for factory in forces.naval_factories.iter().filter(|factory| factory.finished) {
            if let Some(point) = rally.filter(|_| !self.rally_set_on.contains(&factory.id)) {
                self.rally_set_on.push(factory.id);
                commands.push(SimCommand::SetRally { factory: factory.id, point });
            }
            if !factory.queue_is_empty || economy.satisfaction < Res::ONE / 2 || economy.energy < economy.energy_capacity / 4 {
                continue;
            }
            let tier = if factory.support { factory.tier.min(headquarters) } else { factory.tier };
            let upgrade = world.blueprints.get(factory.blueprint).upgrades_to.filter(|&into| {
                let into = world.blueprints.get(into);
                // A fleet first: a factory that only upgrades puts nothing on the water.
                let (income, fleet) = if into.tier == 2 { (NAVAL_T2_INCOME, 3) } else { (NAVAL_T3_INCOME, 6) };
                world.support_allowed(self.team, into.id) && energy_income >= mass_income * 12 && mass_income >= income && forces.naval_units >= fleet
            });
            let item = match upgrade.filter(|_| !upgrade_under_way) {
                Some(into) => {
                    upgrade_under_way = true;
                    into
                }
                None => {
                    if fleet >= self.skill.max_fleet {
                        continue;
                    }
                    fleet += 1;
                    self.unit_counter += 1;
                    let roster = self.roster();
                    let turn = (self.unit_counter % 6) as usize;
                    match tier {
                        1 => [roster.frigate, roster.frigate, roster.submarine][turn % 3],
                        2 => [roster.destroyer, roster.torpedo_boat, roster.cruiser, roster.destroyer, roster.frigate, roster.shield_boat][turn],
                        _ => [roster.battleship, roster.destroyer, roster.battlecruiser, roster.cruiser, roster.torpedo_boat, roster.shield_boat][turn],
                    }
                }
            };
            commands.push(SimCommand::QueueUnit { factory: factory.id, blueprint: item, count: 1 });
        }
    }

    /// Where new ships wait: the water off the first naval site.
    fn naval_rally(&self) -> Option<Vec2Fp> {
        self.naval_plan.as_ref()?.sites.first().map(|&cell| cell_center(cell))
    }

    /// The fleet meets enemy ships that come near home, and otherwise sets out, once it is big
    /// enough, for the water nearest the enemy's start, shelling whatever it finds there.
    fn command_fleet(&mut self, forces: &Forces, sightings: &[Sighting], commands: &mut Vec<SimCommand>) {
        if forces.idle_naval.is_empty() {
            return;
        }
        let Some(plan) = self.naval_plan.as_ref() else {
            return;
        };
        let reach = fp_int(NAVAL_REACH_UNITS);
        let threat = sightings
            .iter()
            .filter(|enemy| enemy.is_naval && enemy.position.distance_squared(&self.home) <= wide_mul(reach, reach))
            .min_by_key(|enemy| (enemy.position.distance_squared(&self.home), enemy.id));
        if let Some(enemy) = threat {
            commands.push(SimCommand::AttackMove { units: forces.idle_naval.clone(), goal: enemy.position });
            return;
        }
        if forces.idle_naval.len() < self.fleet_size {
            return;
        }
        let Some(goal) = plan.enemy_water else {
            return;
        };
        commands.push(SimCommand::AttackMove { units: forces.idle_naval.clone(), goal });
        self.fleet_size = (self.fleet_size + 2).min(LARGEST_FLEET);
    }

    /// In a crisis, pauses its extractors, factories, and everything with upkeep, such as shields
    /// and radar, so that what energy there is goes into generators, and starts them again once
    /// energy is half full. Factories still making units, T3 ones above all, drew so much that
    /// new generators never finished, and a shield and a radar could take all of what little
    /// came in before anything was built.
    fn command_energy(&self, world: &SimWorld, forces: &Forces, commands: &mut Vec<SimCommand>) {
        let economy = &world.economies[self.team.index()];
        let (paused, running): (Vec<&ExtractorInfo>, Vec<&ExtractorInfo>) = forces.extractor_sites.iter().partition(|extractor| extractor.paused);
        let factories = forces.factories.iter().chain(&forces.air_factories).chain(&forces.naval_factories).filter(|factory| factory.finished).map(|factory| factory.id);
        let extractors: Vec<SimId> = forces.extractor_sites.iter().map(|extractor| extractor.id).collect();
        let upkeep = world
            .units
            .iter()
            .filter(|&&(id, entity)| {
                !extractors.contains(&id)
                    && world.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team == self.team)
                    && world.ecs.get::<&Construction>(entity).is_err()
                    && world.ecs.get::<&Blueprinted>(entity).is_ok_and(|unit| {
                        let unit = world.blueprints.get(unit.0);
                        unit.shield.is_some() || unit.upkeep > Res::ZERO
                    })
            })
            .map(|&(id, _)| id);
        let (paused_factories, running_factories): (Vec<SimId>, Vec<SimId>) = factories
            .chain(upkeep)
            .partition(|&id| world.unit_entity(id).is_some_and(|entity| world.ecs.get::<&crate::sim::components::Paused>(entity).is_ok()));
        if forces.energy_crisis && !(running.is_empty() && running_factories.is_empty()) {
            let units = running.iter().map(|extractor| extractor.id).chain(running_factories).collect();
            commands.push(SimCommand::SetPaused { units, paused: true });
        } else if economy.energy >= economy.energy_capacity / 2 && !(paused.is_empty() && paused_factories.is_empty()) {
            let units = paused.iter().map(|extractor| extractor.id).chain(paused_factories).collect();
            commands.push(SimCommand::SetPaused { units, paused: false });
        }
    }

    fn air_threat(&self, world: &SimWorld) -> bool {
        self.enemy_air_seen.is_some_and(|seen| world.current_tick.saturating_sub(seen) < AIR_THREAT_TICKS)
    }

    /// Air factories upgrade with the economy, one at a time, and make fighters and strike
    /// aircraft: mostly strike aircraft, unless enemy aircraft have been seen. Aircraft cost a
    /// great deal of energy, so none are started while energy is short.
    fn command_air_factories(&mut self, world: &SimWorld, forces: &Forces, commands: &mut Vec<SimCommand>) {
        let economy = world.economies[self.team.index()];
        let mass_income = self.planned_income((economy.mass_income * 10).to_num::<i64>());
        let energy_income = self.planned_income((economy.energy_income * 10).to_num::<i64>());
        let energy_is_low = economy.energy < economy.energy_capacity / 2;
        let mut upgrade_under_way = forces.air_factories.iter().any(|factory| factory.upgrading);
        let headquarters = forces.air_headquarters_tier;
        let air_threat = self.air_threat(world);
        let mut aircraft = forces.aircraft + forces.air_factories.iter().filter(|factory| factory.finished && !factory.queue_is_empty).count();
        // One light transport at a time, for drops.
        let mut transport_wanted = self.skill.transports
            && forces.transports.is_empty()
            && mass_income >= TRANSPORT_INCOME
            && self.transport_ordered.is_none_or(|at| world.current_tick >= at + TRANSPORT_RETRY_TICKS);
        for factory in forces.air_factories.iter().filter(|factory| factory.finished && factory.queue_is_empty) {
            if economy.satisfaction < Res::ONE / 2 || energy_is_low {
                continue;
            }
            if transport_wanted && aircraft < self.skill.max_aircraft {
                transport_wanted = false;
                aircraft += 1;
                self.transport_ordered = Some(world.current_tick);
                commands.push(SimCommand::QueueUnit { factory: factory.id, blueprint: self.roster().light_transport, count: 1 });
                continue;
            }
            let tier = if factory.support { factory.tier.min(headquarters) } else { factory.tier };
            let upgrade = world.blueprints.get(factory.blueprint).upgrades_to.filter(|&into| {
                let into = world.blueprints.get(into);
                let income = if into.tier == 2 { AIR_T2_INCOME } else { AIR_T3_INCOME };
                // By now mass always runs short, as the AI spends all it makes, so only energy is checked.
                world.support_allowed(self.team, into.id) && energy_income >= mass_income * 20 && mass_income >= income
            });
            let item = match upgrade.filter(|_| !upgrade_under_way) {
                Some(into) => {
                    upgrade_under_way = true;
                    into
                }
                None => {
                    if aircraft >= self.skill.max_aircraft {
                        continue;
                    }
                    aircraft += 1;
                    self.air_counter += 1;
                    let roster = self.roster();
                    let fighter = if tier >= 3 { roster.air_superiority } else { roster.interceptor };
                    let strike = match tier {
                        1 => roster.strike_aircraft[0],
                        2 if self.air_counter.is_multiple_of(4) => roster.fighter_bomber,
                        _ => roster.strike_aircraft[usize::from(tier.clamp(1, 3)) - 1],
                    };
                    // A third fighters, or two thirds against enemy aircraft.
                    let fighter_turn = if air_threat { !self.air_counter.is_multiple_of(3) } else { self.air_counter.is_multiple_of(3) };
                    if fighter_turn { fighter } else { strike }
                }
            };
            commands.push(SimCommand::QueueUnit { factory: factory.id, blueprint: item, count: 1 });
        }
    }

    /// Fighters patrol between home and the rally point, where they meet raiders. Strike
    /// aircraft gather at home and set out together, against what can be seen, or else an
    /// outlying deposit or the enemy's base, and fly home afterwards.
    fn command_air(&mut self, world: &SimWorld, forces: &Forces, sightings: &[Sighting], commands: &mut Vec<SimCommand>) {
        if !forces.idle_fighters.is_empty() {
            commands.push(SimCommand::Patrol { units: forces.idle_fighters.clone(), points: vec![self.home, self.rally_point()], queued: false });
        }
        if forces.idle_strike.len() < self.air_wave_size {
            return;
        }
        // Structures first, which cannot dodge, then anything else on the ground.
        let target = sightings
            .iter()
            .filter(|enemy| !enemy.is_air)
            .min_by_key(|enemy| (!enemy.is_structure, enemy.position.distance_squared(&self.home), enemy.id))
            .map(|enemy| enemy.position);
        let goal = target.unwrap_or_else(|| self.search_point(world, false));
        commands.push(SimCommand::AttackMove { units: forces.idle_strike.clone(), goal });
        commands.push(SimCommand::QueueMove { units: forces.idle_strike.clone(), goal: self.home, attack: false });
        self.air_wave_size = (self.air_wave_size + 2).min(LARGEST_AIR_WAVE);
    }

    /// Extractors near home upgrade first, a few at a time as income grows, unless energy is short.
    fn command_upgrades(&mut self, world: &SimWorld, forces: &Forces, commands: &mut Vec<SimCommand>) {
        let economy = world.economies[self.team.index()];
        let mass_income = self.planned_income((economy.mass_income * 10).to_num::<i64>());
        // Upgrades pay for themselves, so they go ahead while factories spend all the mass, but
        // not into a stall, which would slow everything.
        if mass_income < EXTRACTOR_T2_INCOME || economy.satisfaction < Res::ONE * 9 / 10 {
            return;
        }
        let energy_is_low = economy.energy < economy.energy_capacity * 2 / 5 && economy.energy_demand > economy.energy_income;
        if energy_is_low {
            return;
        }
        // One radar goes to T2, for its reach, once the income can carry its upkeep.
        let has_t2_radar = forces.radars > forces.radar_sites.len() || forces.radar_sites.iter().any(|&(_, upgrading)| upgrading);
        if mass_income >= RADAR_T2_INCOME && !has_t2_radar {
            if let Some(&(radar, _)) = forces.radar_sites.first() {
                commands.push(SimCommand::Upgrade { units: vec![radar] });
            }
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

    /// Beside the first finished land factory, so that a station reaches what it builds, or
    /// near home if there is none.
    fn station_site(&self, world: &SimWorld, forces: &Forces, planned: &[(u32, u32)]) -> Option<(u32, u32)> {
        let station = self.roster().engineering_stations[0];
        match forces.factories.iter().find(|factory| factory.finished) {
            Some(factory) => {
                // The search runs on the plan, which Red sees turned around: a factory's cell
                // given as it is on the map sent Red's engineers to build beside Blue's base.
                let size = footprint(world, factory.blueprint);
                // The footprint's exact middle, which turns into its partner's.
                let edge = |cell: u32| fp_int(cell as i32 * TERRAIN_CELL_UNITS + size * TERRAIN_CELL_UNITS / 2);
                let middle = Vec2Fp::new(edge(factory.cell.0), edge(factory.cell.1));
                let on_plan = self.plan_position(world, middle);
                self.search_site(world, station, 1, cell_i32(cell_of(on_plan)), planned)
            }
            None => self.site_near_home(world, station, 1, planned),
        }
    }

    /// The commander's enhancements, one at a time, while it is idle at home and no enemy is near: the
    /// T2 engineering suite, then the heavy cannon, then the T3 suite, each once the income can
    /// carry it without a stall.
    fn command_enhancements(&self, world: &SimWorld, forces: &Forces, sightings: &[Sighting], commands: &mut Vec<SimCommand>) {
        let Some(commander) = forces.commander else {
            return;
        };
        let Some(entity) = world.unit_entity(commander) else {
            return;
        };
        if world.ecs.get::<&crate::sim::components::Enhancing>(entity).is_ok() {
            return;
        }
        // Only when it has nothing else to do: starting one drops what it was building, and a
        // naval factory dropped that way never went up.
        let idle = forces.idle_builders.iter().any(|builder| builder.id == commander);
        let given_work = commands.iter().any(|command| matches!(command, SimCommand::Build { units, .. } | SimCommand::Assist { units, .. } | SimCommand::Reclaim { units, .. } if units.contains(&commander)));
        if !idle || given_work {
            return;
        }
        let economy = &world.economies[self.team.index()];
        let mass_income = self.planned_income((economy.mass_income * 10).to_num::<i64>());
        let energy_is_low = economy.energy < economy.energy_capacity * 2 / 5 && economy.energy_demand > economy.energy_income;
        if mass_income < COMMANDER_T2_SUITE_INCOME || economy.satisfaction < Res::ONE * 9 / 10 || energy_is_low || forces.energy_crisis {
            return;
        }
        let at = world.ecs.get::<&Position>(entity).expect("units have positions").0.xy();
        let leash = fp_int(COMMANDER_LEASH_UNITS);
        if at.distance_squared(&self.home) > wide_mul(leash, leash) {
            return;
        }
        let danger = fp_int(DEFEND_RADIUS_UNITS);
        if sightings.iter().any(|enemy| !enemy.is_air && enemy.position.distance_squared(&at) <= wide_mul(danger, danger)) {
            return;
        }
        let roster = self.roster();
        let installed = world.ecs.get::<&crate::sim::components::Enhancements>(entity).map_or([None; 3], |slots| slots.0);
        let has = |enhancement| installed.contains(&Some(enhancement));
        let next = if !has(roster.t2_suite) && !has(roster.t3_suite) {
            Some(roster.t2_suite)
        } else if !has(roster.heavy_cannon) && mass_income >= COMMANDER_CANNON_INCOME {
            Some(roster.heavy_cannon)
        } else if has(roster.t2_suite) && mass_income >= COMMANDER_T3_SUITE_INCOME {
            Some(roster.t3_suite)
        } else {
            None
        };
        if let Some(enhancement) = next {
            commands.push(SimCommand::Enhance { units: vec![commander], enhancement });
        }
    }

    /// Transport drops: an idle light transport at home takes on a few idle T1 land units,
    /// sets them down short of an outlying enemy extractor, sends them at it, and flies home.
    /// The units it takes are kept out of the army's orders while they board.
    fn command_drops(&mut self, world: &SimWorld, forces: &mut Forces, sightings: &[Sighting], commands: &mut Vec<SimCommand>) {
        let tick = world.current_tick;
        let Some(transport) = forces.transports.first().copied() else {
            self.drop = None;
            return;
        };
        let Some(mut drop) = self.drop.take().filter(|drop| drop.transport == transport.id) else {
            // A new drop, from home, once there are enough to make it worth it.
            let leash = fp_int(COMMANDER_LEASH_UNITS * 2);
            if !transport.idle || transport.cargo > 0 || transport.position.distance_squared(&self.home) > wide_mul(leash, leash) {
                return;
            }
            let mut nearby = forces.idle_light_combat.clone();
            nearby.sort_by_key(|(id, at)| (at.distance_squared(&transport.position), *id));
            let passengers: Vec<SimId> = nearby.into_iter().take(DROP_SIZE).map(|(id, _)| id).collect();
            if passengers.len() < FEWEST_DROPPED {
                return;
            }
            forces.idle_combat.retain(|id| !passengers.contains(id));
            commands.push(SimCommand::Load { units: passengers.clone(), transport: transport.id });
            self.drop = Some(Drop { transport: transport.id, passengers, target: self.drop_target(world, sightings), phase: DropPhase::Boarding, since: tick });
            return;
        };
        // Its passengers are the transport's while the drop lasts.
        forces.idle_combat.retain(|id| !drop.passengers.contains(id));
        match drop.phase {
            DropPhase::Boarding => {
                let all_aboard = transport.cargo >= drop.passengers.len();
                let waited = tick >= drop.since + BOARDING_PATIENCE_TICKS;
                if all_aboard || (waited && transport.cargo > 0) {
                    let toward_home = (self.home - drop.target).normalized().scaled(fp_int(DROP_SHORT_UNITS));
                    commands.push(SimCommand::Unload { transports: vec![transport.id], point: drop.target + toward_home });
                    drop.phase = DropPhase::Flying;
                    drop.since = tick;
                } else if waited {
                    // Nobody came: give the units back to the army.
                    return;
                }
            }
            DropPhase::Flying => {
                if transport.cargo == 0 && !transport.unloading {
                    commands.push(SimCommand::AttackMove { units: drop.passengers.clone(), goal: drop.target });
                    commands.push(SimCommand::Move { units: vec![transport.id], goal: self.home });
                    drop.phase = DropPhase::Returning;
                    drop.since = tick;
                }
            }
            DropPhase::Returning => {
                if transport.idle {
                    return;
                }
            }
        }
        self.drop = Some(drop);
    }

    /// Where a drop goes: the enemy extractor in sight farthest from the enemy's base, if it is
    /// outlying, or else the outlying deposit on the enemy's side nearest home.
    fn drop_target(&self, world: &SimWorld, sightings: &[Sighting]) -> Vec2Fp {
        let outlying = fp_int(RAID_DISTANCE_UNITS);
        let is_extractor = |id: SimId| {
            world.unit_entity(id).and_then(|entity| world.ecs.get::<&Blueprinted>(entity).ok().map(|unit| world.blueprints.get(unit.0).requires_deposit)).unwrap_or(false)
        };
        let seen = sightings
            .iter()
            .filter(|enemy| enemy.is_structure && enemy.position.distance_squared(&self.enemy_home) > wide_mul(outlying, outlying) && is_extractor(enemy.id))
            .max_by_key(|enemy| (enemy.position.distance_squared(&self.enemy_home), std::cmp::Reverse(enemy.id)));
        if let Some(enemy) = seen {
            return enemy.position;
        }
        world
            .deposits
            .iter()
            .map(|&cell| cell_center(cell))
            .filter(|center| {
                let from_enemy = center.distance_squared(&self.enemy_home);
                from_enemy < center.distance_squared(&self.home) && from_enemy > wide_mul(outlying, outlying)
            })
            .min_by_key(|center| {
                // Ties broken the same way on both sides of a mirrored map.
                let seen = self.plan_position(world, *center);
                (center.distance_squared(&self.home), seen.x, seen.y)
            })
            .unwrap_or(self.enemy_home)
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
        // The land army cannot catch aircraft, which are the anti-air's and the fighters' job.
        let threat = sightings
            .iter()
            .filter(|enemy| !enemy.is_air && !enemy.is_submerged && enemy.position.distance_squared(&self.home) <= wide_mul(defend, defend))
            .min_by_key(|enemy| (enemy.position.distance_squared(&self.home), enemy.id));
        if let Some(enemy) = threat {
            commands.push(SimCommand::AttackMove { units: forces.idle_combat.clone(), goal: enemy.position });
            return;
        }
        // With a radar, something unseen coming close is met as well, unless it flies.
        let blips = if forces.radars > 0 { world.blips_for(self.team) } else { Vec::new() };
        let blip = blips
            .into_iter()
            .map(|[x, y, z]| (Vec2Fp::new(Fp::from_bits(x), Fp::from_bits(y)), Fp::from_bits(z)))
            .filter(|&(at, z)| z <= world.terrain.height_at(at).max(world.terrain.water_level()) + fp_int(BLIP_GROUND_HEIGHT))
            .filter(|(at, _)| at.distance_squared(&self.home) <= wide_mul(defend, defend))
            .min_by_key(|(at, _)| {
                let seen = self.plan_position(world, *at);
                (at.distance_squared(&self.home), seen.x, seen.y)
            });
        if let Some((at, _)) = blip {
            commands.push(SimCommand::AttackMove { units: forces.idle_combat.clone(), goal: at });
            return;
        }
        if world.current_tick < self.skill.first_attack_tick {
            return;
        }
        let gathered = forces.idle_combat.len();
        if gathered > self.gathered.0 {
            self.gathered = (gathered, world.current_tick);
        }
        let stopped_growing = world.current_tick - self.gathered.1 >= WAVE_PATIENCE_TICKS;
        let sudden_death = world.current_tick >= SUDDEN_DEATH_TICKS.max(self.skill.commander_push_ticks);
        if sudden_death {
            let enemy_commander = world.commanders[self.team.enemy().index()];
            let goal = sightings.iter().find(|enemy| Some(enemy.id) == enemy_commander && !enemy.is_submerged).map_or(self.enemy_home, |enemy| enemy.position);
            let mut everyone = forces.idle_combat.clone();
            if !commander_enhancing(world, forces) && world.terrain.height_at(goal) >= world.terrain.water_level() {
                everyone.extend(forces.commander);
            }
            commands.push(SimCommand::AttackMove { units: everyone, goal });
            self.gathered = (0, world.current_tick);
            return;
        }
        if gathered < self.wave_size && !(stopped_growing && gathered >= self.skill.first_wave) {
            return;
        }
        // Send a wave at what can be seen, then the enemy's base, then search their deposits.
        let commander = world.commanders[self.team.enemy().index()];
        // Nothing under the water, which the army cannot hit: two commanders that followed
        // their waves onto the seabed once stalked each other there until the time ran out.
        let reachable = |enemy: &&Sighting| !enemy.is_air && !enemy.is_submerged;
        let target = sightings
            .iter()
            .filter(reachable)
            .find(|enemy| Some(enemy.id) == commander)
            .or_else(|| sightings.iter().filter(reachable).find(|enemy| enemy.is_structure))
            .or_else(|| sightings.iter().find(reachable))
            .map(|enemy| enemy.position);
        let late = world.current_tick >= LATE_PUSH_TICKS.max(self.skill.commander_push_ticks);
        let full_strength = gathered >= self.skill.largest_wave || (late && gathered >= self.skill.first_wave * 2);
        let goal = target.unwrap_or_else(|| self.search_point(world, full_strength));
        // Not in formation: waves at the slowest unit's pace arrived late enough that, in 200
        // AI-vs-AI matches, 6 went past the time limit, against none with each at its own speed.
        let mut wave = forces.idle_combat.clone();
        // A long, even game is one a defended base keeps winning. The commander joins a
        // full-size wave, which either breaks the enemy's base or loses the commander.
        let on_land = world.terrain.height_at(goal) >= world.terrain.water_level();
        // Not while it is fitting an enhancement, which an order would throw away.
        if full_strength && world.current_tick >= self.skill.commander_push_ticks && on_land && !commander_enhancing(world, forces) {
            wave.extend(forces.commander);
        }
        commands.push(SimCommand::AttackMove { units: wave, goal });
        if gathered >= self.wave_size {
            self.wave_size = (self.wave_size + self.wave_size / 4 + 1).min(self.skill.largest_wave);
        }
        self.gathered = (0, world.current_tick);
    }

    /// Tactical missile launchers build missiles one after another, and fire them at enemy
    /// structures in sight and in range: extractors first, which a missile or two destroys,
    /// then the nearest.
    fn command_missiles(&self, world: &SimWorld, forces: &Forces, sightings: &[Sighting], commands: &mut Vec<SimCommand>) {
        let roster = self.roster();
        // Defenses keep interceptors coming, and fire them on their own.
        for defense in forces.strategic_defenses.iter().filter(|defense| defense.finished && defense.queue_is_empty) {
            commands.push(SimCommand::QueueUnit { factory: defense.id, blueprint: roster.anti_nuke, count: 1 });
            commands.push(SimCommand::SetRepeat { factory: defense.id, repeat: true });
        }
        // A launcher keeps building missiles, and fires each at the costliest enemy structure in
        // sight, or with none in sight, at the enemy's start, which every player knows.
        let worth = |id: SimId| {
            world.unit_entity(id).and_then(|entity| world.ecs.get::<&Blueprinted>(entity).ok().map(|unit| world.blueprints.get(unit.0).mass)).unwrap_or(Res::ZERO)
        };
        for launcher in forces.strategic_launchers.iter().filter(|launcher| launcher.finished) {
            if launcher.queue_is_empty {
                commands.push(SimCommand::QueueUnit { factory: launcher.id, blueprint: roster.strategic_missile, count: 1 });
                commands.push(SimCommand::SetRepeat { factory: launcher.id, repeat: true });
            }
            if launcher.busy || launcher.stock == 0 {
                continue;
            }
            let target = sightings.iter().filter(|enemy| enemy.is_structure).max_by_key(|enemy| (worth(enemy.id), std::cmp::Reverse(enemy.id)));
            match target {
                Some(enemy) => commands.push(SimCommand::AttackGround { units: vec![launcher.id], point: enemy.position }),
                None => commands.push(SimCommand::AttackGround { units: vec![launcher.id], point: self.enemy_home }),
            }
        }
        for launcher in forces.tactical_launchers.iter().filter(|launcher| launcher.finished) {
            if launcher.queue_is_empty {
                commands.push(SimCommand::QueueUnit { factory: launcher.id, blueprint: roster.tactical_missile, count: 1 });
                commands.push(SimCommand::SetRepeat { factory: launcher.id, repeat: true });
            }
            if launcher.busy || launcher.stock == 0 {
                continue;
            }
            let Some(range) = world.unit_entity(launcher.id).and_then(|entity| world.ecs.get::<&Combat>(entity).ok().map(|combat| combat.range)) else {
                continue;
            };
            let is_extractor = |id: SimId| {
                world.unit_entity(id).and_then(|entity| world.ecs.get::<&Blueprinted>(entity).ok().map(|unit| world.blueprints.get(unit.0).requires_deposit)).unwrap_or(false)
            };
            let target = sightings
                .iter()
                .filter(|enemy| enemy.is_structure && enemy.position.distance_squared(&launcher.position) <= wide_mul(range, range))
                .min_by_key(|enemy| (!is_extractor(enemy.id), enemy.position.distance_squared(&launcher.position), enemy.id));
            if let Some(enemy) = target {
                commands.push(SimCommand::Attack { units: vec![launcher.id], target: enemy.id });
            }
        }
    }

    /// The commander overcharges the costliest enemy unit in its gun's range, other than the
    /// enemy commander, which takes little from it, when the energy the shot needs is in store
    /// and it is above half health. The shot spends what it takes to destroy its target.
    fn command_overcharge(&self, world: &SimWorld, forces: &Forces, sightings: &[Sighting], commands: &mut Vec<SimCommand>) {
        let Some(commander) = forces.commander else {
            return;
        };
        let Some(entity) = world.unit_entity(commander) else {
            return;
        };
        // An enemy this close is worth dropping an enhancement for: the commander is the match.
        if world.ecs.get::<&crate::sim::components::OverchargeOrder>(entity).is_ok() {
            return;
        }
        let Ok(weapon) = world.ecs.get::<&crate::sim::components::OverchargeWeapon>(entity).map(|weapon| *weapon) else {
            return;
        };
        let economy = &world.economies[self.team.index()];
        if weapon.ticks_until_ready > 0 || economy.energy < weapon.def.energy_required {
            return;
        }
        let health = *world.ecs.get::<&crate::sim::components::Health>(entity).expect("units have health");
        if wide_mul(health.current, fp_int(100)) < wide_mul(health.max, fp_int(OVERCHARGE_HEALTH_PERCENT)) {
            return;
        }
        let at = world.ecs.get::<&Position>(entity).expect("units have positions").0.xy();
        // Only what is already in range, so that a shot never draws the commander forward.
        let Some(reach) = world.ecs.get::<&Combat>(entity).ok().map(|combat| combat.range) else {
            return;
        };
        let enemy_commander = world.commanders[self.team.enemy().index()];
        let worth = |id: SimId| {
            world.unit_entity(id).and_then(|entity| world.ecs.get::<&Blueprinted>(entity).ok().map(|unit| world.blueprints.get(unit.0).mass)).unwrap_or(Res::ZERO)
        };
        let target = sightings
            .iter()
            .filter(|enemy| !enemy.is_structure && !enemy.is_air && Some(enemy.id) != enemy_commander && enemy.position.distance_squared(&at) <= wide_mul(reach, reach))
            .max_by_key(|enemy| (worth(enemy.id), std::cmp::Reverse(enemy.position.distance_squared(&at)), std::cmp::Reverse(enemy.id)));
        if let Some(enemy) = target {
            commands.push(SimCommand::Overcharge { unit: commander, target: enemy.id });
        }
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

/// Whether the team's commander is fitting an enhancement, which any order would cancel.
fn commander_enhancing(world: &SimWorld, forces: &Forces) -> bool {
    forces.commander.and_then(|id| world.unit_entity(id)).is_some_and(|entity| world.ecs.get::<&crate::sim::components::Enhancing>(entity).is_ok())
}

fn footprint(world: &SimWorld, blueprint: BlueprintId) -> i32 {
    i32::from(world.blueprints.get(blueprint).footprint)
}

/// Every cell a structure with its corner at `corner` covers. Planned sites are kept as their
/// whole footprints: a corner alone is the bottom-left on Blue's plan but the top-right on Red's,
/// turned about the map, so the two sides would keep different ground clear around it.
fn footprint_cells(world: &SimWorld, blueprint: BlueprintId, corner: (u32, u32)) -> impl Iterator<Item = (u32, u32)> {
    let size = world.blueprints.get(blueprint).footprint.max(1) as u32;
    (0..size * size).map(move |index| (corner.0 + index % size, corner.1 + index / size))
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
            is_air: entry.is_air,
            is_submerged: entry.is_sub,
            is_naval: entry.is_naval || entry.is_sub || (entry.is_structure && world.unit_entity(entry.id).is_some_and(|entity| world.ecs.get::<&Blueprinted>(entity).is_ok_and(|unit| world.blueprints.get(unit.0).on_water))),
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
