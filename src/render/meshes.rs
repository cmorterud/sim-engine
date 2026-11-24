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
    StrategicInterceptorSilo,
    /// A mast with a tilted dish.
    Radar,
    /// A cone wrapped in rings.
    StealthGenerator,
    /// A slab with four pillars around a glowing core, under a ring.
    RigYard,
    /// A wide, low drum with three stacks.
    GeothermalPlant,
    /// A block wrapped in rings.
    Fabricator,
    /// A fighter: a slim body with swept delta wings and a tail fin, pointing along +Y.
    Aircraft,
    /// A thick body with long straight wings and a tailplane, pointing along +Y.
    Bomber,
    /// A turret with two barrels raised to the sky, which swivels to aim.
    AntiAirGun,
    /// A low deck with four round landing pads and a control tower.
    Airfield,
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
    /// A short hull with a square stern, one gun forward, and a team-colored helipad aft,
    /// pointing along +Y.
    Frigate,
    /// A long, lean hull with a long raked bow and a gun at each end.
    Destroyer,
    /// A hull widest amidships, with radar domes out past its sides on sponsons, so that from
    /// above it is a cross, and missile cells fore and aft.
    Cruiser,
    /// A broad hull with a blunt, round bow and three twin-gun turrets around a stepped tower.
    Battleship,
    /// A long, narrow hull with both turrets forward of a raked tower and missile cells aft.
    Battlecruiser,
    /// A low, rounded hull with a tower amidships.
    Submarine,
    /// A longer submarine with two rows of glowing missile hatches.
    MissileSubmarine,
    /// A trimaran: a slim hull between two outriggers, joined by a team-colored cross deck under
    /// a tall emitter mast.
    EscortCorvette,
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
    /// A heavy fighter, pointing along +Y: a canard delta with a long nose, its wing set far
    /// back, two engines, and two tail fins.
    AirSuperiority,
    /// A long gunship with two rotors, pointing along +Y.
    HeavyGunship,
    /// A light four-wheeled car with a radar mast and a small gun.
    LandScout,
    /// A bot's upper body with a gatling arm and a grenade launcher on its shoulder.
    GrenadierWalker,
    /// A shield emitter on a tracked hull: a mast with a glowing ring and an orb.
    ShieldEmitter,
    /// A broad armored bot's upper body, with a cannon in each arm and shield emitters.
    HeavyWalker,
    /// A bulky bot's upper body, with one heavy cannon over its shoulder.
    SiegeWalker,
    /// A short, wide wedge of a boat with torpedo tubes along its sides and a team-colored
    /// chevron on its foredeck.
    TorpedoBoat,
    /// A gatling turret for the heavy point defense.
    HeavyPointDefense,
    /// A wall section: a block with crenellations along the top.
    Wall,
    /// A field workshop: a pad with a tower, and a drone hovering over it.
    FieldWorkshop,
    /// An air transport, pointing along +Y: a long body with a cargo bay under it and a fan
    /// in a ring at each side.
    Transport,
    /// A recon plane, pointing along +Y: a slim body on long, straight, narrow wings, a T-tail,
    /// and a radar disc on a pylon over its back.
    Scout,
    /// A strike fighter, pointing along +Y: wings swept back, two tail fins, two engines, and
    /// bomb canisters under its wings.
    FighterBomber,
    /// A torpedo bomber, pointing along +Y: twin booms joined by a tailplane, either side of a
    /// short pod on straight wings, with a long torpedo slung under the pod.
    TorpedoBomber,
    /// A shield's dome: an open half-sphere, drawn see-through.
    Dome,
    /// Flat, around the feet of a selected mobile unit.
    SelectionRing,
    /// Flat, around the footprint of a selected structure.
    SelectionBox,
    /// A heavy tank's hull: wide tracks under armored skirts, a long sloped front, and engine
    /// grilles at the back.
    HeavyTankHull,
    /// A heavy tank's big turret, with a long gun, a cupola, and a bustle rack.
    HeavyTank,
    /// A hovercraft's hull: a rounded oval on a rubber skirt, with two big team-colored fan
    /// ducts at the stern. The only vehicle without corners.
    HoverHull,
    /// A long, light six-wheeled truck, with a cab in front and a pedestal on its bed.
    WheeledHull,
    /// A self-propelled heavy gun: a sloped casemate with a team-colored roof and a very long
    /// barrel.
    HeavyArtillery,
    /// A long, low team-colored box of missile tubes along a truck's bed.
    MissileRack,
    /// A bank of energy cells with glowing caps, on a slab.
    EnergyStorage,
    /// A sonar float with a hydrophone mast.
    Sonar,
    /// A deep scan array: a great sensor sphere on a tripod.
    DeepScan,
    /// The concrete foundation a structure on land stands on, with the team's corner marks.
    Apron,
    /// A long, narrow eight-wheeled truck with a cab in front, under a missile launcher's box.
    MissileTruck,
    /// A short, wide four-wheeled carrier with a cab in front, under a mobile shield's dome.
    ShieldCarrier,
    /// A long, narrow tracked hull under a self-propelled gun's casemate.
    CasemateHull,
    /// An engineer's tracked hull, longer, with a team-colored gun turret over its right track.
    CombatEngineerHull,
    /// A team-colored twin anti-aircraft gun on a truck's bed.
    TruckAntiAir,
    /// A mobile flak gun's wide round gun tub in team colors, with four barrels and a radar
    /// panel turning at its back.
    FlakTurret,
    /// A battleship's twin-gun turret, drawn on its own so that it turns to its gun's aim: its
    /// drum stands on the middle of its box, which is the hull's, where it sits on the deck.
    NavalTurret,
    /// A builder's beam to its work: a glowing rod along X, stretched from end to end.
    Beam,
    /// Wiring sunk in the floor between structures sharing adjacency: a dark cable along X with
    /// a glowing strip down its top, stretched from end to end.
    Cable,
    /// Where adjacency wiring meets a structure: a small box with a light on top.
    JunctionBox,
    /// A support factory's bay: the T2 bay's walls, with two booms slanting across its floor in
    /// place of the headquarters' gantries and stacks.
    FactoryT2Support,
    /// The T3 bay's sloped walls, with four booms meeting over it in a diamond, and no gantry,
    /// tower, or stacks.
    FactoryT3Support,
    /// An air factory's deck and pad, with a low service block along its north edge and two
    /// booms slanting over the pad, in place of a hangar and towers.
    AirFactoryT2Support,
    /// An air factory's deck and pad between two service blocks, with four booms meeting over
    /// the pad.
    AirFactoryT3Support,
    /// A dock's piers, with two booms slanting over the slip in place of cranes and a workshop.
    NavalFactoryT2Support,
    /// A dock's piers and lit seawalls, with four booms meeting over the slip in a diamond.
    NavalFactoryT3Support,
    /// A torpedo launcher's turret, low on the water: a wide team-colored drum, and two tubes
    /// under a hood.
    TorpedoLauncher,
    /// The T2 torpedo launcher's: a larger drum, with two pods of tubes either side of a cupola
    /// with a sonar mast.
    TorpedoLauncherT2,
    /// The T2 engineer's turntable, carrying two arms side by side, braced into one.
    EngineerT2,
    /// The T3 engineer's turntable, carrying three arms side by side, braced into one.
    EngineerT3,
    /// The T2 transport: a longer body with two pairs of stub wings and four ducted rotors.
    TransportT2,
    /// The T3 transport: a wide cargo body between two spars, with six ducted rotors.
    TransportT3,
    /// The spy plane: a long slender jet with a swept delta wing, two engine nacelles, and
    /// canted fins.
    SpyPlane,
    /// The field rig's upper body, slender where the commander's is broad: a narrow body, a small
    /// head, a mast and dish over its back, a thin tool arm on its left, and a thin gun on its
    /// right.
    FieldRig,
    /// The field rig's legs: a pelvis and two legs with their knees bent back, each on a
    /// three-toed foot pointing the way it walks.
    FieldRigLegs,
    /// The T2 radar: two huts, a taller mast, and two antennas crossing, turning together.
    RadarT2,
    /// The T2 sonar: a float with three outrigger pontoons on booms, and a taller mast.
    SonarT2,
    /// The T2 point defense's turret: wider, with armored cheeks and two barrels.
    PointDefenseT2,
    /// The T3 artillery's gun, on its emplacement's turntable: a sloped casemate between braces,
    /// and a long barrel laid low, with recoil cylinders and a muzzle brake.
    ArtilleryGunT3,
    /// The T3 shield generator: the T2's column, ringed by four emitter pylons with lit tips.
    ShieldGeneratorT3,
    /// The T3 mass fabricator: a core on a slab, between four cooling fins in an X.
    FabricatorT3,
    /// The T3 field workshop: a pad with two masts, their drones' booms crossing over it.
    FieldWorkshopT3,
    /// The T3 artillery's emplacement, filling its four by four footprint: a slab ringed by
    /// sloped revetments, bunkers at its corners, ammunition rails, and a turntable for the gun.
    ArtilleryEmplacement,
    /// The mobile flak's hull, a half-track, long and narrow, with outriggers either side at
    /// mid-length: wheels, a hood, and a cab at the front, and tracks under a bed behind.
    FlakHull,
    /// What the T1 anti-air gun stands on: a pedestal on four splayed legs, a cross from above
    /// where a point defense is a ring. Each kind of defense has a mount of its own, so that
    /// they are told apart by more than their guns.
    AntiAirMount,
    /// The T2 point defense's mount: the point defense's ring, with a bastion at each corner.
    PointDefenseBaseT2,
    /// The flak cannon's mount: a square pit walled on all four sides, with the gun in the middle.
    FlakPit,
    /// The SAM launcher's mount: a hub with a broad outrigger to each side, a plus from above.
    SamOutriggers,
    /// The T3 heavy point defense's mount: an eight-sided bunker under the ring, buttressed on
    /// each side.
    HeavyDefenseBase,
    /// The T2 artillery's mount: an oblong firing pad with its trails dug in behind and
    /// ammunition stacked either side.
    ArtilleryPad,
}

impl Shape {
    pub const ALL: [Self; 134] = [
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
        Self::StrategicInterceptorSilo,
        Self::Radar,
        Self::StealthGenerator,
        Self::RigYard,
        Self::GeothermalPlant,
        Self::Fabricator,
        Self::Aircraft,
        Self::Bomber,
        Self::AntiAirGun,
        Self::Airfield,
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
        Self::EscortCorvette,
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
        Self::GrenadierWalker,
        Self::ShieldEmitter,
        Self::HeavyWalker,
        Self::SiegeWalker,
        Self::TorpedoBoat,
        Self::HeavyPointDefense,
        Self::Wall,
        Self::FieldWorkshop,
        Self::Transport,
        Self::Scout,
        Self::FighterBomber,
        Self::TorpedoBomber,
        Self::Dome,
        Self::SelectionRing,
        Self::SelectionBox,
        Self::HeavyTankHull,
        Self::HeavyTank,
        Self::HoverHull,
        Self::WheeledHull,
        Self::HeavyArtillery,
        Self::MissileRack,
        Self::EnergyStorage,
        Self::Sonar,
        Self::DeepScan,
        Self::Apron,
        Self::MissileTruck,
        Self::ShieldCarrier,
        Self::CasemateHull,
        Self::CombatEngineerHull,
        Self::TruckAntiAir,
        Self::FlakTurret,
        Self::NavalTurret,
        Self::Beam,
        Self::Cable,
        Self::JunctionBox,
        Self::FactoryT2Support,
        Self::FactoryT3Support,
        Self::AirFactoryT2Support,
        Self::AirFactoryT3Support,
        Self::NavalFactoryT2Support,
        Self::NavalFactoryT3Support,
        Self::TorpedoLauncher,
        Self::TorpedoLauncherT2,
        Self::EngineerT2,
        Self::EngineerT3,
        Self::TransportT2,
        Self::TransportT3,
        Self::SpyPlane,
        Self::FieldRig,
        Self::FieldRigLegs,
        Self::RadarT2,
        Self::SonarT2,
        Self::PointDefenseT2,
        Self::ArtilleryGunT3,
        Self::ShieldGeneratorT3,
        Self::FabricatorT3,
        Self::FieldWorkshopT3,
        Self::ArtilleryEmplacement,
        Self::FlakHull,
        Self::AntiAirMount,
        Self::PointDefenseBaseT2,
        Self::FlakPit,
        Self::SamOutriggers,
        Self::HeavyDefenseBase,
        Self::ArtilleryPad,
    ];
    pub const COUNT: usize = Self::ALL.len();

    /// A fixed-wing aircraft's main wing. From above, sweep tells the role: fighters' wings
    /// sweep well back, bombers' are nearly straight, and the fighter-bomber's are between.
    pub fn main_wing(self) -> Option<Wing> {
        let wing = |root_x, tip_x, root_chord, tip_chord, half_thickness, z| Some(Wing { root_x, tip_x, root_chord, tip_chord, half_thickness, z });
        match self {
            Self::Aircraft => wing(0.07, 0.5, [-0.42, 0.18], [-0.42, -0.3], [0.025, 0.006], -0.01),
            Self::AirSuperiority => wing(0.06, 0.36, [-0.46, 0.1], [-0.46, -0.38], [0.022, 0.006], 0.0),
            Self::FighterBomber => wing(0.08, 0.47, [-0.3, 0.14], [-0.2, -0.07], [0.022, 0.006], -0.01),
            Self::Bomber => wing(0.12, 0.5, [-0.06, 0.2], [0.0, 0.14], [0.03, 0.008], 0.02),
            Self::TorpedoBomber => wing(0.05, 0.5, [-0.06, 0.12], [-0.03, 0.07], [0.025, 0.008], 0.03),
            Self::StrategicBomber => wing(0.08, 0.5, [-0.12, 0.13], [-0.04, 0.07], [0.03, 0.008], 0.0),
            _ => None,
        }
    }

    /// Whether the shape throws a shadow on the ground. Markers, see-through domes, and shells
    /// too small to see do not.
    pub const fn casts_shadow(self) -> bool {
        !matches!(self, Self::Dome | Self::SelectionRing | Self::SelectionBox | Self::Shell | Self::Fireball | Self::Smoke | Self::Wake | Self::Scar | Self::Apron | Self::Beam | Self::Cable)
    }

    /// How many turns a second the shape's spinning parts make, if it has any.
    pub const fn spin_rate(self) -> Option<f32> {
        match self {
            Self::Gunship => Some(2.2),
            Self::Transport | Self::TransportT2 => Some(1.8),
            Self::TransportT3 => Some(1.5),
            Self::HeavyGunship => Some(1.7),
            Self::Radar | Self::RadarT2 => Some(0.25),
            Self::AirFactoryT3 => Some(0.3),
            Self::LandScout => Some(0.6),
            Self::FlakTurret => Some(0.5),
            // The extractor's pump nods as it works.
            Self::Extractor => Some(0.35),
            _ => None,
        }
    }

    /// For a factory, how high the floor of its bay is, as a share of its height: where what it
    /// builds stands while it is built. A dock builds ships afloat, at its foot.
    pub const fn bay_floor(self) -> Option<f32> {
        match self {
            Self::Factory | Self::FactoryT2 | Self::FactoryT3 | Self::FactoryT2Support | Self::FactoryT3Support => Some(0.06),
            Self::AirFactory | Self::AirFactoryT2 | Self::AirFactoryT3 | Self::AirFactoryT2Support | Self::AirFactoryT3Support => Some(0.07),
            Self::NavalFactory | Self::NavalFactoryT2 | Self::NavalFactoryT3 | Self::NavalFactoryT2Support | Self::NavalFactoryT3Support => Some(0.0),
            _ => None,
        }
    }

    /// Whether the shape is a hull that leaves a wake behind it on the water.
    pub const fn leaves_wake(self) -> bool {
        matches!(self, Self::Frigate | Self::Destroyer | Self::Cruiser | Self::Battleship | Self::Battlecruiser | Self::Submarine | Self::MissileSubmarine | Self::EscortCorvette | Self::TorpedoBoat)
    }

    /// Whether the shape runs on tracks, and leaves their marks on the ground behind it.
    pub const fn leaves_tracks(self) -> bool {
        matches!(self, Self::TankHull | Self::HeavyTankHull | Self::ArtilleryHull | Self::EngineerHull | Self::CasemateHull | Self::CombatEngineerHull | Self::FlakHull)
    }

    /// Whether the shape is a pair of legs, which swing as the unit walks.
    pub const fn walks(self) -> bool {
        matches!(self, Self::CommanderLegs | Self::BotLegs | Self::FieldRigLegs)
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
    /// Recessed armor: the body's color darkened, for hatches, vents, and panel insets.
    Panel = 4,
    /// Cockpit canopies, bridge windows, and vision slits: dark, tinted, and shiny.
    Glass = 5,
}

/// A pair of wings, as `MeshBuilder::wings` draws them: from `root_x` out to `tip_x` either
/// side, each chord `[trailing edge, leading edge]` along Y.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Wing {
    pub root_x: f32,
    pub tip_x: f32,
    pub root_chord: [f32; 2],
    pub tip_chord: [f32; 2],
    pub half_thickness: [f32; 2],
    pub z: f32,
}

impl Wing {
    /// How far back the leading edge sweeps from straight across, in degrees.
    pub fn sweep_degrees(&self) -> f32 {
        (self.root_chord[1] - self.tip_chord[1]).atan2(self.tip_x - self.root_x).to_degrees()
    }
}

impl Material {
    pub const ALL: [Self; 6] = [Self::Trim, Self::Hull, Self::Metal, Self::Glow, Self::Panel, Self::Glass];
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
    /// How far up the top byte holds how open the vertex is to the sky: 255 for nothing in
    /// the way, less where other parts of the shape, or the ground, crowd it.
    pub const OPENNESS_SHIFT: u32 = 24;
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
    indices: Vec<u32>,
    /// What the parts added next are made of.
    material: Material,
    /// How the parts added next move, and about where.
    motion: u32,
    pivot: Vec3,
    /// Whether large faces are cut into grids, for shapes shaded where their parts meet.
    subdivide: bool,
    /// How far each side of a box drawn now leans in, as a share of its height (`ARMOR_SLOPE`),
    /// or zero for upright boxes.
    slope: f32,
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
        let (sides, rings) = (round_sides(sides, radius), round_sides(rings * 2, radius) / 2);
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
        let base = self.vertices.len() as u32;
        let vertex = |point: Vec3| MeshVertex {
            position: point.to_array(),
            normal: normal.to_array(),
            material: self.material as u32 | self.motion | (255 << motion::OPENNESS_SHIFT),
            pivot: self.pivot.to_array(),
        };
        // A large four-sided face is cut into a grid, so that the shading baked into its
        // vertices where parts meet can show across it, not only at its corners.
        if let (&[p0, p1, p2, p3], true) = (points, self.subdivide) {
            let across = ((p1 - p0).length().max((p2 - p3).length()) / SUBDIVIDE_EDGE).ceil().max(1.0) as u32;
            let along = ((p3 - p0).length().max((p2 - p1).length()) / SUBDIVIDE_EDGE).ceil().max(1.0) as u32;
            if across * along > 1 {
                for j in 0..=along {
                    let v = j as f32 / along as f32;
                    for i in 0..=across {
                        let u = i as f32 / across as f32;
                        let point = (p0 * (1.0 - u) + p1 * u) * (1.0 - v) + (p3 * (1.0 - u) + p2 * u) * v;
                        self.vertices.push(vertex(point));
                    }
                }
                let at = |i: u32, j: u32| base + j * (across + 1) + i;
                for j in 0..along {
                    for i in 0..across {
                        let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
                        if flipped {
                            self.indices.extend_from_slice(&[a, c, b, a, d, c]);
                        } else {
                            self.indices.extend_from_slice(&[a, b, c, a, c, d]);
                        }
                    }
                }
                return;
            }
        }
        for point in points {
            self.vertices.push(vertex(*point));
        }
        for index in 1..points.len() as u32 - 1 {
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
        let [lean_x, lean_y] = self.slope_inset(max[0] - min[0], max[1] - min[1], max[2] - min[2]);
        let corners = |z: f32, x: f32, y: f32| {
            [
                Vec3::new(min[0] + x, min[1] + y, z),
                Vec3::new(max[0] - x, min[1] + y, z),
                Vec3::new(max[0] - x, max[1] - y, z),
                Vec3::new(min[0] + x, max[1] - y, z),
            ]
        };
        self.cuboid(corners(min[2], 0.0, 0.0), corners(max[2], lean_x, lean_y));
    }

