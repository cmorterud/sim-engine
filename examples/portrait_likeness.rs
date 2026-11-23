//! Lists the units whose portraits are most alike, as a worklist for making them easier to tell
//! apart: each pair with how unlike they are, from 0 for the same picture, and whether their
//! map symbols are the same too, in which case the portrait is all there is to go on.
//!
//! `cargo run --release --example portrait_likeness [-- <how many>] [--outline]`
//!
//! Pairs are ranked by the whole picture, or with `--outline` by outline alone, whatever the
//! colors, which is how far apart their shapes are. Both measures are printed.
//!
//! Portraits are compared shrunk to about what can be made out on a bar button, and only those
//! that could share a build bar: on the same backdrop (land, shore, air, water, depths), and
//! structures with structures.
use sim_engine::hud::{self, Likeness};
use sim_engine::icons::Icon;
use sim_engine::sim::blueprints::BlueprintDb;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let by = if args.iter().any(|arg| arg == "--outline") { Likeness::Outline } else { Likeness::Picture };
    let shown: usize = args.iter().find_map(|arg| arg.parse().ok()).unwrap_or(30);
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
    let glances: Vec<hud::Glance> = hud::portrait_glances();
    let glance = |key: &str| glances.iter().find(|glance| glance.key == key).expect("every ranked portrait has a glance");
    println!("Ranked by {by:?}.");
    println!("{:>4}  {:>8}  {:>8}  {:<36} {:<36} same symbol", "rank", "picture", "outline", "first", "second");
    for (rank, (_, first, second)) in hud::most_alike_portraits(&db, by).into_iter().take(shown).enumerate() {
        let (picture, outline) = (glance(first).distance(glance(second), Likeness::Picture), glance(first).distance(glance(second), Likeness::Outline));
        let same_symbol = if Icon::for_key(first) == Icon::for_key(second) { "yes" } else { "" };
        println!("{:>4}  {picture:>8.4}  {outline:>8.4}  {:<36} {:<36} {same_symbol}", rank + 1, name(first), name(second));
    }
}
