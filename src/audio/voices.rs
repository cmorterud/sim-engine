//! Sounds that play once: the fighting, and the two great explosions, a commander's and a
//! strategic missile's. Each is struck softly, never switched on, so that nothing clicks; the
//! fighting is low-passed and soft, a battle heard rather than a barrage. Only the commander's
//! explosion and the strategic missile's are loud, and they dip everything else while they last.

use super::{advance, sine, LowPass, Noise};

/// A sound that plays once.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum OneShot {
    /// A gun firing, from 0, the lightest, to 1, the heaviest.
    Gun(f32),
    /// A laser's beam striking.
    Laser,
    /// A cruise missile leaving its launcher.
    Missile,
    /// A shell or missile striking, by the size of its blast, from 0 to 1.
    Impact(f32),
    /// A unit destroyed, by its size, from 0 to 1.
    Destroyed(f32),
    /// A commander blowing up as it dies.
    CommanderExplosion,
    /// A strategic missile striking.
    Nuke,
    /// A strategic missile launched, by anyone: a low, soft warning.
    LaunchWarning,
}

impl OneShot {
    /// The two great explosions, which are loud and dip everything else.
    pub const fn is_great(self) -> bool {
        matches!(self, Self::CommanderExplosion | Self::Nuke)
    }

    /// How long it lasts, in seconds.
    pub fn seconds(self) -> f32 {
        match self {
            Self::Gun(size) => 0.22 + 0.3 * size,
            Self::Laser => 0.3,
            Self::Missile => 1.0,
            Self::Impact(size) => 0.45 + 0.9 * size,
            Self::Destroyed(size) => 0.7 + 1.3 * size,
            Self::CommanderExplosion => 4.0,
            Self::Nuke => 10.0,
            Self::LaunchWarning => WARNING_STEP * WARNING_NOTES as f32,
        }
    }

    /// How far it dips everything else, as a share of its level, and for how long, in seconds.
    pub const fn dips(self) -> Option<(f32, f32)> {
        match self {
            Self::CommanderExplosion => Some((0.45, 3.0)),
            Self::Nuke => Some((0.3, 6.0)),
            _ => None,
        }
    }

    /// How loud it is struck: heavier guns, bigger blasts, and bigger units a little louder.
    fn gain(self) -> f32 {
        match self {
            Self::Gun(size) => 0.075 * (0.8 + 0.4 * size),
            Self::Laser => 0.06,
            Self::Missile => 0.07,
            Self::Impact(size) => 0.09 * (0.8 + 0.45 * size),
            // A bigger unit's boom is darker, which carries less: it is struck harder to make up.
            Self::Destroyed(size) => 0.09 * (0.75 + 0.6 * size),
            Self::CommanderExplosion => 0.55,
            Self::Nuke => 0.7,
            Self::LaunchWarning => 0.14,
        }
    }
}

/// The launch warning: two low notes, high then low, three times.
const WARNING_NOTES: u32 = 6;
const WARNING_STEP: f32 = 0.42;
const WARNING_NOTE: f32 = 0.34;

/// A sound to play once: which, how loud from 0 to 1, and where across the screen.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SoundEvent {
    pub sound: OneShot,
    pub gain: f32,
    pub pan: f32,
}

/// A one-shot sound playing.
#[derive(Clone, Debug)]
pub(super) struct Voice {
    pub event: SoundEvent,
    /// Samples played, counted whole: a time added up a sample at a time drifts, and a sound
    /// ten seconds long ran over by a twentieth of a second.
    played: u32,
    rate: f32,
    noise: Noise,
    /// Two low-passes for the noise, a third for a deep rumble, and two oscillators.
    filters: [LowPass; 3],
    phases: [f32; 2],
}

/// Rises over `attack`, dies away by `decay`, and fades to nothing at `length`: never switched on
/// or off.
fn envelope(age: f32, attack: f32, decay: f32, length: f32) -> f32 {
    (age / attack).min(1.0) * (-age / decay).exp() * ((length - age) / 0.05).clamp(0.0, 1.0)
}

/// A low-pass whose cutoff moves as the sound goes on.
fn moving(filter: &mut LowPass, cutoff: f32, input: f32, rate: f32) -> f32 {
    filter.gain = 1.0 - (-std::f32::consts::TAU * cutoff / rate).exp();
    filter.run(input)
}

impl Voice {
    pub fn new(event: SoundEvent, seed: u32, rate: f32) -> Self {
        Self { event, played: 0, rate, noise: Noise(seed | 1), filters: [LowPass::new(1_000.0, rate); 3], phases: [0.0; 2] }
    }

    /// How long it has played, in seconds.
    pub fn age(&self) -> f32 {
        self.played as f32 / self.rate
    }

    /// Whether it has finished.
    pub fn done(&self) -> bool {
        self.age() >= self.event.sound.seconds()
    }

