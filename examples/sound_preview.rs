//! Writes each of the game's sounds to a WAV file in a folder, as the game makes them, for
//! listening to them without playing a match: the continuous sounds, the fighting, and the great
//! explosions. Each plays at the default volume, zoomed all the way in, fading in at the start
//! and out at the end as it does in the game.
//!
//! `cargo run --release --example sound_preview -- <output folder>`
use std::io::Write;
use std::path::{Path, PathBuf};

use sim_engine::audio::{Bed, BedLevel, OneShot, SoundEvent, SoundSettings, SoundTargets, Synth, BEDS};

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

/// Plays `targets` for `seconds`, starting each sound once at its time, in stereo.
fn render_events(targets: SoundTargets, events: &[(f32, SoundEvent)], seconds: f32) -> Vec<[f32; 2]> {
    let mut synth = Synth::new(RATE as f32);
    let mut events: Vec<(f32, SoundEvent)> = events.to_vec();
    events.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut next = 0;
    (0..(seconds * RATE as f32) as usize)
        .map(|sample| {
            let now = sample as f32 / RATE as f32;
            while next < events.len() && events[next].0 <= now {
                synth.play(events[next].1);
                next += 1;
            }
            synth.next_frame(&targets)
        })
        .collect()
}

/// A preview of sounds that play once: its name, each sound with its time, and how long it runs.
type Preview<'a> = (&'a str, Vec<(f32, SoundEvent)>, f32);

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

    // The fighting, a little apart so that each is heard, and the great explosions.
    let shot = |sound: OneShot, pan: f32| SoundEvent { sound, gain: 1.0, pan };
    let apart = |sounds: &[OneShot]| sounds.iter().enumerate().map(|(index, &sound)| (0.4 + 1.1 * index as f32, shot(sound, 0.0))).collect::<Vec<_>>();
    let once: [Preview; 5] = [
        ("10-guns-light-to-heavy", apart(&[OneShot::Gun(0.1), OneShot::Gun(0.5), OneShot::Gun(0.9)]), 4.0),
        ("11-lasers-then-a-missile", apart(&[OneShot::Laser, OneShot::Laser, OneShot::Missile]), 4.5),
        ("12-shells-striking-small-to-large", apart(&[OneShot::Impact(0.1), OneShot::Impact(0.5), OneShot::Impact(0.9)]), 5.0),
        ("13-units-destroyed-small-to-large", apart(&[OneShot::Destroyed(0.2), OneShot::Destroyed(0.5), OneShot::Destroyed(0.9)]), 6.0),
        ("17-a-strategic-launch-warning", vec![(0.3, shot(OneShot::LaunchWarning, 0.0))], 3.5),
    ];
    for (name, events, seconds) in once {
        save(&out.join(format!("{name}.wav")), &render_events(quiet, &events, seconds));
    }
    // A battle in view: tanks moving, guns firing across the screen, shells striking, now and
    // then a unit destroyed. Drawn from a fixed sequence, the same every time.
    let mut battle = Vec::new();
    let mut seed = 12_345_u32;
    let mut roll = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as f32 / (1 << 24) as f32
    };
    let mut time = 0.3;
    while time < 11.0 {
        let pan = roll() * 2.0 - 1.0;
        let pick = roll();
        let sound = if pick < 0.5 { OneShot::Gun(roll() * 0.6) } else if pick < 0.85 { OneShot::Impact(roll() * 0.7) } else if pick < 0.95 { OneShot::Laser } else { OneShot::Destroyed(0.2 + roll() * 0.6) };
        battle.push((time, shot(sound, pan)));
        time += 0.08 + roll() * 0.35;
    }
    let mut armies = [BedLevel::default(); BEDS];
    armies[Bed::Engines as usize] = BedLevel::new(0.8, 0.0, 0.0);
    save(&out.join("14-a-battle.wav"), &render_events(SoundTargets { beds: armies, ..quiet }, &battle, 12.0));
    // The great explosions, over a coast while building, to hear how everything dips under them
    // and comes back.
    let mut coast = [BedLevel::default(); BEDS];
    coast[Bed::Ocean as usize] = BedLevel::new(0.8, -0.4, 0.0);
    coast[Bed::Build as usize] = BedLevel::new(1.0, 0.4, 0.5);
    let coast = SoundTargets { beds: coast, ..quiet };
    save(&out.join("15-a-commander-blowing-up.wav"), &render_events(coast, &[(1.5, shot(OneShot::CommanderExplosion, 0.2))], 8.0));
    save(&out.join("16-a-strategic-missile-striking.wav"), &render_events(coast, &[(0.5, shot(OneShot::LaunchWarning, 0.0)), (4.0, shot(OneShot::Nuke, 0.0))], 17.0));
}
