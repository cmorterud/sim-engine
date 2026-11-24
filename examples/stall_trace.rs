#![allow(clippy::disallowed_types, clippy::float_arithmetic)]
//! Plays one soak match and prints, once a minute of game time, where each side stands: income
//! and stores, structures, engineers, the army by domain, its orders, how far toward the enemy
//! it stands, mass built and lost that minute, and each commander's health and nearby enemies.
//! For finding out why a match the soak reports as unfinished never ends.
//!
//! `cargo run --release --example stall_trace -- <seed> [map] [minutes between lines]`,
//! where the map is a bundled one, such as `twin_passes`, or `skirmish`.
use std::collections::BTreeMap;
use std::sync::Arc;

use sim_engine::ai::play_match_with;
use sim_engine::ai_strategy::Strategy;
use sim_engine::sim::blueprints::UnitClass;
use sim_engine::sim::components::{BuildTask, Builder, Blueprinted, Construction, Factory, Health, MoveClass, Order, OrderKind, Position, UnitStats};
use sim_engine::sim::scenario::MapSource;
use sim_engine::sim::world::SimWorld;
use sim_engine::soak::MATCH_TICK_LIMIT;

const TICKS_PER_MINUTE: u32 = 600;

#[derive(Default)]
struct Side {
    structures: usize,
    defenses: usize,
    engineers: usize,
    /// Count and mass by domain: land, hover, naval, air.
    army: [(usize, f64); 4],
    orders: [usize; 4],
    /// Combat units in the home third, the middle, and the enemy's third of the way across.
    zones: [usize; 3],
    /// Every unit by blueprint key, printed when `STALL_DETAIL` is set.
    keys: BTreeMap<String, usize>,
}

fn domain(class: UnitClass, move_class: MoveClass) -> Option<usize> {
    if matches!(class, UnitClass::Commander | UnitClass::FieldRig | UnitClass::Engineer | UnitClass::Factory | UnitClass::Economy | UnitClass::Defense | UnitClass::Ammo) {
        return None;
    }
    Some(match move_class {
        MoveClass::Land | MoveClass::Seabed => 0,
        MoveClass::Hover => 1,
        MoveClass::Naval | MoveClass::Submarine => 2,
        MoveClass::Air => 3,
    })
}

fn xy(world: &SimWorld, entity: hecs::Entity) -> (f64, f64) {
    let p = world.ecs.get::<&Position>(entity).unwrap().0;
    (p.x.to_num(), p.y.to_num())
}

