//! Icons for units and structures. An icon is a frame, a mark inside it, and a tab of rank bars
//! beneath for its tech level.
//!
//! - The frame says how the unit moves, by its outline alone (`Frame`): squares and rectangles
//!   are structures and nothing else; land units stand on a flat base with angled sides; ships
//!   are a curved hull under a flat deck and submarines a cigar, with no corner below the
//!   waterline; aircraft are triangles and a trapezoid.
//! - The mark says what it does (`Mark`), and the same job has the same mark in every domain: a
//!   gun is a disc on a tank, a walker, a warship, a gunship, or a point defense. Upgrades share a
//!   mark. Where two units would otherwise share frame, mark, and tech level, one of them carries
//!   a small extra mark (`Extra`).
//! - The tab holds one to three upright bars, white, yellow, and orange, so the level reads by
//!   count or by color. It is dark, so it reads the same on any ground.
//!
//! All are vector shapes, drawn in code, so they stay sharp at any size and need no image files.
//! Until 2026-11-20 every kind of unit had a small picture of its own (a turret, a funnel, an
//! arrowhead), which blurred together at map size, and the tech level was small white dots.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};

/// How an icon's unit moves, which sets its frame's outline.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    /// A head over rounded shoulders, larger: the command rig and the field rig.
    Commander,
    /// A rectangle, wider than tall: every factory.
    Factory,
    /// A square: every other structure.
    Structure,
    /// A trapezoid on its wide base, like tracks under a hull: tanks, artillery, anti-air,
    /// mobile shields, scouts, and engineers.
    Vehicle,
    /// A pentagon on a flat base with a peaked top, standing up: the walkers.
    Bot,
    /// A flat deck over a curved hull: surface ships.
    Ship,
    /// A long rounded hull, like a cigar, with no corner anywhere: submarines.
    Submarine,
    /// A triangle pointing up, the bomber's flipped: fighters and scouts.
    Fighter,
    /// A triangle pointing down: bombers, the torpedo bomber, and the fighter-bomber.
    Bomber,
    /// A trapezoid on its narrow end, the vehicle's turned over and squatter: gunships and
    /// transports (a circle until 2026-11-20).
    Gunship,
    /// No frame: what a silo builds, which only ever shows on its build bar.
    Munition,
}

/// How far the camera must be from the ground before units show as icons.
pub const ICON_ALTITUDE: f64 = 1_200.0;
/// The area round a unit's icon where a click picks it, in points from its center: half its
/// width, and how far it reaches above and below, the tech tab being beneath. Enough to take in
/// every icon, its frame and tech tab, whatever its kind (a test checks).
pub const HIT_HALF_WIDTH_PT: f64 = 15.0;
pub const HIT_ABOVE_PT: f64 = 15.0;
pub const HIT_BELOW_PT: f64 = 22.5;

/// One look on the map: units sharing it share frame and mark, and differ by tech level.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Icon {
    Commander,
    FieldRig,
    Engineer,
    CombatEngineer,
    /// The medium and heavy tanks.
    Tank,
    AssaultHovercraft,
    Artillery,
    MobileMissile,
    MobileAntiAir,
    MobileShield,
    LandScout,
    /// The light, grenadier, and siege walkers.
    Walker,
    HeavyWalker,
    Scout,
    Fighter,
    Bomber,
    FighterBomber,
    TorpedoBomber,
    Gunship,
    Transport,
    /// The frigate, destroyer, and battlecruiser.
    Warship,
    Battleship,
    Cruiser,
    TorpedoBoat,
    EscortCorvette,
    Submarine,
    MissileSubmarine,
    LandFactory,
    LandSupportFactory,
    AirFactory,
    AirSupportFactory,
    NavalFactory,
    NavalSupportFactory,
    RigYard,
    Airfield,
    Extractor,
    Generator,
    Geothermal,
    MassStorage,
    EnergyStorage,
    Fabricator,
    PointDefense,
    AntiAirTurret,
    ArtilleryInstallation,
    TorpedoLauncher,
    ShieldGenerator,
    TacticalLauncher,
    TacticalDefense,
    StrategicLauncher,
    StrategicDefense,
    Radar,
    Sonar,
    Stealth,
    Wall,
    FieldWorkshop,
    TacticalMissile,
    StrategicMissile,
    StrategicInterceptor,
    /// Anything without an icon of its own, such as the stress test's plain cubes.
    Unknown,
}

/// What an icon's mark shows: what the unit does. The same job has the same mark wherever it is.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mark {
    /// A solid disc: guns that fire straight at what they see.
    Gun,
    /// A shell's arc ending in a dot: what fires over what is in the way.
    Artillery,
    /// A chevron pointing up, at the sky: fires only at aircraft.
    AntiAir,
    /// A chevron pointing down, under the water: torpedoes.
    Torpedo,
    /// A missile with fins.
    Missile,
    /// A falling bomb.
    Bomb,
    /// Three blades round a dot: the strategic missile.
    Strategic,
    /// A dome on a base line.
    Shield,
    /// A small missile under a bar: stops tactical missiles.
    MissileDefense,
    /// A small strategic mark under a bar: stops strategic missiles.
    StrategicDefense,
    /// A dot with two arcs above it: radar, and scouts.
    Radar,
    /// The radar mark turned to face down.
    Sonar,
    /// The radar mark struck out.
    Stealth,
    /// A wrench: builders.
    Build,
    /// A cradle holding a load.
    Transport,
    /// A filled star: the command rig.
    Star,
    /// The star as an outline: the field rig, and the yard that makes it.
    HollowStar,
    /// The economy panel's materiel diamond.
    Materiel,
    /// The economy panel's energy bolt.
    Energy,
    /// The bolt over a wave.
    Geothermal,
    /// The diamond in a box.
    MaterielStore,
    /// The bolt in a box.
    EnergyStore,
    /// The bolt inside the diamond: energy made into materiel.
    Fabricator,
    /// A small vehicle trapezoid, filled for a headquarters and outlined for a support factory.
    MakesLand { support: bool },
    /// A small aircraft triangle.
    MakesAir { support: bool },
    /// A small ship's bowl.
    MakesNaval { support: bool },
    /// An H, for the landing pad.
    Airfield,
    /// Courses of bricks.
    Wall,
    /// A dot, for what has no icon.
    Unknown,
}

/// A small mark beside the main one, telling apart two units that would otherwise look alike.
/// Each means the same wherever it appears.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Extra {
    /// A small dome over the mark: it carries a shield of its own (the heavy walker).
    Dome,
    /// A wave under the mark: it crosses water (the assault hovercraft).
    Wave,
    /// A dot beside the mark: it also fights (the combat engineer).
    Armed,
    /// A small chevron over the mark: it also fires at aircraft (the fighter-bomber).
    AlsoAntiAir,
}

/// The groups a builder's bar is laid out in, in order, with a divider between each.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BarGroup {
    Factories,
    Resources,
    Defenses,
    Intel,
    Strategic,
    /// What factories make, which their bars do not divide.
    Units,
}

impl Icon {
    pub const ALL: [Self; 59] = [
        Self::Commander,
        Self::FieldRig,
        Self::Engineer,
        Self::CombatEngineer,
        Self::Tank,
        Self::AssaultHovercraft,
        Self::Artillery,
        Self::MobileMissile,
        Self::MobileAntiAir,
        Self::MobileShield,
        Self::LandScout,
        Self::Walker,
        Self::HeavyWalker,
        Self::Scout,
        Self::Fighter,
        Self::Bomber,
        Self::FighterBomber,
        Self::TorpedoBomber,
        Self::Gunship,
        Self::Transport,
        Self::Warship,
        Self::Battleship,
        Self::Cruiser,
        Self::TorpedoBoat,
        Self::EscortCorvette,
        Self::Submarine,
        Self::MissileSubmarine,
        Self::LandFactory,
        Self::LandSupportFactory,
        Self::AirFactory,
        Self::AirSupportFactory,
        Self::NavalFactory,
        Self::NavalSupportFactory,
        Self::RigYard,
        Self::Airfield,
        Self::Extractor,
        Self::Generator,
        Self::Geothermal,
        Self::MassStorage,
        Self::EnergyStorage,
        Self::Fabricator,
        Self::PointDefense,
        Self::AntiAirTurret,
        Self::ArtilleryInstallation,
        Self::TorpedoLauncher,
        Self::ShieldGenerator,
        Self::TacticalLauncher,
        Self::TacticalDefense,
        Self::StrategicLauncher,
        Self::StrategicDefense,
        Self::Radar,
        Self::Sonar,
        Self::Stealth,
        Self::Wall,
        Self::FieldWorkshop,
        Self::TacticalMissile,
        Self::StrategicMissile,
        Self::StrategicInterceptor,
        Self::Unknown,
    ];

