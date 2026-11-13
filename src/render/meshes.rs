//! Simple low-poly shapes, so that units and structures can be told apart at a glance.
//! Each shape fits in a box from -0.5 to 0.5 on every axis, faces +Y, and is scaled per instance.

use glam::Vec3;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    Cube,
    /// The commander's upper body, which turns to face its work.
    Commander,
    /// The commander's legs, which face the way it walks.
    CommanderLegs,
    /// The engineer's construction arm, which turns to face its work.
    Engineer,
    /// The engineer's hull, which faces the way it drives.
    EngineerHull,
    /// The bot's upper body and gun, which turn to aim.
    Bot,
    BotLegs,
    /// The tank's turret and gun, which swivel to aim.
    Tank,
    TankHull,
    /// The artillery's barrel, which swivels to aim.
    Artillery,
    ArtilleryHull,
    Factory,
    Extractor,
    Generator,
    Storage,
    /// The point defense's gun, which swivels to aim.
    PointDefense,
    PointDefenseBase,
    Shell,
    /// A body with fins, pointing along +Y.
    Missile,
    Wreck,
    /// A conifer: a thin trunk under a cone.
    Tree,
    /// A squat, many-sided boulder.
    Rock,
    /// A pylon with an emitter on top.
    ShieldGenerator,
    /// A box with a launch rail tilted up.
    MissileLauncher,
    /// A low base with a tall laser mast.
    MissileDefense,
    /// A wide, low block with a hatch on top.
    NukeSilo,
    /// A drum with a ring of launch tubes.
    AntiNukeSilo,
    /// A mast with a tilted dish.
    Radar,
    /// A cone wrapped in rings.
    StealthGenerator,
    /// A slab with four pillars around a glowing core, under a ring.
    Gateway,
    /// A wide, low drum with three stacks.
    HydrocarbonPlant,
    /// A block wrapped in rings.
    Fabricator,
    /// A fighter: a slim body with swept delta wings and a tail fin, pointing along +Y.
    Aircraft,
    /// A thick body with long straight wings and a tailplane, pointing along +Y.
    Bomber,
    /// A turret with two barrels raised to the sky, which swivels to aim.
    AntiAirGun,
    /// A low deck with four round landing pads and a control tower.
    AirStaging,
    /// A gunship: a thick body under a rotor disc, with a tail boom, pointing along +Y.
    Gunship,
    /// A wide flying wing, pointing along +Y.
    StrategicBomber,
    /// A turret with one short, fat barrel raised to the sky, which swivels to aim.
    FlakGun,
    /// A turret carrying a box of missiles raised to the sky, which swivels to aim.
    SamLauncher,
    /// A heavy gun on a low armored base, its long barrel raised high, which swivels to aim.
    ArtilleryGun,
    /// Two long piers with a gantry across them, for building ships between.
    NavalFactory,
    /// A slim hull with one gun forward and a short mast, pointing along +Y.
    Frigate,
    /// A longer hull with a gun fore and aft, a bridge, and a mast.
    Destroyer,
    /// A hull with missile cells fore and aft and a tall radar mast.
    Cruiser,
    /// A broad hull with three twin-gun turrets around a stepped tower.
    Battleship,
    /// A long, fast hull with two turrets forward and a raked tower.
    Battlecruiser,
    /// A low, rounded hull with a tower amidships.
    Submarine,
    /// A longer submarine with a row of glowing missile hatches.
    MissileSubmarine,
    /// A hull with a tall emitter mast.
    ShieldBoat,
    /// The land factory at tier 2: a side hall with two stacks.
    FactoryT2,
    /// The land factory at tier 3: a stepped roof under a gantry, with three stacks.
    FactoryT3,
    /// An arched hangar with a runway and a control tower.
    AirFactory,
    AirFactoryT2,
    AirFactoryT3,
    /// The tier 2 power generator: two cooling towers around a glowing core.
    GeneratorT2,
    /// The tier 3 power generator: a domed reactor ringed with vents.
    GeneratorT3,
    /// The tier 2 extractor: a collar with three pistons.
    ExtractorT2,
    /// The tier 3 extractor: a tall collar with a glowing ring and four pistons.
    ExtractorT3,
    /// The tier 2 naval factory: a second gantry and a crane.
    NavalFactoryT2,
    /// The tier 3 naval factory: tall piers lit along their length, two gantries, and a tower.
    NavalFactoryT3,
    /// A heavy fighter with two engines and two tail fins, pointing along +Y.
    AirSuperiority,
    /// A long gunship with two rotors, pointing along +Y.
    HeavyGunship,
    /// A shield's dome: an open half-sphere, drawn see-through.
    Dome,
    /// Flat, around the feet of a selected mobile unit.
    SelectionRing,
    /// Flat, around the footprint of a selected structure.
    SelectionBox,
}

impl Shape {
    pub const ALL: [Self; 66] = [
        Self::Cube,
        Self::Commander,
        Self::CommanderLegs,
        Self::Engineer,
        Self::EngineerHull,
        Self::Bot,
        Self::BotLegs,
        Self::Tank,
        Self::TankHull,
        Self::Artillery,
        Self::ArtilleryHull,
        Self::Factory,
        Self::Extractor,
        Self::Generator,
        Self::Storage,
        Self::PointDefense,
        Self::PointDefenseBase,
        Self::Shell,
        Self::Missile,
        Self::Wreck,
        Self::Tree,
        Self::Rock,
        Self::ShieldGenerator,
        Self::MissileLauncher,
        Self::MissileDefense,
        Self::NukeSilo,
        Self::AntiNukeSilo,
        Self::Radar,
        Self::StealthGenerator,
        Self::Gateway,
        Self::HydrocarbonPlant,
        Self::Fabricator,
        Self::Aircraft,
        Self::Bomber,
        Self::AntiAirGun,
        Self::AirStaging,
        Self::Gunship,
        Self::StrategicBomber,
        Self::FlakGun,
        Self::SamLauncher,
        Self::ArtilleryGun,
        Self::NavalFactory,
        Self::Frigate,
        Self::Destroyer,
        Self::Cruiser,
        Self::Battleship,
        Self::Battlecruiser,
        Self::Submarine,
        Self::MissileSubmarine,
        Self::ShieldBoat,
        Self::FactoryT2,
        Self::FactoryT3,
        Self::AirFactory,
        Self::AirFactoryT2,
        Self::AirFactoryT3,
        Self::GeneratorT2,
        Self::GeneratorT3,
        Self::ExtractorT2,
        Self::ExtractorT3,
        Self::NavalFactoryT2,
        Self::NavalFactoryT3,
        Self::AirSuperiority,
        Self::HeavyGunship,
        Self::Dome,
        Self::SelectionRing,
        Self::SelectionBox,
    ];
    pub const COUNT: usize = Self::ALL.len();

    /// Whether the shape throws a shadow on the ground. Markers, see-through domes, and shells
    /// too small to see do not.
    pub const fn casts_shadow(self) -> bool {
        !matches!(self, Self::Dome | Self::SelectionRing | Self::SelectionBox | Self::Shell)
    }
}

/// What a part of a shape is made of, which decides its color.
#[repr(u32)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Material {
    /// The team's color, and the instance's only color for shapes that are not painted.
    #[default]
    Trim = 0,
    /// The body: grey armor, colored a little by what the unit is.
    Hull = 1,
    /// Dark gunmetal, for guns, tracks, legs, and masts.
    Metal = 2,
    /// Lit from inside: engines, emitters, and cores. Never shaded.
    Glow = 3,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// A `Material`.
    pub material: u32,
}

#[derive(Default)]
struct MeshBuilder {
    vertices: Vec<MeshVertex>,
    indices: Vec<u16>,
    /// What the parts added next are made of.
    material: Material,
}

