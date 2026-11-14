//! Icons for units and structures: one symbol for each kind of thing, the same at every tech
//! level, with the level shown as dots beneath. The frame says what moves where: a rectangle on
//! land, a diamond in the air, a rounded hull on the water, and a square for a structure. All are
//! vector shapes, drawn in code, so they stay sharp at any size and need no image files.

use egui::{Color32, Painter, Pos2, Rect, Rounding, Shape, Stroke, Vec2};

/// What kind of thing an icon stands for, which sets its frame.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    Land,
    Air,
    Naval,
    Structure,
    /// Wider, so that the commander is found at a glance.
    Commander,
}

/// One kind of unit or structure, whatever its tech level.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Icon {
    Commander,
    SupportCommander,
    Engineer,
    LightBot,
    Tank,
    AmphibiousTank,
    Artillery,
    MobileMissile,
    MobileAntiAir,
    Scout,
    Fighter,
    Bomber,
    FighterBomber,
    TorpedoBomber,
    Gunship,
    Frigate,
    Destroyer,
    Cruiser,
    Battleship,
    Battlecruiser,
    Submarine,
    MissileSubmarine,
    ShieldBoat,
    LandFactory,
    AirFactory,
    NavalFactory,
    Gateway,
    AirStaging,
    Extractor,
    Generator,
    Hydrocarbon,
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
    TacticalMissile,
    StrategicMissile,
    AntiNuke,
    LandScout,
    MobileShield,
    FieldEngineer,
    TorpedoBoat,
    Wall,
    EngineeringStation,
    /// Anything without an icon of its own, such as the stress test's plain cubes.
    Unknown,
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
    pub const ALL: [Self; 56] = [
        Self::Commander,
        Self::SupportCommander,
        Self::Engineer,
        Self::LightBot,
        Self::Tank,
        Self::AmphibiousTank,
        Self::Artillery,
        Self::MobileMissile,
        Self::MobileAntiAir,
        Self::Scout,
        Self::Fighter,
        Self::Bomber,
        Self::FighterBomber,
        Self::TorpedoBomber,
        Self::Gunship,
        Self::Frigate,
        Self::Destroyer,
        Self::Cruiser,
        Self::Battleship,
        Self::Battlecruiser,
        Self::Submarine,
        Self::MissileSubmarine,
        Self::ShieldBoat,
        Self::LandFactory,
        Self::AirFactory,
        Self::NavalFactory,
        Self::Gateway,
        Self::AirStaging,
        Self::Extractor,
        Self::Generator,
        Self::Hydrocarbon,
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
        Self::TacticalMissile,
        Self::StrategicMissile,
        Self::AntiNuke,
        Self::LandScout,
        Self::MobileShield,
        Self::FieldEngineer,
        Self::TorpedoBoat,
        Self::Wall,
        Self::EngineeringStation,
        Self::Unknown,
    ];

    /// The icon for a blueprint, by its key. Tiers of the same kind share one.
    pub fn for_key(key: &str) -> Self {
        match key {
            "acu" => Self::Commander,
            "support_commander" => Self::SupportCommander,
            "t1_engineer" | "t2_engineer" | "t3_engineer" => Self::Engineer,
            // Bots and tanks of each tier share a symbol.
            "t1_light_bot" | "t2_gatling_bot" | "t3_heavy_assault_bot" => Self::LightBot,
            "t1_medium_tank" | "t2_heavy_tank" | "t3_armored_assault_bot" => Self::Tank,
            "t1_land_scout" => Self::LandScout,
            "t2_mobile_shield" => Self::MobileShield,
            "t2_field_engineer" => Self::FieldEngineer,
            "t2_torpedo_boat" => Self::TorpedoBoat,
            "wall" => Self::Wall,
            "engineering_station" | "engineering_station_t3" => Self::EngineeringStation,
            "t2_amphibious_tank" => Self::AmphibiousTank,
            "t1_light_artillery" | "t3_mobile_artillery" => Self::Artillery,
            "t2_mobile_missile_launcher" => Self::MobileMissile,
            "t1_mobile_aa" | "t2_mobile_flak" => Self::MobileAntiAir,
            "t1_air_scout" | "t3_spy_plane" => Self::Scout,
            "t1_interceptor" | "t3_air_superiority" => Self::Fighter,
            "t1_bomber" | "t3_strategic_bomber" => Self::Bomber,
            "t2_fighter_bomber" => Self::FighterBomber,
            "t2_torpedo_bomber" => Self::TorpedoBomber,
            "t2_gunship" | "t3_heavy_gunship" => Self::Gunship,
            "t1_frigate" => Self::Frigate,
            "t2_destroyer" => Self::Destroyer,
            "t2_cruiser" => Self::Cruiser,
            "t3_battleship" => Self::Battleship,
            "t3_battlecruiser" => Self::Battlecruiser,
            "t1_attack_sub" => Self::Submarine,
            "t3_missile_sub" => Self::MissileSubmarine,
            "t2_shield_boat" => Self::ShieldBoat,
            key if key.starts_with("land_factory") => Self::LandFactory,
            key if key.starts_with("air_factory") => Self::AirFactory,
            key if key.starts_with("naval_factory") => Self::NavalFactory,
            "quantum_gateway" => Self::Gateway,
            "air_staging" => Self::AirStaging,
            key if key.starts_with("mass_extractor") => Self::Extractor,
            key if key.starts_with("power_generator") => Self::Generator,
            "hydrocarbon_plant" => Self::Hydrocarbon,
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
            "radar" | "radar_t2" | "omni_sensor" => Self::Radar,
            "t1_sonar" | "t2_sonar" => Self::Sonar,
            "stealth_field_generator" => Self::Stealth,
            "tactical_missile" => Self::TacticalMissile,
            "strategic_missile" => Self::StrategicMissile,
            "anti_nuke" => Self::AntiNuke,
            _ => Self::Unknown,
        }
    }

    pub const fn frame(self) -> Frame {
        match self {
            Self::Commander | Self::SupportCommander => Frame::Commander,
            Self::Engineer | Self::LightBot | Self::Tank | Self::AmphibiousTank | Self::Artillery | Self::MobileMissile | Self::MobileAntiAir | Self::LandScout | Self::MobileShield | Self::FieldEngineer | Self::Unknown => Frame::Land,
            Self::Scout | Self::Fighter | Self::Bomber | Self::FighterBomber | Self::TorpedoBomber | Self::Gunship => Frame::Air,
            Self::Frigate | Self::Destroyer | Self::Cruiser | Self::Battleship | Self::Battlecruiser | Self::Submarine | Self::MissileSubmarine | Self::ShieldBoat | Self::TorpedoBoat => Frame::Naval,
            _ => Frame::Structure,
        }
    }

    pub const fn group(self) -> BarGroup {
        match self {
            Self::LandFactory | Self::AirFactory | Self::NavalFactory | Self::Gateway | Self::AirStaging | Self::EngineeringStation => BarGroup::Factories,
            Self::Extractor | Self::Generator | Self::Hydrocarbon | Self::MassStorage | Self::EnergyStorage | Self::Fabricator => BarGroup::Resources,
            Self::PointDefense | Self::AntiAirTurret | Self::TorpedoLauncher | Self::ShieldGenerator | Self::TacticalDefense | Self::StrategicDefense | Self::Wall => BarGroup::Defenses,
            Self::Radar | Self::Sonar | Self::Stealth => BarGroup::Intel,
            Self::ArtilleryInstallation | Self::TacticalLauncher | Self::StrategicLauncher => BarGroup::Strategic,
            _ => BarGroup::Units,
        }
    }
}

