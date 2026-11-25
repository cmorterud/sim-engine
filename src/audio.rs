//! Sound, generated in code so that there are no audio files to source. Presentation only:
//! it reads snapshots and never touches the simulation, so it cannot affect determinism.
//!
//! Everything is soft enough to hear all match long, and fades in and out rather than switching,
//! but for a commander blowing up and a strategic missile striking, which are meant to be big.
//! The battlefield is heard as it is seen, close up: the sea, ships under way, engines, aircraft,
//! and the beams of builders at work, each from where it is across the screen. It is in stereo, but never hard
//! to one side.

use std::f32::consts::{FRAC_PI_4, TAU};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

use crate::sim::blueprints::{BlueprintDb, BlueprintId};
use crate::sim::components::{MoveClass, SimId};
use crate::sim::world::{SimSnapshot, UnitView};

mod events;
mod voices;

pub use events::scene_events;
pub use voices::{OneShot, SoundEvent};

/// The most one-shot sounds playing at once. When full, the oldest small one gives way.
const MOST_VOICES: usize = 16;
/// How fast the rest dips under a great explosion, and comes back after.
const DIP_SECONDS: f32 = 0.15;
const RECOVER_SECONDS: f32 = 1.2;

/// How long "commander under attack" stays silent after playing, however often the commander is hit.
pub const ALARM_COOLDOWN_SECONDS: f64 = 30.0;
/// How long the text alert stays on screen.
const ALERT_SECONDS: f64 = 4.0;

/// How long sounds take to fade in and out, so that they never click.
const FADE_SECONDS: f32 = 0.08;
/// Work, ships, engines, and aircraft are only heard from close up: at full volume up to the
/// first zoom level, fading to silence by the second. Zoom runs from 0, as close as allowed, to
/// 1, all the way out.
const FULL_VOLUME_ZOOM: f64 = 0.12;
const SILENT_ZOOM: f64 = 0.3;
/// The sea carries a little further out.
const OCEAN_FULL_ZOOM: f64 = 0.15;
const OCEAN_SILENT_ZOOM: f64 = 0.38;
/// How far to one side a sound can sit, at the screen's edge: never all the way.
const MAX_PAN: f32 = 0.6;
/// Below this, a continuous sound is not worked out at all.
const SILENT_LEVEL: f32 = 1e-4;

/// The builders' beams, and the factories', which are lower.
const BUILD_GAIN: f32 = 0.019;
const FACTORY_GAIN: f32 = 0.017;
const OCEAN_GAIN: f32 = 0.027;
const SAILING_GAIN: f32 = 0.028;
const ENGINE_GAIN: f32 = 0.024;
const AIRCRAFT_GAIN: f32 = 0.018;
const ALARM_GAIN: f32 = 0.4;

/// The band of a builder's beam fizz, in Hz, and a factory's, lower.
const BUILD_BAND: (f32, f32) = (1_200.0, 3_500.0);
const FACTORY_BAND: (f32, f32) = (600.0, 1_800.0);
/// How long a beam takes to fade in, and out: slower than the rest. A beam that rose in pitch as
/// it started and fell as it stopped was a motor spinning up and down, and sounded like a power
/// drill (2026-11-24).
const BEAM_FADE_IN_SECONDS: f32 = 0.25;
const BEAM_FADE_OUT_SECONDS: f32 = 0.35;

const SELECT_GAIN: f32 = 0.14;
const SELECT_SECONDS: f32 = 0.12;

const ALARM_NOTE_SECONDS: f32 = 0.2;
const ALARM_STEP_SECONDS: f32 = 0.22;
const ALARM_NOTES: u32 = 4;

/// The continuous sounds, each with a level and a place across the screen.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Bed {
    /// Builders at work: the soft fizz of their beams.
    Build,
    /// Factories making something: the same fizz, lower.
    Factory,
    /// The sea, wherever water is in view.
    Ocean,
    /// The wash of ships under way.
    Sailing,
    /// Land units and hovercraft moving.
    Engines,
    /// Aircraft passing.
    Aircraft,
}

pub const BEDS: usize = 6;

impl Bed {
    pub const ALL: [Self; BEDS] = [Self::Build, Self::Factory, Self::Ocean, Self::Sailing, Self::Engines, Self::Aircraft];
}

/// One continuous sound: how loud, from 0 to 1, where across the screen, from -1 at the left
/// edge to 1 at the right, and, for the beams, how busy, from 0 to 1, which makes the sound
/// fuller without making it much louder.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct BedLevel {
    pub level: f32,
    pub pan: f32,
    pub busy: f32,
}