    pub fn for_key(key: &str) -> Self {
        match key {
            "command_rig" => Self::Commander,
            "field_rig" => Self::FieldRig,
            "t1_engineer" | "t2_engineer" | "t3_engineer" => Self::Engineer,
            "t2_combat_engineer" => Self::CombatEngineer,
            "t1_medium_tank" | "t2_heavy_tank" => Self::Tank,
            "t2_assault_hovercraft" => Self::AssaultHovercraft,
            "t1_light_artillery" | "t3_mobile_artillery" => Self::Artillery,
            "t2_mobile_missile_launcher" => Self::MobileMissile,
            "t1_mobile_aa" | "t2_mobile_flak" => Self::MobileAntiAir,
            "t2_mobile_shield" => Self::MobileShield,
            "t1_land_scout" => Self::LandScout,
            // The grenadier lobs its grenades, but plays as the walkers' gun at T2.
            "t1_light_walker" | "t2_grenadier_walker" | "t3_siege_walker" => Self::Walker,
            "t3_heavy_walker" => Self::HeavyWalker,
            "t1_air_scout" | "t3_spy_plane" => Self::Scout,
            "t1_interceptor" | "t3_air_superiority" => Self::Fighter,
            "t1_bomber" | "t3_strategic_bomber" => Self::Bomber,
            "t2_fighter_bomber" => Self::FighterBomber,
            "t2_torpedo_bomber" => Self::TorpedoBomber,
            "t2_gunship" | "t3_heavy_gunship" => Self::Gunship,
            "t1_light_transport" | "t2_transport" | "t3_heavy_transport" => Self::Transport,
            "t1_frigate" | "t2_destroyer" | "t3_battlecruiser" => Self::Warship,
            // It fires from 3,000 away, over the horizon.
            "t3_battleship" => Self::Battleship,
            "t2_cruiser" => Self::Cruiser,
            "t2_torpedo_boat" => Self::TorpedoBoat,
            "t2_escort_corvette" => Self::EscortCorvette,
            "t1_attack_sub" => Self::Submarine,
            "t3_missile_sub" => Self::MissileSubmarine,
            "land_factory_t2_support" | "land_factory_t3_support" => Self::LandSupportFactory,
            "air_factory_t2_support" | "air_factory_t3_support" => Self::AirSupportFactory,
            "naval_factory_t2_support" | "naval_factory_t3_support" => Self::NavalSupportFactory,
            key if key.starts_with("land_factory") => Self::LandFactory,
            key if key.starts_with("air_factory") => Self::AirFactory,
            key if key.starts_with("naval_factory") => Self::NavalFactory,
            "rig_yard" => Self::RigYard,
            "airfield" => Self::Airfield,
            key if key.starts_with("mass_extractor") => Self::Extractor,
            key if key.starts_with("power_generator") => Self::Generator,
            "geothermal_plant" => Self::Geothermal,
            "mass_storage" => Self::MassStorage,
            "energy_storage" => Self::EnergyStorage,
            key if key.starts_with("mass_fabricator") => Self::Fabricator,
            "t1_point_defense" | "t2_point_defense" | "t3_heavy_point_defense" => Self::PointDefense,
            "t1_aa_turret" | "t2_flak" | "t3_sam" => Self::AntiAirTurret,
            "t2_artillery" | "t3_artillery" => Self::ArtilleryInstallation,
            "t1_torpedo_launcher" | "t2_torpedo_launcher" => Self::TorpedoLauncher,
            "shield_generator" | "t3_heavy_shield" => Self::ShieldGenerator,
            "tactical_missile_launcher" => Self::TacticalLauncher,
            "tactical_missile_defense" => Self::TacticalDefense,
            "strategic_missile_launcher" => Self::StrategicLauncher,
            "strategic_missile_defense" => Self::StrategicDefense,
            "radar" | "radar_t2" | "deep_scan_array" => Self::Radar,
            "t1_sonar" | "t2_sonar" => Self::Sonar,
            "stealth_field_generator" => Self::Stealth,
            "wall" => Self::Wall,
            "field_workshop" | "field_workshop_t3" => Self::FieldWorkshop,
            "tactical_missile" => Self::TacticalMissile,
            "strategic_missile" => Self::StrategicMissile,
            "strategic_interceptor" => Self::StrategicInterceptor,
            _ => Self::Unknown,
        }
    }

    pub const fn frame(self) -> Frame {
        match self {
            Self::Commander | Self::FieldRig => Frame::Commander,
            Self::Walker | Self::HeavyWalker => Frame::Bot,
            Self::Engineer | Self::CombatEngineer | Self::Tank | Self::AssaultHovercraft | Self::Artillery | Self::MobileMissile | Self::MobileAntiAir | Self::MobileShield | Self::LandScout | Self::Unknown => Frame::Vehicle,
            Self::Scout | Self::Fighter => Frame::Fighter,
            Self::Bomber | Self::FighterBomber | Self::TorpedoBomber => Frame::Bomber,
            Self::Gunship | Self::Transport => Frame::Gunship,
            Self::Warship | Self::Battleship | Self::Cruiser | Self::TorpedoBoat | Self::EscortCorvette => Frame::Ship,
            Self::Submarine | Self::MissileSubmarine => Frame::Submarine,
            Self::LandFactory | Self::LandSupportFactory | Self::AirFactory | Self::AirSupportFactory | Self::NavalFactory | Self::NavalSupportFactory | Self::RigYard => Frame::Factory,
            Self::TacticalMissile | Self::StrategicMissile | Self::StrategicInterceptor => Frame::Munition,
            _ => Frame::Structure,
        }
    }

    /// The mark inside the frame, and any small extra mark beside it.
    pub const fn mark(self) -> (Mark, Option<Extra>) {
        let mark = match self {
            Self::Commander => Mark::Star,
            Self::FieldRig | Self::RigYard => Mark::HollowStar,
            Self::Engineer | Self::CombatEngineer | Self::FieldWorkshop => Mark::Build,
            Self::Tank | Self::AssaultHovercraft | Self::Walker | Self::HeavyWalker | Self::Gunship | Self::Warship | Self::PointDefense => Mark::Gun,
            Self::Artillery | Self::Battleship | Self::ArtilleryInstallation => Mark::Artillery,
            Self::MobileAntiAir | Self::Fighter | Self::AntiAirTurret => Mark::AntiAir,
            Self::TorpedoBomber | Self::TorpedoBoat | Self::Submarine | Self::TorpedoLauncher => Mark::Torpedo,
            Self::MobileMissile | Self::Cruiser | Self::TacticalLauncher | Self::TacticalMissile => Mark::Missile,
            Self::Bomber | Self::FighterBomber => Mark::Bomb,
            Self::MissileSubmarine | Self::StrategicLauncher | Self::StrategicMissile => Mark::Strategic,
            Self::MobileShield | Self::EscortCorvette | Self::ShieldGenerator => Mark::Shield,
            Self::TacticalDefense => Mark::MissileDefense,
            Self::StrategicDefense | Self::StrategicInterceptor => Mark::StrategicDefense,
            Self::LandScout | Self::Scout | Self::Radar => Mark::Radar,
            Self::Sonar => Mark::Sonar,
            Self::Stealth => Mark::Stealth,
            Self::Transport => Mark::Transport,
            Self::Extractor => Mark::Materiel,
            Self::Generator => Mark::Energy,
            Self::Geothermal => Mark::Geothermal,
            Self::MassStorage => Mark::MaterielStore,
            Self::EnergyStorage => Mark::EnergyStore,
            Self::Fabricator => Mark::Fabricator,
            Self::LandFactory => Mark::MakesLand { support: false },
            Self::LandSupportFactory => Mark::MakesLand { support: true },
            Self::AirFactory => Mark::MakesAir { support: false },
            Self::AirSupportFactory => Mark::MakesAir { support: true },
            Self::NavalFactory => Mark::MakesNaval { support: false },
            Self::NavalSupportFactory => Mark::MakesNaval { support: true },
            Self::Airfield => Mark::Airfield,
            Self::Wall => Mark::Wall,
            Self::Unknown => Mark::Unknown,
        };
        let extra = match self {
            Self::HeavyWalker => Some(Extra::Dome),
            Self::AssaultHovercraft => Some(Extra::Wave),
            Self::CombatEngineer => Some(Extra::Armed),
            Self::FighterBomber => Some(Extra::AlsoAntiAir),
            _ => None,
        };
        (mark, extra)
    }

    pub const fn group(self) -> BarGroup {
        match self {
            Self::LandFactory | Self::LandSupportFactory | Self::AirFactory | Self::AirSupportFactory | Self::NavalFactory | Self::NavalSupportFactory | Self::RigYard | Self::Airfield | Self::FieldWorkshop => BarGroup::Factories,
            Self::Extractor | Self::Generator | Self::Geothermal | Self::MassStorage | Self::EnergyStorage | Self::Fabricator => BarGroup::Resources,
            Self::PointDefense | Self::AntiAirTurret | Self::TorpedoLauncher | Self::ShieldGenerator | Self::TacticalDefense | Self::StrategicDefense | Self::Wall => BarGroup::Defenses,
            Self::Radar | Self::Sonar | Self::Stealth => BarGroup::Intel,
            Self::ArtilleryInstallation | Self::TacticalLauncher | Self::StrategicLauncher => BarGroup::Strategic,
            _ => BarGroup::Units,
        }
    }

    pub const fn role(self) -> Role {
        self.mark().0.role()
    }
}

/// What a mark is for, which colors it on the build bar: the color of the range ring it draws,
/// where it has one, so that a bar of a dozen items reads by color first and shape second.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Role {
    /// Builders, commanders, factories, transports, airfields, and walls.
    Build,
    Mass,
    Energy,
    /// Guns and bombs: ground fire's yellow.
    DirectFire,
    /// The red of artillery's too-close ring.
    Artillery,
    AntiAir,
    Torpedo,
    /// Missiles, and what stops them: missile defense's orange.
    Missile,
    /// Strategic missiles, and what stops them.
    Strategic,
    Shield,
    /// Radar and scouts.
    Intel,
    Sonar,
    Stealth,
}

impl Role {
    pub const ALL: [Self; 13] = [
        Self::Build, Self::Mass, Self::Energy, Self::DirectFire, Self::Artillery, Self::AntiAir, Self::Torpedo, Self::Missile, Self::Strategic, Self::Shield, Self::Intel, Self::Sonar, Self::Stealth,
    ];

