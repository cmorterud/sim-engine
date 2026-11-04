//! Simple low-poly shapes, so that units and structures can be told apart at a glance.
//! Each shape fits in a box from -0.5 to 0.5 on every axis, faces +Y, and is scaled per instance.

use glam::Vec3;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    Cube,
    Commander,
    Engineer,
    Bot,
    Tank,
    Artillery,
    Factory,
    Extractor,
    Generator,
    Storage,
    PointDefense,
    Shell,
    Wreck,
}

impl Shape {
    pub const ALL: [Self; 13] = [
        Self::Cube,
        Self::Commander,
        Self::Engineer,
        Self::Bot,
        Self::Tank,
        Self::Artillery,
        Self::Factory,
        Self::Extractor,
        Self::Generator,
        Self::Storage,
        Self::PointDefense,
        Self::Shell,
        Self::Wreck,
    ];
    pub const COUNT: usize = Self::ALL.len();
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

#[derive(Default)]
struct MeshBuilder {
    vertices: Vec<MeshVertex>,
    indices: Vec<u16>,
}

impl MeshBuilder {
    /// Adds a flat, convex polygon facing away from `part_center`.
    fn face(&mut self, points: &[Vec3], part_center: Vec3) {
        let face_center = points.iter().copied().sum::<Vec3>() / points.len() as f32;
        let mut normal = Vec3::ZERO;
        for index in 1..points.len() - 1 {
            normal = (points[index] - points[0]).cross(points[index + 1] - points[0]);
            if normal.length_squared() > 1e-10 {
                break;
            }
        }
        let normal = normal.normalize_or_zero();
        let flipped = normal.dot(face_center - part_center) < 0.0;
        let normal = if flipped { -normal } else { normal };
        let base = self.vertices.len() as u16;
        for point in points {
            self.vertices.push(MeshVertex { position: point.to_array(), normal: normal.to_array() });
        }
        for index in 1..points.len() as u16 - 1 {
            let (second, third) = if flipped { (index + 1, index) } else { (index, index + 1) };
            self.indices.extend_from_slice(&[base, base + second, base + third]);
        }
    }

    /// Adds a six-sided solid from its bottom four corners and top four corners, in the same order.
    fn cuboid(&mut self, bottom: [Vec3; 4], top: [Vec3; 4]) {
        let center = (bottom.iter().chain(&top).copied().sum::<Vec3>()) / 8.0;
        self.face(&bottom, center);
        self.face(&top, center);
        for index in 0..4 {
            let next = (index + 1) % 4;
            self.face(&[bottom[index], bottom[next], top[next], top[index]], center);
        }
    }

    fn block(&mut self, min: [f32; 3], max: [f32; 3]) {
        let corners = |z: f32| {
            [
                Vec3::new(min[0], min[1], z),
                Vec3::new(max[0], min[1], z),
                Vec3::new(max[0], max[1], z),
                Vec3::new(min[0], max[1], z),
            ]
        };
        self.cuboid(corners(min[2]), corners(max[2]));
    }

    /// A long box between two points, such as a gun barrel.
    fn beam(&mut self, from: [f32; 3], to: [f32; 3], half_width: f32, half_height: f32) {
        let (from, to) = (Vec3::from(from), Vec3::from(to));
        let side = (to - from).cross(Vec3::Z).normalize_or_zero() * half_width;
        let up = side.cross(to - from).normalize_or_zero() * half_height;
        let ring = |at: Vec3| [at - side - up, at + side - up, at + side + up, at - side + up];
        self.cuboid(ring(from), ring(to));
    }