impl BedLevel {
    pub const fn new(level: f32, pan: f32, busy: f32) -> Self {
        Self { level, pan, busy }
    }
}

/// What the sound should be doing right now. All levels run from 0 to 1.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct SoundTargets {
    pub beds: [BedLevel; BEDS],
    pub volume: f32,
    /// Goes up by one for each alarm to play.
    pub alarm_serial: u32,
    /// Goes up by one for each selection cue to play.
    pub select_serial: u32,
}

impl SoundTargets {
    /// These targets with one continuous sound set.
    pub fn with(mut self, bed: Bed, level: BedLevel) -> Self {
        self.beds[bed as usize] = level;
        self
    }
}

fn advance(phase: &mut f32, frequency: f32, rate: f32) -> f32 {
    let now = *phase;
    *phase = (now + frequency / rate).fract();
    now
}

fn sine(phase: f32) -> f32 {
    (phase * TAU).sin()
}

/// A rise and fall from 0 to 1 and back, smooth at both ends.
fn swell(phase: f32) -> f32 {
    0.5 - 0.5 * (phase * TAU).cos()
}

/// How a sound across the screen is shared between the two speakers, at the same power wherever
/// it is: in the middle each speaker gets all of it, as a single speaker did before stereo.
fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) * MAX_PAN + 1.0) * FRAC_PI_4;
    (angle.cos() * std::f32::consts::SQRT_2, angle.sin() * std::f32::consts::SQRT_2)
}

/// White noise from a seeded generator, so that tests hear the same thing every time.
#[derive(Clone, Debug)]
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

#[derive(Copy, Clone, Debug)]
struct LowPass {
    gain: f32,
    out: f32,
}

impl LowPass {
    fn new(cutoff: f32, rate: f32) -> Self {
        Self { gain: 1.0 - (-TAU * cutoff / rate).exp(), out: 0.0 }
    }

    fn run(&mut self, input: f32) -> f32 {
        self.out += self.gain * (input - self.out);
        self.out
    }
}

/// Two low-passes in a row: it falls away twice as steeply above the cutoff, so that noise
/// through it is a soft rush rather than a hiss.
#[derive(Copy, Clone, Debug)]
struct SoftPass(LowPass, LowPass);

impl SoftPass {
    fn new(cutoff: f32, rate: f32) -> Self {
        Self(LowPass::new(cutoff, rate), LowPass::new(cutoff, rate))
    }

    fn run(&mut self, input: f32) -> f32 {
        self.1.run(self.0.run(input))
    }
}

/// Noise between two pitches: what is above `low`, softened above `high`.
#[derive(Copy, Clone, Debug)]
struct Band {
    low: LowPass,
    high: SoftPass,
}

impl Band {
    fn new(low: f32, high: f32, rate: f32) -> Self {
        Self { low: LowPass::new(low, rate), high: SoftPass::new(high, rate) }
    }

    fn run(&mut self, input: f32) -> f32 {
        self.high.run(input - self.low.run(input))
    }
}

/// The sea: a soft rush, deep beneath, rising and falling in two slow swells out of step with
/// each other, so that it never audibly repeats.
#[derive(Clone, Debug)]
struct Ocean {
    surf: SoftPass,
    deep: SoftPass,
    swells: [f32; 2],
}

impl Ocean {
    fn new(rate: f32) -> Self {
        Self { surf: SoftPass::new(650.0, rate), deep: SoftPass::new(130.0, rate), swells: [0.0, 0.37] }
    }

    fn voice(&mut self, noise: &mut Noise, rate: f32) -> f32 {
        let white = noise.next();
        let surf = self.surf.run(white);
        let deep = self.deep.run(white);
        let first = swell(advance(&mut self.swells[0], 1.0 / 8.7, rate));
        let second = swell(advance(&mut self.swells[1], 1.0 / 5.9, rate));
        let rise = 0.35 + 0.65 * (0.65 * first * first + 0.35 * second);
        (9.0 * surf + 14.0 * deep) * rise
    }
}

/// The wash along a hull under way: water rushing and lapping, in the middle of the range.
#[derive(Clone, Debug)]
struct Wash {
    band: Band,
    laps: [f32; 2],
}

impl Wash {
    fn new(rate: f32) -> Self {
        Self { band: Band::new(240.0, 750.0, rate), laps: [0.0, 0.5] }
    }

