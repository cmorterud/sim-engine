//! Sound, generated in code so that there are no audio files to source. Presentation only:
//! it reads snapshots and never touches the simulation, so it cannot affect determinism.

use std::f32::consts::TAU;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

use crate::sim::components::SimId;
use crate::sim::world::{SimSnapshot, UnitView};

/// How long "ACU under attack" stays silent after playing, however often the ACU is hit.
pub const ALARM_COOLDOWN_SECONDS: f64 = 30.0;
/// How long the text alert stays on screen.
const ALERT_SECONDS: f64 = 4.0;

const BUILD_GAIN: f32 = 0.085;
const FACTORY_GAIN: f32 = 0.07;
const ALARM_GAIN: f32 = 0.4;
/// How long sounds take to fade in and out, so that they never click.
const FADE_SECONDS: f32 = 0.08;
/// Building is only heard from close up: at full volume up to the first zoom level,
/// fading to silence by the second. Zoom runs from 0, as close as allowed, to 1, all the way out.
const FULL_VOLUME_ZOOM: f64 = 0.12;
const SILENT_ZOOM: f64 = 0.3;

const SELECT_GAIN: f32 = 0.14;
const SELECT_SECONDS: f32 = 0.09;

const ALARM_NOTE_SECONDS: f32 = 0.18;
const ALARM_STEP_SECONDS: f32 = 0.22;
const ALARM_NOTES: u32 = 4;

/// What the sound should be doing right now. All levels run from 0 to 1.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct SoundTargets {
    pub build: f32,
    pub factory: f32,
    pub volume: f32,
    /// Goes up by one for each alarm to play.
    pub alarm_serial: u32,
    /// Goes up by one for each selection cue to play.
    pub select_serial: u32,
}

fn advance(phase: &mut f32, frequency: f32, rate: f32) -> f32 {
    let now = *phase;
    *phase = (now + frequency / rate).fract();
    now
}

fn sine(phase: f32) -> f32 {
    (phase * TAU).sin()
}

/// Rich in harmonics, which is what makes the factory sound harsher than the builders.
fn saw(phase: f32) -> f32 {
    phase * 2.0 - 1.0
}

/// Makes the samples. It does not know about audio devices, so that tests can check its output.
#[derive(Clone, Debug)]
pub struct Synth {
    rate: f32,
    fade: f32,
    filter: f32,
    build_level: f32,
    factory_level: f32,
    volume: f32,
    build_phases: [f32; 5],
    factory_phases: [f32; 4],
    factory_filtered: f32,
    alarm_phase: f32,
    alarm_elapsed: Option<f32>,
    alarm_serial: u32,
    select_phase: f32,
    select_elapsed: Option<f32>,
    select_serial: u32,
}

impl Synth {
    pub fn new(rate: f32) -> Self {
        Self {
            rate,
            fade: 1.0 - (-1.0 / (FADE_SECONDS * rate)).exp(),
            filter: 1.0 - (-TAU * 2_600.0 / rate).exp(),
            build_level: 0.0,
            factory_level: 0.0,
            volume: 0.0,
            build_phases: [0.0; 5],
            factory_phases: [0.0; 4],
            factory_filtered: 0.0,
            alarm_phase: 0.0,
            alarm_elapsed: None,
            alarm_serial: 0,
            select_phase: 0.0,
            select_elapsed: None,
            select_serial: 0,
        }
    }

    /// A soft hum with a slow shimmer. A second, slightly detuned voice makes a crowd sound fuller.
    fn build_voice(&mut self) -> f32 {
        let rate = self.rate;
        let [low, fifth, octave, detuned, shimmer] = &mut self.build_phases;
        let tone = 0.5 * sine(advance(low, 164.81, rate))
            + 0.3 * sine(advance(fifth, 246.94, rate))
            + 0.05 * sine(advance(octave, 329.63, rate))
            + 0.3 * self.build_level * sine(advance(detuned, 165.5, rate));
        tone * (0.92 + 0.08 * sine(advance(shimmer, 2.5, rate)))
    }