    /// The next sample, before its gain and place across the screen.
    pub fn next(&mut self) -> f32 {
        let (age, rate) = (self.age(), self.rate);
        self.played = self.played.saturating_add(1);
        let sound = self.event.sound;
        let length = sound.seconds();
        if age >= length {
            return 0.0;
        }
        let white = self.noise.next();
        let [first, second, deep] = &mut self.filters;
        let [tone, other] = &mut self.phases;
        // Noise through two low-passes in a row, its cutoff moving: a soft rush, never a hiss.
        let mut rush = |cutoff: f32| moving(second, cutoff, moving(first, cutoff, white, rate), rate);
        let out = match sound {
            // A soft thump and a puff, lower and longer for heavier guns.
            OneShot::Gun(size) => {
                let pitch = (150.0 - 70.0 * size) * (0.7 + 0.3 * (-age / 0.03).exp());
                let thump = sine(advance(tone, pitch, rate));
                let puff = rush(1_300.0 - 700.0 * size) * 4.0;
                envelope(age, 0.003, 0.04 + 0.07 * size, length) * (0.65 * thump + 0.9 * puff)
            }
            // A soft, falling "fsss": noise whose brightness falls away at once. No tone in it.
            OneShot::Laser => {
                let cutoff = 700.0 + 1_400.0 * (-age / 0.06).exp();
                let hiss = rush(cutoff) - moving(deep, 500.0, white, rate) * 0.5;
                envelope(age, 0.006, 0.08, length) * hiss * 4.5
            }
            // A whoosh that brightens as the missile leaves, over a low rumble.
            OneShot::Missile => {
                let cutoff = 300.0 + 1_300.0 * (1.0 - (-age / 0.12).exp());
                let whoosh = rush(cutoff) * 4.0;
                let rumble = moving(deep, 150.0, white, rate) * 6.0;
                envelope(age, 0.03, 0.3, length) * (whoosh + 0.4 * rumble)
            }
            // A low thump that drops, and a dull burst, bigger and longer for a bigger blast.
            OneShot::Impact(size) => {
                let pitch = (115.0 - 45.0 * size) * (0.45 + 0.55 * (-age / 0.07).exp());
                let thump = sine(advance(tone, pitch, rate));
                let burst = rush((1_000.0 - 500.0 * size) * (0.4 + 0.6 * (-age / 0.15).exp())) * 4.0;
                envelope(age, 0.004, 0.08 + 0.3 * size, length) * (0.7 * thump + 0.8 * burst)
            }
            // A crunching boom that darkens as it dies away, deeper for a bigger unit.
            OneShot::Destroyed(size) => {
                let pitch = (95.0 - 40.0 * size) * (0.45 + 0.55 * (-age / 0.12).exp());
                let thump = sine(advance(tone, pitch, rate));
                let crunch = rush((350.0 + 850.0 * (-age / 0.25).exp()) * (1.0 - 0.45 * size)) * 4.5;
                envelope(age, 0.006, 0.15 + 0.4 * size, length) * (0.55 * thump + 0.9 * crunch)
            }
            // A deep thump, a roar that darkens, and a rumble under it all.
            OneShot::CommanderExplosion => {
                let pitch = 26.0 + 32.0 * (-age / 0.18).exp();
                let thump = sine(advance(tone, pitch, rate)) * envelope(age, 0.008, 0.35, length);
                let roar = rush(180.0 + 1_100.0 * (-age / 0.9).exp()) * 4.0 * envelope(age, 0.01, 1.1, length);
                let rumble = moving(deep, 90.0, white, rate) * 9.0 * envelope(age, 0.05, 1.6, length);
                0.8 * thump + 0.9 * roar + 0.6 * rumble
            }
            // The deepest thump, a roar that swells open over the first second and closes slowly,
            // and a long rumble that outlasts it.
            OneShot::Nuke => {
                let pitch = 22.0 + 26.0 * (-age / 0.3).exp();
                let thump = sine(advance(tone, pitch, rate)) * envelope(age, 0.015, 0.6, length);
                let opening = (1.0 - (-age / 0.3).exp()) * (-age / 2.5).exp();
                let roar = rush(120.0 + 780.0 * opening) * 4.5 * envelope(age, 0.02, 2.8, length);
                let rumble = moving(deep, 70.0, white, rate) * 11.0 * envelope(age, 0.2, 4.0, length);
                0.8 * thump + 0.9 * roar + 0.7 * rumble
            }
            // Two low notes, high then low, three times: soft, warm, unlike the high alarm chime.
            OneShot::LaunchWarning => {
                let note = (age / WARNING_STEP) as u32;
                let within = age - note as f32 * WARNING_STEP;
                let pitch = if note.is_multiple_of(2) { 220.0 } else { 164.81 };
                let voice = sine(advance(tone, pitch, rate)) + 0.25 * sine(advance(other, pitch * 2.0, rate));
                let shape = (within / 0.03).min(1.0) * ((WARNING_NOTE - within) / 0.08).clamp(0.0, 1.0);
                voice * shape
            }
        };
        out * sound.gain()
    }
}