    fn voice(&mut self, noise: &mut Noise, rate: f32) -> f32 {
        let water = self.band.run(noise.next());
        let lap = 0.7 + 0.18 * swell(advance(&mut self.laps[0], 0.63, rate)) + 0.12 * swell(advance(&mut self.laps[1], 1.07, rate));
        12.0 * water * lap
    }
}

/// Engines: a low, soft rumble, with a quiet tone in it that wanders a little in pitch, so that
/// it reads as machinery at work without droning.
#[derive(Clone, Debug)]
struct Engines {
    rumble: SoftPass,
    tone: f32,
    overtone: f32,
    wander: f32,
}

impl Engines {
    fn new(rate: f32) -> Self {
        Self { rumble: SoftPass::new(170.0, rate), tone: 0.0, overtone: 0.0, wander: 0.0 }
    }

    fn voice(&mut self, noise: &mut Noise, rate: f32) -> f32 {
        let rumble = self.rumble.run(noise.next());
        let pitch = 58.0 * (1.0 + 0.03 * sine(advance(&mut self.wander, 0.21, rate)));
        let tone = sine(advance(&mut self.tone, pitch, rate)) + 0.25 * sine(advance(&mut self.overtone, pitch * 2.0, rate));
        16.0 * rumble + 0.35 * tone
    }
}

/// Aircraft: an airy rush with a faint tone, rising and falling a little as they pass.
#[derive(Clone, Debug)]
struct Aircraft {
    air: Band,
    tone: f32,
    pass: f32,
}

impl Aircraft {
    fn new(rate: f32) -> Self {
        Self { air: Band::new(380.0, 1100.0, rate), tone: 0.0, pass: 0.0 }
    }

    fn voice(&mut self, noise: &mut Noise, rate: f32) -> f32 {
        let air = self.air.run(noise.next());
        let tone = sine(advance(&mut self.tone, 205.0, rate));
        let passing = 0.8 + 0.2 * swell(advance(&mut self.pass, 0.27, rate));
        (9.0 * air + 0.12 * tone) * passing
    }
}

/// A building beam: a soft, steady fizz of energy, noise between two pitches with no tone in it at
/// all, chosen by ear from four candidates (2026-11-24). Tones were tried first, and failed:
/// chimes did not sound like building; a laser of pure tones stacked into a chord, with a gentle
/// pulse, was heard as chiming; and one that rose in pitch as it started sounded like a power
/// drill. With no tone, it can do neither. A crowd adds a little of a band below, which makes it
/// fuller rather than louder.
#[derive(Clone, Debug)]
struct Fizz {
    band: Band,
    under: Band,
}

impl Fizz {
    fn new((low, high): (f32, f32), rate: f32) -> Self {
        Self { band: Band::new(low, high, rate), under: Band::new(low * 0.55, high * 0.55, rate) }
    }

    fn voice(&mut self, noise: &mut Noise, busy: f32) -> f32 {
        let white = noise.next();
        6.5 * (self.band.run(white) + 0.35 * busy.clamp(0.0, 1.0) * self.under.run(white))
    }
}

/// Makes the samples. It does not know about audio devices, so that tests can check its output.
#[derive(Clone, Debug)]
pub struct Synth {
    rate: f32,
    fade: f32,
    beds: [BedLevel; BEDS],
    volume: f32,
    noise: Noise,
    build: Fizz,
    factory: Fizz,
    /// How fast a beam fades in, and out, a sample.
    beam_rise: f32,
    beam_fall: f32,
    ocean: Ocean,
    wash: Wash,
    engines: Engines,
    aircraft: Aircraft,
    alarm_phase: f32,
    alarm_elapsed: Option<f32>,
    alarm_serial: u32,
    select_phase: f32,
    select_elapsed: Option<f32>,
    select_serial: u32,
    /// The one-shot sounds playing, and the seed for the next one's noise.
    voices: Vec<voices::Voice>,
    seed: u32,
    /// How far everything but the great explosions is dipped, from 1, not at all, down.
    dip: f32,
    dipping: f32,
    recovering: f32,
}