    /// A low, buzzing machine sound, pulsing as if something is being stamped out.
    fn factory_voice(&mut self) -> f32 {
        let rate = self.rate;
        let [first, second, buzz, pulse] = &mut self.factory_phases;
        let gate = if advance(pulse, 5.0, rate) < 0.5 { 1.0 } else { 0.88 };
        // A narrow pulse is full of upper harmonics, and clipping adds a little grit.
        let narrow = if advance(buzz, 246.94, rate) < 0.15 { 1.0 } else { -0.2 };
        let tone = 0.45 * saw(advance(first, 82.41, rate)) + 0.35 * saw(advance(second, 123.47, rate)) + 0.2 * narrow;
        let raw = (tone * gate * 1.1).clamp(-1.0, 1.0);
        self.factory_filtered += self.filter * (raw - self.factory_filtered);
        self.factory_filtered
    }

    /// A short, soft tick that rises a little, so that it reads as a response and not a warning.
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
        let phase = advance(&mut self.select_phase, 880.0 + 1_800.0 * elapsed, self.rate);
        let envelope = (elapsed / 0.003).min(1.0) * (-elapsed / 0.025).exp() * ((SELECT_SECONDS - elapsed) / 0.01).min(1.0);
        envelope * sine(phase)
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
        let frequency = if note.is_multiple_of(2) { 880.0 } else { 660.0 };
        let phase = advance(&mut self.alarm_phase, frequency, self.rate);
        let within = elapsed - note as f32 * ALARM_STEP_SECONDS;
        if within >= ALARM_NOTE_SECONDS {
            return 0.0;
        }
        let envelope = (within / 0.005).min((ALARM_NOTE_SECONDS - within) / 0.005).min(1.0);
        envelope * (sine(phase) + 0.25 * sine((phase * 3.0).fract()))
    }

    pub fn next_sample(&mut self, targets: SoundTargets) -> f32 {
        self.build_level += self.fade * (targets.build - self.build_level);
        self.factory_level += self.fade * (targets.factory - self.factory_level);
        self.volume += self.fade * (targets.volume - self.volume);
        let build = self.build_voice() * self.build_level * BUILD_GAIN;
        let factory = self.factory_voice() * self.factory_level * FACTORY_GAIN;
        let alarm = self.alarm_voice(targets.alarm_serial) * ALARM_GAIN;
        let select = self.select_voice(targets.select_serial) * SELECT_GAIN;
        // Soft clipping, so that everything at once cannot distort harshly.
        ((build + factory + alarm + select) * self.volume).tanh()
    }
}

/// The values shared with the audio thread. Levels are stored as `f32` bits.
#[derive(Debug, Default)]
struct SharedTargets {
    build: AtomicU32,
    factory: AtomicU32,
    volume: AtomicU32,
    alarm_serial: AtomicU32,
    select_serial: AtomicU32,
    frames_played: AtomicU64,
}

impl SharedTargets {
    fn read(&self) -> SoundTargets {
        SoundTargets {
            build: f32::from_bits(self.build.load(Ordering::Relaxed)),
            factory: f32::from_bits(self.factory.load(Ordering::Relaxed)),
            volume: f32::from_bits(self.volume.load(Ordering::Relaxed)),
            alarm_serial: self.alarm_serial.load(Ordering::Relaxed),
            select_serial: self.select_serial.load(Ordering::Relaxed),
        }
    }
}

/// Plays sound through the default output device for as long as it is kept.
pub struct SoundEngine {
    shared: Arc<SharedTargets>,
    _stream: cpal::Stream,
}

impl SoundEngine {
    /// Fails if there is no usable output device. The game carries on without sound.
    pub fn start() -> Result<Self, String> {
        let device = cpal::default_host().default_output_device().ok_or("no audio output device")?;
        let config = device.default_output_config().map_err(|error| error.to_string())?;
        let shared = Arc::new(SharedTargets::default());
        let format = config.sample_format();
        let config: cpal::StreamConfig = config.into();
        let stream = match format {
            SampleFormat::F32 => open_stream::<f32>(&device, &config, &shared),
            SampleFormat::F64 => open_stream::<f64>(&device, &config, &shared),
            SampleFormat::I16 => open_stream::<i16>(&device, &config, &shared),
            SampleFormat::I32 => open_stream::<i32>(&device, &config, &shared),
            SampleFormat::U16 => open_stream::<u16>(&device, &config, &shared),
            other => Err(format!("unsupported sample format {other}")),
        }?;
        stream.play().map_err(|error| error.to_string())?;
        Ok(Self { shared, _stream: stream })
    }

