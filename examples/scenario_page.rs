//! Writes the playtest scenarios page, as the game writes it in the browser (`?scenarios`), to a
//! folder, for looking at it without building for the browser.
//!
//! `cargo run --release --example scenario_page -- <output folder>`
use std::path::PathBuf;

use sim_engine::sim::blueprints::BlueprintDb;

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).expect("output folder"));
    std::fs::create_dir_all(&out).expect("output folder can be made");
    let page = sim_engine::scenario_page::page(&sim_engine::sim::playtest_scenarios::all(), &BlueprintDb::standard());
    std::fs::write(out.join("scenarios.html"), page).expect("page saves");
    println!("saved {}", out.join("scenarios.html").display());
}
