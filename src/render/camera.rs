//! Strategic Zoom camera. Positions are `f64` world coordinates, and everything sent
//! to the GPU is relative to the camera, because `f32` only resolves about 0.008 units
//! at the far edge of a maximum-size map.

use glam::{Mat4, Vec3, Vec4};

pub const MIN_ALTITUDE: f64 = 10.0;
/// Enough for a 45° vertical field of view to show a maximum-size map.
const ALTITUDE_PER_MAP_UNIT: f64 = 100_000.0 / 81_920.0;
const NEAR_PITCH_DEGREES: f64 = 45.0;
const FAR_PITCH_DEGREES: f64 = 89.0;
const VERTICAL_FOV_DEGREES: f32 = 45.0;
const NEAR_PLANE: f32 = 0.5;
/// How quickly the altitude closes on its target, per second.
const ZOOM_RATE: f64 = 8.0;
/// One scroll step changes altitude by this factor.
const ZOOM_STEP: f64 = 1.18;

/// Render-side copy of the height map, for keeping the camera above ground and for clicks.
pub struct HeightField {
    samples: usize,
    heights: Vec<i16>,
    cell_units: f64,
    units_per_step: f64,
}

impl HeightField {
    pub fn new(samples: usize, heights: Vec<i16>, cell_units: f64, units_per_step: f64) -> Self {
        Self { samples, heights, cell_units, units_per_step }
    }

    pub const fn samples(&self) -> usize {
        self.samples
    }

    pub fn heights(&self) -> &[i16] {
        &self.heights
    }

    pub const fn cell_units(&self) -> f64 {
        self.cell_units
    }

    pub const fn units_per_step(&self) -> f64 {
        self.units_per_step
    }

    pub fn height_at(&self, x: f64, y: f64) -> f64 {
        let last = (self.samples - 1) as f64;
        let fx = (x / self.cell_units).clamp(0.0, last - 1e-6);
        let fy = (y / self.cell_units).clamp(0.0, last - 1e-6);
        let (cx, cy) = (fx as usize, fy as usize);
        let (tx, ty) = (fx - cx as f64, fy - cy as f64);
        let at = |ix: usize, iy: usize| f64::from(self.heights[iy * self.samples + ix]);
        let near = at(cx, cy) + (at(cx + 1, cy) - at(cx, cy)) * tx;
        let far = at(cx, cy + 1) + (at(cx + 1, cy + 1) - at(cx, cy + 1)) * tx;
        (near + (far - near) * ty) * self.units_per_step
    }
}

pub struct Camera {
    map_units: f64,
    viewport: [f64; 2],
    focus: [f64; 2],
    ground: f64,
    altitude: f64,
    target_altitude: f64,
    max_altitude: f64,
    zoom_anchor: Option<[f64; 2]>,
}

impl Camera {
    pub fn new(map_units: f64, viewport: [f64; 2]) -> Self {
        let center = map_units / 2.0;
        Self {
            map_units,
            viewport,
            focus: [center, center],
            ground: 0.0,
            altitude: 400.0,
            target_altitude: 400.0,
            max_altitude: map_units * ALTITUDE_PER_MAP_UNIT,
            zoom_anchor: None,
        }
    }

    pub const fn altitude(&self) -> f64 {
        self.altitude
    }

    pub const fn max_altitude(&self) -> f64 {
        self.max_altitude
    }

    pub const fn viewport(&self) -> [f64; 2] {
        self.viewport
    }

    pub fn set_viewport(&mut self, viewport: [f64; 2]) {
        self.viewport = viewport;
    }

    /// Jumps straight to a view, with no smoothing.
    pub fn look_at(&mut self, x: f64, y: f64, altitude: f64) {
        self.focus = [x, y];
        self.set_altitude(if altitude > 0.0 { altitude } else { self.altitude });
    }

    pub fn set_altitude(&mut self, altitude: f64) {
        self.altitude = altitude.clamp(MIN_ALTITUDE, self.max_altitude);
        self.target_altitude = self.altitude;
    }

    /// One scroll step. Positive is in. The world point under `anchor` stays under the cursor.
    pub fn zoom(&mut self, steps: f64, anchor: Option<[f64; 2]>) {
        self.target_altitude = (self.target_altitude * ZOOM_STEP.powf(-steps)).clamp(MIN_ALTITUDE, self.max_altitude);
        self.zoom_anchor = anchor;
    }