    /// How far in the top of a box `width` across, `depth` deep, and `height` tall is drawn, either
    /// way, for the slope its sides lean at now: never more than `SLOPE_INSET_MOST` of the box.
    fn slope_inset(&self, width: f32, depth: f32, height: f32) -> [f32; 2] {
        let lean = self.slope * height.max(0.0);
        [lean.min(width * SLOPE_INSET_MOST), lean.min(depth * SLOPE_INSET_MOST)]
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
        let lower = max[2] - cut;
        self.block(min, [max[0], max[1], lower]);
        // The cut starts where the box's sides end, leaning in if they do.
        let (width, depth) = (max[0] - min[0], max[1] - min[1]);
        let [lean_x, lean_y] = self.slope_inset(width, depth, lower - min[2]);
        let corners = |x: f32, y: f32, z: f32| {
            [
                Vec3::new(min[0] + x, min[1] + y, z),
                Vec3::new(max[0] - x, min[1] + y, z),
                Vec3::new(max[0] - x, max[1] - y, z),
                Vec3::new(min[0] + x, max[1] - y, z),
            ]
        };
        self.cuboid(corners(lean_x, lean_y, lower), corners((lean_x + cut).min(width * 0.45), (lean_y + cut).min(depth * 0.45), max[2]));
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

    /// A ship's hull from stern to bow along Y, from one side of its outline as seen from above:
    /// `[half width, y]` from the stern to the bow, which the other side mirrors. The outline is
    /// what tells one ship from another at a distance, so each ship has its own. Narrower at the
    /// keel, with a dark band at the waterline and a raised forecastle from `forecastle` forward.
    /// The deck is at -0.2, and -0.16 on the forecastle.
    fn hull(&mut self, side: &[[f32; 2]], forecastle: f32) {
        let both_sides = |side: &[[f32; 2]]| side.iter().copied().chain(side.iter().rev().map(|&[x, y]| [-x, y])).collect::<Vec<_>>();
        let outline = both_sides(side);
        self.paint(Material::Metal);
        self.prism(&outline, [-0.5, -0.46], [0.62, 0.67]);
        self.paint(Material::Hull);
        self.prism(&outline, [-0.46, -0.2], [0.67, 1.0]);
        // The side forward of the forecastle's break, starting where it crosses the side.
        let crossing = side.windows(2).find(|pair| pair[0][1] <= forecastle && pair[1][1] > forecastle).map(|pair| {
            let along = (forecastle - pair[0][1]) / (pair[1][1] - pair[0][1]);
            [pair[0][0] + (pair[1][0] - pair[0][0]) * along, forecastle]
        });
        let fore: Vec<[f32; 2]> = crossing.into_iter().chain(side.iter().copied().filter(|point| point[1] > forecastle)).collect();
        self.prism(&both_sides(&fore), [-0.2, -0.16], [1.0, 1.0]);
    }

    /// A rounded body along Y, such as a fuselage or an engine, through cross-sections of
    /// `sides` sides, each `[y, half width, half height, height of its middle]`, centered on `x`.
    fn body(&mut self, x: f32, stations: &[[f32; 4]], sides: usize) {
        let sides = round_sides(sides, stations.iter().map(|station| station[1].max(station[2])).fold(0.0, f32::max));
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

    /// The outer `share` of a pair of `wings` drawn with the same arguments, in the current
    /// material, a little thicker so that it wraps the wing under it: team-colored wingtips,
    /// which show from above.
    /// The shape's main wing (`Shape::main_wing`), and its tips in the given paint over the
    /// outer share of it, if any.
    fn main_wing(&mut self, shape: Shape, tips: Option<(Material, f32)>) {
        let Wing { root_x, tip_x, root_chord, tip_chord, half_thickness, z } = shape.main_wing().expect("a fixed-wing aircraft");
        self.wings(root_x, tip_x, root_chord, tip_chord, half_thickness, z);
        if let Some((material, share)) = tips {
            let before = self.material;
            self.paint(material);
            self.wing_tips(root_x, tip_x, root_chord, tip_chord, half_thickness, z, share);
            self.paint(before);
        }
    }

    // The same arguments as `wings`, to be called with them, and the share.
    #[allow(clippy::too_many_arguments)]
    fn wing_tips(&mut self, root_x: f32, tip_x: f32, root_chord: [f32; 2], tip_chord: [f32; 2], half_thickness: [f32; 2], z: f32, share: f32) {
        let along = 1.0 - share;
        let lerp = |from: f32, to: f32| from + (to - from) * along;
        let inner_x = lerp(root_x, tip_x);
        let inner_chord = [lerp(root_chord[0], tip_chord[0]), lerp(root_chord[1], tip_chord[1])];
        let inner_thickness = lerp(half_thickness[0], half_thickness[1]);
        let wrap = 0.003_f32;
        let inside = |value: f32| value.clamp(-0.5, 0.5);
        self.wings(
            inner_x,
            inside(tip_x + wrap.copysign(tip_x)),
            [inside(inner_chord[0] - wrap), inside(inner_chord[1] + wrap)],
            [inside(tip_chord[0] - wrap), inside(tip_chord[1] + wrap)],
            [inner_thickness + wrap, half_thickness[1] + wrap],
            z,
        );
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

    /// The floor of a land factory's bay, ramps down from both of its open ends, east and west,
    /// and guide lights along it, lit while it builds.
    fn factory_floor(&mut self) {
        self.paint(Material::Hull);
        self.block([-0.42, -0.5, -0.5], [0.42, 0.5, -0.44]);
        for side in [-1.0_f32, 1.0] {
            let (inner, outer) = (0.42 * side, 0.5 * side);
            let (low, high) = (inner.min(outer), inner.max(outer));
            let z = |x: f32| if (x - inner).abs() < 1e-6 { -0.44 } else { -0.495 };
            self.cuboid(
                [Vec3::new(low, -0.34, -0.5), Vec3::new(high, -0.34, -0.5), Vec3::new(high, 0.34, -0.5), Vec3::new(low, 0.34, -0.5)],
                [Vec3::new(low, -0.34, z(low)), Vec3::new(high, -0.34, z(high)), Vec3::new(high, 0.34, z(high)), Vec3::new(low, 0.34, z(low))],
            );
        }
        // Guide lights along the floor, lit while it builds.
        self.paint(Material::Glow);
        self.moving(motion::PULSE, [0.0; 3], |mesh| {
            for y in [-0.33_f32, 0.33] {
                mesh.block([-0.4, y - 0.01, -0.44], [0.4, y + 0.01, -0.43]);
            }
        });
    }

    /// The T2 bay's higher walls, with buttresses, a row of windows, and the team's stripe.
    fn bay_walls_t2(&mut self) {
        for (y, outward) in [(-0.47_f32, -1.0_f32), (0.31, 1.0)] {
            self.paint(Material::Hull);
            self.bevel([-0.46, y, -0.44], [0.46, y + 0.16, 0.0], 0.03);
            for x in [-0.3_f32, 0.0, 0.3] {
                let face = if outward < 0.0 { y } else { y + 0.16 };
                self.bevel([x - 0.04, face.min(face + 0.03 * outward), -0.44], [x + 0.04, face.max(face + 0.03 * outward), -0.08], 0.01);
            }
            self.paint(Material::Glow);
            let inner = if outward < 0.0 { y + 0.16 } else { y };
            self.block([-0.4, inner - 0.005, -0.2], [0.4, inner + 0.005, -0.14]);
            self.paint(Material::Trim);
            self.block([-0.46, y, 0.0], [0.46, y + 0.16, 0.05]);
        }
    }

    /// The T3 bay's heavy walls, with sloped armor, inside faces lit, and the team's stripe on top.
    fn bay_walls_t3(&mut self) {
        for (y, outward) in [(-0.5_f32, -1.0_f32), (0.32, 1.0)] {
            let (outer, inner) = if outward < 0.0 { (y, y + 0.18) } else { (y + 0.18, y) };
            let top_outer = outer - 0.07 * outward;
            self.paint(Material::Hull);
            let (low, high) = (outer.min(inner), outer.max(inner));
            let (top_low, top_high) = (top_outer.min(inner), top_outer.max(inner));
            self.cuboid(
                [Vec3::new(-0.47, low, -0.44), Vec3::new(0.47, low, -0.44), Vec3::new(0.47, high, -0.44), Vec3::new(-0.47, high, -0.44)],
                [Vec3::new(-0.47, top_low, 0.06), Vec3::new(0.47, top_low, 0.06), Vec3::new(0.47, top_high, 0.06), Vec3::new(-0.47, top_high, 0.06)],
            );
            self.paint(Material::Glow);
            self.block([-0.42, inner - 0.006, -0.3], [0.42, inner + 0.006, -0.25]);
            self.block([-0.42, inner - 0.006, -0.1], [0.42, inner + 0.006, -0.06]);
            self.paint(Material::Trim);
            self.block([-0.47, top_low, 0.06], [0.47, top_high, 0.1]);
            // And a band across the sloped armor outside.
            let face = |z: f32| outer + (top_outer - outer) * ((z + 0.44) / 0.5);
            let ring = |z: f32| {
                let (inside, outside) = (face(z) - 0.002 * outward, face(z) + 0.012 * outward);
                [Vec3::new(-0.47, inside, z), Vec3::new(0.47, inside, z), Vec3::new(0.47, outside, z), Vec3::new(-0.47, outside, z)]
            };
            self.cuboid(ring(-0.24), ring(-0.12));
        }
    }

    /// An air factory's deck, with a round pad in the middle for what it builds, ringed with
    /// lights, and lines painted across it.
    fn air_deck(&mut self) {
        self.paint(Material::Hull);
        self.bevel([-0.5, -0.5, -0.5], [0.5, 0.5, -0.44], 0.02);
        self.paint(Material::Metal);
        self.frustum([0.0, 0.0], [-0.44, -0.43], [0.31, 0.31], 20);
        self.paint(Material::Glow);
        self.moving(motion::PULSE, [0.0; 3], |mesh| {
            for step in 0..10 {
                let angle = step as f32 / 10.0 * std::f32::consts::TAU;
                let (x, y) = (angle.cos() * 0.33, angle.sin() * 0.33);
                mesh.block([x - 0.015, y - 0.015, -0.44], [x + 0.015, y + 0.015, -0.42]);
            }
        });
        // Painted lines across the pad.
        self.paint(Material::Trim);
        self.block([-0.22, -0.02, -0.43], [0.22, 0.02, -0.425]);
    }

    /// A dock's piers down the long sides, north and south, with open water between them: a slip
    /// open at both ends, east and west, where ships are built afloat and sail out. With the
    /// team's stripe, bollards, and buoy lights at the open ends.
    fn piers(&mut self) {
        self.paint(Material::Hull);
        for y in [-0.5_f32, 0.32] {
            self.bevel([-0.5, y, -0.5], [0.5, y + 0.18, -0.32], 0.03);
        }
        // The team's stripe along each pier's outer edge.
        self.paint(Material::Trim);
        for y in [-0.49_f32, 0.43] {
            self.block([-0.47, y, -0.33], [0.47, y + 0.06, -0.31]);
        }
        // Bollards along the piers' inner edges.
        self.paint(Material::Metal);
        for y in [-0.33_f32, 0.33] {
            for x in [-0.4_f32, -0.2, 0.0, 0.2, 0.4] {
                self.frustum([x, y], [-0.32, -0.28], [0.015, 0.015], 6);
            }
        }
        // Buoy lights at the open ends.
        self.paint(Material::Glow);
        self.moving(motion::PULSE, [0.0; 3], |mesh| {
            for x in [-0.48_f32, 0.48] {
                for y in [-0.3_f32, 0.3] {
                    mesh.block([x - 0.02, y - 0.02, -0.34], [x + 0.02, y + 0.02, -0.3]);
                }
            }
        });
    }

    /// Lights along the faces of a T3 dock's seawalls.
    fn lit_seawalls(&mut self) {
        self.paint(Material::Glow);
        for y in [-0.325_f32, 0.325] {
            self.block([-0.48, y - 0.006, -0.38], [0.48, y + 0.006, -0.34]);
        }
    }

    /// A support factory's service arm: a team-colored housing standing on `foot`, and a jointed
    /// boom reaching `reach` out from it, across and along, with a tool at its end that glows
    /// while it builds. Support factories are built with booms slanting over the bay in place of
    /// their headquarters' straight gantries, cranes, and towers, since from the camera's height
    /// how parts lie across the ground shows far more than how high they stand.
    fn service_arm(&mut self, foot: [f32; 3], reach: [f32; 2]) {
        let [x, y, z] = foot;
        self.paint(Material::Trim);
        self.bevel([x - 0.075, y - 0.075, z], [x + 0.075, y + 0.075, z + 0.08], 0.02);
        let shoulder = [x, y, z + 0.07];
        let elbow = [x + reach[0] * 0.4, y + reach[1] * 0.4, z + 0.22];
        let wrist = [x + reach[0], y + reach[1], z + 0.12];
        self.paint(Material::Metal);
        self.beam(shoulder, elbow, 0.045, 0.04);
        self.beam(elbow, wrist, 0.038, 0.032);
        self.paint(Material::Glow);
        self.moving(motion::PULSE, [0.0; 3], |mesh| mesh.block([wrist[0] - 0.035, wrist[1] - 0.035, wrist[2] - 0.08], [wrist[0] + 0.035, wrist[1] + 0.035, wrist[2]]));
    }

    /// A low team-colored service block across an air deck, between the two `y`, on a plinth, for
    /// service arms to stand on.
    fn service_block(&mut self, y: [f32; 2]) {
        self.paint(Material::Hull);
        self.block([-0.45, y[0], -0.44], [0.45, y[1], -0.41]);
        self.paint(Material::Trim);
        self.bevel([-0.44, y[0], -0.41], [0.44, y[1], -0.27], 0.02);
    }

    /// A torpedo launcher's turret, low on the water: a wide team-colored drum of `radius`, and a
    /// pod of two tubes under a hood at each of `pods` across it.
    fn torpedo_turret(&mut self, radius: f32, tube: f32, pods: &[f32]) {
        self.paint(Material::Trim);
        self.frustum([0.0, -0.04], [-0.05, 0.06], [radius, radius * 0.88], 10);
        for &x in pods {
            self.torpedo_pod(x, tube);
        }
    }

    /// Two torpedo tubes side by side about `x`, which kick back as they fire, their mouths lit,
    /// under an armored hood.
    fn torpedo_pod(&mut self, x: f32, tube: f32) {
        let z = 0.04;
        for side in [-1.0_f32, 1.0] {
            let at = x + side * tube * 1.15;
            self.paint(Material::Metal);
            self.moving(motion::RECOIL, [at, 0.0, z], |mesh| mesh.body(at, &[[-0.14, tube, tube, z], [0.46, tube, tube, z]], 8));
            self.paint(Material::Glow);
            self.block([at - tube * 0.6, 0.46, z - tube * 0.6], [at + tube * 0.6, 0.47, z + tube * 0.6]);
        }
        let half = tube * 2.15 + 0.02;
        self.paint(Material::Hull);
        self.bevel([x - half, -0.24, z + tube], [x + half, 0.16, z + tube + 0.07], 0.02);
    }

    /// An engineer's turntable in team colors, carrying `arms` jointed arms side by side, braced
    /// together at the elbow and the wrist so that they move as one, each with a tool at its end
    /// that glows at work. An engineer's tech level is the number of its arms.
    fn engineer_arms(&mut self, arms: usize) {
        let spacing = 0.12;
        let xs: Vec<f32> = (0..arms).map(|index| (index as f32 - (arms as f32 - 1.0) * 0.5) * spacing).collect();
        let half = xs.last().copied().unwrap_or(0.0);
        self.paint(Material::Trim);
        self.frustum([0.0, -0.12], [-0.12, 0.02], [0.24 + half * 0.4, 0.2 + half * 0.4], 8);
        self.paint(Material::Metal);
        self.frustum([0.0, -0.12], [0.02, 0.1], [0.08 + half * 0.8, 0.07 + half * 0.8], 6);
        for &x in &xs {
            self.paint(Material::Hull);
            self.beam([x, -0.12, 0.06], [x, 0.12, 0.36], 0.05, 0.05);
            self.beam([x, 0.12, 0.36], [x, 0.36, 0.3], 0.042, 0.04);
            self.paint(Material::Metal);
            self.frustum([x, 0.38], [0.3, 0.36], [0.045, 0.045], 6);
            self.paint(Material::Glow);
            self.moving(motion::PULSE, [x, 0.38, 0.25], |mesh| mesh.frustum([x, 0.38], [0.18, 0.3], [0.026, 0.056], 6));
        }
        self.paint(Material::Metal);
        self.block([-half - 0.05, 0.08, 0.3], [half + 0.05, 0.16, 0.42]);
        if arms > 1 {
            self.block([-half - 0.03, 0.33, 0.27], [half + 0.03, 0.39, 0.33]);
        }
    }

    /// A field workshop's pad, with the team's edging round it.
    fn workshop_pad(&mut self) {
        self.paint(Material::Hull);
        self.bevel([-0.5, -0.5, -0.5], [0.5, 0.5, -0.38], 0.03);
        self.paint(Material::Trim);
        for (min, max) in [([-0.47, -0.47], [0.47, -0.41]), ([-0.47, 0.41], [0.47, 0.47]), ([-0.47, -0.41], [-0.41, 0.41]), ([0.41, -0.41], [0.47, 0.41])] {
            self.block([min[0], min[1], -0.39], [max[0], max[1], -0.37]);
        }
    }

    /// A field workshop's mast standing at `foot`, its boom at `height` reaching out to a drone
    /// hovering over `drone`, lit while it works.
    fn workshop_mast(&mut self, foot: [f32; 2], drone: [f32; 2], height: f32) {
        self.paint(Material::Trim);
        self.frustum(foot, [-0.38, 0.12], [0.16, 0.12], 8);
        self.paint(Material::Metal);
        self.frustum(foot, [0.12, height - 0.04], [0.04, 0.03], 6);
        self.beam([foot[0], foot[1], height - 0.06], [drone[0] - 0.01, drone[1] - 0.01, height], 0.025, 0.025);
        self.paint(Material::Hull);
        self.bevel([drone[0] - 0.11, drone[1] - 0.11, height - 0.1], [drone[0] + 0.11, drone[1] + 0.11, height - 0.02], 0.02);
        self.paint(Material::Glow);
        self.moving(motion::PULSE, [0.0; 3], |mesh| mesh.ball([drone[0], drone[1], height - 0.13], 0.04, 6, 3));
    }

    /// Lights at the corners of a field workshop's pad, lit while it works.
    fn workshop_lights(&mut self) {
        self.paint(Material::Glow);
        self.moving(motion::PULSE, [0.0; 3], |mesh| {
            for (x, y) in [(-0.4_f32, -0.4_f32), (0.4, -0.4), (-0.4, 0.4), (0.4, 0.4)] {
                mesh.block([x - 0.02, y - 0.02, -0.38], [x + 0.02, y + 0.02, -0.36]);
            }
        });
    }

    /// A transport's ducted rotor at `at`, of `radius`: a duct, a team-colored ring over it if
    /// `ring`, and two blades crossing inside, which turn.
    fn ducted_rotor(&mut self, at: [f32; 2], radius: f32, ring: bool) {
        let [x, y] = at;
        self.paint(Material::Metal);
        self.frustum(at, [0.0, 0.1], [radius, radius], 12);
        if ring {
            self.paint(Material::Panel);
            self.frustum(at, [0.1, 0.104], [radius * 0.83, radius * 0.83], 12);
        }
        self.paint(Material::Trim);
        let blade = radius * 0.8;
        self.moving(motion::SPIN, [x, y, 0.0], |mesh| {
            mesh.block([x - blade, y - 0.018, 0.104], [x + blade, y + 0.018, 0.118]);
            mesh.block([x - 0.018, y - blade, 0.104], [x + 0.018, y + blade, 0.118]);
        });
    }

    /// A point defense's mount: a broad skirt, a team-colored ring, and a pedestal for the gun.
    fn defense_ring(&mut self) {
        self.paint(Material::Hull);
        self.frustum([0.0, 0.0], [-0.5, -0.32], [0.48, 0.44], 8);
        self.paint(Material::Trim);
        self.frustum([0.0, 0.0], [-0.32, -0.25], [0.42, 0.4], 8);
        self.paint(Material::Hull);
        self.frustum([0.0, 0.0], [-0.25, -0.05], [0.3, 0.24], 8);
    }

    /// A round-ish solid from one radius to another. A top radius of zero makes a cone.
    fn frustum(&mut self, center: [f32; 2], z: [f32; 2], radius: [f32; 2], sides: usize) {
        let sides = round_sides(sides, radius[0].max(radius[1]));
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

/// One side of a bow, from `start` round to its tip at the front of the box, bending toward
/// `control`, as `[half width, y]` for `MeshBuilder::hull`, not including `start`.
fn bow(start: [f32; 2], control: [f32; 2], steps: usize) -> Vec<[f32; 2]> {
    let tip = [0.015, 0.5];
    (1..=steps)
        .map(|step| {
            let along = step as f32 / steps as f32;
            let rest = 1.0 - along;
            std::array::from_fn(|axis| rest * rest * start[axis] + 2.0 * rest * along * control[axis] + along * along * tip[axis])
        })
        .collect()
}

/// A hull's side with straight sides from `stern` to `bow_start`, and a bow from there.
fn side(stern: &[[f32; 2]], bow_start: [f32; 2], control: [f32; 2], steps: usize) -> Vec<[f32; 2]> {
    let mut side = stern.to_vec();
    side.push(bow_start);
    side.extend(bow(bow_start, control, steps));
    side
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
        Shape::Engineer => mesh.engineer_arms(1),
        // A narrow two-legged walker, with a gun in its weapons bay and its fabrication bay raised.
        Shape::Bot => {
            mesh.bevel([-0.22, -0.18, -0.02], [0.22, 0.18, 0.36], 0.05);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.2, -0.18, 0.36], [0.2, -0.02, 0.4], 0.02);
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
            // Long and lean: narrow tracks and a long hull, under a turret that overhangs it.
            mesh.paint(Material::Metal);
            for (low, high) in [(-0.3_f32, -0.16_f32), (0.16, 0.3)] {
                mesh.bevel([low, -0.5, -0.5], [high, 0.34, -0.27], 0.05);
            }
            mesh.paint(Material::Hull);
            for (low, high) in [(-0.31_f32, -0.15_f32), (0.15, 0.31)] {
                mesh.block([low, -0.5, -0.27], [high, 0.36, -0.24]);
            }
            mesh.cuboid(
                [Vec3::new(-0.17, -0.47, -0.44), Vec3::new(0.17, -0.47, -0.44), Vec3::new(0.17, 0.36, -0.44), Vec3::new(-0.17, 0.36, -0.44)],
                [Vec3::new(-0.17, -0.47, -0.1), Vec3::new(0.17, -0.47, -0.1), Vec3::new(0.17, 0.12, -0.1), Vec3::new(-0.17, 0.12, -0.1)],
            );
            mesh.paint(Material::Metal);
            for y in [-0.42_f32, -0.35, -0.28] {
                mesh.block([-0.11, y - 0.022, -0.1], [0.11, y + 0.022, -0.085]);
            }
        }
        // A team-colored turret with a sloped front, a mantlet, a long gun with a muzzle brake,
        // and a hatch.
        Shape::Tank => {
            // A rounded cast turret in team colors: round from above, where the heavy tank's is
            // a long wedge.
            mesh.frustum([0.0, -0.06], [-0.1, 0.1], [0.22, 0.16], 12);
            mesh.frustum([0.0, -0.06], [0.1, 0.13], [0.16, 0.1], 12);
            mesh.paint(Material::Metal);
            mesh.block([-0.07, 0.13, -0.04], [0.07, 0.19, 0.07]);
            mesh.moving(motion::RECOIL, [0.0, 0.16, 0.015], |mesh| {
                mesh.body(0.0, &[[0.17, 0.034, 0.034, 0.015], [0.43, 0.03, 0.03, 0.015]], 6);
                mesh.body(0.0, &[[0.43, 0.046, 0.046, 0.015], [0.49, 0.046, 0.046, 0.015]], 6);
            });
            mesh.flash([0.0, 0.47, 0.015], 0.06);
            mesh.paint(Material::Hull);
            mesh.frustum([0.08, -0.15], [0.13, 0.18], [0.05, 0.045], 8);
        }
        Shape::HeavyTankHull => {
            // A wide slab, wider than it is long: tracks out to the edges, under skirts.
            mesh.paint(Material::Metal);
            for (low, high) in [(-0.47_f32, -0.31_f32), (0.31, 0.47)] {
                mesh.bevel([low, -0.46, -0.5], [high, 0.3, -0.26], 0.05);
            }
            // Armored skirts over the upper run of the tracks, team-colored along their length:
            // the heavy tank's mark from above.
            mesh.paint(Material::Hull);
            for (low, high) in [(-0.49_f32, -0.29_f32), (0.29, 0.49)] {
                mesh.bevel([low, -0.44, -0.38], [high, 0.28, -0.2], 0.02);
            }
            mesh.paint(Material::Trim);
            for (low, high) in [(-0.48_f32, -0.31_f32), (0.31, 0.48)] {
                mesh.block([low, -0.42, -0.2], [high, 0.26, -0.17]);
            }
            // The body, with a long sloped front.
            mesh.paint(Material::Hull);
            mesh.cuboid(
                [Vec3::new(-0.31, -0.44, -0.46), Vec3::new(0.31, -0.44, -0.46), Vec3::new(0.31, 0.32, -0.46), Vec3::new(-0.31, 0.32, -0.46)],
                [Vec3::new(-0.31, -0.44, -0.06), Vec3::new(0.31, -0.44, -0.06), Vec3::new(0.31, 0.06, -0.06), Vec3::new(-0.31, 0.06, -0.06)],
            );
            // Engine grilles on the back deck.
            mesh.paint(Material::Panel);
            for y in [-0.39_f32, -0.31, -0.23] {
                mesh.block([-0.2, y - 0.025, -0.06], [0.2, y + 0.025, -0.05]);
            }
        }
        Shape::HeavyTank => {
            // A wide wedge, sharp in front and long behind, over the gun's mantlet.
            mesh.paint(Material::Hull);
            mesh.prism(&[[-0.24, -0.42], [0.24, -0.42], [0.3, -0.12], [0.3, 0.02], [0.14, 0.18], [-0.14, 0.18], [-0.3, 0.02], [-0.3, -0.12]], [-0.06, 0.16], [1.0, 0.85]);
            mesh.paint(Material::Metal);
            mesh.block([-0.1, 0.13, -0.02], [0.1, 0.2, 0.11]);
            mesh.moving(motion::RECOIL, [0.0, 0.18, 0.045], |mesh| {
                mesh.body(0.0, &[[0.18, 0.046, 0.046, 0.045], [0.44, 0.038, 0.038, 0.045]], 6);
                mesh.body(0.0, &[[0.43, 0.058, 0.058, 0.045], [0.5, 0.058, 0.058, 0.045]], 6);
            });
            mesh.flash([0.0, 0.46, 0.045], 0.07);
            // A commander's cupola, a vision slit, and a stowage rack at the end of the bustle.
            mesh.paint(Material::Hull);
            mesh.frustum([0.1, -0.16], [0.14, 0.22], [0.07, 0.06], 8);
            mesh.paint(Material::Glass);
            mesh.block([-0.16, 0.12, 0.07], [-0.06, 0.135, 0.1]);
            mesh.paint(Material::Panel);
            mesh.block([-0.2, -0.47, -0.02], [0.2, -0.41, 0.1]);
        }
        Shape::HoverHull => {
            // A rounded oval: a rectangle with deep round corners, the only vehicle without
            // corners.
            let (half_width, corner) = (0.346_f32, 0.2_f32);
            let mut outline = Vec::new();
            for (cx, cy, from) in [(half_width - corner, 0.5 - corner, 0.0_f32), (corner - half_width, 0.5 - corner, 90.0), (corner - half_width, corner - 0.5, 180.0), (half_width - corner, corner - 0.5, 270.0)] {
                for step in 0..=4 {
                    let angle = (from + step as f32 * 22.5).to_radians();
                    outline.push([cx + angle.cos() * corner, cy + angle.sin() * corner]);
                }
            }
            mesh.paint(Material::Metal);
            mesh.prism(&outline, [-0.5, -0.36], [0.92, 1.0]);
            mesh.paint(Material::Hull);
            mesh.prism(&outline, [-0.36, -0.1], [0.9, 0.8]);
            // Two big fan ducts in the team's colors at the stern.
            for x in [-0.15_f32, 0.15] {
                mesh.paint(Material::Metal);
                mesh.beam([x, -0.28, -0.1], [x, -0.34, 0.0], 0.02, 0.02);
                mesh.paint(Material::Trim);
                mesh.body(x, &[[-0.47, 0.13, 0.13, -0.02], [-0.34, 0.13, 0.13, -0.02]], 10);
                mesh.paint(Material::Panel);
                mesh.body(x, &[[-0.485, 0.095, 0.095, -0.02], [-0.465, 0.095, 0.095, -0.02]], 10);
            }
        }
        Shape::WheeledHull => {
            // Long and narrow, twice as long as it is wide, on six wheels.
            mesh.paint(Material::Metal);
            for x in [-0.19_f32, 0.19] {
                for y in [-0.36_f32, -0.17, 0.32] {
                    mesh.bevel([x - 0.045, y - 0.075, -0.5], [x + 0.045, y + 0.075, -0.32], 0.02);
                }
            }
            // Fenders over the wheels.
            mesh.paint(Material::Hull);
            for x in [-0.19_f32, 0.19] {
                for y in [-0.265_f32, 0.32] {
                    let reach = if y < 0.0 { 0.19 } else { 0.09 };
                    mesh.block([x - 0.05, y - reach, -0.32], [x + 0.05, y + reach, -0.29]);
                }
            }
            mesh.paint(Material::Hull);
            mesh.bevel([-0.15, -0.48, -0.44], [0.15, 0.48, -0.28], 0.02);
            mesh.bevel([-0.14, -0.48, -0.28], [0.14, 0.22, -0.22], 0.02);
            // The cab in front, with its windscreen, and a pedestal on the bed for what it carries.
            mesh.bevel([-0.14, 0.24, -0.28], [0.14, 0.46, -0.06], 0.03);
            mesh.paint(Material::Trim);
            mesh.block([-0.1, 0.27, -0.06], [0.1, 0.43, -0.045]);
            mesh.paint(Material::Glass);
            mesh.block([-0.11, 0.46, -0.18], [0.11, 0.475, -0.1]);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.08], [-0.22, -0.16], [0.09, 0.08], 8);
        }
        Shape::HeavyArtillery => {
            // A sloped casemate in armor, with a team-colored roof, and a very long barrel.
            mesh.paint(Material::Hull);
            mesh.bevel([-0.17, -0.42, -0.16], [0.17, 0.1, 0.06], 0.04);
            mesh.paint(Material::Trim);
            mesh.block([-0.13, -0.38, 0.06], [0.13, 0.02, 0.075]);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, 0.06], [-0.08, 0.04], [0.08, 0.07], 8);
            mesh.moving(motion::RECOIL, [0.0, 0.0, -0.02], |mesh| {
                mesh.beam([0.0, 0.0, -0.02], [0.0, 0.48, 0.16], 0.055, 0.055);
                for x in [-0.055_f32, 0.055] {
                    mesh.beam([x, 0.04, 0.04], [x, 0.22, 0.1], 0.025, 0.025);
                }
            });
            mesh.flash([0.0, 0.48, 0.16], 0.07);
        }
        Shape::MissileRack => {
            // A long, low box of missile tubes along the bed, in the team's colors, tilted up a
            // little to fire, with the tubes' caps at its front end.
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.1], [-0.27, -0.22], [0.08, 0.07], 6);
            mesh.paint(Material::Trim);
            let (back, front) = ([0.0, -0.4, -0.18], [0.0, 0.2, -0.08]);
            mesh.beam(back, front, 0.09, 0.07);
            let along = (Vec3::from(front) - Vec3::from(back)).normalize();
            let up = Vec3::X.cross(along).normalize();
            mesh.paint(Material::Panel);
            for x in [-0.05_f32, 0.0, 0.05] {
                for lift in [-0.035_f32, 0.035] {
                    let cap = Vec3::from(front) + Vec3::X * x + up * lift;
                    mesh.beam(cap.to_array(), (cap + along * 0.02).to_array(), 0.02, 0.025);
                }
            }
            mesh.paint(Material::Metal);
            for y in [-0.3_f32, -0.15, 0.0] {
                let at = Vec3::from(back).lerp(Vec3::from(front), (y + 0.4) / 0.6);
                mesh.block([-0.095, at.y - 0.015, at.z + 0.06], [0.095, at.y + 0.015, at.z + 0.075]);
            }
        }
        // A narrow tracked carriage with spades at the back, to brace against the recoil.
        Shape::ArtilleryHull => {
            mesh.paint(Material::Metal);
            for (low, high) in [(-0.21_f32, -0.13_f32), (0.13, 0.21)] {
                mesh.bevel([low, -0.3, -0.5], [high, 0.12, -0.3], 0.03);
            }
            mesh.paint(Material::Hull);
            for (low, high) in [(-0.227_f32, -0.12_f32), (0.12, 0.227)] {
                mesh.block([low, -0.32, -0.3], [high, 0.14, -0.27]);
            }
            mesh.bevel([-0.14, -0.44, -0.46], [0.14, 0.12, -0.22], 0.03);
            // A team-colored ammunition locker on the back deck.
            mesh.paint(Material::Trim);
            mesh.bevel([-0.11, -0.42, -0.22], [0.11, -0.28, -0.17], 0.02);
            mesh.paint(Material::Metal);
            for x in [-0.1_f32, 0.1] {
                mesh.beam([x, -0.4, -0.3], [x * 1.4, -0.465, -0.44], 0.03, 0.02);
            }
        }
        // A team-colored gun shield over a cradle, and a long barrel reaching far ahead, with
        // its recoil cylinder above it.
        Shape::Artillery => {
            mesh.bevel([-0.15, -0.14, -0.22], [0.15, 0.02, 0.04], 0.03);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.08, -0.36, -0.22], [0.08, -0.12, -0.06], 0.03);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.18], [-0.06, 0.02], [0.06, 0.055], 8);
            mesh.moving(motion::RECOIL, [0.0, -0.2, -0.06], |mesh| {
                mesh.beam([0.0, -0.2, -0.06], [0.0, 0.47, 0.14], 0.04, 0.04);
                mesh.beam([0.0, -0.1, 0.0], [0.0, 0.14, 0.07], 0.022, 0.022);
            });
            mesh.flash([0.0, 0.47, 0.14], 0.06);
            // A wide gun shield across the barrel, leaning back.
            mesh.paint(Material::Hull);
            mesh.cuboid(
                [Vec3::new(-0.25, 0.02, -0.2), Vec3::new(0.25, 0.02, -0.2), Vec3::new(0.25, 0.06, -0.2), Vec3::new(-0.25, 0.06, -0.2)],
                [Vec3::new(-0.22, -0.01, 0.06), Vec3::new(0.22, -0.01, 0.06), Vec3::new(0.22, 0.03, 0.06), Vec3::new(-0.22, 0.03, 0.06)],
            );
        }
        // An open-air assembly bay: a floor open at both ends, east and west, between long walls,
        // under a gantry crane, so that what it builds can be seen and drives out either end.
        Shape::Factory => {
            mesh.factory_floor();

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
        // A pump rig over the deposit: a team-colored pad, a glowing well head, and a walking
        // beam on an A-frame that nods as it pumps.
        Shape::Extractor => {
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.5, -0.38], [0.5, 0.44], 6);
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, 0.0], [-0.38, -0.35], [0.43, 0.41], 6);
            mesh.paint(Material::Glow);
            mesh.frustum([0.0, 0.24], [-0.35, -0.14], [0.08, 0.06], 6);
            mesh.paint(Material::Metal);
            for x in [-0.14_f32, 0.14] {
                for y in [-0.16_f32, 0.06] {
                    mesh.beam([x, y, -0.35], [x * 0.15, -0.05, 0.2], 0.018, 0.018);
                }
            }
            mesh.moving(motion::SWING, [0.01, -0.05, 0.22], |mesh| {
                mesh.paint(Material::Trim);
                mesh.block([-0.05, -0.38, 0.18], [0.05, 0.28, 0.26]);
                mesh.block([-0.06, 0.22, 0.02], [0.06, 0.32, 0.28]);
                mesh.paint(Material::Metal);
                mesh.block([-0.09, -0.44, 0.04], [0.09, -0.32, 0.2]);
            });
        }
        // A generator hall with a glowing window, and a cooling tower beside it, pinched at the
        // waist, with the team's band round its top.
        Shape::Generator => {
            mesh.paint(Material::Hull);
            mesh.bevel([-0.5, -0.5, -0.5], [0.5, 0.5, -0.42], 0.02);
            mesh.bevel([-0.46, -0.4, -0.42], [-0.06, 0.3, -0.02], 0.03);
            mesh.paint(Material::Trim);
            mesh.block([-0.47, -0.41, -0.02], [-0.05, 0.31, 0.03]);
            mesh.paint(Material::Glow);
            mesh.block([-0.4, 0.3, -0.3], [-0.12, 0.31, -0.2]);
            mesh.paint(Material::Metal);
            mesh.beam([-0.06, -0.05, -0.2], [0.04, -0.05, -0.2], 0.04, 0.04);
            mesh.paint(Material::Hull);
            mesh.frustum([0.24, 0.0], [-0.42, 0.22], [0.25, 0.16], 10);
            mesh.frustum([0.24, 0.0], [0.22, 0.46], [0.16, 0.2], 10);
            mesh.paint(Material::Trim);
            mesh.frustum([0.24, 0.0], [0.3, 0.4], [0.185, 0.205], 10);
            mesh.paint(Material::Glow);
            mesh.frustum([0.24, 0.0], [0.44, 0.47], [0.17, 0.17], 10);
        }
        Shape::EnergyStorage => {
            mesh.paint(Material::Hull);
            mesh.bevel([-0.5, -0.5, -0.5], [0.5, 0.5, -0.38], 0.03);
            for x in [-0.3_f32, 0.0, 0.3] {
                mesh.paint(Material::Trim);
                mesh.bevel([x - 0.11, -0.36, -0.38], [x + 0.11, 0.36, 0.26], 0.02);
                mesh.paint(Material::Panel);
                mesh.block([x - 0.115, -0.365, -0.02], [x + 0.115, 0.365, 0.08]);
                mesh.paint(Material::Glow);
                mesh.block([x - 0.07, -0.3, 0.26], [x + 0.07, 0.3, 0.31]);
            }
            mesh.paint(Material::Metal);
            mesh.block([-0.42, -0.04, 0.31], [0.42, 0.04, 0.35]);
        }
        Shape::Sonar => {
            let float = [[-0.2, -0.36], [0.2, -0.36], [0.4, 0.0], [0.2, 0.36], [-0.2, 0.36], [-0.4, 0.0]];
            mesh.paint(Material::Hull);
            mesh.prism(&float, [-0.5, -0.3], [0.85, 1.0]);
            mesh.paint(Material::Trim);
            mesh.prism(&float, [-0.3, -0.26], [0.98, 0.9]);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, 0.0], [-0.26, 0.32], [0.05, 0.035], 6);
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| {
                for z in [-0.02_f32, 0.16] {
                    mesh.frustum([0.0, 0.0], [z, z + 0.04], [0.17, 0.17], 10);
                }
            });
            mesh.paint(Material::Trim);
            mesh.ball([0.0, 0.0, 0.38], 0.09, 8, 4);
        }
        Shape::DeepScan => {
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.5, -0.36], [0.48, 0.42], 8);
            mesh.paint(Material::Metal);
            for index in 0..3 {
                let angle = index as f32 / 3.0 * std::f32::consts::TAU + 0.3;
                mesh.beam([angle.cos() * 0.34, angle.sin() * 0.34, -0.36], [angle.cos() * 0.12, angle.sin() * 0.12, -0.05], 0.025, 0.025);
            }
            mesh.paint(Material::Trim);
            mesh.ball([0.0, 0.0, 0.16], 0.3, 10, 6);
            mesh.paint(Material::Glow);
            mesh.frustum([0.0, 0.0], [0.12, 0.2], [0.305, 0.305], 14);
        }
        Shape::Apron => {
            mesh.paint(Material::Panel);
            mesh.block([-0.5, -0.5, -0.5], [0.5, 0.5, 0.1]);
            // The team's marks at the corners.
            mesh.paint(Material::Trim);
            for (x, y) in [(-1.0_f32, -1.0_f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let (corner_x, corner_y) = (0.47 * x, 0.47 * y);
                mesh.block([corner_x.min(corner_x - 0.16 * x), corner_y.min(corner_y - 0.04 * y), 0.1], [corner_x.max(corner_x - 0.16 * x), corner_y.max(corner_y - 0.04 * y), 0.5]);
                mesh.block([corner_x.min(corner_x - 0.04 * x), corner_y.min(corner_y - 0.16 * y), 0.1], [corner_x.max(corner_x - 0.04 * x), corner_y.max(corner_y - 0.16 * y), 0.5]);
            }
        }
        // A wide storage drum with a domed lid.
        Shape::Storage => {
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.5, 0.15], [0.46, 0.46], 10);
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, 0.0], [0.15, 0.5], [0.46, 0.14], 10);
        }
        // A stepped plinth with a team-colored ring, for turrets to stand on.
        Shape::PointDefenseBase => mesh.defense_ring(),
        Shape::PointDefenseBaseT2 => {
            mesh.defense_ring();
            for (x, y) in [(-0.37_f32, -0.37_f32), (0.37, -0.37), (0.37, 0.37), (-0.37, 0.37)] {
                mesh.paint(Material::Hull);
                mesh.bevel([x - 0.12, y - 0.12, -0.5], [x + 0.12, y + 0.12, -0.2], 0.04);
                mesh.paint(Material::Trim);
                mesh.block([x - 0.05, y - 0.05, -0.2], [x + 0.05, y + 0.05, -0.16]);
            }
        }
        Shape::HeavyDefenseBase => {
            let eight = |radius: f32| -> Vec<[f32; 2]> {
                (0..8).map(|side| { let angle = (side as f32 + 0.5) * std::f32::consts::FRAC_PI_4; [radius * angle.cos(), radius * angle.sin()] }).collect()
            };
            mesh.paint(Material::Hull);
            mesh.prism(&eight(0.5), [-0.5, -0.32], [1.0, 0.9]);
            mesh.paint(Material::Trim);
            mesh.prism(&eight(0.4), [-0.32, -0.25], [1.0, 0.95]);
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.25, -0.05], [0.3, 0.26], 8);
            // A buttress against each side.
            for (x, y) in [(0.0_f32, 0.38_f32), (0.38, 0.0), (0.0, -0.38), (-0.38, 0.0)] {
                let (half_x, half_y) = if x == 0.0 { (0.09, 0.1) } else { (0.1, 0.09) };
                mesh.paint(Material::Hull);
                mesh.bevel([x - half_x, y - half_y, -0.34], [x + half_x, y + half_y, -0.14], 0.04);
            }
        }
        Shape::AntiAirMount => {
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.5, -0.05], [0.18, 0.14], 8);
            for (x, y) in [(-1.0_f32, -1.0_f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                mesh.paint(Material::Metal);
                mesh.beam([x * 0.1, y * 0.1, -0.2], [x * 0.4, y * 0.4, -0.45], 0.05, 0.04);
                mesh.paint(Material::Trim);
                mesh.bevel([x * 0.41 - 0.08, y * 0.41 - 0.08, -0.5], [x * 0.41 + 0.08, y * 0.41 + 0.08, -0.42], 0.02);
            }
        }
        Shape::FlakPit => {
            // Four walls round a sunken floor.
            mesh.paint(Material::Hull);
            mesh.bevel([-0.49, 0.33, -0.5], [0.49, 0.49, -0.18], 0.04);
            mesh.bevel([-0.49, -0.49, -0.5], [0.49, -0.33, -0.18], 0.04);
            mesh.bevel([0.33, -0.33, -0.5], [0.49, 0.33, -0.18], 0.04);
            mesh.bevel([-0.49, -0.33, -0.5], [-0.33, 0.33, -0.18], 0.04);
            mesh.paint(Material::Panel);
            mesh.block([-0.33, -0.33, -0.5], [0.33, 0.33, -0.44]);
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, 0.0], [-0.44, -0.36], [0.27, 0.25], 8);
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.36, -0.05], [0.19, 0.16], 8);
        }
        Shape::SamOutriggers => {
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.5, -0.05], [0.24, 0.2], 8);
            for (x, y) in [(0.0_f32, 1.0_f32), (1.0, 0.0), (0.0, -1.0), (-1.0, 0.0)] {
                // An arm out to a pad, along x or along y.
                let (across, along) = (0.09, 0.33);
                let (arm_x, arm_y) = if x == 0.0 { (across, along * 0.5) } else { (along * 0.5, across) };
                let middle = [x * (0.16 + along * 0.5), y * (0.16 + along * 0.5)];
                mesh.paint(Material::Hull);
                mesh.block([middle[0] - arm_x, middle[1] - arm_y, -0.5], [middle[0] + arm_x, middle[1] + arm_y, -0.3]);
                mesh.paint(Material::Trim);
                mesh.bevel([x * 0.4 - 0.1 - x.abs() * 0.0, y * 0.4 - 0.1, -0.5], [x * 0.4 + 0.1, y * 0.4 + 0.1, -0.24], 0.03);
            }
        }
        Shape::ArtilleryPad => {
            mesh.paint(Material::Hull);
            mesh.bevel([-0.3, -0.3, -0.5], [0.3, 0.46, -0.36], 0.03);
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, 0.0], [-0.36, -0.3], [0.27, 0.25], 8);
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.3, -0.05], [0.2, 0.17], 8);
            for x in [-1.0_f32, 1.0] {
                // A trail back to a spade, and shells stacked beside the pad.
                mesh.paint(Material::Metal);
                mesh.beam([x * 0.14, -0.2, -0.34], [x * 0.4, -0.43, -0.45], 0.045, 0.04);
                mesh.paint(Material::Trim);
                mesh.bevel([x * 0.4 - 0.09, -0.5, -0.5], [x * 0.4 + 0.09, -0.4, -0.36], 0.02);
                mesh.paint(Material::Hull);
                mesh.bevel([x * 0.41 - 0.08, 0.06, -0.5], [x * 0.41 + 0.08, 0.4, -0.36], 0.02);
                mesh.paint(Material::Metal);
                mesh.block([x * 0.41 - 0.05, 0.1, -0.36], [x * 0.41 + 0.05, 0.36, -0.33]);
            }
        }
        // A team-colored turret with a sloped front, a long gun, and a sensor with a glowing eye.
        // A fast buggy, twice as long as it is wide, whose team color is the radar dish turning
        // at its back and a stripe down its nose.
        Shape::LandScout => {
            mesh.paint(Material::Metal);
            for x in [-0.215_f32, 0.215] {
                for y in [-0.3_f32, 0.3] {
                    mesh.bevel([x - 0.05, y - 0.09, -0.5], [x + 0.05, y + 0.09, -0.3], 0.03);
                }
            }
            mesh.paint(Material::Hull);
            mesh.bevel([-0.17, -0.46, -0.42], [0.17, 0.46, -0.26], 0.04);
            mesh.bevel([-0.13, -0.1, -0.26], [0.13, 0.16, -0.12], 0.03);
            mesh.paint(Material::Glass);
            mesh.block([-0.11, 0.16, -0.22], [0.11, 0.175, -0.15]);
            mesh.paint(Material::Trim);
            mesh.block([-0.07, 0.17, -0.26], [0.07, 0.44, -0.25]);
            mesh.paint(Material::Metal);
            mesh.body(0.0, &[[0.18, 0.02, 0.02, -0.08], [0.46, 0.016, 0.016, -0.08]], 6);
            mesh.flash([0.0, 0.45, -0.08], 0.035);
            mesh.frustum([0.0, -0.3], [-0.26, 0.2], [0.025, 0.02], 6);
            mesh.paint(Material::Trim);
            mesh.moving(motion::SPIN, [0.0, -0.3, 0.0], |mesh| {
                mesh.cuboid(
                    [Vec3::new(-0.2, -0.32, 0.16), Vec3::new(0.2, -0.32, 0.16), Vec3::new(0.2, -0.29, 0.16), Vec3::new(-0.2, -0.29, 0.16)],
                    [Vec3::new(-0.2, -0.28, 0.32), Vec3::new(0.2, -0.28, 0.32), Vec3::new(0.2, -0.25, 0.32), Vec3::new(-0.2, -0.25, 0.32)],
                );
            });
        }
        Shape::GrenadierWalker => {
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
        // A big glowing dome on a low mount, ringed in the team's colors, over most of a carrier.
        Shape::ShieldEmitter => {
            mesh.paint(Material::Hull);
            mesh.bevel([-0.2, -0.32, -0.22], [0.2, 0.16, -0.12], 0.03);
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, -0.08], [-0.12, -0.07], [0.31, 0.3], 16);
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| {
                mesh.frustum([0.0, -0.08], [-0.07, 0.02], [0.28, 0.25], 16);
                mesh.frustum([0.0, -0.08], [0.02, 0.1], [0.25, 0.17], 16);
                mesh.frustum([0.0, -0.08], [0.1, 0.14], [0.17, 0.0], 16);
            });
        }
        Shape::HeavyWalker => {
            mesh.prism(&[[-0.3, -0.18], [0.3, -0.18], [0.34, 0.04], [0.22, 0.22], [-0.22, 0.22], [-0.34, 0.04]], [-0.06, 0.3], [0.8, 1.0]);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.28, -0.18, 0.3], [0.28, 0.02, 0.34], 0.02);
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
        Shape::SiegeWalker => {
            mesh.bevel([-0.3, -0.26, -0.04], [0.3, 0.2, 0.3], 0.06);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.26, -0.24, 0.3], [0.26, 0.02, 0.34], 0.02);
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
            // A wedge, wide and square at the stern: a fast planing boat, the only one so short
            // and so wide.
            mesh.hull(&[[0.208, -0.5], [0.208, -0.167], [0.015, 0.5]], 0.05);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.1, -0.375, -0.2], [0.1, -0.146, -0.06], 0.03);
            mesh.paint(Material::Metal);
            for x in [-0.17_f32, 0.17] {
                mesh.body(x, &[[-0.46, 0.03, 0.03, -0.19], [-0.2, 0.03, 0.03, -0.19]], 6);
            }
            mesh.frustum([0.0, -0.26], [-0.06, 0.2], [0.015, 0.01], 6);
            // A team-colored chevron on the foredeck, pointing ahead.
            mesh.paint(Material::Trim);
            mesh.prism(&[[-0.142, -0.042], [0.0, 0.092], [0.0, 0.375]], [-0.2, -0.15], [1.0, 1.0]);
            mesh.prism(&[[0.0, 0.092], [0.142, -0.042], [0.0, 0.375]], [-0.2, -0.15], [1.0, 1.0]);
            mesh.paint(Material::Glow);
            mesh.ball([0.0, -0.26, 0.21], 0.025, 6, 3);
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
        // A tiltrotor: a team-colored cargo body with a glazed cockpit and a ramp at the back,
        // stub wings, and a ducted rotor at each wingtip, its blades turning.
        Shape::Transport => {
            mesh.body(0.0, &[[-0.46, 0.07, 0.06, 0.02], [-0.3, 0.12, 0.11, 0.02], [0.22, 0.13, 0.12, 0.02], [0.38, 0.1, 0.09, 0.0], [0.48, 0.04, 0.04, -0.02]], 8);
            mesh.paint(Material::Glass);
            mesh.body(0.0, &[[0.3, 0.05, 0.03, 0.1], [0.4, 0.07, 0.045, 0.08], [0.46, 0.025, 0.02, 0.04]], 6);
            mesh.paint(Material::Panel);
            mesh.block([-0.08, -0.47, -0.07], [0.08, -0.42, 0.06]);
            mesh.paint(Material::Hull);
            mesh.wings(0.1, 0.24, [-0.08, 0.12], [-0.06, 0.08], [0.022, 0.012], 0.05);
            mesh.fin(0.0, [-0.46, -0.32], 0.1, [-0.46, -0.38], 0.28, 0.015);
            for x in [-0.34_f32, 0.34] {
                mesh.paint(Material::Metal);
                mesh.frustum([x, 0.0], [0.0, 0.1], [0.15, 0.15], 12);
                mesh.paint(Material::Panel);
                mesh.frustum([x, 0.0], [0.1, 0.104], [0.125, 0.125], 12);
                mesh.paint(Material::Trim);
                mesh.moving(motion::SPIN, [x, 0.0, 0.0], |mesh| {
                    mesh.block([x - 0.12, -0.018, 0.104], [x + 0.12, 0.018, 0.118]);
                    mesh.block([x - 0.018, -0.12, 0.104], [x + 0.018, 0.12, 0.118]);
                });
            }
            mesh.paint(Material::Glow);
            mesh.block([-0.05, 0.43, 0.04], [0.05, 0.45, 0.07]);
        }
        Shape::FieldWorkshop => {
            mesh.workshop_pad();
            mesh.workshop_mast([-0.18, -0.18], [0.23, 0.19], 0.34);
            mesh.workshop_lights();
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
        // A tracer: a glowing slug along x, the way it flies, brightest at its nose.
        Shape::Shell => {
            mesh.paint(Material::Glow);
            mesh.beam([-0.5, 0.0, 0.0], [0.3, 0.0, 0.0], 0.32, 0.32);
            mesh.beam([0.3, 0.0, 0.0], [0.5, 0.0, 0.0], 0.5, 0.5);
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
            // A collar in the team's colors round the column's foot, where it shows from above:
            // the band up the column flares out, so only its underside faces the camera.
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, 0.0], [-0.3, -0.24], [0.34, 0.3], 8);
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
        Shape::StrategicInterceptorSilo => {
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.5, 0.15], [0.5, 0.48], 10);
            for (x, y) in [(-0.22_f32, -0.22_f32), (0.22, -0.22), (-0.22, 0.22), (0.22, 0.22)] {
                mesh.paint(Material::Trim);
                mesh.frustum([x, y], [0.15, 0.5], [0.12, 0.12], 6);
            }
        }
        // A lattice mast over an equipment hut, with a turning antenna on top.
        Shape::Radar => {
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.5, -0.4], [0.42, 0.36], 6);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.3, -0.32, -0.4], [-0.02, -0.06, -0.2], 0.02);
            mesh.paint(Material::Metal);
            for (x, y) in [(-1.0_f32, -1.0_f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                mesh.beam([x * 0.14, y * 0.14, -0.4], [x * 0.04, y * 0.04, 0.24], 0.014, 0.014);
            }
            for z in [-0.2_f32, 0.02] {
                let half = 0.14 - (z + 0.4) / 0.64 * 0.1;
                mesh.block([-half, -half, z], [half, half, z + 0.02]);
            }
            mesh.paint(Material::Trim);
            mesh.moving(motion::SPIN, [0.0; 3], |mesh| {
                mesh.cuboid(
                    [Vec3::new(-0.42, -0.06, 0.24), Vec3::new(0.42, -0.06, 0.24), Vec3::new(0.42, 0.04, 0.24), Vec3::new(-0.42, 0.04, 0.24)],
                    [Vec3::new(-0.42, 0.06, 0.5), Vec3::new(0.42, 0.06, 0.5), Vec3::new(0.42, 0.16, 0.5), Vec3::new(-0.42, 0.16, 0.5)],
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
        Shape::GeothermalPlant => {
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
            // A team-colored cap over the core.
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, 0.0], [0.38, 0.47], [0.3, 0.22], 12);
        }
        // A fighter: a team-colored body tapering to a point, delta wings, and a swept fin.
        Shape::Aircraft => {
            mesh.body(0.0, &[[-0.46, 0.065, 0.05, 0.0], [-0.2, 0.09, 0.075, 0.0], [0.12, 0.085, 0.07, 0.0], [0.34, 0.05, 0.045, -0.005], [0.5, 0.006, 0.006, -0.01]], 6);
            mesh.paint(Material::Glow);
            mesh.body(0.0, &[[-0.5, 0.05, 0.04, 0.0], [-0.46, 0.062, 0.048, 0.0]], 6);
            mesh.paint(Material::Hull);
            mesh.main_wing(shape, None);
            mesh.wings(0.05, 0.2, [-0.46, -0.32], [-0.47, -0.42], [0.01, 0.004], 0.0);
            mesh.paint(Material::Trim);
            mesh.fin(0.0, [-0.44, -0.22], 0.05, [-0.47, -0.37], 0.28, 0.012);
            mesh.paint(Material::Glass);
            mesh.body(0.0, &[[0.08, 0.02, 0.012, 0.065], [0.16, 0.042, 0.03, 0.07], [0.27, 0.03, 0.02, 0.058], [0.32, 0.006, 0.005, 0.05]], 6);
        }
        Shape::Scout => {
            mesh.paint(Material::Hull);
            mesh.body(0.0, &[[-0.5, 0.02, 0.02, 0.03], [-0.25, 0.045, 0.045, 0.01], [0.15, 0.06, 0.055, 0.0], [0.38, 0.045, 0.04, -0.01], [0.5, 0.008, 0.008, -0.02]], 6);
            // Long, straight, narrow wings, set high, for loitering over what it watches.
            mesh.wings(0.04, 0.5, [-0.04, 0.08], [-0.02, 0.05], [0.012, 0.005], 0.04);
            mesh.paint(Material::Trim);
            mesh.fin(0.0, [-0.5, -0.38], 0.02, [-0.5, -0.44], 0.13, 0.01);
            mesh.paint(Material::Hull);
            mesh.wings(0.01, 0.17, [-0.5, -0.43], [-0.5, -0.46], [0.008, 0.004], 0.13);
            mesh.paint(Material::Glow);
            mesh.body(0.0, &[[0.18, 0.025, 0.02, 0.045], [0.3, 0.02, 0.015, 0.035]], 6);
            // The radar disc on its pylon, in team colors.
            mesh.paint(Material::Metal);
            mesh.beam([0.0, -0.16, 0.04], [0.0, -0.16, 0.13], 0.012, 0.035);
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, -0.16], [0.13, 0.17], [0.17, 0.17], 14);
        }
        Shape::FighterBomber => {
            mesh.paint(Material::Hull);
            mesh.body(0.0, &[[-0.44, 0.07, 0.05, 0.0], [-0.1, 0.09, 0.07, 0.0], [0.2, 0.07, 0.06, 0.0], [0.4, 0.035, 0.035, -0.01], [0.5, 0.006, 0.006, -0.015]], 6);
            // Half a fighter and half a bomber: wings swept back half as far as a fighter's,
            // broad at the root and tapering to the tips, and a swept tailplane.
            mesh.main_wing(shape, Some((Material::Trim, 0.4)));
            mesh.wings(0.06, 0.22, [-0.5, -0.36], [-0.5, -0.45], [0.01, 0.004], 0.0);
            mesh.paint(Material::Trim);
            for x in [-0.075_f32, 0.075] {
                mesh.fin(x, [-0.48, -0.3], 0.04, [-0.5, -0.42], 0.22, 0.01);
            }
            mesh.paint(Material::Glow);
            for x in [-0.035_f32, 0.035] {
                mesh.body(x, &[[-0.5, 0.028, 0.028, 0.0], [-0.44, 0.034, 0.034, 0.0]], 6);
            }
            mesh.paint(Material::Glass);
            mesh.body(0.0, &[[0.14, 0.03, 0.02, 0.06], [0.24, 0.045, 0.035, 0.06], [0.34, 0.025, 0.02, 0.04]], 6);
            mesh.paint(Material::Metal);
            // Bombs and napalm canisters under the wings.
            for x in [-0.3_f32, -0.18, 0.18, 0.3] {
                mesh.body(x, &[[-0.24, 0.008, 0.008, -0.055], [-0.18, 0.028, 0.028, -0.055], [-0.02, 0.028, 0.028, -0.055], [0.04, 0.01, 0.01, -0.055]], 6);
            }
        }
        // Twin booms: a short central pod with the torpedo slung under it, straight wings with
        // an engine at the front of each boom, and a team-colored tailplane joining the booms.
        // From above the only aircraft with a hole in its outline.
        Shape::TorpedoBomber => {
            mesh.paint(Material::Hull);
            mesh.body(0.0, &[[-0.14, 0.03, 0.03, 0.02], [0.0, 0.075, 0.07, 0.02], [0.3, 0.075, 0.07, 0.01], [0.44, 0.04, 0.035, 0.0], [0.5, 0.012, 0.012, -0.005]], 6);
            for x in [-0.17_f32, 0.17] {
                mesh.paint(Material::Hull);
                mesh.body(x, &[[-0.5, 0.02, 0.022, 0.03], [-0.1, 0.035, 0.035, 0.03], [0.16, 0.05, 0.05, 0.03], [0.24, 0.03, 0.03, 0.03]], 6);
                mesh.paint(Material::Glow);
                mesh.body(x, &[[0.24, 0.03, 0.03, 0.03], [0.27, 0.02, 0.02, 0.03]], 6);
                mesh.paint(Material::Trim);
                mesh.fin(x, [-0.5, -0.4], 0.04, [-0.5, -0.44], 0.17, 0.01);
            }
            mesh.paint(Material::Hull);
            mesh.main_wing(shape, None);
            mesh.paint(Material::Trim);
            mesh.wing_tips(0.05, 0.5, [-0.06, 0.12], [-0.03, 0.07], [0.025, 0.008], 0.03, 0.3);
            mesh.block([-0.19, -0.5, 0.025], [0.19, -0.42, 0.04]);
            mesh.paint(Material::Glass);
            mesh.body(0.0, &[[0.3, 0.045, 0.03, 0.07], [0.4, 0.05, 0.035, 0.06], [0.46, 0.02, 0.016, 0.03]], 6);
            mesh.paint(Material::Metal);
            // The torpedo, long, slung under the pod and reaching back between the booms, with
            // a team-colored warhead and fins.
            mesh.body(0.0, &[[-0.42, 0.006, 0.006, -0.1], [-0.36, 0.038, 0.038, -0.1], [0.16, 0.038, 0.038, -0.1], [0.2, 0.034, 0.034, -0.1]], 6);
            mesh.paint(Material::Trim);
            mesh.body(0.0, &[[0.2, 0.034, 0.034, -0.1], [0.28, 0.008, 0.008, -0.1]], 6);
            mesh.wings(0.0, 0.065, [-0.42, -0.36], [-0.42, -0.39], [0.004, 0.003], -0.1);
            mesh.paint(Material::Metal);
            mesh.beam([0.0, -0.06, -0.06], [0.0, -0.06, -0.02], 0.01, 0.03);
            mesh.beam([0.0, 0.12, -0.06], [0.0, 0.12, -0.02], 0.01, 0.03);
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
        // A canard delta: a long team-colored body with a long nose, the delta wing set far
        // back, small canard wings near the cockpit, and two engines. From above an arrow,
        // where the interceptor is a triangle.
        Shape::AirSuperiority => {
            mesh.body(0.0, &[[-0.44, 0.05, 0.045, 0.02], [-0.18, 0.07, 0.065, 0.02], [0.1, 0.06, 0.055, 0.0], [0.36, 0.035, 0.035, -0.01], [0.5, 0.005, 0.005, -0.015]], 6);
            for x in [-0.065_f32, 0.065] {
                mesh.paint(Material::Hull);
                mesh.body(x, &[[-0.46, 0.045, 0.04, 0.0], [-0.16, 0.05, 0.045, 0.0], [0.02, 0.03, 0.03, -0.01]], 6);
                mesh.paint(Material::Glow);
                mesh.body(x, &[[-0.5, 0.036, 0.032, 0.0], [-0.46, 0.045, 0.04, 0.0]], 6);
            }
            mesh.paint(Material::Hull);
            mesh.main_wing(shape, Some((Material::Trim, 0.3)));
            mesh.wings(0.04, 0.15, [0.17, 0.3], [0.19, 0.24], [0.01, 0.004], 0.01);
            mesh.paint(Material::Trim);
            for x in [-0.07_f32, 0.07] {
                mesh.fin(x, [-0.46, -0.3], 0.04, [-0.48, -0.4], 0.2, 0.01);
            }
            mesh.paint(Material::Glass);
            mesh.body(0.0, &[[0.2, 0.018, 0.012, 0.05], [0.28, 0.035, 0.026, 0.055], [0.38, 0.022, 0.016, 0.04], [0.42, 0.005, 0.004, 0.03]], 6);
        }
        // A bomber: a thick team-colored body, straight wings with engines under them, and a glazed nose.
        Shape::Bomber => {
            mesh.body(0.0, &[[-0.5, 0.03, 0.03, 0.04], [-0.3, 0.09, 0.09, 0.01], [0.1, 0.14, 0.13, 0.0], [0.35, 0.11, 0.1, -0.01], [0.48, 0.04, 0.04, -0.02]], 6);
            mesh.paint(Material::Hull);
            mesh.main_wing(shape, None);
            mesh.wings(0.02, 0.24, [-0.5, -0.38], [-0.48, -0.42], [0.012, 0.004], 0.04);
            mesh.paint(Material::Trim);
            mesh.fin(0.0, [-0.48, -0.3], 0.07, [-0.5, -0.42], 0.3, 0.014);
            for x in [-0.3_f32, 0.3] {
                mesh.paint(Material::Metal);
                mesh.body(x, &[[-0.12, 0.045, 0.04, -0.03], [0.1, 0.055, 0.05, -0.03], [0.27, 0.035, 0.035, -0.03]], 6);
                mesh.paint(Material::Glow);
                mesh.body(x, &[[-0.17, 0.036, 0.03, -0.03], [-0.12, 0.045, 0.04, -0.03]], 6);
            }
            mesh.paint(Material::Glass);
            mesh.body(0.0, &[[0.28, 0.05, 0.025, 0.09], [0.37, 0.065, 0.04, 0.08], [0.45, 0.03, 0.02, 0.04]], 6);
            mesh.paint(Material::Metal);
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
            mesh.paint(Material::Glass);
            mesh.body(0.0, &[[0.26, 0.06, 0.03, -0.03], [0.36, 0.085, 0.05, -0.07], [0.46, 0.035, 0.025, -0.13]], 6);
            mesh.paint(Material::Metal);
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
            mesh.paint(Material::Glass);
            mesh.body(0.0, &[[0.28, 0.06, 0.03, -0.06], [0.37, 0.08, 0.045, -0.09], [0.46, 0.035, 0.025, -0.15]], 6);
            mesh.paint(Material::Metal);
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
        // The largest aircraft with wings: a long body, a long, straight team-colored wing
        // with four engines slung under it, and a tailplane with twin fins. Where the bomber's
        // body is team-colored and its wing armor, this is the other way round.
        Shape::StrategicBomber => {
            mesh.main_wing(shape, None);
            mesh.paint(Material::Hull);
            mesh.body(0.0, &[[-0.5, 0.03, 0.03, 0.04], [-0.3, 0.065, 0.065, 0.02], [0.0, 0.08, 0.08, 0.0], [0.3, 0.07, 0.065, -0.01], [0.45, 0.04, 0.035, -0.02], [0.5, 0.01, 0.01, -0.025]], 6);
            mesh.wings(0.03, 0.21, [-0.5, -0.37], [-0.49, -0.43], [0.012, 0.004], 0.04);
            for x in [-0.19_f32, 0.19] {
                mesh.fin(x, [-0.5, -0.4], 0.045, [-0.5, -0.45], 0.2, 0.01);
            }
            for x in [-0.34_f32, -0.18, 0.18, 0.34] {
                mesh.paint(Material::Metal);
                mesh.body(x, &[[-0.08, 0.034, 0.03, -0.035], [0.1, 0.04, 0.036, -0.035], [0.22, 0.028, 0.026, -0.035]], 6);
                mesh.paint(Material::Glow);
                mesh.body(x, &[[-0.12, 0.026, 0.024, -0.035], [-0.08, 0.034, 0.03, -0.035]], 6);
            }
            mesh.paint(Material::Glass);
            mesh.body(0.0, &[[0.32, 0.03, 0.015, 0.05], [0.39, 0.04, 0.025, 0.045], [0.45, 0.02, 0.01, 0.025]], 6);
            mesh.paint(Material::Metal);
        }
        // A round gun house with one fat barrel, and a rangefinder's arm out either side.
        Shape::FlakGun => {
            mesh.frustum([0.0, -0.04], [-0.1, 0.18], [0.25, 0.2], 8);
            mesh.paint(Material::Hull);
            mesh.block([-0.38, -0.12, 0.02], [0.38, -0.04, 0.1]);
            mesh.paint(Material::Metal);
            for x in [-0.38_f32, 0.38] {
                mesh.block([x - 0.035, -0.15, -0.01], [x + 0.035, -0.01, 0.13]);
            }
            mesh.moving(motion::RECOIL, [0.0, -0.02, 0.1], |mesh| mesh.beam([0.0, -0.02, 0.1], [0.0, 0.3, 0.38], 0.095, 0.095));
            mesh.flash([0.0, 0.32, 0.4], 0.08);
        }
        // A turntable carrying a raised rack of four missiles, side by side.
        Shape::SamLauncher => {
            mesh.bevel([-0.2, -0.26, -0.1], [0.2, 0.08, 0.1], 0.03);
            mesh.cuboid(
                [Vec3::new(-0.36, -0.24, 0.04), Vec3::new(0.36, -0.24, 0.04), Vec3::new(0.36, -0.16, 0.0), Vec3::new(-0.36, -0.16, 0.0)],
                [Vec3::new(-0.36, 0.2, 0.36), Vec3::new(0.36, 0.2, 0.36), Vec3::new(0.36, 0.28, 0.32), Vec3::new(-0.36, 0.28, 0.32)],
            );
            for x in [-0.27_f32, -0.09, 0.09, 0.27] {
                mesh.paint(Material::Hull);
                mesh.body(x, &[[-0.26, 0.05, 0.05, 0.11], [0.2, 0.05, 0.05, 0.41]], 6);
                mesh.paint(Material::Glow);
                mesh.body(x, &[[0.2, 0.05, 0.05, 0.41], [0.3, 0.0, 0.0, 0.475]], 6);
            }
        }
        // A long barrel laid nearly flat, half the plot's width out from a housing at its back to
        // its edge, with a muzzle brake, and an ammunition box behind.
        Shape::ArtilleryGun => {
            mesh.block([-0.26, -0.42, -0.15], [0.26, 0.0, 0.14]);
            mesh.paint(Material::Metal);
            mesh.moving(motion::RECOIL, [0.0, -0.2, 0.04], |mesh| {
                mesh.beam([0.0, -0.2, 0.04], [0.0, 0.47, 0.16], 0.055, 0.055);
                mesh.beam([0.0, 0.42, 0.151], [0.0, 0.47, 0.16], 0.08, 0.065);
            });
            mesh.flash([0.0, 0.48, 0.17], 0.06);
            mesh.paint(Material::Hull);
            mesh.block([-0.12, -0.5, 0.0], [0.12, -0.42, 0.11]);
        }
        // A dock: two piers with a slip of open water between them, open at both ends, under a crane.
        Shape::NavalFactory => {
            mesh.piers();

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
            // Short, with a square stern for its helipad.
            mesh.hull(&side(&[[0.118, -0.5]], [0.118, 0.18], [0.093, 0.41], 4), 0.06);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.07, -0.22, -0.2], [0.07, 0.03, -0.02], 0.03);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.05, -0.06, -0.02], [0.05, 0.03, 0.06], 0.02);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.1], [-0.02, 0.36], [0.016, 0.01], 6);
            mesh.block([-0.06, -0.12, 0.26], [0.06, -0.1, 0.28]);
            // A team-colored helipad on the stern.
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, -0.345], [-0.2, -0.19], [0.09, 0.09], 12);
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, -0.345], [-0.19, -0.186], [0.05, 0.05], 12);
            mesh.turret([0.0, 0.2], [-0.16, -0.08], 0.055, 1, 0.1, false);
        }
        Shape::Destroyer => {
            // Long and lean, with a long raked bow.
            mesh.hull(&side(&[[0.076, -0.5]], [0.076, 0.087], [0.065, 0.348], 5), 0.1);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.055, -0.15, -0.2], [0.055, 0.17, -0.03], 0.025);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.045, 0.06, -0.03], [0.045, 0.16, 0.07], 0.02);
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, -0.02], [-0.03, 0.08], [0.03, 0.026], 8);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, 0.04], [0.07, 0.42], [0.015, 0.009], 6);
            mesh.block([-0.06, 0.03, 0.32], [0.06, 0.05, 0.34]);
            // A big raked funnel behind the tower: a fast ship, low and lean.
            mesh.paint(Material::Hull);
            mesh.beam([0.0, -0.1, -0.06], [0.0, -0.17, 0.16], 0.038, 0.05);
            mesh.paint(Material::Metal);
            mesh.beam([0.0, -0.165, 0.15], [0.0, -0.172, 0.17], 0.033, 0.044);
            // Torpedo tubes either side, behind the tower.
            for x in [-0.058_f32, 0.058] {
                mesh.beam([x, -0.3, -0.17], [x, -0.2, -0.17], 0.013, 0.013);
            }
            mesh.turret([0.0, 0.28], [-0.16, -0.08], 0.046, 1, 0.08, false);
            mesh.turret([0.0, -0.39], [-0.2, -0.12], 0.046, 1, 0.05, true);
        }
        Shape::Cruiser => {
            // Widest amidships, where sponsons carry its radar domes out past its sides.
            mesh.hull(&side(&[[0.105, -0.5], [0.125, -0.2]], [0.125, 0.2], [0.09, 0.4], 4), 0.08);
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
            // A tall, square block over the bridge, its radar faces in team colors.
            mesh.paint(Material::Hull);
            mesh.bevel([-0.05, -0.04, 0.12], [0.05, 0.04, 0.24], 0.015);
            mesh.paint(Material::Trim);
            mesh.block([-0.052, 0.035, 0.15], [0.052, 0.042, 0.22]);
            // The sponsons, just under the deck so that it covers them inside the hull, and a big
            // team-colored radar dome on each: the fleet's anti-air at a glance, and from above a
            // cross.
            mesh.paint(Material::Hull);
            mesh.block([-0.2, -0.06, -0.26], [0.2, 0.06, -0.21]);
            for x in [-0.175_f32, 0.175] {
                mesh.paint(Material::Metal);
                mesh.frustum([x, 0.0], [-0.21, -0.1], [0.03, 0.025], 6);
                mesh.paint(Material::Trim);
                mesh.ball([x, 0.0, -0.04], 0.08, 10, 6);
            }
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
            // Broad, with bulged sides and a blunt, round bow.
            mesh.hull(&side(&[[0.136, -0.5], [0.163, -0.39]], [0.163, 0.24], [0.152, 0.457], 5), 0.06);
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
            for x in [-0.14_f32, 0.14] {
                for y in [-0.16_f32, -0.04] {
                    mesh.block([x - 0.018, y - 0.022, -0.2], [x + 0.018, y + 0.022, -0.16]);
                }
            }
            // Its three turrets are `Shape::NavalTurret`s of their own, which turn to aim. The
            // middle one stands on a barbette, to fire over the front one.
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, 0.109], [-0.16, -0.11], [0.085, 0.085], 8);
        }
        Shape::Battlecruiser => {
            // Long and narrow, its guns all forward.
            mesh.hull(&side(&[[0.077, -0.5]], [0.087, 0.14], [0.07, 0.384], 5), 0.0);
            // A tower raked back: low at the stern, high at the bow.
            mesh.paint(Material::Hull);
            mesh.cuboid(
                [Vec3::new(-0.065, -0.2, -0.2), Vec3::new(0.065, -0.2, -0.2), Vec3::new(0.065, 0.06, -0.2), Vec3::new(-0.065, 0.06, -0.2)],
                [Vec3::new(-0.05, -0.16, -0.08), Vec3::new(0.05, -0.16, -0.08), Vec3::new(0.05, 0.03, 0.04), Vec3::new(-0.05, 0.03, 0.04)],
            );
            mesh.paint(Material::Trim);
            mesh.cuboid(
                [Vec3::new(-0.04, -0.04, -0.03), Vec3::new(0.04, -0.04, -0.03), Vec3::new(0.04, 0.02, 0.03), Vec3::new(-0.04, 0.02, 0.03)],
                [Vec3::new(-0.033, -0.03, 0.05), Vec3::new(0.033, -0.03, 0.05), Vec3::new(0.033, 0.01, 0.11), Vec3::new(-0.033, 0.01, 0.11)],
            );
            // A raked funnel and a mast.
            mesh.paint(Material::Hull);
            mesh.beam([0.0, -0.17, -0.12], [0.0, -0.23, 0.06], 0.025, 0.03);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.02], [0.08, 0.38], [0.014, 0.008], 6);
            // A long quarterdeck aft, bare but for its missile cells.
            mesh.paint(Material::Trim);
            mesh.block([-0.055, -0.465, -0.2], [0.055, -0.33, -0.17]);
            mesh.paint(Material::Metal);
            for x in [-0.028_f32, 0.0, 0.028] {
                mesh.block([x - 0.01, -0.45, -0.17], [x + 0.01, -0.345, -0.165]);
            }
            mesh.turret([0.0, 0.3], [-0.16, -0.08], 0.06, 2, 0.06, false);
            mesh.turret([0.0, 0.17], [-0.14, -0.05], 0.06, 2, 0.06, false);
        }
        Shape::Submarine => {
            let outline = [[-0.03, -0.5], [0.03, -0.5], [0.08, -0.38], [0.09, 0.24], [0.045, 0.44], [-0.045, 0.44], [-0.09, 0.24], [-0.08, -0.38]];
            mesh.paint(Material::Hull);
            mesh.prism(&outline, [-0.5, -0.32], [0.55, 1.0]);
            mesh.prism(&outline, [-0.32, -0.25], [1.0, 0.7]);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.035, 0.02, -0.27], [0.035, 0.2, -0.08], 0.02);
            // The team's stripe down the deck.
            mesh.block([-0.03, -0.36, -0.255], [0.03, 0.36, -0.245]);
            mesh.paint(Material::Hull);
            mesh.block([-0.1, 0.1, -0.13], [0.1, 0.14, -0.115]);
            mesh.block([-0.09, -0.46, -0.38], [0.09, -0.41, -0.365]);
            mesh.block([-0.008, -0.47, -0.33], [0.008, -0.39, -0.21]);
            // The deck gun, forward of the sail.
            mesh.turret([0.0, 0.29], [-0.25, -0.2], 0.035, 1, 0.07, false);
        }
        Shape::MissileSubmarine => {
            let outline = [[-0.03, -0.5], [0.03, -0.5], [0.076, -0.38], [0.084, 0.28], [0.042, 0.46], [-0.042, 0.46], [-0.084, 0.28], [-0.076, -0.38]];
            mesh.paint(Material::Hull);
            mesh.prism(&outline, [-0.5, -0.3], [0.55, 1.0]);
            mesh.prism(&outline, [-0.3, -0.23], [1.0, 0.75]);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.036, 0.14, -0.25], [0.036, 0.34, -0.04], 0.02);
            // The team's stripe down the deck, either side of the hatches.
            for x in [-0.064_f32, 0.064] {
                mesh.block([x - 0.01, -0.36, -0.235], [x + 0.01, 0.4, -0.225]);
            }
            mesh.paint(Material::Hull);
            mesh.block([-0.095, 0.22, -0.09], [0.095, 0.26, -0.075]);
            mesh.block([-0.008, -0.47, -0.3], [0.008, -0.38, -0.17]);
            // Two long rows of missile hatches behind the tower.
            mesh.paint(Material::Glow);
            for y in [-0.33_f32, -0.23, -0.13, -0.03, 0.07] {
                for x in [-0.03_f32, 0.03] {
                    mesh.block([x - 0.018, y - 0.03, -0.24], [x + 0.018, y + 0.03, -0.228]);
                }
            }
        }
        Shape::EscortCorvette => {
            // A trimaran: a slim middle hull, and an outrigger either side under a team-colored
            // cross deck. The only ship with more than one hull.
            mesh.hull(&side(&[[0.087, -0.5]], [0.087, 0.133], [0.06, 0.383], 4), 0.08);
            mesh.paint(Material::Hull);
            for sign in [-1.0_f32, 1.0] {
                let outrigger = [[0.173, -0.4], [0.253, -0.4], [0.253, 0.18], [0.213, 0.233], [0.173, 0.18]].map(|[x, y]| [x * sign, y]);
                mesh.prism(&outrigger, [-0.4, -0.24], [0.8, 1.0]);
            }
            // Just under the deck, so that the deck covers it inside the middle hull.
            mesh.paint(Material::Trim);
            mesh.block([-0.24, -0.3, -0.25], [0.24, 0.033, -0.205]);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.065, -0.25, -0.2], [0.065, 0.12, 0.0], 0.03);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.05, 0.03, 0.0], [0.05, 0.11, 0.08], 0.02);
            // The shield emitter on a tall mast, over the cross deck.
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.133], [0.0, 0.28], [0.03, 0.022], 8);
            mesh.paint(Material::Glow);
            mesh.frustum([0.0, -0.133], [0.28, 0.42], [0.13, 0.13], 12);
            mesh.frustum([0.0, -0.133], [0.42, 0.5], [0.1, 0.04], 12);
        }
        // The T2 bay: higher buttressed walls with windows, two cranes, and two stacks.
        Shape::FactoryT2 => {
            mesh.factory_floor();

            mesh.bay_walls_t2();
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
            mesh.factory_floor();

            mesh.bay_walls_t3();
            // A heavy double gantry with an operator's cab, spanning the bay.
            mesh.paint(Material::Metal);
            for x in [-0.12_f32, 0.12] {
                for y in [-0.47_f32, 0.38] {
                    mesh.block([x - 0.04, y, 0.1], [x + 0.04, y + 0.09, 0.4]);
                }
                mesh.block([x - 0.05, -0.47, 0.36], [x + 0.05, 0.47, 0.44]);
            }
            mesh.paint(Material::Trim);
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
            mesh.air_deck();

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
            mesh.air_deck();

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
            mesh.air_deck();

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
            mesh.piers();

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
            mesh.piers();

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
            mesh.lit_seawalls();
            // A control tower on the north pier.
            mesh.paint(Material::Hull);
            mesh.frustum([-0.42, 0.42], [-0.32, 0.2], [0.06, 0.05], 6);
            mesh.paint(Material::Glow);
            mesh.frustum([-0.42, 0.42], [0.2, 0.28], [0.075, 0.07], 6);
        }
        // A support factory: its headquarters' floor, walls, deck, or piers, with booms slanting
        // over it in place of straight gantries, cranes, towers, and stacks: two at T2, the same
        // way, and four at T3, meeting in a diamond.
        Shape::FactoryT2Support => {
            mesh.factory_floor();
            mesh.bay_walls_t2();
            // Two booms slanting the same way across the bay, from opposite walls.
            mesh.service_arm([-0.3, -0.39, 0.05], [0.26, 0.3]);
            mesh.service_arm([0.3, 0.39, 0.05], [-0.26, -0.3]);
        }
        Shape::FactoryT3Support => {
            mesh.factory_floor();
            mesh.bay_walls_t3();
            // Four booms from the corners, meeting over the middle in a diamond.
            for (x, y) in [(-0.32_f32, -0.375_f32), (0.32, -0.375), (-0.32, 0.375), (0.32, 0.375)] {
                mesh.service_arm([x, y, 0.1], [-x * 0.7, -y * 0.75]);
            }
        }
        Shape::AirFactoryT2Support => {
            mesh.air_deck();
            mesh.service_block([0.32, 0.5]);
            for x in [-0.34_f32, 0.06] {
                mesh.service_arm([x, 0.42, -0.27], [0.24, -0.32]);
            }
        }
        Shape::AirFactoryT3Support => {
            mesh.air_deck();
            for (y, inward) in [([0.36_f32, 0.5_f32], -1.0_f32), ([-0.5, -0.36], 1.0)] {
                mesh.service_block(y);
                for x in [-0.3_f32, 0.3] {
                    mesh.service_arm([x, -inward * 0.42, -0.27], [-x * 0.6, inward * 0.3]);
                }
            }
        }
        Shape::NavalFactoryT2Support => {
            mesh.piers();
            mesh.service_arm([-0.3, -0.41, -0.32], [0.26, 0.32]);
            mesh.service_arm([0.3, 0.41, -0.32], [-0.26, -0.32]);
        }
        Shape::NavalFactoryT3Support => {
            mesh.piers();
            mesh.lit_seawalls();
            for (x, y) in [(-0.32_f32, -0.41_f32), (0.32, -0.41), (-0.32, 0.41), (0.32, 0.41)] {
                mesh.service_arm([x, y, -0.32], [-x * 0.7, -y * 0.75]);
            }
        }
        Shape::TorpedoLauncher => mesh.torpedo_turret(0.3, 0.06, &[0.0]),
        // Two pods of tubes either side of a cupola with a sonar mast.
        Shape::TorpedoLauncherT2 => {
            mesh.torpedo_turret(0.4, 0.06, &[-0.2, 0.2]);
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, -0.12], [0.06, 0.22], [0.1, 0.08], 8);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.12], [0.22, 0.44], [0.022, 0.016], 6);
            mesh.paint(Material::Glow);
            mesh.block([-0.035, -0.155, 0.42], [0.035, -0.085, 0.47]);
        }
        Shape::EngineerT2 => mesh.engineer_arms(2),
        Shape::EngineerT3 => mesh.engineer_arms(3),
        Shape::TransportT2 => {
            mesh.body(0.0, &[[-0.48, 0.07, 0.06, 0.02], [-0.32, 0.12, 0.11, 0.02], [0.26, 0.12, 0.11, 0.02], [0.4, 0.09, 0.08, 0.0], [0.49, 0.035, 0.035, -0.02]], 8);
            mesh.paint(Material::Glass);
            mesh.body(0.0, &[[0.32, 0.05, 0.03, 0.1], [0.41, 0.07, 0.045, 0.08], [0.47, 0.025, 0.02, 0.04]], 6);
            mesh.paint(Material::Panel);
            mesh.block([-0.08, -0.49, -0.07], [0.08, -0.44, 0.06]);
            mesh.paint(Material::Hull);
            for y in [-0.27_f32, 0.2] {
                mesh.wings(0.1, 0.22, [y - 0.06, y + 0.06], [y - 0.05, y + 0.04], [0.02, 0.012], 0.05);
            }
            mesh.fin(0.0, [-0.48, -0.36], 0.1, [-0.48, -0.42], 0.26, 0.015);
            for x in [-0.34_f32, 0.34] {
                for y in [-0.27_f32, 0.2] {
                    mesh.ducted_rotor([x, y], 0.13, true);
                }
            }
            mesh.paint(Material::Glow);
            mesh.block([-0.05, 0.44, 0.04], [0.05, 0.46, 0.07]);
        }
        Shape::TransportT3 => {
            // A wide, flat-topped cargo body, a cockpit at its nose, and a ramp at its tail.
            mesh.paint(Material::Hull);
            mesh.bevel([-0.17, -0.46, -0.1], [0.17, 0.3, 0.1], 0.05);
            mesh.cuboid(
                [Vec3::new(-0.17, 0.3, -0.1), Vec3::new(0.17, 0.3, -0.1), Vec3::new(0.1, 0.48, -0.06), Vec3::new(-0.1, 0.48, -0.06)],
                [Vec3::new(-0.17, 0.3, 0.05), Vec3::new(0.17, 0.3, 0.05), Vec3::new(0.1, 0.48, 0.0), Vec3::new(-0.1, 0.48, 0.0)],
            );
            mesh.paint(Material::Glass);
            mesh.block([-0.09, 0.38, 0.02], [0.09, 0.44, 0.05]);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.12, -0.42, 0.1], [0.12, 0.26, 0.13], 0.02);
            mesh.paint(Material::Panel);
            mesh.block([-0.12, -0.5, -0.08], [0.12, -0.46, 0.06]);
            // A spar either side, carrying three rotors.
            mesh.paint(Material::Hull);
            for y in [-0.3_f32, 0.0, 0.3] {
                mesh.wings(0.17, 0.26, [y - 0.04, y + 0.04], [y - 0.04, y + 0.04], [0.02, 0.02], 0.04);
            }
            mesh.paint(Material::Metal);
            for x in [-0.36_f32, 0.36] {
                mesh.block([x - 0.025, -0.42, 0.02], [x + 0.025, 0.42, 0.06]);
                for y in [-0.3_f32, 0.0, 0.3] {
                    mesh.ducted_rotor([x, y], 0.12, false);
                }
            }
            mesh.paint(Material::Glow);
            mesh.block([-0.05, 0.47, -0.04], [0.05, 0.49, -0.01]);
        }
        Shape::SpyPlane => {
            mesh.body(0.0, &[[-0.5, 0.03, 0.02, 0.0], [-0.2, 0.06, 0.04, 0.0], [0.2, 0.05, 0.035, 0.0], [0.42, 0.025, 0.02, -0.005], [0.5, 0.004, 0.004, -0.01]], 8);
            mesh.paint(Material::Hull);
            mesh.wings(0.04, 0.3, [-0.42, 0.15], [-0.42, -0.3], [0.014, 0.006], 0.0);
            mesh.paint(Material::Metal);
            for x in [-0.15_f32, 0.15] {
                mesh.body(x, &[[-0.46, 0.035, 0.03, 0.01], [-0.1, 0.045, 0.04, 0.01], [0.12, 0.03, 0.03, 0.01]], 8);
            }
            mesh.paint(Material::Hull);
            for x in [-0.15_f32, 0.15] {
                mesh.fin(x, [-0.44, -0.3], 0.04, [-0.44, -0.38], 0.16, 0.008);
            }
            mesh.paint(Material::Glass);
            mesh.body(0.0, &[[0.24, 0.02, 0.012, 0.03], [0.32, 0.025, 0.018, 0.03], [0.38, 0.008, 0.006, 0.02]], 6);
        }
        // Slender where the commander is broad: a narrow body, collar, and backpack in team
        // colors, a small head, a mast and dish over its back, a thin tool arm on its left, and a
        // thin gun on its right.
        Shape::FieldRig => {
            mesh.frustum([0.0, 0.0], [-0.1, 0.22], [0.13, 0.17], 10);
            mesh.frustum([0.0, 0.0], [0.22, 0.27], [0.17, 0.12], 10);
            mesh.bevel([-0.08, -0.25, 0.0], [0.08, -0.14, 0.2], 0.03);
            mesh.paint(Material::Glow);
            mesh.block([-0.04, 0.155, 0.04], [0.04, 0.18, 0.14]);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, -0.2], [0.2, 0.4], [0.018, 0.014], 6);
            mesh.frustum([0.0, -0.2], [0.38, 0.43], [0.08, 0.11], 10);
            mesh.paint(Material::Hull);
            mesh.ball([0.0, 0.03, 0.33], 0.065, 8, 4);
            for x in [-0.22_f32, 0.22] {
                mesh.ball([x, 0.0, 0.2], 0.05, 6, 3);
            }
            mesh.paint(Material::Glow);
            mesh.block([-0.045, 0.085, 0.32], [0.045, 0.1, 0.36]);
            // A tool arm on the left, reaching forward.
            mesh.paint(Material::Hull);
            mesh.beam([-0.22, 0.0, 0.18], [-0.26, 0.16, 0.06], 0.03, 0.03);
            mesh.paint(Material::Metal);
            mesh.beam([-0.26, 0.16, 0.06], [-0.22, 0.36, 0.1], 0.026, 0.026);
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [-0.22, 0.38, 0.1], |mesh| mesh.frustum([-0.22, 0.38], [0.04, 0.14], [0.04, 0.024], 6));
            // The gun on the right.
            mesh.paint(Material::Hull);
            mesh.beam([0.22, 0.0, 0.18], [0.24, 0.08, 0.04], 0.035, 0.035);
            mesh.paint(Material::Metal);
            mesh.bevel([0.19, 0.0, -0.02], [0.29, 0.24, 0.08], 0.02);
            mesh.moving(motion::RECOIL, [0.24, 0.0, 0.03], |mesh| mesh.body(0.24, &[[0.22, 0.03, 0.03, 0.03], [0.5, 0.025, 0.025, 0.03]], 6));
            mesh.flash([0.24, 0.47, 0.03], 0.05);
        }
        Shape::FieldRigLegs => {
            mesh.paint(Material::Hull);
            mesh.bevel([-0.13, -0.08, -0.17], [0.13, 0.08, -0.08], 0.025);
            for x in [-0.11_f32, 0.11] {
                mesh.moving(motion::SWING, [x, 0.0, -0.12], |mesh| {
                    mesh.paint(Material::Hull);
                    mesh.beam([x, 0.0, -0.12], [x, -0.13, -0.28], 0.04, 0.038);
                    mesh.paint(Material::Metal);
                    mesh.beam([x, -0.13, -0.28], [x, 0.03, -0.45], 0.032, 0.032);
                    mesh.ball([x, -0.13, -0.28], 0.035, 6, 3);
                    mesh.paint(Material::Hull);
                    for (toe, spread) in [(-0.035_f32, -0.02_f32), (0.0, 0.0), (0.035, 0.02)] {
                        mesh.beam([x, 0.0, -0.47], [x + toe + spread, 0.15, -0.47], 0.018, 0.025);
                    }
                    mesh.beam([x, 0.02, -0.47], [x, -0.1, -0.47], 0.018, 0.025);
                });
            }
        }
        Shape::RadarT2 => {
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.5, -0.4], [0.44, 0.38], 6);
            mesh.paint(Material::Trim);
            mesh.bevel([-0.34, -0.32, -0.4], [-0.08, -0.06, -0.2], 0.02);
            mesh.bevel([0.08, 0.06, -0.4], [0.34, 0.32, -0.2], 0.02);
            mesh.paint(Material::Metal);
            for (x, y) in [(-1.0_f32, -1.0_f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                mesh.beam([x * 0.15, y * 0.15, -0.4], [x * 0.04, y * 0.04, 0.26], 0.014, 0.014);
            }
            for z in [-0.22_f32, -0.02, 0.16] {
                let half = 0.15 - (z + 0.4) / 0.66 * 0.11;
                mesh.block([-half, -half, z], [half, half, z + 0.02]);
            }
            // Two antennas crossing, which turn together.
            mesh.paint(Material::Trim);
            mesh.moving(motion::SPIN, [0.0; 3], |mesh| {
                mesh.cuboid(
                    [Vec3::new(-0.4, -0.06, 0.26), Vec3::new(0.4, -0.06, 0.26), Vec3::new(0.4, 0.03, 0.26), Vec3::new(-0.4, 0.03, 0.26)],
                    [Vec3::new(-0.4, 0.05, 0.5), Vec3::new(0.4, 0.05, 0.5), Vec3::new(0.4, 0.14, 0.5), Vec3::new(-0.4, 0.14, 0.5)],
                );
                mesh.cuboid(
                    [Vec3::new(-0.06, -0.4, 0.26), Vec3::new(0.03, -0.4, 0.26), Vec3::new(0.03, 0.4, 0.26), Vec3::new(-0.06, 0.4, 0.26)],
                    [Vec3::new(0.05, -0.4, 0.5), Vec3::new(0.14, -0.4, 0.5), Vec3::new(0.14, 0.4, 0.5), Vec3::new(0.05, 0.4, 0.5)],
                );
            });
        }
        Shape::SonarT2 => {
            let float = [[-0.15, -0.27], [0.15, -0.27], [0.3, 0.0], [0.15, 0.27], [-0.15, 0.27], [-0.3, 0.0]];
            mesh.paint(Material::Hull);
            mesh.prism(&float, [-0.5, -0.3], [0.85, 1.0]);
            mesh.paint(Material::Trim);
            mesh.prism(&float, [-0.3, -0.26], [0.98, 0.9]);
            // Three outrigger pontoons on booms, each with a team-colored cap.
            for index in 0..3 {
                let angle = 0.52 + index as f32 / 3.0 * std::f32::consts::TAU;
                let (x, y) = (angle.cos() * 0.36, angle.sin() * 0.36);
                mesh.paint(Material::Metal);
                mesh.beam([angle.cos() * 0.2, angle.sin() * 0.2, -0.36], [x, y, -0.36], 0.025, 0.02);
                mesh.paint(Material::Hull);
                mesh.frustum([x, y], [-0.5, -0.32], [0.12, 0.1], 8);
                mesh.paint(Material::Trim);
                mesh.frustum([x, y], [-0.32, -0.29], [0.1, 0.09], 8);
            }
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, 0.0], [-0.26, 0.36], [0.05, 0.035], 6);
            mesh.paint(Material::Glow);
            mesh.moving(motion::PULSE, [0.0; 3], |mesh| {
                for z in [-0.06_f32, 0.08, 0.22] {
                    mesh.frustum([0.0, 0.0], [z, z + 0.04], [0.15, 0.15], 10);
                }
            });
            mesh.paint(Material::Trim);
            mesh.ball([0.0, 0.0, 0.41], 0.08, 8, 4);
        }
        Shape::PointDefenseT2 => {
            mesh.prism(&[[-0.26, -0.26], [0.26, -0.26], [0.3, -0.02], [0.2, 0.18], [-0.2, 0.18], [-0.3, -0.02]], [-0.05, 0.26], [1.0, 0.85]);
            // Armored cheeks either side.
            mesh.paint(Material::Hull);
            for x in [-0.3_f32, 0.3] {
                mesh.bevel([x - 0.045, -0.22, -0.04], [x + 0.045, 0.12, 0.2], 0.02);
            }
            mesh.paint(Material::Metal);
            for x in [-0.09_f32, 0.09] {
                mesh.moving(motion::RECOIL, [x, 0.12, 0.12], |mesh| mesh.body(x, &[[0.12, 0.042, 0.042, 0.12], [0.48, 0.035, 0.035, 0.12]], 6));
                mesh.flash([x, 0.47, 0.12], 0.05);
            }
            mesh.paint(Material::Hull);
            mesh.bevel([-0.08, -0.2, 0.26], [0.08, 0.0, 0.38], 0.03);
            mesh.paint(Material::Glow);
            mesh.block([-0.06, 0.0, 0.3], [0.06, 0.012, 0.34]);
        }
        // On its emplacement's turntable: a sloped casemate between trunnion braces, an
        // ammunition hoist behind, and a long barrel laid low, with recoil cylinders and a muzzle
        // brake, all kicking back.
        Shape::ArtilleryGunT3 => {
            mesh.cuboid(
                [Vec3::new(-0.24, -0.3, -0.4), Vec3::new(0.24, -0.3, -0.4), Vec3::new(0.24, 0.14, -0.4), Vec3::new(-0.24, 0.14, -0.4)],
                [Vec3::new(-0.17, -0.24, -0.12), Vec3::new(0.17, -0.24, -0.12), Vec3::new(0.17, 0.06, -0.12), Vec3::new(-0.17, 0.06, -0.12)],
            );
            mesh.paint(Material::Hull);
            for x in [-0.27_f32, 0.27] {
                mesh.bevel([x - 0.04, -0.1, -0.4], [x + 0.04, 0.1, -0.06], 0.02);
            }
            mesh.bevel([-0.1, -0.38, -0.4], [0.1, -0.3, -0.18], 0.02);
            mesh.paint(Material::Metal);
            mesh.moving(motion::RECOIL, [0.0, -0.12, -0.2], |mesh| {
                mesh.beam([0.0, -0.12, -0.2], [0.0, 0.47, -0.04], 0.065, 0.065);
                mesh.beam([0.0, 0.41, -0.056], [0.0, 0.475, -0.038], 0.095, 0.065);
                for x in [-0.06_f32, 0.06] {
                    mesh.beam([x, -0.06, -0.13], [x, 0.2, -0.06], 0.02, 0.02);
                }
            });
            mesh.flash([0.0, 0.48, -0.03], 0.07);
        }
        Shape::ArtilleryEmplacement => {
            mesh.paint(Material::Hull);
            mesh.block([-0.5, -0.5, -0.5], [0.5, 0.5, -0.46]);
            // Revetments round the edge, sloping in.
            let wall = |mesh: &mut MeshBuilder, bottom: [[f32; 2]; 4], top: [[f32; 2]; 4]| {
                mesh.cuboid(bottom.map(|[x, y]| Vec3::new(x, y, -0.46)), top.map(|[x, y]| Vec3::new(x, y, -0.36)));
            };
            for side in [-1.0_f32, 1.0] {
                let (outer, inner, top_outer, top_inner) = (0.5 * side, 0.4 * side, 0.47 * side, 0.42 * side);
                wall(&mut *mesh, [[-0.5, outer], [0.5, outer], [0.5, inner], [-0.5, inner]], [[-0.47, top_outer], [0.47, top_outer], [0.47, top_inner], [-0.47, top_inner]]);
                wall(&mut *mesh, [[outer, -0.4], [inner, -0.4], [inner, 0.4], [outer, 0.4]], [[top_outer, -0.4], [top_inner, -0.4], [top_inner, 0.4], [top_outer, 0.4]]);
            }
            // Bunkers at the corners, with team-colored roofs and a light each.
            for (x, y) in [(-0.39_f32, -0.39_f32), (0.39, -0.39), (-0.39, 0.39), (0.39, 0.39)] {
                mesh.paint(Material::Hull);
                mesh.bevel([x - 0.08, y - 0.08, -0.46], [x + 0.08, y + 0.08, -0.33], 0.02);
                mesh.paint(Material::Trim);
                mesh.block([x - 0.07, y - 0.07, -0.33], [x + 0.07, y + 0.07, -0.31]);
                mesh.paint(Material::Glow);
                mesh.block([x - x.signum() * 0.09, y - 0.015, -0.4], [x - x.signum() * 0.08, y + 0.015, -0.37]);
            }
            // The turntable, in team colors, and rails bringing shells to it from the back.
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, 0.0], [-0.46, -0.41], [0.32, 0.31], 16);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, 0.0], [-0.41, -0.4], [0.27, 0.27], 16);
            for x in [-0.05_f32, 0.05] {
                mesh.block([x - 0.012, -0.4, -0.46], [x + 0.012, -0.3, -0.445]);
            }
        }
        Shape::ShieldGeneratorT3 => {
            mesh.paint(Material::Hull);
            mesh.frustum([0.0, 0.0], [-0.5, -0.3], [0.5, 0.42], 8);
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, 0.0], [-0.3, -0.24], [0.34, 0.3], 8);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, 0.0], [-0.3, 0.2], [0.16, 0.12], 6);
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, 0.0], [0.2, 0.35], [0.12, 0.3], 8);
            mesh.paint(Material::Glow);
            mesh.frustum([0.0, 0.0], [0.35, 0.5], [0.3, 0.1], 8);
            // Four emitter pylons round the column, with lit tips.
            for (x, y) in [(-0.27_f32, -0.27_f32), (0.27, -0.27), (-0.27, 0.27), (0.27, 0.27)] {
                mesh.paint(Material::Hull);
                mesh.frustum([x, y], [-0.3, 0.08], [0.06, 0.045], 6);
                mesh.paint(Material::Glow);
                mesh.ball([x, y, 0.12], 0.05, 6, 3);
            }
        }
        Shape::FabricatorT3 => {
            mesh.paint(Material::Hull);
            mesh.bevel([-0.48, -0.48, -0.5], [0.48, 0.48, -0.4], 0.03);
            // Four cooling fins in an X.
            mesh.paint(Material::Metal);
            for (x, y) in [(-1.0_f32, -1.0_f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                mesh.beam([x * 0.16, y * 0.16, -0.12], [x * 0.42, y * 0.42, -0.12], 0.03, 0.28);
            }
            // The core in team colors, ringed with light, under a team-colored dome.
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, 0.0], [-0.4, 0.3], [0.26, 0.24], 12);
            mesh.paint(Material::Glow);
            for z in [-0.26_f32, 0.0, 0.24] {
                mesh.frustum([0.0, 0.0], [z, z + 0.06], [0.3, 0.3], 12);
            }
            mesh.paint(Material::Trim);
            mesh.frustum([0.0, 0.0], [0.3, 0.45], [0.24, 0.1], 12);
        }
        Shape::FieldWorkshopT3 => {
            mesh.workshop_pad();
            mesh.workshop_mast([-0.22, -0.22], [0.22, -0.02], 0.34);
            mesh.workshop_mast([0.22, 0.22], [-0.22, 0.02], 0.42);
            mesh.workshop_lights();
        }
        // A half-track, long and narrow: wheels under fenders at the front, a hood and a cab, a
        // pedestal for the guns over the tracks, outriggers either side of it, and an ammunition
        // locker on the bed behind.
        Shape::FlakHull => {
            mesh.paint(Material::Metal);
            for (low, high) in [(-0.22_f32, -0.11_f32), (0.11, 0.22)] {
                mesh.bevel([low, -0.5, -0.5], [high, 0.12, -0.3], 0.04);
            }
            for x in [-0.165_f32, 0.165] {
                mesh.bevel([x - 0.05, 0.24, -0.5], [x + 0.05, 0.44, -0.32], 0.05);
            }
            mesh.paint(Material::Hull);
            for x in [-0.165_f32, 0.165] {
                mesh.block([x - 0.055, 0.22, -0.32], [x + 0.055, 0.46, -0.29]);
            }
            mesh.bevel([-0.13, -0.48, -0.46], [0.13, 0.2, -0.2], 0.03);
            // The hood, sloping down to the nose, and the cab behind it.
            mesh.cuboid(
                [Vec3::new(-0.11, 0.2, -0.46), Vec3::new(0.11, 0.2, -0.46), Vec3::new(0.11, 0.5, -0.46), Vec3::new(-0.11, 0.5, -0.46)],
                [Vec3::new(-0.11, 0.2, -0.2), Vec3::new(0.11, 0.2, -0.2), Vec3::new(0.11, 0.48, -0.3), Vec3::new(-0.11, 0.48, -0.3)],
            );
            mesh.bevel([-0.12, 0.16, -0.2], [0.12, 0.3, -0.1], 0.03);
            mesh.paint(Material::Glass);
            mesh.block([-0.09, 0.3, -0.17], [0.09, 0.31, -0.12]);
            mesh.paint(Material::Metal);
            mesh.frustum([0.0, 0.0], [-0.2, -0.1], [0.12, 0.11], 8);
            // Outriggers either side at mid-length, each on a pad: with the hull, a cross from above.
            for x in [-1.0_f32, 1.0] {
                mesh.beam([x * 0.13, 0.0, -0.24], [x * 0.41, 0.0, -0.44], 0.035, 0.03);
                mesh.bevel([x * 0.41 - 0.06, -0.07, -0.5], [x * 0.41 + 0.06, 0.07, -0.44], 0.015);
            }
            mesh.paint(Material::Trim);
            mesh.bevel([-0.11, -0.46, -0.2], [0.11, -0.3, -0.14], 0.02);
        }
        Shape::Airfield => {
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
        // A slim breech between two trunnion plates, with a long barrel outside each, raised to
        // the sky: an H from above, where a point defense is a block with its gun down the middle.
        Shape::AntiAirGun => {
            mesh.bevel([-0.1, -0.24, -0.1], [0.1, 0.12, 0.16], 0.03);
            for x in [-0.2_f32, 0.2] {
                mesh.paint(Material::Trim);
                mesh.bevel([x - 0.045, -0.16, -0.1], [x + 0.045, 0.08, 0.22], 0.02);
                mesh.paint(Material::Metal);
                mesh.moving(motion::RECOIL, [x, -0.12, 0.08], |mesh| mesh.beam([x, -0.2, 0.02], [x, 0.4, 0.44], 0.035, 0.035));
                mesh.flash([x, 0.41, 0.45], 0.045);
            }
            mesh.paint(Material::Hull);
            mesh.block([-0.2, -0.1, 0.02], [0.2, -0.02, 0.08]);
        }
        Shape::RigYard => {
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
        // Eight wheels under a long chassis, a cab in front, and a bed for the missile box.
        Shape::MissileTruck => {
            mesh.paint(Material::Metal);
            for x in [-0.165_f32, 0.165] {
                for y in [-0.38_f32, -0.22, -0.06, 0.36] {
                    mesh.bevel([x - 0.035, y - 0.06, -0.5], [x + 0.035, y + 0.06, -0.34], 0.02);
                }
            }
            mesh.paint(Material::Hull);
            for x in [-0.165_f32, 0.165] {
                mesh.block([x - 0.04, -0.45, -0.34], [x + 0.04, 0.01, -0.315]);
                mesh.block([x - 0.04, 0.29, -0.34], [x + 0.04, 0.43, -0.315]);
            }
            mesh.paint(Material::Hull);
            mesh.bevel([-0.125, -0.5, -0.46], [0.125, 0.48, -0.32], 0.02);
            mesh.bevel([-0.14, -0.48, -0.32], [0.14, 0.27, -0.27], 0.02);
            mesh.bevel([-0.14, 0.29, -0.32], [0.14, 0.47, -0.12], 0.03);
            mesh.paint(Material::Glass);
            mesh.block([-0.11, 0.47, -0.23], [0.11, 0.485, -0.16]);
        }
        // Four wheels under a short, wide body, a cab in front, and team-colored fenders.
        Shape::ShieldCarrier => {
            mesh.paint(Material::Metal);
            for x in [-0.34_f32, 0.34] {
                for y in [-0.28_f32, 0.28] {
                    mesh.bevel([x - 0.06, y - 0.11, -0.5], [x + 0.06, y + 0.11, -0.28], 0.03);
                }
            }
            mesh.paint(Material::Trim);
            for x in [-0.34_f32, 0.34] {
                for y in [-0.28_f32, 0.28] {
                    mesh.block([x - 0.065, y - 0.13, -0.28], [x + 0.065, y + 0.13, -0.25]);
                }
            }
            mesh.paint(Material::Hull);
            mesh.bevel([-0.28, -0.46, -0.44], [0.28, 0.46, -0.22], 0.03);
            mesh.bevel([-0.24, 0.26, -0.22], [0.24, 0.45, -0.06], 0.03);
            mesh.paint(Material::Glass);
            mesh.block([-0.2, 0.45, -0.16], [0.2, 0.465, -0.09]);
        }
        // Narrow tracks under fenders, either side of a long body for the casemate.
        Shape::CasemateHull => {
            mesh.paint(Material::Metal);
            for (low, high) in [(-0.265_f32, -0.17_f32), (0.17, 0.265)] {
                mesh.bevel([low, -0.46, -0.5], [high, 0.2, -0.28], 0.04);
            }
            mesh.paint(Material::Hull);
            for (low, high) in [(-0.267_f32, -0.16_f32), (0.16, 0.267)] {
                mesh.block([low, -0.48, -0.28], [high, 0.22, -0.25]);
            }
            mesh.cuboid(
                [Vec3::new(-0.18, -0.44, -0.46), Vec3::new(0.18, -0.44, -0.46), Vec3::new(0.18, 0.24, -0.46), Vec3::new(-0.18, 0.24, -0.46)],
                [Vec3::new(-0.18, -0.44, -0.16), Vec3::new(0.18, -0.44, -0.16), Vec3::new(0.18, 0.1, -0.16), Vec3::new(-0.18, 0.1, -0.16)],
            );
            mesh.paint(Material::Panel);
            for y in [-0.4_f32, -0.33] {
                mesh.block([-0.12, y - 0.02, -0.16], [0.12, y + 0.02, -0.15]);
            }
            // Stabilizer spades splayed out behind, dug in when it fires.
            for x in [-1.0_f32, 1.0] {
                mesh.paint(Material::Metal);
                mesh.beam([x * 0.14, -0.42, -0.3], [x * 0.3, -0.45, -0.45], 0.03, 0.02);
                mesh.paint(Material::Hull);
                mesh.bevel([x * 0.3 - 0.07, -0.49, -0.5], [x * 0.3 + 0.07, -0.44, -0.42], 0.01);
            }
        }
        // An engineer's hull, longer, with a gun turret over its right track: it fights too.
        Shape::CombatEngineerHull => {
            mesh.paint(Material::Metal);
            for (low, high) in [(-0.39_f32, -0.24_f32), (0.24, 0.39)] {
                mesh.bevel([low, -0.5, -0.5], [high, 0.36, -0.3], 0.04);
            }
            mesh.paint(Material::Hull);
            for (low, high) in [(-0.395_f32, -0.22_f32), (0.22, 0.395)] {
                mesh.block([low, -0.5, -0.3], [high, 0.36, -0.27]);
            }
            mesh.cuboid(
                [Vec3::new(-0.24, -0.46, -0.46), Vec3::new(0.24, -0.46, -0.46), Vec3::new(0.24, 0.34, -0.46), Vec3::new(-0.24, 0.34, -0.46)],
                [Vec3::new(-0.24, -0.46, -0.18), Vec3::new(0.24, -0.46, -0.18), Vec3::new(0.24, 0.14, -0.18), Vec3::new(-0.24, 0.14, -0.18)],
            );
            mesh.paint(Material::Trim);
            mesh.bevel([-0.2, -0.46, -0.18], [0.2, -0.38, -0.14], 0.02);
            mesh.frustum([0.3, 0.12], [-0.27, -0.17], [0.1, 0.085], 8);
            mesh.paint(Material::Metal);
            mesh.moving(motion::RECOIL, [0.3, 0.18, -0.22], |mesh| mesh.beam([0.3, 0.18, -0.22], [0.3, 0.48, -0.22], 0.022, 0.022));
            mesh.flash([0.3, 0.47, -0.22], 0.04);
            // A dozer blade across the front, wider than the tracks, on push arms.
            mesh.paint(Material::Metal);
            for x in [-0.3_f32, 0.3] {
                mesh.beam([x, 0.2, -0.36], [x, 0.41, -0.42], 0.025, 0.025);
            }
            mesh.paint(Material::Hull);
            mesh.cuboid(
                [Vec3::new(-0.46, 0.4, -0.5), Vec3::new(0.46, 0.4, -0.5), Vec3::new(0.46, 0.46, -0.5), Vec3::new(-0.46, 0.46, -0.5)],
                [Vec3::new(-0.46, 0.43, -0.31), Vec3::new(0.46, 0.43, -0.31), Vec3::new(0.46, 0.49, -0.31), Vec3::new(-0.46, 0.49, -0.31)],
            );
            mesh.paint(Material::Trim);
            mesh.cuboid(
                [Vec3::new(-0.46, 0.43, -0.31), Vec3::new(0.46, 0.43, -0.31), Vec3::new(0.46, 0.49, -0.31), Vec3::new(-0.46, 0.49, -0.31)],
                [Vec3::new(-0.46, 0.432, -0.29), Vec3::new(0.46, 0.432, -0.29), Vec3::new(0.46, 0.492, -0.29), Vec3::new(-0.46, 0.492, -0.29)],
            );
        }
        // A team-colored twin anti-aircraft gun on a mount, for a truck's bed.
        Shape::TruckAntiAir => {
            mesh.frustum([0.0, -0.08], [-0.16, -0.06], [0.12, 0.11], 8);
            mesh.block([-0.11, -0.18, -0.06], [0.11, 0.04, 0.06]);
            mesh.paint(Material::Metal);
            for x in [-0.065_f32, 0.065] {
                mesh.moving(motion::RECOIL, [x, -0.02, 0.0], |mesh| mesh.beam([x, -0.02, 0.0], [x, 0.3, 0.22], 0.028, 0.028));
                mesh.flash([x, 0.3, 0.22], 0.04);
            }
        }
        // A square team-colored turret with four barrels side by side, raised for aircraft.
        // A wide round gun tub in team colors, overhanging the hull either side, with four
        // barrels and a radar panel turning on a mast at its back.
        Shape::FlakTurret => {
            mesh.frustum([0.0, 0.0], [-0.1, 0.05], [0.27, 0.25], 12);
            mesh.paint(Material::Panel);
            mesh.frustum([0.0, 0.0], [0.05, 0.06], [0.2, 0.2], 12);
            mesh.paint(Material::Hull);
            mesh.bevel([-0.1, -0.12, 0.06], [0.1, 0.08, 0.16], 0.03);
            mesh.paint(Material::Metal);
            for x in [-0.075_f32, -0.025, 0.025, 0.075] {
                mesh.moving(motion::RECOIL, [x, 0.04, 0.1], |mesh| mesh.beam([x, 0.04, 0.1], [x, 0.36, 0.26], 0.018, 0.018));
            }
            mesh.flash([0.0, 0.36, 0.26], 0.07);
            mesh.frustum([0.0, -0.2], [0.05, 0.28], [0.016, 0.012], 6);
            mesh.paint(Material::Trim);
            mesh.moving(motion::SPIN, [0.0, -0.2, 0.0], |mesh| {
                mesh.cuboid(
                    [Vec3::new(-0.13, -0.23, 0.26), Vec3::new(0.13, -0.23, 0.26), Vec3::new(0.13, -0.2, 0.26), Vec3::new(-0.13, -0.2, 0.26)],
                    [Vec3::new(-0.13, -0.19, 0.38), Vec3::new(0.13, -0.19, 0.38), Vec3::new(0.13, -0.16, 0.38), Vec3::new(-0.13, -0.16, 0.38)],
                );
            });
        }
        Shape::NavalTurret => mesh.turret([0.0, 0.0], [0.0, 0.09], 0.095, 2, 0.06, false),
        Shape::Beam => {
            mesh.paint(Material::Glow);
            mesh.beam([-0.5, 0.0, 0.0], [0.5, 0.0, 0.0], 0.5, 0.5);
        }
        Shape::Cable => {
            mesh.paint(Material::Hull);
            mesh.block([-0.5, -0.5, -0.5], [0.5, 0.5, 0.2]);
            mesh.paint(Material::Glow);
            mesh.block([-0.5, -0.2, 0.2], [0.5, 0.2, 0.5]);
        }
        Shape::JunctionBox => {
            mesh.paint(Material::Hull);
            mesh.bevel([-0.46, -0.5, -0.5], [0.46, 0.5, 0.2], 0.12);
            // A team-colored band where the cable plugs in, either side.
            mesh.paint(Material::Trim);
            mesh.block([-0.5, -0.2, -0.3], [0.5, 0.2, 0.0]);
            mesh.paint(Material::Glow);
            mesh.frustum([0.0, 0.0], [0.2, 0.5], [0.22, 0.16], 8);
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

/// Whether a shape is shaded darker where its parts crowd one another (`Some`), and if so
/// whether it stands on the ground, which crowds its lowest parts too. Markers and effects are
/// not shaded so; aircraft fly and ships float, clear of the ground at the bottom of their box.
const fn occlusion_ground(shape: Shape) -> Option<bool> {
    match shape {
        Shape::Cube | Shape::Shell | Shape::Missile | Shape::Fireball | Shape::Smoke | Shape::Wake | Shape::Scar | Shape::Dome | Shape::SelectionRing | Shape::SelectionBox | Shape::Beam | Shape::Cable => None,
        Shape::Aircraft
        | Shape::Bomber
        | Shape::Gunship
        | Shape::StrategicBomber
        | Shape::AirSuperiority
        | Shape::HeavyGunship
        | Shape::Transport
        | Shape::TransportT2
        | Shape::TransportT3
        | Shape::Scout
        | Shape::SpyPlane
        | Shape::FighterBomber
        | Shape::TorpedoBomber
        | Shape::Frigate
        | Shape::Destroyer
        | Shape::Cruiser
        | Shape::Battleship
        | Shape::Battlecruiser
        | Shape::Submarine
        | Shape::MissileSubmarine
        | Shape::EscortCorvette
        | Shape::TorpedoBoat
        | Shape::NavalTurret => Some(false),
        _ => Some(true),
    }
}

/// How far the sides of a box-built land unit's boxes lean in, as a share of their height, so
/// that its hulls, turrets, and housings read as sloped armor rather than as blocks: about 17
/// degrees, well inside `SMOOTH_CREASE_DEGREES`' crisp edges. It costs no triangles.
const ARMOR_SLOPE: f32 = 0.3;
/// The most a leaning box's top is drawn in on each side, as a share of the box, so that thin
/// parts keep a top at least two fifths as wide as their foot.
const SLOPE_INSET_MOST: f32 = 0.3;

/// Whether a shape is drawn in sloped armor (`ARMOR_SLOPE`): the land units built from boxes,
/// their hulls, turrets, and legs. The command rig and field rig keep their own lines.
const fn sloped_armor(shape: Shape) -> bool {
    matches!(
        shape,
        Shape::Bot
            | Shape::BotLegs
            | Shape::GrenadierWalker
            | Shape::HeavyWalker
            | Shape::SiegeWalker
            | Shape::Tank
            | Shape::TankHull
            | Shape::HeavyTank
            | Shape::HeavyTankHull
            | Shape::Artillery
            | Shape::ArtilleryHull
            | Shape::HeavyArtillery
            | Shape::CasemateHull
            | Shape::TruckAntiAir
            | Shape::WheeledHull
            | Shape::FlakTurret
            | Shape::FlakHull
            | Shape::HoverHull
            | Shape::MissileRack
            | Shape::MissileTruck
            | Shape::LandScout
            | Shape::ShieldEmitter
            | Shape::ShieldCarrier
            | Shape::CombatEngineerHull
            | Shape::EngineerHull
    )
}

/// Faces meeting at less than this angle, in degrees, are shaded as one smooth surface, so that
/// round parts look round; sharper edges stay crisp: a box's corners, a bevel's 45 degrees, and
/// the rim of a cylinder's cap. Until 2026-11-23 every face was shaded flat, which made every
/// unit look cut from facets.
const SMOOTH_CREASE_DEGREES: f32 = 40.0;
/// Round parts asked for at least `ROUND_SIDES_LEAST` sides, and as wide as `ROUND_SIDES_RADIUS`
/// or wider, in shares of the shape's box, are drawn with twice as many, up to
/// `ROUND_SIDES_MOST`, so that their outlines look round too. Narrower parts, such as barrels and
/// bollards, are too small for their outline to show, and smooth shading is enough. Parts of
/// fewer sides, such as boulders, are faceted on purpose and keep theirs.
const ROUND_SIDES_LEAST: usize = 6;
const ROUND_SIDES_RADIUS: f32 = 0.06;
const ROUND_SIDES_MOST: usize = 24;

/// How many sides a round part of `radius` asked for with `sides` is drawn with
/// (`ROUND_SIDES_LEAST`).
fn round_sides(sides: usize, radius: f32) -> usize {
    if sides < ROUND_SIDES_LEAST || radius < ROUND_SIDES_RADIUS {
        return sides;
    }
    let doubled = if sides * 2 < ROUND_SIDES_MOST { sides * 2 } else { ROUND_SIDES_MOST };
    if doubled > sides { doubled } else { sides }
}

/// Shades faces that meet at a shallow angle as one smooth surface (`SMOOTH_CREASE_DEGREES`):
/// each vertex from `first` on takes the mean of the normals of the faces at its point that lie
/// within the crease of its own face, among parts that move alike.
fn smooth_normals(vertices: &mut [MeshVertex], first: usize) {
    let crease = SMOOTH_CREASE_DEGREES.to_radians().cos();
    let quantize = |value: f32| (value * 8192.0).round() as i32;
    let motion_bits = |material: u32| material & !(motion::MATERIAL_MASK | (0xff << motion::OPENNESS_SHIFT));
    let key = |vertex: &MeshVertex| (vertex.position.map(quantize), vertex.pivot.map(quantize), motion_bits(vertex.material));
    let mut groups: std::collections::BTreeMap<_, Vec<usize>> = std::collections::BTreeMap::new();
    for (index, vertex) in vertices.iter().enumerate().skip(first) {
        groups.entry(key(vertex)).or_default().push(index);
    }
    let faces: Vec<Vec3> = vertices.iter().map(|vertex| Vec3::from(vertex.normal)).collect();
    for members in groups.values().filter(|members| members.len() > 1) {
        for &index in members {
            let own = faces[index];
            let sum: Vec3 = members.iter().map(|&other| faces[other]).filter(|normal| normal.dot(own) >= crease).sum();
            vertices[index].normal = sum.try_normalize().unwrap_or(own).to_array();
        }
    }
}

/// The longest a side of a four-sided face may be, in shares of the shape's box, before the
/// face is cut into a grid for its shading.
const SUBDIVIDE_EDGE: f32 = 0.25;

/// Cells along each side of the grid a shape is sampled into for shading where parts meet.
const OPENNESS_CELLS: usize = 40;
/// How far, in shares of the shape's box, a vertex looks for parts crowding it.
const OPENNESS_REACH: f32 = 0.3;
/// How dark the most crowded vertex gets, as a share of its light.
const OPENNESS_FLOOR: f32 = 0.2;
/// How much darker the foot of a grounded shape is than its top.
const OPENNESS_FOOT: f32 = 0.22;

/// Works out how open each vertex of one shape is to the sky, from `first_vertex` on, and
/// keeps it in the top byte of its material: the shape's surfaces are marked in a grid, and
/// from each vertex a fan of rays over its side of the surface looks for them nearby. A
/// vertex in a corner, under an overhang, or by the ground sees less sky, and is shaded darker.
fn bake_openness(vertices: &mut [MeshVertex], first_vertex: usize, indices: &[u32], grounded: bool) {
    let cells = OPENNESS_CELLS;
    let cell = 1.0 / cells as f32;
    let index_of = |point: Vec3| -> Option<usize> {
        let at = ((point + Vec3::splat(0.5)) / cell).floor();
        if at.min_element() < 0.0 || at.max_element() >= cells as f32 {
            return None;
        }
        Some((at.z as usize * cells + at.y as usize) * cells + at.x as usize)
    };
    let mut solid = vec![false; cells * cells * cells];
    for triangle in indices.chunks(3) {
        let [a, b, c] = [0, 1, 2].map(|corner| Vec3::from(vertices[triangle[corner] as usize].position));
        let longest = (b - a).length().max((c - a).length()).max((c - b).length());
        let steps = ((longest / (cell * 0.5)).ceil() as usize).max(1);
        for i in 0..=steps {
            for j in 0..=steps - i {
                let (u, v) = (i as f32 / steps as f32, j as f32 / steps as f32);
                if let Some(at) = index_of(a + (b - a) * u + (c - a) * v) {
                    solid[at] = true;
                }
            }
        }
    }
    // A fixed fan of directions spread evenly over the hemisphere about +Z, low ones as many
    // as high, so that walls close by count: exaggerated, for a stylized look, beside light
    // from the sky, which falls mostly from overhead.
    const RAYS: usize = 32;
    let fan: Vec<Vec3> = (0..RAYS)
        .map(|index| {
            let height = (index as f32 + 0.5) / RAYS as f32;
            let around = index as f32 * 2.399_963;
            let spread = (1.0 - height * height).sqrt();
            Vec3::new(around.cos() * spread, around.sin() * spread, height)
        })
        .collect();
    let steps = (OPENNESS_REACH / cell).ceil() as usize;
    let mut done = std::collections::HashSet::new();
    for &index in indices {
        let index = index as usize;
        if index < first_vertex || !done.insert(index) {
            continue;
        }
        let vertex = vertices[index];
        let (normal, position) = (Vec3::from(vertex.normal), Vec3::from(vertex.position));
        if normal.length_squared() < 0.5 {
            continue;
        }
        // Turn the fan from +Z to the normal.
        let helper = if normal.z.abs() < 0.9 { Vec3::Z } else { Vec3::X };
        let side = helper.cross(normal).normalize();
        let other = normal.cross(side);
        let start = position + normal * cell * 1.5;
        let mut blocked = 0.0;
        for direction in &fan {
            let ray = side * direction.x + other * direction.y + normal * direction.z;
            for step in 0..steps {
                let point = start + ray * (step as f32 * cell);
                let hit = if grounded && point.z < -0.5 { true } else { index_of(point).is_some_and(|at| solid[at]) };
                if hit {
                    // Nearer parts crowd it more.
                    blocked += 1.0 - step as f32 / steps as f32 * 0.5;
                    break;
                }
            }
        }
        let mut openness = 1.0 - (blocked / RAYS as f32) * (1.0 - OPENNESS_FLOOR);
        // What stands on the ground darkens a little towards its foot, which grounds it.
        if grounded {
            openness *= 1.0 - OPENNESS_FOOT * (0.5 - position.z).clamp(0.0, 1.0);
        }
        let byte = (openness.clamp(0.0, 1.0) * 255.0).round() as u32;
        vertices[index].material = (vertex.material & !(0xff << motion::OPENNESS_SHIFT)) | (byte << motion::OPENNESS_SHIFT);
    }
}

/// Every shape, in one vertex and index buffer.
pub struct MeshSet {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
    /// The indices belonging to each shape, in the order of `Shape::ALL`.
    pub ranges: Vec<std::ops::Range<u32>>,
    /// The indices of each shape's far model, in the same order: a few boxes of its bulk and
    /// colors, drawn in its place once it is small on screen (`far_model`). A shape too simple to
    /// gain from one has its own indices here.
    pub far_ranges: Vec<std::ops::Range<u32>>,
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
            let first_vertex = mesh.vertices.len();
            mesh.subdivide = occlusion_ground(shape).is_some();
            mesh.slope = if sloped_armor(shape) { ARMOR_SLOPE } else { 0.0 };
            build_shape(shape, &mut mesh);
            smooth_normals(&mut mesh.vertices, first_vertex);
            let end = mesh.indices.len() as u32;
            if let Some(grounded) = occlusion_ground(shape) {
                bake_openness(&mut mesh.vertices, first_vertex, &mesh.indices[start as usize..end as usize], grounded);
            }
            ranges.push(start..end);
        }
        // Far models are plain: upright boxes, lit as open to the sky, with nothing moving.
        mesh.motion = 0;
        mesh.pivot = Vec3::ZERO;
        mesh.subdivide = false;
        mesh.slope = 0.0;
        let far_ranges = ranges
            .iter()
            .map(|range| {
                let model = far_model(&mesh.vertices, &mesh.indices[range.start as usize..range.end as usize], FAR_CELLS);
                let start = mesh.indices.len() as u32;
                // Only where it saves most of the work.
                if model.triangles() * FAR_SAVING > (range.end - range.start) / 3 {
                    return range.clone();
                }
                model.add_to(&mut mesh);
                start..mesh.indices.len() as u32
            })
            .collect();
        Self { vertices: mesh.vertices, indices: mesh.indices, ranges, far_ranges }
    }
}

/// How many cells a side a shape is cut into, seen from above, for its far model: each cell
/// that the shape covers becomes one box.
const FAR_CELLS: usize = 3;
/// A far model is only made for a shape with at least this many times its triangles.
const FAR_SAVING: u32 = 3;
/// A cell is part of the far model if the shape covers this share of it, seen from above.
const FAR_COVER: f32 = 0.25;
/// Half the width of the speck at a far model's middle that grows into its dot.
const FAR_DOT_SPECK: f32 = 1.0e-4;

/// One box of a far model: its corners, and what its top is painted with (a material, with
/// `motion::PULSE` if it glows only at work).
struct FarBox {
    min: [f32; 3],
    max: [f32; 3],
    top: u32,
}

/// A shape's far model: its boxes, and how high the shape's top is.
struct FarModel {
    boxes: Vec<FarBox>,
    top: f32,
}

impl FarModel {
    fn triangles(&self) -> u32 {
        self.boxes.len() as u32 * 10 + 2
    }

    fn add_to(&self, mesh: &mut MeshBuilder) {
        for part in &self.boxes {
            let corners = |z: f32| [Vec3::new(part.min[0], part.min[1], z), Vec3::new(part.max[0], part.min[1], z), Vec3::new(part.max[0], part.max[1], z), Vec3::new(part.min[0], part.max[1], z)];
            let (bottom, top) = (corners(part.min[2]), corners(part.max[2]));
            let center = (Vec3::from(part.min) + Vec3::from(part.max)) * 0.5;
            // Its sides are the body's color, and its top what the shape mostly shows there.
            // Nothing sees it from below.
            mesh.paint(Material::Hull);
            mesh.motion = 0;
            for index in 0..4 {
                let next = (index + 1) % 4;
                mesh.face(&[bottom[index], bottom[next], top[next], top[index]], center);
            }
            mesh.paint(material_of(part.top & motion::MATERIAL_MASK));
            mesh.motion = part.top & motion::PULSE;
            mesh.face(&top, center);
        }
        // As a shape turns into its dot, each vertex goes to the dot's corner on its side of the
        // middle, so the boxes alone may not fill it. This speck does: unseen at the middle of
        // the shape, with a corner on every side, it grows into the whole dot.
        mesh.paint(Material::Trim);
        mesh.motion = 0;
        let speck = FAR_DOT_SPECK;
        let corners = [Vec3::new(-speck, -speck, self.top), Vec3::new(speck, -speck, self.top), Vec3::new(speck, speck, self.top), Vec3::new(-speck, speck, self.top)];
        mesh.face(&corners, Vec3::new(0.0, 0.0, self.top - 1.0));
    }
}

fn material_of(bits: u32) -> Material {
    match bits {
        1 => Material::Hull,
        2 => Material::Metal,
        3 => Material::Glow,
        4 => Material::Panel,
        5 => Material::Glass,
        _ => Material::Trim,
    }
}

/// Works out a shape's far model from its triangles: seen from above, the shape is cut into
/// `cells` by `cells` cells, and each one it covers enough of becomes a box as high as the
/// shape's top is there on average, painted on top with what shows most there. So a far model
/// keeps the shape's outline, bulk, and colors, at a few dozen triangles. Muzzle flashes, which
/// show only for a moment, are left out.
fn far_model(vertices: &[MeshVertex], indices: &[u32], cells: usize) -> FarModel {
    const MATERIALS: usize = 6;
    let cells = cells.max(1);
    let kept = || indices.as_chunks::<3>().0.iter().filter(|triangle| triangle.iter().all(|&index| vertices[index as usize].material & motion::FLASH == 0));
    let triangles: Vec<[Vec3; 3]> = kept().map(|triangle| triangle.map(|corner| Vec3::from(vertices[corner as usize].position))).collect();
    let kinds: Vec<u32> = kept().map(|triangle| vertices[triangle[0] as usize].material & (motion::MATERIAL_MASK | motion::PULSE)).collect();
    let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for point in triangles.iter().flatten() {
        low = low.min(*point);
        high = high.max(*point);
    }
    if triangles.is_empty() {
        return FarModel { boxes: Vec::new(), top: 0.0 };
    }
    let size = (high - low).max(Vec3::splat(1.0e-3));
    let cell_of = |point: Vec3| {
        let along = |value: f32, from: f32, span: f32| (((value - from) / span * cells as f32) as usize).min(cells - 1);
        along(point.y, low.y, size.y) * cells + along(point.x, low.x, size.x)
    };
    // What shows from above in each cell: how much, how high, and of which material.
    let mut cover = vec![0.0_f32; cells * cells];
    let mut height = vec![0.0_f32; cells * cells];
    let mut shown = vec![[[0.0_f32; 2]; MATERIALS]; cells * cells];
    const STEPS: usize = 8;
    for (corners, kind) in triangles.iter().zip(&kinds) {
        let facing = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
        // Twice its area as seen from above, and only if it faces up.
        if facing.z <= 1.0e-7 {
            continue;
        }
        let samples = STEPS * (STEPS + 1) / 2;
        let share = facing.z * 0.5 / samples as f32;
        for row in 0..STEPS {
            for column in 0..STEPS - row {
                // The middles of a triangle of smaller triangles.
                let (u, v) = ((column as f32 + 1.0 / 3.0) / STEPS as f32, (row as f32 + 1.0 / 3.0) / STEPS as f32);
                let point = corners[0] + (corners[1] - corners[0]) * u + (corners[2] - corners[0]) * v;
                let cell = cell_of(point);
                cover[cell] += share;
                height[cell] += share * point.z;
                let material = ((kind & motion::MATERIAL_MASK) as usize).min(MATERIALS - 1);
                shown[cell][material][usize::from(kind & motion::PULSE != 0)] += share;
            }
        }
    }
    let cell_area = size.x * size.y / (cells * cells) as f32;
    let mut boxes = Vec::new();
    for cell in 0..cells * cells {
        if cover[cell] < cell_area * FAR_COVER {
            continue;
        }
        let (column, row) = (cell % cells, cell / cells);
        let edge = |index: usize, from: f32, span: f32| from + span * index as f32 / cells as f32;
        let top = (height[cell] / cover[cell]).max(low.z + size.z * 0.05);
        let mut most = (0_u32, 0.0_f32);
        for (material, by_pulse) in shown[cell].iter().enumerate() {
            for (pulsing, &area) in by_pulse.iter().enumerate() {
                if area > most.1 {
                    most = (material as u32 | if pulsing == 1 { motion::PULSE } else { 0 }, area);
                }
            }
        }
        boxes.push(FarBox { min: [edge(column, low.x, size.x), edge(row, low.y, size.y), low.z], max: [edge(column + 1, low.x, size.x), edge(row + 1, low.y, size.y), top], top: most.0 });
    }
    // A shape too thin to cover any cell is one box, all of it.
    if boxes.is_empty() {
        boxes.push(FarBox { min: low.to_array(), max: (low + size).to_array(), top: kinds.first().copied().unwrap_or(0) });
    }
    FarModel { boxes, top: high.z }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    fn shape_vertices(set: &MeshSet, shape: usize) -> Vec<MeshVertex> {
        set.indices[set.ranges[shape].start as usize..set.ranges[shape].end as usize]
            .iter()
            .map(|&index| set.vertices[index as usize])
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
            let at = |i: usize| Vec3::from(set.vertices[triangle[i] as usize].position);
            let facing = (at(1) - at(0)).cross(at(2) - at(0));
            let normal = Vec3::from(set.vertices[triangle[0] as usize].normal);
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
            let material = Material::ALL[(vertex.material & motion::MATERIAL_MASK) as usize];
            if !found.contains(&material) {
                found.push(material);
            }
        }
        found
    }

    /// The side of a `view`, in pixels.
    pub const VIEW_SIDE: usize = 96;

    /// A shape drawn as the camera sees it, looking north and tilted down by `pitch` degrees
    /// (90 is straight down), without perspective: each pixel holds the material nearest the
    /// eye there, if any. `parts` are drawn together, as a unit's base and the part on it are,
    /// each turned by its angle in radians about the vertical, then moved by its offset in
    /// shares of the box. The view spans -0.8 to 0.8 of the shape's box either way, so that
    /// nothing tilted leaves it.
    pub fn view(set: &MeshSet, parts: &[(Shape, f32, [f32; 3])], pitch: f32) -> Vec<Option<Material>> {
        let (sin, cos) = pitch.to_radians().sin_cos();
        let mut depth = vec![f32::MAX; VIEW_SIDE * VIEW_SIDE];
        let mut seen = vec![None; VIEW_SIDE * VIEW_SIDE];
        let span = 0.8_f32;
        for &(shape, turn, [dx, dy, dz]) in parts {
            let (turn_sin, turn_cos) = turn.sin_cos();
            let range = set.ranges[shape as usize].clone();
            for triangle in set.indices[range.start as usize..range.end as usize].chunks(3) {
                let corners: Vec<(f32, f32, f32)> = triangle
                    .iter()
                    .map(|&index| {
                        let [x, y, z] = set.vertices[index as usize].position;
                        let (x, y) = (x * turn_cos - y * turn_sin + dx, x * turn_sin + y * turn_cos + dy);
                        let z = z + dz;
                        // Screen right is +X, screen up is north and up tilted, depth runs away from the eye.
                        let (up, away) = (y * sin + z * cos, y * cos - z * sin);
                        (((x + span) / (2.0 * span)) * VIEW_SIDE as f32, ((span - up) / (2.0 * span)) * VIEW_SIDE as f32, away)
                    })
                    .collect();
                let material = Material::ALL[(set.vertices[triangle[0] as usize].material & motion::MATERIAL_MASK) as usize];
                let (a, b, c) = (corners[0], corners[1], corners[2]);
                let area = (b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1);
                if area.abs() < 1e-9 {
                    continue;
                }
                let (left, right) = (a.0.min(b.0).min(c.0).floor().max(0.0) as usize, (a.0.max(b.0).max(c.0).ceil() as usize).min(VIEW_SIDE - 1));
                let (top, bottom) = (a.1.min(b.1).min(c.1).floor().max(0.0) as usize, (a.1.max(b.1).max(c.1).ceil() as usize).min(VIEW_SIDE - 1));
                for py in top..=bottom {
                    for px in left..=right {
                        let (x, y) = (px as f32 + 0.5, py as f32 + 0.5);
                        let w0 = ((b.0 - x) * (c.1 - y) - (c.0 - x) * (b.1 - y)) / area;
                        let w1 = ((c.0 - x) * (a.1 - y) - (a.0 - x) * (c.1 - y)) / area;
                        let w2 = 1.0 - w0 - w1;
                        if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                            continue;
                        }
                        let away = w0 * a.2 + w1 * b.2 + w2 * c.2;
                        let at = py * VIEW_SIDE + px;
                        if away < depth[at] {
                            depth[at] = away;
                            seen[at] = Some(material);
                        }
                    }
                }
            }
        }
        seen
    }

    /// The most triangles one shape may have, so that a crowd of units stays cheap to draw. Raised
    /// from 1,200 on 2026-11-23, when round parts were given more sides.
    const TRIANGLE_BUDGET: u32 = 1_600;

    #[test]
    fn every_shape_keeps_to_its_triangle_budget() {
        let set = MeshSet::build();
        for shape in Shape::ALL {
            let triangles = (set.ranges[shape as usize].end - set.ranges[shape as usize].start) / 3;
            assert!(triangles <= TRIANGLE_BUDGET, "{shape:?} has {triangles} triangles, over {TRIANGLE_BUDGET}");
        }
    }

    #[test]
    fn every_vertex_has_a_known_material() {
        let set = MeshSet::build();
        assert!(set.vertices.iter().all(|vertex| vertex.material & motion::MATERIAL_MASK <= Material::Glass as u32));
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
            // Legs swing as a unit walks; anything else that moves, spinning or rocking, does so
            // at its spin rate, and nothing moves without one or the other to drive it.
            let swings = shape_vertices(&set, index).iter().any(|vertex| vertex.material & motion::SWING != 0);
            let spins = shape_vertices(&set, index).iter().any(|vertex| vertex.material & motion::SPIN != 0);
            if walking.contains(&shape) {
                assert!(swings, "{shape:?} walks, so its legs swing");
            } else {
                assert_eq!(spins || swings, shape.spin_rate().is_some(), "{shape:?} moves only if it has a spin rate");
            }
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

    fn far_vertices(set: &MeshSet, shape: usize) -> Vec<MeshVertex> {
        set.indices[set.far_ranges[shape].start as usize..set.far_ranges[shape].end as usize].iter().map(|&index| set.vertices[index as usize]).collect()
    }

    /// A far model is there to save work, so each is a small fraction of its shape, and the
    /// shapes that fill armies and bases all have one.
    #[test]
    fn far_models_are_a_fraction_of_their_shapes() {
        let set = MeshSet::build();
        assert_eq!(set.far_ranges.len(), Shape::COUNT);
        let most = (FAR_CELLS * FAR_CELLS * 10 + 2) as u32;
        let mut with_one = 0;
        for (index, shape) in Shape::ALL.iter().enumerate() {
            let (full, far) = (&set.ranges[index], &set.far_ranges[index]);
            if far == full {
                assert!((full.end - full.start) / 3 < most * FAR_SAVING, "{shape:?} has {} triangles and no far model", (full.end - full.start) / 3);
                continue;
            }
            with_one += 1;
            let triangles = (far.end - far.start) / 3;
            assert!(triangles <= most, "{shape:?}'s far model has {triangles} triangles");
            assert!(triangles * FAR_SAVING <= (full.end - full.start) / 3, "{shape:?}'s far model saves too little");
        }
        assert!(with_one * 2 > Shape::COUNT, "only {with_one} shapes have far models");
        for shape in [Shape::Tank, Shape::TankHull, Shape::Factory, Shape::Battleship, Shape::Gunship, Shape::Commander] {
            assert_ne!(set.far_ranges[shape as usize], set.ranges[shape as usize], "{shape:?} should have a far model");
        }
    }

    /// A far model stands where its shape does: inside the shape's own bounds, reaching most of
    /// the way across them, so that a unit keeps its place, size, and proportions as it shrinks.
    #[test]
    fn a_far_model_keeps_its_shapes_place_and_size() {
        let set = MeshSet::build();
        for (index, shape) in Shape::ALL.iter().enumerate() {
            if set.far_ranges[index] == set.ranges[index] {
                continue;
            }
            let bounds = |vertices: &[MeshVertex]| {
                let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                for vertex in vertices.iter().filter(|vertex| vertex.material & motion::FLASH == 0) {
                    low = low.min(Vec3::from(vertex.position));
                    high = high.max(Vec3::from(vertex.position));
                }
                (low, high)
            };
            let (low, high) = bounds(&shape_vertices(&set, index));
            let (far_low, far_high) = bounds(&far_vertices(&set, index));
            for axis in 0..3 {
                assert!(far_low[axis] >= low[axis] - 1e-4 && far_high[axis] <= high[axis] + 1e-4, "{shape:?}'s far model leaves its shape's bounds on axis {axis}");
            }
            // Seen from above it covers at least a third of the shape's width and length.
            for axis in 0..2 {
                assert!(far_high[axis] - far_low[axis] >= (high[axis] - low[axis]) / 3.0 - 1e-4, "{shape:?}'s far model is too small on axis {axis}");
            }
        }
    }

    /// A far model shows its shape's colors: where the shape shows team color from above, so
    /// does its far model, and its sides are the body's color.
    #[test]
    fn a_far_model_shows_its_shapes_colors() {
        let set = MeshSet::build();
        for (index, shape) in Shape::ALL.iter().enumerate() {
            if set.far_ranges[index] == set.ranges[index] {
                continue;
            }
            let far = far_vertices(&set, index);
            let kinds = |vertices: &[MeshVertex]| {
                let mut seen = [false; 6];
                for vertex in vertices.iter().filter(|vertex| vertex.normal[2] > 0.5 && vertex.material & motion::FLASH == 0) {
                    seen[(vertex.material & motion::MATERIAL_MASK) as usize] = true;
                }
                seen
            };
            let (own, shown) = (kinds(&shape_vertices(&set, index)), kinds(&far));
            for material in 0..6 {
                // (Team color is always there, in the speck that grows into the dot.)
                assert!(!shown[material] || own[material] || material == Material::Trim as usize, "{shape:?}'s far model shows a material its shape does not show from above: {material}");
            }
            assert!(far.iter().filter(|vertex| vertex.normal[2].abs() < 0.5).all(|vertex| vertex.material & motion::MATERIAL_MASK == Material::Hull as u32), "{shape:?}'s far model's sides are the body's color");
            assert!(far.iter().all(|vertex| vertex.material & (motion::SPIN | motion::SWING | motion::RECOIL | motion::FLASH) == 0), "{shape:?}'s far model has nothing that moves");
        }
    }

    /// A far model turns into the same dot its shape does: it has a corner in every quarter, and
    /// one face whose corners are in all four, which grows into the whole square.
    #[test]
    fn every_far_model_fills_a_dot() {
        let set = MeshSet::build();
        for (index, shape) in Shape::ALL.iter().enumerate() {
            if set.far_ranges[index] == set.ranges[index] {
                continue;
            }
            let range = set.far_ranges[index].clone();
            let indices = &set.indices[range.start as usize..range.end as usize];
            // The last face added is the speck: two triangles over the four corners.
            let speck: Vec<[f32; 3]> = indices[indices.len() - 6..].iter().map(|&vertex| set.vertices[vertex as usize].position).collect();
            for (x_sign, y_sign) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
                assert!(speck.iter().any(|corner| corner[0] * x_sign > 0.0 && corner[1] * y_sign > 0.0), "{shape:?}'s far model has no corner in quarter ({x_sign}, {y_sign})");
            }
            assert!(speck.iter().all(|corner| corner[0].abs() <= FAR_DOT_SPECK && corner[1].abs() <= FAR_DOT_SPECK), "{shape:?}'s speck is too small to see");
        }
    }

    /// How a far model is worked out, on a shape simple enough to check by hand: one box for
    /// each cell the shape covers, as high as the shape is there, topped with its color there.
    #[test]
    fn a_far_model_is_one_box_for_each_cell_its_shape_covers() {
        let mut mesh = MeshBuilder::default();
        // An L: a long bar along the left, and a foot at the front right.
        mesh.paint(Material::Hull);
        mesh.block([-0.5, -0.5, -0.5], [-0.2, 0.5, 0.0]);
        mesh.paint(Material::Trim);
        mesh.block([-0.2, 0.2, -0.5], [0.5, 0.5, -0.2]);
        let model = far_model(&mesh.vertices, &mesh.indices, 3);
        // The bar fills the left column of three cells, and the foot the other two at the front.
        assert_eq!(model.boxes.len(), 5, "{:?}", model.boxes.iter().map(|part| (part.min, part.max)).collect::<Vec<_>>());
        let at = |x: f32, y: f32| model.boxes.iter().find(|part| part.min[0] < x && x < part.max[0] && part.min[1] < y && y < part.max[1]);
        let bar = at(-0.4, 0.0).expect("the bar");
        assert!((bar.max[2] - 0.0).abs() < 1e-3 && bar.top == Material::Hull as u32, "as high as the bar, and its color");
        let foot = at(0.3, 0.4).expect("the foot");
        assert!((foot.max[2] + 0.2).abs() < 1e-3 && foot.top == Material::Trim as u32, "as high as the foot, and its color");
        assert!(at(0.3, -0.3).is_none(), "nothing where the shape is not");
        assert_eq!(model.triangles(), 52);
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
