//! What is heard once, from what the latest snapshot shows has happened since the one before:
//! guns fired, lasers and shells striking, units destroyed, and anywhere on the map, a commander
//! blowing up, a strategic missile striking, or one being launched. Call once for each snapshot.

use super::{closeness, Listener, OneShot, SoundEvent};
use crate::sim::blueprints::{BlueprintDb, BlueprintId};
use crate::sim::components::{SimId, WreckKind};
use crate::sim::world::{DetonationKind, ProjectileKind, SimSnapshot, UnitView};

/// The fighting carries a little further out than work and engines: in full up to the first of
/// these zooms, gone by the second.
const COMBAT_FULL_ZOOM: f64 = 0.15;
const COMBAT_SILENT_ZOOM: f64 = 0.45;
/// How loud a commander's explosion, and a strategic missile's, are off screen, against on it.
const COMMANDER_OFF_SCREEN: f32 = 0.33;
const NUKE_OFF_SCREEN: f32 = 0.4;
/// The most of each kind of sound started for one snapshot: a battle is heard as a rumble of
/// fire, not a barrage. The nearest and heaviest are kept, a little louder for the rest.
const MOST_GUNS: usize = 4;
const MOST_LASERS: usize = 2;
const MOST_MISSILES: usize = 2;
const MOST_IMPACTS: usize = 3;
const MOST_DESTROYED: usize = 3;

fn units_of(raw: i32) -> f32 {
    raw as f32 / 4096.0
}

/// Sounds of one kind for one snapshot: the heaviest kept, then the nearest the middle of the
/// screen, a little louder for those left out.
fn keep(mut events: Vec<SoundEvent>, most: usize, size: impl Fn(&SoundEvent) -> f32) -> Vec<SoundEvent> {
    let count = events.len();
    if count <= most {
        return events;
    }
    events.sort_by(|a, b| size(b).total_cmp(&size(a)).then(a.pan.abs().total_cmp(&b.pan.abs())));
    events.truncate(most);
    let fuller = (count as f32 / most as f32).powf(0.3).min(1.6);
    for event in &mut events {
        event.gain = (event.gain * fuller).min(1.0);
    }
    events
}

fn size_of(event: &SoundEvent) -> f32 {
    match event.sound {
        OneShot::Gun(size) | OneShot::Impact(size) | OneShot::Destroyed(size) => size,
        _ => 0.5,
    }
}

/// A unit's size for its sound, from 0 to 1: by tier, and by how big it is.
fn unit_size(unit: &UnitView, blueprints: &BlueprintDb) -> f32 {
    let tier = if unit.blueprint == u16::MAX { 1 } else { blueprints.get(BlueprintId(unit.blueprint)).tier };
    (0.15 + 0.2 * f32::from(tier.saturating_sub(1)) + units_of(unit.radius) / 60.0).clamp(0.0, 1.0)
}