impl MeshBuilder {
    fn paint(&mut self, material: Material) {
        self.material = material;
    }

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
            self.vertices.push(MeshVertex { position: point.to_array(), normal: normal.to_array(), material: self.material as u32 });
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

    /// A solid from a flat, convex outline, from one height to another. Each end is the outline
    /// scaled about its middle, so that a hull can be narrower at the keel than at the deck.
    fn prism(&mut self, outline: &[[f32; 2]], z: [f32; 2], scale: [f32; 2]) {
        let middle = outline.iter().fold([0.0, 0.0], |sum, point| [sum[0] + point[0], sum[1] + point[1]]).map(|sum| sum / outline.len() as f32);
        let ring = |z: f32, scale: f32| outline.iter().map(|point| Vec3::new(middle[0] + (point[0] - middle[0]) * scale, middle[1] + (point[1] - middle[1]) * scale, z)).collect::<Vec<_>>();
        let (bottom, top) = (ring(z[0], scale[0]), ring(z[1], scale[1]));
        let part_center = Vec3::new(middle[0], middle[1], (z[0] + z[1]) * 0.5);
        self.face(&bottom, part_center);
        self.face(&top, part_center);
        for index in 0..outline.len() {
            let next = (index + 1) % outline.len();
            self.face(&[bottom[index], bottom[next], top[next], top[index]], part_center);
        }
    }

    /// A box whose top edges are cut back by `cut`, so that it catches the light along them.
    fn bevel(&mut self, min: [f32; 3], max: [f32; 3], cut: f32) {
        self.block(min, [max[0], max[1], max[2] - cut]);
        let corners = |inset: f32, z: f32| {
            [
                Vec3::new(min[0] + inset, min[1] + inset, z),
                Vec3::new(max[0] - inset, min[1] + inset, z),
                Vec3::new(max[0] - inset, max[1] - inset, z),
                Vec3::new(min[0] + inset, max[1] - inset, z),
            ]
        };
        self.cuboid(corners(0.0, max[2] - cut), corners(cut, max[2]));
    }

    /// A half-round roof along Y, from the side walls at `half_width` up to `rise` above `z`.
    fn arch(&mut self, half_width: f32, y: [f32; 2], z: f32, rise: f32, segments: usize) {
        let at = |index: usize, y: f32| {
            let angle = index as f32 / segments as f32 * std::f32::consts::PI;
            Vec3::new(angle.cos() * half_width, y, z + angle.sin() * rise)
        };
        let part_center = Vec3::new(0.0, (y[0] + y[1]) * 0.5, z);
        for index in 0..segments {
            self.face(&[at(index, y[0]), at(index + 1, y[0]), at(index + 1, y[1]), at(index, y[1])], part_center);
        }
        for end in y {
            self.face(&(0..=segments).map(|index| at(index, end)).collect::<Vec<_>>(), part_center);
        }
    }