    /// The range rings' colors (`ui::RangeKind::color`), light enough to read on the bar's grey.
    pub const fn color(self) -> Color32 {
        match self {
            Self::Build => Color32::from_rgb(232, 238, 245),
            Self::Mass => Color32::from_rgb(132, 214, 92),
            Self::Energy => Color32::from_rgb(250, 230, 90),
            Self::DirectFire => Color32::from_rgb(255, 212, 72),
            Self::Artillery => Color32::from_rgb(255, 96, 84),
            Self::AntiAir => Color32::from_rgb(255, 120, 220),
            Self::Torpedo => Color32::from_rgb(90, 220, 190),
            Self::Missile => Color32::from_rgb(255, 160, 70),
            Self::Strategic => Color32::from_rgb(200, 140, 255),
            Self::Shield => Color32::from_rgb(140, 190, 255),
            Self::Intel => Color32::from_rgb(120, 230, 140),
            Self::Sonar => Color32::from_rgb(80, 140, 255),
            Self::Stealth => Color32::from_rgb(190, 190, 200),
        }
    }
}

impl Mark {
    pub const fn role(self) -> Role {
        match self {
            Self::Gun | Self::Bomb => Role::DirectFire,
            Self::Artillery => Role::Artillery,
            Self::AntiAir => Role::AntiAir,
            Self::Torpedo => Role::Torpedo,
            Self::Missile | Self::MissileDefense => Role::Missile,
            Self::Strategic | Self::StrategicDefense => Role::Strategic,
            Self::Shield => Role::Shield,
            Self::Radar => Role::Intel,
            Self::Sonar => Role::Sonar,
            Self::Stealth => Role::Stealth,
            Self::Materiel | Self::MaterielStore | Self::Fabricator => Role::Mass,
            Self::Energy | Self::Geothermal | Self::EnergyStore => Role::Energy,
            Self::Build | Self::Transport | Self::Star | Self::HollowStar | Self::MakesLand { .. } | Self::MakesAir { .. } | Self::MakesNaval { .. } | Self::Airfield | Self::Wall | Self::Unknown => Role::Build,
        }
    }
}

/// Where a unit can go, which sets the backdrop behind its button: land units stand on grass,
/// amphibious ones on a shore, aircraft against sky, ships on open water, and submarines in
/// the dark depths.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Ground {
    Land,
    /// Hovercraft and seabed walkers, which cross both land and water.
    Coast,
    Air,
    Ocean,
    /// Submarines.
    Deep,
}

impl Ground {
    pub const ALL: [Self; 5] = [Self::Land, Self::Coast, Self::Air, Self::Ocean, Self::Deep];

    /// Where a blueprint's unit or structure can be: structures on water count as ships.
    pub fn of(unit: &crate::sim::blueprints::Blueprint) -> Self {
        use crate::sim::components::MoveClass;
        match unit.move_class() {
            MoveClass::Air => Self::Air,
            _ if unit.on_water => Self::Ocean,
            MoveClass::Land => Self::Land,
            MoveClass::Hover | MoveClass::Seabed => Self::Coast,
            MoveClass::Naval => Self::Ocean,
            MoveClass::Submarine => Self::Deep,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Land => "land",
            Self::Coast => "land and water",
            Self::Air => "air",
            Self::Ocean => "water",
            Self::Deep => "underwater",
        }
    }
}

/// A wavy line across `rect`, `rows` from the top, as a polyline.
fn wave(rect: Rect, y: f32, phase: f32, amplitude: f32) -> Vec<Pos2> {
    (0..=12)
        .map(|step| {
            let t = step as f32 / 12.0;
            Pos2::new(rect.left() + rect.width() * t, rect.top() + rect.height() * y + amplitude * (t * std::f32::consts::TAU * 1.5 + phase).sin())
        })
        .collect()
}

/// A vertical gradient filling `rect`.
fn gradient(rect: Rect, top: Color32, bottom: Color32) -> Shape {
    let mut mesh = egui::epaint::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    Shape::mesh(mesh)
}

/// The backdrop for a button of `rect`: what the shapes of `Ground` look like, kept muted so
/// the team-colored unit on top stands out.
pub fn ground_shapes(rect: Rect, ground: Ground) -> Vec<Shape> {
    let at = |x: f32, y: f32| Pos2::new(rect.left() + rect.width() * x, rect.top() + rect.height() * y);
    let line = |points: Vec<Pos2>, color: Color32| Shape::line(points, Stroke::new((rect.height() * 0.035).max(1.0), color));
    let mut out = Vec::new();
    match ground {
        Ground::Land => {
            out.push(gradient(rect, Color32::from_rgb(88, 118, 62), Color32::from_rgb(106, 92, 58)));
            // Tufts of grass and a pale track.
            for (x, y) in [(0.2, 0.25), (0.75, 0.2), (0.12, 0.7), (0.86, 0.62), (0.55, 0.88)] {
                let tuft = [(-0.04, 0.0), (0.0, -0.07), (0.04, 0.0)];
                out.push(line(tuft.iter().map(|(dx, dy)| at(x + dx, y + dy)).collect(), Color32::from_rgb(60, 92, 44)));
            }
            out.push(line(vec![at(0.0, 0.92), at(0.4, 0.8), at(0.7, 0.9), at(1.0, 0.78)], Color32::from_rgba_unmultiplied(190, 170, 120, 90)));
        }
        Ground::Coast => {
            // Sand above, water below, split on a diagonal with a line of foam.
            out.push(Shape::convex_polygon(vec![rect.left_top(), rect.right_top(), at(1.0, 0.35), at(0.0, 0.75)], Color32::from_rgb(176, 160, 106), Stroke::NONE));
            out.push(Shape::convex_polygon(vec![at(0.0, 0.75), at(1.0, 0.35), rect.right_bottom(), rect.left_bottom()], Color32::from_rgb(40, 104, 148), Stroke::NONE));
            out.push(Shape::convex_polygon(vec![at(0.0, 0.7), at(1.0, 0.3), at(1.0, 0.38), at(0.0, 0.78)], Color32::from_rgb(70, 140, 170), Stroke::NONE));
            out.push(line(vec![at(0.0, 0.7), at(0.25, 0.6), at(0.5, 0.5), at(0.75, 0.4), at(1.0, 0.3)], Color32::from_rgba_unmultiplied(240, 248, 250, 200)));
            out.push(line(vec![at(0.0, 0.92), at(0.3, 0.84), at(0.6, 0.92), at(1.0, 0.8)], Color32::from_rgba_unmultiplied(160, 210, 235, 110)));
            for (x, y) in [(0.2, 0.2), (0.7, 0.08), (0.45, 0.32)] {
                out.push(Shape::circle_filled(at(x, y), rect.height() * 0.018, Color32::from_rgb(140, 124, 80)));
            }
        }
        Ground::Air => {
            out.push(gradient(rect, Color32::from_rgb(70, 130, 200), Color32::from_rgb(168, 208, 236)));
            let cloud = Color32::from_rgba_unmultiplied(255, 255, 255, 190);
            for (x, y, r) in [(0.22, 0.78, 0.1), (0.31, 0.8, 0.13), (0.42, 0.78, 0.09), (0.74, 0.28, 0.08), (0.82, 0.3, 0.1), (0.9, 0.28, 0.07)] {
                out.push(Shape::circle_filled(at(x, y), rect.height() * r, cloud));
            }
        }
        Ground::Ocean => {
            out.push(gradient(rect, Color32::from_rgb(38, 108, 160), Color32::from_rgb(18, 58, 104)));
            for (y, phase) in [(0.22, 0.0), (0.5, 1.7), (0.78, 3.1)] {
                out.push(line(wave(rect, y, phase, rect.height() * 0.04), Color32::from_rgba_unmultiplied(190, 228, 245, 120)));
            }
        }
        Ground::Deep => {
            out.push(gradient(rect, Color32::from_rgb(14, 40, 78), Color32::from_rgb(4, 12, 32)));
            for (y, phase) in [(0.12, 0.6), (0.4, 2.2)] {
                out.push(line(wave(rect, y, phase, rect.height() * 0.03), Color32::from_rgba_unmultiplied(120, 170, 220, 60)));
            }
            for (x, y, r) in [(0.18, 0.62, 0.03), (0.24, 0.5, 0.02), (0.8, 0.72, 0.03), (0.86, 0.55, 0.02), (0.72, 0.4, 0.015)] {
                out.push(Shape::circle_stroke(at(x, y), rect.height() * r, Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(150, 200, 240, 120))));
            }
        }
    }
    out
}

/// The colors of the rank bars, T1 to T3.
pub const TIER_COLORS: [Color32; 3] = [Color32::WHITE, Color32::from_rgb(255, 212, 72), Color32::from_rgb(255, 138, 61)];

/// Draws an icon to fill a build bar button: its frame and mark, large, in its role's color. The
/// bar's tech toggle says the level, so no tab is drawn. The top left corner is left for the
/// button's key.
pub fn draw_bar(painter: &Painter, rect: Rect, icon: Icon) {
    for shape in bar_shapes(rect, icon) {
        painter.add(shape);
    }
}

/// The shapes `draw_bar` paints.
pub fn bar_shapes(rect: Rect, icon: Icon) -> Vec<Shape> {
    let half = rect.height() * 0.23;
    // Below and right of center, clear of the key in the corner.
    let center = rect.center() + Vec2::new(rect.width() * 0.08, rect.height() * 0.1);
    shapes(center, half, icon, 0, icon.role().color())
}