    /// A round-ish solid from one radius to another. A top radius of zero makes a cone.
    fn frustum(&mut self, center: [f32; 2], z: [f32; 2], radius: [f32; 2], sides: usize) {
        // The small offset keeps vertices off the axes, which dots rely on when zoomed out.
        let point = |index: usize, radius: f32, z: f32| {
            let angle = 0.3 + index as f32 / sides as f32 * std::f32::consts::TAU;
            Vec3::new(center[0] + angle.cos() * radius, center[1] + angle.sin() * radius, z)
        };
        let part_center = Vec3::new(center[0], center[1], (z[0] + z[1]) * 0.5);
        for index in 0..sides {
            let next = (index + 1) % sides;
            if radius[1] > 0.0 {
                self.face(&[point(index, radius[0], z[0]), point(next, radius[0], z[0]), point(next, radius[1], z[1]), point(index, radius[1], z[1])], part_center);
            } else {
                self.face(&[point(index, radius[0], z[0]), point(next, radius[0], z[0]), Vec3::new(center[0], center[1], z[1])], part_center);
            }
        }
        let ring = |radius: f32, z: f32| (0..sides).map(|index| point(index, radius, z)).collect::<Vec<_>>();
        if radius[0] > 0.0 {
            self.face(&ring(radius[0], z[0]), part_center);
        }
        if radius[1] > 0.0 {
            self.face(&ring(radius[1], z[1]), part_center);
        }
    }
}

fn build_shape(shape: Shape, mesh: &mut MeshBuilder) {
    match shape {
        Shape::Cube => mesh.block([-0.5; 3], [0.5; 3]),
        // A broad walking figure with a gun arm: the one unit that must never be missed.
        Shape::Commander => {
            mesh.block([-0.28, -0.14, -0.5], [-0.07, 0.14, -0.1]);
            mesh.block([0.07, -0.14, -0.5], [0.28, 0.14, -0.1]);
            mesh.block([-0.3, -0.2, -0.1], [0.3, 0.2, 0.28]);
            mesh.block([-0.5, -0.12, -0.05], [-0.3, 0.12, 0.26]);
            mesh.block([0.3, -0.12, -0.05], [0.5, 0.12, 0.26]);
            mesh.beam([0.4, 0.1, 0.1], [0.4, 0.5, 0.1], 0.07, 0.07);
            mesh.block([-0.13, -0.1, 0.28], [0.13, 0.13, 0.5]);
        }
        // A low hull with a raised construction arm.
        Shape::Engineer => {
            mesh.block([-0.42, -0.5, -0.5], [0.42, 0.45, -0.12]);
            mesh.frustum([0.0, -0.15], [-0.12, 0.05], [0.3, 0.22], 6);
            mesh.beam([0.0, -0.15, 0.03], [0.0, 0.42, 0.42], 0.07, 0.06);
            mesh.frustum([0.0, 0.4], [0.3, 0.5], [0.04, 0.09], 6);
        }
        // A narrow two-legged walker.
        Shape::Bot => {
            mesh.block([-0.3, -0.12, -0.5], [-0.1, 0.12, -0.02]);
            mesh.block([0.1, -0.12, -0.5], [0.3, 0.12, -0.02]);
            mesh.block([-0.34, -0.22, -0.02], [0.34, 0.22, 0.42]);
            mesh.beam([0.0, 0.1, 0.2], [0.0, 0.5, 0.2], 0.08, 0.07);
            mesh.block([-0.12, -0.1, 0.42], [0.12, 0.1, 0.5]);
        }
        // A wide hull, a turret, and one long gun.
        Shape::Tank => {
            mesh.block([-0.48, -0.5, -0.5], [-0.2, 0.42, -0.18]);
            mesh.block([0.2, -0.5, -0.5], [0.48, 0.42, -0.18]);
            mesh.block([-0.3, -0.42, -0.42], [0.3, 0.34, -0.1]);
            mesh.frustum([0.0, -0.08], [-0.1, 0.16], [0.28, 0.2], 8);
            mesh.beam([0.0, 0.1, 0.03], [0.0, 0.49, 0.03], 0.05, 0.05);
        }
        // A small hull with a long barrel pointing up.
        Shape::Artillery => {
            mesh.block([-0.45, -0.5, -0.5], [0.45, 0.25, -0.2]);
            mesh.block([-0.2, -0.4, -0.2], [0.2, 0.05, 0.02]);
            mesh.beam([0.0, -0.2, -0.05], [0.0, 0.42, 0.42], 0.08, 0.08);
        }
        // A wide hall with a sawtooth roof and a smokestack.
        Shape::Factory => {
            mesh.block([-0.5, -0.5, -0.5], [0.5, 0.5, 0.05]);
            for start in [-0.5_f32, -0.17, 0.16] {
                let end = start + 0.33;
                let bottom = [
                    Vec3::new(start, -0.5, 0.05),
                    Vec3::new(end, -0.5, 0.05),
                    Vec3::new(end, 0.3, 0.05),
                    Vec3::new(start, 0.3, 0.05),
                ];
                let top = [
                    Vec3::new(start, -0.5, 0.05),
                    Vec3::new(end, -0.5, 0.4),
                    Vec3::new(end, 0.3, 0.4),
                    Vec3::new(start, 0.3, 0.05),
                ];
                mesh.cuboid(bottom, top);
            }
            mesh.frustum([0.32, 0.4], [0.05, 0.5], [0.09, 0.06], 6);
        }
        // A squat hexagonal collar with a central drill spike.
        Shape::Extractor => {
            mesh.frustum([0.0, 0.0], [-0.5, -0.15], [0.5, 0.36], 6);
            mesh.frustum([0.0, 0.0], [-0.15, 0.5], [0.16, 0.0], 6);
        }
        // A cooling tower, pinched at the waist.
        Shape::Generator => {
            mesh.frustum([0.0, 0.0], [-0.5, 0.2], [0.5, 0.26], 10);
            mesh.frustum([0.0, 0.0], [0.2, 0.5], [0.26, 0.36], 10);
        }
        // A wide storage drum with a domed lid.
        Shape::Storage => {
            mesh.frustum([0.0, 0.0], [-0.5, 0.15], [0.46, 0.46], 10);
            mesh.frustum([0.0, 0.0], [0.15, 0.5], [0.46, 0.14], 10);
        }
        // A turret on a pedestal, with a long gun.
        Shape::PointDefense => {
            mesh.frustum([0.0, 0.0], [-0.5, -0.05], [0.48, 0.22], 8);
            mesh.block([-0.26, -0.3, -0.05], [0.26, 0.2, 0.3]);
            mesh.beam([0.0, 0.1, 0.12], [0.0, 0.5, 0.12], 0.07, 0.07);
            mesh.block([-0.1, -0.2, 0.3], [0.1, 0.05, 0.5]);
        }
        Shape::Shell => {
            mesh.frustum([0.0, 0.0], [0.0, 0.5], [0.5, 0.0], 4);
            mesh.frustum([0.0, 0.0], [0.0, -0.5], [0.5, 0.0], 4);
        }
        // A broken pile: a few tilted slabs.
        Shape::Wreck => {
            mesh.block([-0.5, -0.5, -0.5], [0.5, 0.5, -0.25]);
            mesh.beam([-0.4, -0.35, -0.2], [0.35, -0.1, 0.3], 0.14, 0.1);
            mesh.beam([0.25, 0.35, -0.2], [-0.2, 0.05, 0.15], 0.12, 0.09);
        }
    }
}

/// Every shape, in one vertex and index buffer.
pub struct MeshSet {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u16>,
    /// The indices belonging to each shape, in the order of `Shape::ALL`.
    pub ranges: Vec<std::ops::Range<u32>>,
}

impl MeshSet {
    pub fn build() -> Self {
        let mut mesh = MeshBuilder::default();
        let mut ranges = Vec::with_capacity(Shape::COUNT);
        for shape in Shape::ALL {
            let start = mesh.indices.len() as u32;
            build_shape(shape, &mut mesh);
            ranges.push(start..mesh.indices.len() as u32);
        }
        assert!(mesh.vertices.len() < usize::from(u16::MAX), "too many vertices for 16-bit indices");
        Self { vertices: mesh.vertices, indices: mesh.indices, ranges }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape_vertices(set: &MeshSet, shape: usize) -> Vec<MeshVertex> {
        set.indices[set.ranges[shape].start as usize..set.ranges[shape].end as usize]
            .iter()
            .map(|&index| set.vertices[usize::from(index)])
            .collect()
    }

    #[test]
    fn every_shape_exists_and_fits_in_its_box() {
        let set = MeshSet::build();
        assert_eq!(set.ranges.len(), Shape::COUNT);
        for (index, shape) in Shape::ALL.iter().enumerate() {
            let vertices = shape_vertices(&set, index);
            assert!(!vertices.is_empty(), "{shape:?} is empty");
            for vertex in vertices {
                for value in vertex.position {
                    assert!(value.abs() <= 0.5001, "{shape:?} leaves its box at {value}");
                }
            }
        }
    }

    #[test]
    fn triangles_face_the_way_their_normals_say() {
        let set = MeshSet::build();
        for triangle in set.indices.chunks(3) {
            let at = |i: usize| Vec3::from(set.vertices[usize::from(triangle[i])].position);
            let facing = (at(1) - at(0)).cross(at(2) - at(0));
            let normal = Vec3::from(set.vertices[usize::from(triangle[0])].normal);
            assert!(facing.dot(normal) >= -1e-5, "triangle {triangle:?} faces against its normal");
        }
    }

    #[test]
    fn no_two_shapes_are_the_same() {
        let set = MeshSet::build();
        for first in 0..Shape::COUNT {
            for second in first + 1..Shape::COUNT {
                assert_ne!(shape_vertices(&set, first), shape_vertices(&set, second), "{:?} and {:?} match", Shape::ALL[first], Shape::ALL[second]);
            }
        }
    }

    /// When zoomed out, each vertex moves to a corner of a dot based on its quadrant, so a shape
    /// needs vertices in every quadrant to fill the dot.
    #[test]
    fn every_shape_can_fill_a_dot() {
        let set = MeshSet::build();
        for (index, shape) in Shape::ALL.iter().enumerate() {
            let vertices = shape_vertices(&set, index);
            for (x_sign, y_sign) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
                let found = vertices.iter().any(|vertex| vertex.position[0] * x_sign > 0.0 && vertex.position[1] * y_sign > 0.0);
                assert!(found, "{shape:?} has nothing in quadrant ({x_sign}, {y_sign})");
            }
        }
    }
}
