//! What the camera can see, so that what it cannot is left out of a frame before it reaches the
//! graphics card. Nothing drawn changes: a part is only left out when neither it, nor its
//! tactical dot, nor its shadow can fall inside the view.

use glam::{Mat4, Vec3};

use super::instances::UnitInstanceRaw;

/// The sun's direction, as the shaders have it (`LIGHT`): shadows fall away from it.
const LIGHT: [f32; 3] = [0.449, -0.249, 0.858];
/// Moving parts, such as rotors and legs, swing a little outside their shape's box.
const REACH_SLACK: f32 = 1.3;
/// A dot's corner is further from its middle than its side, and its size is rounded up.
const DOT_SLACK: f32 = 1.5;

/// How a kind of part takes up room in the world.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum Presence {
    /// Where its shape is, and nowhere else.
    Solid,
    /// Where its shape is, and where its shadow falls on the ground.
    Shadowed,
    /// Laid on the ground under it, whatever height it was given.
    OnGround,
}

/// The view from the camera: everything inside the four planes through the eye and the edges of
/// the picture. Positions are relative to the eye, as instances are.
pub(super) struct Sight {
    /// The planes' normals, pointing inwards.
    sides: [Vec3; 4],
    /// The width in the world of one pixel, for each unit of distance from the eye.
    pixel: f32,
}

impl Sight {
    /// `icon_scale` is how many pixels an icon's pixel is on this display.
    pub(super) fn new(view_projection: Mat4, viewport_height: f32, icon_scale: f32) -> Self {
        let (across, up, depth) = (view_projection.row(0).truncate(), view_projection.row(1).truncate(), view_projection.row(3).truncate());
        let sides = [depth + across, depth - across, depth + up, depth - up].map(Vec3::normalize);
        // The projection scales heights by the cotangent of half the angle of view.
        let pixel = 2.0 / (up.length() * viewport_height.max(1.0)) * icon_scale;
        Self { sides, pixel }
    }

    /// Whether any of a ball can be inside the view.
    fn sees_ball(&self, center: Vec3, radius: f32) -> bool {
        self.sides.iter().all(|side| side.dot(center) >= -radius)
    }

    /// Whether any of a part can show in the picture: the part itself, the dot it is drawn as
    /// from afar, or its shadow. `ground` gives the height of the ground, or the water over it,
    /// at a point, relative to the eye as the part is.
    pub(super) fn sees(&self, part: &UnitInstanceRaw, presence: Presence, ground: impl Fn(f32, f32) -> f32) -> bool {
        let column = |index: usize| Vec3::new(part.model_matrix[index][0], part.model_matrix[index][1], part.model_matrix[index][2]);
        let center = column(3);
        if presence == Presence::OnGround {
            // As wide as it is, and as much higher or lower as the ground can be across it.
            let reach = column(0).length().hypot(column(1).length());
            return self.sees_ball(Vec3::new(center.x, center.y, ground(center.x, center.y)), reach);
        }
        // A shape fills a box one unit a side, so half the box's diagonal reaches all of it.
        let reach = 0.5 * (column(0).length_squared() + column(1).length_squared() + column(2).length_squared()).sqrt() * REACH_SLACK;
        // Its dot is a size in pixels, however far off, so in the world it grows with distance.
        let dot = part.color[3].max(0.0) * self.pixel * center.length() * DOT_SLACK;
        if self.sees_ball(center, reach.max(dot)) {
            return true;
        }
        if presence != Presence::Shadowed {
            return false;
        }
        // Its shadow lies on the ground, away from the sun, as far along as it is high. The
        // ground there may be higher or lower than under it, by as much again on a steep slope.
        let floor = ground(center.x, center.y);
        let height = (center.z - floor).max(0.0) + reach;
        let (along_x, along_y) = (LIGHT[0] / LIGHT[2] * height, LIGHT[1] / LIGHT[2] * height);
        let length = along_x.hypot(along_y);
        self.sees_ball(Vec3::new(center.x - along_x * 0.5, center.y - along_y * 0.5, floor), reach + length * 1.5)
    }
}

