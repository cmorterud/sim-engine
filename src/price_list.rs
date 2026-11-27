//! The ability price list (BALANCE.md step 3, `[price_list]` in data/balance_targets.toml): what a
//! unit costs from what it can do, in materiel at its tier's prices (`balance::cost_at`).
//!
//! ```text
//! cost = Σ over jobs √(health × damage a second at the job) / σ_job + what its sensors cost
//! σ_job = anchor × tier efficiency × domain base / (1 + Σ the job's prices)
//! ```
//!
//! reports/plans/2026-11-26-ability-price-list.md works the units through it. Until 2026-11-27 the
//! list lived in that report and in scripts that were not kept; this is the list as code, so that
//! `--balance-derive unit <key>` prints any unit's price and tests/price_list_test.rs fails when a
//! unit the list prices drifts from it.

use crate::balance::{cost_at, damage_per_second, TARGETS};
use crate::sim::blueprints::{Blueprint, BlueprintDb, Targets, WeaponDef, WeaponKind, TICKS_PER_SECOND};
use crate::sim::components::MoveClass;

/// A kind of target a weapon is for. Separate weapons firing at once each do their own job; one
/// weapon that can hit several kinds counts once, at its best job, with a flexibility price.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Job {
    /// What stands on the ground or sails on the water.
    Surface,
    /// Aircraft.
    Air,
    /// Ships and submarines, with torpedoes.
    Torpedo,
}

impl Job {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Surface => "surface",
            Self::Air => "air",
            Self::Torpedo => "torpedo",
        }
    }
}

/// Where a unit fights, which sets its base (`[price_list] domain_base`) and its line.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Domain {
    Land,
    Navy,
    Air,
}

/// One job's part of a unit's price.
#[derive(Clone, Debug)]
pub struct JobPrice {
    pub job: Job,
    /// The damage a second it lands at the job: its card's, times the share an aircraft lands
    /// (`[price_list.landed_share]`).
    pub damage_per_second: f64,
    /// √(health × that).
    pub strength: f64,
    /// Each price, by name.
    pub prices: Vec<(&'static str, f64)>,
    pub sigma: f64,
    pub cost: f64,
}

/// A unit's price by the list, against what it costs.
#[derive(Clone, Debug)]
pub struct Pricing {
    pub key: String,
    pub tier: u8,
    pub domain: Domain,
    /// The domain base it is priced at.
    pub base: f64,
    pub jobs: Vec<JobPrice>,
    /// What each sensor costs, by name: what it sees past its own vision.
    pub sensors: Vec<(&'static str, f64)>,
    /// The list's price.
    pub list: f64,
    /// What it costs, at its tier's prices.
    pub actual: f64,
}

impl Pricing {
    /// Its cost over the list's price, less one: above zero, it costs more than the list says.
    pub fn drift(&self) -> f64 {
        self.actual / self.list - 1.0
    }

    /// What its sensors cost, together.
    pub fn sensor_cost(&self) -> f64 {
        self.sensors.iter().map(|(_, cost)| cost).sum()
    }

    /// The list's price at another domain base: each job's part goes as one over the base.
    pub fn at_base(&self, base: f64) -> f64 {
        (self.list - self.sensor_cost()) * self.base / base + self.sensor_cost()
    }

