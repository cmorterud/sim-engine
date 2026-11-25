//! The exit criteria's stress test: many AI-vs-AI matches, each replayed from its command log.
#![allow(clippy::disallowed_types)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::ai::{play_match_with, replay_match_on, CHECKSUM_INTERVAL};
use crate::ai_strategy::Strategy;
use crate::sim::scenario::MapSource;
use crate::sim::components::Team;
use crate::sim::world::MatchResult;

/// Matches running past this many ticks (about 67 minutes) are counted as unfinished.
pub const MATCH_TICK_LIMIT: u32 = 40_000;
/// Team matches of three or more a side, and any match on a map 20 km across or more, past this
/// many ticks (100 minutes) instead, the user's decisions (2026-11-25): Scattered Isles 4v4s ran to
/// 65 minutes, and one to 73 once defenses were made worth more for their cost; and since matches
/// on mirrored maps play as mirror images, a Scattered Isles 1v1 ran to 74, both finishing, where
/// the limit is for stalemates.
pub const TEAM_MATCH_TICK_LIMIT: u32 = 60_000;
/// Maps this wide or wider, in units, get the longer limit: 20 km.
pub const BIG_MAP_UNITS: i32 = 20 * 1_024;

/// The ticks past which a match of `per_team` players a side, on a map `map_units` across, counts as
/// unfinished.
pub const fn match_tick_limit(per_team: usize, map_units: i32) -> u32 {
    if per_team >= 3 || map_units >= BIG_MAP_UNITS { TEAM_MATCH_TICK_LIMIT } else { MATCH_TICK_LIMIT }
}

#[derive(Debug)]
pub struct MatchSummary {
    pub seed: u64,
    pub ticks: u32,
    pub result: Option<MatchResult>,
    pub replay_matches: bool,
    pub peak_units: usize,
    /// The mass each team put into each domain, and lost from it (`Domain`), Blue's then Red's.
    pub spent: [[u64; DOMAINS]; 2],
    pub lost: [[u64; DOMAINS]; 2],
}

/// What a unit is for, for the soak's account of where each side's mass went (2026-11-24, for
/// balance between domains): the base (structures, engineers, commanders, field rigs), and the
/// four domains an army is made of.
pub const DOMAINS: usize = 5;
pub const DOMAIN_NAMES: [&str; DOMAINS] = ["base", "land", "hover", "naval", "air"];

fn domain_of(unit: &crate::sim::blueprints::Blueprint) -> usize {
    use crate::sim::blueprints::UnitClass;
    use crate::sim::components::MoveClass;
    if unit.is_structure() || matches!(unit.class, UnitClass::Engineer | UnitClass::Commander | UnitClass::FieldRig) {
        return 0;
    }
    match unit.move_class() {
        MoveClass::Land | MoveClass::Seabed => 1,
        MoveClass::Hover => 2,
        MoveClass::Naval | MoveClass::Submarine => 3,
        MoveClass::Air => 4,
    }
}

/// Every tenth tick, the units on the map: those new since the last look are mass spent, and
/// those gone are mass lost, by team and domain. Units come and go over many ticks, so a look
/// every tenth tick misses next to nothing, and costs little.
#[derive(Default)]
struct DomainTally {
    alive: std::collections::BTreeMap<u32, (usize, usize, u64)>,
    spent: [[u64; DOMAINS]; 2],
    lost: [[u64; DOMAINS]; 2],
}

impl DomainTally {
    fn look(&mut self, world: &crate::sim::world::SimWorld) {
        use crate::sim::components::{Blueprinted, UnitStats};
        let mut now = std::collections::BTreeMap::new();
        for &(id, entity) in &world.units {
            let (Ok(stats), Ok(unit)) = (world.ecs.get::<&UnitStats>(entity), world.ecs.get::<&Blueprinted>(entity)) else { continue };
            let blueprint = world.blueprints.get(unit.0);
            let entry = (stats.team.index(), domain_of(blueprint), blueprint.mass.to_num::<u64>());
            // New, or upgraded in place since the last look, which keeps its id and adds its mass.
            let before = self.alive.get(&id.0).map_or(0, |&(_, _, mass)| mass);
            self.spent[entry.0][entry.1] += entry.2.saturating_sub(before);
            now.insert(id.0, entry);
        }
        for (id, &(team, domain, mass)) in &self.alive {
            if !now.contains_key(id) {
                self.lost[team][domain] += mass;
            }
        }
        self.alive = now;
    }
}

pub struct SoakReport {
    pub matches: Vec<MatchSummary>,
    pub tick_times: Vec<Duration>,
}

/// Plays and replays one match per seed, on several threads, Blue and Red playing these
/// strategies. A panic in any match is a failure.
pub fn run_soak(first_seed: u64, count: u64, threads: usize, map: &MapSource, strategies: [&'static Strategy; 2]) -> SoakReport {
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
                let mut tally = DomainTally::default();
                let record = play_match_with(&map, seed, match_tick_limit(map.per_team(), map.map_units()), strategies, |world| {
                    let now = Instant::now();
                    // Ticks that also compute a checksum would inflate the 99th percentile.
                    if !world.current_tick.is_multiple_of(CHECKSUM_INTERVAL) {
                        local_times.push(now - last);
                    }
                    // Counted before the clock restarts, so that it adds nothing to tick times.
                    if world.current_tick.is_multiple_of(10) || world.match_result.is_some() {
                        tally.look(world);
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
                    spent: tally.spent,
                    lost: tally.lost,
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
    /// Prints the results and returns whether every exit criterion held. Without
    /// `gate_tick_time`, tick times are reported but cannot fail the run, since on a shared
    /// machine they measure its other load as much as the game.
    pub fn print(&mut self, gate_tick_time: bool) -> bool {
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
        let drawn_seeds: Vec<u64> = self.matches.iter().filter(|summary| summary.result == Some(MatchResult::Draw)).map(|summary| summary.seed).collect();
        if !drawn_seeds.is_empty() {
            println!("  drawn seeds: {drawn_seeds:?}");
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
        // Where each side's mass went, over every match: spent on each domain, and the share of
        // it lost. A strategy soak reads these for balance between domains.
        for (team, name) in [(0, "blue"), (1, "red")] {
            let spent: Vec<u64> = (0..DOMAINS).map(|domain| self.matches.iter().map(|summary| summary.spent[team][domain]).sum()).collect();
            let lost: Vec<u64> = (0..DOMAINS).map(|domain| self.matches.iter().map(|summary| summary.lost[team][domain]).sum()).collect();
            let line: Vec<String> = DOMAIN_NAMES.iter().enumerate().map(|(domain, label)| format!("{label} {} lost {}", spent[domain], lost[domain])).collect();
            println!("  {name} mass by domain: {}", line.join(", "));
        }
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
                match (tick_gate, gate_tick_time) {
                    (true, _) => "PASS",
                    (false, true) => "FAIL",
                    (false, false) => "over, not gated",
                }
            );
            tick_gate |= !gate_tick_time;
        }
        let passed = unfinished == 0 && mismatched == 0 && tick_gate;
        println!("  result: {}", if passed { "PASS" } else { "FAIL" });
        passed
    }
}