    /// Pans by a fraction of the current view per call, so speed scales with altitude.
    pub fn pan(&mut self, dx: f64, dy: f64, dt: f64) {
        let speed = self.altitude * 1.2;
        self.focus[0] = (self.focus[0] + dx * speed * dt).clamp(0.0, self.map_units);
        self.focus[1] = (self.focus[1] + dy * speed * dt).clamp(0.0, self.map_units);
    }

    /// Exponential decay towards the target altitude, in log space so that it feels even at every scale.
    pub fn update(&mut self, dt: f64, heights: &HeightField) {
        let before = self.altitude;
        let blend = 1.0 - (-ZOOM_RATE * dt).exp();
        let log_altitude = before.ln() + (self.target_altitude.ln() - before.ln()) * blend;
        self.altitude = log_altitude.exp().clamp(MIN_ALTITUDE, self.max_altitude);
        if let Some(anchor) = self.zoom_anchor {
            let keep = 1.0 - self.altitude / before;
            self.focus[0] = (self.focus[0] + (anchor[0] - self.focus[0]) * keep).clamp(0.0, self.map_units);
            self.focus[1] = (self.focus[1] + (anchor[1] - self.focus[1]) * keep).clamp(0.0, self.map_units);
        }
        let ground = heights.height_at(self.focus[0], self.focus[1]);
        self.ground += (ground - self.ground) * (1.0 - (-6.0 * dt).exp());
    }

    /// 0 at minimum altitude and 1 at maximum, on a log scale.
    pub fn zoom_fraction(&self) -> f64 {
        ((self.altitude / MIN_ALTITUDE).ln() / (self.max_altitude / MIN_ALTITUDE).ln()).clamp(0.0, 1.0)
    }

    pub fn pitch_degrees(&self) -> f64 {
        NEAR_PITCH_DEGREES + (FAR_PITCH_DEGREES - NEAR_PITCH_DEGREES) * self.zoom_fraction()
    }

    fn direction(&self) -> [f64; 3] {
        let pitch = self.pitch_degrees().to_radians();
        [0.0, pitch.cos(), -pitch.sin()]
    }

    /// The camera's world position.
    pub fn eye(&self) -> [f64; 3] {
        let direction = self.direction();
        let reach = self.altitude / -direction[2];
        [
            self.focus[0] - direction[0] * reach,
            self.focus[1] - direction[1] * reach,
            self.ground + self.altitude,
        ]
    }

    pub const fn focus(&self) -> [f64; 2] {
        self.focus
    }

    /// The view-projection matrix for camera-relative coordinates, with reversed Z.
    pub fn view_projection(&self) -> Mat4 {
        let direction = self.direction();
        let forward = Vec3::new(direction[0] as f32, direction[1] as f32, direction[2] as f32);
        let aspect = (self.viewport[0] / self.viewport[1].max(1.0)) as f32;
        let projection = Mat4::perspective_infinite_reverse_rh(VERTICAL_FOV_DEGREES.to_radians(), aspect, NEAR_PLANE);
        projection * Mat4::look_to_rh(Vec3::ZERO, forward, Vec3::Z)
    }

    /// Where a world point lands on screen in pixels, or `None` if it is behind the camera.
    pub fn world_to_screen(&self, world: [f64; 3]) -> Option<[f64; 2]> {
        let eye = self.eye();
        let relative = Vec3::new((world[0] - eye[0]) as f32, (world[1] - eye[1]) as f32, (world[2] - eye[2]) as f32);
        let clip = self.view_projection() * relative.extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let (ndc_x, ndc_y) = (f64::from(clip.x / clip.w), f64::from(clip.y / clip.w));
        Some([(ndc_x + 1.0) * 0.5 * self.viewport[0], (1.0 - ndc_y) * 0.5 * self.viewport[1]])
    }