    /// A gun turret: a team-colored drum with barrels pointing along +Y, or -Y if `aft`.
    fn turret(&mut self, at: [f32; 2], z: [f32; 2], radius: f32, barrels: usize, length: f32, aft: bool) {
        self.paint(Material::Trim);
        self.frustum(at, z, [radius, radius * 0.8], 8);
        self.paint(Material::Metal);
        let direction = if aft { -1.0 } else { 1.0 };
        let height = (z[0] + z[1]) * 0.5;
        for index in 0..barrels {
            let x = at[0] + (index as f32 - (barrels as f32 - 1.0) * 0.5) * radius * 0.6;
            self.beam([x, at[1], height], [x, at[1] + direction * (radius + length), height], radius * 0.14, radius * 0.14);
        }
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
            mesh.block([-0.3, -0.2, -0.1], [0.3, 0.2, 0.28]);
            mesh.paint(Material::Hull);
            mesh.block([-0.5, -0.12, -0.05], [-0.3, 0.12, 0.26]);
            mesh.block([0.3, -0.12, -0.05], [0.5, 0.12, 0.26]);
            mesh.paint(Material::Metal);
            mesh.beam([0.4, 0.1, 0.1], [0.4, 0.5, 0.1], 0.07, 0.07);
            mesh.paint(Material::Hull);
            mesh.block([-0.13, -0.1, 0.28], [0.13, 0.13, 0.5]);
            // A visor across the front of the head, and a core in the chest.
            mesh.paint(Material::Glow);
            mesh.block([-0.1, 0.13, 0.38], [0.1, 0.16, 0.45]);
            mesh.block([-0.08, 0.2, 0.05], [0.08, 0.23, 0.18]);
        }
        // Two legs, with feet that point the way it walks.
        Shape::CommanderLegs => {
            mesh.paint(Material::Hull);
            mesh.block([-0.28, -0.14, -0.5], [-0.07, 0.14, -0.1]);
            mesh.block([0.07, -0.14, -0.5], [0.28, 0.14, -0.1]);
            mesh.paint(Material::Metal);
            mesh.block([-0.28, 0.14, -0.5], [-0.07, 0.24, -0.4]);
            mesh.block([0.07, 0.14, -0.5], [0.28, 0.24, -0.4]);
        }
        // A low hull, with a sloped front so that its direction shows.
        Shape::EngineerHull => {
            mesh.block([-0.42, -0.5, -0.5], [0.42, 0.3, -0.12]);
            mesh.paint(Material::Metal);
            mesh.block([-0.36, 0.3, -0.5], [0.36, 0.45, -0.22]);
        }
        // A raised construction arm on a turntable.
        Shape::Engineer => {
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, -0.15], [-0.12, 0.05], [0.3, 0.22], 6);
            mesh.beam([0.0, -0.15, 0.03], [0.0, 0.42, 0.42], 0.07, 0.06);
            mesh.paint(Material::Glow);
            mesh.frustum([0.0, 0.4], [0.3, 0.5], [0.04, 0.09], 6);
        }
        // A narrow two-legged walker, with a gun in its right arm and its left arm raised.
        Shape::Bot => {
            mesh.bevel([-0.22, -0.18, -0.02], [0.22, 0.18, 0.36], 0.05);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.36, -0.14, 0.18], [-0.22, 0.12, 0.4], 0.04);
            mesh.bevel([0.22, -0.14, 0.18], [0.36, 0.12, 0.4], 0.04);
            mesh.beam([-0.3, 0.0, 0.2], [-0.3, 0.28, 0.12], 0.05, 0.05);
            mesh.beam([0.3, -0.02, 0.2], [0.3, 0.2, 0.18], 0.06, 0.06);
            mesh.paint(Material::Metal);
            mesh.beam([0.3, 0.18, 0.18], [0.3, 0.5, 0.18], 0.045, 0.045);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.1, -0.08, 0.36], [0.1, 0.1, 0.5], 0.03);
            mesh.paint(Material::Glow);
            mesh.block([-0.07, 0.1, 0.41], [0.07, 0.12, 0.45]);
        }
        Shape::BotLegs => {
            mesh.paint(Material::Metal);
            mesh.block([-0.3, -0.12, -0.5], [-0.1, 0.12, -0.02]);
            mesh.block([0.1, -0.12, -0.5], [0.3, 0.12, -0.02]);
            mesh.block([-0.3, 0.12, -0.5], [-0.1, 0.2, -0.42]);
            mesh.block([0.1, 0.12, -0.5], [0.3, 0.2, -0.42]);
        }
        // A wide hull on two tracks.
        Shape::TankHull => {
            mesh.paint(Material::Metal);
            mesh.block([-0.48, -0.5, -0.5], [-0.2, 0.42, -0.18]);
            mesh.block([0.2, -0.5, -0.5], [0.48, 0.42, -0.18]);
            mesh.paint(Material::Hull);
            mesh.cuboid(
                [Vec3::new(-0.3, -0.42, -0.42), Vec3::new(0.3, -0.42, -0.42), Vec3::new(0.3, 0.42, -0.42), Vec3::new(-0.3, 0.42, -0.42)],
                [Vec3::new(-0.3, -0.42, -0.1), Vec3::new(0.3, -0.42, -0.1), Vec3::new(0.3, 0.22, -0.1), Vec3::new(-0.3, 0.22, -0.1)],
            );
        }
        // A turret and one long gun.
        Shape::Tank => {
            mesh.frustum([0.0, -0.08], [-0.1, 0.16], [0.28, 0.2], 8);
            mesh.paint(Material::Metal);
            mesh.beam([0.0, 0.1, 0.03], [0.0, 0.49, 0.03], 0.05, 0.05);
        }
        // A small hull with a long barrel pointing up.
        Shape::ArtilleryHull => {
            mesh.paint(Material::Hull);
            mesh.block([-0.45, -0.5, -0.5], [0.45, 0.25, -0.2]);
            mesh.paint(Material::Metal);
            mesh.block([-0.3, 0.25, -0.5], [0.3, 0.38, -0.3]);
        }
        Shape::Artillery => {
            mesh.block([-0.2, -0.4, -0.2], [0.2, 0.05, 0.02]);
            mesh.paint(Material::Metal);
            mesh.beam([0.0, -0.2, -0.05], [0.0, 0.42, 0.42], 0.08, 0.08);
        }
        // A wide hall with a sawtooth roof and a smokestack.
        Shape::Factory => {
            mesh.paint(Material::Hull);
            mesh.block([-0.5, -0.5, -0.5], [0.5, 0.5, 0.05]);
            mesh.paint(Material::Trim);
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
            mesh.paint(Material::Metal);
            mesh.frustum([0.32, 0.4], [0.05, 0.5], [0.09, 0.06], 6);
        }
        // A squat hexagonal collar with a central drill spike.
        Shape::Extractor => {
            mesh.frustum([0.0, 0.0], [-0.5, -0.15], [0.5, 0.36], 6);
            mesh.paint(Material::Glow);
            mesh.frustum([0.0, 0.0], [-0.15, 0.5], [0.16, 0.0], 6);
        }
        // A cooling tower, pinched at the waist.
        Shape::Generator => {
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.5, 0.2], [0.5, 0.26], 10);
            mesh.paint(Material::Glow);
            mesh.frustum([0.0, 0.0], [0.14, 0.26], [0.31, 0.31], 10);
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, 0.0], [0.2, 0.5], [0.26, 0.36], 10);
        }
        // A wide storage drum with a domed lid.
        Shape::Storage => {
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.5, 0.15], [0.46, 0.46], 10);
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, 0.0], [0.15, 0.5], [0.46, 0.14], 10);
        }
        // A turret on a pedestal, with a long gun.
        Shape::PointDefenseBase => {
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.5, -0.05], [0.48, 0.22], 8);
        }
        Shape::PointDefense => {
            mesh.block([-0.26, -0.3, -0.05], [0.26, 0.2, 0.3]);
            mesh.paint(Material::Metal);
            mesh.beam([0.0, 0.1, 0.12], [0.0, 0.5, 0.12], 0.07, 0.07);
            mesh.paint(Material::Hull);
            mesh.block([-0.1, -0.2, 0.3], [0.1, 0.05, 0.5]);
        }
        Shape::Shell => {
            mesh.frustum([0.0, 0.0], [0.0, 0.5], [0.5, 0.0], 4);
            mesh.frustum([0.0, 0.0], [0.0, -0.5], [0.5, 0.0], 4);
        }
        Shape::Missile => {
            mesh.beam([0.0, -0.5, 0.0], [0.0, 0.3, 0.0], 0.12, 0.12);
            mesh.beam([0.0, 0.3, 0.0], [0.0, 0.5, 0.0], 0.06, 0.06);
            mesh.block([-0.3, -0.5, -0.04], [0.3, -0.3, 0.04]);
            mesh.block([-0.04, -0.5, -0.3], [0.04, -0.3, 0.3]);
        }
        // A broken pile: a few tilted slabs.
        Shape::Wreck => {
            mesh.block([-0.5, -0.5, -0.5], [0.5, 0.5, -0.25]);
            mesh.beam([-0.4, -0.35, -0.2], [0.35, -0.1, 0.3], 0.14, 0.1);
            mesh.beam([0.25, 0.35, -0.2], [-0.2, 0.05, 0.15], 0.12, 0.09);
        }
        Shape::Tree => {
            mesh.frustum([0.0, 0.0], [-0.5, -0.2], [0.09, 0.07], 6);
            mesh.frustum([0.0, 0.0], [-0.3, 0.5], [0.5, 0.0], 7);
        }
        Shape::Rock => {
            mesh.frustum([0.0, 0.0], [-0.5, 0.15], [0.5, 0.36], 5);
            mesh.frustum([0.04, -0.03], [0.15, 0.5], [0.36, 0.08], 5);
        }
        Shape::ShieldGenerator => {
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.5, -0.3], [0.5, 0.42], 8);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, 0.0], [-0.3, 0.2], [0.16, 0.12], 6);
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, 0.0], [0.2, 0.35], [0.12, 0.3], 8);
            mesh.paint(Material::Glow);
            mesh.frustum([0.0, 0.0], [0.35, 0.5], [0.3, 0.1], 8);
        }
        Shape::MissileLauncher => {
            mesh.block([-0.5, -0.5, -0.5], [0.5, 0.5, -0.15]);
            mesh.paint(Material::Hull);
            mesh.beam([0.0, -0.3, -0.1], [0.0, 0.36, 0.36], 0.18, 0.1);
        }
        Shape::MissileDefense => {
            mesh.frustum([0.0, 0.0], [-0.5, -0.25], [0.5, 0.4], 6);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, 0.0], [-0.25, 0.3], [0.1, 0.07], 6);
            mesh.paint(Material::Glow);
            mesh.block([-0.22, -0.15, 0.3], [0.22, 0.15, 0.5]);
        }
        Shape::NukeSilo => {
            mesh.paint(Material::Hull);
            mesh.block([-0.5, -0.5, -0.5], [0.5, 0.5, 0.1]);
            mesh.paint(Material::Trim);
            mesh.block([-0.25, -0.25, 0.1], [0.25, 0.25, 0.25]);
            mesh.paint(Material::Metal);
            mesh.frustum([0.3, 0.3], [0.1, 0.5], [0.12, 0.08], 6);
        }
        Shape::AntiNukeSilo => {
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.5, 0.15], [0.5, 0.48], 10);
            for (x, y) in [(-0.22_f32, -0.22_f32), (0.22, -0.22), (-0.22, 0.22), (0.22, 0.22)] {
                mesh.paint(Material::Trim);
                mesh.frustum([x, y], [0.15, 0.5], [0.12, 0.12], 6);
            }
        }
        Shape::Radar => {
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.5, -0.35], [0.4, 0.32], 6);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, 0.0], [-0.35, 0.15], [0.07, 0.05], 6);
            mesh.paint(Material::Trim);
            mesh.cuboid(
                [Vec3::new(-0.45, -0.1, 0.1), Vec3::new(0.45, -0.1, 0.1), Vec3::new(0.45, 0.05, 0.1), Vec3::new(-0.45, 0.05, 0.1)],
                [Vec3::new(-0.45, 0.1, 0.5), Vec3::new(0.45, 0.1, 0.5), Vec3::new(0.45, 0.25, 0.5), Vec3::new(-0.45, 0.25, 0.5)],
            );
        }
        Shape::StealthGenerator => {
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.5, 0.5], [0.3, 0.0], 8);
            mesh.paint(Material::Trim);
            for z in [-0.35_f32, -0.05, 0.25] {
                let radius = 0.5 - (z + 0.5) * 0.4;
                mesh.frustum([0.0, 0.0], [z, z + 0.06], [radius, radius], 12);
            }
        }
        Shape::HydrocarbonPlant => {
            mesh.frustum([0.0, 0.0], [-0.5, -0.15], [0.48, 0.44], 12);
            for (x, y) in [(-0.22_f32, -0.15_f32), (0.22, -0.15), (0.0, 0.25)] {
                mesh.paint(Material::Metal);
                mesh.frustum([x, y], [-0.15, 0.5], [0.09, 0.06], 8);
            }
        }
        Shape::Fabricator => {
            mesh.block([-0.32, -0.32, -0.5], [0.32, 0.32, 0.35]);
            mesh.paint(Material::Glow);
            for z in [-0.3_f32, 0.0, 0.3] {
                mesh.frustum([0.0, 0.0], [z, z + 0.08], [0.48, 0.48], 12);
            }
        }
        Shape::Aircraft => {
            mesh.prism(&[[-0.1, -0.45], [0.1, -0.45], [0.12, 0.1], [0.02, 0.5], [-0.02, 0.5], [-0.12, 0.1]], [-0.1, 0.1], [0.8, 0.7]);
            mesh.paint(Material::Hull);
            mesh.prism(&[[-0.5, -0.4], [0.5, -0.4], [0.08, 0.22], [-0.08, 0.22]], [-0.03, 0.03], [1.0, 1.0]);
            mesh.paint(Material::Trim);
            mesh.prism(&[[-0.02, -0.45], [0.02, -0.45], [0.02, -0.25], [-0.02, -0.3]], [0.0, 0.32], [1.0, 1.0]);
            // A dark canopy, and an engine glowing at the tail.
            mesh.paint(Material::Metal);
            mesh.bevel([-0.06, 0.12, 0.08], [0.06, 0.34, 0.15], 0.03);
            mesh.paint(Material::Glow);
            mesh.block([-0.07, -0.5, -0.06], [0.07, -0.45, 0.06]);
        }
        Shape::AirSuperiority => {
            mesh.prism(&[[-0.16, -0.42], [0.16, -0.42], [0.14, 0.15], [0.02, 0.5], [-0.02, 0.5], [-0.14, 0.15]], [-0.1, 0.1], [0.8, 0.7]);
            mesh.paint(Material::Hull);
            mesh.prism(&[[-0.5, -0.32], [0.5, -0.32], [0.48, -0.18], [0.1, 0.25], [-0.1, 0.25], [-0.48, -0.18]], [-0.03, 0.03], [1.0, 1.0]);
            mesh.prism(&[[-0.3, -0.48], [0.3, -0.48], [0.12, -0.32], [-0.12, -0.32]], [-0.02, 0.02], [1.0, 1.0]);
            mesh.paint(Material::Trim);
            for x in [-0.12_f32, 0.12] {
                mesh.prism(&[[x - 0.02, -0.45], [x + 0.02, -0.45], [x + 0.02, -0.22], [x - 0.02, -0.28]], [0.05, 0.3], [1.0, 1.0]);
            }
            mesh.paint(Material::Metal);
            mesh.bevel([-0.06, 0.15, 0.08], [0.06, 0.38, 0.16], 0.03);
            mesh.paint(Material::Glow);
            for x in [-0.08_f32, 0.08] {
                mesh.block([x - 0.06, -0.5, -0.06], [x + 0.06, -0.42, 0.06]);
            }
        }
        Shape::Bomber => {
            mesh.prism(&[[-0.14, -0.5], [0.14, -0.5], [0.2, -0.1], [0.18, 0.38], [0.06, 0.5], [-0.06, 0.5], [-0.18, 0.38], [-0.2, -0.1]], [-0.18, 0.16], [0.8, 0.75]);
            mesh.paint(Material::Hull);
            mesh.block([-0.5, -0.05, -0.03], [0.5, 0.18, 0.03]);
            mesh.block([-0.22, -0.5, -0.02], [0.22, -0.36, 0.02]);
            mesh.paint(Material::Trim);
            mesh.block([-0.02, -0.5, 0.0], [0.02, -0.36, 0.25]);
            // Engines under the wings, glowing behind, and a dark canopy at the nose.
            mesh.paint(Material::Metal);
            for x in [-0.32_f32, 0.32] {
                mesh.bevel([x - 0.06, -0.1, -0.12], [x + 0.06, 0.24, -0.03], 0.02);
            }
            mesh.bevel([-0.09, 0.28, 0.1], [0.09, 0.44, 0.19], 0.03);
            mesh.paint(Material::Glow);
            for x in [-0.32_f32, 0.32] {
                mesh.block([x - 0.05, -0.14, -0.11], [x + 0.05, -0.1, -0.04]);
            }
        }
        Shape::Gunship => {
            mesh.paint(Material::Hull);
            mesh.prism(&[[-0.14, -0.12], [0.14, -0.12], [0.2, 0.05], [0.16, 0.4], [0.06, 0.48], [-0.06, 0.48], [-0.16, 0.4], [-0.2, 0.05]], [-0.3, 0.05], [0.75, 0.85]);
            mesh.beam([0.0, -0.5, -0.08], [0.0, -0.1, -0.08], 0.05, 0.05);
            mesh.paint(Material::Trim);
            mesh.block([-0.02, -0.5, -0.06], [0.02, -0.4, 0.1]);
            mesh.bevel([-0.1, -0.05, 0.05], [0.1, 0.3, 0.1], 0.02);
            // Stub wings with rocket pods, and a dark canopy at the nose.
            mesh.paint(Material::Hull);
            mesh.block([-0.34, 0.0, -0.14], [0.34, 0.12, -0.1]);
            mesh.paint(Material::Metal);
            for x in [-0.3_f32, 0.3] {
                mesh.beam([x, -0.04, -0.18], [x, 0.2, -0.18], 0.04, 0.04);
            }
            mesh.bevel([-0.1, 0.32, -0.12], [0.1, 0.46, 0.0], 0.03);
            // Four team-colored blades on a hub, so that the team shows from above.
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, 0.0], [0.05, 0.16], [0.05, 0.04], 6);
            mesh.paint(Material::Trim);
            for (x, y) in [(0.33_f32, 0.33_f32), (-0.33, 0.33)] {
                mesh.beam([-x, -y, 0.14], [x, y, 0.14], 0.035, 0.01);
            }
        }
        Shape::HeavyGunship => {
            mesh.paint(Material::Hull);
            mesh.prism(&[[-0.14, -0.4], [0.14, -0.4], [0.18, -0.2], [0.18, 0.36], [0.06, 0.48], [-0.06, 0.48], [-0.18, 0.36], [-0.18, -0.2]], [-0.32, 0.0], [0.75, 0.85]);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.1, -0.3, 0.0], [0.1, 0.3, 0.06], 0.02);
            mesh.paint(Material::Hull);
            mesh.block([-0.36, -0.06, -0.16], [0.36, 0.08, -0.12]);
            mesh.paint(Material::Metal);
            for x in [-0.32_f32, 0.32] {
                mesh.beam([x, -0.1, -0.2], [x, 0.16, -0.2], 0.045, 0.045);
            }
            mesh.bevel([-0.1, 0.34, -0.12], [0.1, 0.47, -0.02], 0.03);
            // A rotor at each end, on a mast.
            for y in [-0.26_f32, 0.24] {
                mesh.paint(Material::Metal);
                mesh.frustum([0.0, y], [0.0, 0.14], [0.05, 0.04], 6);
                mesh.paint(Material::Trim);
                for (dx, dy) in [(0.17_f32, 0.17_f32), (-0.17, 0.17)] {
                    mesh.beam([-dx, y - dy, 0.12], [dx, y + dy, 0.12], 0.03, 0.01);
                }
            }
        }
        Shape::StrategicBomber => {
            mesh.prism(&[[-0.5, -0.3], [-0.25, -0.38], [0.25, -0.38], [0.5, -0.3], [0.08, 0.5], [-0.08, 0.5]], [-0.05, 0.05], [1.0, 1.0]);
            mesh.paint(Material::Hull);
            mesh.prism(&[[-0.12, -0.4], [0.12, -0.4], [0.14, 0.2], [0.04, 0.48], [-0.04, 0.48], [-0.14, 0.2]], [-0.1, 0.14], [0.8, 0.7]);
            mesh.paint(Material::Metal);
            mesh.bevel([-0.07, 0.24, 0.1], [0.07, 0.4, 0.17], 0.03);
            // A row of engines glowing along the trailing edge.
            mesh.paint(Material::Glow);
            for x in [-0.3_f32, -0.18, 0.18, 0.3] {
                mesh.block([x - 0.04, -0.37, -0.03], [x + 0.04, -0.33, 0.04]);
            }
        }
        Shape::FlakGun => {
            mesh.block([-0.28, -0.28, -0.1], [0.28, 0.2, 0.2]);
            mesh.paint(Material::Metal);
            mesh.beam([0.0, -0.05, 0.1], [0.0, 0.28, 0.38], 0.1, 0.1);
        }
        Shape::SamLauncher => {
            mesh.block([-0.25, -0.25, -0.1], [0.25, 0.2, 0.15]);
            mesh.cuboid(
                [Vec3::new(-0.3, -0.2, 0.05), Vec3::new(0.3, -0.2, 0.05), Vec3::new(0.3, -0.05, 0.05), Vec3::new(-0.3, -0.05, 0.05)],
                [Vec3::new(-0.3, 0.2, 0.45), Vec3::new(0.3, 0.2, 0.45), Vec3::new(0.3, 0.35, 0.45), Vec3::new(-0.3, 0.35, 0.45)],
            );
        }
        Shape::ArtilleryGun => {
            mesh.block([-0.3, -0.35, -0.15], [0.3, 0.2, 0.15]);
            mesh.paint(Material::Metal);
            mesh.beam([0.0, -0.1, 0.0], [0.0, 0.42, 0.4], 0.07, 0.07);
            mesh.paint(Material::Hull);
            mesh.block([-0.12, -0.45, 0.0], [0.12, -0.3, 0.12]);
        }
        Shape::NavalFactory => {
            mesh.paint(Material::Hull);
            mesh.block([-0.5, -0.5, -0.5], [-0.3, 0.5, -0.15]);
            mesh.block([0.3, -0.5, -0.5], [0.5, 0.5, -0.15]);
            mesh.paint(Material::Trim);
            mesh.block([-0.3, -0.5, -0.5], [0.3, -0.35, 0.05]);
            mesh.block([-0.5, -0.1, 0.25], [0.5, 0.1, 0.4]);
            mesh.paint(Material::Metal);
            mesh.block([-0.48, -0.08, -0.15], [-0.36, 0.08, 0.25]);
            mesh.block([0.36, -0.08, -0.15], [0.48, 0.08, 0.25]);
        }
        Shape::Frigate => {
            mesh.paint(Material::Hull);
            mesh.prism(&[[-0.13, -0.5], [0.13, -0.5], [0.17, -0.38], [0.17, 0.18], [0.02, 0.5], [-0.02, 0.5], [-0.17, 0.18], [-0.17, -0.38]], [-0.5, -0.2], [0.65, 1.0]);
            mesh.paint(Material::Trim);
            mesh.block([-0.1, -0.46, -0.2], [0.1, -0.3, -0.17]);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.1, -0.24, -0.2], [0.1, 0.02, 0.04], 0.04);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.12], [0.04, 0.4], [0.025, 0.015], 6);
            mesh.turret([0.0, 0.16], [-0.2, -0.1], 0.08, 1, 0.16, false);
        }
        Shape::Destroyer => {
            mesh.paint(Material::Hull);
            mesh.prism(&[[-0.15, -0.5], [0.15, -0.5], [0.2, -0.36], [0.2, 0.22], [0.02, 0.5], [-0.02, 0.5], [-0.2, 0.22], [-0.2, -0.36]], [-0.5, -0.2], [0.65, 1.0]);
            mesh.bevel([-0.13, -0.18, -0.2], [0.13, 0.12, 0.02], 0.04);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.09, 0.02, 0.02], [0.09, 0.12, 0.12], 0.03);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.1], [0.02, 0.18], [0.06, 0.05], 8);
            mesh.frustum([0.0, -0.02], [0.12, 0.46], [0.022, 0.014], 6);
            mesh.block([-0.1, -0.03, 0.34], [0.1, -0.01, 0.36]);
            mesh.turret([0.0, 0.26], [-0.2, -0.1], 0.09, 1, 0.14, false);
            mesh.turret([0.0, -0.32], [-0.2, -0.1], 0.09, 1, 0.06, true);
        }
        Shape::Cruiser => {
            mesh.paint(Material::Hull);
            mesh.prism(&[[-0.16, -0.5], [0.16, -0.5], [0.22, -0.34], [0.22, 0.2], [0.02, 0.5], [-0.02, 0.5], [-0.22, 0.2], [-0.22, -0.34]], [-0.5, -0.2], [0.65, 1.0]);
            mesh.bevel([-0.15, -0.2, -0.2], [0.15, 0.1, 0.06], 0.05);
            // Missile cells: team-colored decks with dark hatches.
            for (y0, y1) in [(0.12_f32, 0.3_f32), (-0.44, -0.24)] {
                mesh.paint(Material::Trim);
                mesh.block([-0.13, y0, -0.2], [0.13, y1, -0.15]);
                mesh.paint(Material::Metal);
                for x in [-0.07_f32, 0.0, 0.07] {
                    mesh.block([x - 0.025, y0 + 0.03, -0.15], [x + 0.025, y1 - 0.03, -0.14]);
                }
            }
            mesh.paint(Material::Hull);
            mesh.bevel([-0.08, -0.1, 0.06], [0.08, 0.06, 0.2], 0.03);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.04], [0.2, 0.42], [0.025, 0.018], 6);
            mesh.paint(Material::Trim);
            mesh.cuboid(
                [Vec3::new(-0.12, -0.06, 0.36), Vec3::new(0.12, -0.06, 0.36), Vec3::new(0.12, -0.02, 0.36), Vec3::new(-0.12, -0.02, 0.36)],
                [Vec3::new(-0.12, -0.02, 0.5), Vec3::new(0.12, -0.02, 0.5), Vec3::new(0.12, 0.02, 0.5), Vec3::new(-0.12, 0.02, 0.5)],
            );
            mesh.turret([0.0, 0.34], [-0.2, -0.12], 0.07, 1, 0.07, false);
        }
        Shape::Battleship => {
            mesh.paint(Material::Hull);
            mesh.prism(&[[-0.2, -0.5], [0.2, -0.5], [0.27, -0.38], [0.28, 0.18], [0.04, 0.5], [-0.04, 0.5], [-0.28, 0.18], [-0.27, -0.38]], [-0.5, -0.22], [0.7, 1.0]);
            mesh.bevel([-0.18, -0.2, -0.22], [0.18, 0.06, 0.0], 0.04);
            mesh.bevel([-0.12, -0.14, 0.0], [0.12, 0.04, 0.14], 0.03);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.07, -0.08, 0.14], [0.07, 0.03, 0.28], 0.025);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.16], [0.0, 0.2], [0.07, 0.06], 8);
            mesh.frustum([0.0, -0.03], [0.28, 0.5], [0.02, 0.012], 6);
            mesh.turret([0.0, 0.28], [-0.22, -0.1], 0.11, 2, 0.1, false);
            mesh.turret([0.0, 0.13], [-0.1, 0.02], 0.11, 2, 0.12, false);
            mesh.turret([0.0, -0.3], [-0.22, -0.1], 0.11, 2, 0.07, true);
        }
        Shape::Battlecruiser => {
            mesh.paint(Material::Hull);
            mesh.prism(&[[-0.15, -0.5], [0.15, -0.5], [0.22, -0.36], [0.22, 0.12], [0.02, 0.5], [-0.02, 0.5], [-0.22, 0.12], [-0.22, -0.36]], [-0.5, -0.22], [0.65, 1.0]);
            // A tower raked back, lower at the stern than at the bow.
            mesh.cuboid(
                [Vec3::new(-0.14, -0.3, -0.22), Vec3::new(0.14, -0.3, -0.22), Vec3::new(0.14, 0.02, -0.22), Vec3::new(-0.14, 0.02, -0.22)],
                [Vec3::new(-0.1, -0.24, 0.0), Vec3::new(0.1, -0.24, 0.0), Vec3::new(0.1, -0.02, 0.12), Vec3::new(-0.1, -0.02, 0.12)],
            );
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.1], [0.0, 0.38], [0.022, 0.014], 6);
            mesh.paint(Material::Trim);
            mesh.block([-0.12, -0.46, -0.22], [0.12, -0.34, -0.16]);
            mesh.turret([0.0, 0.28], [-0.22, -0.1], 0.1, 2, 0.1, false);
            mesh.turret([0.0, 0.12], [-0.12, 0.0], 0.1, 2, 0.12, false);
        }
        Shape::Submarine => {
            let outline = [[-0.04, -0.5], [0.04, -0.5], [0.12, -0.36], [0.13, 0.24], [0.06, 0.44], [-0.06, 0.44], [-0.13, 0.24], [-0.12, -0.36]];
            mesh.paint(Material::Hull);
            mesh.prism(&outline, [-0.5, -0.3], [0.55, 1.0]);
            mesh.prism(&outline, [-0.3, -0.22], [1.0, 0.7]);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.05, 0.0, -0.24], [0.05, 0.2, -0.02], 0.025);
            mesh.paint(Material::Hull);
            mesh.block([-0.13, 0.08, -0.08], [0.13, 0.13, -0.06]);
            mesh.block([-0.12, -0.46, -0.37], [0.12, -0.4, -0.35]);
            mesh.block([-0.01, -0.46, -0.3], [0.01, -0.38, -0.18]);
        }
        Shape::MissileSubmarine => {
            let outline = [[-0.05, -0.5], [0.05, -0.5], [0.14, -0.36], [0.15, 0.28], [0.07, 0.46], [-0.07, 0.46], [-0.15, 0.28], [-0.14, -0.36]];
            mesh.paint(Material::Hull);
            mesh.prism(&outline, [-0.5, -0.28], [0.55, 1.0]);
            mesh.prism(&outline, [-0.28, -0.2], [1.0, 0.75]);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.06, 0.12, -0.22], [0.06, 0.34, 0.02], 0.025);
            mesh.paint(Material::Hull);
            mesh.block([-0.15, 0.2, -0.06], [0.15, 0.25, -0.04]);
            mesh.block([-0.01, -0.47, -0.28], [0.01, -0.38, -0.14]);
            // Two rows of missile hatches behind the tower.
            mesh.paint(Material::Glow);
            for y in [-0.24_f32, -0.12, 0.0] {
                for x in [-0.05_f32, 0.05] {
                    mesh.block([x - 0.03, y - 0.035, -0.2], [x + 0.03, y + 0.035, -0.19]);
                }
            }
        }
        Shape::ShieldBoat => {
            mesh.paint(Material::Hull);
            mesh.prism(&[[-0.15, -0.5], [0.15, -0.5], [0.2, -0.36], [0.2, 0.2], [0.03, 0.5], [-0.03, 0.5], [-0.2, 0.2], [-0.2, -0.36]], [-0.5, -0.2], [0.65, 1.0]);
            mesh.paint(Material::Trim);
            mesh.block([-0.14, -0.44, -0.2], [0.14, 0.3, -0.17]);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.12, -0.18, -0.17], [0.12, 0.14, 0.02], 0.04);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.02], [0.02, 0.32], [0.06, 0.04], 8);
            mesh.paint(Material::Glow);
            mesh.frustum([0.0, -0.02], [0.3, 0.42], [0.14, 0.14], 10);
            mesh.frustum([0.0, -0.02], [0.42, 0.5], [0.1, 0.04], 10);
        }
        Shape::FactoryT2 => {
            mesh.paint(Material::Hull);
            mesh.bevel([-0.5, -0.48, -0.5], [0.2, 0.5, 0.05], 0.04);
            mesh.bevel([0.2, -0.3, -0.5], [0.5, 0.5, -0.1], 0.04);
            mesh.paint(Material::Trim);
            for start in [-0.5_f32, -0.15] {
                let end = start + 0.35;
                let bottom = [Vec3::new(start, -0.5, 0.05), Vec3::new(end, -0.5, 0.05), Vec3::new(end, 0.3, 0.05), Vec3::new(start, 0.3, 0.05)];
                let top = [Vec3::new(start, -0.5, 0.05), Vec3::new(end, -0.5, 0.42), Vec3::new(end, 0.3, 0.42), Vec3::new(start, 0.3, 0.05)];
                mesh.cuboid(bottom, top);
            }
            mesh.paint(Material::Glow);
            mesh.block([-0.4, -0.5, -0.3], [0.1, -0.48, -0.24]);
            mesh.paint(Material::Metal);
            for y in [0.05_f32, 0.32] {
                mesh.frustum([0.36, y], [-0.1, 0.48], [0.07, 0.05], 6);
            }
        }
        Shape::FactoryT3 => {
            mesh.paint(Material::Hull);
            mesh.bevel([-0.5, -0.5, -0.5], [0.5, 0.5, -0.05], 0.05);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.44, -0.44, -0.05], [0.44, 0.2, 0.12], 0.04);
            mesh.bevel([-0.3, -0.3, 0.12], [0.3, 0.1, 0.26], 0.04);
            mesh.paint(Material::Glow);
            mesh.block([-0.44, -0.45, 0.0], [0.44, -0.44, 0.05]);
            mesh.block([-0.3, -0.31, 0.16], [0.3, -0.3, 0.2]);
            // A gantry across the roof.
            mesh.paint(Material::Metal);
            for x in [-0.46_f32, 0.46] {
                mesh.block([x - 0.03, -0.1, -0.05], [x + 0.03, -0.04, 0.36]);
            }
            mesh.block([-0.49, -0.1, 0.32], [0.49, -0.04, 0.38]);
            for x in [-0.3_f32, 0.0, 0.3] {
                mesh.frustum([x, 0.36], [-0.05, 0.5], [0.07, 0.05], 6);
            }
        }
        Shape::AirFactory => {
            mesh.paint(Material::Hull);
            mesh.block([-0.5, -0.5, -0.5], [0.5, 0.5, -0.38]);
            mesh.paint(Material::Trim);
            mesh.arch(0.32, [-0.44, 0.18], -0.38, 0.6, 8);
            mesh.paint(Material::Metal);
            mesh.block([-0.14, 0.18, -0.38], [0.14, 0.5, -0.36]);
            mesh.paint(Material::Glow);
            for y in [0.24_f32, 0.34, 0.44] {
                mesh.block([-0.13, y - 0.015, -0.36], [-0.1, y + 0.015, -0.35]);
                mesh.block([0.1, y - 0.015, -0.36], [0.13, y + 0.015, -0.35]);
            }
            mesh.paint(Material::Hull);
            mesh.frustum([0.42, 0.36], [-0.38, 0.3], [0.06, 0.05], 6);
            mesh.paint(Material::Glow);
            mesh.frustum([0.42, 0.36], [0.3, 0.42], [0.08, 0.06], 6);
        }
        Shape::AirFactoryT2 => {
            mesh.paint(Material::Hull);
            mesh.bevel([-0.5, -0.5, -0.5], [0.5, 0.5, -0.36], 0.03);
            mesh.paint(Material::Trim);
            mesh.arch(0.3, [-0.46, 0.2], -0.36, 0.66, 8);
            mesh.paint(Material::Hull);
            mesh.bevel([0.32, -0.46, -0.36], [0.48, 0.0, -0.1], 0.03);
            mesh.bevel([-0.48, -0.46, -0.36], [-0.32, 0.0, -0.1], 0.03);
            mesh.paint(Material::Metal);
            mesh.block([-0.14, 0.2, -0.36], [0.14, 0.5, -0.34]);
            mesh.paint(Material::Glow);
            for y in [0.26_f32, 0.38] {
                mesh.block([-0.13, y - 0.015, -0.34], [-0.1, y + 0.015, -0.33]);
                mesh.block([0.1, y - 0.015, -0.34], [0.13, y + 0.015, -0.33]);
            }
            mesh.paint(Material::Hull);
            mesh.frustum([0.42, 0.36], [-0.36, 0.38], [0.06, 0.05], 6);
            mesh.paint(Material::Glow);
            mesh.frustum([0.42, 0.36], [0.38, 0.5], [0.08, 0.06], 6);
        }
        Shape::AirFactoryT3 => {
            mesh.paint(Material::Hull);
            mesh.bevel([-0.5, -0.5, -0.5], [0.5, 0.5, -0.32], 0.04);
            mesh.paint(Material::Trim);
            mesh.arch(0.34, [-0.46, 0.22], -0.32, 0.7, 10);
            mesh.paint(Material::Glow);
            mesh.block([-0.3, -0.47, -0.3], [0.3, -0.46, -0.24]);
            mesh.paint(Material::Metal);
            mesh.block([-0.16, 0.22, -0.32], [0.16, 0.5, -0.3]);
            mesh.paint(Material::Glow);
            for y in [0.28_f32, 0.36, 0.44] {
                mesh.block([-0.15, y - 0.015, -0.3], [-0.12, y + 0.015, -0.29]);
                mesh.block([0.12, y - 0.015, -0.3], [0.15, y + 0.015, -0.29]);
            }
            // A tall tower with a radar dish.
            mesh.paint(Material::Hull);
            mesh.frustum([0.42, 0.36], [-0.32, 0.3], [0.07, 0.05], 6);
            mesh.paint(Material::Trim);
            mesh.frustum([0.42, 0.36], [0.3, 0.4], [0.08, 0.07], 6);
            mesh.paint(Material::Metal);
            mesh.frustum([-0.42, 0.38], [-0.32, 0.36], [0.03, 0.02], 6);
            mesh.cuboid(
                [Vec3::new(-0.49, 0.3, 0.32), Vec3::new(-0.35, 0.3, 0.32), Vec3::new(-0.35, 0.34, 0.32), Vec3::new(-0.49, 0.34, 0.32)],
                [Vec3::new(-0.49, 0.36, 0.48), Vec3::new(-0.35, 0.36, 0.48), Vec3::new(-0.35, 0.4, 0.48), Vec3::new(-0.49, 0.4, 0.48)],
            );
        }
        Shape::GeneratorT2 => {
            mesh.paint(Material::Hull);
            mesh.bevel([-0.5, -0.36, -0.5], [0.5, 0.36, -0.3], 0.04);
            for x in [-0.25_f32, 0.25] {
                mesh.paint(Material::Hull);
                mesh.frustum([x, 0.0], [-0.3, 0.2], [0.24, 0.13], 10);
                mesh.paint(Material::Trim);
                mesh.frustum([x, 0.0], [0.2, 0.5], [0.13, 0.18], 10);
            }
            mesh.paint(Material::Glow);
            mesh.frustum([0.0, 0.0], [-0.3, -0.05], [0.1, 0.07], 8);
        }
        Shape::GeneratorT3 => {
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.5, -0.2], [0.5, 0.46], 14);
            mesh.paint(Material::Glow);
            mesh.frustum([0.0, 0.0], [-0.2, -0.1], [0.4, 0.4], 14);
            // A dome in three steps.
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, 0.0], [-0.1, 0.15], [0.38, 0.32], 14);
            mesh.frustum([0.0, 0.0], [0.15, 0.35], [0.32, 0.2], 14);
            mesh.frustum([0.0, 0.0], [0.35, 0.5], [0.2, 0.0], 14);
            mesh.paint(Material::Metal);
            for (x, y) in [(0.36_f32, 0.36_f32), (-0.36, 0.36), (-0.36, -0.36), (0.36, -0.36)] {
                mesh.frustum([x, y], [-0.5, -0.05], [0.07, 0.06], 6);
            }
        }
        Shape::ExtractorT2 => {
            mesh.frustum([0.0, 0.0], [-0.5, -0.1], [0.5, 0.36], 6);
            mesh.paint(Material::Metal);
            for index in 0..3 {
                let angle = 0.5 + index as f32 / 3.0 * std::f32::consts::TAU;
                mesh.frustum([angle.cos() * 0.3, angle.sin() * 0.3], [-0.1, 0.3], [0.07, 0.05], 6);
            }
            mesh.paint(Material::Glow);
            mesh.frustum([0.0, 0.0], [-0.1, 0.5], [0.16, 0.0], 6);
        }
        Shape::ExtractorT3 => {
            mesh.frustum([0.0, 0.0], [-0.5, -0.05], [0.5, 0.4], 8);
            mesh.paint(Material::Glow);
            mesh.frustum([0.0, 0.0], [-0.05, 0.03], [0.42, 0.42], 12);
            mesh.paint(Material::Metal);
            for (x, y) in [(0.27_f32, 0.27_f32), (-0.27, 0.27), (-0.27, -0.27), (0.27, -0.27)] {
                mesh.frustum([x, y], [0.03, 0.35], [0.06, 0.05], 6);
            }
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [0.03, 0.3], [0.22, 0.16], 8);
            mesh.paint(Material::Glow);
            mesh.frustum([0.0, 0.0], [0.3, 0.5], [0.12, 0.0], 8);
        }
        Shape::NavalFactoryT2 => {
            mesh.paint(Material::Hull);
            mesh.bevel([-0.5, -0.5, -0.5], [-0.3, 0.5, -0.12], 0.03);
            mesh.bevel([0.3, -0.5, -0.5], [0.5, 0.5, -0.12], 0.03);
            mesh.paint(Material::Trim);
            mesh.block([-0.3, -0.5, -0.5], [0.3, -0.35, 0.05]);
            for y in [-0.15_f32, 0.25] {
                mesh.block([-0.5, y - 0.08, 0.25], [0.5, y + 0.08, 0.38]);
                mesh.paint(Material::Metal);
                mesh.block([-0.48, y - 0.06, -0.12], [-0.38, y + 0.06, 0.25]);
                mesh.block([0.38, y - 0.06, -0.12], [0.48, y + 0.06, 0.25]);
                mesh.paint(Material::Trim);
            }
            mesh.paint(Material::Metal);
            mesh.frustum([0.4, 0.42], [-0.12, 0.45], [0.05, 0.04], 6);
            mesh.beam([0.4, 0.42, 0.42], [0.1, 0.42, 0.42], 0.025, 0.025);
        }
        Shape::NavalFactoryT3 => {
            mesh.paint(Material::Hull);
            mesh.bevel([-0.5, -0.5, -0.5], [-0.28, 0.5, -0.05], 0.04);
            mesh.bevel([0.28, -0.5, -0.5], [0.5, 0.5, -0.05], 0.04);
            mesh.paint(Material::Glow);
            for x in [-0.39_f32, 0.39] {
                mesh.block([x - 0.02, -0.46, -0.05], [x + 0.02, 0.46, -0.04]);
            }
            mesh.paint(Material::Trim);
            mesh.bevel([-0.28, -0.5, -0.5], [0.28, -0.32, 0.1], 0.03);
            for y in [-0.12_f32, 0.24] {
                mesh.block([-0.5, y - 0.08, 0.28], [0.5, y + 0.08, 0.4]);
                mesh.paint(Material::Metal);
                mesh.block([-0.5, y - 0.06, -0.05], [-0.42, y + 0.06, 0.28]);
                mesh.block([0.42, y - 0.06, -0.05], [0.5, y + 0.06, 0.28]);
                mesh.paint(Material::Trim);
            }
            mesh.paint(Material::Hull);
            mesh.frustum([-0.39, 0.42], [-0.05, 0.38], [0.06, 0.05], 6);
            mesh.paint(Material::Glow);
            mesh.frustum([-0.39, 0.42], [0.38, 0.5], [0.07, 0.05], 6);
        }
        Shape::AirStaging => {
            mesh.paint(Material::Hull);
            mesh.block([-0.5, -0.5, -0.5], [0.5, 0.5, -0.3]);
            mesh.paint(Material::Trim);
            for (x, y) in [(-0.25_f32, -0.25_f32), (0.25, -0.25), (-0.25, 0.25), (0.25, 0.25)] {
                mesh.frustum([x, y], [-0.3, -0.24], [0.2, 0.2], 12);
            }
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, 0.0], [-0.3, 0.4], [0.07, 0.05], 6);
            mesh.paint(Material::Glow);
            mesh.block([-0.1, -0.1, 0.4], [0.1, 0.1, 0.5]);
        }
        Shape::AntiAirGun => {
            mesh.block([-0.25, -0.25, -0.1], [0.25, 0.2, 0.22]);
            mesh.paint(Material::Metal);
            for x in [-0.12_f32, 0.12] {
                mesh.beam([x, -0.05, 0.1], [x, 0.38, 0.44], 0.05, 0.05);
            }
        }
        Shape::Gateway => {
            mesh.paint(Material::Hull);
            mesh.block([-0.5, -0.5, -0.5], [0.5, 0.5, -0.35]);
            mesh.paint(Material::Metal);
            for (x, y) in [(-0.38_f32, -0.38_f32), (0.38, -0.38), (-0.38, 0.38), (0.38, 0.38)] {
                mesh.frustum([x, y], [-0.35, 0.3], [0.09, 0.07], 6);
            }
            mesh.paint(Material::Glow);
            mesh.frustum([0.0, 0.0], [-0.35, -0.05], [0.26, 0.18], 12);
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, 0.0], [0.3, 0.4], [0.48, 0.42], 16);
        }
        // Rings from the rim at the bottom of the box up to a point at the top, with no base,
        // so that it can be drawn over whatever it covers.
        Shape::Dome => {
            const SIDES: usize = 32;
            const RINGS: usize = 10;
            let at = |ring: usize, side: usize| {
                let up = ring as f32 / RINGS as f32 * std::f32::consts::FRAC_PI_2;
                let around = 0.1 + side as f32 / SIDES as f32 * std::f32::consts::TAU;
                Vec3::new(0.5 * up.cos() * around.cos(), 0.5 * up.cos() * around.sin(), -0.5 + up.sin())
            };
            let center = Vec3::new(0.0, 0.0, -0.5);
            for ring in 0..RINGS {
                for side in 0..SIDES {
                    let next = (side + 1) % SIDES;
                    if ring + 1 == RINGS {
                        mesh.face(&[at(ring, side), at(ring, next), Vec3::new(0.0, 0.0, 0.5)], center);
                    } else {
                        mesh.face(&[at(ring, side), at(ring, next), at(ring + 1, next), at(ring + 1, side)], center);
                    }
                }
            }
        }
        Shape::SelectionRing => {
            const SEGMENTS: usize = 24;
            const INNER: f32 = 0.42;
            let at = |index: usize, radius: f32, z: f32| {
                let angle = 0.13 + index as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
                Vec3::new(angle.cos() * radius, angle.sin() * radius, z)
            };
            for index in 0..SEGMENTS {
                let next = (index + 1) % SEGMENTS;
                let corners = |z: f32| [at(index, INNER, z), at(next, INNER, z), at(next, 0.5, z), at(index, 0.5, z)];
                mesh.cuboid(corners(-0.5), corners(0.5));
            }
        }
        Shape::SelectionBox => {
            const THICKNESS: f32 = 0.05;
            let edge = 0.5 - THICKNESS;
            mesh.block([-0.5, -0.5, -0.5], [0.5, -edge, 0.5]);
            mesh.block([-0.5, edge, -0.5], [0.5, 0.5, 0.5]);
            mesh.block([-0.5, -edge, -0.5], [-edge, edge, 0.5]);
            mesh.block([edge, -edge, -0.5], [0.5, edge, 0.5]);
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
            mesh.paint(Material::Trim);
            build_shape(shape, &mut mesh);
            ranges.push(start..mesh.indices.len() as u32);
        }
        assert!(mesh.vertices.len() < usize::from(u16::MAX), "too many vertices for 16-bit indices");
        Self { vertices: mesh.vertices, indices: mesh.indices, ranges }
    }
}

