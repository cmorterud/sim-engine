//! Phase 4 gate: the threaded driver behind the window produces the same state as a
//! direct headless run, whether paced in real time or not.

use sim_engine::driver::{CommandSource, SimDriver};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::scenario::{self, advance_scripted, Scenario};

const SEED: u64 = 99;
const TICKS: u32 = 30;

fn make_scenario() -> Scenario {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Hills, ..SimConfig::default() };
    scenario::battle(SEED, 200, config)
}

fn headless_checksum() -> u64 {
    let scenario = make_scenario();
    let opening = scenario.opening_commands;
    let mut world = scenario.world;
    for _ in 0..TICKS {
        advance_scripted(&mut world, &opening, SEED);
    }
    world.compute_checksum()
}

fn driver_checksum(paced: bool) -> (u32, u64, usize) {
    let driver = SimDriver::start(make_scenario(), CommandSource::Script { seed: SEED, ticks: TICKS }, paced);
    while !driver.is_finished() {
        std::thread::yield_now();
    }
    let mut snapshots = 0;
    while driver.try_snapshot().is_some() {
        snapshots += 1;
    }
    let result = driver.finish();
    (result.tick, result.checksum, snapshots)
}

#[test]
fn unpaced_driver_matches_headless() {
    let (tick, checksum, snapshots) = driver_checksum(false);
    assert_eq!(tick, TICKS);
    assert_eq!(checksum, headless_checksum());
    // One initial snapshot, plus one per tick.
    assert_eq!(snapshots, TICKS as usize + 1);
}

#[test]
fn real_time_paced_driver_matches_headless() {
    let (tick, checksum, _) = driver_checksum(true);
    assert_eq!(tick, TICKS);
    assert_eq!(checksum, headless_checksum());
}
