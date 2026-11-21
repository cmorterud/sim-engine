//! Unit and structure data, loaded from `data/units.toml` (GAME_SPECIFICATION.md §19).
//! The file is compiled in, so that every instance runs on identical data.

use serde::Deserialize;
use std::hash::Hasher;
use twox_hash::XxHash64;

use super::config::TERRAIN_CELL_UNITS;
use crate::math::fp::{fp_int, Fp, FpWide};

/// Economy totals, costs, and rates. 64-bit, because they can exceed the range of `Fp`.
pub type Res = FpWide;

const STANDARD_DATA: &str = include_str!("../../data/units.toml");
pub const TICKS_PER_SECOND: i64 = 10;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlueprintId(pub u16);

/// One of the commanders' field enhancements (GAME_SPECIFICATION.md §2.1).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnhancementId(pub u16);

/// Where an enhancement fits. A unit has one of each, so a new one replaces what is there.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    /// Weapon upgrades.
    Weapons,
    /// Building upgrades.
    Fabrication,
    /// Protection, sensors, repair, and the rest.
    Systems,
}

impl Slot {
    pub const ALL: [Self; 3] = [Self::Weapons, Self::Fabrication, Self::Systems];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Weapons => "Weapons",
            Self::Fabrication => "Fabrication",
            Self::Systems => "Systems",
        }
    }
}

/// A personal teleporter (GAME_SPECIFICATION.md §2.1): its charge costs energy and time that
/// grow with the distance, as in FAF.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TeleporterDef {
    /// Energy for a jump of no distance.
    pub flat_energy: Res,
    pub max_energy: Res,
    /// Charge time for a jump of no distance.
    pub min_ticks: u32,
    pub max_ticks: u32,
}

/// What an enhancement does. Each is a total over the unit's own blueprint, not a step over
/// the enhancement it replaces.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct EnhancementEffects {
    /// The new build power, if it changes.
    pub build_power: Option<Fp>,
    /// The tier of structures it can build, if higher than its own.
    pub build_tier: u8,
    /// Added to maximum health.
    pub health: Fp,
    /// Health per tick.
    pub regen_per_tick: Fp,
    /// The gun's new range, damage, reload, and blast, if they change.
    pub weapon_range: Option<Fp>,
    pub weapon_damage: Option<Fp>,
    pub weapon_reload_ticks: Option<u32>,
    pub weapon_splash: Option<Fp>,
    /// New sight and deep scan ranges, if they change.
    pub vision: Option<Fp>,
    pub deep_scan: Option<Fp>,
    /// Decoy blips on enemy radar, and the energy per tick they cost.
    pub jammer: Option<JammerDef>,
    pub jammer_upkeep: Option<Res>,
    pub shield: Option<ShieldDef>,
    /// Energy per tick while cloaked. A cloak hides from sight and radar both.
    pub cloak_upkeep: Option<Res>,
    /// Energy per tick to stay hidden from enemy radar.
    pub stealth_upkeep: Option<Res>,
    pub teleporter: Option<TeleporterDef>,
    /// Per tick.
    pub mass_income: Res,
    pub energy_income: Res,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Enhancement {
    pub id: EnhancementId,
    pub key: String,
    pub name: String,
    /// The units that can take it.
    pub units: Vec<BlueprintId>,
    pub slot: Slot,
    /// The enhancement it builds on, which must be in the slot already, and which it replaces.
    pub requires: Option<EnhancementId>,
    pub mass: Res,
    pub energy: Res,
    pub build_time: Res,
    pub effects: EnhancementEffects,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UnitClass {
    Commander,
    /// Built at a Rig Yard (GAME_SPECIFICATION.md §2.2). Its death does not end the match.
    FieldRig,
    /// Aircraft (GAME_SPECIFICATION.md §10).
    Air,
    Engineer,
    Bot,
    Artillery,
    Tank,
    Factory,
    Economy,
    Defense,
    /// A missile that a silo builds and keeps until it is fired. Never a unit on the map.
    Ammo,
}

/// What a weapon fires.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WeaponKind {
    /// A ballistic shell.
    Shell,
    /// A cruise missile that flies low over the ground to a structure (GAME_SPECIFICATION.md §8).
    /// A cruise missile, fired only on order, one for each missile built.
    TacticalMissile,
    /// A nuclear missile, fired only on order, one for each missile built, and once per order.
    StrategicMissile,
    /// A bomb, dropped from an aircraft as it passes over its target.
    Bomb,
    /// A flak shell: fired at where an aircraft will be if it keeps its course, it flies there
    /// straight and bursts, hitting every enemy aircraft within its blast. It does not steer,
    /// and the faster the aircraft and the longer the flight, the wider it scatters
    /// (`FLAK_LEAD_ERROR`), so that slow aircraft are caught and fast ones mostly are not.
    Flak,
    /// A gravity beam (GAME_SPECIFICATION.md §12): it lifts an enemy land unit, holds it, and
    /// crushes it once `crush_ticks` have passed.
    Tractor,
}

/// What a structure gives the finished structures of its team that touch it (GAME_SPECIFICATION.md
/// §3.2), as in FAF's `AdjacencyBuffs.lua`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AdjacencyGiver {
    T1Power,
    T2Power,
    T3Power,
    Geothermal,
    T1Extractor,
    T2Extractor,
    T3Extractor,
    T1Fabricator,
    T3Fabricator,
    MassStorage,
    EnergyStorage,
}

/// One neighbor's bonuses to a structure, in hundred-thousandths, by the structure's size class:
/// FAF's SIZE4, SIZE8, SIZE12, SIZE16, and SIZE20. Discounts are positive.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct AdjacencyBonus {
    /// Off the energy, and the mass, that what it builds or upgrades to costs.
    pub energy_active: [i32; 5],
    pub mass_active: [i32; 5],
    /// Off the energy it needs to keep running.
    pub upkeep: [i32; 5],
    /// Onto the mass and the energy it makes.
    pub mass_output: [i32; 5],
    pub energy_output: [i32; 5],
    /// For artillery: off the energy each shot costs, and off the time between shots.
    pub weapon_energy: [i32; 5],
    pub rate_of_fire: [i32; 5],
}

/// What adjacency values are measured in: 100,000 is all of it.
pub const ADJACENCY_SCALE: i64 = 100_000;

