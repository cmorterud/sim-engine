//! The unit database: a web page listing every unit and structure with all its numbers, by
//! kind (land, air, naval, structures) and tech level. It is made from the very blueprints the
//! simulation plays with, `BlueprintDb::standard()`, parsed from `data/units.toml`, with nothing
//! copied by hand: in the browser, the game itself writes the page (`?units`, linked from the
//! landing page), from the data built into it, so the page always shows what the game plays.

use std::fmt::Write as _;

use crate::icons::Ground;
use crate::math::fp::Fp;
use crate::sim::blueprints::{Blueprint, BlueprintDb, FactoryKind, Res, Targets, UnitClass, WeaponDef, WeaponKind, TICKS_PER_SECOND};
use crate::sim::components::MoveClass;

/// The kinds of unit the page groups by, in order, with their headings and anchors.
const KINDS: [(Kind, &str, &str); 4] = [(Kind::Land, "Land", "land"), (Kind::Air, "Air", "air"), (Kind::Naval, "Naval", "naval"), (Kind::Structure, "Structures", "structures")];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Kind {
    Land,
    Air,
    Naval,
    Structure,
}

fn kind_of(unit: &Blueprint) -> Kind {
    if unit.is_structure() {
        Kind::Structure
    } else if unit.is_air() {
        Kind::Air
    } else if matches!(unit.movement, MoveClass::Naval | MoveClass::Submarine) {
        Kind::Naval
    } else {
        Kind::Land
    }
}

/// Text made safe to put in a page.
pub(crate) fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A name in code, `FieldRig` or `T1Power`, as words: "field rig", "T1 power".
fn words(name: &str) -> String {
    let mut out = String::new();
    for (index, letter) in name.char_indices() {
        if index > 0 && letter.is_ascii_uppercase() {
            out.push(' ');
        }
        out.push(letter);
    }
    // Lower case, but for tech levels such as "T1".
    out.split(' ').map(|word| if word.len() == 2 && word.starts_with('T') { word.to_string() } else { word.to_lowercase() }).collect::<Vec<_>>().join(" ")
}

/// A number with its thousands apart, rounded to `places` decimals, without a trailing ".0".
pub(crate) fn number(value: f64, places: usize) -> String {
    let text = format!("{value:.places$}");
    let (whole, fraction) = text.split_once('.').map_or((text.as_str(), ""), |(whole, fraction)| (whole, fraction));
    let (sign, digits) = whole.strip_prefix('-').map_or(("", whole), |digits| ("-", digits));
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() { format!("{sign}{grouped}") } else { format!("{sign}{grouped}.{fraction}") }
}

fn fp(value: Fp) -> f64 {
    value.to_num::<f64>()
}

fn res(value: Res) -> f64 {
    value.to_num::<f64>()
}

const PER_SECOND: f64 = TICKS_PER_SECOND as f64;

pub(crate) fn seconds(ticks: u32) -> String {
    format!("{} s", number(f64::from(ticks) / PER_SECOND, 1))
}