impl Synth {
    pub fn new(rate: f32) -> Self {
        Self {
            rate,
            fade: 1.0 - (-1.0 / (FADE_SECONDS * rate)).exp(),
            beds: [BedLevel::default(); BEDS],
            volume: 0.0,
            noise: Noise(0x9E37_79B9),
            build: Fizz::new(BUILD_BAND, rate),
            factory: Fizz::new(FACTORY_BAND, rate),
            beam_rise: 1.0 - (-1.0 / (BEAM_FADE_IN_SECONDS * rate)).exp(),
            beam_fall: 1.0 - (-1.0 / (BEAM_FADE_OUT_SECONDS * rate)).exp(),
            ocean: Ocean::new(rate),
            wash: Wash::new(rate),
            engines: Engines::new(rate),
            aircraft: Aircraft::new(rate),
            alarm_phase: 0.0,
            alarm_elapsed: None,
            alarm_serial: 0,
            select_phase: 0.0,
            select_elapsed: None,
            select_serial: 0,
            voices: Vec::with_capacity(MOST_VOICES),
            seed: 0x2545_F491,
            dip: 1.0,
            dipping: 1.0 - (-1.0 / (DIP_SECONDS * rate)).exp(),
            recovering: 1.0 - (-1.0 / (RECOVER_SECONDS * rate)).exp(),
        }
    }

    /// Starts a sound that plays once. With every voice busy, the oldest small sound gives way;
    /// a great explosion never does.
    pub fn play(&mut self, event: SoundEvent) {
        if self.voices.len() >= MOST_VOICES {
            let oldest = self.voices.iter().enumerate().filter(|(_, voice)| !voice.event.sound.is_great()).max_by(|a, b| a.1.age().total_cmp(&b.1.age())).map(|(index, _)| index);
            match oldest {
                Some(index) => {
                    self.voices.swap_remove(index);
                }
                None => return,
            }
        }
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.voices.push(voices::Voice::new(event, self.seed, self.rate));
    }

    /// How many one-shot sounds are playing.
    pub fn playing(&self) -> usize {
        self.voices.len()
    }

    /// A short, soft blip that rises a little, so that it reads as a response and not a warning.
    /// Until 2026-11-19 it swept far higher, to nearly 1 kHz above, and clicked in sharply.
    fn select_voice(&mut self, serial: u32) -> f32 {
        if serial != self.select_serial {
            self.select_serial = serial;
            self.select_elapsed = Some(0.0);
        }
        let Some(elapsed) = self.select_elapsed else {
            return 0.0;
        };
        if elapsed >= SELECT_SECONDS {
            self.select_elapsed = None;
            return 0.0;
        }
        self.select_elapsed = Some(elapsed + 1.0 / self.rate);
        let phase = advance(&mut self.select_phase, 620.0 + 900.0 * elapsed, self.rate);
        let envelope = (elapsed / 0.006).min(1.0) * (-elapsed / 0.03).exp() * ((SELECT_SECONDS - elapsed) / 0.02).min(1.0);
        envelope * (sine(phase) + 0.15 * sine((phase * 2.0).fract()))
    }

    /// Two notes, high then low, played twice.
    fn alarm_voice(&mut self, serial: u32) -> f32 {
        if serial != self.alarm_serial {
            self.alarm_serial = serial;
            self.alarm_elapsed = Some(0.0);
        }
        let Some(elapsed) = self.alarm_elapsed else {
            return 0.0;
        };
        let note = (elapsed / ALARM_STEP_SECONDS) as u32;
        if note >= ALARM_NOTES {
            self.alarm_elapsed = None;
            return 0.0;
        }
        self.alarm_elapsed = Some(elapsed + 1.0 / self.rate);
        // A falling fourth, struck softly and ringing out like a chime, rather than switched on
        // and off: clear over everything else without being shrill.
        let frequency = if note.is_multiple_of(2) { 784.0 } else { 587.33 };
        let phase = advance(&mut self.alarm_phase, frequency, self.rate);
        let within = elapsed - note as f32 * ALARM_STEP_SECONDS;
        if within >= ALARM_NOTE_SECONDS {
            return 0.0;
        }
        let envelope = (within / 0.012).min(1.0) * (-within / 0.12).exp() * ((ALARM_NOTE_SECONDS - within) / 0.03).min(1.0);
        1.1 * envelope * (sine(phase) + 0.2 * sine((phase * 2.0).fract()))
    }