#[cfg(test)]
mod tests {
    use super::super::camera::{Camera, HeightField};
    use super::*;

    const VIEWPORT: [f64; 2] = [1920.0, 1080.0];
    /// The ground's height, as a camera that has not yet moved takes it to be under its view.
    const GROUND: f32 = 0.0;

    /// A camera over flat ground, looking at the middle of a map, and what it sees.
    fn view(altitude: f64) -> (Camera, Sight) {
        let heights = HeightField::new(2, vec![GROUND as i16; 4], 6_400.0, 1.0);
        let mut camera = Camera::new(6_400.0, VIEWPORT);
        camera.look_at(3_200.0, 3_200.0, altitude);
        camera.update(0.0, &heights);
        let sight = Sight::new(camera.view_projection(), VIEWPORT[1] as f32, 1.0);
        (camera, sight)
    }

    /// A part at a place in the world, `size` across, with a dot of `icon` pixels.
    fn part(camera: &Camera, at: [f64; 3], size: f32, icon: f32) -> UnitInstanceRaw {
        let eye = camera.eye();
        UnitInstanceRaw {
            model_matrix: [[size, 0.0, 0.0, 0.0], [0.0, size, 0.0, 0.0], [0.0, 0.0, size, 0.0], [(at[0] - eye[0]) as f32, (at[1] - eye[1]) as f32, (at[2] - eye[2]) as f32, 1.0]],
            color: [1.0, 1.0, 1.0, icon],
            hull: [0; 4],
            glow: [0; 4],
        }
    }

    fn flat(camera: &Camera) -> impl Fn(f32, f32) -> f32 {
        let floor = GROUND - camera.eye()[2] as f32;
        move |_, _| floor
    }

    /// Where a point in the world lands in the picture, from -1 to 1 either way, if it is in
    /// front of the camera.
    fn in_picture(camera: &Camera, at: [f64; 3]) -> Option<[f32; 2]> {
        let eye = camera.eye();
        let clip = camera.view_projection() * glam::Vec4::new((at[0] - eye[0]) as f32, (at[1] - eye[1]) as f32, (at[2] - eye[2]) as f32, 1.0);
        (clip.w > 0.0).then(|| [clip.x / clip.w, clip.y / clip.w])
    }

    #[test]
    fn the_shaders_sun_is_the_one_shadows_are_looked_for_under() {
        let shader = include_str!("shaders.wgsl");
        let line = format!("const LIGHT: vec3<f32> = vec3<f32>({}, {}, {});", LIGHT[0], LIGHT[1], LIGHT[2]);
        assert!(shader.contains(&line), "the shader should say `{line}`");
    }

    #[test]
    fn what_is_in_the_picture_is_seen_and_what_is_far_outside_it_is_not() {
        let (camera, sight) = view(400.0);
        let ground = flat(&camera);
        let middle = part(&camera, [3_200.0, 3_200.0, 5.0], 18.0, 2.5);
        assert!(sight.sees(&middle, Presence::Shadowed, &ground));
        for at in [[200.0, 3_200.0, 5.0], [6_200.0, 3_200.0, 5.0], [3_200.0, 200.0, 5.0], [3_200.0, 6_200.0, 5.0]] {
            assert!(in_picture(&camera, at).is_none_or(|[x, y]| x.abs() > 1.5 || y.abs() > 1.5), "{at:?} should be outside the picture");
            for presence in [Presence::Solid, Presence::Shadowed, Presence::OnGround] {
                assert!(!sight.sees(&part(&camera, at, 18.0, 2.5), presence, &ground), "{at:?} is out of sight as {presence:?}");
            }
        }
    }