    pub fn set_levels(&self, levels: SoundLevels) {
        self.shared.build.store(levels.build.to_bits(), Ordering::Relaxed);
        self.shared.factory.store(levels.factory.to_bits(), Ordering::Relaxed);
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

    /// How many frames the device has asked for, to confirm that sound is really running.
    pub fn frames_played(&self) -> u64 {
        self.shared.frames_played.load(Ordering::Relaxed)
    }
}

fn open_stream<T>(device: &cpal::Device, config: &cpal::StreamConfig, shared: &Arc<SharedTargets>) -> Result<cpal::Stream, String>
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
                for frame in output.chunks_mut(channels) {
                    let value = T::from_sample(synth.next_sample(targets));
                    frame.fill(value);
                }
                shared.frames_played.fetch_add((output.len() / channels) as u64, Ordering::Relaxed);
            },
            |error| eprintln!("audio error: {error}"),
            None,
        )
        .map_err(|error| error.to_string())
}

/// How loud the building sounds should be.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct SoundLevels {
    pub build: f32,
    pub factory: f32,
}

/// One worker is light. More get louder and fuller, but never ten times as loud.
fn crowd_level(count: usize) -> f32 {
    count as f32 / (count as f32 + 2.0)
}

/// Works out the levels from what is really happening on screen, when zoomed in. Builders only count while
/// they have work links, which already stop when walking, waiting, or out of resources.
pub fn sound_levels(snapshot: &SimSnapshot, zoom_fraction: f64, on_screen: impl Fn(&UnitView) -> bool) -> SoundLevels {
    if snapshot.match_result.is_some() {
        return SoundLevels::default();
    }
    let find = |id: SimId| snapshot.units.binary_search_by_key(&id, |unit| unit.id).ok().map(|index| &snapshot.units[index]);
    let builders = snapshot.work_links.iter().filter(|link| find(link.builder).is_some_and(&on_screen)).count();
    let can_spend = snapshot.economy.satisfaction_permille > 0;
    let factories = snapshot
        .factories
        .iter()
        .filter(|factory| can_spend && !factory.queue.is_empty() && snapshot.paused.binary_search(&factory.id).is_err())
        .filter(|factory| find(factory.id).is_some_and(|unit| unit.built_permille == 1000 && on_screen(unit)))
        .count();
    let distance = ((SILENT_ZOOM - zoom_fraction) / (SILENT_ZOOM - FULL_VOLUME_ZOOM)).clamp(0.0, 1.0) as f32;
    SoundLevels { build: crowd_level(builders) * distance, factory: crowd_level(factories) * distance }
}

/// Decides when "ACU under attack" plays.
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
mod tests {
    use super::*;
    use crate::sim::components::Team;
    use crate::sim::world::{FactoryView, WorkLinkView};

    const RATE: f32 = 48_000.0;

    fn render(synth: &mut Synth, seconds: f32, targets: SoundTargets) -> Vec<f32> {
        (0..(seconds * RATE) as usize).map(|_| synth.next_sample(targets)).collect()
    }

    fn loudness(samples: &[f32]) -> f32 {
        (samples.iter().map(|value| value * value).sum::<f32>() / samples.len() as f32).sqrt()
    }

    /// The share of a sound's energy above about 1 kHz, which is what makes it buzz.
    fn harshness(samples: &[f32]) -> f32 {
        let keep = (-TAU * 1_000.0 / RATE).exp();
        let (mut last_in, mut last_out) = (0.0, 0.0);
        let high: Vec<f32> = samples
            .iter()
            .map(|&sample| {
                last_out = keep * (last_out + sample - last_in);
                last_in = sample;
                last_out
            })
            .collect();
        (loudness(&high) / loudness(samples)).powi(2)
    }

