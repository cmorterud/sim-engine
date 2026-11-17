//! Movement against the unit data: every unit moves at its stated speed and turns at its stated
//! rate, and walks straight across open ground rather than along the eight directions of its
//! route and through the middles of its sectors' edges (GAME_SPECIFICATION.md §13).

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::{Blueprint, Res};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{MoveClass, OrderKind, SimId, Team, Turning};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::terrain::{Terrain, HEIGHT_FRAC_BITS};
use sim_engine::sim::world::SimWorld;

const MAP: i32 = 10_240;

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

/// Open ground, or with `sea`, open water 40 deep everywhere but a strip of land along the edges.
fn world(sea: bool) -> SimWorld {
    let config = SimConfig { map_units: MAP, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = if sea {
        let cells = (MAP / 20) as usize;
        let samples = cells + 1;
        let height = |x: usize, y: usize| {
            let (wx, wy) = (x as i32 * 20, y as i32 * 20);
            if (400..=MAP - 400).contains(&wx) && (400..=MAP - 400).contains(&wy) { -40 } else { 20 }
        };
        let heights: Vec<i16> = (0..samples * samples).map(|index| (height(index % samples, index / samples) << HEIGHT_FRAC_BITS) as i16).collect();
        let blocked: Vec<bool> = (0..cells * cells)
            .map(|cell| {
                let (x, y) = (cell % cells, cell / cells);
                [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)].iter().any(|&(cx, cy)| height(cx, cy) < 0)
            })
            .collect();
        let terrain = Terrain::from_heights(MAP, heights, &blocked, Fp::ZERO);
        SimWorld::with_terrain(config, 1, terrain)
    } else {
        SimWorld::new(config, 1)
    };
    world.prepare_map();
    for economy in &mut world.economies {
        economy.energy = Res::from_num(1_000_000);
    }
    world
}

fn sails(def: &Blueprint) -> bool {
    matches!(def.movement, MoveClass::Naval | MoveClass::Submarine)
}

/// The units that move, other than aircraft.
fn movers() -> Vec<Blueprint> {
    world(false).blueprints.iter().filter(|def| !def.is_structure() && !def.is_air() && def.speed > Fp::ZERO).cloned().collect()
}

fn facing(world: &SimWorld, id: SimId) -> Vec2Fp {
    world.ecs.get::<&Turning>(world.unit_entity(id).unwrap()).unwrap().facing
}