    /// The domain base at which the list prices it at `cost`.
    pub fn base_for(&self, cost: f64) -> f64 {
        self.base * (self.list - self.sensor_cost()) / (cost - self.sensor_cost())
    }
}

fn table() -> toml::Table {
    TARGETS.parse().expect("data/balance_targets.toml is valid")
}

fn number(value: &toml::Value) -> f64 {
    value.as_float().or_else(|| value.as_integer().map(|value| value as f64)).expect("a number")
}

fn numbers(value: &toml::Value) -> Vec<f64> {
    value.as_array().expect("a list of numbers").iter().map(number).collect()
}

/// A per-unit number in a `[price_list.<name>]` table, if the unit has one.
fn per_unit(table: &toml::Table, name: &str, key: &str) -> Option<f64> {
    table["price_list"].get(name).and_then(|entries| entries.get(key)).map(number)
}

pub fn domain_of(unit: &Blueprint) -> Domain {
    match unit.move_class() {
        MoveClass::Air => Domain::Air,
        MoveClass::Naval | MoveClass::Submarine => Domain::Navy,
        _ => Domain::Land,
    }
}

/// The weapons that fight: every weapon but the missiles and bombs a silo builds and fires on
/// order, which are priced as game-ending weapons (`[economy]`), not by a fight.
pub fn fighting_weapons(unit: &Blueprint) -> Vec<WeaponDef> {
    unit.weapon
        .iter()
        .chain(&unit.extra_weapons)
        .filter(|weapon| match weapon.kind {
            WeaponKind::StrategicMissile => false,
            WeaponKind::TacticalMissile => weapon.automatic,
            _ => true,
        })
        .copied()
        .collect()
}

pub const fn job_of(weapon: &WeaponDef) -> Job {
    match weapon.targets {
        Targets::Air => Job::Air,
        Targets::Naval => Job::Torpedo,
        Targets::Ground | Targets::Both => Job::Surface,
    }
}

/// The [model] time to kill at a tier, in a domain's fights.
fn fight_seconds(table: &toml::Table, tier: u8, domain: &str) -> f64 {
    let model = &table["model"];
    number(&model["time_to_kill_base_seconds"]) * numbers(&model["time_to_kill_tier"])[usize::from(tier.clamp(1, 3)) - 1] * number(&model["time_to_kill_domain"][domain])
}

/// The line unit a unit's reach and speed are measured against: its tier's, in its domain.
fn line_of<'a>(db: &'a BlueprintDb, table: &toml::Table, domain: Domain, tier: u8) -> Option<&'a Blueprint> {
    let name = match domain {
        Domain::Land => "land",
        Domain::Navy => "navy",
        Domain::Air => return None,
    };
    let keys = table["price_list"]["line"][name].as_array().expect("[price_list] line lists a unit for each tier");
    let key = keys[usize::from(tier.clamp(1, 3)) - 1].as_str().expect("a unit key");
    Some(db.get(db.id(key)))
}

/// The closing share at a tier, in a domain ([model]).
fn closing_share(table: &toml::Table, domain: Domain, tier: u8) -> f64 {
    let model = &table["model"];
    let land = numbers(&model["closing_share_land"])[usize::from(tier.clamp(1, 3)) - 1];
    match domain {
        Domain::Navy => land * number(&model["closing_share_navy_over_land"]),
        _ => land,
    }
}

/// √(1 / (1 − d)) − 1: the price of a share `d` of an enemy's damage stopped, or of one's own
/// that cannot be stopped.
fn share_price(d: f64) -> f64 {
    (1.0 / (1.0 - d)).sqrt() - 1.0
}

