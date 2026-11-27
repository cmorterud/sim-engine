//! Builders' beams to their work, and the wiring between structures that share adjacency
//! bonuses, drawn in the world rather than over it: lit, hidden behind what stands in front of
//! them, and as large as the world around them. Both are left out once the view is zoomed out
//! to icons, where they would only be clutter.

use super::instances::{painted, InstanceBatches, UnitInstanceRaw, BLUE, RED, STRUCTURE_SHARE};
use super::meshes::Shape;
use crate::sim::components::{SimId, Team};
use crate::sim::world::{SimSnapshot, UnitView};

/// Warm, so that beams stand out against both blue structures and the terrain.
const BEAM_BUILD: [f32; 3] = [1.0, 0.84, 0.43];
const BEAM_RECLAIM: [f32; 3] = [0.59, 1.0, 0.67];
/// How many pulses are on a work beam at once.
pub const BEAM_PULSES: usize = 5;
/// How long a pulse takes to travel along a beam, in seconds.
const BEAM_PULSE_SECONDS: f64 = 1.1;
/// How thick a beam is, and how large its pulses grow, in world units.
const BEAM_WIDTH: f32 = 0.7;
const PULSE_SIZE: [f32; 2] = [1.0, 2.6];
/// How many sparks fly about the work, and how far from it.
const SPARKS: usize = 4;
const SPARK_REACH: f64 = 2.6;

/// The adjacency wiring's glowing strip, and its dark sheath.
const CABLE_GLOW: [f32; 3] = [0.47, 0.9, 1.0];
const CABLE_SHEATH: [f32; 3] = [0.12, 0.13, 0.15];
/// How wide and deep a cable is, in world units, at most and as a share of the strip of floor
/// it runs down, and how high its middle lies above the ground: on top of a structure's
/// concrete foundation, which stands this high.
const CABLE_WIDTH: f32 = 2.4;
const CABLE_WIDTH_SHARE: f64 = 0.7;
const CABLE_DEPTH: f32 = 0.5;
const FOUNDATION_TOP: f64 = 0.24;
/// A junction box's size, in world units: along the seam, across it, and up. It stands on the
/// seam, in the strip of floor between the two structures.
const JUNCTION_SIZE: [f32; 3] = [4.0, 1.9, 1.6];
/// How far short of each end of the seam a cable stops, as a share of the seam's length.
const CABLE_INSET: f64 = 0.1;
/// Pulses a second running down a cable, from the giver to the receiver.
const CABLE_PULSE_RATE: f64 = 0.8;

const RAW_TO_WORLD: f64 = 1.0 / 4096.0;

/// A beam between a builder and what it is working on, in world coordinates.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct WorkBeam {
    pub builder: SimId,
    pub from: [f64; 3],
    pub to: [f64; 3],
    pub reclaim: bool,
}

fn find_unit(snapshot: &SimSnapshot, id: SimId) -> Option<&UnitView> {
    snapshot.units.binary_search_by_key(&id, |unit| unit.id).ok().map(|index| &snapshot.units[index])
}

/// Works out the beams for all work being done. Each starts at the front of the builder,
/// drawn where the builder appears between ticks.
pub fn work_beams(previous: &SimSnapshot, current: &SimSnapshot, alpha: f64) -> Vec<WorkBeam> {
    let raw = |value: i32| f64::from(value) * RAW_TO_WORLD;
    current
        .work_links
        .iter()
        .filter_map(|link| {
            let unit = find_unit(current, link.builder)?;
            let now = [raw(unit.x), raw(unit.y), raw(unit.z)];
            let before = find_unit(previous, link.builder).map_or(now, |unit| [raw(unit.x), raw(unit.y), raw(unit.z)]);
            let center: [f64; 3] = std::array::from_fn(|axis| before[axis] + (now[axis] - before[axis]) * alpha);
            let to = link.target.map(raw);
            let (dx, dy) = (to[0] - center[0], to[1] - center[1]);
            let distance = dx.hypot(dy);
            let reach = raw(unit.radius).min(distance);
            let (forward_x, forward_y) = if distance > 0.0 { (dx / distance * reach, dy / distance * reach) } else { (0.0, 0.0) };
            Some(WorkBeam { builder: link.builder, from: [center[0] + forward_x, center[1] + forward_y, center[2]], to, reclaim: link.reclaim })
        })
        .collect()
}

