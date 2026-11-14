//! Phase 4 gate: the threaded driver behind the window produces the same state as a
//! direct headless run, whether paced in real time or not.
// Timeouts, so that a broken driver fails the test instead of hanging it.
#![allow(clippy::disallowed_types)]

use sim_engine::driver::{CommandSource, DriverOptions, SimDriver};
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

#[test]
fn a_paused_driver_runs_no_ticks_and_resumes_where_it_stopped() {
    let options = DriverOptions { start_paused: true, ..DriverOptions::default() };
    let driver = SimDriver::start_with(make_scenario(), CommandSource::Script { seed: SEED, ticks: TICKS }, false, options);
    assert!(driver.is_paused());
    std::thread::sleep(std::time::Duration::from_millis(200));
    let mut last = None;
    while let Some(snapshot) = driver.try_snapshot() {
        last = Some(snapshot.tick);
    }
    // Only the snapshot from before the first tick: unpaced, it would have finished by now.
    assert_eq!(last, Some(0));
    assert!(!driver.is_finished());
    driver.set_paused(false);
    while !driver.is_finished() {
        std::thread::yield_now();
    }
    let result = driver.finish();
    assert_eq!(result.tick, TICKS);
    assert_eq!(result.checksum, headless_checksum(), "pausing changes nothing in the simulation");
}

/// Watching a match plays the soak's match for the seed: the same state, tick for tick.
#[test]
fn watching_an_ai_match_plays_the_soaks_match_for_the_seed() {
    use sim_engine::ai::play_match_on;
    use sim_engine::sim::scenario::MapSource;
    const WATCHED: u32 = 300;
    let map = MapSource::Skirmish;
    let soak = play_match_on(&map, 7, WATCHED, |_| {});
    let driver = SimDriver::start(map.scenario(7), CommandSource::AiVsAi { seed: 7, ticks: WATCHED }, false);
    while !driver.is_finished() {
        std::thread::yield_now();
    }
    let result = driver.finish();
    assert_eq!(result.tick, WATCHED);
    assert_eq!(Some(&result.checksum), soak.checksums.last());
}

/// A watcher can switch between seeing everything and one side's sight, even while paused, and
/// switching changes only what the snapshots show.
#[test]
fn a_watcher_switches_sides_while_paused_without_changing_the_match() {
    use sim_engine::sim::components::Team;
    use sim_engine::sim::scenario::MapSource;
    use std::time::{Duration, Instant};
    let map = MapSource::Skirmish;
    let options = DriverOptions { start_paused: true, linger: true, ..DriverOptions::default() };
    let driver = SimDriver::start_with(map.scenario(7), CommandSource::AiVsAi { seed: 7, ticks: 50 }, false, options);
    let next_snapshot = || {
        let started = Instant::now();
        loop {
            if let Some(snapshot) = driver.try_snapshot() {
                return snapshot;
            }
            assert!(started.elapsed() < Duration::from_secs(10), "no snapshot came");
            std::thread::yield_now();
        }
    };
    let everything = next_snapshot();
    assert!(everything.fog.is_none() && everything.team_economies.is_some(), "seeing everything: no fog, and both economies");
    driver.set_viewer(Some(Team::Red));
    let red = next_snapshot();
    assert_eq!(red.tick, 0, "still paused");
    assert!(red.fog.is_some() && red.team_economies.is_none(), "Red's sight: Red's fog, and Red's economy alone");
    assert!(red.units.len() < everything.units.len(), "Blue's base is out of Red's sight");
    driver.set_paused(false);
    let started = Instant::now();
    while driver.try_snapshot().is_none_or(|snapshot| snapshot.tick < 50) {
        assert!(started.elapsed() < Duration::from_secs(30), "the match did not reach tick 50");
    }
    // Lingering at the end, it still switches.
    driver.set_viewer(None);
    assert!(next_snapshot().fog.is_none());
    assert!(!driver.is_finished(), "a lingering driver stays up for the watcher");
    let soak = sim_engine::ai::play_match_on(&map, 7, 50, |_| {});
    assert_eq!(Some(&driver.finish().checksum), soak.checksums.last(), "the view changes nothing in the match");
}