/// What a unit or structure is for, which colors its symbol on the build bar, so that a bar of a
/// dozen items reads by color first and shape second.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Role {
    /// Factories, builders, and commanders.
    Build,
    Mass,
    Energy,
    /// Guns that fire straight at what they see.
    DirectFire,
    AntiAir,
    /// Artillery and missiles, which fire over what is in the way.
    Indirect,
    /// Torpedoes and what fires them.
    Underwater,
    /// Shields and missile defenses.
    Protection,
    /// Radar, sonar, scouts, and stealth.
    Intel,
}

impl Role {
    /// Hues spaced around the wheel, light enough to read on the bar's dark grey.
    pub const fn color(self) -> Color32 {
        match self {
            Self::Build => Color32::from_rgb(245, 165, 70),
            Self::Mass => Color32::from_rgb(110, 215, 105),
            Self::Energy => Color32::from_rgb(245, 225, 75),
            Self::DirectFire => Color32::from_rgb(245, 95, 85),
            Self::AntiAir => Color32::from_rgb(95, 185, 255),
            Self::Indirect => Color32::from_rgb(225, 115, 235),
            Self::Underwater => Color32::from_rgb(60, 215, 180),
            Self::Protection => Color32::from_rgb(150, 150, 255),
            Self::Intel => Color32::from_rgb(235, 235, 235),
        }
    }
}

