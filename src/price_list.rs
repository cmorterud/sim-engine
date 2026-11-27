//! The ability price list (BALANCE.md step 3, `[price_list]` in data/balance_targets.toml): what a
//! unit costs from what it can do, in materiel at its tier's prices (`balance::cost_at`).
//!
//! ```text
//! cost = Σ over jobs √(health × damage a second at the job) / σ_job
//!        + what its sensors cost at the cheapest building's price per area / domain base
//!        + its build power at the cheapest builder's rate / domain base
//!        + its cargo: the Light Air Transport's share of a full load of its tier's line units
//!        + its strategic launching: a share of the Strategic Missile Launcher's cost
//! σ_job = anchor × tier efficiency × domain base / (1 + Σ the job's prices)
//! ```
//!
//! Health is the hull's, and a shield of its own adds its health and its regeneration over the
//! [model] time to kill, as a dome's does to its group (`shield_price`).
//!
//! reports/plans/2026-11-26-ability-price-list.md works the units through it. Until 2026-11-27 the
//! list lived in that report and in scripts that were not kept; this is the list as code, so that
//! `--balance-derive unit <key>` prints any unit's price and tests/price_list_test.rs fails when a
//! unit the list prices drifts from it.

use crate::balance::{cost_at, damage_per_second, MAX_TIER, TARGETS};
use crate::sim::blueprints::{Blueprint, BlueprintDb, BlueprintId, Targets, WeaponDef, WeaponKind, TICKS_PER_SECOND};
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
    /// What its build power costs: its points at the cheapest builder's rate, over its base.
    pub build_power: f64,
    /// What its room for cargo costs (`cargo_share`).
    pub cargo: f64,
    /// What launching strategic missiles costs, for a unit that is not a launcher (`launching`).
    pub launching: f64,
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

    /// The part of its price that does not go with the domain base: cargo, whose load is land
    /// units priced as they are, and strategic launching, priced in minutes of a T3 economy as the
    /// launcher is. Sensors go with it, as every other job does (2026-11-27).
    fn fixed_cost(&self) -> f64 {
        self.cargo + self.launching
    }

    /// The list's price at another domain base: each job's part goes as one over the base.
    pub fn at_base(&self, base: f64) -> f64 {
        (self.list - self.fixed_cost()) * self.base / base + self.fixed_cost()
    }

    /// The domain base at which the list prices it at `cost`.
    pub fn base_for(&self, cost: f64) -> f64 {
        self.base * (self.list - self.fixed_cost()) / (cost - self.fixed_cost())
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
    number(&model["time_to_kill_base_seconds"]) * numbers(&model["time_to_kill_tier"])[usize::from(tier.clamp(1, MAX_TIER)) - 1] * number(&model["time_to_kill_domain"][domain])
}

