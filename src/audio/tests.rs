use super::*;
use crate::sim::components::{Team, WreckKind};
use crate::sim::world::{BlastView, Detonation, DetonationKind, FactoryView, ProjectileKind, ProjectileView, WorkLinkView, WreckMotion, WreckView};

const RATE: f32 = 48_000.0;

fn render(synth: &mut Synth, seconds: f32, targets: SoundTargets) -> Vec<f32> {
    (0..(seconds * RATE) as usize).map(|_| synth.next_sample(&targets)).collect()
}

fn render_stereo(synth: &mut Synth, seconds: f32, targets: SoundTargets) -> (Vec<f32>, Vec<f32>) {
    (0..(seconds * RATE) as usize).map(|_| synth.next_frame(&targets)).map(|[left, right]| (left, right)).unzip()
}

fn loudness(samples: &[f32]) -> f32 {
    (samples.iter().map(|value| value * value).sum::<f32>() / samples.len() as f32).sqrt()
}

/// The share of a sound's energy above about 1 kHz, which is what makes it buzz or hiss. The
/// filter is gentle, so a pure tone of 350 to 600 Hz still measures 0.1 to 0.26.
fn harshness(samples: &[f32]) -> f32 {
    share_above(samples, 1_000.0)
}

/// The share of a sound's energy above about `cutoff`, through a gentle one-pole filter.
fn share_above(samples: &[f32], cutoff: f32) -> f32 {
    let keep = (-TAU * cutoff / RATE).exp();
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

/// The steepest rise in loudness from one window to the next, against the sound's average: how
/// abruptly it starts or pulses.
fn abruptness(samples: &[f32], window_seconds: f32) -> f32 {
    let window = (RATE * window_seconds) as usize;
    let levels: Vec<f32> = samples.chunks(window).map(loudness).collect();
    let rise = levels.windows(2).map(|pair| pair[1] - pair[0]).fold(0.0, f32::max);
    rise / loudness(samples).max(1e-6)
}

/// How often the sound crosses zero a second: about twice its main pitch.
fn crossings(samples: &[f32]) -> f32 {
    samples.windows(2).filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0)).count() as f32 / (samples.len() as f32 / RATE)
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0, |most, sample| most.max(sample.abs()))
}

/// The biggest step from one sample to the next: a big one is heard as a click.
fn biggest_step(samples: &[f32]) -> f32 {
    samples.windows(2).map(|pair| (pair[1] - pair[0]).abs()).fold(0.0, f32::max)
}

fn quiet() -> SoundTargets {
    SoundTargets { volume: 1.0, ..SoundTargets::default() }
}

fn playing(bed: Bed, level: f32) -> SoundTargets {
    quiet().with(bed, BedLevel::new(level, 0.0, 0.5))
}

/// A sound settled in, after a second to fade in.
fn settled(targets: SoundTargets, seconds: f32) -> Vec<f32> {
    let mut synth = Synth::new(RATE);
    render(&mut synth, 1.0, targets);
    render(&mut synth, seconds, targets)
}

/// The ambience: heard under everything, never over it.
const AMBIENCE: [Bed; 4] = [Bed::Ocean, Bed::Sailing, Bed::Engines, Bed::Aircraft];

#[test]
fn silence_when_nothing_is_happening_or_when_muted() {
    let silent = settled(quiet(), 2.0);
    assert!(silent.iter().all(|sample| sample.abs() < 1e-6));
    let mut everything = SoundTargets::default();
    for bed in Bed::ALL {
        everything = everything.with(bed, BedLevel::new(1.0, 0.0, 1.0));
    }
    let muted = settled(everything, 2.0);
    assert!(muted.iter().all(|sample| sample.abs() < 1e-3), "muted, it peaks at {}", peak(&muted));
}

/// Each continuous sound, as loud as it gets, zoomed all the way in: light, and soft. The
/// ambience sits under the builders' beams, and none is near the selection cue's peak, so that
/// a click is always heard over them.
#[test]
fn every_continuous_sound_is_light_and_soft() {
    let beams = settled(playing(Bed::Build, 1.0), 8.0);
    for bed in Bed::ALL {
        let samples = settled(playing(bed, 1.0), 8.0);
        let (loud, harsh) = (loudness(&samples), harshness(&samples));
        assert!((0.008..0.025).contains(&loud), "{bed:?} at {loud}");
        assert!(peak(&samples) < SELECT_GAIN * 0.8, "{bed:?} peaks at {}", peak(&samples));
        // The rest is noise, softened above its band, and the aircraft's rush the airiest of it.
        // The beams' fizz is meant to sit above 1 kHz, so it is held instead to having little
        // above 6 kHz, where a fizz turns to a hiss.
        let (share, limit, above) = match bed {
            Bed::Build | Bed::Factory => (share_above(&samples, 6_000.0), 0.35, "6 kHz"),
            Bed::Ocean | Bed::Engines => (harsh, 0.1, "1 kHz"),
            Bed::Sailing => (harsh, 0.3, "1 kHz"),
            Bed::Aircraft => (harsh, 0.4, "1 kHz"),
        };
        assert!(share < limit, "{bed:?} has {share} of its energy above {above}, over {limit}");
        if AMBIENCE.contains(&bed) {
            assert!(loud <= loudness(&beams) * 1.4, "{bed:?} at {loud}, over the beams at {}", loudness(&beams));
        }
    }
}