impl AdjacencyGiver {
    /// FAF's numbers, unchanged.
    pub const fn bonus(self) -> AdjacencyBonus {
        const NONE: [i32; 5] = [0; 5];
        let (weapon_energy, rate_of_fire) = match self {
            Self::T1Power => ([2_500, 1_250, 830, 625, 500], [4_000, 1_250, 830, 625, 500]),
            Self::T2Power | Self::Geothermal => ([5_000; 5], [6_250, 6_250, 6_250, 6_250, 2_500]),
            Self::T3Power => ([7_500; 5], [10_000, 10_000, 10_000, 10_000, 4_000]),
            _ => (NONE, NONE),
        };
        let (energy_active, upkeep, mass_active, mass_output, energy_output) = match self {
            Self::T1Power => ([6_250, 3_125, 2_080, 1_563, 250], [6_250, 3_125, 2_080, 1_563, 1_250], NONE, NONE, NONE),
            Self::T2Power | Self::Geothermal => ([12_500; 5], [12_500; 5], NONE, NONE, NONE),
            Self::T3Power => ([18_750, 18_750, 18_750, 15_620, 5_000], [18_750; 5], NONE, NONE, NONE),
            Self::T1Extractor => (NONE, NONE, [10_000, 5_000, 3_330, 7_500, 7_500], NONE, NONE),
            Self::T2Extractor => (NONE, NONE, [15_000, 7_500, 5_000, 10_000, 10_000], NONE, NONE),
            Self::T3Extractor => (NONE, NONE, [20_000, 10_000, 6_670, 12_500, 12_500], NONE, NONE),
            Self::T1Fabricator => (NONE, NONE, [2_500, 1_250, 833, 1_250, 750], NONE, NONE),
            Self::T3Fabricator => (NONE, NONE, [20_000, 20_000, 12_500, 20_000, 3_750], NONE, NONE),
            Self::MassStorage => (NONE, NONE, NONE, [12_500, 6_250, 3_000, 3_125, 2_500], NONE),
            Self::EnergyStorage => (NONE, NONE, NONE, NONE, [25_000, 12_500, 8_334, 6_250, 5_000]),
        };
        AdjacencyBonus { energy_active, mass_active, upkeep, mass_output, energy_output, weapon_energy, rate_of_fire }
    }
}

/// What a factory builds: land units or aircraft. Headquarters and support factories of a kind
/// go together (GAME_SPECIFICATION.md §15).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FactoryKind {
    Land,
    Air,
    Naval,
}

/// What a weapon can aim at: things on the ground, aircraft, or both.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Targets {
    Ground,
    Air,
    Both,
    /// Torpedoes: ships and submarines only.
    Naval,
}

/// How an aircraft flies (GAME_SPECIFICATION.md §10).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FlightDef {
    /// How high above the ground it flies.
    pub elevation: Fp,
    /// Degrees per second. Planes keep flying, so they turn in arcs, and circle when idle.
    pub turn_rate: u32,
    /// How far past its target it flies before turning back for another pass.
    pub break_off: Fp,
    /// Ticks of flight on a full tank.
    pub fuel_ticks: u32,
    /// A gunship hovers over what it attacks, instead of making passes.
    pub hovers: bool,
}

/// An aircraft that carries land units (GAME_SPECIFICATION.md §10).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TransportDef {
    /// Room by size of what it carries: T1 land units, T2, and T3 and field rigs.
    /// Mixed loads share the room: each small unit takes a `small`th of it, and so on.
    pub slots: [u32; 3],
}

/// A pad where aircraft land to refuel and be repaired (GAME_SPECIFICATION.md §10).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AirfieldDef {
    /// Room for this many T1 aircraft. A T2 aircraft takes two, and a T3 one four.
    pub slots: u8,
    /// Health given back to each aircraft on it, per tick.
    pub repair_per_tick: Fp,
    /// Energy each aircraft being repaired costs, per tick.
    pub energy_per_tick: Res,
}



#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WeaponDef {
    pub range: Fp,
    pub reload_ticks: u32,
    pub projectile_speed: Fp,
    pub damage: Fp,
    pub splash_radius: Fp,
    /// How fast the weapon swivels, in degrees per second. `None` fires in any direction at once.
    pub turn_rate: Option<u32>,
    pub kind: WeaponKind,
    /// How high a missile flies above the ground. Zero for shells.
    pub cruise_height: Fp,
    pub targets: Targets,
    /// A nuclear blast's inner ring, on top of `damage` over all of `splash_radius`. Zero for none.
    pub inner_radius: Fp,
    pub inner_damage: Fp,
    /// Artillery: it cannot hit what is closer than `min_range`, its shells land up to `spread`
    /// from where they are aimed, and each shot costs `energy_per_shot`, if any.
    pub artillery: bool,
    pub min_range: Fp,
    pub spread: Fp,
    pub energy_per_shot: Res,
    /// A missile weapon that picks its own targets and needs no missiles built, as a ship's do.
    pub automatic: bool,
    /// An anti-air missile, which steers itself to its aircraft. Anti-air guns and flak do not:
    /// their shells fly straight to where the aircraft will be if it holds its course.
    pub homing: bool,
    /// EMP (GAME_SPECIFICATION.md §12): what it hits, directly or in its blast, is stunned this long.
    pub stun_ticks: u32,
    /// For a tractor beam: how long it holds a unit before crushing it.
    pub crush_ticks: u32,
    /// Where its turret sits, from the unit's middle, as `[right, forward]` in units: its shots
    /// leave from there. Zero for the middle.
    pub mount: [Fp; 2],
    /// Its turret faces the stern while it has nothing to aim at.
    pub aft: bool,
    /// Artillery that must set up before it fires: it stands still this long first, and moving
    /// at all packs it up again. Zero fires on the move.
    pub setup_ticks: u32,
}

/// A storm a unit leaves where it is destroyed (GAME_SPECIFICATION.md §12), that roams on its
/// own and damages every enemy it covers.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StormDef {
    pub radius: Fp,
    /// Per tick, to each enemy under it.
    pub damage_per_tick: Fp,
    pub ticks: u32,
    /// Units per tick.
    pub speed: Fp,
}

/// A defense that destroys enemy missiles in flight.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MissileDefenseDef {
    /// Strategic defenses stop nuclear missiles, using one interceptor each, which their silo
    /// builds. Tactical ones stop cruise missiles, one at a time, as often as they reload.
    pub strategic: bool,
    pub range: Fp,
    pub reload_ticks: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TorpedoDefenseDef {
    pub range: Fp,
    pub reload_ticks: u32,
}

