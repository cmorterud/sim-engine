//! The exit criteria's stress test: many AI-vs-AI matches, each replayed from its command log.
#![allow(clippy::disallowed_types)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::ai::{play_match_on, replay_match_on, CHECKSUM_INTERVAL};
use crate::sim::scenario::MapSource;
use crate::sim::components::Team;
use crate::sim::world::MatchResult;

/// Matches running past this many ticks (about 67 minutes) are counted as unfinished.
pub const MATCH_TICK_LIMIT: u32 = 40_000;

#[derive(Debug)]
pub struct MatchSummary {
    pub seed: u64,
    pub ticks: u32,
    pub result: Option<MatchResult>,
    pub replay_matches: bool,
    pub peak_units: usize,
}

pub struct SoakReport {
    pub matches: Vec<MatchSummary>,
    pub tick_times: Vec<Duration>,
}

/// Plays and replays one match per seed, on several threads. A panic in any match is a failure.
pub fn run_soak(first_seed: u64, count: u64, threads: usize, map: &MapSource) -> SoakReport {
    let next = Arc::new(AtomicU64::new(0));
    let summaries = Arc::new(Mutex::new(Vec::new()));
    let times = Arc::new(Mutex::new(Vec::new()));
    let workers: Vec<_> = (0..threads.max(1))
        .map(|_| {
            let (next, summaries, times, map) = (Arc::clone(&next), Arc::clone(&summaries), Arc::clone(&times), map.clone());
            thread::spawn(move || loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                if index >= count {
                    return;
                }
                let seed = first_seed + index;
                let mut local_times = Vec::with_capacity(20_000);
                let mut last = Instant::now();
                let record = play_match_on(&map, seed, MATCH_TICK_LIMIT, |world| {
                    let now = Instant::now();
                    // Ticks that also compute a checksum would inflate the 99th percentile.
                    if !world.current_tick.is_multiple_of(CHECKSUM_INTERVAL) {
                        local_times.push(now - last);
                    }
                    last = Instant::now();
                });
                let replay_matches = replay_match_on(&map, seed, &record.log) == record.checksums;
                summaries.lock().expect("no worker panicked").push(MatchSummary {
                    seed,
                    ticks: record.ticks,
                    result: record.result,
                    replay_matches,
                    peak_units: record.units_built_peak,
                });
                times.lock().expect("no worker panicked").extend(local_times);
            })
        })
        .collect();
    for worker in workers {
        worker.join().expect("a match panicked");
    }
    let mut matches = std::mem::take(&mut *summaries.lock().expect("no worker panicked"));
    matches.sort_by_key(|summary| summary.seed);
    let tick_times = std::mem::take(&mut *times.lock().expect("no worker panicked"));
    SoakReport { matches, tick_times }
}

impl SoakReport {
    /// Prints the results and returns whether every exit criterion held.
    pub fn print(&mut self) -> bool {
        let count = self.matches.len();
        let unfinished = self.matches.iter().filter(|summary| summary.result.is_none()).count();
        let mismatched = self.matches.iter().filter(|summary| !summary.replay_matches).count();
        let wins = |team: Team| self.matches.iter().filter(|summary| summary.result == Some(MatchResult::Winner(team))).count();
        let draws = self.matches.iter().filter(|summary| summary.result == Some(MatchResult::Draw)).count();
        let mut lengths: Vec<u32> = self.matches.iter().map(|summary| summary.ticks).collect();
        lengths.sort_unstable();
        let peak = self.matches.iter().map(|summary| summary.peak_units).max().unwrap_or(0);
        println!("soak: {count} matches");
        println!("  Blue wins {}, Red wins {}, draws {draws}, unfinished {unfinished}", wins(Team::Blue), wins(Team::Red));
        println!("  replay mismatches {mismatched}");
        let unfinished_seeds: Vec<u64> = self.matches.iter().filter(|summary| summary.result.is_none()).map(|summary| summary.seed).collect();
        if !unfinished_seeds.is_empty() {
            println!("  unfinished seeds: {unfinished_seeds:?}");
        }
        if let (Some(first), Some(last)) = (lengths.first(), lengths.last()) {
            println!(
                "  match length: shortest {} s, median {} s, longest {} s",
                first / 10,
                lengths[lengths.len() / 2] / 10,
                last / 10
            );
        }
        println!("  peak units on the map {peak}");
        self.tick_times.sort_unstable();
        let mut tick_gate = true;
        if !self.tick_times.is_empty() {
            let at = |percent: usize| self.tick_times[(self.tick_times.len() * percent / 100).min(self.tick_times.len() - 1)];
            let p99 = at(99);
            tick_gate = p99.as_secs_f64() * 1000.0 < 6.0;
            println!(
                "  tick time (with AI): p50 {:.3} ms   p99 {:.3} ms   max {:.3} ms   p99 limit 6.0 ms: {}",
                at(50).as_secs_f64() * 1000.0,
                p99.as_secs_f64() * 1000.0,
                self.tick_times[self.tick_times.len() - 1].as_secs_f64() * 1000.0,
                if tick_gate { "PASS" } else { "FAIL" }
            );
        }
        let passed = unfinished == 0 && mismatched == 0 && tick_gate;
        println!("  result: {}", if passed { "PASS" } else { "FAIL" });
        passed
    }
}