    fn settled(targets: SoundTargets) -> Vec<f32> {
        let mut synth = Synth::new(RATE);
        render(&mut synth, 1.0, targets);
        render(&mut synth, 2.0, targets)
    }

    #[test]
    fn silence_when_nothing_is_happening_or_when_muted() {
        let quiet = settled(SoundTargets { volume: 1.0, ..SoundTargets::default() });
        assert!(quiet.iter().all(|sample| sample.abs() < 1e-6));
        let muted = settled(SoundTargets { build: 1.0, factory: 1.0, volume: 0.0, ..SoundTargets::default() });
        assert!(muted.iter().all(|sample| sample.abs() < 1e-3));
    }

    #[test]
    fn the_factory_is_harsher_than_the_builders_and_both_stay_light() {
        let build = settled(SoundTargets { build: 1.0, volume: 1.0, ..SoundTargets::default() });
        let factory = settled(SoundTargets { factory: 1.0, volume: 1.0, ..SoundTargets::default() });
        assert!(harshness(&factory) > harshness(&build) * 1.3, "factory {} builders {}", harshness(&factory), harshness(&build));
        // Played on repeat for a whole match, so they must not nag. The first version was
        // called annoying at about 0.1 harshness, and with 0.06 loudness.
        assert!(harshness(&factory) < 0.075, "factory at {}", harshness(&factory));
        for (name, samples) in [("builders", &build), ("factory", &factory)] {
            assert!((0.015..0.045).contains(&loudness(samples)), "{name} at {}", loudness(samples));
        }
    }

    #[test]
    fn sounds_fade_in_and_out_without_clicking() {
        let mut synth = Synth::new(RATE);
        let on = SoundTargets { build: 1.0, volume: 1.0, ..SoundTargets::default() };
        let off = SoundTargets { volume: 1.0, ..SoundTargets::default() };
        let mut samples = render(&mut synth, 0.5, on);
        samples.extend(render(&mut synth, 0.5, off));
        let jump = samples.windows(2).map(|pair| (pair[1] - pair[0]).abs()).fold(0.0, f32::max);
        assert!(jump < 0.02, "a jump of {jump} would click");
    }

