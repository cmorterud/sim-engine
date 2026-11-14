#![allow(clippy::disallowed_types, clippy::float_arithmetic)]
//! Follows a unit and its mirror image through the ticks before the two sides drift apart, to
//! find out why: position, velocity, cell, order, the flow field's direction for it, its target,
//! and what is near, all with Red's turned about the center to compare with Blue's. Take the
//! tick, blueprint, and position from `mirror_probe`.
//!
//! `cargo run --release --example mirror_trace -- <seed> <tick> <blueprint> <x> <y>`
use sim_engine::ai::AiBrain;
use sim_engine::sim::components::{Blueprinted, Combat, Order, Position, Team, UnitStats, Velocity};
use sim_engine::sim::scenario::{skirmish, SKIRMISH_MAP_UNITS};
use sim_engine::sim::world::SimWorld;

const MAP: f64 = SKIRMISH_MAP_UNITS as f64;

fn flip(team: Team, x: f64, y: f64) -> (f64, f64) {
    if team == Team::Red { (MAP - x, MAP - y) } else { (x, y) }
}

fn find(world: &SimWorld, team: Team, blueprint: u16, x: f64, y: f64) -> Option<hecs::Entity> {
    let mut best = None;
    for &(_, e) in &world.units {
        let (Ok(s), Ok(b)) = (world.ecs.get::<&UnitStats>(e), world.ecs.get::<&Blueprinted>(e)) else { continue };
        if s.team != team || b.0 .0 != blueprint {
            continue;
        }
        let p = world.ecs.get::<&Position>(e).unwrap().0;
        let (px, py) = flip(team, p.x.to_num(), p.y.to_num());
        let d = (px - x).hypot(py - y);
        if d < 40.0 && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, e));
        }
    }
    best.map(|(_, e)| e)
}

fn show(world: &mut SimWorld, team: Team, e: hecs::Entity) {
    let p = world.ecs.get::<&Position>(e).unwrap().0;
    let v = world.ecs.get::<&Velocity>(e).unwrap().0;
    let o = *world.ecs.get::<&Order>(e).unwrap();
    let c = *world.ecs.get::<&Combat>(e).unwrap();
    let s = if team == Team::Red { -1.0 } else { 1.0 };
    let (px, py) = flip(team, p.x.to_num(), p.y.to_num());
    let cell = world.terrain.cell_of(p.xy());
    let tick = world.current_tick;
    let sample = if o.kind == sim_engine::sim::components::OrderKind::Idle {
        "idle".to_string()
    } else {
        let sample = world.flow_field.sample(o.route, tick, p.xy(), &world.terrain);
        format!("{sample:?}").replace("Vec2Fp ", "")
    };
    let target = |id: Option<sim_engine::sim::components::SimId>| {
        id.and_then(|id| world.unit_position(id)).map(|q| {
            let (qx, qy) = flip(team, q.x.to_num(), q.y.to_num());
            format!("({qx:.1},{qy:.1})")
        })
    };
    let mut near = Vec::new();
    for &(_, other) in &world.units {
        if other == e {
            continue;
        }
        let q = world.ecs.get::<&Position>(other).unwrap().0;
        let (qx, qy) = flip(team, q.x.to_num(), q.y.to_num());
        if (qx - px).hypot(qy - py) < 60.0 {
            let st = world.ecs.get::<&UnitStats>(other).unwrap();
            let mine = if st.team == team { "own" } else { "enemy" };
            near.push(format!("{mine}{} ({qx:.2},{qy:.2})", if st.is_structure { "S" } else { "" }));
        }
    }
    println!(
        "  {team:?} pos ({px:.3},{py:.3}) vel ({:.3},{:.3}) cell {cell:?} {:?} sample {sample} target {:?} forced {:?} near {near:?}",
        s * v.x.to_num::<f64>(),
        s * v.y.to_num::<f64>(),
        o.kind,
        target(c.target),
        target(c.forced_target)
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let seed: u64 = args[0].parse().unwrap();
    let at: u32 = args[1].parse().unwrap();
    let blueprint: u16 = args[2].parse().unwrap();
    let (x, y): (f64, f64) = (args[3].parse().unwrap(), args[4].parse().unwrap());
    let mut world = skirmish(seed).world;
    let mut brains = [AiBrain::new(Team::Blue, seed, world.starts), AiBrain::new(Team::Red, seed, world.starts)];
    while world.current_tick < at {
        let mut commands = brains[0].think(&world);
        commands.extend(brains[1].think(&world));
        world.advance_tick(&commands);
        if world.current_tick + 4 >= at {
            println!("tick {}", world.current_tick);
            for team in [Team::Blue, Team::Red] {
                if let Some(e) = find(&world, team, blueprint, x, y) {
                    show(&mut world, team, e);
                }
            }
        }
    }
}