    /// The next sample for each speaker, left then right.
    pub fn next_frame(&mut self, targets: &SoundTargets) -> [f32; 2] {
        let rate = self.rate;
        self.volume += self.fade * (targets.volume - self.volume);
        let (mut left, mut right) = (0.0, 0.0);
        for bed in Bed::ALL {
            let index = bed as usize;
            let (target, now) = (targets.beds[index], &mut self.beds[index]);
            let fade = match bed {
                Bed::Build | Bed::Factory if target.level > now.level => self.beam_rise,
                Bed::Build | Bed::Factory => self.beam_fall,
                _ => self.fade,
            };
            now.level += fade * (target.level - now.level);
            now.pan += self.fade * (target.pan - now.pan);
            now.busy += self.fade * (target.busy - now.busy);
            let (level, pan, busy) = (now.level, now.pan, now.busy);
            if level < SILENT_LEVEL && target.level <= 0.0 {
                continue;
            }
            let noise = &mut self.noise;
            let sound = match bed {
                Bed::Build => self.build.voice(noise, busy) * BUILD_GAIN,
                Bed::Factory => self.factory.voice(noise, busy) * FACTORY_GAIN,
                Bed::Ocean => self.ocean.voice(noise, rate) * OCEAN_GAIN,
                Bed::Sailing => self.wash.voice(noise, rate) * SAILING_GAIN,
                Bed::Engines => self.engines.voice(noise, rate) * ENGINE_GAIN,
                Bed::Aircraft => self.aircraft.voice(noise, rate) * AIRCRAFT_GAIN,
            } * level;
            let (to_left, to_right) = pan_gains(pan);
            left += sound * to_left;
            right += sound * to_right;
        }
        // However many play at once, together they stay about as loud as one at its fullest:
        // each gets quieter as more join, so that a busy scene never piles up into clutter, and
        // the selection cue and the alarm are always heard over it.
        let together = self.beds.iter().map(|bed| bed.level).sum::<f32>().max(1.0).sqrt().recip();
        // A great explosion dips everything else for a while, then lets it back.
        let dip_to = self.voices.iter().filter_map(|voice| voice.event.sound.dips().filter(|&(_, seconds)| voice.age() < seconds).map(|(level, _)| level)).fold(1.0, f32::min);
        self.dip += if dip_to < self.dip { self.dipping } else { self.recovering } * (dip_to - self.dip);
        let (mut left, mut right) = (left * together * self.dip, right * together * self.dip);
        for voice in &mut self.voices {
            let event = voice.event;
            // The great explosions and the launch warning are what everything dips for.
            let dipped = if event.sound.is_great() || event.sound == OneShot::LaunchWarning { 1.0 } else { self.dip };
            let sound = voice.next() * event.gain * dipped;
            let (to_left, to_right) = pan_gains(event.pan);
            left += sound * to_left;
            right += sound * to_right;
        }
        self.voices.retain(|voice| !voice.done());
        let middle = self.alarm_voice(targets.alarm_serial) * ALARM_GAIN + self.select_voice(targets.select_serial) * SELECT_GAIN;
        // Soft clipping, so that everything at once cannot distort harshly.
        [((left + middle) * self.volume).tanh(), ((right + middle) * self.volume).tanh()]
    }

    /// Both speakers mixed to one, for checking how a sound sounds.
    pub fn next_sample(&mut self, targets: &SoundTargets) -> f32 {
        let [left, right] = self.next_frame(targets);
        0.5 * (left + right)
    }
}

/// The values shared with the audio thread. Levels are stored as `f32` bits.
#[derive(Debug, Default)]
struct SharedTargets {
    /// Each continuous sound's level, pan, and busyness.
    beds: [[AtomicU32; 3]; BEDS],
    volume: AtomicU32,
    alarm_serial: AtomicU32,
    select_serial: AtomicU32,
    frames_played: AtomicU64,
}

impl SharedTargets {
    fn read(&self) -> SoundTargets {
        let load = |value: &AtomicU32| f32::from_bits(value.load(Ordering::Relaxed));
        SoundTargets {
            beds: std::array::from_fn(|index| {
                let [level, pan, busy] = &self.beds[index];
                BedLevel { level: load(level), pan: load(pan), busy: load(busy) }
            }),
            volume: load(&self.volume),
            alarm_serial: self.alarm_serial.load(Ordering::Relaxed),
            select_serial: self.select_serial.load(Ordering::Relaxed),
        }
    }
}

/// Plays sound through the default output device for as long as it is kept. In the browser,
/// that is Web Audio, which stays silent until the page has been clicked or typed on: call
/// `resume` then.
pub struct SoundEngine {
    shared: Arc<SharedTargets>,
    stream: cpal::Stream,
    /// One-shot sounds, to the audio thread.
    events: Sender<SoundEvent>,
}