/// The commander's Discharge: a shot paid for from stored energy, that destroys almost any
/// mobile unit but does little to structures.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
/// The commander's Discharge, by FAF's rule (GAME_SPECIFICATION.md §2.1): it fires only with
/// `energy_required` in store, and when the shell lands, it spends what it needs to destroy
/// the toughest enemy where it lands, within `min_damage` and `max_damage` and what a share
/// of the store can pay for, at `energy_per_damage` a point.
pub struct DischargeDef {
    pub energy_required: Res,
    /// The most of the store one shot may spend, in thousandths.
    pub energy_share_permille: u32,
    pub energy_per_damage: u32,
    pub min_damage: Fp,
    pub max_damage: Fp,
    /// What it does to a commander, and to a structure or a structure's dome, at most.
    pub commander_damage: Fp,
    pub structure_damage: Fp,
    /// Its blast, and how far round where it lands it looks for the toughest enemy.
    pub splash_radius: Fp,
    pub search_radius: Fp,
    pub reload_ticks: u32,
}

/// A shield generator's dome (GAME_SPECIFICATION.md §8.1).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ShieldDef {
    pub radius: Fp,
    pub health: Fp,
    pub regen_per_tick: Fp,
    /// How long a dome that has collapsed stays down before it comes back at full strength.
    pub recharge_ticks: u32,
    /// Energy per tick while the dome is up, before any discount from neighbors.
    pub upkeep: Res,
}

