//! Phase 3 gate: `advance_tick` under 6.0 ms at the 99th percentile for 5,000 units
//! plus 2,000 projectiles, with the projectile step under 5.0 ms, on one core.
#![allow(clippy::disallowed_types)]

use std::time::{Duration, Instant};

use criterion::{criterion_group, Criterion};
use sim_engine::math::fp::{fp_int, Fp, Vec3Fp};
use sim_engine::sim::combat::firing_solution;
use sim_engine::sim::components::{Combat, Projectile, Team};
use sim_engine::sim::config::SimConfig;
use sim_engine::sim::scenario;
use sim_engine::sim::world::SimWorld;

const UNITS_PER_TEAM: usize = 2_500;
const PROJECTILES: usize = 2_000;
const WARMUP_TICKS: u32 = 150;

/// The full battle, with units that cannot die so that the workload stays constant.
fn benchmark_world() -> SimWorld {
    let config = SimConfig { invulnerable_units: true, ..SimConfig::default() };
    let scenario = scenario::battle(0xBE7C4, UNITS_PER_TEAM, config);
    let opening = scenario.opening_commands;
    let mut world = scenario.world;
    // Slow the natural firing rate, then top up to exactly 2,000 projectiles each tick.
    let entities: Vec<_> = world.units.iter().map(|unit| unit.1).collect();
    for entity in entities {
        world.ecs.get::<&mut Combat>(entity).unwrap().reload_ticks = 120;
    }
    world.advance_tick(&opening);
    for _ in 0..WARMUP_TICKS {
        top_up_projectiles(&mut world);
        world.advance_tick(&[]);
    }
    world
}

/// Fires extra shots from random units across the front line at a random spot
/// in the enemy block, so that they fly over and land among dense units.
fn top_up_projectiles(world: &mut SimWorld) {
    while world.projectiles.len() < PROJECTILES {
        let (id, _) = world.units[world.rng.below(world.units.len() as u32) as usize];
        let start = world.unit_position(id).unwrap();
        let spread = fp_int(30);
        let aim = Vec3Fp::new(
            start.x + world.rng.range_fp(-spread, spread),
            start.y + world.rng.range_fp(-spread, spread),
            start.z,
        );
        let solution = firing_solution(start, aim, fp_int(2));
        let team = if id.0 as usize % (UNITS_PER_TEAM * 2) < UNITS_PER_TEAM { Team::Blue } else { Team::Red };
        world.spawn_projectile(
            start,
            Projectile {
                velocity: solution.velocity,
                damage: Fp::ONE,
                source: id,
                source_team: team,
                lifetime_ticks: solution.flight_ticks + 10,
                splash_radius: Fp::ZERO,
                allied_safe_ticks: 0,
            },
        );
    }
}

fn percentile(sorted: &[Duration], percent: usize) -> Duration {
    sorted[(sorted.len() * percent / 100).min(sorted.len() - 1)]
}

fn report(name: &str, samples: &mut [Duration], limit_ms: Option<f64>) -> bool {
    samples.sort_unstable();
    let p99 = percentile(samples, 99);
    let passed = limit_ms.is_none_or(|limit| p99.as_secs_f64() * 1000.0 < limit);
    println!(
        "  {name:<18} p50 {:>6.3} ms   p99 {:>6.3} ms   max {:>6.3} ms{}",
        percentile(samples, 50).as_secs_f64() * 1000.0,
        p99.as_secs_f64() * 1000.0,
        samples[samples.len() - 1].as_secs_f64() * 1000.0,
        limit_ms.map_or(String::new(), |limit| format!("   limit {limit:.1} ms: {}", if passed { "PASS" } else { "FAIL" })),
    );
    passed
}

