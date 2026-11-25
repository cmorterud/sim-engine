//! The playtest scenarios page (`?scenarios`, linked from the landing page): every scenario in
//! `data/scenarios.toml`, what it shows and what to try, each side's units with their materiel,
//! reach, and sight, and links to play it with fog of war on or off. The game writes it in the
//! browser from the scenarios and blueprints built into it, the ones it plays, so the page
//! always says what the game does.

use std::fmt::Write as _;

use crate::balance::{cost_at, TARGETS};
use crate::math::fp::Fp;
use crate::sim::blueprints::{BlueprintDb, UnitClass};
use crate::sim::playtest_scenarios::{Group, PlaytestScenario};
use crate::unit_database::{escape, number, seconds, STYLE};

const EXTRA_STYLE: &str = r#"
.scenarios { display: grid; gap: 16px; margin-top: 20px; }
.scenario { background: var(--panel); border: 1px solid var(--line); border-radius: 10px; padding: 16px; scroll-margin-top: 16px; }
.scenario:target { border-color: var(--accent-strong); }
.scenario h2 { border: 0; margin: 0 0 6px; padding: 0; font-size: 20px; }
.scenario p { margin: 6px 0; max-width: 80ch; }
.try { color: var(--text-strong); }
.ground, .target { color: var(--muted); font-size: 14px; }
.target b { font-weight: 600; }
.try b { color: var(--muted); font-weight: 600; }
.sides { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 300px), 1fr)); gap: 12px; margin: 12px 0; }
.side { background: var(--panel-raised); border-radius: 8px; padding: 10px 12px; }
.side h3 { margin: 0 0 6px; }
.side.blue h3 { color: #6f9bff; }
.side.red h3 { color: #ff7a6b; }
.side ul { list-style: none; margin: 0; padding: 0; }
.side li { padding: 5px 0; border-top: 1px solid var(--line); }
.side li:first-child { border-top: 0; }
.side .what { color: var(--text-strong); }
.side .numbers { color: var(--muted); font-size: 13px; font-variant-numeric: tabular-nums; }
.side .total { margin-top: 6px; color: var(--muted); font-size: 13px; }
.play { display: flex; flex-wrap: wrap; gap: 8px; }
.play a { display: inline-block; text-decoration: none; font-weight: 600; border-radius: 8px; padding: 8px 16px; border: 1px solid var(--accent); }
.play a.primary { background: var(--accent); color: #fff; }
.play a.secondary { color: var(--accent-strong); }
.play a:focus-visible { outline: 2px solid var(--text-strong); outline-offset: 2px; }
"#;

/// A group's materiel at a tier's prices, as the balance harness weighs a fight (`cost_at`):
/// energy valued at the tier's rate, so ships and aircraft, which spend far more energy for
/// their materiel than land units, are weighed as the harness weighs them. A silo's is with the
/// missiles it starts with, which cost far more than it does.
fn materiel(db: &BlueprintDb, group: &Group, tier: u8) -> f64 {
    let unit = db.get(db.id(&group.unit));
    let stocked = stocked(db, group).map_or(0.0, |(missile, stock)| cost_at(db.get(missile), tier) * f64::from(stock));
    cost_at(unit, tier) * f64::from(group.count) + stocked
}

/// What a silo's group starts with built, and how many: none for anything else.
fn stocked(db: &BlueprintDb, group: &Group) -> Option<(crate::sim::blueprints::BlueprintId, u32)> {
    let stock = group.stock.filter(|&stock| stock > 0)?;
    let missile = db.get(db.id(&group.unit)).builds.iter().copied().find(|&made| db.get(made).class == UnitClass::Ammo)?;
    Some((missile, stock))
}

/// A side's materiel at a tier's prices.
pub fn side_materiel(db: &BlueprintDb, groups: &[Group], tier: u8) -> f64 {
    groups.iter().map(|group| materiel(db, group, tier)).sum()
}

/// A target's value from data/balance_targets.toml, by its dotted path, as the file writes it.
pub fn target_value(path: &str) -> Option<String> {
    let table: toml::Table = TARGETS.parse().ok()?;
    let mut value = table.get(path.split('.').next()?)?;
    for key in path.split('.').skip(1) {
        value = value.get(key)?;
    }
    Some(match value {
        toml::Value::Array(items) => format!("[{}]", items.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")),
        other => other.to_string(),
    })
}

/// One group, with the numbers that matter in these fights.
fn group_item(out: &mut String, db: &BlueprintDb, group: &Group, red: bool, tier: u8) {
    let unit = db.get(db.id(&group.unit));
    let count = if unit.is_structure() { String::new() } else { format!("{} &times; ", group.count) };
    let order = match (red, unit.is_structure(), group.attacks()) {
        (false, ..) => "",
        (true, true, _) => " &middot; holds",
        (true, false, true) => " &middot; attacks at once",
        (true, false, false) => " &middot; holds its ground",
    };
    let mut numbers = vec![format!("{} materiel", number(materiel(db, group, tier), 0))];
    if let Some((missile, stock)) = stocked(db, group) {
        numbers.push(format!("{stock} {}s built, of {} it keeps", escape(&db.get(missile).name), unit.max_stock));
    }
    if let Some(weapon) = unit.weapon {
        numbers.push(format!("reach {}", number(weapon.range.to_num::<f64>(), 0)));
        if weapon.min_range > Fp::ZERO {
            numbers.push(format!("least {}", number(weapon.min_range.to_num::<f64>(), 0)));
        }
        if weapon.setup_ticks > 0 {
            numbers.push(format!("sets up in {}", seconds(weapon.setup_ticks)));
        }
    }
    numbers.push(format!("sight {}", number(unit.vision.to_num::<f64>(), 0)));
    if !unit.is_structure() {
        numbers.push(format!("speed {}", number(unit.speed.to_num::<f64>(), 1)));
    }
    let _ = write!(
        out,
        "<li><span class=\"what\">{count}<a href=\"?units#{}\">{}</a>{order}</span><br><span class=\"numbers\">{}</span></li>",
        escape(&unit.key),
        escape(&unit.name),
        numbers.join(" &middot; "),
    );
}

fn side(out: &mut String, db: &BlueprintDb, groups: &[Group], red: bool, tier: u8) {
    let (class, heading) = if red { ("red", "Red") } else { ("blue", "You, Blue") };
    let _ = write!(out, "<div class=\"side {class}\"><h3>{heading}</h3><ul>");
    for group in groups {
        group_item(out, db, group, red, tier);
    }
    let _ = write!(out, "</ul><p class=\"total\">{} materiel in all, at T{tier} prices</p></div>", number(side_materiel(db, groups, tier), 0));
}

/// The page, from the scenarios and the blueprints the game plays with.
pub fn page(scenarios: &[PlaytestScenario], db: &BlueprintDb) -> String {
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>Playtest scenarios</title>\n<meta name=\"description\" content=\"The balance harness's fights, to play by hand.\">\n");
    out.push_str("<!-- Written by the game from data/scenarios.toml and data/units.toml, by src/scenario_page.rs. -->\n");
    let _ = writeln!(out, "<style>{STYLE}{EXTRA_STYLE}</style>\n</head>\n<body>\n<div class=\"page\">");
    out.push_str("<p><a href=\"./\">&larr; Back to the game</a></p>\n<h1>Playtest scenarios</h1>\n");
    out.push_str("<p class=\"lede\">The fights the balance harness measures, to play yourself. Each is on flat open ground, open sea, or a shore, the sea in the west and land in the east, as it says. Materiel is valued as the harness values it, energy at the fight's tier's rate. You play Blue, in the west, with only the units listed: no commander, factories, or income. Red's units either attack at once or hold their ground. The bar at the top of the game shows how much of each side's materiel is left, weighted by health, as the harness scores a fight: with fog of war on, Red's is hidden. Reload the page to play a scenario again.</p>\n");
    out.push_str("<div class=\"scenarios\">\n");
    for scenario in scenarios {
        let id = escape(&scenario.id);
        let tier = scenario.tier(db);
        let _ = writeln!(out, "<article class=\"scenario\" id=\"{id}\">\n<h2>{}</h2>\n<p class=\"ground\">On {}, at T{tier} prices.</p>\n<p>{}</p>\n<p class=\"try\"><b>Try:</b> {}</p>", escape(&scenario.title), escape(scenario.ground.name()), escape(&scenario.shows), escape(&scenario.to_try));
        if let Some(target) = &scenario.target {
            let value = target_value(target).unwrap_or_else(|| "missing".to_string());
            let _ = writeln!(out, "<p class=\"target\"><b>Balance target:</b> <code>{}</code> = <code>{}</code>, in data/balance_targets.toml</p>", escape(target), escape(&value));
        }
        out.push_str("<div class=\"sides\">");
        side(&mut out, db, &scenario.blue, false, tier);
        side(&mut out, db, &scenario.red, true, tier);
        out.push_str("</div>\n");
        let _ = writeln!(
            out,
            "<div class=\"play\"><a class=\"primary\" href=\"?scenario={id}&amp;fog=on\">Play with fog of war</a><a class=\"secondary\" href=\"?scenario={id}&amp;fog=off\">Play without fog of war</a></div>\n</article>"
        );
    }
    out.push_str("</div>\n</div>\n</body>\n</html>\n");
    out
}