/// How tall the role strip across the top of a unit's button is, as a share of the button's
/// height, and the least and most in points.
const ROLE_STRIP: (f32, f32, f32) = (0.07, 2.0, 3.5);
/// Half the height of the symbol on a unit's button, the same way. Until 2026-11-23 it was a
/// tenth, at most 6 points, on a round dark disc: too small for its color to read along a bar.
const ROLE_SYMBOL_HALF: (f32, f32, f32) = (0.15, 4.5, 8.0);
/// The space round the symbol on its tab, in points.
const ROLE_TAB_PAD: f32 = 2.0;
/// How much of the button's width the tab may take, and how far down it may reach: wide frames,
/// such as aircraft and factories, and the commanders' tall one are drawn smaller than the rest,
/// so that every tab keeps to its corner and none hides much of the portrait.
const ROLE_TAB_WIDEST: f32 = 0.45;
const ROLE_TAB_REACH: f32 = 0.5;

/// The header of a unit's button, over its portrait: a strip across the top in its role's color,
/// so that a bar of buttons reads by color first, as the bar of symbols did before portraits,
/// and with `symbol`, its map symbol in that color on a dark tab hanging from the strip's left end.
pub fn role_header_shapes(rect: Rect, icon: Icon, symbol: bool) -> Vec<Shape> {
    let color = icon.role().color();
    let sized = |(share, least, most): (f32, f32, f32)| (rect.height() * share).clamp(least, most);
    let strip = sized(ROLE_STRIP);
    let mut out = vec![Shape::rect_filled(Rect::from_min_size(rect.min, Vec2::new(rect.width(), strip)), 0.0, color)];
    if symbol {
        let measured = |half: f32| {
            let mark = shapes(Pos2::ZERO, half, icon, 0, color);
            let bounds = mark.iter().fold(Rect::NOTHING, |bounds, shape| bounds.union(shape.visual_bounding_rect()));
            (mark, bounds)
        };
        let half = sized(ROLE_SYMBOL_HALF);
        let (mut mark, mut bounds) = measured(half);
        let widest = rect.width() * ROLE_TAB_WIDEST - ROLE_TAB_PAD * 2.0;
        let tallest = rect.height() * ROLE_TAB_REACH - strip - ROLE_TAB_PAD * 2.0;
        let fits = (widest / bounds.width()).min(tallest / bounds.height());
        if fits < 1.0 {
            (mark, bounds) = measured(half * fits);
        }
        let tab = Rect::from_min_size(rect.min + Vec2::new(0.0, strip), bounds.size() + Vec2::splat(ROLE_TAB_PAD * 2.0));
        out.push(Shape::rect_filled(tab, egui::Rounding { se: 4.0, ..egui::Rounding::ZERO }, Color32::from_black_alpha(200)));
        let offset = tab.min + Vec2::splat(ROLE_TAB_PAD) - bounds.min;
        for shape in &mut mark {
            shape.translate(offset);
        }
        out.extend(mark);
    }
    out
}

/// Half the height, in points, of an icon on the map: large enough that its mark reads without
/// squinting, and the command rigs larger still. Made a quarter larger on 2026-11-20, then a
/// quarter smaller than that, which left them a little under their first size.
pub const fn map_half(icon: Icon) -> f32 {
    match icon.frame() {
        Frame::Commander => 9.8,
        Frame::Factory | Frame::Structure => 8.9,
        _ => 8.45,
    }
}

/// How far the frame reaches above and below its center, as shares of `half`.
pub const fn frame_reach(frame: Frame) -> (f32, f32) {
    match frame {
        Frame::Commander => (1.17, 1.0),
        Frame::Factory | Frame::Ship => (0.85, 1.0),
        Frame::Fighter | Frame::Bomber => (1.15, 1.15),
        Frame::Gunship => (0.85, 0.85),
        Frame::Submarine => (0.8, 0.8),
        Frame::Bot => (1.1, 1.0),
        Frame::Structure | Frame::Vehicle | Frame::Munition => (1.0, 1.0),
    }
}

/// The rank bars' sizes, as shares of `half`: each bar's width and height, the space between
/// bars, the tab's margin round them, and the gap between frame and tab, in points.
const BAR_WIDTH: f32 = 0.3;
const BAR_HEIGHT: f32 = 0.62;
const BAR_GAP: f32 = 0.24;
const TAB_MARGIN: f32 = 0.2;
const TAB_GAP: f32 = 1.0;

/// How far, in points, an icon with `half` and its tech tab reach below its center.
pub fn reach_below(icon: Icon, half: f32, tier: u8) -> f32 {
    let frame = half * frame_reach(icon.frame()).1;
    if tier == 0 || icon.frame() == Frame::Munition {
        // With half its outline's width.
        frame + (half / 4.5).clamp(1.2, 3.0) * 0.5
    } else {
        frame + TAB_GAP + half * (BAR_HEIGHT + TAB_MARGIN * 2.0) + 1.0
    }
}

/// The nuclear marker's radius, in points, at every zoom: well over a unit icon's, so that a
/// strategic missile in flight is plain to see from anywhere (2026-11-25).
pub const NUCLEAR_RADIUS_PT: f32 = 14.0;
/// How many times a second its halo pulses out.
pub const NUCLEAR_PULSES: f64 = 1.5;
const NUCLEAR_YELLOW: Color32 = Color32::from_rgb(255, 214, 40);
const NUCLEAR_GLOW: Color32 = Color32::from_rgb(255, 190, 40);
const NUCLEAR_INK: Color32 = Color32::from_rgb(18, 16, 10);

/// The nuclear marker, drawn over a strategic missile in flight: the radiation trefoil, dark on
/// a yellow disc, in a soft glow with rings pulsing out from it. `phase` runs from 0 to 1 through
/// each pulse.
pub fn nuclear_shapes(center: Pos2, radius: f32, phase: f32) -> Vec<Shape> {
    let mut out = Vec::new();
    // The glow, breathing with the pulse.
    let breath = 0.5 + 0.5 * (phase * std::f32::consts::TAU).cos();
    for (reach, alpha) in [(1.9_f32, 0.10_f32), (1.5, 0.16), (1.22, 0.24)] {
        out.push(Shape::circle_filled(center, radius * reach, NUCLEAR_GLOW.gamma_multiply(alpha * (0.6 + 0.4 * breath))));
    }
    // Three rings spreading out and fading, a third of a pulse apart.
    for ring in 0..3 {
        let along = (phase + ring as f32 / 3.0).fract();
        let fade = (1.0 - along) * (1.0 - along);
        out.push(Shape::circle_stroke(center, radius * (1.1 + 1.3 * along), Stroke::new(2.0_f32, NUCLEAR_GLOW.gamma_multiply(0.7 * fade))));
    }
    out.push(Shape::circle_filled(center, radius, NUCLEAR_YELLOW));
    out.push(Shape::circle_stroke(center, radius, Stroke::new((radius * 0.12).max(1.5), NUCLEAR_INK)));
    // The trefoil: three blades a sixth of a turn wide, a sixth apart, one pointing down (screen
    // y grows downward), round a dot, with a ring of yellow between.
    let mut blades = egui::epaint::Mesh::default();
    let (inner, outer) = (radius * 0.28, radius * 0.84);
    const STEPS: u32 = 8;
    for blade in 0..3 {
        let middle = std::f32::consts::FRAC_PI_2 + blade as f32 * std::f32::consts::TAU / 3.0;
        let first = blades.vertices.len() as u32;
        for step in 0..=STEPS {
            let angle = middle - std::f32::consts::PI / 6.0 + step as f32 / STEPS as f32 * std::f32::consts::PI / 3.0;
            let way = Vec2::angled(angle);
            blades.colored_vertex(center + way * inner, NUCLEAR_INK);
            blades.colored_vertex(center + way * outer, NUCLEAR_INK);
        }
        for step in 0..STEPS {
            let at = first + step * 2;
            blades.add_triangle(at, at + 1, at + 2);
            blades.add_triangle(at + 1, at + 3, at + 2);
        }
    }
    out.push(Shape::mesh(blades));
    out.push(Shape::circle_filled(center, radius * 0.18, NUCLEAR_INK));
    out
}

/// A frame's outline, in shares of `half` from the center, clockwise. The commander's head is
/// drawn apart.
#[cfg(test)]
fn outline(frame: Frame) -> Vec<(f32, f32)> {
    parts(frame).into_iter().next().unwrap_or_default()
}

/// Points round an ellipse, from one angle to another, measured clockwise from the right.
fn arc(cx: f32, cy: f32, rx: f32, ry: f32, from: f32, to: f32, steps: usize) -> Vec<(f32, f32)> {
    (0..=steps)
        .map(|step| {
            let angle = (from + (to - from) * step as f32 / steps as f32).to_radians();
            (cx + angle.cos() * rx, cy + angle.sin() * ry)
        })
        .collect()
}

