//! Writes each of the game's continuous sounds to a WAV file in a folder, as the game makes
//! them, for listening to them without playing a match. Each plays at the default volume,
//! zoomed all the way in, fading in at the start and out at the end as it does in the game.
//!
//! `cargo run --release --example sound_preview -- <output folder>`
use std::io::Write;
use std::path::{Path, PathBuf};

use sim_engine::audio::{Bed, BedLevel, SoundSettings, SoundTargets, Synth, BEDS};

const RATE: u32 = 48_000;

/// Plays `targets` for `seconds`, then lets it fade out for a second, in stereo.
fn render(scenes: &[(f32, SoundTargets)]) -> Vec<[f32; 2]> {
    let mut synth = Synth::new(RATE as f32);
    let mut frames = Vec::new();
    for (seconds, targets) in scenes {
        frames.extend((0..(seconds * RATE as f32) as usize).map(|_| synth.next_frame(targets)));
    }
    let quiet = SoundTargets { volume: SoundSettings::default().volume(), ..SoundTargets::default() };
    frames.extend((0..RATE as usize).map(|_| synth.next_frame(&quiet)));
    frames
}

/// 16-bit stereo PCM.
fn save(path: &Path, frames: &[[f32; 2]]) {
    let data = (frames.len() * 4) as u32;
    let mut bytes = Vec::with_capacity(44 + data as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 4).to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for frame in frames {
        for sample in frame {
            bytes.extend_from_slice(&((sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16).to_le_bytes());
        }
    }
    std::fs::File::create(path).and_then(|mut file| file.write_all(&bytes)).expect("the sound saves");
    println!("saved {}", path.display());
}

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).expect("output folder"));
    std::fs::create_dir_all(&out).expect("output folder can be made");
    let quiet = SoundTargets { volume: SoundSettings::default().volume(), ..SoundTargets::default() };
    let one = |bed: Bed, level: BedLevel| quiet.with(bed, level);
    let sounds: [(&str, Vec<(f32, SoundTargets)>); 10] = [
        ("1-one-builder", vec![(10.0, one(Bed::Build, BedLevel::new(1.0, 0.0, 0.33)))]),
        ("2-ten-builders", vec![(10.0, one(Bed::Build, BedLevel::new(1.0, 0.0, 0.83)))]),
        // A builder starting work, stopping, and starting again: its beam powers on and off.
        ("2b-a-builder-starting-and-stopping", vec![(3.0, one(Bed::Build, BedLevel::new(1.0, 0.0, 0.33))), (2.0, quiet), (3.0, one(Bed::Build, BedLevel::new(1.0, 0.0, 0.33)))]),
        ("3-factories", vec![(10.0, one(Bed::Factory, BedLevel::new(1.0, 0.0, 0.6)))]),
        // From the left of the screen, then the middle, then the right.
        ("4-the-sea", vec![(6.0, one(Bed::Ocean, BedLevel::new(1.0, -1.0, 0.0))), (6.0, one(Bed::Ocean, BedLevel::new(1.0, 0.0, 0.0))), (6.0, one(Bed::Ocean, BedLevel::new(1.0, 1.0, 0.0)))]),
        ("5-one-ship-then-a-fleet", vec![(6.0, one(Bed::Sailing, BedLevel::new(0.5, 0.0, 0.0))), (6.0, one(Bed::Sailing, BedLevel::new(0.85, 0.0, 0.0)))]),
        ("6-land-units-moving", vec![(8.0, one(Bed::Engines, BedLevel::new(0.75, 0.0, 0.0)))]),
        ("7-aircraft-passing", vec![(8.0, one(Bed::Aircraft, BedLevel::new(0.75, 0.0, 0.0)))]),
        // A coast while building: the sea on the left, ships under way, builders at work.
        ("8-a-coast-while-building", {
            let mut beds = [BedLevel::default(); BEDS];
            beds[Bed::Ocean as usize] = BedLevel::new(0.7, -0.6, 0.0);
            beds[Bed::Sailing as usize] = BedLevel::new(0.6, -0.3, 0.0);
            beds[Bed::Build as usize] = BedLevel::new(1.0, 0.4, 0.6);
            vec![(14.0, SoundTargets { beds, ..quiet })]
        }),
        // An army on the move inland, with aircraft overhead and a factory working.
        ("9-an-army-on-the-move", {
            let mut beds = [BedLevel::default(); BEDS];
            beds[Bed::Engines as usize] = BedLevel::new(0.85, 0.2, 0.0);
            beds[Bed::Aircraft as usize] = BedLevel::new(0.5, -0.4, 0.0);
            beds[Bed::Factory as usize] = BedLevel::new(1.0, 0.5, 0.33);
            vec![(14.0, SoundTargets { beds, ..quiet })]
        }),
    ];
    for (name, scenes) in sounds {
        save(&out.join(format!("{name}.wav")), &render(&scenes));
    }
}