/// The ambience rises and falls slowly, like the sea, rather than pulsing. The old factory sound
/// was gated five times a second until it was called jarring (2026-11-19).
#[test]
fn the_ambience_swells_gently_and_never_pulses() {
    for bed in AMBIENCE {
        let samples = settled(playing(bed, 1.0), 12.0);
        // Noise alone moves a quarter second's loudness by about a third; a sound gated on and
        // off rises by its whole level and more.
        let rise = abruptness(&samples, 0.25);
        assert!(rise < 0.45, "{bed:?} rises by {rise} of its level in a quarter second");
    }
}

/// The sea is low and the aircraft airy: each sound has a place of its own in the range, so that
/// they are told apart when heard together.
#[test]
fn each_sound_has_its_own_place_in_the_range() {
    let pitch = |bed: Bed| crossings(&settled(playing(bed, 1.0), 6.0));
    let (engines, ocean, wash, aircraft) = (pitch(Bed::Engines), pitch(Bed::Ocean), pitch(Bed::Sailing), pitch(Bed::Aircraft));
    assert!(engines < ocean && ocean < wash && wash < aircraft, "engines {engines}, sea {ocean}, wash {wash}, aircraft {aircraft}");
}

/// The builders' beam: fuller with more builders but hardly louder, and lower for factories, so
/// that the two are told apart.
#[test]
fn the_build_beam_is_fuller_with_more_builders_and_lower_for_factories() {
    let busy = |busy: f32| quiet().with(Bed::Build, BedLevel::new(1.0, 0.0, busy));
    let one = settled(busy(crowd(1.0)), 8.0);
    let ten = settled(busy(crowd(10.0)), 8.0);
    assert!(loudness(&ten) > loudness(&one) && loudness(&ten) < loudness(&one) * 1.3, "one builder at {}, ten at {}", loudness(&one), loudness(&ten));
    let factories = settled(playing(Bed::Factory, 1.0), 8.0);
    assert!(crossings(&factories) < crossings(&one) * 0.75, "factories {} a second, builders {}", crossings(&factories), crossings(&one));
}

/// The building sound has no tone in it: no frequency stands out from the rest, as one would
/// with any note in it. Chimes were tried, then a laser of pure tones stacked into a chord, which
/// was heard as chiming too (2026-11-24); the fizz chosen by ear has none.
#[test]
fn the_building_sound_has_no_tone_in_it_so_it_can_never_chime() {
    for (name, bed, band) in [("builders", Bed::Build, BUILD_BAND), ("factories", Bed::Factory, FACTORY_BAND)] {
        let samples = settled(quiet().with(bed, BedLevel::new(1.0, 0.0, 1.0)), 1.0);
        // How strongly each of 60 frequencies across the band is present.
        let strength = |frequency: f32| {
            let (mut real, mut imaginary) = (0.0_f64, 0.0_f64);
            for (index, &sample) in samples.iter().enumerate() {
                let angle = f64::from(TAU * frequency) * index as f64 / f64::from(RATE);
                real += f64::from(sample) * angle.cos();
                imaginary += f64::from(sample) * angle.sin();
            }
            real.hypot(imaginary)
        };
        let mut levels: Vec<f64> = (0..60).map(|step| strength(band.0 + (band.1 - band.0) * step as f32 / 59.0)).collect();
        levels.sort_by(f64::total_cmp);
        let (median, loudest) = (levels[30], levels[59]);
        assert!(loudest < median * 4.0, "{name}: the strongest frequency is {} times the typical one, as a tone would be", loudest / median);
    }
}

/// The beam holds steady: from one 50 ms stretch to the next, it never falls below two thirds of
/// its loudest. A laser's two voices once beat twice a second down to a quarter as loud, which
/// was heard as chiming (2026-11-24).
#[test]
fn the_build_beam_holds_steady_rather_than_swelling() {
    for (name, targets) in [("builders", playing(Bed::Build, 1.0)), ("a crowd of builders", quiet().with(Bed::Build, BedLevel::new(1.0, 0.0, 1.0))), ("factories", playing(Bed::Factory, 1.0))] {
        let samples = settled(targets, 4.0);
        let windows: Vec<f32> = samples.chunks((0.05 * RATE) as usize).map(loudness).collect();
        let (low, high) = windows.iter().fold((f32::MAX, 0.0_f32), |(low, high), &level| (low.min(level), high.max(level)));
        assert!(low > high * 0.66, "{name} falls to {} of its loudest", low / high);
    }
}

/// A beam starts and stops by fading, over a quarter second in and a third out, never switched on
/// or off. One that rose in pitch as it started sounded like a power drill (2026-11-24); the fizz
/// has no pitch to move.
#[test]
fn the_build_beam_fades_in_and_out() {
    let mut synth = Synth::new(RATE);
    let on = playing(Bed::Build, 1.0);
    let opening = render(&mut synth, 0.3, on);
    render(&mut synth, 1.0, on);
    let steady = render(&mut synth, 0.3, on);
    let first = loudness(&opening[..(0.05 * RATE) as usize]);
    assert!(first < loudness(&steady) * 0.4, "it fades in, at {first} of {} in its first 50 ms", loudness(&steady));
    let closing = render(&mut synth, 0.3, quiet());
    let fading = loudness(&closing[(0.1 * RATE) as usize..(0.2 * RATE) as usize]);
    assert!(fading > loudness(&steady) * 0.2 && fading < loudness(&steady) * 0.9, "it fades out, at {fading} of {} a tenth of a second on", loudness(&steady));
}

#[test]
fn the_beam_stops_when_the_work_stops() {
    let mut synth = Synth::new(RATE);
    render(&mut synth, 2.0, playing(Bed::Build, 1.0));
    let after = render(&mut synth, 4.0, quiet());
    assert!(peak(&after[(3.0 * RATE) as usize..]) < 1e-4, "it should be silent three seconds after the work stops");
}

