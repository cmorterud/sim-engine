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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UnitClass {
    Commander,
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
    /// A nuclear blast's inner ring, on top of `damage` over all of `splash_radius`. Zero for none.
    pub inner_radius: Fp,
    pub inner_damage: Fp,
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

/// The commander's Overcharge: a shot paid for from stored energy, that destroys almost any
/// mobile unit but does little to structures.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OverchargeDef {
    pub energy: Res,
    pub damage: Fp,
    /// The most one shot does to a structure.
    pub structure_damage: Fp,
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
    pub requires_deposit: bool,
    /// Per tick.
    pub mass_income: Res,
    /// Per tick.
    pub energy_income: Res,
    pub mass_storage: Res,
    pub energy_storage: Res,
    pub death_explosion: Option<ExplosionDef>,
    pub overcharge: Option<OverchargeDef>,
    pub shield: Option<ShieldDef>,
    pub missile_defense: Option<MissileDefenseDef>,
    /// Sensor ranges (GAME_SPECIFICATION.md §9). Zero for none.
    pub radar: Fp,
    pub omni: Fp,
    /// Friendly units within this range of it make no radar blip. Zero for none.
    pub stealth_field: Fp,
    pub jammer: Option<JammerDef>,
    /// Energy per tick while cloaked, for a unit that can cloak.
    pub cloak_upkeep: Option<Res>,
    /// Energy per tick while running, for intel structures. Zero for none.
    pub upkeep: Res,
    /// For a silo, the most missiles it keeps. It builds no more until it fires one. Zero for no limit.
    pub max_stock: u32,
}

impl Blueprint {
    /// Whether it does anything for intel: sensors, stealth, jamming, or cloaking.
    pub fn has_intel(&self) -> bool {
        self.radar > Fp::ZERO || self.omni > Fp::ZERO || self.stealth_field > Fp::ZERO || self.jammer.is_some() || self.cloak_upkeep.is_some()
    }

    pub const fn is_structure(&self) -> bool {
        self.footprint > 0
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
    inner_radius: Option<Number>,
    inner_damage: Option<Number>,
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
struct RawOvercharge {
    energy: Number,
    damage: Number,
    structure_damage: Number,
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
    #[serde(default)]
    requires_deposit: bool,
    mass_income: Option<Number>,
    energy_income: Option<Number>,
    mass_storage: Option<Number>,
    energy_storage: Option<Number>,
    death_explosion: Option<RawExplosion>,
    overcharge: Option<RawOvercharge>,
    shield: Option<RawShield>,
    missile_defense: Option<RawMissileDefense>,
    radar: Option<Number>,
    omni: Option<Number>,
    stealth_field: Option<Number>,
    jammer: Option<RawJammer>,
    cloak: Option<RawCloak>,
    /// Energy per second while running.
    upkeep: Option<Number>,
    max_stock: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    version: u32,
    unit: Vec<RawUnit>,
}

#[derive(Debug)]
pub struct BlueprintDb {
    blueprints: Vec<Blueprint>,
    content_hash: u64,
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
            let weapon = match &unit.weapon {
                None => None,
                Some(weapon) => Some(WeaponDef {
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
                        Some(other) => return Err(context(format!("unknown weapon kind {other:?}"))),
                    },
                    cruise_height: whole(&weapon.cruise_height)?,
                    inner_radius: whole(&weapon.inner_radius)?,
                    inner_damage: whole(&weapon.inner_damage)?,
                }),
            };
            let death_explosion = match &unit.death_explosion {
                None => None,
                Some(explosion) => Some(ExplosionDef {
                    damage: explosion.damage.fp().map_err(context)?,
                    radius: explosion.radius.fp().map_err(context)?,
                }),
            };
            let overcharge = match &unit.overcharge {
                None => None,
                Some(overcharge) => Some(OverchargeDef {
                    energy: overcharge.energy.res().map_err(context)?,
                    damage: overcharge.damage.fp().map_err(context)?,
                    structure_damage: overcharge.structure_damage.fp().map_err(context)?,
                    reload_ticks: overcharge.reload_ticks,
                }),
            };
            let shield = match &unit.shield {
                None => None,
                Some(shield) => Some(ShieldDef {
                    radius: shield.radius.fp().map_err(context)?,
                    health: shield.health.fp().map_err(context)?,
                    regen_per_tick: shield.regen.fp().map_err(context)? / TICKS_PER_SECOND as i32,
                    recharge_ticks: shield.recharge_seconds * TICKS_PER_SECOND as u32,
                    upkeep: shield.upkeep.res().map_err(context)? / TICKS_PER_SECOND,
                }),
            };
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
            let jammer = match &unit.jammer {
                None => None,
                Some(jammer) => Some(JammerDef { radius: jammer.radius.fp().map_err(context)?, decoys: jammer.decoys }),
            };
            let cloak_upkeep = match &unit.cloak {
                None => None,
                Some(cloak) => Some(cloak.upkeep.res().map_err(context)? / TICKS_PER_SECOND),
            };
            if shield.is_some() && footprint == 0 {
                return Err(context("only structures can have shields".into()));
            }
            blueprints.push(Blueprint {
                id: BlueprintId(index as u16),
                key: unit.id.clone(),
                name: unit.name.clone(),
                class: match unit.class.as_str() {
                    "commander" => UnitClass::Commander,
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
                requires_deposit: unit.requires_deposit,
                mass_income: rate(&unit.mass_income)?,
                energy_income: rate(&unit.energy_income)?,
                mass_storage: total(&unit.mass_storage)?,
                energy_storage: total(&unit.energy_storage)?,
                death_explosion,
                overcharge,
                shield,
                missile_defense,
                radar: whole(&unit.radar)?,
                omni: whole(&unit.omni)?,
                stealth_field: whole(&unit.stealth_field)?,
                jammer,
                cloak_upkeep,
                upkeep: rate(&unit.upkeep)?,
                max_stock: unit.max_stock.unwrap_or(0),
            });
        }
        // Line endings can differ by checkout, and must not change the checksum.
        let mut hasher = XxHash64::with_seed(0);
        hasher.write(text.replace('\r', "").as_bytes());
        Ok(Self { blueprints, content_hash: hasher.finish() })
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
            "acu",
            "t1_engineer",
            "t1_light_bot",
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
            "anti_nuke",
            "radar",
            "omni_sensor",
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