impl SoundEngine {
    /// Fails if there is no usable output device. The game carries on without sound.
    pub fn start() -> Result<Self, String> {
        let device = cpal::default_host().default_output_device().ok_or("no audio output device")?;
        let config = device.default_output_config().map_err(|error| error.to_string())?;
        let shared = Arc::new(SharedTargets::default());
        let (events, received) = std::sync::mpsc::channel();
        let format = config.sample_format();
        let config: cpal::StreamConfig = config.into();
        let stream = match format {
            SampleFormat::F32 => open_stream::<f32>(&device, &config, &shared, received),
            SampleFormat::F64 => open_stream::<f64>(&device, &config, &shared, received),
            SampleFormat::I16 => open_stream::<i16>(&device, &config, &shared, received),
            SampleFormat::I32 => open_stream::<i32>(&device, &config, &shared, received),
            SampleFormat::U16 => open_stream::<u16>(&device, &config, &shared, received),
            other => Err(format!("unsupported sample format {other}")),
        }?;
        stream.play().map_err(|error| error.to_string())?;
        Ok(Self { shared, stream, events })
    }

    /// Starts the sound again: in the browser, which holds it back until the player clicks or
    /// presses a key, call this on the first click or key press.
    pub fn resume(&self) {
        if let Err(error) = self.stream.play() {
            log::warn!("sound could not start: {error}");
        }
    }

    /// What each continuous sound should be doing.
    pub fn set_beds(&self, beds: &[BedLevel; BEDS]) {
        for (shared, bed) in self.shared.beds.iter().zip(beds) {
            shared[0].store(bed.level.to_bits(), Ordering::Relaxed);
            shared[1].store(bed.pan.to_bits(), Ordering::Relaxed);
            shared[2].store(bed.busy.to_bits(), Ordering::Relaxed);
        }
    }

    pub fn set_volume(&self, volume: f32) {
        self.shared.volume.store(volume.to_bits(), Ordering::Relaxed);
    }

    pub fn play_alarm(&self) {
        self.shared.alarm_serial.fetch_add(1, Ordering::Relaxed);
    }

    pub fn play_select(&self) {
        self.shared.select_serial.fetch_add(1, Ordering::Relaxed);
    }

    /// Plays a sound once.
    pub fn play(&self, event: SoundEvent) {
        // Only fails if the audio thread has gone, when there is nothing to hear anyway.
        let _ = self.events.send(event);
    }

    /// How many frames the device has asked for, to confirm that sound is really running.
    pub fn frames_played(&self) -> u64 {
        self.shared.frames_played.load(Ordering::Relaxed)
    }
}

fn open_stream<T>(device: &cpal::Device, config: &cpal::StreamConfig, shared: &Arc<SharedTargets>, events: Receiver<SoundEvent>) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels).max(1);
    let mut synth = Synth::new(config.sample_rate as f32);
    let shared = Arc::clone(shared);
    device
        .build_output_stream(
            *config,
            move |output: &mut [T], _: &cpal::OutputCallbackInfo| {
                let targets = shared.read();
                // Without waiting: what has not arrived yet starts next time.
                while let Ok(event) = events.try_recv() {
                    synth.play(event);
                }
                for frame in output.chunks_mut(channels) {
                    let [left, right] = synth.next_frame(&targets);
                    // A single speaker gets both; more than two get the middle on the rest.
                    match frame {
                        [only] => *only = T::from_sample(0.5 * (left + right)),
                        [first, second, rest @ ..] => {
                            *first = T::from_sample(left);
                            *second = T::from_sample(right);
                            rest.fill(T::from_sample(0.5 * (left + right)));
                        }
                        [] => {}
                    }
                }
                shared.frames_played.fetch_add((output.len() / channels) as u64, Ordering::Relaxed);
            },
            |error| eprintln!("audio error: {error}"),
            None,
        )
        .map_err(|error| error.to_string())
}

/// One worker is light. More get louder and fuller, but never ten times as loud. Counted in
/// shares: a unit at half speed counts as half.
fn crowd(count: f32) -> f32 {
    count / (count + 2.0)
}

/// How close the camera is, for a sound heard in full up to `full` and gone by `silent`.
fn closeness(zoom: f64, full: f64, silent: f64) -> f32 {
    ((silent - zoom) / (silent - full)).clamp(0.0, 1.0) as f32
}