/// Decoy blips on enemy radar (GAME_SPECIFICATION.md §9).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct JammerDef {
    /// How far from the jammer the decoys appear.
    pub radius: Fp,
    pub decoys: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ExplosionDef {
    pub damage: Fp,
    pub radius: Fp,
    /// Only if it was running when destroyed: built, not paused, and with energy to run on.
    pub only_while_running: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blueprint {
    pub id: BlueprintId,
    pub key: String,
    pub name: String,
    pub class: UnitClass,
    pub tier: u8,
    pub mass: Res,
    pub energy: Res,
    /// Build power multiplied by seconds needed to finish one.
    pub build_time: Res,
    pub health: Fp,
    /// Units per tick. Zero for structures.
    pub speed: Fp,
    /// How fast the body turns, in degrees per second. `None` turns instantly.
    pub turn_rate: Option<u32>,
    /// The tightest arc it can turn in while moving, in units. Zero turns on the spot.
    pub turn_radius: u32,
    pub radius: Fp,
    /// Structure width in terrain cells. Zero for mobile units.
    pub footprint: u8,
    pub vision: Fp,
    pub build_power: Fp,
    pub build_range: Fp,
    pub builds: Vec<BlueprintId>,
    pub upgrades_to: Option<BlueprintId>,
    pub weapon: Option<WeaponDef>,
    /// More weapons than the main one, each picking its own targets.
    pub extra_weapons: Vec<WeaponDef>,
    /// For aircraft.
    pub flight: Option<FlightDef>,
    pub requires_deposit: bool,
    /// Built over a geothermal vent, which must be under the middle of its footprint.
    pub requires_vent: bool,
    /// A structure built on deep water, such as a naval factory, instead of on land.
    pub on_water: bool,
    /// How it moves when it is not flying: over land, on deep water, under it, hovering over
    /// both land and water, or walking over land and along the seabed.
    pub movement: super::components::MoveClass,
    /// Per tick.
    pub mass_income: Res,
    /// Per tick.
    pub energy_income: Res,
    pub mass_storage: Res,
    pub energy_storage: Res,
    pub death_explosion: Option<ExplosionDef>,
    pub death_storm: Option<StormDef>,
    pub discharge: Option<DischargeDef>,
    pub shield: Option<ShieldDef>,
    pub missile_defense: Option<MissileDefenseDef>,
    /// Anti-torpedo charges: they shoot down enemy torpedoes within range, one each reload.
    pub torpedo_defense: Option<TorpedoDefenseDef>,
    pub airfield: Option<AirfieldDef>,
    /// An air transport, which carries land units (GAME_SPECIFICATION.md §10).
    pub transport: Option<TransportDef>,
    /// For factories.
    pub factory_kind: Option<FactoryKind>,
    /// A support factory: cheaper, and it needs a headquarters of its kind and tier.
    pub support: bool,
    /// A field workshop: a structure whose build power helps whatever friendly
    /// construction, production, or repair is within its build range, by itself.
    pub assists: bool,
    pub adjacency_giver: Option<AdjacencyGiver>,
    /// FAF's size class, from 0 for SIZE4 to 4 for SIZE20, which picks the adjacency bonuses it gets.
    pub adjacency_size: usize,
    /// Sensor ranges (GAME_SPECIFICATION.md §9). Zero for none.
    pub radar: Fp,
    /// Reveals submerged submarines within it (GAME_SPECIFICATION.md §10). Zero for none.
    pub sonar: Fp,
    pub deep_scan: Fp,
    /// Friendly units within this range of it make no radar blip. Zero for none.
    pub stealth_field: Fp,
    pub jammer: Option<JammerDef>,
    /// Energy per tick while cloaked, for a unit that can cloak.
    pub cloak_upkeep: Option<Res>,
    /// Energy per tick while running, for intel structures and mass extractors. Zero for none.
    pub upkeep: Res,
    /// For a silo, the most missiles it keeps. It builds no more until it fires one. Zero for no limit.
    pub max_stock: u32,
}

impl Blueprint {
    /// Whether it does anything for intel: sensors, stealth, jamming, or cloaking.
    pub fn has_intel(&self) -> bool {
        self.radar > Fp::ZERO || self.deep_scan > Fp::ZERO || self.sonar > Fp::ZERO || self.stealth_field > Fp::ZERO || self.jammer.is_some() || self.cloak_upkeep.is_some()
    }

    pub const fn is_structure(&self) -> bool {
        self.footprint > 0
    }

    pub const fn is_air(&self) -> bool {
        self.flight.is_some()
    }

    /// How it moves: through the air, on the water, or over land.
    pub const fn move_class(&self) -> super::components::MoveClass {
        if self.flight.is_some() {
            super::components::MoveClass::Air
        } else {
            self.movement
        }
    }

    /// A factory upgraded to T2 or T3, which support factories of its kind need.
    pub fn is_headquarters(&self) -> bool {
        self.factory_kind.is_some() && !self.support && self.tier >= 2
    }

    pub fn is_commander(&self) -> bool {
        self.class == UnitClass::Commander
    }

    pub fn is_factory(&self) -> bool {
        self.class == UnitClass::Factory
    }

    /// Whether this unit is meant to fight, as opposed to build or produce.
    pub fn is_combat_unit(&self) -> bool {
        matches!(self.class, UnitClass::Bot | UnitClass::Artillery | UnitClass::Tank)
    }
}

/// A whole number or a quoted decimal. There is deliberately no float variant:
/// a TOML float fails to parse rather than silently rounding.
#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
enum Number {
    Whole(i64),
    Text(String),
}

impl Number {
    fn fp(&self) -> Result<Fp, String> {
        match self {
            Self::Whole(value) => i32::try_from(*value)
                .ok()
                .filter(|value| value.abs() < (1 << 19))
                .map(fp_int)
                .ok_or_else(|| format!("{value} is out of range")),
            Self::Text(text) => Fp::from_str(text).map_err(|_| format!("{text:?} is not a number")),
        }
    }

    fn res(&self) -> Result<Res, String> {
        match self {
            Self::Whole(value) => Res::checked_from_num(*value).ok_or_else(|| format!("{value} is out of range")),
            Self::Text(text) => Res::from_str(text).map_err(|_| format!("{text:?} is not a number")),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWeapon {
    range: Number,
    reload_ticks: u32,
    projectile_speed: Number,
    damage: Number,
    splash_radius: Option<Number>,
    /// How fast the weapon swivels, in degrees per second. Without one, it fires in any direction at once.
    turn_rate: Option<u32>,
    /// "shell", the default, "tactical_missile", or "strategic_missile".
    kind: Option<String>,
    cruise_height: Option<Number>,
    /// "ground", the default, "air", or "both".
    targets: Option<String>,
    inner_radius: Option<Number>,
    inner_damage: Option<Number>,
    #[serde(default)]
    artillery: bool,
    min_range: Option<Number>,
    spread: Option<Number>,
    energy_per_shot: Option<Number>,
    #[serde(default)]
    automatic: bool,
    #[serde(default)]
    homing: bool,
    stun_seconds: Option<u32>,
    crush_seconds: Option<u32>,
    /// Where its turret sits, as `[right, forward]` in units from the unit's middle.
    mount: Option<[Number; 2]>,
    /// Its turret rests facing the stern.
    #[serde(default)]
    aft: bool,
    /// Artillery only: ticks it stands still before it can fire.
    setup_ticks: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStorm {
    radius: Number,
    /// A second, to each enemy under it.
    damage: Number,
    seconds: u32,
    /// Units a tick.
    speed: Number,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFlight {
    elevation: Number,
    /// Degrees per second.
    turn_rate: u32,
    break_off: Option<Number>,
    /// Seconds of flight on a full tank.
    fuel: u32,
    #[serde(default)]
    hovers: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTransport {
    small: u32,
    medium: u32,
    large: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAirfield {
    slots: u8,
    /// Health a second, for each aircraft.
    repair: Number,
    /// Energy a second, for each aircraft being repaired.
    repair_energy: Number,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMissileDefense {
    /// "tactical" or "strategic".
    kind: String,
    range: Number,
    reload_ticks: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTorpedoDefense {
    range: Number,
    reload_ticks: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDischarge {
    energy_required: Number,
    energy_share_permille: u32,
    energy_per_damage: u32,
    min_damage: Number,
    max_damage: Number,
    commander_damage: Number,
    structure_damage: Number,
    splash_radius: Number,
    search_radius: Number,
    reload_ticks: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawShield {
    radius: Number,
    health: Number,
    /// Health per second.
    regen: Number,
    recharge_seconds: u32,
    /// Energy per second.
    upkeep: Number,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawJammer {
    radius: Number,
    decoys: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCloak {
    /// Energy per second while cloaked.
    upkeep: Number,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExplosion {
    damage: Number,
    radius: Number,
    #[serde(default)]
    only_while_running: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUnit {
    id: String,
    name: String,
    class: String,
    tier: u8,
    mass: Number,
    energy: Number,
    build_time: Number,
    health: Number,
    speed: Option<Number>,
    /// Degrees per second. Units without one turn instantly.
    turn_rate: Option<u32>,
    /// The tightest arc, in units. Without one, a unit turns on the spot.
    turn_radius: Option<u32>,
    radius: Option<Number>,
    footprint: Option<u8>,
    vision: Number,
    build_power: Option<Number>,
    build_range: Option<Number>,
    #[serde(default)]
    builds: Vec<String>,
    upgrades_to: Option<String>,
    weapon: Option<RawWeapon>,
    /// More weapons, each with its own targets, reload, and turret.
    #[serde(default)]
    extra_weapons: Vec<RawWeapon>,
    flight: Option<RawFlight>,
    #[serde(default)]
    requires_deposit: bool,
    #[serde(default)]
    requires_vent: bool,
    #[serde(default)]
    on_water: bool,
    /// "land", the default, "naval", "submarine", "hover", or "seabed".
    movement: Option<String>,
    mass_income: Option<Number>,
    energy_income: Option<Number>,
    mass_storage: Option<Number>,
    energy_storage: Option<Number>,
    death_explosion: Option<RawExplosion>,
    discharge: Option<RawDischarge>,
    shield: Option<RawShield>,
    missile_defense: Option<RawMissileDefense>,
    torpedo_defense: Option<RawTorpedoDefense>,
    death_storm: Option<RawStorm>,
    airfield: Option<RawAirfield>,
    transport: Option<RawTransport>,
    /// "land" or "air".
    factory_kind: Option<String>,
    #[serde(default)]
    support: bool,
    #[serde(default)]
    assists: bool,
    /// What it gives its neighbors, such as "t1_power" or "mass_storage".
    adjacency: Option<String>,
    /// FAF's size class: 4, 8, 12, 16, or 20. Without one, 16 for factories, and otherwise from
    /// the footprint: 1 is 4, 2 is 8, 3 is 12, 4 is 16, and larger is 20.
    adjacency_size: Option<u8>,
    radar: Option<Number>,
    sonar: Option<Number>,
    deep_scan: Option<Number>,
    stealth_field: Option<Number>,
    jammer: Option<RawJammer>,
    cloak: Option<RawCloak>,
    /// Energy per second while running.
    upkeep: Option<Number>,
    max_stock: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTeleporter {
    flat_energy: Number,
    max_energy: Number,
    min_seconds: u32,
    max_seconds: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEnhancement {
    id: String,
    name: String,
    units: Vec<String>,
    /// "weapons", "fabrication", or "systems".
    slot: String,
    requires: Option<String>,
    mass: Number,
    energy: Number,
    build_time: Number,
    build_power: Option<Number>,
    build_tier: Option<u8>,
    health: Option<Number>,
    /// Health per second.
    regen: Option<Number>,
    weapon_range: Option<Number>,
    weapon_damage: Option<Number>,
    weapon_reload_ticks: Option<u32>,
    weapon_splash: Option<Number>,
    vision: Option<Number>,
    deep_scan: Option<Number>,
    jammer: Option<RawJammer>,
    /// Energy per second while jamming.
    jammer_upkeep: Option<Number>,
    shield: Option<RawShield>,
    cloak: Option<RawCloak>,
    /// Energy per second to stay off enemy radar.
    stealth_upkeep: Option<Number>,
    teleporter: Option<RawTeleporter>,
    /// Per second.
    mass_income: Option<Number>,
    energy_income: Option<Number>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    version: u32,
    unit: Vec<RawUnit>,
    #[serde(default)]
    enhancement: Vec<RawEnhancement>,
}

#[derive(Clone, Debug)]
pub struct BlueprintDb {
    blueprints: Vec<Blueprint>,
    enhancements: Vec<Enhancement>,
    content_hash: u64,
}

fn parse_shield(shield: &RawShield) -> Result<ShieldDef, String> {
    Ok(ShieldDef {
        radius: shield.radius.fp()?,
        health: shield.health.fp()?,
        regen_per_tick: shield.regen.fp()? / TICKS_PER_SECOND as i32,
        recharge_ticks: shield.recharge_seconds * TICKS_PER_SECOND as u32,
        upkeep: shield.upkeep.res()? / TICKS_PER_SECOND,
    })
}

fn parse_enhancements(raw: &[RawEnhancement], lookup: impl Fn(&str) -> Result<BlueprintId, String>) -> Result<Vec<Enhancement>, String> {
    let keys: Vec<&str> = raw.iter().map(|enhancement| enhancement.id.as_str()).collect();
    let mut enhancements = Vec::with_capacity(raw.len());
    for (index, enhancement) in raw.iter().enumerate() {
        let context = |error: String| format!("{}: {error}", enhancement.id);
        let whole = |value: &Option<Number>| value.as_ref().map(Number::fp).transpose().map_err(context);
        let per_tick = |value: &Option<Number>| value.as_ref().map(|number| number.res().map(|per_second| per_second / TICKS_PER_SECOND)).transpose().map_err(context);
        let slot = match enhancement.slot.as_str() {
            "weapons" => Slot::Weapons,
            "fabrication" => Slot::Fabrication,
            "systems" => Slot::Systems,
            other => return Err(context(format!("unknown slot {other:?}"))),
        };
        let requires = match &enhancement.requires {
            None => None,
            Some(key) => {
                let found = keys.iter().position(|candidate| candidate == key).ok_or_else(|| context(format!("unknown enhancement {key:?}")))?;
                if raw[found].slot != enhancement.slot {
                    return Err(context(format!("{key:?} is in another slot")));
                }
                Some(EnhancementId(found as u16))
            }
        };
        let teleporter = match &enhancement.teleporter {
            None => None,
            Some(teleporter) => Some(TeleporterDef {
                flat_energy: teleporter.flat_energy.res().map_err(context)?,
                max_energy: teleporter.max_energy.res().map_err(context)?,
                min_ticks: teleporter.min_seconds * TICKS_PER_SECOND as u32,
                max_ticks: teleporter.max_seconds * TICKS_PER_SECOND as u32,
            }),
        };
        let effects = EnhancementEffects {
            build_power: whole(&enhancement.build_power)?,
            build_tier: enhancement.build_tier.unwrap_or(0),
            health: whole(&enhancement.health)?.unwrap_or(Fp::ZERO),
            regen_per_tick: whole(&enhancement.regen)?.map_or(Fp::ZERO, |per_second| per_second / TICKS_PER_SECOND as i32),
            weapon_range: whole(&enhancement.weapon_range)?,
            weapon_damage: whole(&enhancement.weapon_damage)?,
            weapon_reload_ticks: enhancement.weapon_reload_ticks,
            weapon_splash: whole(&enhancement.weapon_splash)?,
            vision: whole(&enhancement.vision)?,
            deep_scan: whole(&enhancement.deep_scan)?,
            jammer: match &enhancement.jammer {
                None => None,
                Some(jammer) => Some(JammerDef { radius: jammer.radius.fp().map_err(context)?, decoys: jammer.decoys }),
            },
            jammer_upkeep: per_tick(&enhancement.jammer_upkeep)?,
            shield: enhancement.shield.as_ref().map(parse_shield).transpose().map_err(context)?,
            cloak_upkeep: enhancement.cloak.as_ref().map(|cloak| cloak.upkeep.res().map(|per_second| per_second / TICKS_PER_SECOND)).transpose().map_err(context)?,
            stealth_upkeep: per_tick(&enhancement.stealth_upkeep)?,
            teleporter,
            mass_income: per_tick(&enhancement.mass_income)?.unwrap_or(Res::ZERO),
            energy_income: per_tick(&enhancement.energy_income)?.unwrap_or(Res::ZERO),
        };
        enhancements.push(Enhancement {
            id: EnhancementId(index as u16),
            key: enhancement.id.clone(),
            name: enhancement.name.clone(),
            units: enhancement.units.iter().map(|key| lookup(key)).collect::<Result<_, _>>().map_err(context)?,
            slot,
            requires,
            mass: enhancement.mass.res().map_err(context)?,
            energy: enhancement.energy.res().map_err(context)?,
            build_time: enhancement.build_time.res().map_err(context)?,
            effects,
        });
    }
    Ok(enhancements)
}

impl BlueprintDb {
    /// The data compiled into the program.
    pub fn standard() -> Self {
        Self::parse(STANDARD_DATA).expect("data/units.toml is valid")
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let raw: RawFile = toml::from_str(text).map_err(|error| error.to_string())?;
        if raw.version != 1 {
            return Err(format!("unsupported data version {}", raw.version));
        }
        let keys: Vec<&str> = raw.unit.iter().map(|unit| unit.id.as_str()).collect();
        let lookup = |key: &str| {
            keys.iter()
                .position(|candidate| *candidate == key)
                .map(|index| BlueprintId(index as u16))
                .ok_or_else(|| format!("unknown unit {key:?}"))
        };
        let mut blueprints = Vec::with_capacity(raw.unit.len());
        for (index, unit) in raw.unit.iter().enumerate() {
            let context = |error: String| format!("{}: {error}", unit.id);
            let whole = |value: &Option<Number>| value.as_ref().map_or(Ok(Fp::ZERO), Number::fp).map_err(context);
            let rate = |value: &Option<Number>| {
                value
                    .as_ref()
                    .map_or(Ok(Res::ZERO), |number| number.res().map(|per_second| per_second / TICKS_PER_SECOND))
                    .map_err(context)
            };
            let total = |value: &Option<Number>| value.as_ref().map_or(Ok(Res::ZERO), Number::res).map_err(context);
            let footprint = unit.footprint.unwrap_or(0);
            let radius = if footprint > 0 {
                fp_int(i32::from(footprint) * TERRAIN_CELL_UNITS / 2)
            } else {
                whole(&unit.radius)?
            };
            let build_time = unit.build_time.res().map_err(context)?;
            if build_time <= Res::ZERO {
                return Err(context("build_time must be positive".into()));
            }
            let parse_weapon = |weapon: &RawWeapon| -> Result<WeaponDef, String> {
                if (weapon.mount.is_some() || weapon.aft) && weapon.turn_rate.is_none() {
                    return Err(context("only a weapon with a turn_rate has a turret to mount".into()));
                }
                if weapon.setup_ticks.is_some() && !weapon.artillery {
                    return Err(context("only artillery sets up before it fires".into()));
                }
                let mount = match &weapon.mount {
                    None => [Fp::ZERO; 2],
                    Some([right, forward]) => [right.fp().map_err(context)?, forward.fp().map_err(context)?],
                };
                Ok(WeaponDef {
                    range: weapon.range.fp().map_err(context)?,
                    reload_ticks: weapon.reload_ticks,
                    projectile_speed: weapon.projectile_speed.fp().map_err(context)?,
                    damage: weapon.damage.fp().map_err(context)?,
                    splash_radius: whole(&weapon.splash_radius)?,
                    turn_rate: weapon.turn_rate,
                    kind: match weapon.kind.as_deref() {
                        None | Some("shell") => WeaponKind::Shell,
                        Some("tactical_missile") => WeaponKind::TacticalMissile,
                        Some("strategic_missile") => WeaponKind::StrategicMissile,
                        Some("bomb") => WeaponKind::Bomb,
                        Some("flak") => WeaponKind::Flak,
                        Some("tractor") => WeaponKind::Tractor,
                        Some(other) => return Err(context(format!("unknown weapon kind {other:?}"))),
                    },
                    cruise_height: whole(&weapon.cruise_height)?,
                    targets: match weapon.targets.as_deref() {
                        None | Some("ground") => Targets::Ground,
                        Some("air") => Targets::Air,
                        Some("both") => Targets::Both,
                        Some("naval") => Targets::Naval,
                        Some(other) => return Err(context(format!("unknown targets {other:?}"))),
                    },
                    inner_radius: whole(&weapon.inner_radius)?,
                    inner_damage: whole(&weapon.inner_damage)?,
                    artillery: weapon.artillery,
                    min_range: whole(&weapon.min_range)?,
                    spread: whole(&weapon.spread)?,
                    energy_per_shot: weapon.energy_per_shot.as_ref().map_or(Ok(Res::ZERO), Number::res).map_err(context)?,
                    automatic: weapon.automatic,
                    homing: weapon.homing,
                    stun_ticks: weapon.stun_seconds.unwrap_or(0) * TICKS_PER_SECOND as u32,
                    crush_ticks: weapon.crush_seconds.unwrap_or(0) * TICKS_PER_SECOND as u32,
                    mount,
                    aft: weapon.aft,
                    setup_ticks: weapon.setup_ticks.unwrap_or(0),
                })
            };
            let weapon = unit.weapon.as_ref().map(parse_weapon).transpose()?;
            // Extra weapons fire shells, or steering shots at aircraft, on their own targets.
            let extra_weapons = unit.extra_weapons.iter().map(parse_weapon).collect::<Result<Vec<_>, _>>()?;
            // Extra weapons fire shells, steering shots, or missiles that pick their own targets.
            if extra_weapons.iter().any(|extra| extra.artillery || !(extra.kind == WeaponKind::Shell || (extra.kind == WeaponKind::TacticalMissile && extra.automatic))) {
                return Err(context("extra weapons can only fire shells, or automatic tactical missiles".into()));
            }
            if !extra_weapons.is_empty() && weapon.is_none() {
                return Err(context("extra weapons need a main weapon".into()));
            }
            let death_explosion = match &unit.death_explosion {
                None => None,
                Some(explosion) => Some(ExplosionDef {
                    damage: explosion.damage.fp().map_err(context)?,
                    radius: explosion.radius.fp().map_err(context)?,
                    only_while_running: explosion.only_while_running,
                }),
            };
            let discharge = match &unit.discharge {
                None => None,
                Some(discharge) => Some(DischargeDef {
                    energy_required: discharge.energy_required.res().map_err(context)?,
                    energy_share_permille: discharge.energy_share_permille.min(1000),
                    energy_per_damage: discharge.energy_per_damage.max(1),
                    min_damage: discharge.min_damage.fp().map_err(context)?,
                    max_damage: discharge.max_damage.fp().map_err(context)?,
                    commander_damage: discharge.commander_damage.fp().map_err(context)?,
                    structure_damage: discharge.structure_damage.fp().map_err(context)?,
                    splash_radius: discharge.splash_radius.fp().map_err(context)?,
                    search_radius: discharge.search_radius.fp().map_err(context)?,
                    reload_ticks: discharge.reload_ticks,
                }),
            };
            let shield = unit.shield.as_ref().map(parse_shield).transpose().map_err(context)?;
            let missile_defense = match &unit.missile_defense {
                None => None,
                Some(defense) => Some(MissileDefenseDef {
                    strategic: match defense.kind.as_str() {
                        "tactical" => false,
                        "strategic" => true,
                        other => return Err(context(format!("unknown missile defense {other:?}"))),
                    },
                    range: defense.range.fp().map_err(context)?,
                    reload_ticks: defense.reload_ticks.unwrap_or(0),
                }),
            };
            let airfield = match &unit.airfield {
                None => None,
                Some(staging) => Some(AirfieldDef {
                    slots: staging.slots,
                    repair_per_tick: staging.repair.fp().map_err(context)? / TICKS_PER_SECOND as i32,
                    energy_per_tick: staging.repair_energy.res().map_err(context)? / TICKS_PER_SECOND,
                }),
            };
            if airfield.is_some() && footprint == 0 {
                return Err(context("only structures can be airfield".into()));
            }
            let jammer = match &unit.jammer {
                None => None,
                Some(jammer) => Some(JammerDef { radius: jammer.radius.fp().map_err(context)?, decoys: jammer.decoys }),
            };
            let cloak_upkeep = match &unit.cloak {
                None => None,
                Some(cloak) => Some(cloak.upkeep.res().map_err(context)? / TICKS_PER_SECOND),
            };

            blueprints.push(Blueprint {
                id: BlueprintId(index as u16),
                key: unit.id.clone(),
                name: unit.name.clone(),
                class: match unit.class.as_str() {
                    "commander" => UnitClass::Commander,
                    "field_rig" => UnitClass::FieldRig,
                    "air" => UnitClass::Air,
                    "engineer" => UnitClass::Engineer,
                    "bot" => UnitClass::Bot,
                    "artillery" => UnitClass::Artillery,
                    "tank" => UnitClass::Tank,
                    "factory" => UnitClass::Factory,
                    "economy" => UnitClass::Economy,
                    "defense" => UnitClass::Defense,
                    "ammo" => UnitClass::Ammo,
                    other => return Err(context(format!("unknown class {other:?}"))),
                },
                tier: unit.tier,
                mass: unit.mass.res().map_err(context)?,
                energy: unit.energy.res().map_err(context)?,
                build_time,
                health: unit.health.fp().map_err(context)?,
                speed: whole(&unit.speed)?,
                turn_rate: unit.turn_rate,
                turn_radius: unit.turn_radius.unwrap_or(0),
                radius,
                footprint,
                vision: unit.vision.fp().map_err(context)?,
                build_power: whole(&unit.build_power)?,
                build_range: whole(&unit.build_range)?,
                builds: unit.builds.iter().map(|key| lookup(key)).collect::<Result<_, _>>().map_err(context)?,
                upgrades_to: unit.upgrades_to.as_deref().map(lookup).transpose().map_err(context)?,
                weapon,
                extra_weapons,
                flight: match &unit.flight {
                    None => None,
                    Some(flight) => Some(FlightDef {
                        elevation: flight.elevation.fp().map_err(context)?,
                        turn_rate: flight.turn_rate,
                        break_off: whole(&flight.break_off)?,
                        fuel_ticks: flight.fuel * TICKS_PER_SECOND as u32,
                        hovers: flight.hovers,
                    }),
                },
                requires_deposit: unit.requires_deposit,
                requires_vent: unit.requires_vent,
                on_water: unit.on_water,
                movement: match unit.movement.as_deref() {
                    None | Some("land") => super::components::MoveClass::Land,
                    Some("naval") => super::components::MoveClass::Naval,
                    Some("submarine") => super::components::MoveClass::Submarine,
                    Some("hover") => super::components::MoveClass::Hover,
                    Some("seabed") => super::components::MoveClass::Seabed,
                    Some(other) => return Err(context(format!("unknown movement {other:?}"))),
                },
                mass_income: rate(&unit.mass_income)?,
                energy_income: rate(&unit.energy_income)?,
                mass_storage: total(&unit.mass_storage)?,
                energy_storage: total(&unit.energy_storage)?,
                death_explosion,
                discharge,
                shield,
                missile_defense,
                death_storm: match &unit.death_storm {
                    None => None,
                    Some(storm) => Some(StormDef {
                        radius: storm.radius.fp().map_err(context)?,
                        damage_per_tick: storm.damage.fp().map_err(context)? / TICKS_PER_SECOND as i32,
                        ticks: storm.seconds * TICKS_PER_SECOND as u32,
                        speed: storm.speed.fp().map_err(context)?,
                    }),
                },
                torpedo_defense: match &unit.torpedo_defense {
                    None => None,
                    Some(defense) => Some(TorpedoDefenseDef { range: defense.range.fp().map_err(context)?, reload_ticks: defense.reload_ticks }),
                },
                airfield,
                transport: unit.transport.as_ref().map(|transport| TransportDef { slots: [transport.small.max(1), transport.medium.max(1), transport.large.max(1)] }),
                factory_kind: match unit.factory_kind.as_deref() {
                    None => None,
                    Some("land") => Some(FactoryKind::Land),
                    Some("air") => Some(FactoryKind::Air),
                    Some("naval") => Some(FactoryKind::Naval),
                    Some(other) => return Err(context(format!("unknown factory kind {other:?}"))),
                },
                support: unit.support,
                assists: unit.assists,
                adjacency_giver: match unit.adjacency.as_deref() {
                    None => None,
                    Some("t1_power") => Some(AdjacencyGiver::T1Power),
                    Some("t2_power") => Some(AdjacencyGiver::T2Power),
                    Some("t3_power") => Some(AdjacencyGiver::T3Power),
                    Some("geothermal") => Some(AdjacencyGiver::Geothermal),
                    Some("t1_extractor") => Some(AdjacencyGiver::T1Extractor),
                    Some("t2_extractor") => Some(AdjacencyGiver::T2Extractor),
                    Some("t3_extractor") => Some(AdjacencyGiver::T3Extractor),
                    Some("t1_fabricator") => Some(AdjacencyGiver::T1Fabricator),
                    Some("t3_fabricator") => Some(AdjacencyGiver::T3Fabricator),
                    Some("mass_storage") => Some(AdjacencyGiver::MassStorage),
                    Some("energy_storage") => Some(AdjacencyGiver::EnergyStorage),
                    Some(other) => return Err(context(format!("unknown adjacency {other:?}"))),
                },
                adjacency_size: match (unit.adjacency_size, unit.factory_kind.is_some(), footprint) {
                    (Some(size @ (4 | 8 | 12 | 16 | 20)), _, _) => usize::from(size / 4 - 1),
                    (Some(other), _, _) => return Err(context(format!("adjacency size {other} is not 4, 8, 12, 16, or 20"))),
                    (None, true, _) => 3,
                    (None, false, size) => usize::from(size.clamp(1, 5) - 1),
                },
                radar: whole(&unit.radar)?,
                sonar: whole(&unit.sonar)?,
                deep_scan: whole(&unit.deep_scan)?,
                stealth_field: whole(&unit.stealth_field)?,
                jammer,
                cloak_upkeep,
                upkeep: rate(&unit.upkeep)?,
                max_stock: unit.max_stock.unwrap_or(0),
            });
        }
        let enhancements = parse_enhancements(&raw.enhancement, lookup)?;
        // Line endings can differ by checkout, and must not change the checksum.
        let mut hasher = XxHash64::with_seed(0);
        hasher.write(text.replace('\r', "").as_bytes());
        Ok(Self { blueprints, enhancements, content_hash: hasher.finish() })
    }

    pub fn enhancement(&self, id: EnhancementId) -> &Enhancement {
        &self.enhancements[id.0 as usize]
    }

    pub fn find_enhancement(&self, key: &str) -> Option<EnhancementId> {
        self.enhancements.iter().find(|enhancement| enhancement.key == key).map(|enhancement| enhancement.id)
    }

    /// Looks up an enhancement that the caller knows exists.
    pub fn enhancement_id(&self, key: &str) -> EnhancementId {
        self.find_enhancement(key).unwrap_or_else(|| panic!("no enhancement named {key}"))
    }

    /// Whether `later` builds on `earlier`, directly or through others, and so includes it.
    pub fn builds_on(&self, later: EnhancementId, earlier: EnhancementId) -> bool {
        let mut next = self.enhancement(later).requires;
        while let Some(id) = next {
            if id == earlier {
                return true;
            }
            next = self.enhancement(id).requires;
        }
        false
    }

    /// Whether a slot holding `current` already has `enhancement`: it is there, or what is
    /// there builds on it, as the T3 suite does on the T2 one.
    pub fn slot_has(&self, current: Option<EnhancementId>, enhancement: EnhancementId) -> bool {
        current.is_some_and(|current| current == enhancement || self.builds_on(current, enhancement))
    }

    /// The enhancements a unit can take, in the order of the data.
    pub fn enhancements_for(&self, blueprint: BlueprintId) -> impl Iterator<Item = &Enhancement> {
        self.enhancements.iter().filter(move |enhancement| enhancement.units.contains(&blueprint))
    }

    pub fn get(&self, id: BlueprintId) -> &Blueprint {
        &self.blueprints[id.0 as usize]
    }

    pub fn find(&self, key: &str) -> Option<BlueprintId> {
        self.blueprints.iter().find(|blueprint| blueprint.key == key).map(|blueprint| blueprint.id)
    }

    /// Looks up a key that the caller knows exists, such as one the game logic depends on.
    pub fn id(&self, key: &str) -> BlueprintId {
        self.find(key).unwrap_or_else(|| panic!("no blueprint named {key}"))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Blueprint> {
        self.blueprints.iter()
    }

    /// A copy with one unit changed, for the balance harness (BALANCE.md), which tries values in
    /// memory. It keeps the original's content hash, so worlds built on it do not stand for any
    /// committed data.
    pub fn with_unit(&self, id: BlueprintId, change: impl FnOnce(&mut Blueprint)) -> Self {
        let mut copy = self.clone();
        change(&mut copy.blueprints[usize::from(id.0)]);
        copy
    }

    pub const fn content_hash(&self) -> u64 {
        self.content_hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
        version = 1
        [[unit]]
        id = "a"
        name = "A"
        class = "bot"
        tier = 1
        mass = 10
        energy = 20
        build_time = 5
        health = "12.5"
        vision = 100
        mass_income = 2
    "#;

    #[test]
    fn standard_data_has_the_vertical_slice_roster() {
        let db = BlueprintDb::standard();
        for key in [
            "command_rig",
            "t1_engineer",
            "t1_light_walker",
            "t1_light_artillery",
            "t2_heavy_tank",
            "land_factory_t1",
            "land_factory_t2",
            "mass_extractor",
            "power_generator",
            "mass_storage",
            "t1_point_defense",
            "shield_generator",
            "tactical_missile_launcher",
            "tactical_missile_defense",
            "tactical_missile",
            "strategic_missile_launcher",
            "strategic_missile",
            "strategic_missile_defense",
            "strategic_interceptor",
            "radar",
            "deep_scan_array",
            "stealth_field_generator",
        ] {
            assert!(db.find(key).is_some(), "{key} is missing");
        }
        let factory = db.get(db.id("land_factory_t1"));
        assert_eq!(factory.upgrades_to, Some(db.id("land_factory_t2")));
        assert!(!factory.builds.contains(&db.id("t2_heavy_tank")));
        assert!(db.get(db.id("land_factory_t2")).builds.contains(&db.id("t2_heavy_tank")));
    }

    #[test]
    fn decimals_parse_exactly_and_rates_convert_to_per_tick() {
        let db = BlueprintDb::parse(MINIMAL).unwrap();
        let unit = db.get(BlueprintId(0));
        assert_eq!(unit.health, Fp::lit("12.5"));
        assert_eq!(unit.mass_income, Res::from_num(2) / TICKS_PER_SECOND);
    }

    #[test]
    fn toml_floats_are_rejected() {
        let error = BlueprintDb::parse(&MINIMAL.replace("mass = 10", "mass = 10.5"));
        assert!(error.is_err(), "a float must not load");
    }

    #[test]
    fn unknown_fields_and_references_are_rejected() {
        assert!(BlueprintDb::parse(&MINIMAL.replace("tier = 1", "tier = 1\nspede = 3")).is_err());
        assert!(BlueprintDb::parse(&MINIMAL.replace("tier = 1", "tier = 1\nbuilds = [\"nope\"]")).is_err());
    }

    #[test]
    fn content_hash_ignores_line_endings_but_not_values() {
        let unix = BlueprintDb::parse(MINIMAL).unwrap();
        let windows = BlueprintDb::parse(&MINIMAL.replace('\n', "\r\n")).unwrap();
        let changed = BlueprintDb::parse(&MINIMAL.replace("mass = 10", "mass = 11")).unwrap();
        assert_eq!(unix.content_hash(), windows.content_hash());
        assert_ne!(unix.content_hash(), changed.content_hash());
    }
}