/// Many sounds at once are together about as loud as one at its fullest, not six times as loud.
#[test]
fn many_sounds_together_stay_about_as_loud_as_one() {
    let loudest_alone = Bed::ALL.iter().map(|&bed| loudness(&settled(playing(bed, 1.0), 6.0))).fold(0.0, f32::max);
    let mut everything = quiet();
    for bed in Bed::ALL {
        everything = everything.with(bed, BedLevel::new(1.0, 0.0, 0.5));
    }
    let together = loudness(&settled(everything, 6.0));
    assert!(together < loudest_alone * 1.4, "all six at {together}, the loudest alone at {loudest_alone}");
}

#[test]
fn sounds_fade_in_and_out_without_clicking() {
    // A click is a step from one sample to the next bigger than the sound ever makes by itself:
    // starting and stopping must take no bigger steps than playing steadily does. A fizz, high
    // and quick, steps further than a low rumble, but no further for starting or stopping.
    for bed in Bed::ALL {
        let mut synth = Synth::new(RATE);
        let starting = render(&mut synth, 0.1, playing(bed, 1.0));
        render(&mut synth, 1.0, playing(bed, 1.0));
        let steady = render(&mut synth, 2.0, playing(bed, 1.0));
        let stopping = render(&mut synth, 0.5, quiet());
        let usual = biggest_step(&steady);
        assert!(usual < 0.04, "{bed:?} steps by {usual} as it plays");
        for (when, samples) in [("starting", &starting), ("stopping", &stopping)] {
            assert!(biggest_step(samples) <= usual * 1.1, "{bed:?} steps by {} {when}, against {usual} as it plays", biggest_step(samples));
        }
    }
}

/// Sounds come from where their source is across the screen, but never all from one side.
#[test]
fn sounds_come_from_where_they_are_on_screen_never_from_one_side_only() {
    let mut synth = Synth::new(RATE);
    let at = |pan: f32| quiet().with(Bed::Ocean, BedLevel::new(1.0, pan, 0.0));
    render_stereo(&mut synth, 1.0, at(-1.0));
    let (left, right) = render_stereo(&mut synth, 3.0, at(-1.0));
    let (left, right) = (loudness(&left), loudness(&right));
    assert!(left > right * 1.8, "from the left edge: left {left}, right {right}");
    assert!(right > left * 0.2, "never all from one side: left {left}, right {right}");

    let mut synth = Synth::new(RATE);
    render_stereo(&mut synth, 1.0, at(0.0));
    let (left, right) = render_stereo(&mut synth, 3.0, at(0.0));
    assert!((loudness(&left) - loudness(&right)).abs() < 1e-6, "from the middle, both alike");
}

#[test]
fn the_alarm_plays_once_for_each_request_and_stands_out() {
    let mut synth = Synth::new(RATE);
    let mut before = quiet().with(Bed::Build, BedLevel::new(1.0, 0.0, 1.0)).with(Bed::Ocean, BedLevel::new(1.0, 0.0, 0.0));
    before.volume = 0.6;
    render(&mut synth, 1.0, before);
    let background = loudness(&render(&mut synth, 1.0, before));
    let alarm = render(&mut synth, 0.8, SoundTargets { alarm_serial: 1, ..before });
    assert!(loudness(&alarm) > background * 2.0, "alarm {} over {background}", loudness(&alarm));
    assert!(alarm.iter().all(|sample| sample.abs() <= 1.0));
    // Struck like a chime, not switched on: each note takes over 10 ms to sound fully.
    let mut synth = Synth::new(RATE);
    let alone = render(&mut synth, 1.0, SoundTargets { alarm_serial: 1, ..quiet() });
    let start = alone.iter().position(|sample| sample.abs() > 1e-4).expect("it plays");
    let first_ms = peak(&alone[start..start + (RATE * 0.002) as usize]);
    let tenth = peak(&alone[start..start + (RATE * 0.1) as usize]);
    assert!(first_ms < tenth * 0.5, "it starts at {first_ms} of {tenth} within 2 ms");
    let after = render(&mut synth, 1.0, SoundTargets { alarm_serial: 1, ..quiet() });
    assert!(peak(&after) < 1e-6, "the alarm must end, and not repeat");
    let again = render(&mut synth, 0.8, SoundTargets { alarm_serial: 2, ..quiet() });
    assert!(peak(&again) > 0.2, "the next request plays again");
}

#[test]
fn the_selection_cue_is_short_soft_and_plays_for_each_request() {
    let mut synth = Synth::new(RATE);
    render(&mut synth, 1.0, quiet());
    let cue = render(&mut synth, 0.2, SoundTargets { select_serial: 1, ..quiet() });
    let tail = (SELECT_SECONDS * RATE) as usize + 2;
    assert!(peak(&cue[..tail]) > 0.05, "it should be heard, at {}", peak(&cue[..tail]));
    assert!(peak(&cue) < ALARM_GAIN * 0.5, "it should be well under the alarm, at {}", peak(&cue));
    assert!(peak(&cue[tail..]) < 1e-6, "it should be over quickly");
    // It swept up past 2 kHz until 2026-11-19, at 0.39 harshness.
    assert!(harshness(&cue[..tail]) < 0.3, "it should be soft, at {}", harshness(&cue[..tail]));
    assert!(biggest_step(&cue) < 0.05, "a jump of {} would click", biggest_step(&cue));

    let silent = render(&mut synth, 0.2, SoundTargets { select_serial: 1, ..quiet() });
    assert!(peak(&silent) < 1e-6, "it must not repeat by itself");
    let again = render(&mut synth, 0.2, SoundTargets { select_serial: 2, ..quiet() });
    assert!(peak(&again) > 0.05, "the next request plays again");
}