/// The convex pieces a frame is drawn as.
fn parts(frame: Frame) -> Vec<Vec<(f32, f32)>> {
    match frame {
        // Rounded shoulders on a flat base; the head is drawn apart.
        Frame::Commander => vec![arc(0.0, 1.0, 1.15, 1.2, 180.0, 360.0, 16)],
        Frame::Factory => vec![vec![(-1.45, -0.85), (1.45, -0.85), (1.45, 1.0), (-1.45, 1.0)]],
        Frame::Structure => vec![vec![(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]],
        Frame::Vehicle => vec![vec![(-0.95, -1.0), (0.95, -1.0), (1.6, 1.0), (-1.6, 1.0)]],
        Frame::Bot => vec![vec![(0.0, -1.1), (1.15, -0.3), (1.15, 1.0), (-1.15, 1.0), (-1.15, -0.3)]],
        Frame::Ship => {
            let mut deck = vec![(-1.6, -0.85), (1.6, -0.85)];
            deck.extend(arc(0.0, -0.85, 1.6, 1.85, 0.0, 180.0, 18).into_iter().skip(1));
            vec![deck]
        }
        Frame::Submarine => vec![arc(0.0, 0.0, 1.6, 0.8, 0.0, 360.0, 48)],
        Frame::Fighter => vec![vec![(0.0, -1.15), (1.55, 1.15), (-1.55, 1.15)]],
        Frame::Bomber => vec![vec![(-1.55, -1.15), (1.55, -1.15), (0.0, 1.15)]],
        Frame::Gunship => vec![vec![(-1.55, -0.85), (1.55, -0.85), (0.95, 0.85), (-0.95, 0.85)]],
        Frame::Munition => Vec::new(),
    }
}

/// Where in its frame an icon's mark sits, as a share of `half` down from the center, and how
/// large it is: a frame narrow at one end holds it toward the other.
const fn mark_place(frame: Frame) -> (f32, f32) {
    match frame {
        Frame::Commander => (0.48, 0.42),
        Frame::Factory => (0.08, 0.72),
        Frame::Structure => (0.0, 0.75),
        Frame::Vehicle => (0.1, 0.7),
        Frame::Bot => (0.15, 0.66),
        Frame::Ship => (-0.12, 0.7),
        Frame::Submarine => (0.0, 0.55),
        Frame::Fighter => (0.4, 0.55),
        Frame::Bomber => (-0.4, 0.55),
        Frame::Gunship => (-0.05, 0.66),
        Frame::Munition => (0.0, 0.85),
    }
}

/// Draws an icon: its frame, filled with a dark shade of `color` and outlined in it, its mark,
/// and its tech level as rank bars in a tab beneath. `half` is half the frame's height, in points.
pub fn draw(painter: &Painter, center: Pos2, half: f32, icon: Icon, tier: u8, color: Color32) {
    for shape in shapes(center, half, icon, tier, color) {
        painter.add(shape);
    }
}

/// The shapes `draw` paints, for drawing and for testing that no two icons look alike.
pub fn shapes(center: Pos2, half: f32, icon: Icon, tier: u8, color: Color32) -> Vec<Shape> {
    let stroke = Stroke::new((half / 4.5).clamp(1.2, 3.0), color);
    // A dark shade of the team's color, so the outline stands out and the frame reads as the
    // team's from a distance.
    let fill = Color32::from_rgba_unmultiplied(color.r() / 5, color.g() / 5, color.b() / 5, 225);
    let mut out = Vec::new();
    let frame = icon.frame();
    let at = |x: f32, y: f32| center + Vec2::new(x * half, y * half);
    for part in parts(frame) {
        out.push(Shape::convex_polygon(part.into_iter().map(|(x, y)| at(x, y)).collect(), fill, stroke));
    }
    if frame == Frame::Commander {
        out.push(Shape::circle_filled(at(0.0, -0.72), half * 0.45, fill));
        out.push(Shape::circle_stroke(at(0.0, -0.72), half * 0.45, stroke));
    }
    let (down, size) = mark_place(frame);
    let (mark, extra) = icon.mark();
    draw_mark(&mut out, at(0.0, down), half * size, mark, extra, stroke);
    if tier > 0 && frame != Frame::Munition {
        rank_tab(&mut out, Pos2::new(center.x, center.y + half * frame_reach(frame).1 + TAB_GAP), half, tier, color);
    }
    out
}

/// The tech tab: a dark tab hanging from `top`, edged in `color`, with one upright bar for each
/// level, in the level's color.
fn rank_tab(out: &mut Vec<Shape>, top: Pos2, half: f32, tier: u8, color: Color32) {
    let tier = tier.min(3);
    let (width, height, gap, margin) = (half * BAR_WIDTH, half * BAR_HEIGHT, half * BAR_GAP, half * TAB_MARGIN);
    let bars = f32::from(tier);
    let tab = Rect::from_min_size(Pos2::new(top.x - (bars * width + (bars - 1.0) * gap) * 0.5 - margin, top.y), Vec2::new(bars * width + (bars - 1.0) * gap + margin * 2.0, height + margin * 2.0));
    out.push(Shape::rect_filled(tab, half * 0.18, Color32::from_black_alpha(230)));
    out.push(Shape::rect_stroke(tab, half * 0.18, Stroke::new((half / 9.0).clamp(0.8, 1.4), color)));
    let level = TIER_COLORS[usize::from(tier - 1)];
    for index in 0..tier {
        let left = tab.left() + margin + f32::from(index) * (width + gap);
        out.push(Shape::rect_filled(Rect::from_min_size(Pos2::new(left, tab.top() + margin), Vec2::new(width, height)), width * 0.25, level));
    }
}

/// A mark, in a box `size` either way of `center`, and its extra mark if it has one.
#[allow(clippy::too_many_lines)]
fn draw_mark(out: &mut Vec<Shape>, center: Pos2, size: f32, mark: Mark, extra: Option<Extra>, stroke: Stroke) {
    let at = |x: f32, y: f32| center + Vec2::new(x * size, y * size);
    let line = |points: &[(f32, f32)]| Shape::line(points.iter().map(|&(x, y)| at(x, y)).collect(), stroke);
    let heavy = |points: &[(f32, f32)]| Shape::line(points.iter().map(|&(x, y)| at(x, y)).collect(), Stroke::new(stroke.width * 1.3, stroke.color));
    let thin = |points: &[(f32, f32)]| Shape::line(points.iter().map(|&(x, y)| at(x, y)).collect(), Stroke::new(stroke.width * 0.8, stroke.color));
    let closed = |points: &[(f32, f32)]| {
        let mut points: Vec<Pos2> = points.iter().map(|&(x, y)| at(x, y)).collect();
        points.push(points[0]);
        Shape::line(points, stroke)
    };
    let solid = |points: &[(f32, f32)]| Shape::convex_polygon(points.iter().map(|&(x, y)| at(x, y)).collect(), stroke.color, Stroke::NONE);
    let dot = |x: f32, y: f32, radius: f32| Shape::circle_filled(at(x, y), radius * size, stroke.color);
    let curve = |cx: f32, cy: f32, radius: f32, from: f32, to: f32| Shape::line(arc(cx, cy, radius, radius, from, to, 12).into_iter().map(|(x, y)| at(x, y)).collect(), stroke);
    let wave = |y: f32| thin(&[(-0.95, y), (-0.6, y - 0.25), (-0.25, y), (0.1, y - 0.25), (0.45, y), (0.8, y - 0.25), (0.95, y - 0.1)]);
    // A missile with fins, scaled and moved down.
    let missile = |out: &mut Vec<Shape>, scale: f32, down: f32| {
        let p = |x: f32, y: f32| (x * scale, y * scale + down);
        out.push(solid(&[p(0.0, -1.0), p(0.4, -0.38), p(0.4, 0.45), p(-0.4, 0.45), p(-0.4, -0.38)]));
        out.push(solid(&[p(0.4, 0.42), p(0.72, 0.95), p(-0.72, 0.95), p(-0.4, 0.42)]));
    };
    // Three blades round a dot, scaled and moved down.
    let trefoil = |out: &mut Vec<Shape>, scale: f32, down: f32| {
        for angle in [-90.0_f32, 30.0, 150.0] {
            let mut blade = arc(0.0, 0.0, 0.92 * scale, 0.92 * scale, angle - 30.0, angle + 30.0, 6);
            blade.extend(arc(0.0, 0.0, 0.3 * scale, 0.3 * scale, angle + 30.0, angle - 30.0, 1));
            out.push(solid(&blade.into_iter().map(|(x, y)| (x, y + down)).collect::<Vec<_>>()));
        }
        out.push(dot(0.0, down, 0.15 * scale));
    };
    // Five points round a center.
    let star_points = || -> Vec<(f32, f32)> {
        (0..10)
            .map(|index| {
                let radius = if index % 2 == 0 { 0.95 } else { 0.4 };
                let angle = (-90.0 + 36.0 * index as f32).to_radians();
                (angle.cos() * radius, angle.sin() * radius + 0.05)
            })
            .collect()
    };
    let diamond = |scale: f32| [(0.0, -0.85 * scale), (0.8 * scale, 0.0), (0.0, 0.85 * scale), (-0.8 * scale, 0.0)];
    let bolt = |scale: f32, down: f32| [(0.25 * scale, -0.9 * scale + down), (-0.45 * scale, 0.1 * scale + down), (0.15 * scale, 0.1 * scale + down), (-0.25 * scale, 0.9 * scale + down)];
    match mark {
        Mark::Gun => out.push(dot(0.0, 0.0, 0.58)),
        Mark::Artillery => {
            // A shell lobbed high, from the lower left, landing at the right.
            let path: Vec<(f32, f32)> = (0..=12)
                .map(|step| {
                    let t = step as f32 / 12.0;
                    let (from, control, to) = ((-0.9, 0.65), (-0.15, -1.3), (0.62, 0.3));
                    let u = 1.0 - t;
                    (u * u * from.0 + 2.0 * u * t * control.0 + t * t * to.0, u * u * from.1 + 2.0 * u * t * control.1 + t * t * to.1)
                })
                .collect();
            out.push(line(&path));
            out.push(dot(0.68, 0.52, 0.3));
        }
        Mark::AntiAir => out.push(heavy(&[(-0.88, 0.55), (0.0, -0.55), (0.88, 0.55)])),
        Mark::Torpedo => out.push(heavy(&[(-0.88, -0.55), (0.0, 0.55), (0.88, -0.55)])),
        Mark::Missile => missile(out, 1.0, 0.0),
        Mark::Bomb => {
            out.push(Shape::convex_polygon(arc(0.0, 0.3, 0.48, 0.62, 0.0, 360.0, 16).into_iter().map(|(x, y)| at(x, y)).collect(), stroke.color, Stroke::NONE));
            out.push(thin(&[(-0.45, -0.85), (0.0, -0.38), (0.45, -0.85), (-0.45, -0.85)]));
        }
        Mark::Strategic => trefoil(out, 1.0, 0.0),
        Mark::Shield => {
            out.push(Shape::line(arc(0.0, 0.62, 0.92, 0.92, 180.0, 360.0, 14).into_iter().map(|(x, y)| at(x, y)).collect(), Stroke::new(stroke.width * 1.15, stroke.color)));
            out.push(thin(&[(-1.05, 0.62), (1.05, 0.62)]));
        }
        Mark::MissileDefense => {
            missile(out, 0.7, 0.28);
            out.push(heavy(&[(-0.92, -0.8), (0.92, -0.8)]));
        }
        Mark::StrategicDefense => {
            trefoil(out, 0.72, 0.3);
            out.push(heavy(&[(-0.92, -0.8), (0.92, -0.8)]));
        }
        Mark::Radar | Mark::Stealth => {
            out.push(dot(0.0, 0.62, 0.24));
            out.push(curve(0.0, 0.62, 0.62, 225.0, 315.0));
            out.push(curve(0.0, 0.62, 1.12, 225.0, 315.0));
            if mark == Mark::Stealth {
                out.push(line(&[(-0.92, 0.88), (0.92, -0.9)]));
            }
        }
        Mark::Sonar => {
            out.push(dot(0.0, -0.62, 0.24));
            out.push(curve(0.0, -0.62, 0.62, 45.0, 135.0));
            out.push(curve(0.0, -0.62, 1.12, 45.0, 135.0));
        }
        Mark::Build => {
            // A wrench.
            out.push(heavy(&[(-0.78, 0.78), (0.18, -0.18)]));
            out.push(curve(0.42, -0.42, 0.4, 135.0, 405.0));
        }
        Mark::Transport => {
            out.push(line(&[(-0.88, -0.62), (-0.88, 0.56), (0.88, 0.56), (0.88, -0.62)]));
            out.push(dot(0.0, -0.06, 0.3));
        }
        Mark::Star => {
            let points = star_points();
            // A star is not convex: its middle, and a triangle for each point.
            out.push(solid(&points.iter().skip(1).step_by(2).copied().collect::<Vec<_>>()));
            for index in (0..10).step_by(2) {
                out.push(solid(&[points[(index + 9) % 10], points[index], points[index + 1]]));
            }
        }
        Mark::HollowStar => out.push(closed(&star_points())),
        Mark::Materiel => {
            out.push(closed(&diamond(1.0)));
            out.push(dot(0.0, 0.0, 0.2));
        }
        Mark::Energy => out.push(line(&bolt(1.0, 0.0))),
        Mark::Geothermal => {
            out.push(line(&bolt(0.72, -0.22)));
            out.push(wave(0.85));
        }
        Mark::MaterielStore => {
            out.push(closed(&[(-0.85, -0.75), (0.85, -0.75), (0.85, 0.75), (-0.85, 0.75)]));
            out.push(closed(&diamond(0.48)));
        }
        Mark::EnergyStore => {
            out.push(closed(&[(-0.85, -0.75), (0.85, -0.75), (0.85, 0.75), (-0.85, 0.75)]));
            out.push(line(&bolt(0.55, 0.0)));
        }
        Mark::Fabricator => {
            out.push(closed(&diamond(1.12)));
            out.push(thin(&bolt(0.48, 0.0)));
        }
        Mark::MakesLand { support } => {
            let shape = [(-0.95, 0.55), (-0.58, -0.55), (0.58, -0.55), (0.95, 0.55)];
            out.push(if support { closed(&shape) } else { solid(&shape) });
        }
        Mark::MakesAir { support } => {
            let shape = [(0.0, -0.7), (0.82, 0.62), (-0.82, 0.62)];
            out.push(if support { closed(&shape) } else { solid(&shape) });
        }
        Mark::MakesNaval { support } => {
            let mut shape = vec![(-1.0, -0.5), (1.0, -0.5)];
            shape.extend(arc(0.0, -0.5, 1.0, 1.1, 0.0, 180.0, 10).into_iter().skip(1));
            out.push(if support { closed(&shape) } else { solid(&shape) });
        }
        Mark::Airfield => {
            out.push(heavy(&[(-0.6, -0.8), (-0.6, 0.8)]));
            out.push(heavy(&[(0.6, -0.8), (0.6, 0.8)]));
            out.push(heavy(&[(-0.6, 0.0), (0.6, 0.0)]));
        }
        Mark::Wall => {
            out.push(closed(&[(-0.9, -0.6), (0.9, -0.6), (0.9, 0.6), (-0.9, 0.6)]));
            out.push(thin(&[(-0.9, 0.0), (0.9, 0.0)]));
            out.push(thin(&[(0.0, -0.6), (0.0, 0.0)]));
            out.push(thin(&[(-0.45, 0.0), (-0.45, 0.6)]));
            out.push(thin(&[(0.45, 0.0), (0.45, 0.6)]));
        }
        Mark::Unknown => out.push(dot(0.0, 0.0, 0.25)),
    }
    match extra {
        Some(Extra::Dome) => out.push(Shape::line(arc(0.0, 0.0, 1.0, 1.0, 205.0, 335.0, 10).into_iter().map(|(x, y)| at(x, y)).collect(), Stroke::new(stroke.width * 0.8, stroke.color))),
        Some(Extra::Wave) => out.push(wave(0.98)),
        Some(Extra::Armed) => out.push(dot(0.86, 0.72, 0.3)),
        Some(Extra::AlsoAntiAir) => out.push(thin(&[(-0.55, -0.8), (0.0, -1.25), (0.55, -0.8)])),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::blueprints::{Blueprint, BlueprintDb, Targets, UnitClass, WeaponKind};

    /// The nuclear marker is larger than any unit's icon, a trefoil of three blades on its disc,
    /// and its halo pulses: its rings spread and fade through each pulse, and stay within it.
    #[test]
    fn the_nuclear_marker_stands_out_and_pulses() {
        let largest = Icon::ALL.iter().map(|&icon| map_half(icon)).fold(0.0_f32, f32::max);
        assert!(NUCLEAR_RADIUS_PT >= largest * 1.4, "the marker, {NUCLEAR_RADIUS_PT} across half, is not well over the largest icon's {largest}");
        let center = Pos2::new(50.0, 50.0);
        let rings = |phase: f32| -> Vec<(f32, u8)> {
            nuclear_shapes(center, NUCLEAR_RADIUS_PT, phase)
                .into_iter()
                .filter_map(|shape| match shape {
                    Shape::Circle(circle) if circle.fill == Color32::TRANSPARENT && circle.radius > NUCLEAR_RADIUS_PT => Some((circle.radius, circle.stroke.color.a())),
                    _ => None,
                })
                .collect()
        };
        let (early, late) = (rings(0.0), rings(0.2));
        assert_eq!(early.len(), 3, "three rings");
        assert!(early.iter().zip(&late).all(|(before, after)| after.0 > before.0 && after.1 <= before.1), "rings spread and fade: {early:?} then {late:?}");
        assert!(rings(0.99).iter().all(|&(radius, _)| radius <= NUCLEAR_RADIUS_PT * 2.4), "and stay within the halo");
        let blades = nuclear_shapes(center, NUCLEAR_RADIUS_PT, 0.0).into_iter().find_map(|shape| match shape {
            Shape::Mesh(mesh) => Some(mesh),
            _ => None,
        });
        let blades = blades.expect("a trefoil");
        assert_eq!(blades.indices.len(), 3 * 8 * 2 * 3, "three blades");
        assert!(blades.vertices.iter().all(|vertex| (vertex.pos - center).length() <= NUCLEAR_RADIUS_PT * 0.85), "within the disc");
    }

    #[test]
    fn every_unit_and_structure_has_an_icon_of_its_own_kind() {
        let db = BlueprintDb::standard();
        for unit in db.iter() {
            assert_ne!(Icon::for_key(&unit.key), Icon::Unknown, "{} has no icon", unit.key);
        }
    }

    /// On the map, no two units or structures look the same: those sharing an icon differ by
    /// tech level.
    #[test]
    fn every_unit_can_be_told_from_every_other_on_the_map() {
        let db = BlueprintDb::standard();
        let mut seen: Vec<(Icon, u8, &str)> = Vec::new();
        for unit in db.iter() {
            let icon = Icon::for_key(&unit.key);
            if let Some((_, _, other)) = seen.iter().find(|(seen_icon, tier, _)| *seen_icon == icon && *tier == unit.tier) {
                panic!("{} and {other} share {icon:?} at T{}", unit.key, unit.tier);
            }
            seen.push((icon, unit.tier, &unit.key));
        }
    }

    #[test]
    fn tiers_of_one_kind_share_an_icon_and_differ_by_rank_bars() {
        for (lower, higher) in [("t1_air_scout", "t3_spy_plane"), ("t1_frigate", "t2_destroyer"), ("t2_destroyer", "t3_battlecruiser"), ("t1_light_walker", "t2_grenadier_walker"), ("radar", "deep_scan_array")] {
            assert_eq!(Icon::for_key(lower), Icon::for_key(higher), "{lower} and {higher}");
        }
        assert_ne!(Icon::for_key("t1_air_scout"), Icon::for_key("t1_interceptor"));
        // One bar more for each level, in the level's own color.
        let drawn = |tier: u8| shapes(Pos2::ZERO, 10.0, Icon::Scout, tier, Color32::WHITE);
        assert_eq!(drawn(3).len() - drawn(1).len(), 2);
        for tier in 1..=3_u8 {
            let bars = drawn(tier).into_iter().filter(|shape| matches!(shape, Shape::Rect(rect) if rect.fill == TIER_COLORS[usize::from(tier - 1)])).count();
            assert_eq!(bars, usize::from(tier), "T{tier}");
        }
        assert!(TIER_COLORS.iter().enumerate().all(|(index, color)| TIER_COLORS[index + 1..].iter().all(|other| other != color)), "each level has its own color");
    }

    /// The mark follows from what the unit does, by its data, so that one job has one mark in
    /// every domain. The few that are chosen rather than derived are listed with why.
    #[test]
    fn every_mark_says_what_its_unit_does() {
        let db = BlueprintDb::standard();
        let chosen: &[(&str, Mark, &str)] = &[
            ("t3_battleship", Mark::Artillery, "fires from 3,000 away, over the horizon"),
            ("t2_grenadier_walker", Mark::Gun, "lobs grenades, but plays as the walkers' gun at T2"),
            ("t1_land_scout", Mark::Radar, "a scout, whose gun is a pea shooter"),
        ];
        let makes_units = |unit: &Blueprint| unit.builds.iter().any(|&made| !db.get(made).is_structure() && db.get(made).class != UnitClass::Ammo);
        let expected = |unit: &Blueprint| -> Mark {
            let all_air = unit.weapon.iter().chain(&unit.extra_weapons).all(|weapon| weapon.targets == Targets::Air);
            if unit.is_commander() {
                Mark::Star
            } else if unit.class == UnitClass::FieldRig {
                Mark::HollowStar
            } else if unit.transport.is_some() {
                Mark::Transport
            } else if unit.class == UnitClass::Engineer || unit.assists {
                Mark::Build
            } else if let Some(weapon) = &unit.weapon {
                match weapon.kind {
                    WeaponKind::StrategicMissile => Mark::Strategic,
                    WeaponKind::TacticalMissile => Mark::Missile,
                    WeaponKind::Bomb => Mark::Bomb,
                    _ if weapon.artillery => Mark::Artillery,
                    _ if all_air => Mark::AntiAir,
                    _ if weapon.targets == Targets::Naval => Mark::Torpedo,
                    _ => Mark::Gun,
                }
            } else if unit.missile_defense.is_some() && unit.builds.iter().any(|&made| db.get(made).class == UnitClass::Ammo) {
                Mark::StrategicDefense
            } else if unit.missile_defense.is_some() && unit.shield.is_none() {
                Mark::MissileDefense
            } else if unit.shield.is_some() {
                Mark::Shield
            } else if unit.stealth_field > crate::math::fp::Fp::ZERO {
                Mark::Stealth
            } else if unit.radar > crate::math::fp::Fp::ZERO {
                Mark::Radar
            } else if unit.sonar > crate::math::fp::Fp::ZERO {
                Mark::Sonar
            } else {
                // Economy and the rest: their marks are their own.
                Icon::for_key(&unit.key).mark().0
            }
        };
        for unit in db.iter().filter(|unit| unit.class != UnitClass::Ammo && !makes_units(unit)) {
            let mark = Icon::for_key(&unit.key).mark().0;
            let want = chosen.iter().find(|(key, _, _)| *key == unit.key).map_or_else(|| expected(unit), |&(_, mark, _)| mark);
            assert_eq!(mark, want, "{}", unit.key);
        }
        // Factories show what they make, filled for a headquarters and outlined for support.
        for unit in db.iter().filter(|unit| makes_units(unit)) {
            let support = unit.key.ends_with("_support");
            let mark = Icon::for_key(&unit.key).mark().0;
            let fits = match mark {
                Mark::MakesLand { support: outlined } | Mark::MakesAir { support: outlined } | Mark::MakesNaval { support: outlined } => outlined == support,
                Mark::HollowStar => unit.key == "rig_yard",
                _ => false,
            };
            assert!(fits, "{} shows {mark:?}", unit.key);
        }
    }

    /// A click anywhere on an icon, frame and tech tab, is within the area that picks its unit,
    /// for every icon at every tier.
    #[test]
    fn every_icon_lies_within_its_click_area() {
        for icon in Icon::ALL {
            for tier in 0..=3 {
                let mut bounds = Rect::NOTHING;
                for shape in shapes(Pos2::ZERO, map_half(icon), icon, tier, Color32::WHITE) {
                    bounds = bounds.union(shape.visual_bounding_rect());
                }
                let (width, above, below) = (HIT_HALF_WIDTH_PT as f32, HIT_ABOVE_PT as f32, HIT_BELOW_PT as f32);
                assert!(bounds.min.x >= -width && bounds.max.x <= width && bounds.min.y >= -above && bounds.max.y <= below, "{icon:?} at T{tier} reaches {bounds:?}");
                assert!(reach_below(icon, map_half(icon), tier) + 0.01 >= bounds.max.y, "{icon:?} at T{tier}: what is drawn under it clears its tab");
            }
        }
    }

    /// Every unit's frame says how it moves, by its outline alone, whatever its mark.
    #[test]
    fn every_unit_has_the_frame_of_its_role() {
        use crate::sim::components::MoveClass;
        let db = BlueprintDb::standard();
        for def in db.iter() {
            let bombs = def.weapon.is_some_and(|weapon| weapon.kind == WeaponKind::Bomb || weapon.targets == Targets::Naval);
            let expected = if def.class == UnitClass::Ammo {
                Frame::Munition
            } else if def.is_commander() || def.class == UnitClass::FieldRig {
                Frame::Commander
            } else if def.is_structure() && def.builds.iter().any(|&made| !db.get(made).is_structure() && db.get(made).class != UnitClass::Ammo) {
                Frame::Factory
            } else if def.is_structure() {
                Frame::Structure
            } else if def.is_air() && (def.transport.is_some() || def.flight.is_some_and(|flight| flight.hovers)) {
                Frame::Gunship
            } else if def.is_air() && bombs {
                Frame::Bomber
            } else if def.is_air() {
                Frame::Fighter
            } else if def.movement == MoveClass::Submarine {
                Frame::Submarine
            } else if def.movement == MoveClass::Naval {
                Frame::Ship
            } else if def.class == UnitClass::Bot {
                Frame::Bot
            } else {
                Frame::Vehicle
            };
            assert_eq!(Icon::for_key(&def.key).frame(), expected, "{}", def.key);
        }
    }

    /// A fighter's frame is a bomber's turned upside down, pointing north, with its mark held in
    /// the wide end as the bomber's is.
    #[test]
    fn a_fighter_points_north_where_a_bomber_points_south() {
        let flipped = |points: Vec<(f32, f32)>| {
            let mut points: Vec<(i32, i32)> = points.into_iter().map(|(x, y)| ((x * 100.0).round() as i32, (-y * 100.0).round() as i32)).collect();
            points.sort_unstable();
            points
        };
        let mut fighter: Vec<(i32, i32)> = outline(Frame::Fighter).into_iter().map(|(x, y)| ((x * 100.0).round() as i32, (y * 100.0).round() as i32)).collect();
        fighter.sort_unstable();
        assert_eq!(fighter, flipped(outline(Frame::Bomber)));
        let apex = outline(Frame::Fighter).into_iter().fold(f32::MAX, |top, (_, y)| top.min(y));
        assert!(apex < 0.0, "the fighter's point is at the top");
        assert_eq!(mark_place(Frame::Fighter), (-mark_place(Frame::Bomber).0, mark_place(Frame::Bomber).1));
    }

    const FRAMES: [Frame; 10] = [Frame::Commander, Frame::Factory, Frame::Structure, Frame::Vehicle, Frame::Bot, Frame::Ship, Frame::Submarine, Frame::Fighter, Frame::Bomber, Frame::Gunship];

    /// How an outline looks at a glance: its corners (curves have none), which end is wider, top
    /// or bottom, and how wide it is for its height, with where its corners are and whether its
    /// bottom and top are flat edges.
    struct Look {
        corners: Vec<(f32, f32)>,
        right_angles: usize,
        wider: &'static str,
        aspect: f32,
        flat_bottom: bool,
        flat_top: bool,
    }

    fn look(frame: Frame) -> Look {
        let mut points = outline(frame);
        points.dedup_by(|a, b| (a.0 - b.0).hypot(a.1 - b.1) < 1e-4);
        if points.len() > 1 && (points[0].0 - points[points.len() - 1].0).hypot(points[0].1 - points[points.len() - 1].1) < 1e-4 {
            points.pop();
        }
        let count = points.len();
        let turns: Vec<f32> = (0..count)
            .map(|index| {
                let (before, at, after) = (points[(index + count - 1) % count], points[index], points[(index + 1) % count]);
                let turn = (after.1 - at.1).atan2(after.0 - at.0) - (at.1 - before.1).atan2(at.0 - before.0);
                (turn + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
            })
            .collect();
        let corners: Vec<(f32, f32)> = (0..count).filter(|&index| turns[index].abs() > 25_f32.to_radians()).map(|index| points[index]).collect();
        let right_angles = (0..count).filter(|&index| (turns[index].abs() - std::f32::consts::FRAC_PI_2).abs() < 4_f32.to_radians()).count();
        let (top, bottom) = (points.iter().map(|p| p.1).fold(f32::MAX, f32::min), points.iter().map(|p| p.1).fold(f32::MIN, f32::max));
        let (left, right) = (points.iter().map(|p| p.0).fold(f32::MAX, f32::min), points.iter().map(|p| p.0).fold(f32::MIN, f32::max));
        let height = bottom - top;
        let width_at = |y: f32| {
            let crossings: Vec<f32> = (0..count)
                .filter_map(|edge| {
                    let (p, q) = (points[edge], points[(edge + 1) % count]);
                    ((p.1 - y) * (q.1 - y) <= 0.0 && (q.1 - p.1).abs() > 1e-6).then(|| p.0 + (q.0 - p.0) * (y - p.1) / (q.1 - p.1))
                })
                .collect();
            crossings.iter().copied().fold(f32::MIN, f32::max) - crossings.iter().copied().fold(f32::MAX, f32::min)
        };
        let (upper, lower) = (width_at(top + height * 0.15), width_at(bottom - height * 0.15));
        let wider = if upper > lower * 1.2 { "top" } else if lower > upper * 1.2 { "bottom" } else { "neither" };
        // A flat edge along the bottom or top: two points at that height, at least half the
        // outline's width apart.
        let flat = |y: f32| {
            let along: Vec<f32> = points.iter().filter(|p| (p.1 - y).abs() < 1e-3).map(|p| p.0).collect();
            along.len() >= 2 && along.iter().copied().fold(f32::MIN, f32::max) - along.iter().copied().fold(f32::MAX, f32::min) >= (right - left) * 0.5
        };
        Look { corners, right_angles, wider, aspect: (right - left) / height, flat_bottom: flat(bottom), flat_top: flat(top) }
    }

    /// Every frame's outline is told from every other's by what the eye goes by at a glance: how
    /// many corners it has, which end is wider, or how wide it is for its height, by more than
    /// half its height.
    #[test]
    fn every_frame_has_an_outline_of_its_own() {
        for (index, &first) in FRAMES.iter().enumerate() {
            for &second in &FRAMES[index + 1..] {
                let (a, b) = (look(first), look(second));
                let alike = a.corners.len() == b.corners.len() && a.wider == b.wider && (a.aspect - b.aspect).abs() <= 0.5;
                assert!(!alike, "{first:?} ({} corners, {} wider, {:.2}) and {second:?} ({} corners, {} wider, {:.2}) are too alike", a.corners.len(), a.wider, a.aspect, b.corners.len(), b.wider, b.aspect);
            }
        }
    }

    /// The rules the frames keep: squares and rectangles are structures and nothing else; land
    /// units stand on a flat base with corners; ships and submarines are curved below, with no
    /// corner under the waterline, a ship under a flat deck; and a gunship is a vehicle turned over.
    #[test]
    fn frames_keep_to_their_domain() {
        for frame in FRAMES {
            let shape = look(frame);
            let boxy = shape.corners.len() == 4 && shape.right_angles == 4;
            assert_eq!(boxy, matches!(frame, Frame::Structure | Frame::Factory), "{frame:?}: only structures are squares or rectangles");
        }
        assert!(look(Frame::Factory).aspect > look(Frame::Structure).aspect + 0.5, "a factory is wider than a square");
        for frame in [Frame::Vehicle, Frame::Bot, Frame::Commander] {
            let shape = look(frame);
            assert!(shape.flat_bottom, "{frame:?} stands on a flat base");
            assert!(!shape.corners.is_empty(), "{frame:?} has corners");
        }
        for frame in [Frame::Ship, Frame::Submarine] {
            let shape = look(frame);
            assert!(!shape.flat_bottom, "{frame:?} has a curved hull");
            let middle = outline(frame).iter().map(|p| p.1).fold(f32::MAX, f32::min) * 0.5 + outline(frame).iter().map(|p| p.1).fold(f32::MIN, f32::max) * 0.5;
            assert!(shape.corners.iter().all(|corner| corner.1 < middle), "{frame:?} has no corner below the waterline: {:?}", shape.corners);
        }
        assert!(look(Frame::Ship).flat_top, "a ship has a flat deck");
        assert!(look(Frame::Submarine).corners.is_empty(), "a submarine has no corner at all");
        let (vehicle, gunship) = (look(Frame::Vehicle), look(Frame::Gunship));
        assert_eq!((vehicle.wider, gunship.wider, vehicle.corners.len(), gunship.corners.len()), ("bottom", "top", 4, 4), "a gunship is a vehicle turned over");
        assert!(gunship.aspect > vehicle.aspect, "and squatter, apart from the bomber's triangle");
    }

    #[test]
    fn no_two_icons_look_alike() {
        let drawn: Vec<String> = Icon::ALL.iter().map(|icon| format!("{:?}", shapes(Pos2::ZERO, 10.0, *icon, 1, Color32::WHITE))).collect();
        for first in 0..drawn.len() {
            for second in first + 1..drawn.len() {
                assert_ne!(drawn[first], drawn[second], "{:?} and {:?} look the same", Icon::ALL[first], Icon::ALL[second]);
            }
        }
    }

    #[test]
    fn no_two_build_bar_icons_look_alike_and_roles_differ_in_color() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(46.0));
        let drawn: Vec<String> = Icon::ALL.iter().map(|icon| format!("{:?}", bar_shapes(rect, *icon))).collect();
        for first in 0..drawn.len() {
            for second in first + 1..drawn.len() {
                assert_ne!(drawn[first], drawn[second], "{:?} and {:?} look the same on the bar", Icon::ALL[first], Icon::ALL[second]);
            }
        }
        for (index, first) in Role::ALL.iter().enumerate() {
            for second in &Role::ALL[index + 1..] {
                assert_ne!(first.color(), second.color(), "{first:?} and {second:?} share a color");
            }
        }
    }

    #[test]
    fn a_build_bar_icon_fills_most_of_its_button() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(46.0));
        for icon in Icon::ALL.into_iter().filter(|icon| icon.frame() != Frame::Munition) {
            let bounds = bar_shapes(rect, icon).iter().fold(Rect::NOTHING, |bounds, shape| bounds.union(shape.visual_bounding_rect()));
            assert!(bounds.width() > 23.0 && bounds.height() > 18.0, "{icon:?} is only {bounds:?}");
            assert!(rect.contains_rect(bounds), "{icon:?} spills out of its button: {bounds:?}");
        }
    }

    #[test]
    fn a_units_backdrop_says_where_it_can_go() {
        let db = BlueprintDb::standard();
        let ground = |key: &str| Ground::of(db.get(db.id(key)));
        assert_eq!(ground("t1_medium_tank"), Ground::Land);
        assert_eq!(ground("land_factory_t1"), Ground::Land);
        assert_eq!(ground("t2_assault_hovercraft"), Ground::Coast);
        assert_eq!(ground("t1_interceptor"), Ground::Air);
        assert_eq!(ground("t2_gunship"), Ground::Air);
        assert_eq!(ground("t1_frigate"), Ground::Ocean);
        assert_eq!(ground("naval_factory"), Ground::Ocean);
        assert_eq!(ground("t1_attack_sub"), Ground::Deep);
        for unit in db.iter() {
            let expected = match unit.move_class() {
                crate::sim::components::MoveClass::Air => Ground::Air,
                crate::sim::components::MoveClass::Hover | crate::sim::components::MoveClass::Seabed if !unit.on_water => Ground::Coast,
                _ => Ground::of(unit),
            };
            assert_eq!(Ground::of(unit), expected, "{}", unit.key);
        }
        for domain in Ground::ALL {
            assert!(db.iter().any(|unit| Ground::of(unit) == domain), "no unit is {domain:?}");
        }
    }

    #[test]
    fn the_backdrops_look_different_and_stay_inside_their_button() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(46.0));
        let drawn: Vec<String> = Ground::ALL.iter().map(|ground| format!("{:?}", ground_shapes(rect, *ground))).collect();
        for first in 0..drawn.len() {
            for second in first + 1..drawn.len() {
                assert_ne!(drawn[first], drawn[second], "{:?} and {:?} look the same", Ground::ALL[first], Ground::ALL[second]);
            }
        }
        for ground in Ground::ALL {
            let bounds = ground_shapes(rect, ground).iter().fold(Rect::NOTHING, |bounds, shape| bounds.union(shape.visual_bounding_rect()));
            assert!(rect.expand(2.0).contains_rect(bounds), "{ground:?} spills out: {bounds:?}");
        }
    }

    #[test]
    fn a_buttons_role_header_is_a_strip_in_its_roles_color_and_a_symbol_tab_clear_of_the_middle() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::splat(42.0));
        for icon in Icon::ALL.into_iter().filter(|icon| icon.frame() != Frame::Munition) {
            let plain = role_header_shapes(rect, icon, false);
            assert_eq!(plain.len(), 1, "{icon:?}: without the symbol, only the strip");
            let Shape::Rect(strip) = &plain[0] else { panic!("{icon:?}: the strip is a rectangle") };
            assert_eq!(strip.fill, icon.role().color(), "{icon:?}: the strip is in its role's color");
            assert_eq!((strip.rect.top(), strip.rect.width()), (rect.top(), rect.width()), "{icon:?}: the strip runs along the top");

            let shapes = role_header_shapes(rect, icon, true);
            let bounds = shapes.iter().fold(Rect::NOTHING, |bounds, shape| bounds.union(shape.visual_bounding_rect()));
            assert!(rect.contains_rect(bounds), "{icon:?} spills out of its button: {bounds:?}");
            let symbol = shapes[2..].iter().fold(Rect::NOTHING, |bounds, shape| bounds.union(shape.visual_bounding_rect()));
            let tab = shapes[1].visual_bounding_rect();
            assert!(tab.contains_rect(symbol), "{icon:?}: the symbol sits on its tab");
            assert!(tab.width() <= rect.width() * ROLE_TAB_WIDEST + 1.0, "{icon:?}: the tab is {} wide", tab.width());
            assert!(tab.bottom() <= rect.top() + rect.height() * ROLE_TAB_REACH + 1.0, "{icon:?}: the tab stays above the middle, where the unit is: {tab:?}");
            assert!(symbol.width().max(symbol.height()) >= rect.height() * 0.3, "{icon:?}: the symbol is large enough to read, not {symbol:?}");
        }
    }

    #[test]
    fn every_icon_is_listed() {
        let db = BlueprintDb::standard();
        for unit in db.iter() {
            assert!(Icon::ALL.contains(&Icon::for_key(&unit.key)));
        }
    }
}