impl Icon {
    pub const fn role(self) -> Role {
        match self {
            Self::Commander | Self::SupportCommander | Self::Engineer | Self::FieldEngineer | Self::EngineeringStation | Self::LandFactory | Self::AirFactory | Self::NavalFactory | Self::Gateway | Self::AirStaging | Self::Unknown => Role::Build,
            Self::Extractor | Self::MassStorage | Self::Fabricator => Role::Mass,
            Self::Generator | Self::Hydrocarbon | Self::EnergyStorage => Role::Energy,
            Self::LightBot | Self::Tank | Self::AmphibiousTank | Self::Gunship | Self::Frigate | Self::Destroyer | Self::Cruiser | Self::Battleship | Self::Battlecruiser | Self::PointDefense => Role::DirectFire,
            Self::MobileAntiAir | Self::Fighter | Self::AntiAirTurret => Role::AntiAir,
            Self::Artillery | Self::MobileMissile | Self::Bomber | Self::FighterBomber | Self::MissileSubmarine | Self::ArtilleryInstallation | Self::TacticalLauncher | Self::StrategicLauncher | Self::TacticalMissile | Self::StrategicMissile => Role::Indirect,
            Self::TorpedoBomber | Self::Submarine | Self::TorpedoLauncher | Self::TorpedoBoat => Role::Underwater,
            Self::ShieldBoat | Self::ShieldGenerator | Self::TacticalDefense | Self::StrategicDefense | Self::AntiNuke | Self::MobileShield | Self::Wall => Role::Protection,
            Self::Scout | Self::Radar | Self::Sonar | Self::Stealth | Self::LandScout => Role::Intel,
        }
    }
}

/// Draws an icon to fill a build bar button: the symbol alone, large, in its role's color. The
/// button is the frame, and the bar's tech toggle says the level, so neither is drawn. The top
/// left corner is left for the button's key.
pub fn draw_bar(painter: &Painter, rect: Rect, icon: Icon) {
    for shape in bar_shapes(rect, icon) {
        painter.add(shape);
    }
}

/// The shapes `draw_bar` paints.
pub fn bar_shapes(rect: Rect, icon: Icon) -> Vec<Shape> {
    let color = icon.role().color();
    let size = rect.height() * 0.3;
    let stroke = Stroke::new((size / 5.5).clamp(1.5, 3.0), color);
    // Below and right of center, clear of the key in the corner.
    let center = rect.center() + Vec2::new(rect.width() * 0.06, rect.height() * 0.08);
    let mut out = Vec::new();
    symbol(&mut out, center, size, icon, stroke);
    out
}

/// Draws an icon: its frame, filled dark and outlined in `color`, its symbol, and a dot beneath
/// for each tech level. `half` is half the frame's height, in points.
pub fn draw(painter: &Painter, center: Pos2, half: f32, icon: Icon, tier: u8, color: Color32) {
    for shape in shapes(center, half, icon, tier, color) {
        painter.add(shape);
    }
}