#[test]
fn the_selection_cue_is_silent_when_muted_and_can_be_heard_over_everything_else() {
    let mut synth = Synth::new(RATE);
    let muted = SoundTargets { select_serial: 1, ..SoundTargets::default() };
    assert!(peak(&render(&mut synth, 0.2, muted)) < 1e-6);

    let mut busy = quiet();
    for bed in Bed::ALL {
        busy = busy.with(bed, BedLevel::new(1.0, 0.0, 1.0));
    }
    // The same scene twice, once with the cue: the difference is the cue alone, heard against
    // everything else over the cue's own tenth of a second.
    let (mut without, mut with) = (Synth::new(RATE), Synth::new(RATE));
    render(&mut without, 1.0, busy);
    render(&mut with, 1.0, busy);
    let background = render(&mut without, SELECT_SECONDS, busy);
    let mixed = render(&mut with, SELECT_SECONDS, SoundTargets { select_serial: 1, ..busy });
    let cue: Vec<f32> = mixed.iter().zip(&background).map(|(mixed, background)| mixed - background).collect();
    assert!(loudness(&cue) > loudness(&background), "the cue at {}, everything else at {}", loudness(&cue), loudness(&background));
}

fn view(id: u32, blueprint: u16, x: i32, built_permille: u16) -> UnitView {
    UnitView {
        id: SimId(id),
        team: Team::Blue,
        x,
        y: 0,
        z: 0,
        radius: 4096,
        blueprint,
        health_permille: 1000,
        built_permille,
        is_structure: false,
        facing: [0, 0],
        aim: [0, 0],
        extra_aims: [[0; 2]; 3], pitches: [0; 4],
        working: false,
        assembling: None,
        upgrade: None,
        remembered: false,
        ally: false,
        owner: None,
    }
}

fn working(ids: &[u32]) -> Vec<WorkLinkView> {
    ids.iter().map(|id| WorkLinkView { builder: SimId(*id), target: [0; 3], reclaim: false }).collect()
}

fn factory(id: u32, queue: Vec<u16>) -> FactoryView {
    FactoryView { id: SimId(id), queue, repeat: false, progress_permille: 0, rally: None }
}

/// Four units, the last still being built: engineers, standing still.
fn scene(db: &BlueprintDb) -> SimSnapshot {
    let engineer = db.id("t1_engineer").0;
    let mut snapshot = SimSnapshot { units: vec![view(1, engineer, 0, 1000), view(2, engineer, 0, 1000), view(3, engineer, 0, 1000), view(4, engineer, 0, 500)], ..SimSnapshot::default() };
    snapshot.economy.satisfaction_permille = 1000;
    snapshot
}

/// Hears a scene, everything on screen at the middle, unless `across` says otherwise.
fn hear(snapshot: &SimSnapshot, movement: &Movement, db: &BlueprintDb, zoom: f64, water: WaterView, across: &dyn Fn(&UnitView) -> Option<f32>) -> [BedLevel; BEDS] {
    scene_levels(snapshot, movement, db, &Listener { zoom, across, water, point: &|_, _, _| Some((0.0, true)) })
}

fn everywhere(_: &UnitView) -> Option<f32> {
    Some(0.0)
}

#[test]
fn builders_are_only_heard_while_working_on_screen_and_a_crowd_only_fills_out_the_beam() {
    let db = BlueprintDb::standard();
    let still = Movement::default();
    let mut snapshot = scene(&db);
    let build = |snapshot: &SimSnapshot, across: &dyn Fn(&UnitView) -> Option<f32>| hear(snapshot, &still, &db, 0.0, WaterView::default(), across)[Bed::Build as usize];
    assert_eq!(build(&snapshot, &everywhere), BedLevel::default(), "nobody is working");
    snapshot.work_links = working(&[1]);
    let one = build(&snapshot, &everywhere);
    assert!(one.level > 0.0);
    assert_eq!(build(&snapshot, &|_| None).level, 0.0, "off screen");
    assert_eq!(build(&snapshot, &|unit| (unit.id != SimId(1)).then_some(0.0)).level, 0.0);

    snapshot.work_links = working(&[1, 2, 3]);
    let three = build(&snapshot, &everywhere);
    assert_eq!(three.level, one.level, "the beam is as loud however many build: a crowd only fills it out");
    assert!(three.busy > one.busy && three.busy < 1.0, "{} then {}", one.busy, three.busy);
    // Where the builders are across the screen.
    let left = build(&snapshot, &|_| Some(-0.8));
    assert!((left.pan + 0.8).abs() < 1e-6, "{}", left.pan);
}

#[test]
fn factories_are_only_heard_while_really_producing() {
    let db = BlueprintDb::standard();
    let still = Movement::default();
    let mut snapshot = scene(&db);
    let factory_level = |snapshot: &SimSnapshot, across: &dyn Fn(&UnitView) -> Option<f32>| hear(snapshot, &still, &db, 0.0, WaterView::default(), across)[Bed::Factory as usize].level;
    snapshot.factories = vec![factory(1, vec![])];
    assert_eq!(factory_level(&snapshot, &everywhere), 0.0, "nothing queued");
    snapshot.factories = vec![factory(4, vec![7])];
    assert_eq!(factory_level(&snapshot, &everywhere), 0.0, "it is still being built");
    snapshot.factories = vec![factory(1, vec![7])];
    assert!(factory_level(&snapshot, &everywhere) > 0.0);
    assert_eq!(factory_level(&snapshot, &|_| None), 0.0, "off screen");
    snapshot.paused = vec![SimId(1)];
    assert_eq!(factory_level(&snapshot, &everywhere), 0.0, "paused");
    snapshot.paused.clear();
    snapshot.economy.satisfaction_permille = 0;
    assert_eq!(factory_level(&snapshot, &everywhere), 0.0, "out of resources");
}

