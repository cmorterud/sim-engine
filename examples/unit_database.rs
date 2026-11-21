//! Writes the unit database page, as the game writes it in the browser (`?units`), to a folder
//! with the portrait sheet beside it, for looking at it without building for the browser.
//!
//! `cargo run --release --example unit_database -- <output folder>`
use std::path::PathBuf;

use sim_engine::sim::blueprints::BlueprintDb;

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).expect("output folder"));
    std::fs::create_dir_all(&out).expect("output folder can be made");
    std::fs::write(out.join("units.html"), sim_engine::unit_database::page(&BlueprintDb::standard())).expect("page saves");
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/portraits.png"), out.join("portraits.png")).expect("portraits copy");
    println!("saved {}", out.join("units.html").display());
}