    #[test]
    fn the_alarm_plays_once_for_each_request_and_stands_out() {
        let mut synth = Synth::new(RATE);
        let before = SoundTargets { build: 1.0, volume: 0.6, ..SoundTargets::default() };
        render(&mut synth, 1.0, before);
        let background = loudness(&render(&mut synth, 1.0, before));
        let alarm = render(&mut synth, 0.8, SoundTargets { alarm_serial: 1, ..before });
        assert!(loudness(&alarm) > background * 2.0, "alarm {} over {background}", loudness(&alarm));
        assert!(alarm.iter().all(|sample| sample.abs() <= 1.0));
        let after = render(&mut synth, 1.0, SoundTargets { alarm_serial: 1, ..before });
        assert!(loudness(&after[RATE as usize / 4..]) < background * 1.1, "the alarm must end, and not repeat");
        let again = render(&mut synth, 0.8, SoundTargets { alarm_serial: 2, ..before });
        assert!(loudness(&again) > background * 2.0, "the next request plays again");
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0, |most, sample| most.max(sample.abs()))
    }

    #[test]
    fn the_selection_cue_is_short_soft_and_plays_for_each_request() {
        let mut synth = Synth::new(RATE);
        let quiet = SoundTargets { volume: 1.0, ..SoundTargets::default() };
        render(&mut synth, 1.0, quiet);
        let cue = render(&mut synth, 0.2, SoundTargets { select_serial: 1, ..quiet });
        let tail = (SELECT_SECONDS * RATE) as usize + 2;
        assert!(peak(&cue[..tail]) > 0.05, "it should be heard, at {}", peak(&cue[..tail]));
        assert!(peak(&cue) < ALARM_GAIN * 0.5, "it should be well under the alarm, at {}", peak(&cue));
        assert!(peak(&cue[tail..]) < 1e-6, "it should be over quickly");
        let jump = cue.windows(2).map(|pair| (pair[1] - pair[0]).abs()).fold(0.0, f32::max);
        assert!(jump < 0.05, "a jump of {jump} would click");

        let silent = render(&mut synth, 0.2, SoundTargets { select_serial: 1, ..quiet });
        assert!(peak(&silent) < 1e-6, "it must not repeat by itself");
        let again = render(&mut synth, 0.2, SoundTargets { select_serial: 2, ..quiet });
        assert!(peak(&again) > 0.05, "the next request plays again");
    }

    #[test]
    fn the_selection_cue_is_silent_when_muted_and_can_be_heard_over_building() {
        let mut synth = Synth::new(RATE);
        let muted = SoundTargets { select_serial: 1, ..SoundTargets::default() };
        assert!(peak(&render(&mut synth, 0.2, muted)) < 1e-6);

        let mut synth = Synth::new(RATE);
        let busy = SoundTargets { build: 1.0, volume: 1.0, ..SoundTargets::default() };
        render(&mut synth, 1.0, busy);
        let hum = peak(&render(&mut synth, 0.2, busy));
        let with_cue = peak(&render(&mut synth, 0.1, SoundTargets { select_serial: 1, ..busy }));
        assert!(with_cue > hum * 1.3, "{with_cue} over {hum}");
    }

    fn view(id: u32, built_permille: u16) -> UnitView {
        UnitView {
            id: SimId(id),
            team: Team::Blue,
            x: 0,
            y: 0,
            z: 0,
            radius: 4096,
            blueprint: 0,
            health_permille: 1000,
            built_permille,
            is_structure: false,
            facing: [0, 0], aim: [0, 0],
        }
    }

    fn working(ids: &[u32]) -> Vec<WorkLinkView> {
        ids.iter().map(|id| WorkLinkView { builder: SimId(*id), target: [0; 3], reclaim: false }).collect()
    }

    fn factory(id: u32, queue: Vec<u16>) -> FactoryView {
        FactoryView { id: SimId(id), queue, repeat: false, progress_permille: 0, rally: None }
    }

    fn scene() -> SimSnapshot {
        let mut snapshot = SimSnapshot { units: vec![view(1, 1000), view(2, 1000), view(3, 1000), view(4, 500)], ..SimSnapshot::default() };
        snapshot.economy.satisfaction_permille = 1000;
        snapshot
    }

    #[test]
    fn builders_are_only_heard_while_working_on_screen() {
        let mut snapshot = scene();
        assert_eq!(sound_levels(&snapshot, 0.0, |_| true), SoundLevels::default(), "nobody is working");
        snapshot.work_links = working(&[1]);
        let one = sound_levels(&snapshot, 0.0, |_| true).build;
        assert!(one > 0.0);
        assert_eq!(sound_levels(&snapshot, 0.0, |_| false).build, 0.0, "off screen");
        assert_eq!(sound_levels(&snapshot, 0.0, |unit| unit.id != SimId(1)).build, 0.0);

        // A crowd is louder, but well short of adding up.
        snapshot.work_links = working(&[1, 2, 3]);
        let three = sound_levels(&snapshot, 0.0, |_| true).build;
        assert!(three > one && three < one * 3.0 && three < 1.0, "{one} then {three}");
    }

    #[test]
    fn factories_are_only_heard_while_really_producing() {
        let mut snapshot = scene();
        snapshot.factories = vec![factory(1, vec![])];
        assert_eq!(sound_levels(&snapshot, 0.0, |_| true).factory, 0.0, "nothing queued");
        snapshot.factories = vec![factory(4, vec![7])];
        assert_eq!(sound_levels(&snapshot, 0.0, |_| true).factory, 0.0, "it is still being built");
        snapshot.factories = vec![factory(1, vec![7])];
        assert!(sound_levels(&snapshot, 0.0, |_| true).factory > 0.0);
        assert_eq!(sound_levels(&snapshot, 0.0, |_| false).factory, 0.0, "off screen");
        snapshot.economy.satisfaction_permille = 0;
        assert_eq!(sound_levels(&snapshot, 0.0, |_| true).factory, 0.0, "out of resources");
    }

    #[test]
    fn building_is_only_heard_zoomed_in_and_silent_after_the_match() {
        let mut snapshot = scene();
        snapshot.work_links = working(&[1]);
        snapshot.factories = vec![factory(2, vec![7])];
        let at = |zoom: f64| sound_levels(&snapshot, zoom, |_| true);
        let near = at(0.0);
        assert!(near.build > 0.0 && near.factory > 0.0);
        assert_eq!(at(FULL_VOLUME_ZOOM), near, "still at full volume");
        let halfway = at((FULL_VOLUME_ZOOM + SILENT_ZOOM) / 2.0);
        assert!(halfway.factory > 0.0 && halfway.factory < near.factory, "it should fade, not cut out");
        assert_eq!(at(SILENT_ZOOM), SoundLevels::default());
        assert_eq!(at(1.0), SoundLevels::default(), "silent zoomed out, whatever is in view");
        snapshot.match_result = Some(crate::sim::world::MatchResult::Draw);
        assert_eq!(sound_levels(&snapshot, 0.0, |_| true), SoundLevels::default());
    }

    #[test]
    fn the_alarm_plays_on_damage_then_waits_thirty_seconds() {
        let mut alarm = AttackAlarm::default();
        let acu = |health| Some((SimId(1), health));
        assert!(!alarm.update(acu(1000), 0.1), "the first sighting is not damage");
        assert!(!alarm.update(acu(1000), 0.1), "no damage");
        assert!(alarm.update(acu(990), 0.1), "first hit");
        assert!(alarm.alert_strength() > 0.9, "the text alert should show");

        // Constant damage for just under 30 seconds stays quiet.
        let mut health = 990;
        for _ in 0..299 {
            health -= 1;
            assert!(!alarm.update(acu(health), 0.1));
        }
        assert_eq!(alarm.alert_strength(), 0.0, "the text alert should have gone by now");
        health -= 1;
        assert!(!alarm.update(acu(health), 0.05), "only just short");
        health -= 1;
        assert!(alarm.update(acu(health), 0.1), "30 seconds later, it plays again");
    }

    #[test]
    fn damage_during_the_cooldown_does_not_count_afterwards() {
        let mut alarm = AttackAlarm::default();
        let acu = |health| Some((SimId(1), health));
        alarm.update(acu(1000), 0.1);
        assert!(alarm.update(acu(900), 0.1));
        assert!(!alarm.update(acu(800), 1.0));
        for _ in 0..40 {
            assert!(!alarm.update(acu(800), 1.0), "no new damage, so nothing to report");
        }
    }

    #[test]
    fn repairs_and_a_dead_or_replaced_commander_are_not_attacks() {
        let mut alarm = AttackAlarm::default();
        alarm.update(Some((SimId(1), 500)), 0.1);
        assert!(!alarm.update(Some((SimId(1), 600)), 0.1), "being repaired");
        assert!(!alarm.update(None, 0.1), "dead");
        assert!(!alarm.update(Some((SimId(2), 100)), 0.1), "a different unit");
    }

    #[test]
    fn volume_steps_stop_at_the_ends_and_mute_remembers_the_level() {
        let mut settings = SoundSettings::default();
        assert!((settings.volume() - 0.6).abs() < 1e-6);
        settings.toggle_mute();
        assert_eq!(settings.volume(), 0.0);
        assert_eq!(settings.label(), "[N] Muted");
        settings.toggle_mute();
        assert!((settings.volume() - 0.6).abs() < 1e-6);
        for _ in 0..20 {
            settings.louder();
        }
        assert_eq!(settings.volume(), 1.0);
        for _ in 0..20 {
            settings.quieter();
        }
        assert_eq!(settings.volume(), 0.0);
        settings.toggle_mute();
        settings.louder();
        assert!(!settings.muted, "changing the volume should unmute");
    }

    /// Needs a real audio device, so it is not run by default. Silent: the volume stays at zero.
    #[test]
    #[ignore = "needs an audio device"]
    fn a_real_device_keeps_asking_for_samples() {
        let engine = SoundEngine::start().expect("audio should start");
        engine.set_volume(0.0);
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(engine.frames_played() > 1_000, "only {} frames", engine.frames_played());
    }
}