/// Times each step separately, which Criterion's mean cannot show.
fn percentile_gate() {
    const TICKS: usize = 2_000;
    let mut world = benchmark_world();
    let mut phases: [Vec<Duration>; 7] = std::array::from_fn(|_| Vec::with_capacity(TICKS));
    let mut unit_total = 0;
    let mut projectile_total = 0;
    for _ in 0..TICKS {
        top_up_projectiles(&mut world);
        unit_total += world.units.len();
        projectile_total += world.projectiles.len();
        let tick_start = Instant::now();
        let mut mark = tick_start;
        let mut lap = |phases: &mut [Vec<Duration>; 7], index: usize| {
            let now = Instant::now();
            phases[index].push(now - mark);
            mark = now;
        };
        world.step_commands(&[]);
        world.rebuild_grid();
        lap(&mut phases, 0);
        world.step_movement();
        lap(&mut phases, 1);
        world.rebuild_grid();
        lap(&mut phases, 2);
        world.step_combat();
        lap(&mut phases, 3);
        world.step_projectiles();
        lap(&mut phases, 4);
        world.step_production();
        world.step_cleanup();
        lap(&mut phases, 5);
        world.current_tick += 1;
        phases[6].push(tick_start.elapsed());
    }
    println!(
        "\nPhase 3 gate over {TICKS} ticks: average {} units, {} projectiles at tick start",
        unit_total / TICKS,
        projectile_total / TICKS
    );
    let names = ["grid (first)", "movement", "grid (second)", "combat"];
    for (index, name) in names.iter().enumerate() {
        report(name, &mut phases[index], None);
    }
    let projectile_passed = report("projectiles", &mut phases[4], Some(5.0));
    report("cleanup", &mut phases[5], None);
    let tick_passed = report("advance_tick", &mut phases[6], Some(6.0));
    println!("  Gate: {}", if tick_passed && projectile_passed { "PASS" } else { "FAIL" });
}

/// The worst single ticks in a large move order: route search and field builds land
/// together, which the steady battle workload above does not exercise.
fn navigation_report() {
    let (scenario, _) = scenario::navigation(7, UNITS_PER_TEAM * 2);
    let opening = scenario.opening_commands;
    let mut world = scenario.world;
    let mut samples = Vec::new();
    let mut commands = opening.as_slice();
    for _ in 0..1_400 {
        let started = Instant::now();
        world.advance_tick(commands);
        samples.push(started.elapsed());
        commands = &[];
    }
    println!("\nLarge move order, 5,000 units across 3 walls ({} fields built):", world.flow_field.field_builds_total());
    let first_tick = samples[0];
    let slowest = samples.iter().enumerate().max_by_key(|(_, time)| **time).map(|(tick, time)| (tick, *time)).unwrap();
    println!(
        "  first tick {:.2} ms; slowest tick was {} at {:.2} ms",
        first_tick.as_secs_f64() * 1000.0,
        slowest.0,
        slowest.1.as_secs_f64() * 1000.0
    );
    report("advance_tick", &mut samples, Some(6.0));
    let graph_started = Instant::now();
    let mut fresh = scenario::navigation(7, 0).0.world;
    let map_built = graph_started.elapsed();
    let route_started = Instant::now();
    let route = fresh.flow_field.create_route(
        &fresh.terrain,
        0,
        sim_engine::math::fp::Vec2Fp::new(fp_int(50_000), fp_int(40_000)),
        1,
        Fp::ONE,
        None,
    );
    println!(
        "  map generation {:.1} ms; portal graph ({} portals) plus first route {:.1} ms",
        map_built.as_secs_f64() * 1000.0,
        fresh.flow_field.portal_count(),
        route_started.elapsed().as_secs_f64() * 1000.0
    );
    assert!(route.is_some());
    let route_started = Instant::now();
    let _ = fresh.flow_field.create_route(
        &fresh.terrain,
        0,
        sim_engine::math::fp::Vec2Fp::new(fp_int(30_000), fp_int(30_000)),
        1,
        Fp::ONE,
        None,
    );
    println!("  later route search {:.2} ms", route_started.elapsed().as_secs_f64() * 1000.0);
}

fn criterion_benchmarks(c: &mut Criterion) {
    let mut world = benchmark_world();
    let mut group = c.benchmark_group("sim");
    group.sample_size(50);
    group.bench_function("advance_tick_5000_units_2000_projectiles", |b| {
        b.iter(|| {
            top_up_projectiles(&mut world);
            world.advance_tick(&[]);
        });
    });
    group.finish();
}

criterion_group!(benches, criterion_benchmarks);

fn main() {
    benches();
    Criterion::default().configure_from_args().final_summary();
    percentile_gate();
    navigation_report();
}