/// Two snapshots a tick apart, each unit moved by `step` units between them.
fn moving(db: &BlueprintDb, keys: &[&str], step: i32) -> (SimSnapshot, Movement) {
    let units = |offset: i32| keys.iter().enumerate().map(|(index, key)| view(index as u32 + 1, db.id(key).0, offset, 1000)).collect::<Vec<_>>();
    let mut movement = Movement::default();
    movement.update(&SimSnapshot { tick: 1, units: units(0), ..SimSnapshot::default() });
    let now = SimSnapshot { tick: 2, units: units(step * 4096), ..SimSnapshot::default() };
    movement.update(&now);
    (now, movement)
}

/// Ships, land units, and aircraft are each heard only while they move on screen, each as their
/// own sound, and louder at speed than creeping.
#[test]
fn ships_engines_and_aircraft_are_heard_only_while_moving_on_screen() {
    let db = BlueprintDb::standard();
    let quiet_sea = WaterView::default();
    for (key, bed) in [("t1_frigate", Bed::Sailing), ("t1_engineer", Bed::Engines), ("t1_interceptor", Bed::Aircraft)] {
        let top = db.get(db.id(key)).speed.to_num::<f32>();
        let (snapshot, movement) = moving(&db, &[key], top.ceil() as i32);
        let beds = hear(&snapshot, &movement, &db, 0.0, quiet_sea, &everywhere);
        assert!(beds[bed as usize].level > 0.0, "{key} moving");
        for other in [Bed::Sailing, Bed::Engines, Bed::Aircraft].into_iter().filter(|other| *other != bed) {
            assert_eq!(beds[other as usize].level, 0.0, "{key} is not heard as {other:?}");
        }
        assert_eq!(hear(&snapshot, &movement, &db, 0.0, quiet_sea, &|_| None)[bed as usize].level, 0.0, "{key} off screen");
        assert_eq!(hear(&snapshot, &movement, &db, SILENT_ZOOM, quiet_sea, &everywhere)[bed as usize].level, 0.0, "{key} zoomed out");
        let (still_snapshot, still) = moving(&db, &[key], 0);
        assert_eq!(hear(&still_snapshot, &still, &db, 0.0, quiet_sea, &everywhere)[bed as usize].level, 0.0, "{key} standing still");
        let creeping = (top / 4.0 * 4096.0) as i32;
        let (slow_snapshot, slow) = moving(&db, &[key], 0);
        let mut slow = slow;
        slow.update(&SimSnapshot { tick: 3, units: vec![view(1, db.id(key).0, creeping, 1000)], ..SimSnapshot::default() });
        let slow_level = hear(&slow_snapshot, &slow, &db, 0.0, quiet_sea, &everywhere)[bed as usize].level;
        assert!(slow_level < beds[bed as usize].level, "{key} creeping at {slow_level}, at speed {}", beds[bed as usize].level);
    }
    // A fleet is louder than one ship, but nowhere near as many times.
    let full_speed = db.get(db.id("t1_frigate")).speed.to_num::<f32>().ceil() as i32;
    let (one, one_moving) = moving(&db, &["t1_frigate"], full_speed);
    let (four, four_moving) = moving(&db, &["t1_frigate"; 4], full_speed);
    let (lone, fleet) = (hear(&one, &one_moving, &db, 0.0, quiet_sea, &everywhere)[Bed::Sailing as usize].level, hear(&four, &four_moving, &db, 0.0, quiet_sea, &everywhere)[Bed::Sailing as usize].level);
    assert!(fleet > lone && fleet < lone * 2.0, "one ship {lone}, four {fleet}");
}

#[test]
fn the_sea_is_heard_only_with_water_in_view_and_zoomed_in() {
    let db = BlueprintDb::standard();
    let snapshot = scene(&db);
    let still = Movement::default();
    let ocean = |zoom: f64, water: WaterView| hear(&snapshot, &still, &db, zoom, water, &everywhere)[Bed::Ocean as usize];
    assert_eq!(ocean(0.0, WaterView::default()), BedLevel::default(), "no water in view");
    let coast = WaterView { share: 0.25, across: 0.5 };
    let open_sea = WaterView { share: 1.0, across: 0.0 };
    assert!(ocean(0.0, coast).level > 0.0 && ocean(0.0, coast).level < ocean(0.0, open_sea).level, "more water, more sea");
    assert!((ocean(0.0, coast).pan - 0.5).abs() < 1e-6, "from where the water is");
    assert_eq!(ocean(OCEAN_FULL_ZOOM, open_sea), ocean(0.0, open_sea), "still in full");
    let halfway = ocean((OCEAN_FULL_ZOOM + OCEAN_SILENT_ZOOM) / 2.0, open_sea).level;
    assert!(halfway > 0.0 && halfway < 1.0, "it fades, at {halfway}");
    assert_eq!(ocean(OCEAN_SILENT_ZOOM, open_sea).level, 0.0, "zoomed out");
}

