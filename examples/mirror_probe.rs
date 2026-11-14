//! Finds where an AI-vs-AI skirmish stops being a mirror image. Each tick, Red is reflected
//! through the map center and compared with Blue: first the orders each AI gives, then the
//! units themselves.
//!
//! Positions are matched within a tolerance, because fixed-point rounding is not symmetric
//! under negation and leaves the two sides a raw bit or two apart.
//!
//! `cargo run --release --example mirror_probe -- <seed> <ticks>`
#![allow(clippy::disallowed_types, clippy::float_arithmetic)]

use sim_engine::ai::AiBrain;
use sim_engine::math::fp::Vec2Fp;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Blueprinted, Construction, Health, Position, Team, UnitStats};
use sim_engine::sim::config::TERRAIN_CELL_UNITS;
use sim_engine::sim::scenario::{skirmish, SKIRMISH_MAP_UNITS};
use sim_engine::sim::world::SimWorld;

/// Blueprint, position in units, build progress and health share in thousandths.
type Unit = (u16, f64, f64, i64, i64);

fn units(world: &SimWorld, team: Team) -> Vec<Unit> {
    let map = f64::from(SKIRMISH_MAP_UNITS);
    world
        .units
        .iter()
        .filter_map(|&(_, entity)| {
            let stats = world.ecs.get::<&UnitStats>(entity).ok()?;
            if stats.team != team {
                return None;
            }
            let blueprint = world.ecs.get::<&Blueprinted>(entity).ok()?.0 .0;
            let position = world.ecs.get::<&Position>(entity).ok()?.0;
            let (mut x, mut y) = (position.x.to_num::<f64>(), position.y.to_num::<f64>());
            if team == Team::Red {
                (x, y) = (map - x, map - y);
            }
            let progress = world.ecs.get::<&Construction>(entity).map(|c| (c.progress * 1000).to_num::<i64>()).unwrap_or(1000);
            let health = world.ecs.get::<&Health>(entity).map(|h| i64::from(h.current.to_bits()) * 1000 / i64::from(h.max.to_bits())).unwrap_or(0);
            Some((blueprint, x, y, progress, health))
        })
        .collect()
}

fn point(team: Team, point: Vec2Fp) -> String {
    let (mut x, mut y) = (point.x.to_num::<i32>(), point.y.to_num::<i32>());
    if team == Team::Red {
        (x, y) = (SKIRMISH_MAP_UNITS - x, SKIRMISH_MAP_UNITS - y);
    }
    format!("({x},{y})")
}

/// An order with its ids left out and its places reflected, so the two teams' can be compared.
fn describe(world: &SimWorld, team: Team, command: &SimCommand) -> String {
    let last = SKIRMISH_MAP_UNITS / TERRAIN_CELL_UNITS - 1;
    match command {
        SimCommand::Build { units, blueprint, cell_x, cell_y, .. } => {
            let size = i32::from(world.blueprints.get(*blueprint).footprint);
            let (mut x, mut y) = (*cell_x as i32, *cell_y as i32);
            if team == Team::Red {
                (x, y) = (last - x - size + 1, last - y - size + 1);
            }
            format!("build {} x{} at ({x},{y})", world.blueprints.get(*blueprint).key, units.len())
        }
        SimCommand::AttackMove { units, goal } => format!("attack-move x{} to {}", units.len(), point(team, *goal)),
        SimCommand::Move { units, goal } => format!("move x{} to {}", units.len(), point(team, *goal)),
        SimCommand::SetRally { point: rally, .. } => format!("rally {}", point(team, *rally)),
        SimCommand::QueueUnit { blueprint, count, .. } => format!("queue {} x{count}", world.blueprints.get(*blueprint).key),
        SimCommand::Assist { units, .. } => format!("assist x{}", units.len()),
        SimCommand::Reclaim { units, .. } => format!("reclaim x{}", units.len()),
        SimCommand::Upgrade { units } => format!("upgrade x{}", units.len()),
        other => format!("{other:?}"),
    }
}

/// Units with no partner on the other side within the tolerance.
fn unmatched(blue: &[Unit], red: &[Unit], tolerance: f64) -> (Vec<Unit>, Vec<Unit>) {
    let mut taken = vec![false; red.len()];
    let mut only_blue = Vec::new();
    for unit in blue {
        let partner = red.iter().enumerate().position(|(index, other)| {
            !taken[index]
                && other.0 == unit.0
                && (other.1 - unit.1).abs() <= tolerance
                && (other.2 - unit.2).abs() <= tolerance
                && (other.3 - unit.3).abs() <= 20
                && (other.4 - unit.4).abs() <= 20
        });
        match partner {
            Some(index) => taken[index] = true,
            None => only_blue.push(*unit),
        }
    }
    let only_red = red.iter().zip(&taken).filter(|(_, taken)| !**taken).map(|(unit, _)| *unit).collect();
    (only_blue, only_red)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let seed: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let ticks: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(6000);
    let tolerance: f64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(4.0);
    let mut world = skirmish(seed).world;
    let mut brains = [AiBrain::new(Team::Blue, seed, world.starts), AiBrain::new(Team::Red, seed, world.starts)];
    let (mut order_reports, mut unit_reports) = (0, 0);
    while world.current_tick < ticks && world.match_result.is_none() {
        let blue = brains[0].think(&world);
        let red = brains[1].think(&world);
        let red_described: Vec<String> = red.iter().map(|c| describe(&world, Team::Red, c)).collect();
        let blue_orders: Vec<String> = blue.iter().map(|c| describe(&world, Team::Blue, c)).collect();
        let mut commands = blue;
        commands.extend(red);
        world.advance_tick(&commands);
        let blue_units = units(&world, Team::Blue);
        if blue_orders != red_described && order_reports < 6 {
            order_reports += 1;
            println!("tick {}: orders differ\n  blue {blue_orders:?}\n  red  {red_described:?}", world.current_tick - 1);
        }
        let (only_blue, only_red) = unmatched(&blue_units, &units(&world, Team::Red), tolerance);
        if (!only_blue.is_empty() || !only_red.is_empty()) && unit_reports < 3 {
            unit_reports += 1;
            println!("tick {}: units differ (blue {}, red {})", world.current_tick, blue_units.len(), blue_units.len() - only_blue.len() + only_red.len());
            for unit in only_blue.iter().take(5) {
                println!("  blue only {unit:?}");
            }
            for unit in only_red.iter().take(5) {
                println!("  red  only {unit:?} (reflected)");
            }
        }
        if order_reports >= 6 && unit_reports >= 3 {
            break;
        }
    }
    println!("ended at tick {}", world.current_tick);
}