    /// The point on the ground under a screen position, found by marching the view ray over the height map.
    pub fn screen_to_ground(&self, screen: [f64; 2], heights: &HeightField) -> Option<[f64; 2]> {
        let ndc_x = (screen[0] / self.viewport[0] * 2.0 - 1.0) as f32;
        let ndc_y = (1.0 - screen[1] / self.viewport[1] * 2.0) as f32;
        let inverse = self.view_projection().inverse();
        // In reversed Z, depth 1 is the near plane and smaller values are farther away.
        let near = inverse * Vec4::new(ndc_x, ndc_y, 1.0, 1.0);
        let far = inverse * Vec4::new(ndc_x, ndc_y, 0.01, 1.0);
        let near = near.truncate() / near.w;
        let far = far.truncate() / far.w;
        let ray = (far - near).normalize();
        if ray.z >= 0.0 {
            return None;
        }
        let ray = [f64::from(ray.x), f64::from(ray.y), f64::from(ray.z)];
        let eye = self.eye();
        // Intersect a horizontal plane at the local ground height, and refine with what is found there.
        let mut plane = self.ground;
        let mut point = None;
        for _ in 0..8 {
            let distance = (plane - eye[2]) / ray[2];
            let hit = [eye[0] + ray[0] * distance, eye[1] + ray[1] * distance];
            point = Some(hit);
            plane = heights.height_at(hit[0], hit[1]);
        }
        point.map(|hit| [hit[0].clamp(0.0, self.map_units - 1.0), hit[1].clamp(0.0, self.map_units - 1.0)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat() -> HeightField {
        HeightField::new(3, vec![0; 9], 20.0, 1.0 / 16.0)
    }

    #[test]
    fn maximum_altitude_shows_a_whole_maximum_size_map() {
        let mut camera = Camera::new(81_920.0, [1000.0, 1000.0]);
        camera.look_at(40_960.0, 40_960.0, f64::MAX);
        assert!(camera.altitude() >= 100_000.0);
        for corner in [[0.0, 0.0, 0.0], [81_920.0, 0.0, 0.0], [0.0, 81_920.0, 0.0], [81_920.0, 81_920.0, 0.0]] {
            let p = camera.world_to_screen(corner).unwrap();
            assert!(p[0] >= 0.0 && p[0] <= 1000.0 && p[1] >= 0.0 && p[1] <= 1000.0, "{corner:?} is at {p:?}");
        }
    }

    #[test]
    fn pitch_follows_altitude() {
        let mut camera = Camera::new(81_920.0, [1000.0, 800.0]);
        camera.set_altitude(MIN_ALTITUDE);
        assert!((camera.pitch_degrees() - 45.0).abs() < 1e-9);
        camera.set_altitude(f64::MAX);
        assert!((camera.pitch_degrees() - 89.0).abs() < 1e-9);
    }

    #[test]
    fn smaller_maps_have_lower_maximum_altitude() {
        let camera = Camera::new(5_120.0, [1000.0, 800.0]);
        assert!((camera.max_altitude() - 6_250.0).abs() < 1.0);
    }

    #[test]
    fn zoom_is_smooth_and_converges() {
        let mut camera = Camera::new(81_920.0, [1000.0, 800.0]);
        let heights = flat();
        camera.set_altitude(1_000.0);
        camera.zoom(5.0, None);
        camera.update(1.0 / 60.0, &heights);
        assert!(camera.altitude() < 1_000.0 && camera.altitude() > 900.0, "jumped to {}", camera.altitude());
        for _ in 0..300 {
            camera.update(1.0 / 60.0, &heights);
        }
        assert!((camera.altitude() - 1_000.0 / 1.18f64.powf(5.0)).abs() < 0.01);
    }

    #[test]
    fn screen_and_ground_round_trip() {
        let heights = flat();
        let mut camera = Camera::new(81_920.0, [1280.0, 720.0]);
        camera.look_at(40_000.0, 40_000.0, 300.0);
        for world in [[40_000.0, 40_000.0], [40_080.0, 40_050.0], [39_950.0, 40_120.0]] {
            let screen = camera.world_to_screen([world[0], world[1], 0.0]).unwrap();
            let ground = camera.screen_to_ground(screen, &heights).unwrap();
            assert!((ground[0] - world[0]).abs() < 0.5 && (ground[1] - world[1]).abs() < 0.5, "{world:?} became {ground:?}");
        }
    }

    #[test]
    fn zoom_keeps_the_anchor_near_the_cursor() {
        let heights = flat();
        let mut camera = Camera::new(81_920.0, [1280.0, 720.0]);
        camera.look_at(40_000.0, 40_000.0, 2_000.0);
        let anchor = [40_600.0, 40_000.0];
        let before = camera.world_to_screen([anchor[0], anchor[1], 0.0]).unwrap();
        camera.zoom(4.0, Some(anchor));
        for _ in 0..240 {
            camera.update(1.0 / 60.0, &heights);
        }
        let after = camera.world_to_screen([anchor[0], anchor[1], 0.0]).unwrap();
        assert!((after[0] - before[0]).abs() < 8.0, "x moved from {} to {}", before[0], after[0]);
    }
}