#[test]
fn the_water_in_view_comes_from_points_over_the_screen() {
    assert_eq!(water_view(&[]), WaterView::default());
    assert_eq!(water_view(&[(-0.5, false), (0.5, false)]), WaterView::default());
    let view = water_view(&[(-0.5, false), (0.25, true), (0.75, true), (-0.75, false)]);
    assert!((view.share - 0.5).abs() < 1e-6 && (view.across - 0.5).abs() < 1e-6, "{view:?}");
}

#[test]
fn movement_is_measured_once_a_snapshot_and_only_for_units_seen_before() {
    let db = BlueprintDb::standard();
    let tank = db.id("t1_engineer").0;
    let mut movement = Movement::default();
    movement.update(&SimSnapshot { tick: 1, units: vec![view(1, tank, 0, 1000)], ..SimSnapshot::default() });
    assert_eq!(movement.speed(SimId(1)), 0.0, "seen once, not yet moving");
    let next = SimSnapshot { tick: 3, units: vec![view(1, tank, 6 * 4096, 1000), view(2, tank, 0, 1000)], ..SimSnapshot::default() };
    movement.update(&next);
    assert!((movement.speed(SimId(1)) - 3.0).abs() < 1e-4, "six units over two ticks: {}", movement.speed(SimId(1)));
    assert_eq!(movement.speed(SimId(2)), 0.0, "new in sight");
    movement.update(&next);
    assert!((movement.speed(SimId(1)) - 3.0).abs() < 1e-4, "the same snapshot is looked at once");
}

#[test]
fn nothing_is_heard_once_the_match_is_over() {
    let db = BlueprintDb::standard();
    let (mut snapshot, movement) = moving(&db, &["t1_frigate", "t1_engineer"], 3);
    snapshot.work_links = working(&[2]);
    snapshot.match_result = Some(crate::sim::world::MatchResult::Draw);
    let beds = hear(&snapshot, &movement, &db, 0.0, WaterView { share: 1.0, across: 0.0 }, &everywhere);
    assert_eq!(beds, [BedLevel::default(); BEDS]);
}

#[test]
fn the_alarm_plays_on_damage_then_waits_thirty_seconds() {
    let mut alarm = AttackAlarm::default();
    let rig = |health| Some((SimId(1), health));
    assert!(!alarm.update(rig(1000), 0.1), "the first sighting is not damage");
    assert!(!alarm.update(rig(1000), 0.1), "no damage");
    assert!(alarm.update(rig(990), 0.1), "first hit");
    assert!(alarm.alert_strength() > 0.9, "the text alert should show");

    // Constant damage for just under 30 seconds stays quiet.
    let mut health = 990;
    for _ in 0..299 {
        health -= 1;
        assert!(!alarm.update(rig(health), 0.1));
    }
    assert_eq!(alarm.alert_strength(), 0.0, "the text alert should have gone by now");
    health -= 1;
    assert!(!alarm.update(rig(health), 0.05), "only just short");
    health -= 1;
    assert!(alarm.update(rig(health), 0.1), "30 seconds later, it plays again");
}