/// The shapes `draw` paints, for drawing and for testing that no two icons look alike.
pub fn shapes(center: Pos2, half: f32, icon: Icon, tier: u8, color: Color32) -> Vec<Shape> {
    let stroke = Stroke::new((half / 5.0).clamp(1.0, 2.5), color);
    let fill = Color32::from_black_alpha(150);
    let mut out = Vec::new();
    let half_width = match icon.frame() {
        Frame::Commander => half * 1.5,
        Frame::Land | Frame::Naval => half * 1.35,
        Frame::Air | Frame::Structure => half,
    };
    match icon.frame() {
        Frame::Air => {
            let points = vec![center + Vec2::new(0.0, -half * 1.15), center + Vec2::new(half * 1.25, 0.0), center + Vec2::new(0.0, half * 1.15), center + Vec2::new(-half * 1.25, 0.0)];
            out.push(Shape::convex_polygon(points, fill, stroke));
        }
        frame => {
            let rect = Rect::from_center_size(center, Vec2::new(half_width * 2.0, half * 2.0));
            let rounding = if frame == Frame::Naval { Rounding { nw: 0.0, ne: 0.0, sw: half * 0.8, se: half * 0.8 } } else { Rounding::ZERO };
            out.push(Shape::rect_filled(rect, rounding, fill));
            out.push(Shape::rect_stroke(rect, rounding, stroke));
        }
    }
    symbol(&mut out, center, half * 0.62, icon, stroke);
    // A dot for each tech level, beneath the frame.
    let gap = (half * 0.42).max(3.0);
    let radius = (half * 0.13).max(1.1);
    let below = center.y + half * if icon.frame() == Frame::Air { 1.15 } else { 1.0 } + radius + 2.0;
    for index in 0..tier {
        let x = center.x + (f32::from(index) - f32::from(tier.saturating_sub(1)) * 0.5) * gap;
        out.push(Shape::circle_filled(Pos2::new(x, below), radius, color));
    }
    out
}