/// The cheapest building with a sensor (a building has a footprint), by its cost at its own tier,
/// and the sensor's range: what a mobile unit's sensor is priced against.
fn cheapest_building(db: &BlueprintDb, sensor: impl Fn(&Blueprint) -> f64) -> Option<(f64, f64)> {
    db.iter()
        .filter(|unit| unit.is_structure() && sensor(unit) > 0.0)
        .map(|unit| (cost_at(unit, unit.tier), sensor(unit)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// How many ships of a ship's tier's standard fleet (`[domains.forces]` t<tier>_fleet) carry a
/// sensor that reaches past their vision: they share one cover, so each pays its share. One for a
/// unit outside that fleet, or not a ship.
fn sharing(db: &BlueprintDb, table: &toml::Table, unit: &Blueprint, sensor: &impl Fn(&Blueprint) -> f64) -> f64 {
    if domain_of(unit) != Domain::Navy {
        return 1.0;
    }
    let Some(fleet) = table["domains"]["forces"].get(format!("t{}_fleet", unit.tier)).and_then(toml::Value::as_table) else {
        return 1.0;
    };
    if !fleet.contains_key(&unit.key) {
        return 1.0;
    }
    let count: f64 = fleet
        .iter()
        .filter(|(key, _)| {
            let ship = db.get(db.id(key));
            sensor(ship) > ship.vision.to_num::<f64>()
        })
        .map(|(_, count)| number(count))
        .sum();
    count.max(1.0)
}

/// A sensor's range on a unit.
type SensorRange = fn(&Blueprint) -> f64;

/// What a unit's sensors cost: each pays only for what it sees past the unit's own vision, at the
/// price per area of the cheapest building that has it, `(range² − vision²) / building range² ×
/// building cost`, shared across a fleet's cover.
fn sensors(db: &BlueprintDb, table: &toml::Table, unit: &Blueprint) -> Vec<(&'static str, f64)> {
    let vision = unit.vision.to_num::<f64>();
    let kinds: [(&'static str, SensorRange); 3] = [("radar", |unit| unit.radar.to_num()), ("sonar", |unit| unit.sonar.to_num()), ("deep scan", |unit| unit.deep_scan.to_num())];
    let mut out = Vec::new();
    for (name, sensor) in kinds {
        let range = sensor(unit);
        if unit.is_structure() || range <= vision {
            continue;
        }
        let Some((building_cost, building_range)) = cheapest_building(db, sensor) else { continue };
        let price = (range * range - vision * vision) / (building_range * building_range) * building_cost / sharing(db, table, unit, &sensor);
        out.push((name, price));
    }
    out
}

/// A unit's price by the list. `None` for one with no fighting weapon, whose jobs (building,
/// seeing, carrying) the list does not price yet, and for structures, which pay the immobile
/// premium instead (`[premiums]`).
pub fn price(db: &BlueprintDb, key: &str) -> Option<Pricing> {
    price_with(db, &table(), key)
}

fn price_with(db: &BlueprintDb, table: &toml::Table, key: &str) -> Option<Pricing> {
    let unit = db.get(db.find(key)?);
    let weapons = fighting_weapons(unit);
    if weapons.is_empty() || unit.is_structure() {
        return None;
    }
    let list = &table["price_list"];
    let domain = domain_of(unit);
    let tier = unit.tier.clamp(1, 3);
    let base_key = match domain {
        Domain::Land => "land",
        Domain::Navy => "navy",
        Domain::Air if tier == 1 => "air_t1",
        Domain::Air if tier == 2 => "air_t2",
        Domain::Air => "air_t3",
    };
    let base = number(&list["domain_base"][base_key]);
    let tier_efficiency = numbers(&list["tier_efficiency"])[usize::from(tier) - 1];
    let anchor = number(&list["anchor_strength_per_cost"]);
    let health = unit.health.to_num::<f64>();
    let speed = unit.speed.to_num::<f64>();
    let line = line_of(db, table, domain, tier);
    let line_speed = match line {
        Some(line) => line.speed.to_num::<f64>(),
        None => number(&list["air_speed"]),
    };
    // Prices every job pays: speed, and decoys that stop a share of the torpedoes aimed at it.
    let mut shared: Vec<(&'static str, f64)> = Vec::new();
    if speed > 0.0 {
        shared.push(("speed", number(&list["speed_share"]) * (1.0 - line_speed / speed)));
    }
    if let Some(stopped) = per_unit(table, "decoy_share", key) {
        shared.push(("decoys", share_price(stopped)));
    }
    let mut jobs: Vec<JobPrice> = Vec::new();
    for job in [Job::Surface, Job::Air, Job::Torpedo] {
        let at_job: Vec<&WeaponDef> = weapons.iter().filter(|weapon| job_of(weapon) == job).collect();
        let Some(first) = at_job.first() else { continue };
        // Aircraft pay for what they land, not their card's damage.
        let landed = if domain == Domain::Air {
            let own = if job == Job::Air { per_unit(table, "landed_share", &format!("{key}_anti_air")) } else { None };
            own.or_else(|| if job == Job::Air && weapons.iter().any(|weapon| job_of(weapon) != Job::Air) { None } else { per_unit(table, "landed_share", key) }).unwrap_or(1.0)
        } else {
            1.0
        };
        let damage: f64 = at_job.iter().map(|weapon| damage_per_second(weapon)).sum::<f64>() * landed;
        let strength = (health * damage).sqrt();
        let mut prices = shared.clone();
        // First volley: the gap between volleys (for a bomb, between passes: its reload over the
        // share it lands) against the [model] time to kill in the domain of what it hits.
        let reload = f64::from(first.reload_ticks.max(1)) / TICKS_PER_SECOND as f64;
        let gap = if first.kind == WeaponKind::Bomb { reload / landed } else { reload };
        let fought = match job {
            Job::Air => "air_to_air",
            Job::Torpedo => "navy",
            Job::Surface if domain == Domain::Navy => "navy",
            Job::Surface => "land",
        };
        let fight = fight_seconds(table, tier, fought);
        prices.push(("first volley", (1.0 + number(&list["first_volley_share"]) * gap / fight).sqrt() - 1.0));
        if job == Job::Surface {
            if let Some(n) = per_unit(table, "splash", key) {
                prices.push(("splash", n.sqrt() - 1.0));
            }
            // Reach, against the line's: shells pay for reach past it, missiles reach it free.
            if let (Some(line), true) = (line, speed > 0.0) {
                let line_reach = line.weapon.expect("a line unit has a weapon").range.to_num::<f64>();
                let reach = first.range.to_num::<f64>();
                let closing = closing_share(table, domain, tier);
                let shell = if matches!(first.kind, WeaponKind::TacticalMissile | WeaponKind::StrategicMissile) { 0.0 } else { 1.0 };
                let price = if reach >= line_reach {
                    let x = closing * (reach / line_reach - 1.0) * shell;
                    x / (1.0 - x)
                } else {
                    -closing * (1.0 - reach / line_reach) * (line_speed / speed)
                };
                prices.push(("reach", price));
            }
        }
        if at_job.iter().any(|weapon| weapon.kind == WeaponKind::Laser) {
            prices.push(("undodgeable", number(&list["undodgeable"])));
        }
        if job == Job::Torpedo {
            prices.push(("more than one kind of target", number(&list["torpedo_flexibility"])));
            if unit.movement == MoveClass::Submarine {
                prices.push(("submerged", number(&list["submerged_t1"])));
            }
        }
        let sigma = anchor * tier_efficiency * base / (1.0 + prices.iter().map(|(_, price)| price).sum::<f64>());
        jobs.push(JobPrice { job, damage_per_second: damage, strength, prices, sigma, cost: strength / sigma });
    }
    let sensors = sensors(db, table, unit);
    let total = jobs.iter().map(|job| job.cost).sum::<f64>() + sensors.iter().map(|(_, cost)| cost).sum::<f64>();
    Some(Pricing { key: key.to_string(), tier, domain, base, jobs, sensors, list: total, actual: cost_at(unit, tier) })
}

/// The `--balance-derive unit <key>` report: one unit's price, job by job.
pub fn unit_report(db: &BlueprintDb, key: &str) {
    if db.find(key).is_none() {
        println!("No unit {key} in data/units.toml.");
        return;
    }
    let table = table();
    let Some(pricing) = price_with(db, &table, key) else {
        println!("{key}: the list does not price it yet (no fighting weapon, or a structure).");
        return;
    };
    let unit = db.get(db.id(key));
    println!("{key} (T{} {:?}), health {:.0}, base {:.3}:", pricing.tier, pricing.domain, unit.health.to_num::<f64>(), pricing.base);
    for job in &pricing.jobs {
        let prices = job.prices.iter().map(|(name, price)| format!("{name} {price:+.3}")).collect::<Vec<_>>().join(", ");
        println!("  {:8} {:8.1} a second, strength {:8.1}, prices [{prices}], sigma {:.3}: {:8.1}", job.job.name(), job.damage_per_second, job.strength, job.sigma, job.cost);
    }
    for (name, cost) in &pricing.sensors {
        println!("  {name:8} {cost:8.1}");
    }
    println!("  list {:.1}, costs {:.1} ({:+.1}%){}", pricing.list, pricing.actual, pricing.drift() * 100.0, held_note(&table, key));
}

/// Why a unit's cost is not the list's, if something else holds it (`[price_list.held]`).
pub fn held_reason(key: &str) -> Option<String> {
    table()["price_list"]["held"].get(key).and_then(toml::Value::as_str).map(str::to_string)
}

fn held_note(table: &toml::Table, key: &str) -> String {
    table["price_list"]["held"].get(key).and_then(toml::Value::as_str).map_or(String::new(), |reason| format!("; held: {reason}"))
}

/// The `--balance-derive list` report: every unit the list prices, against its cost.
pub fn list_report(db: &BlueprintDb) {
    let table = table();
    println!("| Unit | List | Costs | Drift | Held by |");
    println!("| :-- | --: | --: | --: | :-- |");
    for unit in db.iter() {
        if let Some(pricing) = price_with(db, &table, &unit.key) {
            let held = table["price_list"]["held"].get(&unit.key).and_then(toml::Value::as_str).unwrap_or("");
            println!("| {} | {:.1} | {:.1} | {:+.1}% | {held} |", unit.key, pricing.list, pricing.actual, pricing.drift() * 100.0);
        }
    }
}