/// What the camera shows, for working out what is heard.
pub struct Listener<'a> {
    /// How far out the camera is: 0 as close as allowed, 1 all the way out.
    pub zoom: f64,
    /// Where a unit is across the screen, from -1 at the left edge to 1 at the right, if it is
    /// on screen at all.
    pub across: &'a dyn Fn(&UnitView) -> Option<f32>,
    /// The water in view.
    pub water: WaterView,
    /// Where a point is across the screen, from -1 at the left edge to 1 at the right, held to
    /// the edges, and whether it is on screen, if it is in front of the camera at all. Raw `Fp`
    /// bits, and with no height given, on the ground.
    pub point: &'a dyn Fn(i32, i32, Option<i32>) -> Option<(f32, bool)>,
}

/// How much of the ground in view is water, from 0 to 1, and where across the screen it lies.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct WaterView {
    pub share: f32,
    pub across: f32,
}

/// The water in view, from points spread over the screen: each where it is across the screen,
/// from -1 to 1, and whether the ground there is under water. Points that miss the ground, as
/// above the horizon, are left out.
pub fn water_view(points: &[(f32, bool)]) -> WaterView {
    if points.is_empty() {
        return WaterView::default();
    }
    let wet: Vec<f32> = points.iter().filter(|point| point.1).map(|point| point.0).collect();
    if wet.is_empty() {
        return WaterView::default();
    }
    WaterView { share: wet.len() as f32 / points.len() as f32, across: wet.iter().sum::<f32>() / wet.len() as f32 }
}

/// How far each unit moved between the two latest snapshots: units a tick. Kept between frames;
/// each snapshot is looked at once.
#[derive(Clone, Debug, Default)]
pub struct Movement {
    tick: Option<u32>,
    /// Where each unit was in the snapshot before, sorted by unit.
    places: Vec<(SimId, i32, i32)>,
    /// Units that moved since, and how fast, sorted by unit.
    speeds: Vec<(SimId, f32)>,
}

impl Movement {
    pub fn update(&mut self, snapshot: &SimSnapshot) {
        if self.tick == Some(snapshot.tick) {
            return;
        }
        let ticks = self.tick.map_or(1, |tick| snapshot.tick.saturating_sub(tick).max(1)) as f32;
        self.speeds.clear();
        for unit in &snapshot.units {
            if let Ok(index) = self.places.binary_search_by_key(&unit.id, |place| place.0) {
                let (_, x, y) = self.places[index];
                let moved = (f64::from(unit.x - x).hypot(f64::from(unit.y - y)) / 4096.0) as f32 / ticks;
                if moved > 0.0 {
                    self.speeds.push((unit.id, moved));
                }
            }
        }
        self.places.clear();
        self.places.extend(snapshot.units.iter().map(|unit| (unit.id, unit.x, unit.y)));
        self.places.sort_unstable_by_key(|place| place.0);
        self.speeds.sort_unstable_by_key(|speed| speed.0);
        self.tick = Some(snapshot.tick);
    }

    /// How fast a unit moved, in units a tick: 0 if it stood still or was not seen before.
    pub fn speed(&self, id: SimId) -> f32 {
        self.speeds.binary_search_by_key(&id, |speed| speed.0).map_or(0.0, |index| self.speeds[index].1)
    }
}

/// A crowd heard together: how many, counting each by its share, and where they are on average.
#[derive(Copy, Clone, Debug, Default)]
struct Crowd {
    count: f32,
    across: f32,
}

impl Crowd {
    fn add(&mut self, share: f32, across: f32) {
        self.count += share;
        self.across += share * across;
    }

    /// As a continuous sound, at `near` for how close the camera is. One unit under way is
    /// heard at half the level of a crowd, so that a lone ship can still be made out.
    fn bed(self, near: f32) -> BedLevel {
        if self.count <= 0.0 {
            return BedLevel::default();
        }
        BedLevel::new(self.count / (self.count + 1.0) * near, self.across / self.count, crowd(self.count))
    }
}