/// Everything heard once since `previous`, for `current`.
pub fn scene_events(previous: &SimSnapshot, current: &SimSnapshot, blueprints: &BlueprintDb, listener: &Listener) -> Vec<SoundEvent> {
    let mut events = Vec::new();
    if current.tick == previous.tick {
        return events;
    }
    let near = closeness(listener.zoom, COMBAT_FULL_ZOOM, COMBAT_SILENT_ZOOM);
    let find = |units: &[UnitView], id: SimId| units.binary_search_by_key(&id, |unit| unit.id).ok().map(|index| units[index]);
    // Only what is on screen, and only zoomed in.
    let in_view = |x: i32, y: i32, z: Option<i32>| (listener.point)(x, y, z).filter(|&(_, on_screen)| on_screen && near > 0.0).map(|(across, _)| across);

    let (mut guns, mut lasers, mut missiles, mut impacts, mut destroyed) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    // Fired this tick: a new shell or missile, seen leaving its gun.
    for projectile in &current.projectiles {
        let Some([x, y, z]) = projectile.from else { continue };
        if previous.projectiles.binary_search_by_key(&projectile.id, |seen| seen.id).is_ok() {
            continue;
        }
        let Some(across) = in_view(x, y, Some(z)) else { continue };
        let sound = match projectile.kind {
            ProjectileKind::Shell => {
                let tier = find(&current.units, projectile.source).filter(|unit| unit.blueprint != u16::MAX).map_or(1, |unit| blueprints.get(BlueprintId(unit.blueprint)).tier);
                OneShot::Gun((0.15 + 0.22 * f32::from(tier.saturating_sub(1)) + units_of(projectile.splash) / 200.0).clamp(0.0, 1.0))
            }
            ProjectileKind::Discharge => OneShot::Gun(0.9),
            ProjectileKind::TacticalMissile => OneShot::Missile,
            // A strategic launch is heard everywhere as a warning, below.
            ProjectileKind::StrategicMissile | ProjectileKind::Beam => continue,
        };
        let event = SoundEvent { sound, gain: near, pan: across };
        if sound == OneShot::Missile { missiles.push(event) } else { guns.push(event) }
    }
    // What struck this tick. A laser's beam is fired and strikes within the tick: it is heard
    // from where it was fired.
    let detonated = |x: i32, y: i32| current.detonations.iter().any(|detonation| detonation.x == x && detonation.y == y);
    for struck in &current.landed {
        match struck.kind {
            ProjectileKind::Beam => {
                let [x, y, z] = struck.from.unwrap_or([struck.x, struck.y, struck.z]);
                if let Some(across) = in_view(x, y, Some(z)) {
                    lasers.push(SoundEvent { sound: OneShot::Laser, gain: near, pan: across });
                }
            }
            // A strategic missile is heard everywhere, below.
            ProjectileKind::StrategicMissile => {}
            kind => {
                if let Some(across) = in_view(struck.x, struck.y, Some(struck.z)) {
                    let heavier = if kind == ProjectileKind::TacticalMissile { 0.3 } else { 0.0 };
                    let size = (0.1 + units_of(struck.splash) / 150.0 + heavier).clamp(0.0, 1.0);
                    impacts.push(SoundEvent { sound: OneShot::Impact(size), gain: near, pan: across });
                }
            }
        }
    }
    // Units destroyed: gone since the snapshot before, leaving debris where they stood. A unit
    // that only goes out of sight leaves none, and is not heard.
    let new_debris: Vec<[i32; 3]> = current
        .wrecks
        .iter()
        .filter(|wreck| wreck.kind == WreckKind::Debris && previous.wrecks.binary_search_by_key(&wreck.id, |seen| seen.id).is_err())
        .map(|wreck| [wreck.x, wreck.y, wreck.radius])
        .collect();
    for unit in previous.units.iter().filter(|unit| !unit.remembered && find(&current.units, unit.id).is_none()) {
        let reach = i64::from(unit.radius) * 2 + 4096;
        let left_debris = new_debris.iter().any(|&[x, y, _]| (i64::from(x - unit.x)).pow(2) + (i64::from(y - unit.y)).pow(2) <= reach * reach);
        if !left_debris || detonated(unit.x, unit.y) {
            continue;
        }
        if let Some(across) = in_view(unit.x, unit.y, Some(unit.z)) {
            destroyed.push(SoundEvent { sound: OneShot::Destroyed(unit_size(unit, blueprints)), gain: near, pan: across });
        }
    }
    // Units that blow up as they die, by how wide the blast reaches; a commander's is heard
    // everywhere, below.
    for blast in current.blasts.iter().filter(|blast| !detonated(blast.x, blast.y)) {
        if let Some(across) = in_view(blast.x, blast.y, None) {
            let size = (0.3 + units_of(blast.radius) / 300.0).clamp(0.0, 1.0);
            destroyed.push(SoundEvent { sound: OneShot::Destroyed(size), gain: near, pan: across });
        }
    }
    events.extend(keep(guns, MOST_GUNS, size_of));
    events.extend(keep(lasers, MOST_LASERS, size_of));
    events.extend(keep(missiles, MOST_MISSILES, size_of));
    events.extend(keep(impacts, MOST_IMPACTS, size_of));
    events.extend(keep(destroyed, MOST_DESTROYED, size_of));

    // Heard wherever the camera is: loudest in view, softer off it, from its side of the screen.
    for detonation in &current.detonations {
        let (sound, off_screen) = match detonation.kind {
            DetonationKind::Commander => (OneShot::CommanderExplosion, COMMANDER_OFF_SCREEN),
            DetonationKind::StrategicMissile => (OneShot::Nuke, NUKE_OFF_SCREEN),
        };
        let (across, on_screen) = (listener.point)(detonation.x, detonation.y, None).unwrap_or((0.0, false));
        events.push(SoundEvent { sound, gain: if on_screen { 1.0 } else { off_screen }, pan: across });
    }
    if !current.strategic_launches.is_empty() {
        events.push(SoundEvent { sound: OneShot::LaunchWarning, gain: 1.0, pan: 0.0 });
    }
    events
}
