//! Asks which maps to download, and fetches them from Forged Alliance Forever's content server
//! into a cache in the player's own folders, never into this repository (GAME_SPECIFICATION.md
//! §20). A map already in the cache is kept, not fetched again.
//!
//! `cargo run --release --bin map-download`, or with the answer given up front, as in
//! `cargo run --release --bin map-download -- all`. Then play one with `--map <folder>`.
//!
//! It uses the system's `curl` to download, and `unzip` (or `tar`) to unpack, so the game
//! needs no network or zip code of its own.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The maps on offer: a name to show, and the map's folder and file name on the server.
const MAPS: [(&str, &str); 2] = [("Hilly Plateau", "hilly_plateau.v0002"), ("Dual Gap Adaptive", "dualgap_adaptive.v0014")];
const SERVER: &str = "https://content.faforever.com/maps";

/// Where maps are kept: `SIM_ENGINE_MAP_CACHE` if set, or else the system's cache folder.
fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SIM_ENGINE_MAP_CACHE") {
        return PathBuf::from(dir);
    }
    let home = || std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home().join("Library/Caches")
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map_or_else(home, PathBuf::from)
    } else {
        std::env::var_os("XDG_CACHE_HOME").map_or_else(|| home().join(".cache"), PathBuf::from)
    };
    base.join("sim-engine").join("maps")
}

/// Whether a map folder holds what `--map` needs.
fn is_complete(folder: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return false;
    };
    let names: Vec<String> = entries.filter_map(Result::ok).filter_map(|entry| entry.file_name().into_string().ok()).map(|name| name.to_ascii_lowercase()).collect();
    names.iter().any(|name| name.ends_with(".scmap")) && names.iter().any(|name| name.ends_with("_save.lua"))
}

/// Turns an answer into the maps it names, by number from 1: "1", "1 2", "1,2", or "all".
/// Nothing, or anything else, is no maps.
fn parse_choice(answer: &str, count: usize) -> Result<Vec<usize>, String> {
    let answer = answer.trim().to_ascii_lowercase();
    if answer.is_empty() || answer == "q" || answer == "none" {
        return Ok(Vec::new());
    }
    if answer == "all" || answer == "a" || answer == "both" {
        return Ok((0..count).collect());
    }
    let mut chosen = Vec::new();
    for part in answer.split(|character: char| character == ',' || character.is_whitespace()).filter(|part| !part.is_empty()) {
        let number: usize = part.parse().map_err(|_| format!("{part:?} is not a map number"))?;
        if !(1..=count).contains(&number) {
            return Err(format!("there is no map {number}"));
        }
        if !chosen.contains(&(number - 1)) {
            chosen.push(number - 1);
        }
    }
    Ok(chosen)
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program).args(args).status().map_err(|error| format!("cannot run {program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} failed ({status})"))
    }
}

/// Downloads and unpacks one map into the cache, unless it is there already. Returns its folder.
fn fetch(cache: &Path, file: &str) -> Result<(PathBuf, bool), String> {
    let folder = cache.join(file);
    if is_complete(&folder) {
        return Ok((folder, false));
    }
    std::fs::create_dir_all(cache).map_err(|error| format!("cannot make {}: {error}", cache.display()))?;
    let zip = cache.join(format!("{file}.zip.part"));
    let zip_text = zip.to_string_lossy().into_owned();
    let url = format!("{SERVER}/{file}.zip");
    run("curl", &["--fail", "--location", "--silent", "--show-error", "--user-agent", "sim-engine-map-download", "--output", &zip_text, &url])?;
    let cache_text = cache.to_string_lossy().into_owned();
    let unpacked = run("unzip", &["-q", "-o", &zip_text, "-d", &cache_text]).or_else(|_| run("tar", &["-xf", &zip_text, "-C", &cache_text]));
    let _ = std::fs::remove_file(&zip);
    unpacked?;
    if !is_complete(&folder) {
        return Err(format!("the download did not hold a map folder named {file}"));
    }
    Ok((folder, true))
}

fn main() {
    let cache = cache_dir();
    println!("Maps from Forged Alliance Forever's vault, kept in {}", cache.display());
    for (index, (name, file)) in MAPS.iter().enumerate() {
        let status = if is_complete(&cache.join(file)) { "downloaded" } else { "not downloaded" };
        println!("  {}. {name:<20} {status}", index + 1);
    }
    // An answer on the command line skips the question.
    let given: Vec<String> = std::env::args().skip(1).collect();
    let answer = if given.is_empty() {
        print!("Download which? Numbers (like 1 2), all, or nothing to quit: ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
        line
    } else {
        given.join(" ")
    };
    let chosen = match parse_choice(&answer, MAPS.len()) {
        Ok(chosen) => chosen,
        Err(message) => {
            println!("{message}");
            std::process::exit(1);
        }
    };
    let mut failed = false;
    for index in chosen {
        let (name, file) = MAPS[index];
        match fetch(&cache, file) {
            Ok((folder, downloaded)) => {
                println!("{name}: {}", if downloaded { "downloaded" } else { "already downloaded, kept" });
                // Check that the game can read it, and say how to play it.
                match sim_engine::map_import::import(&folder, (1, 2)) {
                    Ok(imported) => println!("  {}\n  play it with: cargo run --release -- --map \"{}\"", imported.notes.join("; "), folder.display()),
                    Err(message) => println!("  but the game cannot load it: {message}"),
                }
            }
            Err(message) => {
                println!("{name}: {message}");
                failed = true;
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_name_maps_by_number() {
        assert_eq!(parse_choice("", 2), Ok(vec![]));
        assert_eq!(parse_choice("  q ", 2), Ok(vec![]));
        assert_eq!(parse_choice("1", 2), Ok(vec![0]));
        assert_eq!(parse_choice("2, 1 2", 2), Ok(vec![1, 0]));
        assert_eq!(parse_choice("ALL", 2), Ok(vec![0, 1]));
        assert!(parse_choice("3", 2).is_err());
        assert!(parse_choice("one", 2).is_err());
    }

    #[test]
    fn a_folder_with_the_map_and_its_markers_counts_as_downloaded() {
        let folder = std::env::temp_dir().join(format!("sim-engine-map-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        assert!(!is_complete(&folder));
        std::fs::write(folder.join("test.scmap"), b"").unwrap();
        assert!(!is_complete(&folder), "the markers are missing");
        std::fs::write(folder.join("test_save.lua"), b"").unwrap();
        assert!(is_complete(&folder));
        // Already there, so nothing is downloaded.
        let cache = folder.parent().unwrap();
        let name = folder.file_name().unwrap().to_str().unwrap();
        assert_eq!(fetch(cache, name), Ok((folder.clone(), false)));
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