/// The line unit a unit's reach and speed are measured against: its tier's, in its domain. T4
/// has no line of its own and is measured against T3's.
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
    let land = numbers(&model["closing_share_land"])[usize::from(tier.clamp(1, MAX_TIER)) - 1];
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
/// building cost`, shared across a fleet's cover, over the unit's domain base (2026-11-27, the
/// design owner's pick: every sensor, so that a radar's price on a scout and on a warship comes
/// from one rule, as every other job's does).
fn sensors(db: &BlueprintDb, table: &toml::Table, unit: &Blueprint, base: f64) -> Vec<(&'static str, f64)> {
    let vision = unit.vision.to_num::<f64>();
    let kinds: [(&'static str, SensorRange); 3] = [("radar", |unit| unit.radar.to_num()), ("sonar", |unit| unit.sonar.to_num()), ("deep scan", |unit| unit.deep_scan.to_num())];
    let mut out = Vec::new();
    for (name, sensor) in kinds {
        let range = sensor(unit);
        if unit.is_structure() || range <= vision {
            continue;
        }
        let Some((building_cost, building_range)) = cheapest_building(db, sensor) else { continue };
        let price = (range * range - vision * vision) / (building_range * building_range) * building_cost / sharing(db, table, unit, &sensor) / base;
        out.push((name, price));
    }
    out
}

/// A dome's price (`[price_list.shield_groups]`, 2026-11-27): the strength it adds to the group it
/// covers. The list counts strength as √(health × damage a second), so a dome that adds `E` to a
/// group's health `H` adds `√(1 + E / H) − 1` of the group's strength, and costs that share of
/// the group's cost. `E` is the dome's health, its regeneration over the [model] time to kill at
/// its tier in its domain, and the unarmed hull's own health. A dome that cannot move is worth
/// more for its cost (`[price_list] immobile`, as `[premiums]`), and a ship's sensors are paid
/// for as the list pays for them.
#[derive(Clone, Debug)]
pub struct ShieldPricing {
    pub key: String,
    pub group: String,
    pub group_cost: f64,
    pub group_health: f64,
    pub effective_health: f64,
    pub sensors: Vec<(&'static str, f64)>,
    pub list: f64,
    pub actual: f64,
}

impl ShieldPricing {
    /// How far its cost is from its price, as a share of the price.
    pub fn drift(&self) -> f64 {
        self.actual / self.list - 1.0
    }
}

/// A dome's price by the strength it adds to its group, if `[price_list.shield_groups]` names one.
pub fn shield_price(db: &BlueprintDb, key: &str) -> Option<ShieldPricing> {
    let table = table();
    let list = &table["price_list"];
    let group_name = list["shield_groups"].get(key)?.as_str().expect("a shield group names a force or a position");
    let unit = db.get(db.find(key)?);
    let dome = unit.shield?;
    let tier = unit.tier.clamp(1, MAX_TIER);
    let group: Vec<(BlueprintId, u32)> = match group_name.strip_prefix("position:") {
        Some(position) => {
            let units = table["price_list"]["positions"][position].as_table().unwrap_or_else(|| panic!("[price_list.positions] has {position}"));
            units.iter().map(|(name, count)| (db.id(name), number(count) as u32)).collect()
        }
        None => crate::balance::domain_force(db, group_name).into_iter().filter(|&(id, _)| db.get(id).key != unit.key).collect(),
    };
    let group_cost: f64 = group.iter().map(|&(id, count)| f64::from(count) * cost_at(db.get(id), tier)).sum();
    let group_health: f64 = group.iter().map(|&(id, count)| f64::from(count) * db.get(id).health.to_num::<f64>()).sum();
    let domain = match domain_of(unit) {
        Domain::Navy => "navy",
        _ => "land",
    };
    let regen_per_second = dome.regen_per_tick.to_num::<f64>() * TICKS_PER_SECOND as f64;
    let effective_health = dome.health.to_num::<f64>() + regen_per_second * crate::balance::model_time_to_kill(tier, domain) + unit.health.to_num::<f64>();
    let immobile = if unit.is_structure() { number(&list["immobile"]) } else { 1.0 };
    let sensors = sensors(db, &table, unit, number(&list["domain_base"][base_key(domain_of(unit), tier)]));
    let price = group_cost * ((1.0 + effective_health / group_health).sqrt() - 1.0) / immobile + sensors.iter().map(|(_, cost)| cost).sum::<f64>();
    Some(ShieldPricing { key: key.to_string(), group: group_name.to_string(), group_cost, group_health, effective_health, sensors, list: price, actual: cost_at(unit, tier) })
}

/// A unit's price by the list. `None` for one with no fighting weapon, no build power and no
/// sensor past its vision (the Light Air Transport, which sets the price of cargo), and for
/// structures but point defenses (`priced_structure`).
pub fn price(db: &BlueprintDb, key: &str) -> Option<Pricing> {
    price_with(db, &table(), key)
}

/// A unit's price by the list with structures' reach priced at another closing share, for
/// solving `[price_list] structure_closing_share`.
pub fn price_at_structure_closing(db: &BlueprintDb, key: &str, closing: f64) -> Option<Pricing> {
    let mut table = table();
    table["price_list"].as_table_mut().expect("[price_list]").insert("structure_closing_share".to_string(), toml::Value::Float(closing));
    price_with(db, &table, key)
}

/// A structure the list prices (2026-11-27): one whose fighting weapons are all shells at what is
/// on the ground or the water, out-reaching its tier's line by less than the line closes in a time
/// to kill: the point defenses. Anti-air, torpedo launchers, artillery and missile launchers are
/// set by their own rules.
fn priced_structure(db: &BlueprintDb, table: &toml::Table, unit: &Blueprint, weapons: &[WeaponDef]) -> bool {
    let Some(line) = line_of(db, table, domain_of(unit), unit.tier.clamp(1, MAX_TIER)) else { return false };
    let line_reach = line.weapon.expect("a line unit has a weapon").range.to_num::<f64>();
    let closing = number(&table["price_list"]["structure_closing_share"]);
    !weapons.is_empty()
        && weapons.iter().all(|weapon| {
            let reach = weapon.range.to_num::<f64>();
            job_of(weapon) == Job::Surface && !matches!(weapon.kind, WeaponKind::TacticalMissile | WeaponKind::StrategicMissile) && reach >= line_reach && closing * (reach / line_reach - 1.0) < 1.0
        })
}

/// The `[price_list] domain_base` a domain's units of a tier are priced at.
pub const fn base_key(domain: Domain, tier: u8) -> &'static str {
    match domain {
        Domain::Land => "land",
        Domain::Navy => "navy",
        Domain::Air if tier == 1 => "air_t1",
        Domain::Air if tier == 2 => "air_t2",
        // T4 aircraft at T3's base until a T4 anti-air unit exists (2026-11-27).
        Domain::Air => "air_t3",
    }
}

/// The domes `[price_list.shield_groups]` prices, by key.
pub fn shield_group_domes() -> Vec<String> {
    table()["price_list"]["shield_groups"].as_table().expect("[price_list.shield_groups]").keys().cloned().collect()
}

fn price_with(db: &BlueprintDb, table: &toml::Table, key: &str) -> Option<Pricing> {
    let unit = db.get(db.find(key)?);
    let weapons = fighting_weapons(unit);
    let points = unit.build_power.to_num::<f64>();
    if unit.is_structure() && !priced_structure(db, table, unit, &weapons) {
        return None;
    }
    let list = &table["price_list"];
    let domain = domain_of(unit);
    let tier = unit.tier.clamp(1, MAX_TIER);
    let base = number(&list["domain_base"][base_key(domain, tier)]);
    let sensors = sensors(db, table, unit, base);
    // A unit that only sees (a scout) is priced by its sensors (2026-11-27); an unarmed dome by
    // its group (`shield_price`).
    let dome = weapons.is_empty() && list["shield_groups"].get(key).is_some();
    if dome || (weapons.is_empty() && points <= 0.0 && sensors.is_empty()) {
        return None;
    }
    let tier_efficiency = numbers(&list["tier_efficiency"])[usize::from(tier) - 1];
    let anchor = number(&list["anchor_strength_per_cost"]);
    let health = unit.health.to_num::<f64>() + own_shield_health(table, unit);
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
            // Reach, against the line's: shells pay for reach past it, missiles reach it free. A
            // structure pays at its own closing share (`structure_closing_share`, 2026-11-27): the
            // line must close on it alone, under its fire.
            if let (Some(line), true) = (line, speed > 0.0 || unit.is_structure()) {
                let line_reach = line.weapon.expect("a line unit has a weapon").range.to_num::<f64>();
                let reach = first.range.to_num::<f64>();
                let closing = if unit.is_structure() { number(&list["structure_closing_share"]) } else { closing_share(table, domain, tier) };
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
        }
        // A submarine's torpedoes and missiles fire from under the water, out of cannons' reach;
        // its deck gun fires only on the surface.
        if unit.movement == MoveClass::Submarine && at_job.iter().all(|weapon| weapon.kind != WeaponKind::Shell || job == Job::Torpedo) {
            prices.push(("submerged", number(&list["submerged_t1"])));
        }
        let sigma = anchor * tier_efficiency * base / (1.0 + prices.iter().map(|(_, price)| price).sum::<f64>());
        jobs.push(JobPrice { job, damage_per_second: damage, strength, prices, sigma, cost: strength / sigma });
    }
    // Build power, a utility job (2026-11-27): what the cheapest builder that does nothing else
    // pays for a point, over the domain base, as any other job. A unit that builds strategic
    // missiles to launch pays for that instead (`launching`).
    let launching = launching(db, unit);
    let build_power = if points > 0.0 && launching == 0.0 { points * build_power_rate(db, key) / base } else { 0.0 };
    // Cargo, a utility job (2026-11-27): the share of a full load that the only unit that only
    // carries pays. Not over the base: the load is land units, priced as they are.
    let cargo = if unit.transport.is_some() { cargo_share(db, table) * full_load(db, table, unit) } else { 0.0 };
    let mut total = jobs.iter().map(|job| job.cost).sum::<f64>() + sensors.iter().map(|(_, cost)| cost).sum::<f64>() + build_power + cargo + launching;
    // What cannot move is worth more for its cost, as a dome is (`[price_list] immobile`).
    if unit.is_structure() {
        total /= number(&list["immobile"]);
    }
    Some(Pricing { key: key.to_string(), tier, domain, base, jobs, sensors, build_power, cargo, launching, list: total, actual: cost_at(unit, tier) })
}

/// What launching strategic missiles costs a unit that is not a launcher (2026-11-27, the design
/// owner's pick, for the Strategic Missile Submarine): the share of the cheapest Strategic Missile
/// Launcher that its build power and its stock make, half the launcher's cost for each,
/// `launcher × ½ (points / launcher's + stock / launcher's)`. Not over the domain base: the
/// launcher's cost is set in minutes of a standard T3 economy (`[economy]`), not by a fight.
fn launching(db: &BlueprintDb, unit: &Blueprint) -> f64 {
    let launches = |unit: &Blueprint| unit.weapon.iter().chain(&unit.extra_weapons).any(|weapon| weapon.kind == WeaponKind::StrategicMissile);
    if unit.is_structure() || !launches(unit) {
        return 0.0;
    }
    let Some(launcher) = db.iter().filter(|other| other.is_structure() && launches(other)).min_by(|a, b| cost_at(a, a.tier).total_cmp(&cost_at(b, b.tier))) else {
        return 0.0;
    };
    let share = |own: f64, theirs: f64| own / theirs;
    let points = share(unit.build_power.to_num(), launcher.build_power.to_num());
    let stock = share(f64::from(unit.max_stock), f64::from(launcher.max_stock));
    cost_at(launcher, launcher.tier) * (points + stock) / 2.0
}

/// What a shield of its own adds to a unit's health (2026-11-27): the shield's health and its
/// regeneration over the [model] time to kill, as a dome adds to its group. Nothing for a dome
/// priced by its group (`[price_list.shield_groups]`).
fn own_shield_health(table: &toml::Table, unit: &Blueprint) -> f64 {
    let Some(shield) = unit.shield else { return 0.0 };
    if table["price_list"]["shield_groups"].get(&unit.key).is_some() {
        return 0.0;
    }
    let domain = if domain_of(unit) == Domain::Navy { "navy" } else { "land" };
    let regen_per_second = shield.regen_per_tick.to_num::<f64>() * TICKS_PER_SECOND as f64;
    shield.health.to_num::<f64>() + regen_per_second * crate::balance::model_time_to_kill(unit.tier.clamp(1, MAX_TIER), domain)
}

/// A transport's full load: as many of its tier's land line unit as it has room for at that
/// unit's size, at T2 energy prices, the energy price of every tier but the first, so that a
/// load costs the same whichever transport carries it.
pub fn full_load(db: &BlueprintDb, table: &toml::Table, unit: &Blueprint) -> f64 {
    let Some(transport) = unit.transport else { return 0.0 };
    let Some(line) = line_of(db, table, Domain::Land, unit.tier.clamp(1, MAX_TIER)) else { return 0.0 };
    let size = crate::sim::transport::transport_size(line).expect("a land line unit can be carried");
    f64::from(transport.slots[size]) * cost_at(line, 2)
}

/// The share of a full load that cargo room costs (2026-11-27, the design owner's pick): what the
/// cheapest unit that only carries, the Light Air Transport, pays for its own, its cost at T2
/// energy prices over its full load.
pub fn cargo_share(db: &BlueprintDb, table: &toml::Table) -> f64 {
    db.iter()
        .filter(|unit| !unit.is_structure() && unit.transport.is_some() && fighting_weapons(unit).is_empty())
        .map(|unit| cost_at(unit, 2) / full_load(db, table, unit))
        .fold(f64::INFINITY, f64::min)
}

/// What a point of build power costs (2026-11-27): the least any unit that only builds pays for
/// one, at its own tier's prices, leaving out the unit being priced, so that each engineer is
/// priced by the others.
pub fn build_power_rate(db: &BlueprintDb, except: &str) -> f64 {
    db.iter()
        .filter(|unit| !unit.is_structure() && unit.key != except && unit.build_power.to_num::<f64>() > 0.0 && fighting_weapons(unit).is_empty())
        .map(|unit| cost_at(unit, unit.tier.clamp(1, MAX_TIER)) / unit.build_power.to_num::<f64>())
        .fold(f64::INFINITY, f64::min)
}

/// The `--balance-derive unit <key>` report: one unit's price, job by job.
pub fn unit_report(db: &BlueprintDb, key: &str) {
    if db.find(key).is_none() {
        println!("No unit {key} in data/units.toml.");
        return;
    }
    let table = table();
    let Some(pricing) = price_with(db, &table, key) else {
        match shield_price(db, key) {
            Some(dome) => println!(
                "{key}: its dome adds {:.0} effective health to {} ({:.0} health, cost {:.0}): {:.1}, sensors {:.1}, against its cost {:.1} ({:+.1}%)",
                dome.effective_health,
                dome.group,
                dome.group_health,
                dome.group_cost,
                dome.list,
                dome.sensors.iter().map(|(_, cost)| cost).sum::<f64>(),
                dome.actual,
                dome.drift() * 100.0
            ),
            None => println!("{key}: the list does not price it (no fighting weapon, build power or sensor past its vision, or a structure it does not price)."),
        }
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
    if pricing.build_power > 0.0 {
        println!("  building {:.1} points at {:.2} a point: {:8.1}", unit.build_power.to_num::<f64>(), build_power_rate(db, key), pricing.build_power);
    }
    if pricing.cargo > 0.0 {
        println!("  cargo, {:.3} of a full load of {:.1}: {:8.1}", cargo_share(db, &table), full_load(db, &table, unit), pricing.cargo);
    }
    if pricing.launching > 0.0 {
        println!("  launching, its share of the Strategic Missile Launcher by build power and stock: {:8.1}", pricing.launching);
    }
    let shield = own_shield_health(&table, unit);
    if shield > 0.0 {
        println!("  its shield adds {shield:.0} health");
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
        } else if let Some(pricing) = shield_price(db, &unit.key) {
            println!("| {} | {:.1} | {:.1} | {:+.1}% | its dome over {} |", unit.key, pricing.list, pricing.actual, pricing.drift() * 100.0, pricing.group);
        }
    }
}