    /// The rule that keeps the picture the same: nothing whose middle is in the picture is ever
    /// left out, at any zoom, whatever its size.
    #[test]
    fn nothing_in_the_picture_is_left_out() {
        for altitude in [20.0, 150.0, 900.0, 4_000.0, 7_800.0] {
            let (camera, sight) = view(altitude);
            let ground = flat(&camera);
            let (mut inside, mut left_out) = (0, 0);
            // A grid of places around the middle of the view, wider than the picture by far.
            let spacing = altitude / 6.0;
            for step_x in -40..=40 {
                for step_y in -40..=40 {
                    for height in [GROUND, GROUND + 5.0, GROUND + 60.0] {
                        let at = [3_200.0 + f64::from(step_x) * spacing, 3_200.0 + f64::from(step_y) * spacing, f64::from(height)];
                        for size in [1.0, 18.0, 160.0] {
                            let seen = sight.sees(&part(&camera, at, size, 2.5), Presence::Solid, &ground);
                            if in_picture(&camera, at).is_some_and(|[x, y]| x.abs() <= 1.0 && y.abs() <= 1.0) {
                                inside += 1;
                                assert!(seen, "{at:?} is in the picture from {altitude}, {size} across");
                            } else if !seen {
                                left_out += 1;
                            }
                        }
                    }
                }
            }
            assert!(inside > 0, "something is in the picture from {altitude}");
            assert!(left_out > 0, "something is left out from {altitude}");
        }
    }

    /// An aircraft off the edge of the picture, whose shadow falls inside it, is still drawn,
    /// for its shadow. The sun is in the east and a little south, so shadows fall west and north.
    #[test]
    fn a_part_out_of_sight_whose_shadow_is_in_sight_is_kept() {
        let (camera, sight) = view(300.0);
        let ground = flat(&camera);
        let height = 400.0;
        let shadow = [3_200.0, 3_200.0];
        let (east, south) = (f64::from(LIGHT[0] / LIGHT[2]) * height, f64::from(LIGHT[1] / LIGHT[2]) * height);
        let aircraft = [shadow[0] + east, shadow[1] + south, f64::from(GROUND) + height];
        assert!(in_picture(&camera, [shadow[0], shadow[1], f64::from(GROUND)]).is_some_and(|[x, y]| x.abs() < 0.2 && y.abs() < 0.2), "its shadow is mid-picture");
        let plane = part(&camera, aircraft, 20.0, 2.5);
        assert!(!sight.sees(&plane, Presence::Solid, &ground), "the aircraft itself is out of the picture");
        assert!(sight.sees(&plane, Presence::Shadowed, &ground), "but is kept for its shadow");
    }

    /// A unit shown as a dot is a few pixels across however far off, which is far wider than
    /// the unit itself from high up: one just off the edge of the picture still shows part of
    /// its dot, so it is kept.
    #[test]
    fn a_dot_whose_middle_is_just_off_the_edge_is_kept() {
        let (camera, sight) = view(7_800.0);
        let ground = flat(&camera);
        let eye = camera.eye();
        // Walk east along the ground from the middle until just past the picture's edge.
        let just_off = (0..6_400).map(|step| [3_200.0 + f64::from(step), 3_200.0, f64::from(GROUND)]).find(|&at| in_picture(&camera, at).is_none_or(|[x, _]| x > 1.0)).expect("the picture has an edge");
        let pixels_off = (in_picture(&camera, just_off).expect("in front")[0] - 1.0) * VIEWPORT[0] as f32 / 2.0;
        assert!(pixels_off < 2.5, "its middle is {pixels_off} pixels off the edge, under its dot's half-width");
        assert!(sight.sees(&part(&camera, just_off, 1.0, 2.5), Presence::Solid, &ground));
        assert!(eye[2] > 1_000.0);
    }

    /// A scar is drawn on the ground whatever height it was given, so it is looked for there.
    #[test]
    fn what_lies_on_the_ground_is_looked_for_on_the_ground() {
        let (camera, sight) = view(300.0);
        let ground = flat(&camera);
        // Given a height far below the map, where nothing under the middle of the picture shows.
        let scar = part(&camera, [3_200.0, 3_200.0, -50_000.0], 12.0, 0.0);
        assert!(sight.sees(&scar, Presence::OnGround, &ground));
    }
}