/// How far along its beam each pulse is, from 0 at the builder to 1 at the work. Pulses
/// flow towards the work, or back to the builder when reclaiming. Builders are out of
/// step with each other, so that a group reads as separate beams.
pub fn beam_pulse_shares(time: f64, builder: SimId, reclaim: bool) -> [f32; BEAM_PULSES] {
    let offset = (f64::from(builder.0) * 0.37).fract();
    std::array::from_fn(|index| {
        let share = (time / BEAM_PULSE_SECONDS + offset + index as f64 / BEAM_PULSES as f64).fract();
        (if reclaim { 1.0 - share } else { share }) as f32
    })
}

fn lerp(from: [f64; 3], to: [f64; 3], share: f64) -> [f64; 3] {
    std::array::from_fn(|axis| from[axis] + (to[axis] - from[axis]) * share)
}

/// A shape lying along X, stretched from `from` to `to`, `width` across and `height` up.
#[allow(clippy::too_many_arguments)]
pub(super) fn stretched(from: [f64; 3], to: [f64; 3], width: f32, height: f32, hull: [f32; 3], glow: [f32; 3], eye: [f64; 3]) -> Option<UnitInstanceRaw> {
    let delta = glam::DVec3::from(to) - glam::DVec3::from(from);
    let length = delta.length();
    if length < 1e-6 {
        return None;
    }
    let along = (delta / length).as_vec3();
    // Across: level, unless it runs straight up or down.
    let across = along.cross(glam::Vec3::Z).try_normalize().unwrap_or(glam::Vec3::X);
    let up = across.cross(along);
    let mut instance = painted(lerp(from, to, 0.5), [1.0; 3], [0.0, 1.0], glow, hull, glow, 0.0, eye);
    let length = length as f32;
    instance.model_matrix[0] = [along.x * length, along.y * length, along.z * length, 0.0];
    instance.model_matrix[1] = [across.x * width, across.y * width, across.z * width, 0.0];
    instance.model_matrix[2] = [up.x * height, up.y * height, up.z * height, 0.0];
    Some(instance)
}

/// A small glowing ball, such as a pulse running along a beam or cable.
fn bead(at: [f64; 3], size: [f32; 3], color: [f32; 3], eye: [f64; 3]) -> UnitInstanceRaw {
    painted(at, size, [0.0, 1.0], color, color, color, 0.0, eye)
}

/// Draws work beams in the world: a glowing rod from the builder to its work, pulses running
/// along it, and sparks flying about the work.
pub fn build_work_beam_instances(beams: &[WorkBeam], time: f64, eye: [f64; 3], out: &mut InstanceBatches) {
    for beam in beams {
        let color = if beam.reclaim { BEAM_RECLAIM } else { BEAM_BUILD };
        let Some(rod) = stretched(beam.from, beam.to, BEAM_WIDTH, BEAM_WIDTH, color, color, eye) else {
            continue;
        };
        out.push(Shape::Beam, rod);
        for share in beam_pulse_shares(time, beam.builder, beam.reclaim) {
            // Pulses swell in the middle and shrink at the ends, so that they do not pop.
            let swell = (share * std::f32::consts::PI).sin();
            let size = PULSE_SIZE[0] + (PULSE_SIZE[1] - PULSE_SIZE[0]) * swell;
            out.push(Shape::Fireball, bead(lerp(beam.from, beam.to, f64::from(share)), [size; 3], color, eye));
        }
        // Sparks circle the work, flickering, each builder's out of step with the next.
        let offset = f64::from(beam.builder.0) * 1.7;
        for index in 0..SPARKS {
            let angle = time * 4.0 + offset + index as f64 * std::f64::consts::TAU / SPARKS as f64;
            let flicker = ((time * 11.0 + offset + index as f64 * 2.3).sin() * 0.5 + 0.5) as f32;
            let reach = SPARK_REACH * (0.6 + 0.4 * f64::from(flicker));
            let at = [beam.to[0] + angle.cos() * reach, beam.to[1] + angle.sin() * reach, beam.to[2] + (angle * 1.3).sin() * reach * 0.5];
            out.push(Shape::Fireball, bead(at, [0.5 + 0.9 * flicker; 3], color, eye));
        }
    }
}

