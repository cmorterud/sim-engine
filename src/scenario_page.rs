//! The playtest scenarios page (`?scenarios`, linked from the landing page): every scenario in
//! `data/scenarios.toml`, what it shows and what to try, each side's units with their materiel,
//! reach, and sight, and links to play it with fog of war on or off. The game writes it in the
//! browser from the scenarios and blueprints built into it, the ones it plays, so the page
//! always says what the game does.

use std::fmt::Write as _;

use crate::math::fp::Fp;
use crate::sim::blueprints::BlueprintDb;
use crate::sim::playtest_scenarios::{Group, PlaytestScenario};
use crate::unit_database::{escape, number, seconds, STYLE};

const EXTRA_STYLE: &str = r#"
.scenarios { display: grid; gap: 16px; margin-top: 20px; }
.scenario { background: var(--panel); border: 1px solid var(--line); border-radius: 10px; padding: 16px; scroll-margin-top: 16px; }
.scenario:target { border-color: var(--accent-strong); }
.scenario h2 { border: 0; margin: 0 0 6px; padding: 0; font-size: 20px; }
.scenario p { margin: 6px 0; max-width: 80ch; }
.try { color: var(--text-strong); }
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

fn materiel(db: &BlueprintDb, group: &Group) -> f64 {
    db.get(db.id(&group.unit)).mass.to_num::<f64>() * f64::from(group.count)
}

/// One group, with the numbers that matter in these fights.
fn group_item(out: &mut String, db: &BlueprintDb, group: &Group, red: bool) {
    let unit = db.get(db.id(&group.unit));
    let count = if unit.is_structure() { String::new() } else { format!("{} &times; ", group.count) };
    let order = match (red, unit.is_structure(), group.attacks()) {
        (false, ..) => "",
        (true, true, _) => " &middot; holds",
        (true, false, true) => " &middot; attacks at once",
        (true, false, false) => " &middot; holds its ground",
    };
    let mut numbers = vec![format!("{} materiel", number(materiel(db, group), 0))];
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

fn side(out: &mut String, db: &BlueprintDb, groups: &[Group], red: bool) {
    let (class, heading) = if red { ("red", "Red") } else { ("blue", "You, Blue") };
    let _ = write!(out, "<div class=\"side {class}\"><h3>{heading}</h3><ul>");
    for group in groups {
        group_item(out, db, group, red);
    }
    let total: f64 = groups.iter().map(|group| materiel(db, group)).sum();
    let _ = write!(out, "</ul><p class=\"total\">{} materiel in all</p></div>", number(total, 0));
}

/// The page, from the scenarios and the blueprints the game plays with.
pub fn page(scenarios: &[PlaytestScenario], db: &BlueprintDb) -> String {
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>Playtest scenarios</title>\n<meta name=\"description\" content=\"The balance harness's fights, to play by hand.\">\n");
    out.push_str("<!-- Written by the game from data/scenarios.toml and data/units.toml, by src/scenario_page.rs. -->\n");
    let _ = writeln!(out, "<style>{STYLE}{EXTRA_STYLE}</style>\n</head>\n<body>\n<div class=\"page\">");
    out.push_str("<p><a href=\"./\">&larr; Back to the game</a></p>\n<h1>Playtest scenarios</h1>\n");
    out.push_str("<p class=\"lede\">The fights the balance harness measures, to play yourself. Each is on flat open ground. You play Blue, in the west, with only the units listed: no commander, factories, or income. Red's units either attack at once or hold their ground. The bar at the top of the game shows how much of each side's materiel is left, weighted by health, as the harness scores a fight: with fog of war on, Red's is hidden. Reload the page to play a scenario again.</p>\n");
    out.push_str("<div class=\"scenarios\">\n");
    for scenario in scenarios {
        let id = escape(&scenario.id);
        let _ = writeln!(out, "<article class=\"scenario\" id=\"{id}\">\n<h2>{}</h2>\n<p>{}</p>\n<p class=\"try\"><b>Try:</b> {}</p>", escape(&scenario.title), escape(&scenario.shows), escape(&scenario.to_try));
        out.push_str("<div class=\"sides\">");
        side(&mut out, db, &scenario.blue, false);
        side(&mut out, db, &scenario.red, true);
        out.push_str("</div>\n");
        let _ = writeln!(
            out,
            "<div class=\"play\"><a class=\"primary\" href=\"?scenario={id}&amp;fog=on\">Play with fog of war</a><a class=\"secondary\" href=\"?scenario={id}&amp;fog=off\">Play without fog of war</a></div>\n</article>"
        );
    }
    out.push_str("</div>\n</div>\n</body>\n</html>\n");
    out
}