/// What each continuous sound should be doing, from what is really happening in view:
///
/// * builders while they have work links, which already stop when walking, waiting, or out of
///   resources, and factories while they are really producing;
/// * ships, land units and hovercraft, and aircraft while they move, by how fast against their
///   top speed;
/// * the sea, by how much of the view is water.
///
/// All only when zoomed in, and nothing once the match is over.
pub fn scene_levels(snapshot: &SimSnapshot, movement: &Movement, blueprints: &BlueprintDb, listener: &Listener) -> [BedLevel; BEDS] {
    let mut beds = [BedLevel::default(); BEDS];
    if snapshot.match_result.is_some() {
        return beds;
    }
    let near = closeness(listener.zoom, FULL_VOLUME_ZOOM, SILENT_ZOOM);
    let find = |id: SimId| snapshot.units.binary_search_by_key(&id, |unit| unit.id).ok().map(|index| &snapshot.units[index]);
    let across = |unit: &UnitView| (listener.across)(unit);

    let mut builders = Crowd::default();
    for link in &snapshot.work_links {
        if let Some(at) = find(link.builder).and_then(across) {
            builders.add(1.0, at);
        }
    }
    let can_spend = snapshot.economy.satisfaction_permille > 0;
    let mut factories = Crowd::default();
    for factory in snapshot.factories.iter().filter(|factory| can_spend && !factory.queue.is_empty() && snapshot.paused.binary_search(&factory.id).is_err()) {
        if let Some(at) = find(factory.id).filter(|unit| unit.built_permille == 1000).and_then(across) {
            factories.add(1.0, at);
        }
    }
    // A beam is about as loud however many build: more only make it fuller.
    beds[Bed::Build as usize] = BedLevel { level: if builders.count > 0.0 { near } else { 0.0 }, ..builders.bed(1.0) };
    beds[Bed::Factory as usize] = BedLevel { level: if factories.count > 0.0 { near } else { 0.0 }, ..factories.bed(1.0) };

    let (mut ships, mut engines, mut aircraft) = (Crowd::default(), Crowd::default(), Crowd::default());
    if near > 0.0 {
        for unit in snapshot.units.iter().filter(|unit| !unit.is_structure && !unit.remembered && unit.blueprint != u16::MAX) {
            let speed = movement.speed(unit.id);
            if speed <= 0.0 {
                continue;
            }
            let Some(at) = across(unit) else { continue };
            let blueprint = blueprints.get(BlueprintId(unit.blueprint));
            let top = blueprint.speed.to_num::<f32>();
            let share = if top > 0.0 { (speed / top).min(1.0) } else { 1.0 };
            match blueprint.move_class() {
                MoveClass::Naval | MoveClass::Submarine => ships.add(share, at),
                MoveClass::Land | MoveClass::Hover | MoveClass::Seabed => engines.add(share, at),
                MoveClass::Air => aircraft.add(share, at),
            }
        }
    }
    beds[Bed::Sailing as usize] = ships.bed(near);
    beds[Bed::Engines as usize] = engines.bed(near);
    beds[Bed::Aircraft as usize] = aircraft.bed(near);

    let water = listener.water;
    if water.share > 0.0 {
        beds[Bed::Ocean as usize] = BedLevel::new(water.share.sqrt() * closeness(listener.zoom, OCEAN_FULL_ZOOM, OCEAN_SILENT_ZOOM), water.across, 0.0);
    }
    beds
}

/// Decides when "commander under attack" plays.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct AttackAlarm {
    commander: Option<(SimId, u16)>,
    cooldown: f64,
    alert: f64,
}

impl AttackAlarm {
    /// Call every frame, with the commander and its health. Returns whether to play the alarm.
    pub fn update(&mut self, commander: Option<(SimId, u16)>, dt: f64) -> bool {
        self.cooldown = (self.cooldown - dt).max(0.0);
        self.alert = (self.alert - dt).max(0.0);
        let hurt = matches!((self.commander, commander), (Some((before, health_before)), Some((now, health))) if before == now && health < health_before);
        self.commander = commander;
        if hurt && self.cooldown <= 0.0 {
            self.cooldown = ALARM_COOLDOWN_SECONDS;
            self.alert = ALERT_SECONDS;
            return true;
        }
        false
    }

    /// How strongly to show the text alert, from 1 down to 0 as it fades away.
    pub fn alert_strength(&self) -> f32 {
        (self.alert.min(1.0)) as f32
    }
}

/// The player's choice of volume.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SoundSettings {
    steps: u8,
    pub muted: bool,
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self { steps: 6, muted: false }
    }
}

impl SoundSettings {
    pub fn louder(&mut self) {
        self.steps = (self.steps + 1).min(10);
        self.muted = false;
    }

    pub fn quieter(&mut self) {
        self.steps = self.steps.saturating_sub(1);
        self.muted = false;
    }

    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
    }

    pub fn volume(&self) -> f32 {
        if self.muted { 0.0 } else { f32::from(self.steps) / 10.0 }
    }

    /// For the resource bar.
    pub fn label(&self) -> String {
        if self.muted {
            "[N] Muted".to_string()
        } else {
            format!("[N] Sound {}%   [-] [=]", u32::from(self.steps) * 10)
        }
    }
}

#[cfg(test)]
mod tests;