/// Where two square footprints touch, `radius` either way about their middles: the seam's two
/// ends, along the edge of the first that faces the second, and the way across it from the
/// first to the second. `None` if they meet only at a corner, or not at all.
fn seam(first: [f64; 2], first_radius: f64, second: [f64; 2], second_radius: f64) -> Option<([[f64; 2]; 2], [f64; 2])> {
    let (dx, dy) = (second[0] - first[0], second[1] - first[1]);
    // Side by side east and west, the seam runs north to south, and the other way round.
    let (across, along) = if dx.abs() >= dy.abs() { (0, 1) } else { (1, 0) };
    let edge = first[across] + first_radius * if [dx, dy][across] >= 0.0 { 1.0 } else { -1.0 };
    let low = (first[along] - first_radius).max(second[along] - second_radius);
    let high = (first[along] + first_radius).min(second[along] + second_radius);
    (high > low).then(|| {
        let point = |at: f64| if across == 0 { [edge, at] } else { [at, edge] };
        let sign = if [dx, dy][across] >= 0.0 { 1.0 } else { -1.0 };
        let normal = if across == 0 { [sign, 0.0] } else { [0.0, sign] };
        ([point(low), point(high)], normal)
    })
}

/// Draws the wiring between structures sharing adjacency bonuses, `links` being the giver's
/// and the receiver's middles, as raw `Fp` bits at ground height. Each structure stands a
/// little inside its footprint, so touching ones leave a strip of floor along the seam where
/// they meet: the cable lies down the middle of it, with a junction box at its middle and
/// pulses running out from the box along it.
pub fn build_wiring_instances(links: &[([i32; 3], [i32; 3])], units: &[UnitView], time: f64, eye: [f64; 3], out: &mut InstanceBatches) {
    if links.is_empty() {
        return;
    }
    let mut structures: Vec<&UnitView> = units.iter().filter(|unit| unit.is_structure).collect();
    structures.sort_unstable_by_key(|unit| (unit.x, unit.y));
    let at = |point: [i32; 3]| structures.binary_search_by_key(&(point[0], point[1]), |unit| (unit.x, unit.y)).ok().map(|index| structures[index]);
    let raw = |value: i32| f64::from(value) * RAW_TO_WORLD;
    let lift = f64::from(CABLE_DEPTH) * 0.5;
    let phase = (time * CABLE_PULSE_RATE).fract();
    for &(giver, receiver) in links {
        let (Some(giver_unit), Some(receiver_unit)) = (at(giver), at(receiver)) else {
            continue;
        };
        let (giver_radius, receiver_radius) = (raw(giver_unit.radius), raw(receiver_unit.radius));
        let Some(([low, high], normal)) = seam([raw(giver[0]), raw(giver[1])], giver_radius, [raw(receiver[0]), raw(receiver[1])], receiver_radius) else {
            continue;
        };
        // The strip of floor runs from where the giver stops, short of the seam, to where the
        // receiver starts, beyond it; the cable lies down its middle.
        let margin = |radius: f64| radius * (1.0 - f64::from(STRUCTURE_SHARE));
        let (giver_margin, receiver_margin) = (margin(giver_radius), margin(receiver_radius));
        let shift = (receiver_margin - giver_margin) * 0.5;
        let width = (CABLE_WIDTH as f64).min((giver_margin + receiver_margin) * CABLE_WIDTH_SHARE) as f32;
        let [low, high] = [low, high].map(|point| [point[0] + normal[0] * shift, point[1] + normal[1] * shift]);
        // On the floor, on the foundations, which may stand at two heights on a slope.
        let floor = (raw(giver[2]).max(raw(receiver[2]))) + FOUNDATION_TOP;
        let inset = |from: [f64; 2], to: [f64; 2]| [from[0] + (to[0] - from[0]) * CABLE_INSET, from[1] + (to[1] - from[1]) * CABLE_INSET, floor + lift];
        let (start, end) = (inset(low, high), inset(high, low));
        if let Some(cable) = stretched(start, end, width, CABLE_DEPTH, CABLE_SHEATH, CABLE_GLOW, eye) {
            out.push(Shape::Cable, cable);
        }
        // The box at the middle of the seam, its +X along the cable: to the right of a heading
        // (x, y) is (y, -x).
        let middle = lerp(start, end, 0.5);
        let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
        let length = dx.hypot(dy).max(1e-9);
        let heading = [(-dy / length) as f32, (dx / length) as f32];
        let team = if giver_unit.team == Team::Blue { BLUE } else { RED };
        let center = [middle[0], middle[1], floor + f64::from(JUNCTION_SIZE[2]) * 0.5];
        out.push(Shape::JunctionBox, painted(center, JUNCTION_SIZE, heading, team, CABLE_SHEATH, CABLE_GLOW, 0.0, eye));
        // Pulses run out from the box to both ends of the cable.
        for offset in [0.0, 0.5] {
            let along = (phase + offset).fract();
            for toward in [start, end] {
                let point = lerp(middle, toward, along);
                out.push(Shape::Fireball, bead([point[0], point[1], point[2] + lift], [1.1, 1.1, 0.5], CABLE_GLOW, eye));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::world::WorkLinkView;

    fn unit(id: u32, x: f64) -> UnitView {
        UnitView {
            id: SimId(id),
            team: Team::Blue,
            x: (x * 4096.0) as i32,
            y: 0,
            z: 0,
            radius: 4096,
            blueprint: u16::MAX,
            health_permille: 1000,
            built_permille: 1000,
            is_structure: false,
            facing: [0, 0],
            aim: [0, 0],
            extra_aims: [[0; 2]; 3], pitches: [0; 4],
            working: false,
            assembling: None,
            upgrade: None,
            remembered: false,
            ally: false,
        }
    }

    fn snapshot(units: Vec<UnitView>) -> SimSnapshot {
        SimSnapshot { units, ..SimSnapshot::default() }
    }

    #[test]
    fn a_work_beam_runs_from_the_front_of_the_builder_to_the_work() {
        let raw = |value: f64| (value * 4096.0) as i32;
        let mut builder = unit(1, 100.0);
        builder.radius = raw(5.0);
        let mut current = snapshot(vec![builder]);
        current.work_links = vec![
            WorkLinkView { builder: SimId(1), target: [raw(150.0), 0, raw(7.0)], reclaim: false },
            // A builder that cannot be seen has no beam.
            WorkLinkView { builder: SimId(2), target: [raw(150.0), 0, 0], reclaim: false },
        ];
        let beams = work_beams(&current, &current, 1.0);
        assert_eq!(beams.len(), 1);
        assert_eq!(beams[0].from[0], 105.0, "the beam starts at the front of the builder");
        assert_eq!(beams[0].to, [150.0, 0.0, 7.0]);

        // Between ticks, the beam stays attached to where the builder is drawn.
        let mut earlier = builder;
        earlier.x = raw(90.0);
        let before = snapshot(vec![earlier]);
        assert_eq!(work_beams(&before, &current, 0.5)[0].from[0], 100.0);
    }

    #[test]
    fn a_builder_standing_on_its_work_still_gets_a_valid_beam() {
        let mut current = snapshot(vec![unit(1, 100.0)]);
        current.work_links = vec![WorkLinkView { builder: SimId(1), target: [100 * 4096, 0, 0], reclaim: false }];
        let beam = work_beams(&current, &current, 1.0)[0];
        assert!(beam.from.iter().all(|value| value.is_finite()));
        // Standing right on it, there is nothing to stretch a beam over, and nothing breaks.
        let mut out = InstanceBatches::default();
        build_work_beam_instances(&[beam], 1.0, [0.0; 3], &mut out);
        assert!(out.shape(Shape::Beam).len() <= 1);
    }

    #[test]
    fn beam_pulses_flow_to_the_work_and_back_when_reclaiming() {
        let later = 0.05;
        let (build_before, build_after) = (beam_pulse_shares(1.0, SimId(1), false), beam_pulse_shares(1.0 + later, SimId(1), false));
        let (reclaim_before, reclaim_after) = (beam_pulse_shares(1.0, SimId(1), true), beam_pulse_shares(1.0 + later, SimId(1), true));
        for index in 0..BEAM_PULSES {
            for share in [build_before[index], reclaim_before[index]] {
                assert!((0.0..=1.0).contains(&share));
            }
            let moved = build_after[index] - build_before[index];
            // A pulse that just wrapped has jumped back to the start.
            assert!(moved > 0.0 || moved < -0.5, "building should flow outwards: {moved}");
            let moved = reclaim_after[index] - reclaim_before[index];
            assert!(moved < 0.0 || moved > 0.5, "reclaiming should flow back: {moved}");
        }
        assert_ne!(beam_pulse_shares(1.0, SimId(1), false), beam_pulse_shares(1.0, SimId(2), false), "builders should be out of step");
    }

    /// A beam is a rod in the world, from the builder to the work, with its pulses on it and
    /// sparks about the work.
    #[test]
    fn a_work_beam_is_a_rod_in_the_world_from_the_builder_to_the_work() {
        let beam = WorkBeam { builder: SimId(1), from: [0.0, 0.0, 5.0], to: [40.0, 0.0, 5.0], reclaim: false };
        let mut out = InstanceBatches::default();
        build_work_beam_instances(&[beam], 0.3, [0.0; 3], &mut out);
        let [rod] = out.shape(Shape::Beam) else { panic!("one rod") };
        assert!((rod.model_matrix[3][0] - 20.0).abs() < 1e-3, "centered between the ends: {:?}", rod.model_matrix[3]);
        assert!((rod.model_matrix[0][0] - 40.0).abs() < 1e-3, "as long as the beam: {:?}", rod.model_matrix[0]);
        let beads = out.shape(Shape::Fireball);
        assert_eq!(beads.len(), BEAM_PULSES + SPARKS);
        for pulse in &beads[..BEAM_PULSES] {
            let x = pulse.model_matrix[3][0];
            assert!((0.0..=40.0).contains(&x) && pulse.model_matrix[3][1].abs() < 1e-3, "pulses run along the beam: {x}");
        }
        for spark in &beads[BEAM_PULSES..] {
            let at = spark.model_matrix[3];
            assert!((at[0] - 40.0).hypot(at[1]) <= SPARK_REACH as f32 + 1e-3, "sparks fly about the work: {at:?}");
        }
    }

    /// Two touching structures sharing a bonus are wired along the seam where they meet, on the
    /// floor: a cable as long as the seam, less a little at each end, with a junction box at its
    /// middle, and pulses running out from the box.
    #[test]
    fn adjacent_structures_are_wired_along_the_seam_where_they_meet() {
        let raw = |value: f64| (value * 4096.0) as i32;
        // A factory 80 wide, and a generator 20 wide touching its east side, 10 north of its middle.
        let mut factory = unit(1, 0.0);
        (factory.is_structure, factory.radius) = (true, raw(40.0));
        let mut generator = unit(2, 50.0);
        (generator.is_structure, generator.radius, generator.y) = (true, raw(10.0), raw(10.0));
        let link = ([generator.x, generator.y, raw(20.0)], [factory.x, 0, raw(20.0)]);
        let mut out = InstanceBatches::default();
        build_wiring_instances(&[link], &[factory, generator], 0.25, [0.0; 3], &mut out);
        let [cable] = out.shape(Shape::Cable) else { panic!("one cable") };
        let at = cable.model_matrix[3];
        // The generator stands 1 short of the seam at x 40 and the factory 4 short of it, so the
        // strip of floor runs from x 36 to 41, and the cable down its middle.
        assert!((at[0] - 38.5).abs() < 1e-3 && (at[1] - 10.0).abs() < 1e-3, "down the strip along the seam, beside the generator: {at:?}");
        assert!((cable.model_matrix[1][0].hypot(cable.model_matrix[1][1]) - 2.4).abs() < 1e-3, "as wide as it may be: {:?}", cable.model_matrix[1]);
        let along = cable.model_matrix[0];
        assert!(along[0].abs() < 1e-3 && (along[1].abs() - 16.0).abs() < 1e-3, "running north and south, the seam's 20 less a tenth each end: {along:?}");
        assert!(at[2] > 20.0 && at[2] < 21.0, "on the floor, on the foundations: {at:?}");
        let [junction] = out.shape(Shape::JunctionBox) else { panic!("one box") };
        assert!((junction.model_matrix[3][0] - 38.5).abs() < 1e-3 && (junction.model_matrix[3][1] - 10.0).abs() < 1e-3, "at the middle of the strip: {:?}", junction.model_matrix[3]);
        // Pulses run out from the box, so they are further from it a moment later.
        let spread = |batches: &InstanceBatches| batches.shape(Shape::Fireball).iter().map(|pulse| (pulse.model_matrix[3][1] - 10.0).abs()).fold(0.0, f32::max);
        let mut later = InstanceBatches::default();
        build_wiring_instances(&[link], &[factory, generator], 0.3, [0.0; 3], &mut later);
        assert!(spread(&later) > spread(&out), "pulses run out along the seam");
        // Wiring needs both ends: one whose structure is not seen is left out, and so is a pair
        // meeting only at a corner.
        let mut alone = InstanceBatches::default();
        build_wiring_instances(&[link], &[factory], 0.25, [0.0; 3], &mut alone);
        assert_eq!(alone.total(), 0);
        assert_eq!(seam([0.0, 0.0], 40.0, [50.0, 50.0], 10.0), None, "corners only");
    }
}