/// Every unit that moves, aircraft included, goes at its stated top speed once under way, over
/// open ground or water, in a straight line.
#[test]
fn every_unit_moves_at_its_stated_speed() {
    let mut failures = Vec::new();
    let db = world(false).blueprints;
    let units: Vec<Blueprint> = db.iter().filter(|def| !def.is_structure() && def.speed > Fp::ZERO).cloned().collect();
    assert!(units.len() >= 40, "only {} units move", units.len());
    for def in units {
        let mut world = world(sails(&def));
        let id = world.spawn_mobile(world.blueprints.id(&def.key), Team::Blue, point(MAP / 2, 1_000));
        // Already facing the way it goes, so that only its speed is measured.
        world.ecs.get::<&mut Turning>(world.unit_entity(id).unwrap()).unwrap().facing = Vec2Fp::new(Fp::ZERO, Fp::ONE);
        world.advance_tick(&[SimCommand::Move { units: vec![id], goal: point(MAP / 2, MAP - 1_000) }]);
        for _ in 0..60 {
            world.advance_tick(&[]);
        }
        let start = world.unit_position(id).unwrap().xy();
        // Short enough that a fast aircraft is still on its way.
        let ticks = 100.min((7_000.0 / def.speed.to_num::<f64>()) as u32 - 60);
        for _ in 0..ticks {
            world.advance_tick(&[]);
        }
        let travelled = (world.unit_position(id).unwrap().xy() - start).length_squared().to_num::<f64>().sqrt();
        let speed = travelled / f64::from(ticks);
        let stated = def.speed.to_num::<f64>();
        if (speed - stated).abs() > stated * 0.02 {
            failures.push(format!("{}: {speed:.2} a tick, stated {stated:.2}", def.key));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Every unit with a turn rate makes a quarter turn in the time its rate says, give or take a tick.
#[test]
fn every_unit_turns_at_its_stated_rate() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for def in movers() {
        let Some(rate) = def.turn_rate else { continue };
        checked += 1;
        let mut world = world(sails(&def));
        let id = world.spawn_mobile(world.blueprints.id(&def.key), Team::Blue, point(MAP / 2, MAP / 2));
        world.ecs.get::<&mut Turning>(world.unit_entity(id).unwrap()).unwrap().facing = Vec2Fp::new(Fp::ONE, Fp::ZERO);
        world.advance_tick(&[SimCommand::Move { units: vec![id], goal: point(MAP / 2, MAP - 600) }]);
        // Degrees a second, ten ticks to a second. One with a turning circle turns while it drives,
        // at least at half speed, and no tighter than that circle: no faster than its rate, and no
        // slower than the circle allows at half speed.
        let per_tick = (f64::from(rate) / 10.0).to_radians();
        let quarter = std::f64::consts::FRAC_PI_2;
        let fastest = quarter / per_tick;
        let slowest = if def.turn_radius > 0 { quarter / per_tick.min(def.speed.to_num::<f64>() * 0.5 / f64::from(def.turn_radius)) } else { fastest };
        let turned = (1..600).find(|_| {
            world.advance_tick(&[]);
            facing(&world, id).y > Fp::from_num(0.999)
        });
        match turned {
            None => failures.push(format!("{}: never turned north", def.key)),
            // The tick it was ordered on counts too.
            Some(ticks) if f64::from(ticks + 1) < fastest * 0.9 - 1.5 || f64::from(ticks + 1) > slowest * 1.1 + 1.5 => {
                failures.push(format!("{}: a quarter turn in {} ticks, expected {fastest:.1} to {slowest:.1}", def.key, ticks + 1));
            }
            _ => {}
        }
    }
    assert!(checked >= 25, "only {checked} units were checked");
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A unit sent across open ground walks the straight line there, at any angle: it used to follow
/// its route's eight directions and the middles of its sectors' edges, so that a move at 27
/// degrees strayed 1,400 off the line and went 8% further.
#[test]
fn a_unit_walks_straight_across_open_ground_at_any_angle() {
    for (from, to) in [((5_650, 500), (5_650, 9_500)), ((5_650, 500), (9_650, 8_500)), ((1_000, 1_000), (9_000, 9_000)), ((2_000, 5_000), (9_000, 5_300)), ((9_000, 9_000), (1_300, 2_100))] {
        let mut world = world(false);
        let tank = world.spawn_mobile(world.blueprints.id("t1_medium_tank"), Team::Blue, point(from.0, from.1));
        // Facing the way it goes: a tank turning round first swings out on its turning circle.
        world.ecs.get::<&mut Turning>(world.unit_entity(tank).unwrap()).unwrap().facing = (point(to.0, to.1) - point(from.0, from.1)).normalized();
        world.advance_tick(&[SimCommand::Move { units: vec![tank], goal: point(to.0, to.1) }]);
        let (dx, dy) = (f64::from(to.0 - from.0), f64::from(to.1 - from.1));
        let length = dx.hypot(dy);
        let (mut walked, mut widest) = (0.0, 0.0f64);
        let mut last = (f64::from(from.0), f64::from(from.1));
        for _ in 0..5_000 {
            if world.unit_order(tank).unwrap().kind == OrderKind::Idle {
                break;
            }
            world.advance_tick(&[]);
            let at = world.unit_position(tank).unwrap();
            let here = (at.x.to_num::<f64>(), at.y.to_num::<f64>());
            walked += (here.0 - last.0).hypot(here.1 - last.1);
            last = here;
            widest = widest.max(((here.0 - f64::from(from.0)) * dy - (here.1 - f64::from(from.1)) * dx).abs() / length);
        }
        assert_eq!(world.unit_order(tank).unwrap().kind, OrderKind::Idle, "{from:?} to {to:?}: it should have arrived");
        assert!(widest <= 30.0, "{from:?} to {to:?}: {widest:.0} off the straight line");
        assert!(walked <= length * 1.01, "{from:?} to {to:?}: walked {walked:.0} for {length:.0}");
    }
}

/// Walking straight never takes a unit through a wall: one with a wall across its way goes round.
#[test]
fn a_unit_with_a_wall_in_its_way_goes_round_it() {
    let mut world = world(false);
    world.add_box_obstacle(sim_engine::math::rect::RectFp::new(point(2_000, 4_000), point(8_000, 4_100)));
    world.prepare_map();
    let tank = world.spawn_mobile(world.blueprints.id("t1_medium_tank"), Team::Blue, point(5_000, 1_000));
    world.advance_tick(&[SimCommand::Move { units: vec![tank], goal: point(5_000, 8_000) }]);
    for _ in 0..4_000 {
        world.advance_tick(&[]);
        let at = world.unit_position(tank).unwrap();
        let inside = (2_000..8_000).contains(&at.x.to_num::<i32>()) && (4_000..4_100).contains(&at.y.to_num::<i32>());
        assert!(!inside, "it walked into the wall at {at:?}");
        if world.unit_order(tank).unwrap().kind == OrderKind::Idle {
            break;
        }
    }
    let at = world.unit_position(tank).unwrap().xy();
    assert!((at - point(5_000, 8_000)).length_squared() < fp_int(40) * fp_int(40), "it should have got round, not stopped at {at:?}");
}
