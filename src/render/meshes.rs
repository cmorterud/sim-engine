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
    /// Scaffolding around a structure being built: posts at the corners and rails between them.
    Scaffold,
    /// A ball of fire, glowing, for explosions.
    Fireball,
    /// A lumpy cloud of three balls, for smoke and missile trails.
    Smoke,
    /// A flat ring of foam on the water, for ships' wakes.
    Wake,
    /// A flat square laid on the ground, for craters and scorch marks, which the shader cuts round.
    Scar,
    /// A heavy fighter with two engines and two tail fins, pointing along +Y.
    AirSuperiority,
    /// A long gunship with two rotors, pointing along +Y.
    HeavyGunship,
    /// A light four-wheeled car with a radar mast and a small gun.
    LandScout,
    /// A bot's upper body with a gatling arm and a grenade launcher on its shoulder.
    GatlingBot,
    /// A shield emitter on a tracked hull: a mast with a glowing ring and an orb.
    ShieldEmitter,
    /// A broad armored bot's upper body, with a cannon in each arm and shield emitters.
    HeavyAssaultBot,
    /// A bulky bot's upper body, with one heavy cannon over its shoulder.
    ArmoredAssaultBot,
    /// A low, fast boat with torpedo tubes along its sides.
    TorpedoBoat,
    /// A gatling turret for the heavy point defense.
    HeavyPointDefense,
    /// A wall section: a block with crenellations along the top.
    Wall,
    /// An engineering station: a pad with a tower, and a drone hovering over it.
    EngineeringStation,
    /// A shield's dome: an open half-sphere, drawn see-through.
    Dome,
    /// Flat, around the feet of a selected mobile unit.
    SelectionRing,
    /// Flat, around the footprint of a selected structure.
    SelectionBox,
}

impl Shape {
    pub const ALL: [Self; 80] = [
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
        Self::Scaffold,
        Self::Fireball,
        Self::Smoke,
        Self::Wake,
        Self::Scar,
        Self::AirSuperiority,
        Self::HeavyGunship,
        Self::LandScout,
        Self::GatlingBot,
        Self::ShieldEmitter,
        Self::HeavyAssaultBot,
        Self::ArmoredAssaultBot,
        Self::TorpedoBoat,
        Self::HeavyPointDefense,
        Self::Wall,
        Self::EngineeringStation,
        Self::Dome,
        Self::SelectionRing,
        Self::SelectionBox,
    ];
    pub const COUNT: usize = Self::ALL.len();

    /// Whether the shape throws a shadow on the ground. Markers, see-through domes, and shells
    /// too small to see do not.
    pub const fn casts_shadow(self) -> bool {
        !matches!(self, Self::Dome | Self::SelectionRing | Self::SelectionBox | Self::Shell | Self::Fireball | Self::Smoke | Self::Wake | Self::Scar)
    }

    /// How many turns a second the shape's spinning parts make, if it has any.
    pub const fn spin_rate(self) -> Option<f32> {
        match self {
            Self::Gunship => Some(2.2),
            Self::HeavyGunship => Some(1.7),
            Self::Radar => Some(0.25),
            Self::AirFactoryT3 => Some(0.3),
            Self::LandScout => Some(0.6),
            _ => None,
        }
    }

    /// For a factory, how high the floor of its bay is, as a share of its height: where what it
    /// builds stands while it is built. A dock builds ships afloat, at its foot.
    pub const fn bay_floor(self) -> Option<f32> {
        match self {
            Self::Factory | Self::FactoryT2 | Self::FactoryT3 => Some(0.06),
            Self::AirFactory | Self::AirFactoryT2 | Self::AirFactoryT3 => Some(0.07),
            Self::NavalFactory | Self::NavalFactoryT2 | Self::NavalFactoryT3 => Some(0.0),
            _ => None,
        }
    }

    /// Whether the shape is a hull that leaves a wake behind it on the water.
    pub const fn leaves_wake(self) -> bool {
        matches!(self, Self::Frigate | Self::Destroyer | Self::Cruiser | Self::Battleship | Self::Battlecruiser | Self::Submarine | Self::MissileSubmarine | Self::ShieldBoat | Self::TorpedoBoat)
    }

