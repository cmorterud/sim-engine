//! Lists the units whose portraits are most alike, as a worklist for making them easier to tell
//! apart: each pair with how unlike they are, from 0 for the same picture, and whether their
//! map symbols are the same too, in which case the portrait is all there is to go on.
//!
//! `cargo run --release --example portrait_likeness [-- <how many>]`
//!
//! Portraits are compared shrunk to about what can be made out on a bar button, and only those
//! that could share a build bar: on the same backdrop (land, shore, air, water, depths), and
//! structures with structures.
use sim_engine::hud;
use sim_engine::icons::Icon;
use sim_engine::sim::blueprints::BlueprintDb;

fn main() {
    let shown: usize = std::env::args().nth(1).and_then(|value| value.parse().ok()).unwrap_or(30);
    let db = BlueprintDb::standard();
    let name = |key: &str| {
        db.find(key).map_or_else(
            || key.to_string(),
            |id| {
                let unit = db.get(id);
                let tier = format!("T{}", unit.tier);
                if unit.name.contains(&tier) { unit.name.clone() } else { format!("{} ({tier})", unit.name) }
            },
        )
    };
    println!("{:>4}  {:>8}  {:<40} {:<40} same symbol", "rank", "unlike", "first", "second");
    for (rank, (distance, first, second)) in hud::most_alike_portraits(&db).into_iter().take(shown).enumerate() {
        let same_symbol = if Icon::for_key(first) == Icon::for_key(second) { "yes" } else { "" };
        println!("{:>4}  {distance:>8.4}  {:<40} {:<40} {same_symbol}", rank + 1, name(first), name(second));
    }
}