/// A unit's stats beyond its weapons, as labelled values, leaving out what it does not have.
fn stats(unit: &Blueprint, db: &BlueprintDb) -> Vec<(&'static str, String)> {
    let mut rows: Vec<(&'static str, String)> = Vec::new();
    let mut add = |label: &'static str, value: String| rows.push((label, value));
    // Its tech level, cost and health head the card, so they are not repeated here.
    add("Build time", format!("{} build power × seconds", number(res(unit.build_time), 0)));
    let ground = Ground::of(unit);
    if unit.is_structure() {
        add("Built on", if ground == Ground::Ocean { "water" } else { "land" }.to_string());
        add("Footprint", format!("{0} × {0} cells", unit.footprint));
    } else {
        add("Can go", ground.name().to_string());
        // Aircraft fly, whatever they would do on the ground.
        if !unit.is_air() {
            add("Moves", words(&format!("{:?}", unit.movement)));
        }
        add("Radius", number(fp(unit.radius), 1));
    }
    if unit.speed > Fp::ZERO {
        add("Speed", format!("{} a second", number(fp(unit.speed) * PER_SECOND, 1)));
    }
    if let Some(rate) = unit.turn_rate {
        add("Turn rate", format!("{rate}° a second"));
    }
    if unit.turn_radius > 0 {
        add("Turn radius", number(f64::from(unit.turn_radius), 0));
    }
    if let Some(flight) = unit.flight {
        add("Flies at", number(fp(flight.elevation), 0));
        add("Turn rate in flight", format!("{}° a second", flight.turn_rate));
        add(
            "Flight",
            match (flight.hovers, flight.break_off > Fp::ZERO) {
                (true, _) => "hovers over its target".to_string(),
                (false, true) => format!("makes passes, breaking off {} past", number(fp(flight.break_off), 0)),
                (false, false) => "makes passes".to_string(),
            },
        );
        if flight.fuel_ticks > 0 {
            add("Fuel", seconds(flight.fuel_ticks));
        }
    }
    add("Vision", number(fp(unit.vision), 0));
    for (label, value) in [("Radar", unit.radar), ("Sonar", unit.sonar), ("Deep scan", unit.deep_scan), ("Stealth field", unit.stealth_field)] {
        if value > Fp::ZERO {
            add(label, number(fp(value), 0));
        }
    }
    if let Some(jammer) = unit.jammer {
        add("Radar jammer", format!("{} decoys within {}", jammer.decoys, number(fp(jammer.radius), 0)));
    }
    if let Some(upkeep) = unit.cloak_upkeep {
        add("Cloak", format!("{} energy a second", number(res(upkeep) * PER_SECOND, 1)));
    }
    if unit.mass_income > Res::ZERO {
        add("Makes materiel", format!("{} a second", number(res(unit.mass_income) * PER_SECOND, 2)));
    }
    if unit.energy_income > Res::ZERO {
        add("Makes energy", format!("{} a second", number(res(unit.energy_income) * PER_SECOND, 1)));
    }
    if unit.upkeep > Res::ZERO {
        add("Upkeep", format!("{} energy a second", number(res(unit.upkeep) * PER_SECOND, 1)));
    }
    if unit.mass_storage > Res::ZERO {
        add("Stores materiel", number(res(unit.mass_storage), 0));
    }
    if unit.energy_storage > Res::ZERO {
        add("Stores energy", number(res(unit.energy_storage), 0));
    }
    if unit.build_power > Fp::ZERO {
        add("Build power", number(fp(unit.build_power), 0));
    }
    if unit.build_range > Fp::ZERO {
        add("Build range", number(fp(unit.build_range), 0));
    }
    if unit.assists {
        add("Assists", "helps build what others build".to_string());
    }
    if let Some(kind) = unit.factory_kind {
        let kind = match kind {
            FactoryKind::Land => "land",
            FactoryKind::Air => "air",
            FactoryKind::Naval => "naval",
        };
        add("Factory", if unit.support { format!("{kind} support factory") } else { format!("{kind} factory") });
    }
    if let Some(next) = unit.upgrades_to {
        let next = db.get(next);
        add("Upgrades to", format!("<a href=\"#{}\">{}</a>", escape(&next.key), escape(&next.name)));
    }
    if unit.requires_deposit {
        add("Built on", "a materiel deposit".to_string());
    }
    if unit.requires_vent {
        add("Built on", "a geothermal vent".to_string());
    }
    if unit.on_water && unit.is_structure() {
        add("Built on", "the water".to_string());
    }
    if let Some(giver) = unit.adjacency_giver {
        add("Adjacency", format!("gives its neighbors the {} bonus", words(&format!("{giver:?}"))));
    }
    if let Some(shield) = unit.shield {
        let height = if shield.height < shield.radius { format!(", height {}", number(fp(shield.height), 0)) } else { String::new() };
        add("Shield", format!("{} health, radius {}{height}", number(fp(shield.health), 0), number(fp(shield.radius), 0)));
        add("Shield regeneration", format!("{} a second", number(fp(shield.regen_per_tick) * PER_SECOND, 1)));
        add("Shield recharge", format!("{} after it collapses", seconds(shield.recharge_ticks)));
        if shield.upkeep > Res::ZERO {
            add("Shield upkeep", format!("{} energy a second", number(res(shield.upkeep) * PER_SECOND, 1)));
        }
    }
    if let Some(defense) = unit.missile_defense {
        let kind = if defense.strategic { "Strategic missile defense" } else { "Tactical missile defense" };
        let mounts = if defense.mounts > 1 { format!(", from {} mounts", defense.mounts) } else { String::new() };
        add(kind, format!("range {}, every {}{mounts}", number(fp(defense.range), 0), seconds(defense.reload_ticks)));
    }
    if let Some(defense) = unit.torpedo_defense {
        add("Torpedo defense", format!("range {}, every {}", number(fp(defense.range), 0), seconds(defense.reload_ticks)));
    }
    if let Some(airfield) = unit.airfield {
        add("Airfield", format!("room for {} T1 aircraft, repairs {} a second each", airfield.slots, number(fp(airfield.repair_per_tick) * PER_SECOND, 1)));
    }
    if let Some(transport) = unit.transport {
        let [small, medium, large] = transport.slots;
        add("Carries", format!("{small} T1, {medium} T2, or {large} T3 land units"));
    }
    if unit.max_stock > 0 {
        add("Missiles stored", unit.max_stock.to_string());
    }
    if let Some(discharge) = unit.discharge {
        add("Discharge", format!("{}–{} damage within {}, needs {} energy stored", number(fp(discharge.min_damage), 0), number(fp(discharge.max_damage), 0), number(fp(discharge.splash_radius), 0), number(res(discharge.energy_required), 0)));
        add("Discharge on commanders and structures", format!("at most {} and {}", number(fp(discharge.commander_damage), 0), number(fp(discharge.structure_damage), 0)));
        add("Discharge reload", seconds(discharge.reload_ticks));
    }
    if let Some(explosion) = unit.death_explosion {
        let when = if explosion.only_while_running { ", if it was running" } else { "" };
        add("Blows up", format!("{} damage within {}{when}", number(fp(explosion.damage), 0), number(fp(explosion.radius), 0)));
    }
    if let Some(storm) = unit.death_storm {
        add("Leaves a storm", format!("{} a second within {}, for {}, roaming {} a second", number(fp(storm.damage_per_tick) * PER_SECOND, 0), number(fp(storm.radius), 0), seconds(storm.ticks), number(fp(storm.speed) * PER_SECOND, 0)));
    }
    rows
}

/// What a unit is, in a word or two, for under its name.
fn what_it_is(unit: &Blueprint) -> String {
    if unit.is_structure() {
        return format!("structure, {0} × {0}", unit.footprint);
    }
    match (unit.class, unit.movement) {
        (UnitClass::Commander, _) => "commander",
        (UnitClass::FieldRig, _) => "field rig",
        (UnitClass::Engineer, _) => "engineer",
        _ if unit.is_air() => "aircraft",
        (_, MoveClass::Submarine) => "submarine",
        (_, MoveClass::Naval) => "ship",
        (_, MoveClass::Hover) => "hovercraft",
        (_, MoveClass::Seabed) => "amphibious walker",
        _ => "land unit",
    }
    .to_string()
}

/// What a weapon does, a second, against one target.
fn damage_per_second(weapon: &WeaponDef) -> f64 {
    fp(weapon.damage) * f64::from(weapon.shots_per_reload()) * PER_SECOND / f64::from(weapon.reload_ticks.max(1))
}

/// A weapon's stats, as labelled values, leaving out what it does not have.
fn weapon_stats(weapon: &WeaponDef) -> Vec<(&'static str, String)> {
    let mut rows: Vec<(&'static str, String)> = Vec::new();
    let mut add = |label: &'static str, value: String| rows.push((label, value));
    let kind = match weapon.kind {
        WeaponKind::Shell if weapon.artillery => "artillery",
        WeaponKind::Shell if weapon.targets == Targets::Naval => "torpedoes",
        WeaponKind::Shell if weapon.homing => "homing missiles",
        WeaponKind::Shell => "shells",
        WeaponKind::TacticalMissile if weapon.automatic => "cruise missiles, on its own",
        WeaponKind::TacticalMissile => "cruise missiles, on order",
        WeaponKind::StrategicMissile => "nuclear missile, on order",
        WeaponKind::Bomb => "bombs",
        WeaponKind::Flak => "flak",
        WeaponKind::Tractor => "tractor beam",
        WeaponKind::Laser => "a laser beam",
    };
    add("Fires", kind.to_string());
    add(
        "At",
        match weapon.targets {
            Targets::Ground => "ground and naval targets",
            Targets::Air => "aircraft only",
            Targets::Both => "aircraft and ground targets",
            Targets::Naval => "ships and submarines only",
        }
        .to_string(),
    );
    match weapon.salvo {
        Some(salvo) => add(
            "Damage",
            format!("{} a shot, {} shots over {}, a volley every {}", number(fp(weapon.damage), 0), salvo.shots, seconds(salvo.ticks), seconds(weapon.reload_ticks)),
        ),
        None => add("Damage", format!("{} every {}", number(fp(weapon.damage), 0), seconds(weapon.reload_ticks))),
    }
    add("Damage a second", number(damage_per_second(weapon), 1));
    add("Range", number(fp(weapon.range), 0));
    if weapon.min_range > Fp::ZERO {
        add("Least range", number(fp(weapon.min_range), 0));
    }
    if weapon.setup_ticks > 0 {
        add("Sets up", format!("{} standing still before it fires", seconds(weapon.setup_ticks)));
    }
    if weapon.splash_radius > Fp::ZERO {
        add("Blast radius", number(fp(weapon.splash_radius), 0));
    }
    if weapon.inner_radius > Fp::ZERO {
        add("Inner blast", format!("{} more within {}", number(fp(weapon.inner_damage), 0), number(fp(weapon.inner_radius), 0)));
    }
    if weapon.kind == WeaponKind::Laser {
        add("Shot speed", "hits at once".to_string());
    } else if weapon.projectile_speed > Fp::ZERO {
        let arc = if weapon.lobbed { ", in a high arc" } else { "" };
        add("Shot speed", format!("{} a second{arc}", number(fp(weapon.projectile_speed) * PER_SECOND, 0)));
    }
    add("Turret", weapon.turn_rate.map_or_else(|| "fires any way at once".to_string(), |rate| format!("turns {rate}° a second")));
    if weapon.spread > Fp::ZERO {
        add("Scatter", format!("lands within {} of its aim", number(fp(weapon.spread), 0)));
    }
    if weapon.cruise_height > Fp::ZERO {
        add("Cruise height", number(fp(weapon.cruise_height), 0));
    }
    if weapon.energy_per_shot > Res::ZERO {
        add("Energy a shot", number(res(weapon.energy_per_shot), 0));
    }
    if weapon.stun_ticks > 0 {
        add("Stuns", format!("for {}", seconds(weapon.stun_ticks)));
    }
    if weapon.crush_ticks > 0 {
        add("Crushes", format!("what it holds, after {}", seconds(weapon.crush_ticks)));
    }
    rows
}

/// A table of labelled values. Its class, `stats` or `weapon`, is how the comparison finds it.
fn table(out: &mut String, class: &str, rows: &[(&'static str, String)]) {
    let _ = write!(out, "<table class=\"{class}\">");
    for (label, value) in rows {
        // Values are numbers and fixed words, or links made above from escaped names.
        let _ = write!(out, "<tr><th>{label}</th><td>{value}</td></tr>");
    }
    out.push_str("</table>");
}

/// Where each unit's portrait is on the portrait sheet, `assets/portraits.txt`: its tile size
/// and columns, then the units, one a line, in the sheet's order. Portraits are shown at
/// `PORTRAIT_PX` whatever the tile size, so only the columns matter here.
struct Portraits<'a> {
    columns: usize,
    keys: Vec<&'a str>,
}

impl<'a> Portraits<'a> {
    fn parse(text: &'a str) -> Self {
        let mut lines = text.lines();
        let columns = lines.next().and_then(|header| header.split_whitespace().nth(1)).and_then(|value| value.parse().ok()).unwrap_or(12_usize).max(1);
        Self { columns, keys: lines.filter(|line| !line.is_empty()).collect() }
    }

    fn rows(&self) -> usize {
        self.keys.len().div_ceil(self.columns)
    }

    fn index(&self, key: &str) -> Option<usize> {
        self.keys.iter().position(|candidate| *candidate == key)
    }
}

/// How large a portrait is shown, in CSS pixels.
const PORTRAIT_PX: usize = 72;

/// The CSS class of a backdrop, which `STYLE` paints to match the game's buttons.
fn ground_class(ground: Ground) -> &'static str {
    match ground {
        Ground::Land => "g-land",
        Ground::Coast => "g-coast",
        Ground::Air => "g-air",
        Ground::Ocean => "g-ocean",
        Ground::Deep => "g-deep",
    }
}

fn card(out: &mut String, unit: &Blueprint, db: &BlueprintDb, portraits: &Portraits, kind: Kind) {
    let anchor = match kind {
        Kind::Land => "land",
        Kind::Air => "air",
        Kind::Naval => "naval",
        Kind::Structure => "structure",
    };
    let _ = write!(out, "<article class=\"unit\" id=\"{}\" data-kind=\"{anchor}\" data-tier=\"{}\" data-name=\"{}\">", escape(&unit.key), unit.tier, escape(&unit.name.to_lowercase()));
    out.push_str("<header>");
    let ground = Ground::of(unit);
    let _ = write!(out, "<span class=\"backdrop {}\" title=\"Can go: {}\">", ground_class(ground), ground.name());
    match portraits.index(&unit.key) {
        Some(index) => {
            let (column, row) = (index % portraits.columns, index / portraits.columns);
            let _ = write!(
                out,
                "<span class=\"portrait\" role=\"img\" aria-label=\"{}\" style=\"background-size:{}px {}px;background-position:-{}px -{}px\"></span>",
                escape(&unit.name),
                portraits.columns * PORTRAIT_PX,
                portraits.rows() * PORTRAIT_PX,
                column * PORTRAIT_PX,
                row * PORTRAIT_PX
            );
        }
        None => out.push_str("<span class=\"portrait none\"></span>"),
    }
    out.push_str("</span>");
    let _ = write!(out, "<div><h4>{}</h4><p class=\"sub\">T{} {}</p></div>", escape(&unit.name), unit.tier, what_it_is(unit));
    // Picks the unit for the comparison; the script shows it, as without scripts there is none.
    let _ = write!(out, "<button type=\"button\" class=\"pick\" aria-pressed=\"false\" aria-label=\"Compare {}\" hidden>Compare</button></header>", escape(&unit.name));
    // At a glance: what it costs, how tough it is, and what it does a second.
    let weapons: Vec<&WeaponDef> = unit.weapon.iter().chain(&unit.extra_weapons).collect();
    let dps: f64 = weapons.iter().map(|weapon| damage_per_second(weapon)).sum();
    out.push_str("<dl class=\"glance\">");
    let _ = write!(out, "<div><dt>Materiel</dt><dd>{}</dd></div>", number(res(unit.mass), 0));
    let _ = write!(out, "<div><dt>Energy</dt><dd>{}</dd></div>", number(res(unit.energy), 0));
    let _ = write!(out, "<div><dt>Health</dt><dd>{}</dd></div>", number(fp(unit.health), 0));
    if dps > 0.0 {
        let _ = write!(out, "<div><dt>Damage/s</dt><dd>{}</dd></div>", number(dps, 0));
    }
    out.push_str("</dl>");
    table(out, "stats", &stats(unit, db));
    for (index, weapon) in weapons.iter().enumerate() {
        let title = if weapons.len() == 1 { "Weapon".to_string() } else { format!("Weapon {}", index + 1) };
        let _ = write!(out, "<h5>{title}</h5>");
        table(out, "weapon", &weapon_stats(weapon));
    }
    if !unit.builds.is_empty() {
        let _ = write!(out, "<details><summary>Builds {} kinds</summary><p class=\"builds\">", unit.builds.len());
        let links: Vec<String> = unit.builds.iter().map(|&made| db.get(made)).map(|made| format!("<a href=\"#{}\">{}</a>", escape(&made.key), escape(&made.name))).collect();
        out.push_str(&links.join(", "));
        out.push_str("</p></details>");
    }
    out.push_str("</article>");
}

pub(crate) const STYLE: &str = r#"
:root { color-scheme: dark; --bg: #05060a; --panel: #10131b; --panel-raised: #171b26; --line: #262c3a; --text: #c8ccd4; --text-strong: #eef0f4; --muted: #8a91a0; --accent: #3c78ff; --accent-strong: #5b8eff; }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--text); font: 15px/1.45 system-ui, sans-serif; }
a { color: var(--accent-strong); }
.page { max-width: 1240px; margin: 0 auto; padding: 24px 16px 64px; }
h1 { color: var(--text-strong); font-size: 28px; margin: 8px 0 4px; }
h2 { color: var(--text-strong); font-size: 22px; margin: 40px 0 8px; padding-top: 8px; border-top: 1px solid var(--line); }
h3 { color: var(--muted); font-size: 14px; text-transform: uppercase; letter-spacing: 0.08em; margin: 20px 0 10px; }
.lede { color: var(--muted); margin: 0 0 16px; max-width: 70ch; }
.bar { position: sticky; top: 0; z-index: 1; display: flex; flex-wrap: wrap; gap: 8px 16px; align-items: center; padding: 10px 0; background: var(--bg); border-bottom: 1px solid var(--line); }
.chips { display: flex; flex-wrap: wrap; gap: 6px; }
.chips button { font: inherit; font-size: 13px; color: var(--text); background: var(--panel); border: 1px solid var(--line); border-radius: 999px; padding: 4px 12px; cursor: pointer; }
.chips button[aria-pressed="true"] { background: var(--accent); border-color: var(--accent); color: #fff; }
.bar input { font: inherit; color: var(--text-strong); background: var(--panel); border: 1px solid var(--line); border-radius: 6px; padding: 5px 10px; min-width: 0; flex: 1 1 180px; max-width: 280px; }
.count { color: var(--muted); font-size: 13px; }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 340px), 1fr)); gap: 12px; }
.unit { background: var(--panel); border: 1px solid var(--line); border-radius: 10px; padding: 12px; scroll-margin-top: 64px; }
.unit:target { border-color: var(--accent-strong); }
.unit header { display: flex; gap: 12px; align-items: center; }
.backdrop { flex: none; display: block; width: 72px; height: 72px; border-radius: 8px; overflow: hidden; background-color: var(--panel-raised); }
.backdrop.g-land { background: linear-gradient(#58763e, #6a5c3a); }
.backdrop.g-coast { background: linear-gradient(160deg, #b0a06a 0 50%, #46808f 50% 56%, #286894 56%); }
.backdrop.g-air { background: radial-gradient(circle at 22% 80%, #fffc 0 9px, #fff0 10px), radial-gradient(circle at 80% 26%, #fffc 0 8px, #fff0 9px), linear-gradient(#4682c8, #a8d0ec); }
.backdrop.g-ocean { background: repeating-linear-gradient(#bee4f578 0 1px, #0000 1px 18px), linear-gradient(#266ca0, #123a68); }
.backdrop.g-deep { background: repeating-linear-gradient(#78aadc3c 0 1px, #0000 1px 28px), linear-gradient(#0e284e, #040c20); }
.portrait { display: block; width: 72px; height: 72px; background-color: transparent; background-image: url("portraits.png"); background-repeat: no-repeat; }
.portrait.none { background-image: none; }
h4 { color: var(--text-strong); font-size: 17px; margin: 0; }
h5 { color: var(--muted); font-size: 12px; text-transform: uppercase; letter-spacing: 0.06em; margin: 12px 0 4px; }
.sub { color: var(--muted); margin: 2px 0 0; font-size: 13px; }
.glance { display: grid; grid-template-columns: repeat(4, 1fr); gap: 6px; margin: 12px 0 8px; }
.glance div { background: var(--panel-raised); border-radius: 6px; padding: 4px 8px; }
.glance dt { color: var(--muted); font-size: 11px; text-transform: uppercase; letter-spacing: 0.05em; }
.glance dd { margin: 0; color: var(--text-strong); font-variant-numeric: tabular-nums; font-weight: 600; }
table { width: 100%; border-collapse: collapse; font-size: 13px; }
th, td { text-align: left; vertical-align: top; padding: 3px 0; border-top: 1px solid var(--line); }
th { color: var(--muted); font-weight: normal; width: 42%; padding-right: 8px; }
td { color: var(--text); font-variant-numeric: tabular-nums; }
details { margin-top: 10px; font-size: 13px; }
summary { cursor: pointer; color: var(--muted); }
.builds { margin: 6px 0 0; }
.empty { color: var(--muted); }
.unit header > div { flex: 1; min-width: 0; }
.pick { flex: none; align-self: flex-start; font: inherit; font-size: 12px; color: var(--text); background: var(--panel-raised); border: 1px solid var(--line); border-radius: 999px; padding: 3px 10px; cursor: pointer; }
.pick::before { content: "+ "; }
.pick[aria-pressed="true"] { background: var(--accent); border-color: var(--accent); color: #fff; }
.pick[aria-pressed="true"]::before { content: "\2713  "; }
.unit.picked { border-color: var(--accent); }
body.picking .page { padding-bottom: 140px; }
.tray { position: fixed; left: 0; right: 0; bottom: 0; z-index: 2; display: flex; flex-wrap: wrap; gap: 8px 16px; align-items: center; justify-content: space-between; padding: 10px max(16px, calc((100% - 1208px) / 2)); background: var(--panel-raised); border-top: 1px solid var(--line); box-shadow: 0 -6px 18px #0008; }
.picked { display: flex; flex-wrap: wrap; gap: 6px; min-width: 0; flex: 1 1 300px; }
.picked button { font: inherit; font-size: 13px; color: var(--text-strong); background: var(--panel); border: 1px solid var(--line); border-radius: 999px; padding: 3px 6px 3px 12px; cursor: pointer; }
.picked button span { color: var(--muted); margin-left: 6px; }
.tray-actions { display: flex; gap: 8px; }
.go, .back { font: inherit; font-size: 14px; border-radius: 6px; padding: 6px 14px; cursor: pointer; }
.go { color: #fff; background: var(--accent); border: 1px solid var(--accent); font-weight: 600; }
.go:disabled { opacity: 0.5; cursor: default; }
.back { color: var(--text); background: var(--panel); border: 1px solid var(--line); }
#compare .bar h2 { margin: 0; padding: 0; border: 0; font-size: 18px; flex: 1 1 auto; }
.toggle { white-space: nowrap; font-size: 13px; color: var(--text); display: flex; gap: 6px; align-items: center; cursor: pointer; }
#compare .lede { margin-top: 12px; }
.compare-scroll { overflow-x: auto; border: 1px solid var(--line); border-radius: 10px; background: var(--panel); }
.compare-table { --label: 160px; --column: 200px; table-layout: fixed; width: max(100%, calc(var(--label) + var(--units, 2) * var(--column))); font-size: 13px; }
@media (max-width: 600px) { .compare-table { --label: 104px; --column: 150px; } .compare-table th, .compare-table td { padding: 5px 8px; } }
.compare-table th, .compare-table td { width: auto; padding: 5px 10px; }
.compare-table tbody th { position: sticky; left: 0; z-index: 1; background: var(--panel); color: var(--muted); overflow-wrap: anywhere; }
.compare-table td { overflow-wrap: anywhere; }
.compare-table thead th { border-top: 0; vertical-align: bottom; padding-top: 12px; }
.compare-table thead th:first-child { position: sticky; left: 0; z-index: 1; background: var(--panel); width: var(--label); }
.compare-table thead .head { display: flex; flex-direction: column; gap: 6px; }
.compare-table thead .head > div { display: flex; justify-content: space-between; align-items: flex-start; gap: 8px; }
.compare-table thead a { color: var(--text-strong); font-weight: 600; font-size: 15px; text-decoration: none; }
.compare-table thead a:hover { text-decoration: underline; }
.compare-table .remove { flex: none; font: inherit; font-size: 15px; line-height: 1; color: var(--muted); background: none; border: 1px solid var(--line); border-radius: 999px; padding: 1px 8px; cursor: pointer; }
.compare-table tr.section th { white-space: nowrap; position: sticky; left: 0; background: var(--panel-raised); color: var(--text-strong); font-size: 12px; text-transform: uppercase; letter-spacing: 0.06em; padding-top: 8px; }
.compare-table tr.section td { background: var(--panel-raised); }
.compare-table td.best { color: var(--text-strong); font-weight: 600; box-shadow: inset 3px 0 0 var(--accent-strong); }
.compare-table td.none { color: var(--muted); }
[hidden] { display: none !important; }
"#;

/// Filters by kind, tech level and name, and hides headings with nothing left under them; picks
/// units to compare, and shows those picked side by side, at `#compare=<key>,<key>`, read from
/// their cards. Without scripts, everything shows, and there is no comparison.
const SCRIPT: &str = r#"
(() => {
  const state = { kind: "all", tier: "all", text: "" };
  const units = [...document.querySelectorAll(".unit")];
  const byKey = new Map(units.map((unit) => [unit.id, unit]));
  const count = document.getElementById("count");
  const apply = () => {
    let shown = 0;
    for (const unit of units) {
      const visible = (state.kind === "all" || unit.dataset.kind === state.kind)
        && (state.tier === "all" || unit.dataset.tier === state.tier)
        && (state.text === "" || unit.dataset.name.includes(state.text) || unit.id.includes(state.text));
      unit.hidden = !visible;
      if (visible) shown++;
    }
    for (const group of document.querySelectorAll(".tier, .kind")) {
      group.hidden = !group.querySelector(".unit:not([hidden])");
    }
    document.getElementById("none").hidden = shown > 0;
    count.textContent = shown === units.length ? `${shown} units` : `${shown} of ${units.length} units`;
  };
  for (const chips of document.querySelectorAll(".chips")) {
    chips.addEventListener("click", (event) => {
      const button = event.target.closest("button");
      if (!button) return;
      for (const other of chips.querySelectorAll("button")) other.setAttribute("aria-pressed", String(other === button));
      state[chips.dataset.filter] = button.dataset.value;
      apply();
    });
  }
  document.getElementById("search").addEventListener("input", (event) => {
    state.text = event.target.value.trim().toLowerCase();
    apply();
  });
  apply();

  // Picking units to compare, in the order picked.
  let picked = [];
  const list = document.getElementById("list");
  const compare = document.getElementById("compare");
  const tray = document.getElementById("tray");
  const nameOf = (unit) => unit.querySelector("h4").textContent;
  const showPicked = () => {
    const inCompare = !compare.hidden;
    for (const unit of units) {
      const on = picked.includes(unit.id);
      unit.classList.toggle("picked", on);
      unit.querySelector(".pick").setAttribute("aria-pressed", String(on));
    }
    const tags = document.getElementById("picked");
    tags.replaceChildren(...picked.map((key) => {
      const tag = document.createElement("button");
      tag.type = "button";
      tag.dataset.key = key;
      tag.title = `Take ${nameOf(byKey.get(key))} out`;
      tag.textContent = nameOf(byKey.get(key));
      const cross = document.createElement("span");
      cross.textContent = "×";
      tag.append(cross);
      return tag;
    }));
    const go = document.getElementById("compare-go");
    go.disabled = picked.length < 2;
    go.textContent = picked.length < 2 ? "Pick another to compare" : `Compare ${picked.length} units`;
    tray.hidden = inCompare || picked.length === 0;
    document.body.classList.toggle("picking", !tray.hidden);
  };
  const toggle = (key) => {
    picked = picked.includes(key) ? picked.filter((other) => other !== key) : [...picked, key];
    showPicked();
  };
  for (const unit of units) {
    const pick = unit.querySelector(".pick");
    pick.hidden = false;
    pick.addEventListener("click", () => toggle(unit.id));
  }
  document.getElementById("picked").addEventListener("click", (event) => {
    const tag = event.target.closest("button");
    if (tag) toggle(tag.dataset.key);
  });
  document.getElementById("compare-clear").addEventListener("click", () => {
    picked = [];
    showPicked();
  });

  // What a card says: its glance numbers, its stats, and each weapon's, as label and value.
  const rowsOf = (table) => {
    const rows = [];
    for (const row of table.rows) {
      const label = row.cells[0].textContent;
      const value = row.cells[1].innerHTML;
      const same = rows.find((other) => other[0] === label);
      // A label a card has twice, such as "Built on", reads as one value.
      if (same) same[1] += "; " + value; else rows.push([label, value]);
    }
    return rows;
  };
  const read = (unit) => ({
    glance: [...unit.querySelectorAll(".glance div")].map((item) => [item.querySelector("dt").textContent, item.querySelector("dd").textContent]),
    stats: rowsOf(unit.querySelector("table.stats")),
    weapons: [...unit.querySelectorAll("table.weapon")].map(rowsOf),
  });
  // The labels of several lists, each once, keeping the order each list has them in.
  const merge = (lists) => {
    const order = [];
    for (const rows of lists) {
      let at = -1;
      for (const [label] of rows) {
        const found = order.indexOf(label);
        if (found === -1) order.splice(++at, 0, label); else at = found;
      }
    }
    return order;
  };
  // At a glance, the best of each is marked: the cheapest, the toughest, the hardest-hitting.
  const better = { Materiel: -1, Energy: -1, Health: 1, "Damage/s": 1 };
  const table = document.getElementById("compare-table");
  const differences = document.getElementById("differences");
  const render = () => {
    const cards = picked.map((key) => byKey.get(key));
    const facts = cards.map(read);
    document.getElementById("compare-title").textContent = `Comparing ${cards.length} ${cards.length === 1 ? "unit" : "units"}`;
    const head = document.createElement("thead");
    const top = head.insertRow();
    top.append(document.createElement("th"));
    for (const card of cards) {
      const cell = document.createElement("th");
      cell.scope = "col";
      const box = document.createElement("div");
      box.className = "head";
      box.append(card.querySelector(".backdrop").cloneNode(true));
      const line = document.createElement("div");
      const name = document.createElement("a");
      name.href = `#${card.id}`;
      name.textContent = nameOf(card);
      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "remove";
      remove.dataset.key = card.id;
      remove.title = `Take ${nameOf(card)} out`;
      remove.textContent = "×";
      line.append(name, remove);
      const sub = document.createElement("p");
      sub.className = "sub";
      sub.textContent = card.querySelector(".sub").textContent;
      box.append(line, sub);
      cell.append(box);
      top.append(cell);
    }
    const body = document.createElement("tbody");
    const section = (title) => {
      const row = body.insertRow();
      row.className = "section";
      const label = document.createElement("th");
      label.textContent = title;
      row.append(label);
      for (const _ of cards) row.insertCell();
      return row;
    };
    const rows = (title, lists) => {
      const heading = section(title);
      let any = false;
      for (const label of merge(lists)) {
        const values = lists.map((list) => (list.find((row) => row[0] === label) || [])[1]);
        const row = body.insertRow();
        const th = document.createElement("th");
        th.scope = "row";
        th.textContent = label;
        row.append(th);
        const cells = values.map((value) => {
          const cell = row.insertCell();
          if (value === undefined) {
            cell.className = "none";
            cell.textContent = "—";
          } else {
            cell.innerHTML = value;
          }
          return cell;
        });
        const direction = better[label];
        const numbers = values.map((value) => value === undefined ? NaN : parseFloat(value.replace(/,/g, "")));
        const known = numbers.filter((value) => !Number.isNaN(value));
        if (direction && known.length > 1 && new Set(known).size > 1) {
          const best = direction > 0 ? Math.max(...known) : Math.min(...known);
          numbers.forEach((value, index) => { if (value === best) cells[index].classList.add("best"); });
        }
        const same = values.every((value) => value === values[0]);
        row.hidden = differences.checked && same && cards.length > 1;
        if (!row.hidden) any = true;
      }
      heading.hidden = !any;
    };
    rows("At a glance", facts.map((fact) => fact.glance));
    rows("Stats", facts.map((fact) => fact.stats));
    const most = Math.max(0, ...facts.map((fact) => fact.weapons.length));
    for (let index = 0; index < most; index++) {
      rows(most === 1 ? "Weapon" : `Weapon ${index + 1}`, facts.map((fact) => fact.weapons[index] || []));
    }
    // Every unit gets a column as wide as the others, and a phone scrolls them sideways.
    table.style.setProperty("--units", cards.length);
    table.replaceChildren(head, body);
  };
  table.addEventListener("click", (event) => {
    const remove = event.target.closest(".remove");
    if (!remove) return;
    picked = picked.filter((key) => key !== remove.dataset.key);
    if (picked.length === 0) {
      location.hash = "";
      return;
    }
    history.replaceState(null, "", `#compare=${picked.join(",")}`);
    render();
  });
  differences.addEventListener("change", render);

  // The list and the comparison are one page, told apart by the address: `#compare=a,b` for
  // the comparison, so it can be linked to and the back button leaves it.
  let listScroll = 0;
  const route = () => {
    const hash = decodeURIComponent(location.hash.slice(1));
    const wanted = hash.startsWith("compare=") ? hash.slice("compare=".length).split(",").filter((key) => byKey.has(key)) : null;
    if (wanted && wanted.length > 0) {
      if (compare.hidden) listScroll = window.scrollY;
      picked = [...new Set(wanted)];
      list.hidden = true;
      compare.hidden = false;
      render();
      showPicked();
      window.scrollTo(0, 0);
      return;
    }
    const leaving = !compare.hidden;
    compare.hidden = true;
    list.hidden = false;
    showPicked();
    // The game writes this page after the address has loaded, so a link to one unit,
    // `?units#t1_medium_tank`, is scrolled to here, as is one from the comparison.
    const linked = hash && document.getElementById(hash);
    if (linked && linked.classList.contains("unit")) {
      if (linked.hidden) {
        for (const chips of document.querySelectorAll(".chips")) chips.querySelector('[data-value="all"]').click();
        const search = document.getElementById("search");
        search.value = "";
        state.text = "";
        apply();
      }
      linked.scrollIntoView();
    } else if (leaving) {
      window.scrollTo(0, listScroll);
    }
  };
  document.getElementById("compare-go").addEventListener("click", () => {
    if (picked.length > 1) location.hash = `compare=${picked.join(",")}`;
  });
  document.getElementById("compare-back").addEventListener("click", () => {
    location.hash = "";
  });
  window.addEventListener("hashchange", route);
  route();
})();
"#;

/// The portrait sheet's index, built in beside the sheet it describes.
const PORTRAIT_INDEX: &str = include_str!("../assets/portraits.txt");

/// The whole page, from the blueprints the game plays with.
pub fn page(db: &BlueprintDb) -> String {
    let portraits = Portraits::parse(PORTRAIT_INDEX);
    let units: Vec<&Blueprint> = db.iter().filter(|unit| unit.class != UnitClass::Ammo).collect();
    let tiers: Vec<u8> = {
        let mut tiers: Vec<u8> = units.iter().map(|unit| unit.tier).collect();
        tiers.sort_unstable();
        tiers.dedup();
        tiers
    };
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>Unit database</title>\n<meta name=\"description\" content=\"Every unit and structure in sim-engine, with all its numbers.\">\n");
    out.push_str("<!-- Written by the game from the blueprints it plays with (data/units.toml), by src/unit_database.rs. -->\n");
    let _ = writeln!(out, "<style>{STYLE}</style>\n</head>\n<body>\n<div class=\"page\">");
    out.push_str("<p><a href=\"./\">&larr; Back to the game</a></p>\n<h1>Unit database</h1>\n");
    let _ = writeln!(
        out,
        "<p class=\"lede\">Every unit and structure, {} in all, with all its numbers, by kind and tech level. Distances are in world units, where a terrain cell is 20 across; build time is the build power it takes for a second, so a builder with 10 build power takes a tenth of it in seconds.</p>",
        units.len()
    );
    out.push_str("<div id=\"list\">\n<div class=\"bar\">\n<div class=\"chips\" data-filter=\"kind\" role=\"group\" aria-label=\"Kind\"><button type=\"button\" data-value=\"all\" aria-pressed=\"true\">All</button>");
    for (kind, heading, _) in KINDS {
        let value = match kind {
            Kind::Land => "land",
            Kind::Air => "air",
            Kind::Naval => "naval",
            Kind::Structure => "structure",
        };
        let _ = write!(out, "<button type=\"button\" data-value=\"{value}\" aria-pressed=\"false\">{heading}</button>");
    }
    out.push_str("</div>\n<div class=\"chips\" data-filter=\"tier\" role=\"group\" aria-label=\"Tech level\"><button type=\"button\" data-value=\"all\" aria-pressed=\"true\">Every tech level</button>");
    for tier in &tiers {
        let _ = write!(out, "<button type=\"button\" data-value=\"{tier}\" aria-pressed=\"false\">T{tier}</button>");
    }
    out.push_str("</div>\n<input id=\"search\" type=\"search\" placeholder=\"Find a unit\" aria-label=\"Find a unit by name\">\n<span class=\"count\" id=\"count\"></span>\n</div>\n");
    for (kind, heading, anchor) in KINDS {
        let of_kind: Vec<&&Blueprint> = units.iter().filter(|unit| kind_of(unit) == kind).collect();
        if of_kind.is_empty() {
            continue;
        }
        let _ = writeln!(out, "<section class=\"kind\" id=\"{anchor}\">\n<h2>{heading}</h2>");
        for tier in &tiers {
            let mut of_tier: Vec<&&Blueprint> = of_kind.iter().copied().filter(|unit| unit.tier == *tier).collect();
            if of_tier.is_empty() {
                continue;
            }
            // Cheapest first, so that each level reads from its basics up.
            of_tier.sort_by(|a, b| a.mass.cmp(&b.mass).then_with(|| a.name.cmp(&b.name)));
            let _ = writeln!(out, "<div class=\"tier\"><h3>Tech {tier}</h3>\n<div class=\"grid\">");
            for unit in of_tier {
                card(&mut out, unit, db, &portraits, kind);
                out.push('\n');
            }
            out.push_str("</div></div>\n");
        }
        out.push_str("</section>\n");
    }
    out.push_str("<p class=\"empty\" id=\"none\" hidden>No unit matches.</p>\n</div>\n");
    // The comparison, which the script fills from the cards above, so it shows just what they do.
    out.push_str(concat!(
        "<section id=\"compare\" hidden>\n",
        "<div class=\"bar\"><button type=\"button\" class=\"back\" id=\"compare-back\">&larr; All units</button>",
        "<h2 id=\"compare-title\">Comparing units</h2>",
        "<label class=\"toggle\"><input type=\"checkbox\" id=\"differences\"> Only what differs</label></div>\n",
        "<p class=\"lede\">Side by side, with the cheapest, toughest and hardest-hitting at a glance marked. A unit's name goes to its card.</p>\n",
        "<div class=\"compare-scroll\"><table class=\"compare-table\" id=\"compare-table\"></table></div>\n",
        "</section>\n",
        // The units picked so far, while looking through the list.
        "<div class=\"tray\" id=\"tray\" hidden><div class=\"picked\" id=\"picked\"></div>",
        "<div class=\"tray-actions\"><button type=\"button\" class=\"go\" id=\"compare-go\">Compare</button>",
        "<button type=\"button\" class=\"back\" id=\"compare-clear\">Clear</button></div></div>\n",
    ));
    let _ = writeln!(out, "</div>\n<script>{SCRIPT}</script>\n</body>\n</html>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_read_well() {
        assert_eq!(number(1234567.0, 0), "1,234,567");
        assert_eq!(number(12.50, 1), "12.5");
        assert_eq!(number(3.0, 2), "3");
        assert_eq!(number(-1500.0, 0), "-1,500");
        assert_eq!(number(999.0, 0), "999");
    }

    #[test]
    fn code_names_read_as_words() {
        assert_eq!(words("FieldRig"), "field rig");
        assert_eq!(words("T1Power"), "T1 power");
        assert_eq!(words("Submarine"), "submarine");
    }

    /// Every unit but missiles is on the page once, under its kind and tech level, with all
    /// its weapons, and each has its portrait.
    #[test]
    fn every_unit_is_listed_once_with_its_weapons_and_portrait() {
        let db = BlueprintDb::standard();
        let html = page(&db);
        let portraits = Portraits::parse(PORTRAIT_INDEX);
        for unit in db.iter() {
            let anchor = format!("id=\"{}\"", unit.key);
            let listed = html.matches(&anchor).count();
            if unit.class == UnitClass::Ammo {
                assert_eq!(listed, 0, "{} is a missile, not a unit", unit.key);
                continue;
            }
            assert_eq!(listed, 1, "{} is listed once", unit.key);
            let start = html.find(&anchor).expect("listed");
            let card = &html[start..start + html[start..].find("</article>").expect("closed")];
            let weapons = unit.weapon.iter().count() + unit.extra_weapons.len();
            assert_eq!(card.matches("<h5>Weapon").count(), weapons, "{} shows all its weapons", unit.key);
            assert!(card.contains(&format!("data-tier=\"{}\"", unit.tier)), "{} is under its tech level", unit.key);
            assert!(portraits.index(&unit.key).is_some(), "{} has a portrait", unit.key);
            assert!(!card.contains("portrait none"), "{} shows its portrait", unit.key);
            let ground = Ground::of(unit);
            assert!(card.contains(&format!("class=\"backdrop {}\"", ground_class(ground))), "{} sits on its {ground:?} backdrop", unit.key);
            assert!(card.contains(&format!("<td>{}</td>", ground.name())) || unit.is_structure(), "{} says where it can go", unit.key);
            assert_eq!(card.matches("<table class=\"weapon\">").count(), weapons, "{} marks each weapon for the comparison", unit.key);
            assert_eq!(card.matches("<table class=\"stats\">").count(), 1, "{} marks its stats for the comparison", unit.key);
            assert_eq!(card.matches("class=\"pick\"").count(), 1, "{} can be picked to compare", unit.key);
        }
        for (key, class) in [("t1_medium_tank", "g-land"), ("t1_interceptor", "g-air"), ("t1_frigate", "g-ocean"), ("t1_attack_sub", "g-deep"), ("t2_assault_hovercraft", "g-coast")] {
            let start = html.find(&format!("id=\"{key}\"")).expect("listed");
            assert!(html[start..start + 400].contains(class), "{key} is on {class}");
        }
    }

    /// The comparison has its place on the page, outside the list it hides, and its tray.
    #[test]
    fn the_page_has_a_comparison() {
        let html = page(&BlueprintDb::standard());
        let list = html.find("<div id=\"list\">").expect("the list is wrapped");
        let compare = html.find("<section id=\"compare\" hidden>").expect("the comparison is there, hidden");
        assert!(list < compare, "the comparison comes after the list");
        assert!(html[list..compare].contains("class=\"unit\""), "the cards are in the list");
        assert!(!html[compare..].contains("class=\"unit\""), "no card is in the comparison");
        for id in ["compare-table", "differences", "compare-back", "tray", "picked", "compare-go", "compare-clear"] {
            assert_eq!(html.matches(&format!("id=\"{id}\"")).count(), 1, "{id} is on the page once");
        }
    }
}