    /// Whether the shape is a pair of legs, which swing as the unit walks.
    pub const fn walks(self) -> bool {
        matches!(self, Self::CommanderLegs | Self::BotLegs)
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

/// How a part moves, as flags above a vertex's material. Each moves about the vertex's pivot,
/// driven by the instance's phase (the hull color's last byte) or action (the glow's last byte).
pub mod motion {
    /// Turns about the vertical line through the pivot, once per phase.
    pub const SPIN: u32 = 1 << 8;
    /// A leg, swinging about the hip at the pivot, forward on one side as it goes back on the
    /// other, through one stride per phase.
    pub const SWING: u32 = 1 << 9;
    /// A gun barrel, kicked back along -Y as it fires, by the action.
    pub const RECOIL: u32 = 1 << 10;
    /// A muzzle flash, which shows about the pivot only just after firing.
    pub const FLASH: u32 = 1 << 11;
    /// Glows fully only while the unit is at work, and dimly when idle.
    pub const PULSE: u32 = 1 << 12;
    /// The bits that hold the material.
    pub const MATERIAL_MASK: u32 = 0xff;
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// A `Material` in the low byte, with any `motion` flags above it.
    pub material: u32,
    /// The point a moving part moves about.
    pub pivot: [f32; 3],
}

#[derive(Default)]
struct MeshBuilder {
    vertices: Vec<MeshVertex>,
    indices: Vec<u16>,
    /// What the parts added next are made of.
    material: Material,
    /// How the parts added next move, and about where.
    motion: u32,
    pivot: Vec3,
}

impl MeshBuilder {
    fn paint(&mut self, material: Material) {
        self.material = material;
    }

    /// Adds the parts `draw` makes as moving parts, then goes back to still ones.
    fn moving(&mut self, motion: u32, pivot: [f32; 3], draw: impl FnOnce(&mut Self)) {
        let (before, before_pivot) = (self.motion, self.pivot);
        self.motion = motion;
        self.pivot = Vec3::from(pivot);
        draw(self);
        self.motion = before;
        self.pivot = before_pivot;
    }

    /// A low-poly ball, from pole to pole in `rings` bands of `sides` faces.
    fn ball(&mut self, center: [f32; 3], radius: f32, sides: usize, rings: usize) {
        let middle = Vec3::from(center);
        let at = |ring: usize, side: usize| {
            let up = -std::f32::consts::FRAC_PI_2 + ring as f32 / rings as f32 * std::f32::consts::PI;
            let around = 0.3 + side as f32 / sides as f32 * std::f32::consts::TAU;
            middle + Vec3::new(up.cos() * around.cos(), up.cos() * around.sin(), up.sin()) * radius
        };
        for ring in 0..rings {
            for side in 0..sides {
                let next = (side + 1) % sides;
                if ring == 0 {
                    self.face(&[at(0, 0), at(1, next), at(1, side)], middle);
                } else if ring + 1 == rings {
                    self.face(&[at(ring, side), at(ring, next), at(rings, 0)], middle);
                } else {
                    self.face(&[at(ring, side), at(ring, next), at(ring + 1, next), at(ring + 1, side)], middle);
                }
            }
        }
    }

    /// A muzzle flash around a gun's tip, which shows only just after it fires.
    fn flash(&mut self, at: [f32; 3], half: f32) {
        let before = self.material;
        self.paint(Material::Glow);
        let min = at.map(|value| (value - half).max(-0.5));
        let max = at.map(|value| (value + half).min(0.5));
        self.moving(motion::FLASH, at, |mesh| mesh.block(min, max));
        self.paint(before);
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
            self.vertices.push(MeshVertex { position: point.to_array(), normal: normal.to_array(), material: self.material as u32 | self.motion, pivot: self.pivot.to_array() });
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

    /// A ship's hull from stern to bow along Y, `half_width` either side, straight sides up to
    /// `bow_start` and then a point. Narrower at the keel, with a dark band at the waterline and
    /// a raised forecastle from `forecastle` forward. The deck is at -0.2, and -0.16 on the forecastle.
    fn hull(&mut self, half_width: f32, bow_start: f32, forecastle: f32) {
        let w = half_width;
        let outline = [[-w * 0.7, -0.5], [w * 0.7, -0.5], [w, -0.38], [w, bow_start], [0.015, 0.5], [-0.015, 0.5], [-w, bow_start], [-w, -0.38]];
        self.paint(Material::Metal);
        self.prism(&outline, [-0.5, -0.46], [0.62, 0.67]);
        self.paint(Material::Hull);
        self.prism(&outline, [-0.46, -0.2], [0.67, 1.0]);
        let fore = [[-w, forecastle], [w, forecastle], [w, bow_start], [0.015, 0.5], [-0.015, 0.5], [-w, bow_start]];
        self.prism(&fore, [-0.2, -0.16], [1.0, 1.0]);
    }

    /// A rounded body along Y, such as a fuselage or an engine, through cross-sections of
    /// `sides` sides, each `[y, half width, half height, height of its middle]`, centered on `x`.
    fn body(&mut self, x: f32, stations: &[[f32; 4]], sides: usize) {
        let ring = |station: [f32; 4]| {
            (0..sides)
                .map(|index| {
                    let angle = index as f32 / sides as f32 * std::f32::consts::TAU;
                    Vec3::new(x + angle.cos() * station[1], station[0], station[3] + angle.sin() * station[2])
                })
                .collect::<Vec<_>>()
        };
        for pair in stations.windows(2) {
            let (from, to) = (ring(pair[0]), ring(pair[1]));
            let middle = Vec3::new(x, (pair[0][0] + pair[1][0]) * 0.5, (pair[0][3] + pair[1][3]) * 0.5);
            // As triangles, since neighbouring cross-sections need not be the same shape.
            for index in 0..sides {
                let next = (index + 1) % sides;
                self.face(&[from[index], from[next], to[next]], middle);
                self.face(&[from[index], to[next], to[index]], middle);
            }
        }
        for station in [stations[0], stations[stations.len() - 1]] {
            let middle = Vec3::new(x, (stations[0][0] + stations[stations.len() - 1][0]) * 0.5, station[3]);
            self.face(&ring(station), middle);
        }
    }

    /// A pair of wings, from roots `root_x` either side of the middle out to tips at `tip_x`.
    /// Each chord is `[trailing edge, leading edge]` along Y, and each wing thins from its root's
    /// half thickness to its tip's, about the height `z`.
    fn wings(&mut self, root_x: f32, tip_x: f32, root_chord: [f32; 2], tip_chord: [f32; 2], half_thickness: [f32; 2], z: f32) {
        for side in [-1.0_f32, 1.0] {
            let (root, tip) = (root_x * side, tip_x * side);
            let corners = |up: f32| {
                [
                    Vec3::new(root, root_chord[0], z + up * half_thickness[0]),
                    Vec3::new(tip, tip_chord[0], z + up * half_thickness[1]),
                    Vec3::new(tip, tip_chord[1], z + up * half_thickness[1]),
                    Vec3::new(root, root_chord[1], z + up * half_thickness[0]),
                ]
            };
            self.cuboid(corners(-1.0), corners(1.0));
        }
    }

    /// An upright fin at `x`, from its root chord at height `root_z` up to its tip chord at
    /// `tip_z`, each `[trailing edge, leading edge]` along Y.
    fn fin(&mut self, x: f32, root_chord: [f32; 2], root_z: f32, tip_chord: [f32; 2], tip_z: f32, half_thickness: f32) {
        let corners = |chord: [f32; 2], z: f32| {
            [
                Vec3::new(x - half_thickness, chord[0], z),
                Vec3::new(x + half_thickness, chord[0], z),
                Vec3::new(x + half_thickness, chord[1], z),
                Vec3::new(x - half_thickness, chord[1], z),
            ]
        };
        self.cuboid(corners(root_chord, root_z), corners(tip_chord, tip_z));
    }

    /// A gun turret: a team-colored drum with barrels pointing along +Y, or -Y if `aft`.
    fn turret(&mut self, at: [f32; 2], z: [f32; 2], radius: f32, barrels: usize, length: f32, aft: bool) {
        self.paint(Material::Trim);
        self.frustum(at, z, [radius, radius * 0.8], 8);
        self.paint(Material::Metal);
        let direction = if aft { -1.0 } else { 1.0 };
        let height = (z[0] + z[1]) * 0.5;
        // Guns forward kick back and flash as they fire. Recoil runs along -Y, so guns aft keep still.
        let motion = if aft { 0 } else { motion::RECOIL };
        for index in 0..barrels {
            let x = at[0] + (index as f32 - (barrels as f32 - 1.0) * 0.5) * radius * 0.6;
            let tip = at[1] + direction * (radius + length);
            self.moving(motion, [x, at[1], height], |mesh| mesh.beam([x, at[1], height], [x, tip, height], radius * 0.14, radius * 0.14));
            if !aft {
                self.flash([x, tip, height], radius * 0.3);
            }
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
        // A broad walking figure, the one unit that must never be missed: a team-colored chest
        // with a glowing core, a backpack, shoulder pads, an arm either side with a gun in the
        // right, and a helmet with a glowing visor.
        Shape::Commander => {
            mesh.prism(&[[-0.26, -0.15], [0.26, -0.15], [0.3, 0.04], [0.2, 0.2], [-0.2, 0.2], [-0.3, 0.04]], [-0.08, 0.3], [0.75, 1.0]);
            mesh.paint(Material::Glow);
            mesh.block([-0.07, 0.17, 0.08], [0.07, 0.215, 0.2]);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.17, -0.3, 0.0], [0.17, -0.13, 0.3], 0.04);
            mesh.paint(Material::Glow);
            for x in [-0.08_f32, 0.08] {
                mesh.block([x - 0.035, -0.315, 0.07], [x + 0.035, -0.295, 0.24]);
            }
            mesh.paint(Material::Hull);
            for x in [-0.37_f32, 0.37] {
                mesh.bevel([x - 0.09, -0.13, 0.16], [x + 0.09, 0.13, 0.34], 0.06);
                mesh.beam([x, 0.0, 0.18], [x, 0.08, 0.02], 0.06, 0.06);
            }
            mesh.bevel([-0.43, 0.06, -0.08], [-0.31, 0.24, 0.04], 0.03);
            mesh.paint(Material::Metal);
            mesh.bevel([0.31, 0.0, -0.04], [0.45, 0.28, 0.1], 0.03);
            mesh.moving(motion::RECOIL, [0.38, 0.0, 0.03], |mesh| mesh.body(0.38, &[[0.26, 0.04, 0.04, 0.03], [0.5, 0.032, 0.032, 0.03]], 6));
            mesh.flash([0.38, 0.47, 0.03], 0.06);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.11, -0.08, 0.3], [0.11, 0.12, 0.48], 0.04);
            mesh.paint(Material::Glow);
            mesh.block([-0.09, 0.12, 0.37], [0.09, 0.14, 0.43]);
        }
        // A pelvis and two legs, each a thigh, a shin, and a foot pointing the way it walks.
        Shape::CommanderLegs => {
            mesh.paint(Material::Hull);
            mesh.bevel([-0.21, -0.12, -0.16], [0.21, 0.1, -0.06], 0.03);
            for x in [-0.17_f32, 0.17] {
                mesh.moving(motion::SWING, [x, 0.0, -0.12], |mesh| {
                    mesh.paint(Material::Hull);
                    mesh.beam([x, 0.0, -0.12], [x, 0.06, -0.3], 0.075, 0.07);
                    mesh.paint(Material::Metal);
                    mesh.beam([x, 0.06, -0.3], [x, -0.02, -0.45], 0.06, 0.055);
                    mesh.block([x - 0.05, 0.02, -0.34], [x + 0.05, 0.12, -0.26]);
                    mesh.paint(Material::Hull);
                    mesh.bevel([x - 0.08, -0.1, -0.5], [x + 0.08, 0.17, -0.42], 0.025);
                });
            }
        }
        // Tracks with fenders either side of a hull with a sloped front, and a team-colored deck.
        Shape::EngineerHull => {
            mesh.paint(Material::Metal);
            for (low, high) in [(-0.46_f32, -0.26_f32), (0.26, 0.46)] {
                mesh.bevel([low, -0.46, -0.5], [high, 0.44, -0.28], 0.04);
            }
            mesh.paint(Material::Hull);
            for (low, high) in [(-0.48_f32, -0.24_f32), (0.24, 0.48)] {
                mesh.block([low, -0.46, -0.28], [high, 0.44, -0.25]);
            }
            mesh.cuboid(
                [Vec3::new(-0.27, -0.42, -0.46), Vec3::new(0.27, -0.42, -0.46), Vec3::new(0.27, 0.42, -0.46), Vec3::new(-0.27, 0.42, -0.46)],
                [Vec3::new(-0.27, -0.42, -0.18), Vec3::new(0.27, -0.42, -0.18), Vec3::new(0.27, 0.2, -0.18), Vec3::new(-0.27, 0.2, -0.18)],
            );
            mesh.paint(Material::Trim);
            mesh.bevel([-0.23, -0.42, -0.18], [0.23, -0.12, -0.13], 0.02);
        }
        // A team-colored turntable carrying a jointed arm, with a tool at its end that glows at work.
        Shape::Engineer => {
            mesh.frustum([0.0, -0.12], [-0.12, 0.02], [0.24, 0.2], 8);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.12], [0.02, 0.1], [0.08, 0.07], 6);
            mesh.paint(Material::Hull);
            mesh.beam([0.0, -0.12, 0.06], [0.0, 0.12, 0.36], 0.055, 0.05);
            mesh.beam([0.0, 0.12, 0.36], [0.0, 0.36, 0.3], 0.045, 0.04);
            mesh.paint(Material::Metal);
            mesh.block([-0.05, 0.08, 0.3], [0.05, 0.16, 0.42]);
            mesh.frustum([0.0, 0.38], [0.3, 0.36], [0.05, 0.05], 6);
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [0.0, 0.38, 0.25], |mesh| mesh.frustum([0.0, 0.38], [0.18, 0.3], [0.03, 0.07], 6));
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
            mesh.moving(motion::RECOIL, [0.3, 0.18, 0.18], |mesh| mesh.beam([0.3, 0.18, 0.18], [0.3, 0.5, 0.18], 0.045, 0.045));
            mesh.flash([0.3, 0.47, 0.18], 0.05);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.1, -0.08, 0.36], [0.1, 0.1, 0.5], 0.03);
            mesh.paint(Material::Glow);
            mesh.block([-0.07, 0.1, 0.41], [0.07, 0.12, 0.45]);
        }
        // Two legs, each a thigh, a shin, and a foot.
        Shape::BotLegs => {
            mesh.paint(Material::Metal);
            mesh.block([-0.17, -0.08, -0.08], [0.17, 0.08, -0.02]);
            for x in [-0.2_f32, 0.2] {
                mesh.moving(motion::SWING, [x, 0.0, -0.05], |mesh| {
                    mesh.paint(Material::Metal);
                    mesh.beam([x, 0.0, -0.05], [x, 0.05, -0.27], 0.06, 0.055);
                    mesh.beam([x, 0.05, -0.27], [x, -0.02, -0.44], 0.05, 0.045);
                    mesh.paint(Material::Hull);
                    mesh.bevel([x - 0.07, -0.08, -0.5], [x + 0.07, 0.14, -0.43], 0.02);
                });
            }
        }
        // A hull with a sloped front and an engine grille, between tracks under fenders.
        Shape::TankHull => {
            mesh.paint(Material::Metal);
            for (low, high) in [(-0.48_f32, -0.24_f32), (0.24, 0.48)] {
                mesh.bevel([low, -0.5, -0.5], [high, 0.44, -0.24], 0.05);
            }
            mesh.paint(Material::Hull);
            for (low, high) in [(-0.5_f32, -0.22_f32), (0.22, 0.5)] {
                mesh.block([low, -0.5, -0.24], [high, 0.46, -0.21]);
            }
            mesh.cuboid(
                [Vec3::new(-0.3, -0.44, -0.44), Vec3::new(0.3, -0.44, -0.44), Vec3::new(0.3, 0.44, -0.44), Vec3::new(-0.3, 0.44, -0.44)],
                [Vec3::new(-0.3, -0.44, -0.1), Vec3::new(0.3, -0.44, -0.1), Vec3::new(0.3, 0.22, -0.1), Vec3::new(-0.3, 0.22, -0.1)],
            );
            mesh.paint(Material::Metal);
            for y in [-0.39_f32, -0.32, -0.25] {
                mesh.block([-0.18, y - 0.022, -0.1], [0.18, y + 0.022, -0.085]);
            }
        }
        // A team-colored turret with a sloped front, a mantlet, a long gun with a muzzle brake,
        // and a hatch.
        Shape::Tank => {
            mesh.prism(&[[-0.2, -0.28], [0.2, -0.28], [0.25, -0.05], [0.14, 0.12], [-0.14, 0.12], [-0.25, -0.05]], [-0.1, 0.12], [1.0, 0.82]);
            mesh.paint(Material::Metal);
            mesh.block([-0.08, 0.1, -0.04], [0.08, 0.16, 0.08]);
            mesh.moving(motion::RECOIL, [0.0, 0.14, 0.02], |mesh| {
                mesh.body(0.0, &[[0.14, 0.036, 0.036, 0.02], [0.42, 0.03, 0.03, 0.02]], 6);
                mesh.body(0.0, &[[0.42, 0.048, 0.048, 0.02], [0.49, 0.048, 0.048, 0.02]], 6);
            });
            mesh.flash([0.0, 0.47, 0.02], 0.06);
            mesh.paint(Material::Hull);
            mesh.frustum([0.08, -0.13], [0.12, 0.17], [0.06, 0.055], 8);
        }
        // A tracked carriage with spades at the back, to brace against the recoil.
        Shape::ArtilleryHull => {
            mesh.paint(Material::Metal);
            for (low, high) in [(-0.46_f32, -0.26_f32), (0.26, 0.46)] {
                mesh.bevel([low, -0.46, -0.5], [high, 0.32, -0.26], 0.04);
            }
            mesh.paint(Material::Hull);
            mesh.bevel([-0.28, -0.44, -0.46], [0.28, 0.34, -0.2], 0.04);
            mesh.paint(Material::Metal);
            for x in [-0.2_f32, 0.2] {
                mesh.beam([x, -0.4, -0.3], [x * 1.2, -0.45, -0.44], 0.035, 0.02);
            }
        }
        // A team-colored cradle with a heavy barrel raised high, and its recoil cylinder above it.
        Shape::Artillery => {
            mesh.bevel([-0.18, -0.38, -0.2], [0.18, 0.02, 0.04], 0.04);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.18], [0.04, 0.12], [0.1, 0.09], 8);
            mesh.moving(motion::RECOIL, [0.0, -0.22, 0.0], |mesh| {
                mesh.beam([0.0, -0.22, 0.0], [0.0, 0.42, 0.42], 0.065, 0.065);
                mesh.beam([0.0, -0.12, 0.09], [0.0, 0.18, 0.29], 0.035, 0.035);
            });
            mesh.flash([0.0, 0.42, 0.42], 0.07);
        }
        // An open-air assembly bay: a floor open at both ends, east and west, between long walls,
        // under a gantry crane, so that what it builds can be seen and drives out either end.
        Shape::Factory => {
            // The floor of the bay, and ramps down from both of its open ends, east and west.
            mesh.paint(Material::Hull);
            mesh.block([-0.42, -0.5, -0.5], [0.42, 0.5, -0.44]);
            for side in [-1.0_f32, 1.0] {
                let (inner, outer) = (0.42 * side, 0.5 * side);
                let (low, high) = (inner.min(outer), inner.max(outer));
                let z = |x: f32| if (x - inner).abs() < 1e-6 { -0.44 } else { -0.495 };
                mesh.cuboid(
                    [Vec3::new(low, -0.34, -0.5), Vec3::new(high, -0.34, -0.5), Vec3::new(high, 0.34, -0.5), Vec3::new(low, 0.34, -0.5)],
                    [Vec3::new(low, -0.34, z(low)), Vec3::new(high, -0.34, z(high)), Vec3::new(high, 0.34, z(high)), Vec3::new(low, 0.34, z(low))],
                );
            }
            // Guide lights along the floor, lit while it builds.
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| {
                for y in [-0.33_f32, 0.33] {
                    mesh.block([-0.4, y - 0.01, -0.44], [0.4, y + 0.01, -0.43]);
                }
            });

            // Walls down the long sides, north and south, with the team's stripe along the top.
            for y in [-0.5_f32, 0.36] {
                mesh.paint(Material::Hull);
                mesh.bevel([-0.44, y, -0.44], [0.44, y + 0.14, -0.14], 0.03);
                mesh.paint(Material::Trim);
                mesh.block([-0.44, y, -0.14], [0.44, y + 0.14, -0.1]);
            }
            // A gantry crane across the bay, with its hoist over the middle.
            mesh.paint(Material::Metal);
            for y in [-0.47_f32, 0.39] {
                mesh.block([-0.04, y, -0.1], [0.04, y + 0.08, 0.3]);
            }
            mesh.block([-0.05, -0.47, 0.26], [0.05, 0.47, 0.34]);
            mesh.paint(Material::Trim);
            mesh.block([-0.07, -0.08, 0.14], [0.07, 0.08, 0.26]);
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| mesh.block([-0.025, -0.025, 0.09], [0.025, 0.025, 0.14]));
            // A smokestack at the west end of the north wall.
            mesh.paint(Material::Metal);
            mesh.frustum([-0.36, 0.43], [-0.1, 0.42], [0.05, 0.04], 6);
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
        // A stepped plinth with a team-colored ring, for turrets to stand on.
        Shape::PointDefenseBase => {
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.5, -0.32], [0.48, 0.44], 8);
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, 0.0], [-0.32, -0.25], [0.42, 0.4], 8);
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.25, -0.05], [0.3, 0.24], 8);
        }
        // A team-colored turret with a sloped front, a long gun, and a sensor with a glowing eye.
        Shape::LandScout => {
            mesh.paint(Material::Metal);
            for x in [-0.33_f32, 0.33] {
                for y in [-0.3_f32, 0.28] {
                    mesh.bevel([x - 0.08, y - 0.12, -0.5], [x + 0.08, y + 0.12, -0.26], 0.04);
                }
            }
            mesh.paint(Material::Trim);
            mesh.bevel([-0.26, -0.44, -0.38], [0.26, 0.42, -0.2], 0.05);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.18, -0.12, -0.2], [0.18, 0.2, -0.02], 0.04);
            mesh.paint(Material::Glow);
            mesh.block([-0.14, 0.2, -0.12], [0.14, 0.215, -0.06]);
            mesh.paint(Material::Metal);
            mesh.body(0.0, &[[0.2, 0.025, 0.025, -0.06], [0.46, 0.02, 0.02, -0.06]], 6);
            mesh.flash([0.0, 0.45, -0.06], 0.04);
            mesh.frustum([0.0, -0.28], [-0.2, 0.3], [0.025, 0.02], 6);
            mesh.moving(motion::SPIN, [0.0, -0.28, 0.0], |mesh| {
                mesh.cuboid(
                    [Vec3::new(-0.16, -0.3, 0.26), Vec3::new(0.16, -0.3, 0.26), Vec3::new(0.16, -0.27, 0.26), Vec3::new(-0.16, -0.27, 0.26)],
                    [Vec3::new(-0.16, -0.26, 0.4), Vec3::new(0.16, -0.26, 0.4), Vec3::new(0.16, -0.23, 0.4), Vec3::new(-0.16, -0.23, 0.4)],
                );
            });
        }
        Shape::GatlingBot => {
            mesh.bevel([-0.22, -0.18, -0.02], [0.22, 0.18, 0.34], 0.05);
            mesh.paint(Material::Hull);
            mesh.bevel([0.22, -0.12, 0.08], [0.38, 0.2, 0.3], 0.04);
            mesh.paint(Material::Metal);
            mesh.moving(motion::RECOIL, [0.3, 0.2, 0.19], |mesh| {
                for (dx, dz) in [(-0.035_f32, 0.0_f32), (0.035, 0.0), (0.0, 0.05)] {
                    mesh.body(0.3 + dx, &[[0.2, 0.02, 0.02, 0.17 + dz], [0.49, 0.018, 0.018, 0.17 + dz]], 5);
                }
            });
            mesh.flash([0.3, 0.48, 0.19], 0.06);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.38, -0.12, 0.22], [-0.2, 0.14, 0.38], 0.03);
            mesh.paint(Material::Metal);
            mesh.body(-0.29, &[[0.14, 0.045, 0.045, 0.3], [0.26, 0.045, 0.045, 0.3]], 6);
            mesh.paint(Material::Hull);
            mesh.beam([-0.3, 0.0, 0.18], [-0.3, 0.2, 0.08], 0.05, 0.05);
            mesh.bevel([-0.1, -0.06, 0.34], [0.1, 0.12, 0.46], 0.03);
            mesh.paint(Material::Glow);
            mesh.block([-0.08, 0.12, 0.38], [0.08, 0.135, 0.42]);
        }
        Shape::ShieldEmitter => {
            mesh.bevel([-0.26, -0.32, -0.2], [0.26, 0.3, 0.0], 0.04);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.02], [0.0, 0.32], [0.06, 0.045], 8);
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| {
                mesh.frustum([0.0, -0.02], [0.16, 0.2], [0.24, 0.24], 14);
                mesh.ball([0.0, -0.02, 0.42], 0.07, 8, 4);
            });
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, -0.02], [0.3, 0.38], [0.12, 0.05], 8);
        }
        Shape::HeavyAssaultBot => {
            mesh.prism(&[[-0.3, -0.18], [0.3, -0.18], [0.34, 0.04], [0.22, 0.22], [-0.22, 0.22], [-0.34, 0.04]], [-0.06, 0.3], [0.8, 1.0]);
            mesh.paint(Material::Hull);
            for x in [-0.4_f32, 0.4] {
                mesh.bevel([x - 0.1, -0.14, 0.04], [x + 0.1, 0.16, 0.26], 0.05);
            }
            mesh.paint(Material::Metal);
            mesh.moving(motion::RECOIL, [0.0, 0.16, 0.14], |mesh| {
                for x in [-0.4_f32, 0.4] {
                    mesh.body(x, &[[0.16, 0.05, 0.05, 0.14], [0.48, 0.042, 0.042, 0.14]], 6);
                }
            });
            mesh.flash([0.4, 0.47, 0.14], 0.07);
            mesh.paint(Material::Glow);
            for x in [-0.25_f32, 0.25] {
                mesh.ball([x, -0.04, 0.32], 0.055, 6, 3);
            }
            mesh.paint(Material::Hull);
            mesh.bevel([-0.1, -0.04, 0.3], [0.1, 0.14, 0.44], 0.03);
            mesh.paint(Material::Glow);
            mesh.block([-0.08, 0.14, 0.35], [0.08, 0.155, 0.4]);
        }
        Shape::ArmoredAssaultBot => {
            mesh.bevel([-0.3, -0.26, -0.04], [0.3, 0.2, 0.3], 0.06);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.27, 0.2, -0.02], [0.27, 0.27, 0.24], 0.03);
            mesh.bevel([-0.36, -0.14, 0.18], [-0.18, 0.1, 0.34], 0.03);
            mesh.paint(Material::Metal);
            mesh.moving(motion::RECOIL, [0.18, -0.1, 0.38], |mesh| {
                mesh.body(0.18, &[[-0.12, 0.085, 0.085, 0.38], [0.2, 0.08, 0.08, 0.38], [0.5, 0.06, 0.06, 0.38]], 8);
            });
            mesh.flash([0.18, 0.48, 0.38], 0.09);
            mesh.paint(Material::Glow);
            mesh.block([-0.22, 0.27, 0.12], [0.06, 0.28, 0.17]);
        }
        Shape::TorpedoBoat => {
            mesh.hull(0.13, 0.18, 0.05);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.09, -0.12, -0.2], [0.09, 0.12, -0.04], 0.03);
            mesh.paint(Material::Metal);
            for x in [-0.1_f32, 0.1] {
                mesh.body(x, &[[-0.32, 0.03, 0.03, -0.19], [0.06, 0.03, 0.03, -0.19]], 6);
            }
            mesh.frustum([0.0, -0.02], [-0.04, 0.26], [0.015, 0.01], 6);
            mesh.paint(Material::Trim);
            mesh.block([-0.1, -0.4, -0.2], [0.1, -0.28, -0.19]);
            mesh.paint(Material::Glow);
            mesh.ball([0.0, -0.02, 0.27], 0.025, 6, 3);
        }
        Shape::HeavyPointDefense => {
            mesh.prism(&[[-0.26, -0.3], [0.26, -0.3], [0.32, -0.02], [0.2, 0.18], [-0.2, 0.18], [-0.32, -0.02]], [-0.05, 0.3], [1.0, 0.85]);
            mesh.paint(Material::Metal);
            mesh.moving(motion::RECOIL, [0.0, 0.16, 0.12], |mesh| {
                for (dx, dz) in [(-0.045_f32, 0.0_f32), (0.045, 0.0), (0.0, 0.07), (0.0, -0.05)] {
                    mesh.body(dx, &[[0.16, 0.026, 0.026, 0.12 + dz], [0.48, 0.022, 0.022, 0.12 + dz]], 5);
                }
            });
            mesh.flash([0.0, 0.47, 0.12], 0.08);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.12, -0.24, 0.3], [0.12, 0.0, 0.42], 0.03);
            mesh.paint(Material::Glow);
            mesh.block([-0.1, 0.0, 0.34], [0.1, 0.012, 0.38]);
        }
        Shape::EngineeringStation => {
            mesh.paint(Material::Hull);
            mesh.bevel([-0.5, -0.5, -0.5], [0.5, 0.5, -0.38], 0.03);
            mesh.paint(Material::Trim);
            mesh.frustum([-0.18, -0.18], [-0.38, 0.12], [0.16, 0.12], 8);
            mesh.paint(Material::Metal);
            mesh.frustum([-0.18, -0.18], [0.12, 0.3], [0.04, 0.03], 6);
            mesh.beam([-0.18, -0.18, 0.28], [0.22, 0.18, 0.34], 0.025, 0.025);
            // The drone, over the pad, lit while it works.
            mesh.paint(Material::Hull);
            mesh.bevel([0.12, 0.08, 0.24], [0.34, 0.3, 0.32], 0.02);
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| {
                mesh.ball([0.23, 0.19, 0.21], 0.04, 6, 3);
                for (x, y) in [(-0.4_f32, -0.4_f32), (0.4, -0.4), (-0.4, 0.4), (0.4, 0.4)] {
                    mesh.block([x - 0.02, y - 0.02, -0.38], [x + 0.02, y + 0.02, -0.36]);
                }
            });
        }
        Shape::Wall => {
            mesh.paint(Material::Hull);
            mesh.bevel([-0.5, -0.5, -0.5], [0.5, 0.5, 0.25], 0.04);
            mesh.paint(Material::Trim);
            mesh.block([-0.5, -0.5, 0.25], [0.5, 0.5, 0.33]);
            mesh.paint(Material::Hull);
            for x in [-0.33_f32, 0.0, 0.33] {
                mesh.block([x - 0.1, -0.5, 0.33], [x + 0.1, 0.5, 0.5]);
            }
        }
        Shape::PointDefense => {
            mesh.prism(&[[-0.2, -0.26], [0.2, -0.26], [0.26, -0.02], [0.16, 0.16], [-0.16, 0.16], [-0.26, -0.02]], [-0.05, 0.28], [1.0, 0.85]);
            mesh.paint(Material::Metal);
            mesh.moving(motion::RECOIL, [0.0, 0.12, 0.12], |mesh| mesh.body(0.0, &[[0.12, 0.048, 0.048, 0.12], [0.47, 0.04, 0.04, 0.12]], 6));
            mesh.flash([0.0, 0.46, 0.12], 0.06);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.08, -0.2, 0.28], [0.08, 0.0, 0.4], 0.03);
            mesh.paint(Material::Glow);
            mesh.block([-0.06, 0.0, 0.32], [0.06, 0.012, 0.36]);
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
            mesh.moving(motion::SPIN, [0.0; 3], |mesh| {
                mesh.cuboid(
                    [Vec3::new(-0.45, -0.1, 0.1), Vec3::new(0.45, -0.1, 0.1), Vec3::new(0.45, 0.05, 0.1), Vec3::new(-0.45, 0.05, 0.1)],
                    [Vec3::new(-0.45, 0.1, 0.5), Vec3::new(0.45, 0.1, 0.5), Vec3::new(0.45, 0.25, 0.5), Vec3::new(-0.45, 0.25, 0.5)],
                );
            });
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
        // A fighter: a team-colored body tapering to a point, delta wings, and a swept fin.
        Shape::Aircraft => {
            mesh.body(0.0, &[[-0.46, 0.065, 0.05, 0.0], [-0.2, 0.09, 0.075, 0.0], [0.12, 0.085, 0.07, 0.0], [0.34, 0.05, 0.045, -0.005], [0.5, 0.006, 0.006, -0.01]], 6);
            mesh.paint(Material::Glow);
            mesh.body(0.0, &[[-0.5, 0.05, 0.04, 0.0], [-0.46, 0.062, 0.048, 0.0]], 6);
            mesh.paint(Material::Hull);
            mesh.wings(0.07, 0.5, [-0.42, 0.18], [-0.42, -0.3], [0.025, 0.006], -0.01);
            mesh.wings(0.05, 0.2, [-0.46, -0.32], [-0.47, -0.42], [0.01, 0.004], 0.0);
            mesh.paint(Material::Trim);
            mesh.fin(0.0, [-0.44, -0.22], 0.05, [-0.47, -0.37], 0.28, 0.012);
            mesh.paint(Material::Metal);
            mesh.body(0.0, &[[0.08, 0.02, 0.012, 0.065], [0.16, 0.042, 0.03, 0.07], [0.27, 0.03, 0.02, 0.058], [0.32, 0.006, 0.005, 0.05]], 6);
        }
        Shape::Scaffold => {
            mesh.paint(Material::Metal);
            const EDGE: f32 = 0.47;
            const POST: f32 = 0.03;
            for (x, y) in [(-EDGE, -EDGE), (EDGE, -EDGE), (EDGE, EDGE), (-EDGE, EDGE)] {
                mesh.block([x - POST, y - POST, -0.5], [x + POST, y + POST, 0.5]);
            }
            for z in [-0.12_f32, 0.26] {
                mesh.block([-EDGE, -EDGE - 0.015, z - 0.015], [EDGE, -EDGE + 0.015, z + 0.015]);
                mesh.block([-EDGE, EDGE - 0.015, z - 0.015], [EDGE, EDGE + 0.015, z + 0.015]);
                mesh.block([-EDGE - 0.015, -EDGE, z - 0.015], [-EDGE + 0.015, EDGE, z + 0.015]);
                mesh.block([EDGE - 0.015, -EDGE, z - 0.015], [EDGE + 0.015, EDGE, z + 0.015]);
            }
            // Team-colored caps on the posts, and a brace across each side.
            mesh.paint(Material::Trim);
            for (x, y) in [(-EDGE, -EDGE), (EDGE, -EDGE), (EDGE, EDGE), (-EDGE, EDGE)] {
                mesh.block([x - 0.03, y - 0.03, 0.44], [x + 0.03, y + 0.03, 0.5]);
            }
            mesh.paint(Material::Metal);
            mesh.beam([-EDGE + 0.02, -EDGE, -0.48], [EDGE - 0.02, -EDGE, 0.26], 0.012, 0.012);
            mesh.beam([EDGE - 0.02, EDGE, -0.48], [-EDGE + 0.02, EDGE, 0.26], 0.012, 0.012);
        }
        Shape::Fireball => {
            mesh.paint(Material::Glow);
            mesh.ball([0.0; 3], 0.5, 9, 6);
        }
        Shape::Smoke => {
            mesh.ball([-0.15, -0.12, -0.1], 0.3, 7, 5);
            mesh.ball([0.18, -0.05, -0.05], 0.3, 7, 5);
            mesh.ball([0.0, 0.17, 0.12], 0.32, 7, 5);
        }
        Shape::Wake => {
            const SEGMENTS: usize = 12;
            const INNER: f32 = 0.3;
            let at = |index: usize, radius: f32, z: f32| {
                let angle = 0.2 + index as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
                Vec3::new(angle.cos() * radius, angle.sin() * radius, z)
            };
            for index in 0..SEGMENTS {
                let next = (index + 1) % SEGMENTS;
                let corners = |z: f32| [at(index, INNER, z), at(next, INNER, z), at(next, 0.5, z), at(index, 0.5, z)];
                mesh.cuboid(corners(-0.5), corners(0.5));
            }
        }
        // A grid, so that it can bend over the ground it lies on.
        Shape::Scar => {
            const SQUARES: usize = 6;
            let at = |index: usize| index as f32 / SQUARES as f32 - 0.5;
            for y in 0..SQUARES {
                for x in 0..SQUARES {
                    let corners = [Vec3::new(at(x), at(y), 0.0), Vec3::new(at(x + 1), at(y), 0.0), Vec3::new(at(x + 1), at(y + 1), 0.0), Vec3::new(at(x), at(y + 1), 0.0)];
                    mesh.face(&corners, Vec3::new(0.0, 0.0, -1.0));
                }
            }
        }
        Shape::AirSuperiority => {
            mesh.body(0.0, &[[-0.38, 0.05, 0.045, 0.02], [-0.1, 0.075, 0.07, 0.02], [0.2, 0.07, 0.065, 0.0], [0.4, 0.04, 0.04, -0.01], [0.5, 0.006, 0.006, -0.015]], 6);
            for x in [-0.11_f32, 0.11] {
                mesh.paint(Material::Hull);
                mesh.body(x, &[[-0.44, 0.055, 0.045, 0.0], [-0.1, 0.065, 0.055, 0.0], [0.14, 0.04, 0.035, -0.01]], 6);
                mesh.paint(Material::Glow);
                mesh.body(x, &[[-0.5, 0.045, 0.036, 0.0], [-0.44, 0.055, 0.045, 0.0]], 6);
            }
            mesh.paint(Material::Hull);
            mesh.wings(0.15, 0.5, [-0.3, 0.12], [-0.3, -0.18], [0.022, 0.006], 0.0);
            mesh.wings(0.15, 0.33, [-0.48, -0.34], [-0.49, -0.43], [0.012, 0.004], 0.0);
            mesh.paint(Material::Trim);
            for x in [-0.11_f32, 0.11] {
                mesh.fin(x, [-0.42, -0.24], 0.04, [-0.46, -0.36], 0.26, 0.01);
            }
            mesh.paint(Material::Metal);
            mesh.body(0.0, &[[0.14, 0.02, 0.012, 0.065], [0.24, 0.045, 0.03, 0.07], [0.36, 0.03, 0.02, 0.05], [0.41, 0.006, 0.005, 0.04]], 6);
        }
        // A bomber: a thick team-colored body, straight wings with engines under them, and a glazed nose.
        Shape::Bomber => {
            mesh.body(0.0, &[[-0.5, 0.03, 0.03, 0.04], [-0.3, 0.09, 0.09, 0.01], [0.1, 0.14, 0.13, 0.0], [0.35, 0.11, 0.1, -0.01], [0.48, 0.04, 0.04, -0.02]], 6);
            mesh.paint(Material::Hull);
            mesh.wings(0.12, 0.5, [-0.06, 0.2], [0.0, 0.14], [0.03, 0.008], 0.02);
            mesh.wings(0.02, 0.24, [-0.5, -0.38], [-0.48, -0.42], [0.012, 0.004], 0.04);
            mesh.paint(Material::Trim);
            mesh.fin(0.0, [-0.48, -0.3], 0.07, [-0.5, -0.42], 0.3, 0.014);
            for x in [-0.3_f32, 0.3] {
                mesh.paint(Material::Metal);
                mesh.body(x, &[[-0.12, 0.045, 0.04, -0.03], [0.1, 0.055, 0.05, -0.03], [0.27, 0.035, 0.035, -0.03]], 6);
                mesh.paint(Material::Glow);
                mesh.body(x, &[[-0.17, 0.036, 0.03, -0.03], [-0.12, 0.045, 0.04, -0.03]], 6);
            }
            mesh.paint(Material::Metal);
            mesh.body(0.0, &[[0.28, 0.05, 0.025, 0.09], [0.37, 0.065, 0.04, 0.08], [0.45, 0.03, 0.02, 0.04]], 6);
        }
        // A gunship: a rounded body tapering into a tail boom, stub wings with rocket pods, a
        // canopy at the nose, and a rotor of four slim team-colored blades.
        Shape::Gunship => {
            mesh.paint(Material::Hull);
            mesh.body(0.0, &[[-0.12, 0.11, 0.1, -0.13], [0.05, 0.15, 0.14, -0.12], [0.28, 0.13, 0.12, -0.13], [0.42, 0.08, 0.08, -0.16], [0.49, 0.02, 0.02, -0.18]], 6);
            mesh.body(0.0, &[[-0.5, 0.025, 0.03, -0.07], [-0.12, 0.06, 0.06, -0.1]], 6);
            mesh.paint(Material::Trim);
            mesh.fin(0.0, [-0.5, -0.4], -0.06, [-0.5, -0.45], 0.1, 0.01);
            mesh.wings(0.02, 0.14, [-0.5, -0.43], [-0.49, -0.45], [0.008, 0.004], -0.07);
            mesh.bevel([-0.06, -0.08, -0.01], [0.06, 0.22, 0.03], 0.015);
            mesh.paint(Material::Hull);
            mesh.wings(0.12, 0.33, [-0.02, 0.12], [0.02, 0.1], [0.02, 0.01], -0.15);
            mesh.paint(Material::Metal);
            for x in [-0.31_f32, 0.31] {
                mesh.body(x, &[[-0.06, 0.028, 0.028, -0.19], [0.17, 0.034, 0.034, -0.19], [0.22, 0.016, 0.016, -0.19]], 6);
            }
            mesh.body(0.0, &[[0.26, 0.06, 0.03, -0.03], [0.36, 0.085, 0.05, -0.07], [0.46, 0.035, 0.025, -0.13]], 6);
            mesh.moving(motion::SPIN, [0.0; 3], |mesh| {
                mesh.paint(Material::Metal);
                mesh.frustum([0.0, 0.0], [0.03, 0.17], [0.05, 0.04], 6);
                mesh.paint(Material::Trim);
                for index in 0..4 {
                    let angle = std::f32::consts::FRAC_PI_4 + index as f32 * std::f32::consts::FRAC_PI_2;
                    let (x, y) = (angle.cos(), angle.sin());
                    mesh.beam([x * 0.06, y * 0.06, 0.14], [x * 0.47, y * 0.47, 0.14], 0.022, 0.007);
                }
            });
        }
        Shape::HeavyGunship => {
            mesh.paint(Material::Hull);
            mesh.body(0.0, &[[-0.48, 0.07, 0.07, -0.14], [-0.3, 0.14, 0.13, -0.14], [0.25, 0.14, 0.13, -0.14], [0.4, 0.09, 0.09, -0.17], [0.49, 0.025, 0.025, -0.2]], 6);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.06, -0.34, -0.02], [0.06, 0.32, 0.02], 0.015);
            mesh.fin(0.0, [-0.5, -0.4], -0.08, [-0.5, -0.45], 0.08, 0.012);
            mesh.paint(Material::Hull);
            mesh.wings(0.12, 0.34, [-0.1, 0.06], [-0.07, 0.04], [0.02, 0.01], -0.16);
            mesh.paint(Material::Metal);
            for x in [-0.32_f32, 0.32] {
                mesh.body(x, &[[-0.14, 0.03, 0.03, -0.2], [0.1, 0.036, 0.036, -0.2], [0.15, 0.018, 0.018, -0.2]], 6);
            }
            mesh.body(0.0, &[[0.28, 0.06, 0.03, -0.06], [0.37, 0.08, 0.045, -0.09], [0.46, 0.035, 0.025, -0.15]], 6);
            // A rotor at each end, on a mast.
            for y in [-0.26_f32, 0.24] {
                mesh.moving(motion::SPIN, [0.0, y, 0.0], |mesh| {
                    mesh.paint(Material::Metal);
                    mesh.frustum([0.0, y], [-0.02, 0.15], [0.045, 0.035], 6);
                    mesh.paint(Material::Trim);
                    for index in 0..4 {
                        let angle = std::f32::consts::FRAC_PI_4 + index as f32 * std::f32::consts::FRAC_PI_2;
                        let (x, dy) = (angle.cos(), angle.sin());
                        mesh.beam([x * 0.045, y + dy * 0.045, 0.13], [x * 0.24, y + dy * 0.24, 0.13], 0.018, 0.006);
                    }
                });
            }
        }
        Shape::StrategicBomber => {
            mesh.wings(0.12, 0.5, [-0.36, 0.3], [-0.42, -0.26], [0.05, 0.012], 0.0);
            mesh.paint(Material::Hull);
            mesh.body(0.0, &[[-0.4, 0.1, 0.04, 0.0], [-0.1, 0.15, 0.08, 0.01], [0.2, 0.13, 0.075, 0.0], [0.4, 0.07, 0.05, -0.005], [0.5, 0.008, 0.008, -0.01]], 6);
            for x in [-0.2_f32, 0.2] {
                mesh.paint(Material::Metal);
                mesh.body(x, &[[-0.3, 0.05, 0.032, 0.035], [-0.05, 0.055, 0.04, 0.045], [0.12, 0.03, 0.02, 0.03]], 6);
                mesh.paint(Material::Glow);
                mesh.body(x, &[[-0.35, 0.04, 0.026, 0.035], [-0.3, 0.05, 0.032, 0.035]], 6);
            }
            mesh.paint(Material::Metal);
            mesh.body(0.0, &[[0.18, 0.03, 0.015, 0.07], [0.26, 0.05, 0.03, 0.07], [0.36, 0.02, 0.012, 0.055]], 6);
        }
        Shape::FlakGun => {
            mesh.block([-0.28, -0.28, -0.1], [0.28, 0.2, 0.2]);
            mesh.paint(Material::Metal);
            mesh.moving(motion::RECOIL, [0.0, -0.05, 0.1], |mesh| mesh.beam([0.0, -0.05, 0.1], [0.0, 0.28, 0.38], 0.1, 0.1));
            mesh.flash([0.0, 0.3, 0.4], 0.08);
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
            mesh.moving(motion::RECOIL, [0.0, -0.1, 0.0], |mesh| mesh.beam([0.0, -0.1, 0.0], [0.0, 0.42, 0.4], 0.07, 0.07));
            mesh.flash([0.0, 0.42, 0.4], 0.07);
            mesh.paint(Material::Hull);
            mesh.block([-0.12, -0.45, 0.0], [0.12, -0.3, 0.12]);
        }
        // A dock: two piers with a slip of open water between them, open at both ends, under a crane.
        Shape::NavalFactory => {
            // Piers down the long sides, north and south, with open water between them: a slip
            // open at both ends, east and west, where ships are built afloat and sail out.
            mesh.paint(Material::Hull);
            for y in [-0.5_f32, 0.32] {
                mesh.bevel([-0.5, y, -0.5], [0.5, y + 0.18, -0.32], 0.03);
            }
            // Bollards along the piers' inner edges.
            mesh.paint(Material::Metal);
            for y in [-0.33_f32, 0.33] {
                for x in [-0.4_f32, -0.2, 0.0, 0.2, 0.4] {
                    mesh.frustum([x, y], [-0.32, -0.28], [0.015, 0.015], 6);
                }
            }
            // Buoy lights at the open ends.
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| {
                for x in [-0.48_f32, 0.48] {
                    for y in [-0.3_f32, 0.3] {
                        mesh.block([x - 0.02, y - 0.02, -0.34], [x + 0.02, y + 0.02, -0.3]);
                    }
                }
            });

            // A crane on rails along the piers, its jib across the slip.
            mesh.paint(Material::Metal);
            for y in [-0.45_f32, 0.37] {
                mesh.block([-0.04, y, -0.32], [0.04, y + 0.08, 0.2]);
            }
            mesh.block([-0.05, -0.45, 0.16], [0.05, 0.45, 0.24]);
            mesh.paint(Material::Trim);
            mesh.block([-0.07, -0.07, 0.06], [0.07, 0.07, 0.16]);
            // A dock office with the team's colors on the north pier.
            mesh.paint(Material::Trim);
            mesh.bevel([-0.42, 0.36, -0.32], [-0.2, 0.48, -0.12], 0.02);
        }
        Shape::Frigate => {
            mesh.hull(0.1, 0.16, 0.06);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.07, -0.22, -0.2], [0.07, 0.03, -0.02], 0.03);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.05, -0.06, -0.02], [0.05, 0.03, 0.06], 0.02);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.1], [-0.02, 0.36], [0.016, 0.01], 6);
            mesh.block([-0.06, -0.12, 0.26], [0.06, -0.1, 0.28]);
            // A landing pad on the stern.
            mesh.paint(Material::Trim);
            mesh.block([-0.055, -0.45, -0.2], [0.055, -0.3, -0.19]);
            mesh.turret([0.0, 0.2], [-0.16, -0.08], 0.055, 1, 0.1, false);
        }
        Shape::Destroyer => {
            mesh.hull(0.11, 0.2, 0.08);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.08, -0.17, -0.2], [0.08, 0.1, -0.03], 0.03);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.06, 0.02, -0.03], [0.06, 0.1, 0.07], 0.02);
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, -0.1], [-0.03, 0.08], [0.035, 0.03], 8);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.02], [0.07, 0.42], [0.015, 0.009], 6);
            mesh.block([-0.07, -0.03, 0.32], [0.07, -0.01, 0.34]);
            // Torpedo tubes either side, behind the tower.
            for x in [-0.075_f32, 0.075] {
                mesh.beam([x, -0.28, -0.17], [x, -0.18, -0.17], 0.015, 0.015);
            }
            mesh.turret([0.0, 0.27], [-0.16, -0.08], 0.06, 1, 0.1, false);
            mesh.turret([0.0, -0.34], [-0.2, -0.12], 0.06, 1, 0.05, true);
        }
        Shape::Cruiser => {
            mesh.hull(0.12, 0.18, 0.08);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.09, -0.2, -0.2], [0.09, 0.07, 0.0], 0.035);
            // Missile cells: team-colored decks with dark hatches.
            for (y0, y1, z) in [(0.12_f32, 0.28_f32, -0.16_f32), (-0.44, -0.26, -0.2)] {
                mesh.paint(Material::Trim);
                mesh.block([-0.075, y0, z], [0.075, y1, z + 0.03]);
                mesh.paint(Material::Metal);
                for x in [-0.04_f32, 0.0, 0.04] {
                    mesh.block([x - 0.014, y0 + 0.02, z + 0.03], [x + 0.014, y1 - 0.02, z + 0.035]);
                }
            }
            mesh.paint(Material::Trim);
            mesh.bevel([-0.06, -0.06, 0.0], [0.06, 0.05, 0.12], 0.025);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.12], [0.0, 0.4], [0.016, 0.011], 6);
            mesh.paint(Material::Hull);
            mesh.cuboid(
                [Vec3::new(-0.08, -0.14, 0.32), Vec3::new(0.08, -0.14, 0.32), Vec3::new(0.08, -0.11, 0.32), Vec3::new(-0.08, -0.11, 0.32)],
                [Vec3::new(-0.08, -0.11, 0.44), Vec3::new(0.08, -0.11, 0.44), Vec3::new(0.08, -0.08, 0.44), Vec3::new(-0.08, -0.08, 0.44)],
            );
            mesh.turret([0.0, 0.36], [-0.16, -0.09], 0.045, 1, 0.06, false);
        }
        Shape::Battleship => {
            mesh.hull(0.14, 0.14, 0.06);
            // A stepped tower amidships, clear of the turrets fore and aft.
            mesh.paint(Material::Hull);
            mesh.bevel([-0.11, -0.22, -0.2], [0.11, 0.02, -0.04], 0.03);
            mesh.bevel([-0.075, -0.17, -0.04], [0.075, 0.0, 0.08], 0.025);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.05, -0.1, 0.08], [0.05, -0.01, 0.18], 0.02);
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, -0.19], [-0.04, 0.12], [0.032, 0.028], 8);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.06], [0.18, 0.44], [0.014, 0.008], 6);
            mesh.block([-0.06, -0.07, 0.34], [0.06, -0.05, 0.36]);
            // Small guns along each side of the tower.
            for x in [-0.12_f32, 0.12] {
                for y in [-0.16_f32, -0.04] {
                    mesh.block([x - 0.018, y - 0.022, -0.2], [x + 0.018, y + 0.022, -0.16]);
                }
            }
            mesh.turret([0.0, 0.3], [-0.16, -0.08], 0.08, 2, 0.06, false);
            mesh.turret([0.0, 0.12], [-0.14, -0.04], 0.08, 2, 0.06, false);
            mesh.turret([0.0, -0.34], [-0.2, -0.12], 0.08, 2, 0.04, true);
        }
        Shape::Battlecruiser => {
            mesh.hull(0.12, 0.06, 0.0);
            // A tower raked back: low at the stern, high at the bow.
            mesh.paint(Material::Hull);
            mesh.cuboid(
                [Vec3::new(-0.09, -0.28, -0.2), Vec3::new(0.09, -0.28, -0.2), Vec3::new(0.09, -0.02, -0.2), Vec3::new(-0.09, -0.02, -0.2)],
                [Vec3::new(-0.07, -0.24, -0.08), Vec3::new(0.07, -0.24, -0.08), Vec3::new(0.07, -0.05, 0.04), Vec3::new(-0.07, -0.05, 0.04)],
            );
            mesh.paint(Material::Trim);
            mesh.cuboid(
                [Vec3::new(-0.055, -0.12, -0.03), Vec3::new(0.055, -0.12, -0.03), Vec3::new(0.055, -0.06, 0.03), Vec3::new(-0.055, -0.06, 0.03)],
                [Vec3::new(-0.045, -0.11, 0.05), Vec3::new(0.045, -0.11, 0.05), Vec3::new(0.045, -0.07, 0.11), Vec3::new(-0.045, -0.07, 0.11)],
            );
            // A raked funnel and a mast.
            mesh.paint(Material::Hull);
            mesh.beam([0.0, -0.2, -0.12], [0.0, -0.26, 0.06], 0.03, 0.035);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.1], [0.08, 0.38], [0.014, 0.008], 6);
            // Missile cells aft.
            mesh.paint(Material::Trim);
            mesh.block([-0.07, -0.46, -0.2], [0.07, -0.33, -0.17]);
            mesh.paint(Material::Metal);
            for x in [-0.035_f32, 0.0, 0.035] {
                mesh.block([x - 0.012, -0.44, -0.17], [x + 0.012, -0.35, -0.165]);
            }
            mesh.turret([0.0, 0.3], [-0.16, -0.08], 0.07, 2, 0.06, false);
            mesh.turret([0.0, 0.13], [-0.14, -0.05], 0.07, 2, 0.06, false);
        }
        Shape::Submarine => {
            let outline = [[-0.03, -0.5], [0.03, -0.5], [0.08, -0.38], [0.09, 0.24], [0.045, 0.44], [-0.045, 0.44], [-0.09, 0.24], [-0.08, -0.38]];
            mesh.paint(Material::Hull);
            mesh.prism(&outline, [-0.5, -0.32], [0.55, 1.0]);
            mesh.prism(&outline, [-0.32, -0.25], [1.0, 0.7]);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.035, 0.02, -0.27], [0.035, 0.2, -0.08], 0.02);
            mesh.paint(Material::Hull);
            mesh.block([-0.1, 0.1, -0.13], [0.1, 0.14, -0.115]);
            mesh.block([-0.09, -0.46, -0.38], [0.09, -0.41, -0.365]);
            mesh.block([-0.008, -0.47, -0.33], [0.008, -0.39, -0.21]);
        }
        Shape::MissileSubmarine => {
            let outline = [[-0.035, -0.5], [0.035, -0.5], [0.09, -0.38], [0.1, 0.28], [0.05, 0.46], [-0.05, 0.46], [-0.1, 0.28], [-0.09, -0.38]];
            mesh.paint(Material::Hull);
            mesh.prism(&outline, [-0.5, -0.3], [0.55, 1.0]);
            mesh.prism(&outline, [-0.3, -0.23], [1.0, 0.75]);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.04, 0.14, -0.25], [0.04, 0.34, -0.04], 0.02);
            mesh.paint(Material::Hull);
            mesh.block([-0.11, 0.22, -0.09], [0.11, 0.26, -0.075]);
            mesh.block([-0.008, -0.47, -0.3], [0.008, -0.38, -0.17]);
            // Two rows of missile hatches behind the tower.
            mesh.paint(Material::Glow);
            for y in [-0.26_f32, -0.14, -0.02, 0.1] {
                for x in [-0.035_f32, 0.035] {
                    mesh.block([x - 0.02, y - 0.03, -0.24], [x + 0.02, y + 0.03, -0.228]);
                }
            }
        }
        Shape::ShieldBoat => {
            mesh.hull(0.11, 0.2, 0.08);
            mesh.paint(Material::Trim);
            mesh.block([-0.08, -0.44, -0.2], [0.08, -0.22, -0.18]);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.08, -0.2, -0.2], [0.08, 0.1, 0.0], 0.03);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.055, 0.02, 0.0], [0.055, 0.1, 0.08], 0.02);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.08], [0.0, 0.3], [0.035, 0.025], 8);
            mesh.paint(Material::Glow);
            mesh.frustum([0.0, -0.08], [0.3, 0.42], [0.1, 0.1], 10);
            mesh.frustum([0.0, -0.08], [0.42, 0.5], [0.07, 0.03], 10);
        }
        // The T2 bay: higher buttressed walls with windows, two cranes, and two stacks.
        Shape::FactoryT2 => {
            // The floor of the bay, and ramps down from both of its open ends, east and west.
            mesh.paint(Material::Hull);
            mesh.block([-0.42, -0.5, -0.5], [0.42, 0.5, -0.44]);
            for side in [-1.0_f32, 1.0] {
                let (inner, outer) = (0.42 * side, 0.5 * side);
                let (low, high) = (inner.min(outer), inner.max(outer));
                let z = |x: f32| if (x - inner).abs() < 1e-6 { -0.44 } else { -0.495 };
                mesh.cuboid(
                    [Vec3::new(low, -0.34, -0.5), Vec3::new(high, -0.34, -0.5), Vec3::new(high, 0.34, -0.5), Vec3::new(low, 0.34, -0.5)],
                    [Vec3::new(low, -0.34, z(low)), Vec3::new(high, -0.34, z(high)), Vec3::new(high, 0.34, z(high)), Vec3::new(low, 0.34, z(low))],
                );
            }
            // Guide lights along the floor, lit while it builds.
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| {
                for y in [-0.33_f32, 0.33] {
                    mesh.block([-0.4, y - 0.01, -0.44], [0.4, y + 0.01, -0.43]);
                }
            });

            // Higher walls with buttresses, a row of windows, and the team's stripe.
            for (y, outward) in [(-0.47_f32, -1.0_f32), (0.31, 1.0)] {
                mesh.paint(Material::Hull);
                mesh.bevel([-0.46, y, -0.44], [0.46, y + 0.16, 0.0], 0.03);
                for x in [-0.3_f32, 0.0, 0.3] {
                    let face = if outward < 0.0 { y } else { y + 0.16 };
                    mesh.bevel([x - 0.04, face.min(face + 0.03 * outward), -0.44], [x + 0.04, face.max(face + 0.03 * outward), -0.08], 0.01);
                }
                mesh.paint(Material::Glow);
                let inner = if outward < 0.0 { y + 0.16 } else { y };
                mesh.block([-0.4, inner - 0.005, -0.2], [0.4, inner + 0.005, -0.14]);
                mesh.paint(Material::Trim);
                mesh.block([-0.46, y, 0.0], [0.46, y + 0.16, 0.05]);
            }
            // Two gantry cranes, with their hoists.
            for x in [-0.2_f32, 0.2] {
                mesh.paint(Material::Metal);
                for y in [-0.47_f32, 0.39] {
                    mesh.block([x - 0.035, y, 0.05], [x + 0.035, y + 0.08, 0.34]);
                }
                mesh.block([x - 0.045, -0.47, 0.3], [x + 0.045, 0.47, 0.38]);
                mesh.paint(Material::Trim);
                mesh.block([x - 0.06, -0.07, 0.2], [x + 0.06, 0.07, 0.3]);
            }
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| {
                for x in [-0.2_f32, 0.2] {
                    mesh.block([x - 0.02, -0.02, 0.15], [x + 0.02, 0.02, 0.2]);
                }
            });
            // Two smokestacks on the north wall.
            mesh.paint(Material::Metal);
            for x in [-0.38_f32, 0.38] {
                mesh.frustum([x, 0.42], [0.05, 0.48], [0.055, 0.045], 6);
            }
        }
        // The T3 bay: sloped armored walls, a heavy double gantry with a cab, a tower, and stacks.
        Shape::FactoryT3 => {
            // The floor of the bay, and ramps down from both of its open ends, east and west.
            mesh.paint(Material::Hull);
            mesh.block([-0.42, -0.5, -0.5], [0.42, 0.5, -0.44]);
            for side in [-1.0_f32, 1.0] {
                let (inner, outer) = (0.42 * side, 0.5 * side);
                let (low, high) = (inner.min(outer), inner.max(outer));
                let z = |x: f32| if (x - inner).abs() < 1e-6 { -0.44 } else { -0.495 };
                mesh.cuboid(
                    [Vec3::new(low, -0.34, -0.5), Vec3::new(high, -0.34, -0.5), Vec3::new(high, 0.34, -0.5), Vec3::new(low, 0.34, -0.5)],
                    [Vec3::new(low, -0.34, z(low)), Vec3::new(high, -0.34, z(high)), Vec3::new(high, 0.34, z(high)), Vec3::new(low, 0.34, z(low))],
                );
            }
            // Guide lights along the floor, lit while it builds.
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| {
                for y in [-0.33_f32, 0.33] {
                    mesh.block([-0.4, y - 0.01, -0.44], [0.4, y + 0.01, -0.43]);
                }
            });

            // Heavy walls with sloped armor, inside faces lit, and the team's stripe on top.
            for (y, outward) in [(-0.5_f32, -1.0_f32), (0.32, 1.0)] {
                let (outer, inner) = if outward < 0.0 { (y, y + 0.18) } else { (y + 0.18, y) };
                let top_outer = outer - 0.07 * outward;
                mesh.paint(Material::Hull);
                let (low, high) = (outer.min(inner), outer.max(inner));
                let (top_low, top_high) = (top_outer.min(inner), top_outer.max(inner));
                mesh.cuboid(
                    [Vec3::new(-0.47, low, -0.44), Vec3::new(0.47, low, -0.44), Vec3::new(0.47, high, -0.44), Vec3::new(-0.47, high, -0.44)],
                    [Vec3::new(-0.47, top_low, 0.06), Vec3::new(0.47, top_low, 0.06), Vec3::new(0.47, top_high, 0.06), Vec3::new(-0.47, top_high, 0.06)],
                );
                mesh.paint(Material::Glow);
                mesh.block([-0.42, inner - 0.006, -0.3], [0.42, inner + 0.006, -0.25]);
                mesh.block([-0.42, inner - 0.006, -0.1], [0.42, inner + 0.006, -0.06]);
                mesh.paint(Material::Trim);
                mesh.block([-0.47, top_low, 0.06], [0.47, top_high, 0.1]);
            }
            // A heavy double gantry with an operator's cab, spanning the bay.
            mesh.paint(Material::Metal);
            for x in [-0.12_f32, 0.12] {
                for y in [-0.47_f32, 0.38] {
                    mesh.block([x - 0.04, y, 0.1], [x + 0.04, y + 0.09, 0.4]);
                }
                mesh.block([x - 0.05, -0.47, 0.36], [x + 0.05, 0.47, 0.44]);
            }
            mesh.paint(Material::Hull);
            mesh.bevel([-0.17, -0.12, 0.26], [0.17, 0.12, 0.36], 0.02);
            mesh.paint(Material::Glow);
            mesh.block([-0.14, -0.125, 0.3], [0.14, -0.115, 0.34]);
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| {
                for x in [-0.08_f32, 0.08] {
                    mesh.block([x - 0.02, -0.02, 0.2], [x + 0.02, 0.02, 0.26]);
                }
            });
            // A control tower at the north-east corner, and three stacks.
            mesh.paint(Material::Hull);
            mesh.frustum([0.42, 0.42], [0.1, 0.4], [0.06, 0.05], 6);
            mesh.paint(Material::Glow);
            mesh.frustum([0.42, 0.42], [0.4, 0.47], [0.075, 0.07], 6);
            mesh.paint(Material::Metal);
            for x in [-0.4_f32, -0.26, -0.12] {
                mesh.frustum([x, 0.42], [0.1, 0.5], [0.045, 0.035], 6);
            }
        }
        // An open deck with a lit round pad, where aircraft are built and lift off, a hangar
        // behind it, and a control tower.
        Shape::AirFactory => {
            // A deck, with a round pad in the middle for what it builds, ringed with lights.
            mesh.paint(Material::Hull);
            mesh.bevel([-0.5, -0.5, -0.5], [0.5, 0.5, -0.44], 0.02);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, 0.0], [-0.44, -0.43], [0.31, 0.31], 20);
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| {
                for step in 0..10 {
                    let angle = step as f32 / 10.0 * std::f32::consts::TAU;
                    let (x, y) = (angle.cos() * 0.33, angle.sin() * 0.33);
                    mesh.block([x - 0.015, y - 0.015, -0.44], [x + 0.015, y + 0.015, -0.42]);
                }
            });
            // Painted lines across the pad.
            mesh.paint(Material::Trim);
            mesh.block([-0.22, -0.02, -0.43], [0.22, 0.02, -0.425]);

            // A hangar along the north edge, open towards the pad.
            mesh.paint(Material::Trim);
            mesh.arch(0.42, [0.36, 0.5], -0.44, 0.42, 10);
            // A control tower in the south-east corner.
            mesh.paint(Material::Hull);
            mesh.frustum([0.41, -0.41], [-0.44, 0.12], [0.06, 0.05], 6);
            mesh.paint(Material::Glow);
            mesh.frustum([0.41, -0.41], [0.12, 0.22], [0.085, 0.075], 6);
            mesh.paint(Material::Metal);
            mesh.frustum([0.41, -0.41], [0.22, 0.32], [0.012, 0.01], 4);
        }
        // The T2 deck: a larger hangar, and two towers joined by approach lights.
        Shape::AirFactoryT2 => {
            // A deck, with a round pad in the middle for what it builds, ringed with lights.
            mesh.paint(Material::Hull);
            mesh.bevel([-0.5, -0.5, -0.5], [0.5, 0.5, -0.44], 0.02);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, 0.0], [-0.44, -0.43], [0.31, 0.31], 20);
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| {
                for step in 0..10 {
                    let angle = step as f32 / 10.0 * std::f32::consts::TAU;
                    let (x, y) = (angle.cos() * 0.33, angle.sin() * 0.33);
                    mesh.block([x - 0.015, y - 0.015, -0.44], [x + 0.015, y + 0.015, -0.42]);
                }
            });
            // Painted lines across the pad.
            mesh.paint(Material::Trim);
            mesh.block([-0.22, -0.02, -0.43], [0.22, 0.02, -0.425]);

            // A larger hangar, with a lit sign over its door.
            mesh.paint(Material::Trim);
            mesh.arch(0.44, [0.34, 0.5], -0.44, 0.5, 12);
            mesh.paint(Material::Glow);
            mesh.block([-0.2, 0.335, -0.06], [0.2, 0.345, -0.02]);
            // Control towers in both southern corners, joined by a gantry of approach lights.
            for x in [-0.41_f32, 0.41] {
                mesh.paint(Material::Hull);
                mesh.frustum([x, -0.41], [-0.44, 0.2], [0.06, 0.05], 6);
                mesh.paint(Material::Glow);
                mesh.frustum([x, -0.41], [0.2, 0.3], [0.08, 0.07], 6);
            }
            mesh.paint(Material::Metal);
            mesh.block([-0.41, -0.43, 0.0], [0.41, -0.39, 0.04]);
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| {
                for x in [-0.24_f32, -0.08, 0.08, 0.24] {
                    mesh.block([x - 0.02, -0.43, -0.02], [x + 0.02, -0.39, 0.0]);
                }
            });
        }
        // The T3 deck: a great hangar of two bays, a tall tower with a turning dish, and fuel tanks.
        Shape::AirFactoryT3 => {
            // A deck, with a round pad in the middle for what it builds, ringed with lights.
            mesh.paint(Material::Hull);
            mesh.bevel([-0.5, -0.5, -0.5], [0.5, 0.5, -0.44], 0.02);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, 0.0], [-0.44, -0.43], [0.31, 0.31], 20);
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| {
                for step in 0..10 {
                    let angle = step as f32 / 10.0 * std::f32::consts::TAU;
                    let (x, y) = (angle.cos() * 0.33, angle.sin() * 0.33);
                    mesh.block([x - 0.015, y - 0.015, -0.44], [x + 0.015, y + 0.015, -0.42]);
                }
            });
            // Painted lines across the pad.
            mesh.paint(Material::Trim);
            mesh.block([-0.22, -0.02, -0.43], [0.22, 0.02, -0.425]);

            // A great hangar along the north edge, divided into two bays.
            mesh.paint(Material::Trim);
            mesh.arch(0.46, [0.3, 0.5], -0.44, 0.62, 14);
            mesh.paint(Material::Hull);
            mesh.block([-0.03, 0.29, -0.44], [0.03, 0.31, 0.16]);
            mesh.paint(Material::Glow);
            mesh.block([-0.44, 0.295, -0.1], [0.44, 0.305, -0.06]);
            // A tall tower with a turning radar dish, in the south-west corner.
            mesh.paint(Material::Hull);
            mesh.frustum([-0.41, -0.41], [-0.44, 0.28], [0.07, 0.05], 6);
            mesh.paint(Material::Glow);
            mesh.frustum([-0.41, -0.41], [0.28, 0.36], [0.09, 0.08], 6);
            mesh.paint(Material::Metal);
            mesh.frustum([-0.41, -0.41], [0.36, 0.42], [0.02, 0.015], 6);
            mesh.moving(motion::SPIN, [-0.41, -0.41, 0.0], |mesh| {
                mesh.cuboid(
                    [Vec3::new(-0.48, -0.43, 0.4), Vec3::new(-0.34, -0.43, 0.4), Vec3::new(-0.34, -0.4, 0.4), Vec3::new(-0.48, -0.4, 0.4)],
                    [Vec3::new(-0.48, -0.39, 0.5), Vec3::new(-0.34, -0.39, 0.5), Vec3::new(-0.34, -0.36, 0.5), Vec3::new(-0.48, -0.36, 0.5)],
                );
            });
            // Fuel tanks in the south-east corner.
            mesh.paint(Material::Hull);
            for x in [0.3_f32, 0.42] {
                mesh.frustum([x, -0.42], [-0.44, -0.2], [0.05, 0.05], 8);
            }
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
        // The T2 dock: two cranes and a covered workshop.
        Shape::NavalFactoryT2 => {
            // Piers down the long sides, north and south, with open water between them: a slip
            // open at both ends, east and west, where ships are built afloat and sail out.
            mesh.paint(Material::Hull);
            for y in [-0.5_f32, 0.32] {
                mesh.bevel([-0.5, y, -0.5], [0.5, y + 0.18, -0.32], 0.03);
            }
            // Bollards along the piers' inner edges.
            mesh.paint(Material::Metal);
            for y in [-0.33_f32, 0.33] {
                for x in [-0.4_f32, -0.2, 0.0, 0.2, 0.4] {
                    mesh.frustum([x, y], [-0.32, -0.28], [0.015, 0.015], 6);
                }
            }
            // Buoy lights at the open ends.
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| {
                for x in [-0.48_f32, 0.48] {
                    for y in [-0.3_f32, 0.3] {
                        mesh.block([x - 0.02, y - 0.02, -0.34], [x + 0.02, y + 0.02, -0.3]);
                    }
                }
            });

            // Two cranes, and a covered workshop on the north pier.
            for x in [-0.22_f32, 0.22] {
                mesh.paint(Material::Metal);
                for y in [-0.45_f32, 0.37] {
                    mesh.block([x - 0.035, y, -0.32], [x + 0.035, y + 0.08, 0.26]);
                }
                mesh.block([x - 0.045, -0.45, 0.22], [x + 0.045, 0.45, 0.3]);
                mesh.paint(Material::Trim);
                mesh.block([x - 0.06, -0.06, 0.12], [x + 0.06, 0.06, 0.22]);
            }
            mesh.paint(Material::Trim);
            mesh.arch(0.18, [0.34, 0.5], -0.32, 0.22, 8);
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| {
                for x in [-0.22_f32, 0.22] {
                    mesh.block([x - 0.02, -0.02, 0.08], [x + 0.02, 0.02, 0.12]);
                }
            });
            mesh.paint(Material::Metal);
            mesh.frustum([0.4, -0.42], [-0.32, 0.1], [0.04, 0.03], 6);
        }
        // The T3 dock: a heavy gantry with a cab over the whole slip, lit seawalls, and a tower.
        Shape::NavalFactoryT3 => {
            // Piers down the long sides, north and south, with open water between them: a slip
            // open at both ends, east and west, where ships are built afloat and sail out.
            mesh.paint(Material::Hull);
            for y in [-0.5_f32, 0.32] {
                mesh.bevel([-0.5, y, -0.5], [0.5, y + 0.18, -0.32], 0.03);
            }
            // Bollards along the piers' inner edges.
            mesh.paint(Material::Metal);
            for y in [-0.33_f32, 0.33] {
                for x in [-0.4_f32, -0.2, 0.0, 0.2, 0.4] {
                    mesh.frustum([x, y], [-0.32, -0.28], [0.015, 0.015], 6);
                }
            }
            // Buoy lights at the open ends.
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| {
                for x in [-0.48_f32, 0.48] {
                    for y in [-0.3_f32, 0.3] {
                        mesh.block([x - 0.02, y - 0.02, -0.34], [x + 0.02, y + 0.02, -0.3]);
                    }
                }
            });

            // A heavy gantry over the whole slip, with an operator's cab, and lit seawalls.
            mesh.paint(Material::Metal);
            for x in [-0.36_f32, 0.36] {
                for y in [-0.46_f32, 0.37] {
                    mesh.block([x - 0.04, y, -0.32], [x + 0.04, y + 0.09, 0.34]);
                }
            }
            mesh.paint(Material::Trim);
            for y in [-0.42_f32, 0.38] {
                mesh.block([-0.4, y, 0.3], [0.4, y + 0.05, 0.38]);
            }
            mesh.paint(Material::Metal);
            for x in [-0.12_f32, 0.12] {
                mesh.block([x - 0.04, -0.42, 0.32], [x + 0.04, 0.43, 0.38]);
            }
            mesh.paint(Material::Hull);
            mesh.bevel([-0.16, -0.1, 0.2], [0.16, 0.1, 0.32], 0.02);
            mesh.paint(Material::Glow);
            mesh.block([-0.13, -0.105, 0.24], [0.13, -0.095, 0.29]);
            for y in [-0.325_f32, 0.325] {
                mesh.block([-0.48, y - 0.006, -0.38], [0.48, y + 0.006, -0.34]);
            }
            // A control tower on the north pier.
            mesh.paint(Material::Hull);
            mesh.frustum([-0.42, 0.42], [-0.32, 0.2], [0.06, 0.05], 6);
            mesh.paint(Material::Glow);
            mesh.frustum([-0.42, 0.42], [0.2, 0.28], [0.075, 0.07], 6);
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
                mesh.moving(motion::RECOIL, [x, -0.05, 0.1], |mesh| mesh.beam([x, -0.05, 0.1], [x, 0.38, 0.44], 0.05, 0.05));
                mesh.flash([x, 0.38, 0.44], 0.05);
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
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| mesh.frustum([0.0, 0.0], [-0.35, -0.05], [0.26, 0.18], 12));
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
            mesh.motion = 0;
            mesh.pivot = Vec3::ZERO;
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
            let material = [Material::Trim, Material::Hull, Material::Metal, Material::Glow][(vertex.material & motion::MATERIAL_MASK) as usize];
            if !found.contains(&material) {
                found.push(material);
            }
        }
        found
    }

    #[test]
    fn every_vertex_has_a_known_material() {
        let set = MeshSet::build();
        assert!(set.vertices.iter().all(|vertex| vertex.material & motion::MATERIAL_MASK <= Material::Glow as u32));
    }

    /// Only glowing parts flash or pulse, and a flash never shows until a gun fires.
    #[test]
    fn flashes_and_pulses_are_glowing_parts() {
        let set = MeshSet::build();
        for vertex in &set.vertices {
            if vertex.material & (motion::FLASH | motion::PULSE) != 0 {
                assert_eq!(vertex.material & motion::MATERIAL_MASK, Material::Glow as u32);
            }
        }
        let walking: Vec<_> = Shape::ALL.into_iter().filter(|shape| shape.walks()).collect();
        for (index, shape) in Shape::ALL.into_iter().enumerate() {
            let swings = shape_vertices(&set, index).iter().any(|vertex| vertex.material & motion::SWING != 0);
            assert_eq!(swings, walking.contains(&shape), "{shape:?} swings only if it walks");
            let spins = shape_vertices(&set, index).iter().any(|vertex| vertex.material & motion::SPIN != 0);
            assert_eq!(spins, shape.spin_rate().is_some(), "{shape:?} spins only if it has a spin rate");
        }
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