/// How far from `home` toward `enemy` a point lies, 0 at home and 1 at the enemy's start.
fn progress(home: (f64, f64), enemy: (f64, f64), at: (f64, f64)) -> f64 {
    let (dx, dy) = (enemy.0 - home.0, enemy.1 - home.1);
    ((at.0 - home.0) * dx + (at.1 - home.1) * dy) / (dx * dx + dy * dy)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let seed: u64 = args.first().expect("a seed").parse().expect("a number");
    let source = match args.get(1).map(String::as_str) {
        None | Some("skirmish") => MapSource::Skirmish,
        Some(name) => MapSource::Map(Arc::new(sim_engine::sim::maps::bundled(name).expect("a bundled map"))),
    };
    let every: u32 = args.get(2).and_then(|text| text.parse().ok()).unwrap_or(2);
    let balanced = Strategy::balanced();
    // Mass alive by unit id at the last look, to count what was built and lost since.
    let mut alive: BTreeMap<u32, (usize, f64)> = BTreeMap::new();
    let mut built = [0.0f64; 2];
    let mut lost = [0.0f64; 2];
    let mut lowest_commander = [1.0f64; 2];
    let record = play_match_with(&source, seed, MATCH_TICK_LIMIT, [balanced, balanced], |world| {
        let tick = world.current_tick;
        if tick % 10 == 0 {
            let mut now = BTreeMap::new();
            for &(id, entity) in &world.units {
                let (Ok(stats), Ok(unit)) = (world.ecs.get::<&UnitStats>(entity), world.ecs.get::<&Blueprinted>(entity)) else { continue };
                let mass: f64 = world.blueprints.get(unit.0).mass.to_num();
                let before = alive.get(&id.0).map_or(0.0, |&(_, m)| m);
                built[stats.team.index()] += (mass - before).max(0.0);
                now.insert(id.0, (stats.team.index(), mass));
            }
            for (id, &(team, mass)) in &alive {
                if !now.contains_key(id) {
                    lost[team] += mass;
                }
            }
            alive = now;
        }
        for (team, commander) in world.commanders.iter().enumerate().take(2) {
            if let Some(entity) = commander.and_then(|id| world.unit_entity(id)) {
                if let Ok(health) = world.ecs.get::<&Health>(entity) {
                    let share: f64 = (health.current / health.max).to_num();
                    lowest_commander[team] = lowest_commander[team].min(share);
                }
            }
        }
        // STALL_COMMANDER=from-to (minutes): Blue's commander's order and tasks, every 5 seconds.
        if let Some((from, to)) = std::env::var("STALL_COMMANDER").ok().and_then(|text| text.split_once('-').map(|(a, b)| (a.parse::<u32>().unwrap(), b.parse::<u32>().unwrap()))) {
            if tick >= from * TICKS_PER_MINUTE && tick <= to * TICKS_PER_MINUTE && tick % std::env::var("STALL_STEP").ok().and_then(|t| t.parse().ok()).unwrap_or(50) == 0 {
                if let Some(entity) = world.commanders[0].and_then(|id| world.unit_entity(id)) {
                    let order = world.ecs.get::<&Order>(entity).unwrap().kind;
                    let tasks: Vec<String> = world.ecs.get::<&Builder>(entity).unwrap().tasks.iter().map(|task| match task {
                        BuildTask::Build { blueprint, site, .. } => format!("build {} {site:?}", world.blueprints.get(*blueprint).key),
                        BuildTask::Assist { target } => format!("assist {target:?}"),
                        other => format!("{other:?}").chars().take(40).collect(),
                    }).collect();
                    let health = world.ecs.get::<&Health>(entity).unwrap();
                    let sites: Vec<String> = world.units.iter().filter_map(|&(id, e)| {
                        let stats = world.ecs.get::<&UnitStats>(e).ok()?;
                        let site = world.ecs.get::<&Construction>(e).ok()?;
                        let key = &world.blueprints.get(world.ecs.get::<&Blueprinted>(e).ok()?.0).key;
                        (stats.team.index() == 0).then(|| format!("{key}#{} {:.3}", id.0, site.progress.to_num::<f64>()))
                    }).collect();
                    println!("    {:>5.0}s commander {order:?} hp {:.0}% tasks {tasks:?} sites {sites:?}", f64::from(tick) / 10.0, (health.current / health.max).to_num::<f64>() * 100.0);
                }
            }
        }
        if tick % (TICKS_PER_MINUTE * every) != 0 && world.match_result.is_none() {
            return;
        }
        let starts = [(world.starts[0].x.to_num::<f64>(), world.starts[0].y.to_num::<f64>()), (world.starts[1].x.to_num::<f64>(), world.starts[1].y.to_num::<f64>())];
        let mut sides = [Side::default(), Side::default()];
        let mut commanders = [None, None];
        for &(id, entity) in &world.units {
            let (Ok(stats), Ok(unit)) = (world.ecs.get::<&UnitStats>(entity), world.ecs.get::<&Blueprinted>(entity)) else { continue };
            let team = stats.team.index();
            let blueprint = world.blueprints.get(unit.0);
            let side = &mut sides[team];
            *side.keys.entry(blueprint.key.clone()).or_default() += 1;
            if world.commanders.get(stats.owner.index()).copied().flatten() == Some(id) {
                commanders[team] = Some(entity);
            }
            if stats.is_structure {
                side.structures += 1;
                side.defenses += usize::from(blueprint.class == UnitClass::Defense);
                continue;
            }
            if blueprint.class == UnitClass::Engineer {
                side.engineers += 1;
            }
            let Some(d) = domain(blueprint.class, stats.move_class) else { continue };
            side.army[d].0 += 1;
            side.army[d].1 += blueprint.mass.to_num::<f64>();
            if let Ok(order) = world.ecs.get::<&Order>(entity) {
                side.orders[order.kind as usize] += 1;
            }
            let along = progress(starts[team], starts[1 - team], xy(world, entity));
            side.zones[if along < 1.0 / 3.0 { 0 } else if along < 2.0 / 3.0 { 1 } else { 2 }] += 1;
        }
        println!("t={:>3}m", tick / TICKS_PER_MINUTE);
        for (team, name) in [(0, "Blue"), (1, "Red ")] {
            let side = &sides[team];
            let (income, stored) = world
                .players
                .iter()
                .zip(&world.economies)
                .filter(|(player_team, _)| player_team.index() == team)
                .fold((0.0, 0.0), |(i, s), (_, economy)| (i + economy.mass_income.to_num::<f64>(), s + economy.mass.to_num::<f64>()));
            let commander = commanders[team].map_or("dead".to_string(), |entity| {
                let health = world.ecs.get::<&Health>(entity).unwrap();
                let at = xy(world, entity);
                let near = world
                    .units
                    .iter()
                    .filter(|&&(_, other)| {
                        world.ecs.get::<&UnitStats>(other).is_ok_and(|stats| stats.team.index() != team && !stats.is_structure) && {
                            let p = xy(world, other);
                            (p.0 - at.0).hypot(p.1 - at.1) < 1500.0
                        }
                    })
                    .count();
                format!(
                    "hp {:>3.0}% (low {:>3.0}%) at {:+.2} of the way, {near} enemies near",
                    (health.current / health.max).to_num::<f64>() * 100.0,
                    lowest_commander[team] * 100.0,
                    progress(starts[team], starts[1 - team], at)
                )
            });
            let army: Vec<String> = ["land", "hover", "naval", "air"].iter().zip(side.army).map(|(label, (count, mass))| format!("{label} {count}/{mass:.0}")).collect();
            println!(
                "  {name} mass +{income:.1}/s stored {stored:.0} | built {:.0} lost {:.0} | structures {} (defenses {}) engineers {} | {} | idle {} move {} amove {} attack {} | home/mid/far {}/{}/{} | commander {commander}",
                built[team],
                lost[team],
                side.structures,
                side.defenses,
                side.engineers,
                army.join(" "),
                side.orders[OrderKind::Idle as usize],
                side.orders[OrderKind::Move as usize],
                side.orders[OrderKind::AttackMove as usize],
                side.orders[OrderKind::Attack as usize],
                side.zones[0],
                side.zones[1],
                side.zones[2],
            );
        }
        if std::env::var_os("STALL_DETAIL").is_some() {
            for (team, name) in [(0, "Blue"), (1, "Red ")] {
                let economy = &world.economies[team];
                println!(
                    "  {name} energy +{:.0}/s demand {:.0}/s stored {:.0}/{:.0} | mass demand {:.1}/s | satisfaction {:.2} | extractor upkeep {:.0}/s",
                    economy.energy_income.to_num::<f64>() * 10.0,
                    economy.energy_demand.to_num::<f64>() * 10.0,
                    economy.energy.to_num::<f64>(),
                    economy.energy_capacity.to_num::<f64>(),
                    economy.mass_demand.to_num::<f64>() * 10.0,
                    economy.satisfaction.to_num::<f64>(),
                    economy.extractor_upkeep.to_num::<f64>() * 10.0,
                );
                for &(_, entity) in &world.units {
                    let (Ok(stats), Ok(unit), Ok(factory)) = (world.ecs.get::<&UnitStats>(entity), world.ecs.get::<&Blueprinted>(entity), world.ecs.get::<&Factory>(entity)) else { continue };
                    if stats.team.index() != team {
                        continue;
                    }
                    let queue: Vec<&str> = factory.queue.iter().map(|item| world.blueprints.get(*item).key.as_str()).collect();
                    let built = world.ecs.get::<&Construction>(entity).map_or("finished".to_string(), |site| format!("UNFINISHED {} ({:?})", site.progress.to_num::<f64>(), site.progress));
                    println!("  {name} {} {built} queue {queue:?} progress {:.2}", world.blueprints.get(unit.0).key, factory.progress.to_num::<f64>());
                }
                let keys: Vec<String> = sides[team].keys.iter().map(|(key, count)| format!("{key} {count}")).collect();
                println!("  {name} units: {}", keys.join(", "));
            }
        }
        built = [0.0; 2];
        lost = [0.0; 2];
    });
    println!("ended at tick {} ({} s): {:?}", record.ticks, record.ticks / 10, record.result);
}