#[cfg(test)]
pub(super) mod tests {
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

    /// Instances are batched by `shape as usize`, in the order of `Shape::ALL`.
    #[test]
    fn all_shapes_are_listed_in_order() {
        for (index, shape) in Shape::ALL.iter().enumerate() {
            assert_eq!(*shape as usize, index, "{shape:?} is out of place in Shape::ALL");
        }
    }

    /// Which materials a shape uses, for tests elsewhere.
    pub fn materials(set: &MeshSet, shape: Shape) -> Vec<Material> {
        let mut found = Vec::new();
        for vertex in shape_vertices(set, shape as usize) {
            let material = [Material::Trim, Material::Hull, Material::Metal, Material::Glow][vertex.material as usize];
            if !found.contains(&material) {
                found.push(material);
            }
        }
        found
    }

    #[test]
    fn every_vertex_has_a_known_material() {
        let set = MeshSet::build();
        assert!(set.vertices.iter().all(|vertex| vertex.material <= Material::Glow as u32));
    }

    #[test]
    fn units_cast_shadows_but_markers_and_domes_do_not() {
        for shape in [Shape::Commander, Shape::Tank, Shape::Factory, Shape::Battleship, Shape::Gunship, Shape::Tree, Shape::Cube] {
            assert!(shape.casts_shadow(), "{shape:?} should cast a shadow");
        }
        for shape in [Shape::Dome, Shape::SelectionRing, Shape::SelectionBox, Shape::Shell] {
            assert!(!shape.casts_shadow(), "{shape:?} should not cast a shadow");
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