/// The symbol inside the frame, in a box `size` either way of `center`.
#[allow(clippy::too_many_lines)]
fn symbol(out: &mut Vec<Shape>, center: Pos2, size: f32, icon: Icon, stroke: Stroke) {
    let at = |x: f32, y: f32| center + Vec2::new(x * size, y * size);
    let line = |points: &[(f32, f32)]| Shape::line(points.iter().map(|&(x, y)| at(x, y)).collect(), stroke);
    let solid = |points: &[(f32, f32)]| Shape::convex_polygon(points.iter().map(|&(x, y)| at(x, y)).collect(), stroke.color, Stroke::NONE);
    let ring = |x: f32, y: f32, radius: f32| Shape::circle_stroke(at(x, y), radius * size, stroke);
    let dot = |x: f32, y: f32, radius: f32| Shape::circle_filled(at(x, y), radius * size, stroke.color);
    // An arc, from one angle to another, measured clockwise from the right.
    let arc = |x: f32, y: f32, radius: f32, from: f32, to: f32| {
        let points = (0..=12).map(|step| {
            let angle = (from + (to - from) * step as f32 / 12.0).to_radians();
            at(x + angle.cos() * radius, y + angle.sin() * radius)
        });
        Shape::line(points.collect(), stroke)
    };
    // A ship's hull from above the waterline.
    let hull = [(-1.1, 0.25), (-0.8, 0.75), (0.8, 0.75), (1.1, 0.25)];
    match icon {
        Icon::Commander => {
            out.push(ring(0.0, 0.0, 0.8));
            out.push(dot(0.0, 0.0, 0.3));
        }
        Icon::SupportCommander => {
            out.push(ring(0.0, 0.0, 0.8));
            out.push(line(&[(-0.4, 0.0), (0.4, 0.0)]));
        }
        Icon::Engineer => {
            out.push(line(&[(-1.0, 0.0), (1.0, 0.0)]));
            out.push(line(&[(-0.5, -0.7), (-0.5, 0.7)]));
            out.push(line(&[(0.5, -0.7), (0.5, 0.7)]));
        }
        Icon::LightBot => {
            out.push(line(&[(-0.9, -0.8), (0.9, 0.8)]));
            out.push(line(&[(-0.9, 0.8), (0.9, -0.8)]));
        }
        Icon::Tank => out.push(line(&[(-0.9, -0.5), (0.9, -0.5), (0.9, 0.5), (-0.9, 0.5), (-0.9, -0.5)])),
        Icon::AmphibiousTank => {
            out.push(line(&[(-0.9, -0.7), (0.9, -0.7), (0.9, 0.1), (-0.9, 0.1), (-0.9, -0.7)]));
            out.push(line(&[(-0.9, 0.6), (-0.45, 0.35), (0.0, 0.6), (0.45, 0.35), (0.9, 0.6)]));
        }
        Icon::Artillery => {
            // A shell lobbed high, from the lower left.
            out.push(arc(0.25, 0.75, 1.05, 180.0, 285.0));
            out.push(dot(0.52, -0.26, 0.24));
            out.push(line(&[(-0.95, 0.85), (0.95, 0.85)]));
        }
        Icon::MobileMissile => {
            out.push(line(&[(0.0, 0.8), (0.0, -0.8)]));
            out.push(line(&[(-0.45, -0.3), (0.0, -0.8), (0.45, -0.3)]));
            out.push(line(&[(-0.45, 0.8), (0.45, 0.8)]));
        }
        Icon::MobileAntiAir => {
            out.push(line(&[(-0.8, 0.0), (0.0, -0.8), (0.8, 0.0)]));
            out.push(line(&[(-0.8, 0.8), (0.0, 0.0), (0.8, 0.8)]));
        }
        Icon::Scout => {
            out.push(ring(0.0, 0.0, 0.55));
            out.push(dot(0.0, 0.0, 0.18));
        }
        Icon::Fighter => {
            out.push(line(&[(-0.9, 0.7), (0.0, -0.8), (0.9, 0.7)]));
            out.push(line(&[(0.0, -0.8), (0.0, 0.7)]));
        }
        Icon::Bomber => out.push(solid(&[(-0.75, -0.55), (0.75, -0.55), (0.0, 0.75)])),
        Icon::FighterBomber => {
            out.push(line(&[(-0.9, 0.2), (0.0, -0.8), (0.9, 0.2)]));
            out.push(solid(&[(-0.35, 0.25), (0.35, 0.25), (0.0, 0.85)]));
        }
        Icon::TorpedoBomber => {
            out.push(line(&[(-0.7, -0.75), (0.7, -0.75), (0.0, 0.3), (-0.7, -0.75)]));
            out.push(line(&[(-0.9, 0.75), (-0.45, 0.5), (0.0, 0.75), (0.45, 0.5), (0.9, 0.75)]));
        }
        Icon::Gunship => {
            out.push(ring(0.0, 0.0, 0.75));
            out.push(line(&[(-0.75, 0.0), (0.75, 0.0)]));
            out.push(line(&[(0.0, -0.75), (0.0, 0.75)]));
        }
        Icon::Frigate => {
            out.push(line(&hull));
            out.push(dot(0.0, -0.15, 0.22));
        }
        Icon::Destroyer => {
            out.push(line(&hull));
            out.push(line(&[(0.0, 0.25), (0.0, -0.45)]));
        }
        Icon::Cruiser => {
            out.push(line(&hull));
            out.push(line(&[(-0.35, 0.25), (-0.35, -0.45)]));
            out.push(line(&[(0.35, 0.25), (0.35, -0.45)]));
        }
        Icon::Battleship => {
            out.push(line(&hull));
            for x in [-0.55, 0.0, 0.55] {
                out.push(line(&[(x, 0.25), (x, -0.6)]));
            }
        }
        Icon::Battlecruiser => {
            out.push(line(&hull));
            out.push(line(&[(-0.6, 0.25), (-0.2, -0.6)]));
            out.push(line(&[(0.1, 0.25), (0.5, -0.6)]));
        }
        Icon::Submarine => {
            out.push(line(&[(-1.0, 0.4), (-0.6, 0.0), (0.6, 0.0), (1.0, 0.4), (0.6, 0.8), (-0.6, 0.8), (-1.0, 0.4)]));
            out.push(line(&[(-0.15, 0.0), (-0.15, -0.5), (0.25, -0.5), (0.25, 0.0)]));
        }
        Icon::MissileSubmarine => {
            out.push(line(&[(-1.0, 0.4), (-0.6, 0.0), (0.6, 0.0), (1.0, 0.4), (0.6, 0.8), (-0.6, 0.8), (-1.0, 0.4)]));
            out.push(line(&[(0.0, 0.0), (0.0, -0.85)]));
            out.push(line(&[(-0.35, -0.5), (0.0, -0.85), (0.35, -0.5)]));
        }
        Icon::ShieldBoat => {
            out.push(line(&hull));
            out.push(arc(0.0, 0.25, 0.75, 180.0, 360.0));
        }
        Icon::LandFactory => {
            out.push(solid(&[(-0.85, -0.2), (0.85, -0.2), (0.85, 0.8), (-0.85, 0.8)]));
            out.push(line(&[(-0.85, -0.2), (-0.85, -0.85), (-0.35, -0.2)]));
            out.push(line(&[(-0.35, -0.2), (-0.35, -0.85), (0.15, -0.2)]));
            out.push(line(&[(0.15, -0.2), (0.15, -0.85), (0.65, -0.2)]));
        }
        Icon::AirFactory => {
            out.push(solid(&[(-0.85, 0.1), (0.85, 0.1), (0.85, 0.8), (-0.85, 0.8)]));
            out.push(line(&[(-0.9, -0.35), (0.0, -0.8), (0.9, -0.35)]));
        }
        Icon::NavalFactory => {
            out.push(solid(&[(-0.85, -0.6), (0.85, -0.6), (0.85, 0.15), (-0.85, 0.15)]));
            out.push(line(&[(-0.9, 0.65), (-0.45, 0.4), (0.0, 0.65), (0.45, 0.4), (0.9, 0.65)]));
        }
        Icon::Gateway => {
            out.push(line(&[(-0.7, 0.8), (-0.7, -0.2)]));
            out.push(line(&[(0.7, 0.8), (0.7, -0.2)]));
            out.push(arc(0.0, -0.2, 0.7, 180.0, 360.0));
            out.push(dot(0.0, 0.3, 0.2));
        }
        Icon::AirStaging => {
            out.push(line(&[(-0.55, -0.75), (-0.55, 0.75)]));
            out.push(line(&[(0.55, -0.75), (0.55, 0.75)]));
            out.push(line(&[(-0.55, 0.0), (0.55, 0.0)]));
        }
        Icon::Extractor => {
            out.push(line(&[(0.0, -0.85), (0.8, 0.0), (0.0, 0.85), (-0.8, 0.0), (0.0, -0.85)]));
            out.push(dot(0.0, 0.0, 0.22));
        }
        Icon::Generator => out.push(line(&[(0.25, -0.9), (-0.45, 0.1), (0.15, 0.1), (-0.25, 0.9)])),
        Icon::Hydrocarbon => {
            out.push(line(&[(0.0, -0.9), (0.55, 0.15), (0.35, 0.65), (0.0, 0.85), (-0.35, 0.65), (-0.55, 0.15), (0.0, -0.9)]));
            out.push(dot(0.0, 0.35, 0.18));
        }
        Icon::MassStorage => {
            out.push(line(&[(-0.85, -0.7), (0.85, -0.7), (0.85, 0.7), (-0.85, 0.7), (-0.85, -0.7)]));
            out.push(line(&[(0.0, -0.4), (0.4, 0.0), (0.0, 0.4), (-0.4, 0.0), (0.0, -0.4)]));
        }
        Icon::EnergyStorage => {
            out.push(line(&[(-0.85, -0.7), (0.85, -0.7), (0.85, 0.7), (-0.85, 0.7), (-0.85, -0.7)]));
            out.push(line(&[(0.15, -0.5), (-0.25, 0.05), (0.15, 0.05), (-0.15, 0.5)]));
        }
        Icon::Fabricator => {
            out.push(line(&[(0.0, -0.85), (0.8, 0.0), (0.0, 0.85), (-0.8, 0.0), (0.0, -0.85)]));
            out.push(line(&[(-0.35, 0.0), (0.35, 0.0)]));
            out.push(line(&[(0.0, -0.35), (0.0, 0.35)]));
        }
        Icon::PointDefense => {
            out.push(ring(0.0, 0.0, 0.55));
            out.push(line(&[(0.0, -0.95), (0.0, 0.95)]));
            out.push(line(&[(-0.95, 0.0), (0.95, 0.0)]));
        }
        Icon::AntiAirTurret => {
            out.push(line(&[(-0.85, 0.15), (-0.4, -0.55), (0.05, 0.15)]));
            out.push(line(&[(-0.05, 0.15), (0.4, -0.55), (0.85, 0.15)]));
            out.push(line(&[(-0.85, 0.7), (0.85, 0.7)]));
        }
        Icon::ArtilleryInstallation => {
            out.push(dot(0.0, 0.35, 0.3));
            out.push(arc(0.0, 0.35, 0.8, 200.0, 340.0));
        }
        Icon::TorpedoLauncher => {
            out.push(line(&[(-0.85, -0.2), (0.7, -0.2)]));
            out.push(line(&[(0.3, -0.55), (0.75, -0.2), (0.3, 0.15)]));
            out.push(line(&[(-0.9, 0.6), (-0.45, 0.35), (0.0, 0.6), (0.45, 0.35), (0.9, 0.6)]));
        }
        Icon::ShieldGenerator => {
            out.push(arc(0.0, 0.6, 0.85, 180.0, 360.0));
            out.push(arc(0.0, 0.6, 0.45, 180.0, 360.0));
        }
        Icon::TacticalLauncher => {
            out.push(line(&[(0.0, 0.85), (0.0, -0.85)]));
            out.push(line(&[(-0.5, -0.35), (0.0, -0.85), (0.5, -0.35)]));
        }
        Icon::TacticalDefense => {
            out.push(line(&[(0.0, 0.85), (0.0, -0.3)]));
            out.push(line(&[(-0.5, 0.2), (0.0, -0.3), (0.5, 0.2)]));
            out.push(line(&[(-0.75, -0.75), (0.75, -0.75)]));
        }
        Icon::StrategicLauncher => {
            out.push(ring(0.0, 0.0, 0.85));
            for angle in [-90.0_f32, 30.0, 150.0] {
                let (from, to) = ((angle - 30.0).to_radians(), (angle + 30.0).to_radians());
                out.push(solid(&[(0.0, 0.0), (from.cos() * 0.75, from.sin() * 0.75), (to.cos() * 0.75, to.sin() * 0.75)]));
            }
        }
        Icon::StrategicDefense => {
            out.push(ring(0.0, 0.0, 0.85));
            out.push(line(&[(-0.55, 0.0), (0.55, 0.0)]));
            out.push(line(&[(0.0, -0.55), (0.0, 0.55)]));
            out.push(dot(0.0, 0.0, 0.2));
        }
        Icon::Radar => {
            out.push(arc(-0.6, 0.6, 0.55, 270.0, 360.0));
            out.push(arc(-0.6, 0.6, 1.05, 270.0, 360.0));
            out.push(dot(-0.6, 0.6, 0.18));
        }
        Icon::Sonar => {
            out.push(arc(0.0, 0.2, 0.45, 200.0, 340.0));
            out.push(arc(0.0, 0.2, 0.85, 200.0, 340.0));
            out.push(line(&[(-0.9, 0.7), (-0.45, 0.45), (0.0, 0.7), (0.45, 0.45), (0.9, 0.7)]));
        }
        Icon::Stealth => {
            out.push(line(&[(-0.9, 0.0), (-0.45, -0.45), (0.45, -0.45), (0.9, 0.0), (0.45, 0.45), (-0.45, 0.45), (-0.9, 0.0)]));
            out.push(line(&[(-0.8, 0.8), (0.8, -0.8)]));
        }
        Icon::TacticalMissile => {
            out.push(solid(&[(0.0, -0.9), (0.3, -0.4), (0.3, 0.7), (-0.3, 0.7), (-0.3, -0.4)]));
        }
        Icon::StrategicMissile => {
            out.push(solid(&[(0.0, -0.9), (0.4, -0.3), (0.4, 0.7), (-0.4, 0.7), (-0.4, -0.3)]));
            out.push(line(&[(-0.75, 0.85), (0.75, 0.85)]));
        }
        Icon::AntiNuke => {
            out.push(line(&[(0.0, 0.85), (0.0, -0.85)]));
            out.push(line(&[(-0.5, -0.35), (0.0, -0.85), (0.5, -0.35)]));
            out.push(ring(0.0, 0.2, 0.5));
        }
        Icon::LandScout => {
            // An eye: a lens with a pupil.
            out.push(line(&[(-0.95, 0.0), (-0.45, -0.45), (0.45, -0.45), (0.95, 0.0), (0.45, 0.45), (-0.45, 0.45), (-0.95, 0.0)]));
            out.push(dot(0.0, 0.0, 0.25));
        }
        Icon::MobileShield => {
            // A dome over a vehicle.
            out.push(arc(0.0, 0.45, 0.85, 180.0, 360.0));
            out.push(line(&[(-0.55, 0.45), (0.55, 0.45), (0.55, 0.8), (-0.55, 0.8), (-0.55, 0.45)]));
        }
        Icon::FieldEngineer => {
            // The engineer's mark with a gun's sight across it.
            out.push(line(&[(-1.0, 0.0), (1.0, 0.0)]));
            out.push(line(&[(-0.5, -0.7), (-0.5, 0.7)]));
            out.push(line(&[(0.5, -0.7), (0.5, 0.7)]));
            out.push(ring(0.0, 0.0, 0.3));
        }
        Icon::TorpedoBoat => {
            out.push(line(&hull));
            out.push(line(&[(-0.6, -0.15), (0.45, -0.15)]));
            out.push(line(&[(0.15, -0.45), (0.5, -0.15), (0.15, 0.15)]));
        }
        Icon::Wall => {
            // Courses of bricks.
            out.push(line(&[(-0.9, -0.6), (0.9, -0.6), (0.9, 0.6), (-0.9, 0.6), (-0.9, -0.6)]));
            out.push(line(&[(-0.9, 0.0), (0.9, 0.0)]));
            out.push(line(&[(0.0, -0.6), (0.0, 0.0)]));
            out.push(line(&[(-0.45, 0.0), (-0.45, 0.6)]));
            out.push(line(&[(0.45, 0.0), (0.45, 0.6)]));
        }
        Icon::EngineeringStation => {
            // A tower with a drone circling it.
            out.push(line(&[(-0.5, 0.85), (0.5, 0.85)]));
            out.push(line(&[(0.0, 0.85), (0.0, -0.1)]));
            out.push(arc(0.0, -0.1, 0.8, 200.0, 340.0));
            out.push(dot(0.55, -0.55, 0.2));
        }
        Icon::Unknown => out.push(dot(0.0, 0.0, 0.25)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::blueprints::BlueprintDb;

    #[test]
    fn every_unit_and_structure_has_an_icon_of_its_own_kind() {
        let db = BlueprintDb::standard();
        for unit in db.iter() {
            assert_ne!(Icon::for_key(&unit.key), Icon::Unknown, "{} has no icon", unit.key);
        }
    }

    #[test]
    fn tiers_of_one_kind_share_an_icon_and_differ_by_dots() {
        assert_eq!(Icon::for_key("t1_air_scout"), Icon::for_key("t3_spy_plane"));
        assert_ne!(Icon::for_key("t1_air_scout"), Icon::for_key("t1_interceptor"));
        let dots = |tier: u8| shapes(Pos2::ZERO, 10.0, Icon::Scout, tier, Color32::WHITE).len();
        assert_eq!(dots(3) - dots(1), 2);
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
        let roles = [Role::Build, Role::Mass, Role::Energy, Role::DirectFire, Role::AntiAir, Role::Indirect, Role::Underwater, Role::Protection, Role::Intel];
        for first in 0..roles.len() {
            for second in first + 1..roles.len() {
                assert_ne!(roles[first].color(), roles[second].color(), "{:?} and {:?} share a color", roles[first], roles[second]);
            }
        }
    }

    #[test]
    fn a_build_bar_symbol_fills_most_of_its_button() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(46.0));
        let bounds = bar_shapes(rect, Icon::LightBot).iter().fold(Rect::NOTHING, |bounds, shape| bounds.union(shape.visual_bounding_rect()));
        assert!(bounds.width() > 23.0 && bounds.height() > 23.0, "the symbol is only {bounds:?}");
        assert!(rect.contains_rect(bounds), "the symbol spills out of its button: {bounds:?}");
    }

    #[test]
    fn every_icon_is_listed() {
        let db = BlueprintDb::standard();
        for unit in db.iter() {
            assert!(Icon::ALL.contains(&Icon::for_key(&unit.key)));
        }
    }
}