#[test]
fn damage_during_the_cooldown_does_not_count_afterwards() {
    let mut alarm = AttackAlarm::default();
    let rig = |health| Some((SimId(1), health));
    alarm.update(rig(1000), 0.1);
    assert!(alarm.update(rig(900), 0.1));
    assert!(!alarm.update(rig(800), 1.0));
    for _ in 0..40 {
        assert!(!alarm.update(rig(800), 1.0), "no new damage, so nothing to report");
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

/// A sound played once on its own, as loud as it gets, and a little after.
fn once(sound: OneShot) -> Vec<f32> {
    let mut synth = Synth::new(RATE);
    render(&mut synth, 0.2, quiet());
    synth.play(SoundEvent { sound, gain: 1.0, pan: 0.0 });
    render(&mut synth, sound.seconds() + 0.5, quiet())
}

const FIGHTING: [OneShot; 8] = [OneShot::Gun(0.1), OneShot::Gun(0.9), OneShot::Laser, OneShot::Missile, OneShot::Impact(0.1), OneShot::Impact(0.9), OneShot::Destroyed(0.2), OneShot::Destroyed(0.9)];

/// Every sound that plays once is struck softly, never switched on, so that none clicks in: its
/// first 2 ms are under half its peak. And it ends when it should, and stays ended.
#[test]
fn every_one_shot_sound_is_struck_softly_and_ends() {
    for sound in FIGHTING.into_iter().chain([OneShot::CommanderExplosion, OneShot::Nuke, OneShot::LaunchWarning]) {
        let samples = once(sound);
        let top = peak(&samples);
        let start = samples.iter().position(|sample| sample.abs() > 1e-4).expect("it plays");
        let opening = peak(&samples[start..start + (0.002 * RATE) as usize]);
        assert!(opening < top * 0.5, "{sound:?} starts at {} of its peak within 2 ms", opening / top);
        assert!(samples.iter().all(|sample| sample.abs() <= 1.0), "{sound:?} goes past full");
        let ended = (start as f32 / RATE + sound.seconds()) * RATE;
        assert!(peak(&samples[ended as usize..]) < 1e-4, "{sound:?} goes on past its {} s", sound.seconds());
    }
}

/// The fighting is light: soft thumps, puffs, and dull bursts, no louder than the selection cue,
/// with little energy above 1 kHz, but for the laser's soft "fsss" and the missile's whoosh.
#[test]
fn the_fighting_is_light_and_soft() {
    for sound in FIGHTING {
        let samples = once(sound);
        let (top, harsh) = (peak(&samples), harshness(&samples));
        let (most, harshest) = match sound {
            OneShot::Gun(_) | OneShot::Laser | OneShot::Missile => (SELECT_GAIN, 0.4),
            _ => (SELECT_GAIN * 1.3, 0.15),
        };
        assert!(top < most, "{sound:?} peaks at {top}, over {most}");
        assert!(harsh < harshest, "{sound:?} is {harsh} harsh, over {harshest}");
    }
}

/// A heavier gun, a bigger blast, and a bigger unit destroyed sound lower, last longer, and are a
/// little louder, so that a battle's weight is heard.
#[test]
fn heavier_guns_and_bigger_blasts_are_lower_longer_and_louder() {
    for (light, heavy) in [(OneShot::Gun(0.1), OneShot::Gun(0.9)), (OneShot::Impact(0.1), OneShot::Impact(0.9)), (OneShot::Destroyed(0.2), OneShot::Destroyed(0.9))] {
        let (small, big) = (once(light), once(heavy));
        assert!(heavy.seconds() > light.seconds(), "{heavy:?} lasts longer than {light:?}");
        assert!(peak(&big) > peak(&small), "{heavy:?} at {}, {light:?} at {}", peak(&big), peak(&small));
        let opening = |samples: &[f32]| crossings(&samples[(0.2 * RATE) as usize..(0.35 * RATE) as usize]);
        assert!(opening(&big) < opening(&small), "{heavy:?} at {} crossings a second, {light:?} at {}", opening(&big), opening(&small));
    }
}

/// A commander blowing up and a strategic missile striking are the loudest sounds there are, but
/// still never past full, and they dip everything else while they last, then let it back.
#[test]
fn the_great_explosions_are_the_loudest_never_clip_and_dip_the_rest() {
    let loudest_fighting = FIGHTING.iter().map(|&sound| peak(&once(sound))).fold(0.0, f32::max);
    for (sound, dipped_to, back_by) in [(OneShot::CommanderExplosion, 0.5, 5.5), (OneShot::Nuke, 0.35, 9.5)] {
        assert!(peak(&once(sound)) > loudest_fighting * 3.0, "{sound:?} is the loudest");
        let mut synth = Synth::new(RATE);
        let sea = quiet().with(Bed::Ocean, BedLevel::new(1.0, 0.0, 0.0));
        render(&mut synth, 1.0, sea);
        synth.play(SoundEvent { sound, gain: 1.0, pan: 0.0 });
        render(&mut synth, 1.0, sea);
        assert!(synth.dip < dipped_to, "{sound:?} dips the rest to {}", synth.dip);
        render(&mut synth, back_by - 1.0, sea);
        assert!(synth.dip > 0.9, "{sound:?} lets the rest back, at {} after {back_by} s", synth.dip);
    }
    // The nuke's rumble outlasts its roar: still heard eight seconds on.
    let nuke = once(OneShot::Nuke);
    let late = loudness(&nuke[(8.2 * RATE) as usize..(8.7 * RATE) as usize]);
    assert!(late > loudness(&nuke) * 0.05, "the rumble is gone by eight seconds, at {late}");
}

/// The launch warning: two low notes, three times, soft and warm, and well under the alarm.
#[test]
fn the_launch_warning_is_low_soft_and_plays_once() {
    let warning = once(OneShot::LaunchWarning);
    let start = warning.iter().position(|sample| sample.abs() > 1e-4).expect("it plays");
    let note = &warning[start + (0.05 * RATE) as usize..start + (0.3 * RATE) as usize];
    assert!(crossings(note) < 2.0 * 260.0, "it is low, at {} crossings a second", crossings(note));
    let mut synth = Synth::new(RATE);
    let alarm = render(&mut synth, 1.0, SoundTargets { alarm_serial: 1, ..quiet() });
    assert!(peak(&warning) < peak(&alarm) * 0.6, "the warning at {}, the alarm at {}", peak(&warning), peak(&alarm));
}

/// With every voice busy, the oldest small sound gives way to a new one; a great explosion never
/// does, and however much fires at once, the sound never goes past full.
#[test]
fn a_full_set_of_voices_drops_the_oldest_small_sound_never_a_great_one() {
    let mut synth = Synth::new(RATE);
    synth.play(SoundEvent { sound: OneShot::Nuke, gain: 1.0, pan: 0.0 });
    let mut samples = Vec::new();
    for index in 0..60 {
        synth.play(SoundEvent { sound: OneShot::Impact(0.9), gain: 1.0, pan: (index % 3) as f32 - 1.0 });
        samples.extend(render(&mut synth, 0.01, quiet()));
        assert!(synth.playing() <= MOST_VOICES);
    }
    assert!(synth.voices.iter().any(|voice| voice.event.sound == OneShot::Nuke), "the nuke plays on");
    assert!(samples.iter().all(|sample| sample.abs() <= 1.0));
}

#[test]
fn a_sound_played_once_comes_from_its_side_of_the_screen() {
    let mut synth = Synth::new(RATE);
    synth.play(SoundEvent { sound: OneShot::Impact(0.5), gain: 1.0, pan: -1.0 });
    let (left, right) = render_stereo(&mut synth, 0.6, quiet());
    assert!(loudness(&left) > loudness(&right) * 1.8, "left {}, right {}", loudness(&left), loudness(&right));
}

fn shell(id: u32, source: u32, x: i32, from: Option<[i32; 3]>, kind: ProjectileKind) -> ProjectileView {
    ProjectileView { id: SimId(id), team: Team::Red, x, y: 0, z: 0, kind, source: SimId(source), from, splash: 0 }
}

/// Hears the events between two snapshots, with what is on screen and how far zoomed out.
fn events_between(previous: &SimSnapshot, current: &SimSnapshot, zoom: f64, on_screen: bool) -> Vec<SoundEvent> {
    let db = BlueprintDb::standard();
    let point = move |x: i32, _: i32, _: Option<i32>| Some(((x as f32 / 4096.0 / 100.0).clamp(-1.0, 1.0), on_screen));
    scene_events(previous, current, &db, &Listener { zoom, across: &everywhere, water: WaterView::default(), point: &point })
}

fn ticks() -> (SimSnapshot, SimSnapshot) {
    (SimSnapshot { tick: 1, ..SimSnapshot::default() }, SimSnapshot { tick: 2, ..SimSnapshot::default() })
}

/// A gun fired, a missile launched, a laser striking, and a shell striking are heard when on
/// screen and zoomed in, and not otherwise; each once, for the snapshot that first shows it.
#[test]
fn the_fighting_is_heard_on_screen_and_zoomed_in_once_each() {
    let (previous, mut current) = ticks();
    current.projectiles = vec![shell(10, 1, 0, Some([0, 0, 0]), ProjectileKind::Shell), shell(11, 1, 0, Some([0, 0, 0]), ProjectileKind::TacticalMissile)];
    current.landed = vec![shell(12, 1, 0, Some([0, 0, 0]), ProjectileKind::Beam), ProjectileView { splash: 100 * 4096, ..shell(13, 1, 0, None, ProjectileKind::Shell) }];
    let heard: Vec<OneShot> = events_between(&previous, &current, 0.0, true).iter().map(|event| event.sound).collect();
    assert!(matches!(heard.as_slice(), [OneShot::Gun(_), OneShot::Laser, OneShot::Missile, OneShot::Impact(_)]), "{heard:?}");
    assert!(events_between(&previous, &current, 0.0, false).is_empty(), "off screen");
    assert!(events_between(&previous, &current, 0.5, true).is_empty(), "zoomed out");
    assert!(events_between(&current, &current, 0.0, true).is_empty(), "the same snapshot is heard once");
    // A shell already in flight is not fired again.
    let mut later = current.clone();
    later.tick = 3;
    later.landed.clear();
    assert!(events_between(&current, &later, 0.0, true).is_empty(), "{:?}", events_between(&current, &later, 0.0, true));
}

/// A unit gone since the snapshot before is heard destroyed only if it left debris: one that
/// only went out of sight leaves none.
#[test]
fn a_unit_is_heard_destroyed_only_if_it_left_debris() {
    let db = BlueprintDb::standard();
    let tank = db.id("t1_engineer").0;
    let (mut previous, mut current) = ticks();
    previous.units = vec![view(5, tank, 0, 1000)];
    assert!(events_between(&previous, &current, 0.0, true).is_empty(), "out of sight is not destroyed");
    current.wrecks = vec![WreckView { id: SimId(90), x: 0, y: 0, z: 0, radius: 4096, mass: 10, energy: 0, kind: WreckKind::Debris, motion: WreckMotion::Still }];
    let heard = events_between(&previous, &current, 0.0, true);
    assert!(matches!(heard.as_slice(), [SoundEvent { sound: OneShot::Destroyed(_), .. }]), "{heard:?}");
    // A blast as it dies is heard too.
    let (previous, mut current) = ticks();
    current.blasts = vec![BlastView { team: Team::Red, x: 0, y: 0, radius: 200 * 4096 }];
    assert!(matches!(events_between(&previous, &current, 0.0, true).as_slice(), [SoundEvent { sound: OneShot::Destroyed(_), .. }]));
}

/// A big battle is heard as a rumble of fire, not a barrage: only a few of each kind start for a
/// snapshot, the rest heard as those few a little louder.
#[test]
fn a_big_battle_starts_only_a_few_of_each_sound_at_once() {
    let (previous, mut current) = ticks();
    current.projectiles = (0..30).map(|index| shell(100 + index, 1, index as i32 * 4096, Some([index as i32 * 4096, 0, 0]), ProjectileKind::Shell)).collect();
    let heard = events_between(&previous, &current, 0.0, true);
    assert!(heard.len() <= 4, "{} guns start at once", heard.len());
    assert!(heard.iter().all(|event| event.gain > 1.0 - 1e-6 || event.gain > 0.99), "the few stand for the rest, at {:?}", heard.iter().map(|event| event.gain).collect::<Vec<_>>());
}

/// A commander blowing up and a strategic missile striking are heard wherever the camera is,
/// softer off screen, and a strategic launch anywhere sounds the warning, even zoomed all the way
/// out with nothing in view.
#[test]
fn the_great_explosions_and_the_launch_warning_are_heard_everywhere() {
    let (previous, mut current) = ticks();
    current.detonations = vec![Detonation { kind: DetonationKind::Commander, x: 0, y: 0 }, Detonation { kind: DetonationKind::StrategicMissile, x: 0, y: 0 }];
    current.strategic_launches = vec![Team::Red];
    // Zoomed all the way out, and off screen.
    let heard = events_between(&previous, &current, 1.0, false);
    let gain = |sound: OneShot| heard.iter().find(|event| event.sound == sound).map(|event| event.gain);
    assert!(gain(OneShot::CommanderExplosion).is_some_and(|gain| gain > 0.2 && gain < 0.6), "{heard:?}");
    assert!(gain(OneShot::Nuke).is_some_and(|gain| gain > 0.2 && gain < 0.6), "{heard:?}");
    assert_eq!(gain(OneShot::LaunchWarning), Some(1.0));
    let in_view = events_between(&previous, &current, 1.0, true);
    assert!(in_view.iter().filter(|event| event.sound.is_great()).all(|event| event.gain == 1.0), "in full on screen: {in_view:?}");
}
