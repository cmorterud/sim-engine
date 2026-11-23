//! The in-game interface: resource bar, build menu, unit overlays, and the end of match screen.
//! It reads snapshots and returns actions. It never touches the simulation.
#![allow(clippy::disallowed_types)]

use std::collections::{BTreeMap, BTreeSet};

use egui::{Align2, Color32, FontId, Pos2, Rect, Rounding, Stroke, Vec2};

use crate::hud::{self, Glyph};
use crate::icons::{self, Icon};
use crate::input::feedback::{Marker, MarkerKind, OrderFeedback, LABEL_SECONDS};
use crate::input::hotkeys::{self, blueprint_hotkey};
use crate::input::orders::PendingBuild;
use crate::math::fp::Fp;
use crate::render::camera::Camera;
use crate::sim::blueprints::{BlueprintDb, BlueprintId, Enhancement, EnhancementId, FactoryKind, Res, Slot, Targets, UnitClass, WeaponKind, TICKS_PER_SECOND};
use crate::sim::components::{BuildTask, OrderKind, SimId, Team};
use crate::sim::world::{EconomyView, MatchResult, PlanStepKind, SimSnapshot, UnitRatesView, UnitView};

/// Trails are capped so that a large selection stays readable and cheap to draw.
const MAX_TRAIL_UNITS: usize = 64;
const ALL_ORDERS_UNIT_LIMIT: usize = 256;
const TRAIL_OPACITY: f32 = 0.85;
/// Rally lines are dimmer: they are a standing setting, not something under way.
const RALLY_OPACITY: f32 = 0.5;
const DASH_POINTS: f32 = 8.0;
const GAP_POINTS: f32 = 5.0;
/// Below this on-screen size, a terrain cell is too small for a placement grid to help.
const MIN_GRID_CELL_POINTS: f32 = 5.0;
/// Below this on-screen radius, a selected unit's ground ring is too small to see.
const MIN_SELECTION_RING_POINTS: f32 = 4.0;
/// The placement grid reaches this many cells from the cursor in each direction.
const GRID_REACH_CELLS: i32 = 8;

/// Above this altitude, units are drawn with tactical icons.
use crate::icons::ICON_ALTITUDE;
const BLUE: Color32 = Color32::from_rgb(90, 160, 255);
const RED: Color32 = Color32::from_rgb(255, 95, 80);
const STALLED: Color32 = Color32::from_rgb(255, 110, 90);
const HEALTHY: Color32 = Color32::from_rgb(120, 230, 120);
/// A factory with nothing to do: red, hatched with paler stripes, and edged paler still.
const IDLE: Color32 = Color32::from_rgb(184, 58, 38);
const IDLE_STRIPE: Color32 = Color32::from_rgb(232, 140, 122);
const IDLE_EDGE: Color32 = Color32::from_rgb(255, 209, 200);
const BUILDING: Color32 = Color32::from_rgb(120, 200, 255);
const SPENDING: Color32 = Color32::from_rgb(250, 190, 120);
const VETERAN: Color32 = Color32::from_rgb(255, 205, 60);
/// The ring and bar shown on anything upgrading.
const UPGRADING: Color32 = Color32::from_rgb(200, 130, 255);
/// The mark on a stunned unit, and the color of tractor beams and storms.
const STUNNED: Color32 = Color32::from_rgb(140, 210, 255);
const TRACTOR: Color32 = Color32::from_rgb(120, 255, 200);
const STORM: Color32 = Color32::from_rgb(170, 110, 255);
/// How fast the ring around an upgrading unit pulses, in radians a second.
const UPGRADE_PULSE_RATE: f64 = 4.0;
/// How close to an order's icon a Shift-press grabs it, in points.
const HANDLE_GRAB_POINTS: f64 = 18.0;
const HANDLE_RADIUS_POINTS: f32 = 10.0;
/// Matches how double-clicking a unit works.
const QUICK_DOUBLE_CLICK_SECONDS: f64 = 0.4;
/// How many units a factory queues at once with Shift held.
const SHIFT_QUEUE_COUNT: u32 = 5;

/// An order waiting for the player to click where it should go.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ArmedOrder {
    Move,
    AttackMove,
    Formation,
    AttackGround,
    Patrol,
    Assist,
    Discharge,
    /// Not on a letter: Ctrl+T, and only for units with a teleporter.
    Teleport,
    /// Not on a letter: Ctrl+C, for builders.
    Capture,
    /// Not on a letter: Ctrl+U, for transports with something on board.
    Unload,
}

impl ArmedOrder {
    const ALL: [Self; 7] = [Self::Move, Self::AttackMove, Self::Formation, Self::AttackGround, Self::Patrol, Self::Assist, Self::Discharge];

    const fn letter(self) -> char {
        match self {
            Self::Move => hotkeys::MOVE,
            Self::AttackMove => hotkeys::ATTACK_MOVE,
            Self::Formation => hotkeys::FORMATION,
            Self::AttackGround => hotkeys::ATTACK_GROUND,
            Self::Patrol => hotkeys::PATROL,
            Self::Assist => hotkeys::ASSIST,
            Self::Discharge => hotkeys::DISCHARGE,
            Self::Teleport => 'T',
            Self::Capture => 'C',
            Self::Unload => 'U',
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Move => "Move",
            Self::AttackMove => "Attack-move",
            Self::Formation => "Formation move",
            Self::AttackGround => "Attack ground",
            Self::Patrol => "Patrol",
            Self::Assist => "Assist",
            Self::Discharge => "Discharge",
            Self::Teleport => "Teleport",
            Self::Capture => "Capture",
            Self::Unload => "Unload",
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::Move => "Walk there, ignoring what is under the cursor. Units still fire at what comes into range.",
            Self::AttackMove => "Walk there, stopping to fire when enemies are in range.",
            Self::Formation => "Travel together at the pace of the slowest unit, keeping the formation all the way. Hold Alt to attack-move, which stops the whole group to fight. Also Ctrl+right-click.",
            Self::AttackGround => "Keep firing at a spot, even out of sight, until given another order. Shells that land on debris destroy it, so the enemy cannot reclaim it.",
            Self::Patrol => "Repeat a route. Combat units fight along it, and engineers also reclaim and repair.",
            Self::Assist => "Stay with a friendly unit and protect it. Builders also help it build.",
            Self::Discharge => "Needs 7,500 energy in store. Spends what destroys the toughest enemy where it lands, 6 energy a point of damage, up to 90% of the store and 15,000 damage. Does 400 to commanders and 800 to structures.",
            Self::Teleport => "Charge up, standing still, and jump to a spot you can see or have on radar. Costs a great deal of energy, more the farther it goes, and aborts if the energy runs out. Lands on enemy units destroy them.",
            Self::Capture => "Take over an enemy unit or structure, in half its build time at your builders' build power, for its energy cost. Stopping starts it over. Commanders cannot be captured.",
            Self::Unload => "Fly there and set down everything on board around the spot. To load, select land units and right-click the transport.",
        }
    }

    /// What to do next, shown while the order is armed.
    pub const fn hint(self) -> &'static str {
        match self {
            Self::Move => "Move: left-click a destination. Right-click or Esc to cancel.",
            Self::AttackMove => "Attack-move: left-click a destination. Right-click or Esc to cancel.",
            Self::Formation => "Formation move: left-click a destination, holding Alt to attack-move. Right-click or Esc to cancel.",
            Self::AttackGround => "Attack ground: left-click a spot or some debris. Right-click or Esc to cancel.",
            Self::Patrol => "Patrol: left-click to patrol there. Shift-click to queue it, and again to add points to the loop. Right-click or Esc to cancel.",
            Self::Assist => "Assist: left-click a friendly unit. Right-click or Esc to cancel.",
            Self::Discharge => "Discharge: left-click an enemy. Right-click or Esc to cancel.",
            Self::Teleport => "Teleport: left-click open ground you can see or have on radar. Right-click or Esc to cancel.",
            Self::Capture => "Capture: left-click an enemy unit or structure. Right-click or Esc to cancel.",
            Self::Unload => "Unload: left-click where to set the cargo down. Right-click or Esc to cancel.",
        }
    }
}

/// What the player asked for through the interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiAction {
    BeginPlacement(BlueprintId),
    /// Queue `count` of an item in each of these factories, out of `selected_count` selected.
    Queue { factories: Vec<SimId>, blueprint: BlueprintId, selected_count: usize, count: u32 },
    SetRepeat { factories: Vec<SimId>, repeat: bool },
    ClearQueue { factories: Vec<SimId> },
    /// The next click on the map gives this order.
    Arm(ArmedOrder),
    Stop,
    /// Pause or resume these factories and construction sites.
    SetPaused { units: Vec<SimId>, paused: bool },
    /// Stop or restart the game clock.
    TogglePause,
    /// Move the camera to look at this world point, in whole units, keeping its height.
    CenterCamera([i32; 2]),
    /// Order the selection to this world point, in whole units, as a right-click on the ground
    /// there would, with the same keys for queuing, attack-moving, and formation.
    MinimapOrder([i32; 2]),
    /// From the quick select bar: select these, or add them to the selection.
    QuickSelect { units: Vec<SimId>, add: bool },
    /// Remove one order from these builders' queues.
    CancelTask { units: Vec<SimId>, task: BuildTask },
    /// Remove one of an item from each of these factories' queues.
    RemoveQueued { factories: Vec<SimId>, blueprint: BlueprintId },
    /// Upgrade these commanders or structures in place.
    Upgrade { units: Vec<SimId> },
    /// Switch these units' cloaks on or off.
    SetCloak { units: Vec<SimId>, on: bool },
    /// Bring the selected submarines up, or take them down.
    SetSurfaced { units: Vec<SimId>, surfaced: bool },
    /// Start a field enhancement on these units.
    Enhance { units: Vec<SimId>, enhancement: EnhancementId },
    /// Several at once, from one key that several options share.
    Batch(Vec<UiAction>),
    Restart,
    /// Watch the match from everyone's sight (`None`) or one team's.
    SetView(Option<Team>),
    /// Play a watched match at this many ticks for each tick of real time.
    SetSpeed(u32),
    /// Watch the match for this seed instead.
    WatchSeed(u64),
}

fn find_unit(snapshot: &SimSnapshot, id: SimId) -> Option<&UnitView> {
    snapshot.units.binary_search_by_key(&id, |unit| unit.id).ok().map(|index| &snapshot.units[index])
}

/// The bold weight of the interface's font, for hotkeys, so that they stand out from the words.
const BOLD: &str = "bold";

/// Adds the bold font to the interface. Call once, before the first frame.
pub fn install_fonts(context: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(BOLD.to_owned(), egui::FontData::from_static(include_bytes!("../assets/fonts/Ubuntu-Bold.ttf")));
    fonts.families.insert(egui::FontFamily::Name(BOLD.into()), vec![BOLD.to_owned()]);
    context.set_fonts(fonts);
}

/// How long after the pointer stops moving a button's popup shows, in seconds. egui waits half
/// a second, and first for the pointer to settle, which is slow for a build bar that players
/// read by sweeping the pointer across it. egui counts the pointer as moving for a moment after
/// it stops, so popups show about a sixth of a second after it does.
pub const TOOLTIP_DELAY: f32 = 0.05;

/// After a popup closes, how long the popups of other buttons show without the delay, so that
/// moving along the bar shows each in turn.
pub const TOOLTIP_GRACE: f32 = 0.5;

/// Sets the interface's popup timing, on top of egui's style.
pub fn install_style(context: &egui::Context) {
    context.style_mut(|style| {
        style.interaction.tooltip_delay = TOOLTIP_DELAY;
        style.interaction.tooltip_grace_time = TOOLTIP_GRACE;
        style.interaction.show_tooltips_only_when_still = false;
        crate::hud::apply_visuals(style);
    });
}

/// A label with each hotkey in it, written in brackets like `[M]`, in bold.
pub fn bold_keys(text: &str) -> egui::WidgetText {
    let size = 14.0;
    let format = |bold: bool| egui::TextFormat {
        font_id: if bold { FontId::new(size, egui::FontFamily::Name(BOLD.into())) } else { FontId::proportional(size) },
        // Takes the widget's own text color, as a plain label would.
        color: Color32::PLACEHOLDER,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    let mut rest = text;
    while let Some(start) = rest.find('[') {
        let Some(length) = rest[start..].find(']').filter(|&length| (2..=12).contains(&length)) else {
            break;
        };
        job.append(&rest[..start], 0.0, format(false));
        job.append(&rest[start..=start + length], 0.0, format(true));
        rest = &rest[start + length + 1..];
    }
    job.append(rest, 0.0, format(false));
    job.into()
}

/// A build menu button: the item's name, with its key in front.
fn item_label(db: &BlueprintDb, blueprint: BlueprintId) -> String {
    let unit = db.get(blueprint);
    let key = blueprint_hotkey(&unit.key).map_or(String::new(), |letter| format!("[{}] ", hotkeys::key_name(letter)));
    format!("{key}{}", unit.name)
}

/// What hovering over a build menu item shows: its cost, and what it does.
pub fn item_details(db: &BlueprintDb, blueprint: BlueprintId) -> String {
    let unit = db.get(blueprint);
    let whole = |value: Fp| value.to_num::<f64>().round() as i64;
    let per_second = |per_tick: Res| per_tick.to_num::<f64>() * TICKS_PER_SECOND as f64;
    let kind = if unit.is_structure() { format!("T{} structure, {} by {}", unit.tier, unit.footprint, unit.footprint) } else { format!("T{} unit", unit.tier) };
    let mut lines = vec![
        format!("{} ({kind})", unit.name),
        format!("Costs {} materiel and {} energy, build time {}", unit.mass.to_num::<i64>(), unit.energy.to_num::<i64>(), unit.build_time.to_num::<i64>()),
        format!("Health {}", whole(unit.health)),
    ];
    if let Some(weapon) = unit.weapon {
        let seconds = f64::from(weapon.reload_ticks) / TICKS_PER_SECOND as f64;
        let aims = match (weapon.kind, weapon.targets) {
            (WeaponKind::Bomb, _) => ", bombs ground targets",
            (WeaponKind::Flak, _) => ", flak at aircraft only",
            (_, Targets::Air) => ", at aircraft only",
            (_, Targets::Both) => ", at aircraft and ground",
            (WeaponKind::TacticalMissile | WeaponKind::StrategicMissile, _) => ", fires on order",
            _ => "",
        };
        let splash = if weapon.splash_radius > Fp::ZERO { format!(", within {}", whole(weapon.splash_radius)) } else { String::new() };
        lines.push(format!("Weapon: {} damage every {seconds:.1} s{splash}, range {}{aims}", whole(weapon.damage), whole(weapon.range)));
    }
    if unit.speed > Fp::ZERO {
        let flies = unit.flight.map_or(String::new(), |flight| format!(", flies at {}", whole(flight.elevation)));
        lines.push(format!("Speed {} a second{flies}", whole(unit.speed * Fp::from_num(TICKS_PER_SECOND))));
    }
    let mut makes = Vec::new();
    if unit.mass_income > Res::ZERO {
        makes.push(format!("{:.1} materiel", per_second(unit.mass_income)));
    }
    if unit.energy_income > Res::ZERO {
        makes.push(format!("{:.0} energy", per_second(unit.energy_income)));
    }
    if !makes.is_empty() {
        lines.push(format!("Makes {} a second", makes.join(" and ")));
    }
    if unit.upkeep > Res::ZERO {
        lines.push(format!("Uses {:.0} energy a second while running", per_second(unit.upkeep)));
    }
    if unit.mass_storage > Res::ZERO || unit.energy_storage > Res::ZERO {
        lines.push(format!("Stores {} materiel and {} energy", unit.mass_storage.to_num::<i64>(), unit.energy_storage.to_num::<i64>()));
    }
    if let Some(shield) = unit.shield {
        lines.push(format!("Shield: {} health, radius {}", whole(shield.health), whole(shield.radius)));
    }
    let mut sensors = vec![format!("vision {}", whole(unit.vision))];
    if unit.radar > Fp::ZERO {
        sensors.push(format!("radar {}", whole(unit.radar)));
    }
    if unit.deep_scan > Fp::ZERO {
        sensors.push(format!("deep scan {}", whole(unit.deep_scan)));
    }
    if unit.stealth_field > Fp::ZERO {
        sensors.push(format!("stealth field {}", whole(unit.stealth_field)));
    }
    lines.push(format!("Sees: {}", sensors.join(", ")));
    if unit.build_power > Fp::ZERO && !unit.builds.is_empty() {
        lines.push(format!("Build power {}", whole(unit.build_power)));
    }
    if unit.requires_deposit {
        lines.push("Built on a materiel deposit".to_string());
    }
    if unit.requires_vent {
        lines.push("Built over a geothermal vent".to_string());
    }
    lines.join("\n")
}

/// The tech levels a build menu can show.
const MENU_TIERS: [u8; 3] = [1, 2, 3];

/// Where the tech level each kind of build bar shows is remembered.
const BUILDER_TIER_MEMORY: &str = "builder_menu_tier";
const FACTORY_TIER_MEMORY: &str = "factory_menu_tier";

/// The tech level last chosen for each kind of build bar, if any.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct BuildTiers {
    pub builder: Option<u8>,
    pub factory: Option<u8>,
}

pub fn build_tiers(ctx: &egui::Context) -> BuildTiers {
    ctx.data(|data| BuildTiers { builder: data.get_temp(egui::Id::new(BUILDER_TIER_MEMORY)), factory: data.get_temp(egui::Id::new(FACTORY_TIER_MEMORY)) })
}

/// The level after the one shown, among those a bar has items of, going round to the first.
pub fn next_tier(chosen: Option<u8>, options: &[BlueprintId], db: &BlueprintDb) -> Option<u8> {
    let shown = shown_tier(chosen, options, db)?;
    let has = |tier: u8| options.iter().any(|option| db.get(*option).tier == tier);
    MENU_TIERS.into_iter().filter(|tier| *tier > shown).find(|tier| has(*tier)).or_else(|| MENU_TIERS.into_iter().find(|tier| has(*tier)))
}

/// Shows the next tech level in the selection's build bars, for the key that does so.
pub fn cycle_build_tiers(ctx: &egui::Context, snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) {
    let tiers = build_tiers(ctx);
    let builder = builder_options(snapshot, selected, db);
    let factory: Vec<BlueprintId> = factory_menu(snapshot, selected, db).map_or_else(Vec::new, |menu| menu.options.iter().map(|(option, _)| *option).collect());
    ctx.data_mut(|data| {
        if let Some(tier) = next_tier(tiers.builder, &builder, db) {
            data.insert_temp(egui::Id::new(BUILDER_TIER_MEMORY), tier);
        }
        if let Some(tier) = next_tier(tiers.factory, &factory, db) {
            data.insert_temp(egui::Id::new(FACTORY_TIER_MEMORY), tier);
        }
    });
}

/// The items a bar shows at a level: by group (factories, then resources, defenses, intel, and
/// strategic weapons), and within a group in the order of their slots, which give them their keys.
pub fn bar_items(options: &[BlueprintId], tier: u8, db: &BlueprintDb) -> Vec<BlueprintId> {
    let mut items: Vec<BlueprintId> = options.iter().copied().filter(|option| db.get(*option).tier == tier).collect();
    items.sort_by_key(|option| {
        let key = &db.get(*option).key;
        hotkeys::bar_order(key).unwrap_or((usize::MAX, usize::MAX))
    });
    items
}

/// The tech level a build menu shows: the one last chosen, if the menu has something of it,
/// or else the highest it has anything of, the best the selection can build.
pub fn shown_tier(chosen: Option<u8>, options: &[BlueprintId], db: &BlueprintDb) -> Option<u8> {
    let has = |tier: u8| options.iter().any(|option| db.get(*option).tier == tier);
    chosen.filter(|tier| has(*tier)).or_else(|| MENU_TIERS.into_iter().rev().find(|tier| has(*tier)))
}

/// Forgets the tech levels chosen in the build bars, so that a new selection opens them at the
/// highest level it can build.
pub fn reset_build_tiers(ctx: &egui::Context) {
    ctx.data_mut(|data| {
        data.remove::<u8>(egui::Id::new(BUILDER_TIER_MEMORY));
        data.remove::<u8>(egui::Id::new(FACTORY_TIER_MEMORY));
        data.remove::<CommandTab>(egui::Id::new(COMMAND_TAB_MEMORY));
    });
}

/// What the command bar's main area shows, picked with the tabs at its left.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CommandTab {
    /// What the selection can build.
    Build,
    /// What the selection is made of, by kind.
    Selection,
    /// A commander's enhancements.
    Enhance,
}

const COMMAND_TAB_MEMORY: &str = "command tab";

/// The tabs a selection has, in order: building if it can build, its make-up always, and
/// enhancements if it can take any.
pub fn command_tabs(can_build: bool, can_enhance: bool) -> Vec<CommandTab> {
    let mut tabs = Vec::new();
    if can_build {
        tabs.push(CommandTab::Build);
    }
    tabs.push(CommandTab::Selection);
    if can_enhance {
        tabs.push(CommandTab::Enhance);
    }
    tabs
}

/// The tab shown: the one last picked if this selection has it, else its first.
pub fn shown_tab(chosen: Option<CommandTab>, tabs: &[CommandTab]) -> CommandTab {
    chosen.filter(|tab| tabs.contains(tab)).unwrap_or(tabs[0])
}

/// A unit's picture on a button: its portrait over a backdrop of where it can go (grass for land,
/// a shore for amphibious, sky for air, waves for ships, depths for submarines), with its map
/// symbol small in the top left corner in its role's color on a dark disc, or the symbol alone,
/// large, if there is no portrait.
fn unit_art(painter: &egui::Painter, rect: Rect, db: &BlueprintDb, key: &str) {
    let icon = Icon::for_key(key);
    if let Some(id) = db.find(key) {
        let backdrop = rect.shrink(0.5);
        let clipped = painter.with_clip_rect(backdrop.intersect(painter.clip_rect()));
        for shape in icons::ground_shapes(backdrop, icons::Ground::of(db.get(id))) {
            clipped.add(shape);
        }
    }
    if hud::draw_portrait(painter, rect, key) {
        let half = (rect.height() * 0.1).clamp(3.5, 6.0);
        let center = rect.left_top() + Vec2::new(half + 3.0, half + 3.0);
        painter.circle_filled(center, half + 2.5, Color32::from_black_alpha(190));
        icons::draw(painter, center, half, icon, 0, icon.role().color());
    } else {
        icons::draw_bar(painter, rect, icon);
    }
}

/// The size of a build bar's buttons, in points.
const BAR_BUTTON: f32 = 46.0;

/// A build bar's button: the item's symbol, colored by its role, its key in the top left corner,
/// and `suffix`, such as "auto", in the bottom right.
fn icon_button(ui: &mut egui::Ui, db: &BlueprintDb, option: BlueprintId, suffix: &str) -> egui::Response {
    let unit = db.get(option);
    let suffix = suffix.trim().trim_matches(|c| c == '(' || c == ')').to_string();
    hud::square_button(ui, Vec2::splat(BAR_BUTTON), false, true, |painter, rect| {
        // The bar's toggle says the level, so there are no tech dots.
        unit_art(painter, rect, db, &unit.key);
        if let Some(letter) = blueprint_hotkey(&unit.key) {
            let font = FontId::new(14.0, egui::FontFamily::Name(BOLD.into()));
            hud::outlined_text(painter, rect.right_bottom() + Vec2::new(-2.0, 0.0), Align2::RIGHT_BOTTOM, &hotkeys::key_name(letter), font, Color32::WHITE);
        }
        if !suffix.is_empty() {
            hud::outlined_text(painter, rect.left_bottom() + Vec2::new(2.0, 0.0), Align2::LEFT_BOTTOM, &suffix, FontId::proportional(10.0), Color32::YELLOW);
        }
    })
}

/// The tech level toggles at the start of a build bar, stacked, filling its height.
fn tier_toggles(ui: &mut egui::Ui, id: egui::Id, options: &[BlueprintId], shown: u8, db: &BlueprintDb) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for tier in MENU_TIERS {
            let available = options.iter().any(|option| db.get(*option).tier == tier);
            let toggle = hud::square_button(ui, Vec2::new(28.0, (BAR_BUTTON - 4.0) / 3.0), tier == shown, available, |painter, rect| {
                let color = if tier == shown { Color32::WHITE } else { hud::DIM_TEXT };
                painter.text(rect.center(), Align2::CENTER_CENTER, format!("T{tier}"), FontId::new(11.0, egui::FontFamily::Name(BOLD.into())), color);
            });
            if toggle.on_hover_text(format!("Show what can be built at tech level {tier}. [{}] shows the next level.", hotkeys::CYCLE_TIER)).clicked() {
                ui.ctx().data_mut(|data| data.insert_temp(id, tier));
            }
        }
    });
}

/// One build menu, as a single bar: a toggle for the tech level, then the items of that level as
/// icons, grouped with dividers between, each with its key, and its name and details on hover.
/// The level chosen is remembered under `memory`. Returns the item clicked.
///
/// What was clicked on a build bar: an item, with the left button or the right.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum BarClick {
    Left(BlueprintId),
    Right(BlueprintId),
}

fn build_bar(
    ui: &mut egui::Ui,
    memory: &str,
    options: &[BlueprintId],
    db: &BlueprintDb,
    extra_hint: impl Fn(BlueprintId) -> String,
    label_suffix: impl Fn(BlueprintId) -> String,
) -> Option<BarClick> {
    let id = egui::Id::new(memory);
    let chosen = ui.ctx().data(|data| data.get_temp::<u8>(id));
    let shown = shown_tier(chosen, options, db)?;
    let mut clicked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        tier_toggles(ui, id, options, shown, db);
        egui::ScrollArea::horizontal().id_source(memory).show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut last_group = None;
                for option in bar_items(options, shown, db) {
                    // A divider between families, which share a key.
                    let group = hotkeys::bar_order(&db.get(option).key).map(|(family, _)| family);
                    if last_group.is_some_and(|last| last != group) {
                        ui.separator();
                    }
                    last_group = Some(group);
                    let extra = extra_hint(option);
                    // The name, after the key, heads the details, since the button shows neither in words.
                    let details = item_details(db, option).replacen(&db.get(option).name, &item_label(db, option), 1);
                    let hint = if extra.is_empty() { details } else { format!("{details}\n\n{extra}") };
                    let button = icon_button(ui, db, option, &label_suffix(option)).on_hover_text(hint);
                    if button.clicked() {
                        clicked = Some(BarClick::Left(option));
                    } else if button.secondary_clicked() {
                        clicked = Some(BarClick::Right(option));
                    }
                }
            });
        });
    });
    clicked
}

/// Whether anything selected can walk, and so take the unit orders.
pub fn has_mobile_units(snapshot: &SimSnapshot, selected: &[SimId]) -> bool {
    selected.iter().filter_map(|id| find_unit(snapshot, *id)).any(|unit| !unit.is_structure)
}

/// Whether anything selected is a builder that can walk, and so can capture.
pub fn has_mobile_builders(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> bool {
    selected.iter().filter_map(|id| find_unit(snapshot, *id)).any(|unit| {
        unit.blueprint != u16::MAX && !unit.is_structure && db.get(BlueprintId(unit.blueprint)).build_power > crate::math::fp::Fp::ZERO
    })
}

/// Whether anything selected has Discharge.
pub fn has_discharge(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> bool {
    selected
        .iter()
        .filter_map(|id| find_unit(snapshot, *id))
        .any(|unit| unit.blueprint != u16::MAX && db.get(BlueprintId(unit.blueprint)).discharge.is_some())
}

/// A shield generator's dome, in words: how strong it is, or how long until it is back.
pub fn shield_status(snapshot: &SimSnapshot, id: SimId) -> Option<String> {
    let shield = snapshot.shields.iter().find(|shield| shield.id == id)?;
    Some(if shield.up {
        format!("shield {}%", shield.health_permille / 10)
    } else if shield.recharging > 0 {
        format!("shield down, back in {} s", shield.recharging.div_ceil(10))
    } else {
        "shield off".to_string()
    })
}

/// Whether an intel structure is short of energy, or how a cloak is doing.
pub fn intel_status(snapshot: &SimSnapshot, id: SimId) -> Option<String> {
    if snapshot.unpowered.contains(&id) {
        return Some("off: no energy".to_string());
    }
    snapshot.cloaks.iter().find(|(unit, _, _)| *unit == id).and_then(|&(_, on, cloaked)| match (on, cloaked) {
        (true, true) => Some("cloaked".to_string()),
        (true, false) => Some("cloak waiting for energy".to_string()),
        _ => None,
    })
}

/// How many missiles a silo has built and is keeping.
pub fn stock_status(snapshot: &SimSnapshot, id: SimId, db: &BlueprintDb) -> Option<String> {
    let &(_, stock) = snapshot.stocks.iter().find(|(silo, _)| *silo == id)?;
    let unit = find_unit(snapshot, id)?;
    let blueprint = db.get(BlueprintId(unit.blueprint));
    let word = if blueprint.missile_defense.is_some() { "interceptor" } else { "missile" };
    Some(format!("{stock} {word}{} ready", if stock == 1 { "" } else { "s" }))
}

/// The selected transports with anything on board.
pub fn loaded_transports(snapshot: &SimSnapshot, selected: &[SimId]) -> Vec<SimId> {
    selected.iter().copied().filter(|id| snapshot.cargo.iter().any(|&(transport, count)| transport == *id && count > 0)).collect()
}

/// How many units a transport carries.
pub fn cargo_status(snapshot: &SimSnapshot, id: SimId) -> Option<String> {
    let &(_, count) = snapshot.cargo.iter().find(|(transport, _)| *transport == id)?;
    Some(match count {
        0 => "empty".to_string(),
        1 => "carrying 1 unit".to_string(),
        count => format!("carrying {count} units"),
    })
}

/// Whether anything selected has a gun, and so can attack the ground.
pub fn has_weapons(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> bool {
    selected
        .iter()
        .filter_map(|id| find_unit(snapshot, *id))
        .any(|unit| unit.blueprint == u16::MAX || db.get(BlueprintId(unit.blueprint)).weapon.is_some())
}

/// Which of the orders that only some units can give the selection can.
#[allow(clippy::struct_excessive_bools)]
#[derive(Copy, Clone)]
struct OrderOffers {
    discharge: bool,
    weapons: bool,
    teleport: bool,
    capture: bool,
    unload: bool,
}

/// The size of the order and toggle buttons, in points.
const ORDER_BUTTON: f32 = 36.0;

/// An order or toggle button: its glyph, its key in the corner, and its name and description on
/// hover.
fn glyph_button(ui: &mut egui::Ui, glyph: Glyph, key: &str, chosen: bool, hint: &str) -> egui::Response {
    hud::square_button(ui, Vec2::splat(ORDER_BUTTON), chosen, true, |painter, rect| {
        hud::draw_glyph(painter, rect.shrink(2.0), glyph);
        if !key.is_empty() {
            let size = if key.chars().count() > 1 { 9.0 } else { 12.0 };
            hud::outlined_text(painter, rect.right_bottom() + Vec2::new(-1.0, 1.0), Align2::RIGHT_BOTTOM, key, FontId::new(size, egui::FontFamily::Name(BOLD.into())), Color32::WHITE);
        }
    })
    .on_hover_text(bold_keys(hint))
}

const fn order_glyph(order: ArmedOrder) -> Glyph {
    match order {
        ArmedOrder::Move => Glyph::Move,
        ArmedOrder::AttackMove => Glyph::AttackMove,
        ArmedOrder::Formation => Glyph::Formation,
        ArmedOrder::AttackGround => Glyph::AttackGround,
        ArmedOrder::Patrol => Glyph::Patrol,
        ArmedOrder::Assist => Glyph::Assist,
        ArmedOrder::Discharge => Glyph::Discharge,
        ArmedOrder::Teleport => Glyph::Teleport,
        ArmedOrder::Capture => Glyph::Capture,
        ArmedOrder::Unload => Glyph::Unload,
    }
}

/// The unit orders, as glyph buttons: the armed one lit.
fn unit_order_buttons(ui: &mut egui::Ui, armed: Option<ArmedOrder>, offers: &OrderOffers, actions: &mut Vec<UiAction>) {
    let OrderOffers { discharge, weapons, teleport, capture, unload } = *offers;
    let offered = |order: &ArmedOrder| match order {
        ArmedOrder::Discharge => discharge,
        ArmedOrder::AttackGround => weapons,
        _ => true,
    };
    let mut arm = |ui: &mut egui::Ui, order: ArmedOrder, key: &str| {
        let hint = format!("[{key}] {}\n{}", order.name(), order.description());
        if glyph_button(ui, order_glyph(order), key, armed == Some(order), &hint).clicked() {
            actions.push(UiAction::Arm(order));
        }
    };
    for order in ArmedOrder::ALL.into_iter().filter(offered) {
        arm(ui, order, &order.letter().to_string());
    }
    if capture {
        arm(ui, ArmedOrder::Capture, "^C");
    }
    if teleport {
        arm(ui, ArmedOrder::Teleport, "^T");
    }
    if unload {
        arm(ui, ArmedOrder::Unload, "^U");
    }
    if glyph_button(ui, Glyph::Stop, &hotkeys::STOP.to_string(), false, &format!("[{}] Stop\nDrop all orders.", hotkeys::STOP)).clicked() {
        actions.push(UiAction::Stop);
    }
}

/// Whether the viewer can sense a spot, and so teleport there: it is in sight, or within the
/// radar or deep scan of one of its sensors.
pub fn senses_spot(snapshot: &SimSnapshot, point: [f64; 2]) -> bool {
    let Some(fog) = &snapshot.fog else {
        return true;
    };
    let side = (fog.len() as f64).sqrt() as usize;
    let cell = |value: f64| ((value / f64::from(crate::sim::config::VISIBILITY_CELL_UNITS)).floor().max(0.0) as usize).min(side.saturating_sub(1));
    if fog.get(cell(point[1]) * side + cell(point[0])) == Some(&255) {
        return true;
    }
    snapshot.sensor_ranges.iter().any(|sensor| {
        let (dx, dy) = (point[0] - f64::from(sensor.center[0]) / 4096.0, point[1] - f64::from(sensor.center[1]) / 4096.0);
        let reach = f64::from(sensor.radar.max(sensor.deep_scan)) / 4096.0;
        dx * dx + dy * dy <= reach * reach
    })
}

/// The enhancement in each of a unit's slots.
pub fn installed_enhancements(snapshot: &SimSnapshot, id: SimId) -> [Option<EnhancementId>; 3] {
    snapshot
        .enhancements
        .iter()
        .find(|(unit, _)| *unit == id)
        .map_or([None; 3], |(_, slots)| slots.map(|slot| (slot != u16::MAX).then_some(EnhancementId(slot))))
}

/// The tier of structures a builder can build: its own, or its fabrication pack's. A suite
/// still being fitted counts, as what it unlocks can be queued behind it.
fn unit_build_tier(snapshot: &SimSnapshot, unit: &UnitView, db: &BlueprintDb) -> u8 {
    let under_way = snapshot.enhancing.iter().filter(|(id, _, _)| *id == unit.id).map(|&(_, enhancement, _)| EnhancementId(enhancement));
    let suites = installed_enhancements(snapshot, unit.id).into_iter().flatten().chain(under_way).map(|id| db.enhancement(id).effects.build_tier);
    suites.fold(db.get(BlueprintId(unit.blueprint)).tier, u8::max)
}

/// The selected units that have a teleporter.
pub fn teleporters(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> Vec<SimId> {
    selected
        .iter()
        .copied()
        .filter(|id| installed_enhancements(snapshot, *id).into_iter().flatten().any(|enhancement| db.enhancement(enhancement).effects.teleporter.is_some()))
        .collect()
}

/// One enhancement button: which units would start it, and whether all of them have it already.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnhancementOffer {
    pub enhancement: EnhancementId,
    pub units: Vec<SimId>,
    pub installed: bool,
    /// What it would remove from the slot, for the first unit that would start it.
    pub replaces: Option<EnhancementId>,
}

/// The enhancements the selection can take, slot by slot, for the first selected unit that
/// can take any. A unit is offered one if it is not there already, and what it builds on is.
pub fn enhancement_offers(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> Vec<(Slot, Vec<EnhancementOffer>)> {
    let units: Vec<&UnitView> = selected
        .iter()
        .filter_map(|id| find_unit(snapshot, *id))
        .filter(|unit| unit.blueprint != u16::MAX && unit.built_permille == 1000 && db.enhancements_for(BlueprintId(unit.blueprint)).next().is_some())
        .collect();
    let Some(first) = units.first() else {
        return Vec::new();
    };
    let enhancing = |id: SimId| snapshot.enhancing.iter().find(|(unit, _, _)| *unit == id).map(|(_, enhancement, _)| EnhancementId(*enhancement));
    Slot::ALL
        .into_iter()
        .map(|slot| {
            let offers = db
                .enhancements_for(BlueprintId(first.blueprint))
                .filter(|enhancement| enhancement.slot == slot)
                .map(|enhancement| {
                    let mut offer = EnhancementOffer { enhancement: enhancement.id, units: Vec::new(), installed: true, replaces: None };
                    for unit in units.iter().filter(|unit| enhancement.units.contains(&BlueprintId(unit.blueprint))) {
                        let current = installed_enhancements(snapshot, unit.id)[slot.index()];
                        // The T3 suite includes the T2 one, so with it, the T2 one shows as there.
                        let has = db.slot_has(current, enhancement.id);
                        offer.installed &= has;
                        let able = !has && enhancing(unit.id) != Some(enhancement.id) && enhancement.requires.is_none_or(|needed| current == Some(needed));
                        if able {
                            if offer.units.is_empty() {
                                offer.replaces = current;
                            }
                            offer.units.push(unit.id);
                        }
                    }
                    offer
                })
                .collect();
            (slot, offers)
        })
        .collect()
}

/// What an enhancement does, in words.
pub fn enhancement_description(enhancement: &Enhancement, db: &BlueprintDb) -> String {
    let effects = &enhancement.effects;
    let per_second = |per_tick: i64| per_tick * TICKS_PER_SECOND;
    let mut parts = Vec::new();
    if let Some(range) = effects.weapon_range {
        parts.push(format!("gun range {}", range.to_num::<i32>()));
    }
    if let Some(damage) = effects.weapon_damage {
        parts.push(format!("gun damage {}", damage.to_num::<i32>()));
    }
    if let Some(power) = effects.build_power {
        parts.push(format!("build power {}", power.to_num::<i32>()));
    }
    if effects.build_tier > 0 {
        parts.push(format!("builds T{} structures", effects.build_tier));
    }
    if effects.health > crate::math::fp::Fp::ZERO {
        parts.push(format!("{} more health", effects.health.to_num::<i32>()));
    }
    if effects.regen_per_tick > crate::math::fp::Fp::ZERO {
        parts.push(format!("heals {} a second", (effects.regen_per_tick * TICKS_PER_SECOND as i32).to_num::<i32>()));
    }
    if let Some(shield) = effects.shield {
        parts.push(format!(
            "a {}-health shield of radius {}, for {} energy a second",
            shield.health.to_num::<i32>(),
            shield.radius.to_num::<i32>(),
            per_second(shield.upkeep.to_num::<i64>())
        ));
    }
    if let Some(upkeep) = effects.stealth_upkeep {
        parts.push(format!("hidden from enemy radar, for {} energy a second", per_second(upkeep.to_num::<i64>())));
    }
    if let Some(upkeep) = effects.cloak_upkeep {
        parts.push(format!("a cloak, switched with O, for {} energy a second", per_second(upkeep.to_num::<i64>())));
    }
    if effects.teleporter.is_some() {
        parts.push("teleporting, with Ctrl+T".to_string());
    }
    if effects.mass_income > crate::sim::blueprints::Res::ZERO || effects.energy_income > crate::sim::blueprints::Res::ZERO {
        parts.push(format!("{} materiel and {} energy a second", per_second(effects.mass_income.to_num::<i64>()), per_second(effects.energy_income.to_num::<i64>())));
    }
    let mut text = format!("{}: {}.", enhancement.name, parts.join(", "));
    if let Some(needed) = enhancement.requires {
        text.push_str(&format!(" Builds on {}.", db.enhancement(needed).name));
    }
    text.push_str(&format!(" Build time {}. It stands still meanwhile, and a new order cancels it.", enhancement.build_time.to_num::<i64>()));
    text
}

/// The enhancements, slot by slot, as buttons along the command bar: each with its name and
/// cost, lit if fitted, and its details on hover.
fn enhancement_bar(ui: &mut egui::Ui, snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb, actions: &mut Vec<UiAction>) {
    let rows = enhancement_offers(snapshot, selected, db);
    egui::ScrollArea::horizontal().id_source("enhancements").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (index, (slot, offers)) in rows.into_iter().enumerate() {
                if index > 0 {
                    ui.separator();
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(egui::RichText::new(slot.name()).size(10.5).color(hud::DIM_TEXT));
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        for offer in offers {
                            let enhancement = db.enhancement(offer.enhancement);
                            let mut hint = enhancement_description(enhancement, db);
                            if let Some(replaced) = offer.replaces.filter(|replaced| Some(*replaced) != enhancement.requires) {
                                hint.push_str(&format!(" Removes {}, which is lost.", db.enhancement(replaced).name));
                            }
                            let enabled = !offer.units.is_empty();
                            let button = hud::square_button(ui, Vec2::new(ENHANCEMENT_BUTTON_WIDTH, BAR_BUTTON - 14.0), offer.installed, enabled, |painter, rect| {
                                painter.text(rect.center_top() + Vec2::new(0.0, 1.0), Align2::CENTER_TOP, &enhancement.name, FontId::proportional(10.5), hud::TEXT);
                                let cost = format!("{}  {}", hud::compact(enhancement.mass.to_num::<i64>()), hud::compact(enhancement.energy.to_num::<i64>()));
                                painter.text(rect.center_bottom() - Vec2::new(0.0, 1.0), Align2::CENTER_BOTTOM, cost, FontId::proportional(9.5), hud::DIM_TEXT);
                            });
                            if button.on_hover_text(hint).clicked() {
                                actions.push(UiAction::Enhance { units: offer.units.clone(), enhancement: offer.enhancement });
                            }
                        }
                    });
                });
            }
        });
    });
}

const ENHANCEMENT_BUTTON_WIDTH: f32 = 112.0;

/// What all the selected builders can build, in the order of the first one's list.
fn builder_options(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> Vec<BlueprintId> {
    let builders: Vec<&UnitView> = selected
        .iter()
        .filter_map(|id| find_unit(snapshot, *id))
        .filter(|unit| {
            unit.blueprint != u16::MAX && {
                let blueprint = db.get(BlueprintId(unit.blueprint));
                !blueprint.is_factory() && !blueprint.builds.is_empty()
            }
        })
        .collect();
    let Some(first) = builders.first() else {
        return Vec::new();
    };
    let team = first.team;
    db.get(BlueprintId(first.blueprint))
        .builds
        .iter()
        .copied()
        .filter(|option| builders.iter().all(|unit| db.get(BlueprintId(unit.blueprint)).builds.contains(option) && db.get(*option).tier <= unit_build_tier(snapshot, unit, db)))
        .filter(|option| support_allowed(snapshot, team, *option, db))
        .collect()
}

/// The highest tier of finished headquarters of a kind the team has, and at least 1.
pub fn headquarters_tier(snapshot: &SimSnapshot, team: Team, kind: FactoryKind, db: &BlueprintDb) -> u8 {
    snapshot
        .units
        .iter()
        .filter(|unit| unit.team == team && unit.blueprint != u16::MAX && unit.built_permille == 1000)
        .map(|unit| db.get(BlueprintId(unit.blueprint)))
        .filter(|unit| unit.is_headquarters() && unit.factory_kind == Some(kind))
        .map(|unit| unit.tier)
        .max()
        .unwrap_or(1)
}

/// Whether the team may build this now: anything but a support factory, which needs a
/// headquarters of its kind of at least its tier.
fn support_allowed(snapshot: &SimSnapshot, team: Team, blueprint: BlueprintId, db: &BlueprintDb) -> bool {
    let unit = db.get(blueprint);
    match (unit.support, unit.factory_kind) {
        (true, Some(kind)) => headquarters_tier(snapshot, team, kind, db) >= unit.tier,
        _ => true,
    }
}

/// Holding Shift queues several units at once. An upgrade only ever happens once.
pub fn with_shift(action: UiAction, shift: bool, db: &BlueprintDb) -> UiAction {
    match action {
        UiAction::Queue { factories, blueprint, selected_count, .. } if shift && !db.get(blueprint).is_factory() => {
            UiAction::Queue { factories, blueprint, selected_count, count: SHIFT_QUEUE_COUNT }
        }
        UiAction::Batch(actions) => UiAction::Batch(actions.into_iter().map(|action| with_shift(action, shift, db)).collect()),
        other => other,
    }
}

impl UiAction {
    /// The actions in a batch, or this one alone.
    pub fn flatten(self) -> Vec<Self> {
        match self {
            Self::Batch(actions) => actions.into_iter().flat_map(Self::flatten).collect(),
            other => vec![other],
        }
    }
}

fn queue_action(menu: &FactoryMenu, able: &[SimId], blueprint: BlueprintId) -> UiAction {
    UiAction::Queue { factories: able.to_vec(), blueprint, selected_count: menu.factories.len(), count: 1 }
}

/// A mixed selection turns on for all, as the next press turns off for all.
fn repeat_action(menu: &FactoryMenu) -> UiAction {
    UiAction::SetRepeat { factories: menu.factories.clone(), repeat: menu.repeat != RepeatState::On }
}

fn clear_action(menu: &FactoryMenu) -> UiAction {
    UiAction::ClearQueue { factories: menu.factories.clone() }
}

/// What a command panel key does for this selection: the same as its button, or `None`
/// if the panel has no such option right now.
/// What a key does for the selection. A build key picks from the level each bar shows.
/// What a key does for the selection. `placing` is the structure being placed, if any: a
/// builder's family key moves on from it to the next of the family.
pub fn hotkey_action(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb, letter: char, tiers: BuildTiers, placing: Option<BlueprintId>) -> Option<UiAction> {
    // O switches things off and on: it pauses what can be paused, or else toggles a cloak.
    if letter == hotkeys::PAUSE {
        return pause_action(snapshot, selected, db).or_else(|| cloak_action(snapshot, selected));
    }
    if has_mobile_units(snapshot, selected) {
        match letter {
            hotkeys::MOVE => return Some(UiAction::Arm(ArmedOrder::Move)),
            hotkeys::ATTACK_MOVE => return Some(UiAction::Arm(ArmedOrder::AttackMove)),
            hotkeys::FORMATION => return Some(UiAction::Arm(ArmedOrder::Formation)),
            hotkeys::ATTACK_GROUND if has_weapons(snapshot, selected, db) => return Some(UiAction::Arm(ArmedOrder::AttackGround)),
            hotkeys::ASSIST => return Some(UiAction::Arm(ArmedOrder::Assist)),
            hotkeys::DISCHARGE if has_discharge(snapshot, selected, db) => return Some(UiAction::Arm(ArmedOrder::Discharge)),
            hotkeys::STOP => return Some(UiAction::Stop),
            _ => {}
        }
    }
    let is_key = |blueprint: BlueprintId| blueprint_hotkey(&db.get(blueprint).key) == Some(letter);
    // With a factory selected, the build keys are its bar's, and a builder's bar is not shown.
    let Some(menu) = factory_menu(snapshot, selected, db) else {
        let options = builder_options(snapshot, selected, db);
        let Some(shown) = shown_tier(tiers.builder, &options, db) else {
            let units = upgradable_units(snapshot, selected, db);
            return (letter == hotkeys::UPGRADE && !units.is_empty()).then_some(UiAction::Upgrade { units });
        };
        // The family on this key, in the bar's order: the first, or the one after what is
        // being placed, round to the first again.
        let family: Vec<BlueprintId> = bar_items(&options, shown, db).into_iter().filter(|option| is_key(*option)).collect();
        if !family.is_empty() {
            let next = placing.and_then(|current| family.iter().position(|option| *option == current)).map_or(0, |index| (index + 1) % family.len());
            return Some(UiAction::BeginPlacement(family[next]));
        }
        let units = upgradable_units(snapshot, selected, db);
        return (letter == hotkeys::UPGRADE && !units.is_empty()).then_some(UiAction::Upgrade { units });
    };
    if letter == hotkeys::UPGRADE {
        let units = upgradable_units(snapshot, selected, db);
        if !units.is_empty() {
            return Some(UiAction::Upgrade { units });
        }
    }
    // Options in different factories may share a key, and then it queues each in its own.
    let all: Vec<BlueprintId> = menu.options.iter().map(|(option, _)| *option).collect();
    let shown = shown_tier(tiers.factory, &all, db);
    let mut matching: Vec<UiAction> = menu
        .options
        .iter()
        .filter(|(option, _)| Some(db.get(*option).tier) == shown && is_key(*option))
        .map(|(option, able)| queue_action(&menu, able, *option))
        .collect();
    match matching.len() {
        0 => {}
        1 => return matching.pop(),
        _ => return Some(UiAction::Batch(matching)),
    }
    match letter {
        hotkeys::UPGRADE => menu.upgrade.as_ref().map(|(upgrade, able)| queue_action(&menu, able, *upgrade)),
        hotkeys::REPEAT => Some(repeat_action(&menu)),
        hotkeys::CLEAR_QUEUE => Some(clear_action(&menu)),
        _ => None,
    }
}

/// A unit's veteran rank, or 0.
/// How many enemies a unit has destroyed, made the final hit on.
pub fn kills_of(snapshot: &SimSnapshot, id: SimId) -> u32 {
    snapshot.kills.binary_search_by_key(&id, |kills| kills.0).map_or(0, |index| snapshot.kills[index].1)
}

/// A small mark at the bottom left of a nameplate's portrait: a cross and how many enemies the
/// selection has destroyed.
fn kills_badge(painter: &egui::Painter, portrait: Rect, kills: u32) {
    let font = FontId::proportional(10.0);
    let text = painter.layout_no_wrap(kills.to_string(), font, Color32::WHITE);
    let badge = Rect::from_min_size(Pos2::new(portrait.left() + 1.0, portrait.bottom() - 14.0), Vec2::new(text.size().x + 17.0, 13.0));
    painter.rect_filled(badge, 3.0, Color32::from_rgba_unmultiplied(10, 14, 20, 220));
    let mark = Rect::from_center_size(Pos2::new(badge.left() + 7.0, badge.center().y), Vec2::splat(7.0));
    let stroke = egui::Stroke::new(1.6_f32, Color32::from_rgb(235, 90, 70));
    painter.line_segment([mark.left_top(), mark.right_bottom()], stroke);
    painter.line_segment([mark.right_top(), mark.left_bottom()], stroke);
    painter.galley(Pos2::new(badge.left() + 13.0, badge.center().y - text.size().y / 2.0), text, Color32::WHITE);
}

pub fn rank_of(snapshot: &SimSnapshot, id: SimId) -> u8 {
    snapshot.veterans.binary_search_by_key(&id, |veteran| veteran.0).map_or(0, |index| snapshot.veterans[index].1)
}

/// The selected units that can be paused: factories, and the player's sites still being built.
pub fn pausable_units(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> Vec<SimId> {
    selected
        .iter()
        .copied()
        .filter(|id| {
            find_unit(snapshot, *id).is_some_and(|unit| {
                unit.blueprint != u16::MAX && {
                    let blueprint = db.get(BlueprintId(unit.blueprint));
                    blueprint.is_factory() || blueprint.shield.is_some() || blueprint.upkeep > crate::sim::blueprints::Res::ZERO || unit.built_permille < 1000
                }
            })
        })
        .collect()
}

/// The selected commanders and structures that can upgrade in place now: finished, not
/// upgrading already, and with somewhere to go. Factories upgrade through their queue instead.
pub fn upgradable_units(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> Vec<SimId> {
    selected
        .iter()
        .copied()
        .filter(|id| {
            find_unit(snapshot, *id).is_some_and(|unit| {
                unit.blueprint != u16::MAX && unit.built_permille == 1000 && {
                    let blueprint = db.get(BlueprintId(unit.blueprint));
                    blueprint.upgrades_to.is_some() && !blueprint.is_factory()
                }
            }) && snapshot.upgrades.iter().all(|(upgrading, _)| upgrading != id)
        })
        .collect()
}

/// Brings the selected submarines up, unless they all are up already, in which case it takes
/// them down.
pub fn surface_action(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> Option<UiAction> {
    let subs: Vec<SimId> = selected
        .iter()
        .filter_map(|id| find_unit(snapshot, *id))
        .filter(|unit| unit.blueprint != u16::MAX && db.get(BlueprintId(unit.blueprint)).movement == crate::sim::components::MoveClass::Submarine)
        .map(|unit| unit.id)
        .collect();
    if subs.is_empty() {
        return None;
    }
    let all_up = subs.iter().all(|id| snapshot.surfaced.binary_search(id).is_ok());
    Some(UiAction::SetSurfaced { units: subs, surfaced: !all_up })
}

/// Cloaks the selected units that can cloak, unless they all are already, in which case it
/// uncloaks them.
pub fn cloak_action(snapshot: &SimSnapshot, selected: &[SimId]) -> Option<UiAction> {
    let able: Vec<&(SimId, bool, bool)> = snapshot.cloaks.iter().filter(|(id, _, _)| selected.contains(id)).collect();
    if able.is_empty() {
        return None;
    }
    let all_on = able.iter().all(|(_, on, _)| *on);
    Some(UiAction::SetCloak { units: able.iter().map(|(id, _, _)| *id).collect(), on: !all_on })
}

/// Pauses them all, unless they are all paused already, in which case it resumes them.
pub fn pause_action(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> Option<UiAction> {
    let units = pausable_units(snapshot, selected, db);
    if units.is_empty() {
        return None;
    }
    let all_paused = units.iter().all(|id| snapshot.paused.binary_search(id).is_ok());
    Some(UiAction::SetPaused { units, paused: !all_paused })
}

/// What the selection makes and spends, as text. Empty if it has nothing to show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectionRates {
    pub output: String,
    pub spending: String,
}

fn per_second(tenths: i32) -> String {
    format!("{}.{}", tenths / 10, tenths % 10)
}

/// Adds up what the selected structures make, and what the selected factories are spending.
pub fn selection_rates(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> SelectionRates {
    let mut total = UnitRatesView { id: SimId(0), mass_income: 0, energy_income: 0, mass_use: 0, energy_use: 0 };
    let mut factories = 0;
    for &id in selected {
        let Ok(index) = snapshot.rates.binary_search_by_key(&id, |rates| rates.id) else {
            continue;
        };
        let rates = snapshot.rates[index];
        total.mass_income += rates.mass_income;
        total.energy_income += rates.energy_income;
        total.mass_use += rates.mass_use;
        total.energy_use += rates.energy_use;
        // Factories and shields spend, so they say so even when they are spending nothing.
        factories += usize::from(find_unit(snapshot, id).is_some_and(|unit| unit.blueprint != u16::MAX && {
            let blueprint = db.get(BlueprintId(unit.blueprint));
            blueprint.is_factory() || blueprint.shield.is_some()
        }));
    }
    let mut output = Vec::new();
    if total.mass_income > 0 {
        output.push(format!("+{} materiel/s", per_second(total.mass_income)));
    }
    if total.energy_income > 0 {
        output.push(format!("+{} energy/s", per_second(total.energy_income)));
    }
    let spending = match (factories, total.mass_use + total.energy_use) {
        (0, _) => String::new(),
        (_, 0) => "Using nothing".to_string(),
        _ => format!("Using {} materiel/s, {} energy/s", per_second(total.mass_use), per_second(total.energy_use)),
    };
    SelectionRates { output: output.join(", "), spending }
}

/// What the selection makes less what it uses, in tenths a second, mass then energy, if any of
/// it reports rates.
pub fn selection_net(snapshot: &SimSnapshot, selected: &[SimId]) -> Option<(i32, i32)> {
    let mut net = None;
    for id in selected {
        if let Ok(index) = snapshot.rates.binary_search_by_key(id, |rates| rates.id) {
            let rates = snapshot.rates[index];
            let (mass, energy) = net.unwrap_or((0, 0));
            net = Some((mass + rates.mass_income - rates.mass_use, energy + rates.energy_income - rates.energy_use));
        }
    }
    net
}

/// The selection's build power, all together.
pub fn selection_build_power(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> i64 {
    selected.iter().filter_map(|id| find_unit(snapshot, *id)).filter(|unit| unit.blueprint != u16::MAX).map(|unit| db.get(BlueprintId(unit.blueprint)).build_power.to_num::<i64>()).sum()
}

/// A tenths figure, signed, with one decimal: "+2.5", "-14.1".
fn signed_tenths(tenths: i32) -> String {
    let sign = if tenths < 0 { "-" } else { "+" };
    format!("{sign}{}.{}", tenths.unsigned_abs() / 10, tenths.unsigned_abs() % 10)
}

/// An order that can be picked up and put somewhere else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DraggableOrder {
    Walk { units: Vec<SimId>, task: BuildTask },
    Build { units: Vec<SimId>, task: BuildTask },
    Rally { factories: Vec<SimId> },
}

/// An order's icon on the map. A group's order has one, in the middle of the group's spots.
#[derive(Clone, Debug, PartialEq)]
pub struct OrderHandle {
    pub position: [f64; 3],
    pub kind: MarkerKind,
    pub order: DraggableOrder,
}

/// One icon for each order that can be moved. Orders shared by several units count once.
pub fn order_handles(snapshot: &SimSnapshot) -> Vec<OrderHandle> {
    let raw = |value: i32| f64::from(value) / 4096.0;
    // The task, its units, and the sum of their positions.
    let mut orders: Vec<(BuildTask, Vec<SimId>, [f64; 3])> = Vec::new();
    for stop in &snapshot.order_stops {
        let position = stop.position.map(raw);
        match orders.iter_mut().find(|(task, _, _)| task.is_same_order(&stop.task)) {
            Some((_, units, sum)) => {
                units.push(stop.unit);
                *sum = std::array::from_fn(|axis| sum[axis] + position[axis]);
            }
            None => orders.push((stop.task, vec![stop.unit], position)),
        }
    }
    let mut handles: Vec<OrderHandle> = orders
        .into_iter()
        .map(|(task, units, sum)| {
            let position = sum.map(|value| value / units.len() as f64);
            match task {
                BuildTask::Walk { order, .. } => {
                    let kind = if order.kind == OrderKind::AttackMove { MarkerKind::AttackMove } else { MarkerKind::Move };
                    OrderHandle { position, kind, order: DraggableOrder::Walk { units, task } }
                }
                _ => OrderHandle { position, kind: MarkerKind::Build, order: DraggableOrder::Build { units, task } },
            }
        })
        .collect();
    // Factories given a rally point together keep sharing it.
    let mut rallies: Vec<([i32; 3], Vec<SimId>)> = Vec::new();
    for factory in &snapshot.factories {
        let Some(rally) = factory.rally else {
            continue;
        };
        match rallies.iter_mut().find(|(point, _)| *point == rally) {
            Some((_, factories)) => factories.push(factory.id),
            None => rallies.push((rally, vec![factory.id])),
        }
    }
    handles.extend(rallies.into_iter().map(|(point, factories)| OrderHandle { position: point.map(raw), kind: MarkerKind::Rally, order: DraggableOrder::Rally { factories } }));
    handles
}

/// The index of the handle under the cursor, if any. Both are in pixels.
pub fn handle_under_cursor(handles: &[OrderHandle], camera: &Camera, cursor: [f64; 2], pixels_per_point: f64) -> Option<usize> {
    let reach = HANDLE_GRAB_POINTS * pixels_per_point;
    let mut best: Option<(f64, usize)> = None;
    for (index, handle) in handles.iter().enumerate() {
        if let Some(at) = camera.world_to_screen(handle.position) {
            let away = (at[0] - cursor[0]).hypot(at[1] - cursor[1]);
            if away <= reach && best.is_none_or(|(closest, _)| away < closest) {
                best = Some((away, index));
            }
        }
    }
    best.map(|(_, index)| index)
}

fn draw_handle(painter: &egui::Painter, center: Pos2, kind: MarkerKind, radius: f32, opacity: f32) {
    let color = marker_color(kind).gamma_multiply(opacity);
    painter.circle(center, radius, Color32::from_black_alpha((210.0 * opacity) as u8), Stroke::new(2.0_f32, color));
    let letter = match kind {
        MarkerKind::AttackMove => "A",
        MarkerKind::Build => "B",
        MarkerKind::Rally => "R",
        _ => "M",
    };
    painter.text(center, Align2::CENTER_CENTER, letter, FontId::proportional(radius * 1.2), color);
}

/// Draws the icons that can be grabbed, for while Shift is held.
pub fn draw_order_handles(ctx: &egui::Context, camera: &Camera, handles: &[OrderHandle], hovered: Option<usize>, strength: f32, pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    for (index, handle) in handles.iter().enumerate() {
        if let Some(at) = camera.world_to_screen(handle.position) {
            let radius = if hovered == Some(index) { HANDLE_RADIUS_POINTS * 1.35 } else { HANDLE_RADIUS_POINTS };
            draw_handle(&painter, Pos2::new(at[0] as f32 / pixels_per_point, at[1] as f32 / pixels_per_point), handle.kind, radius, strength);
        }
    }
}

/// Shows where an order being dragged was, and where it would go.
pub fn draw_dragged_order(ctx: &egui::Context, camera: &Camera, handle: &OrderHandle, cursor: [f64; 2], pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let to = Pos2::new(cursor[0] as f32 / pixels_per_point, cursor[1] as f32 / pixels_per_point);
    if let Some(at) = camera.world_to_screen(handle.position) {
        let from = Pos2::new(at[0] as f32 / pixels_per_point, at[1] as f32 / pixels_per_point);
        painter.extend(egui::Shape::dashed_line(&[from, to], Stroke::new(1.5_f32, marker_color(handle.kind).gamma_multiply(0.7)), DASH_POINTS, GAP_POINTS));
        draw_handle(&painter, from, handle.kind, HANDLE_RADIUS_POINTS * 0.7, 0.5);
    }
    draw_handle(&painter, to, handle.kind, HANDLE_RADIUS_POINTS * 1.35, 1.0);
}

/// One entry in the queue shown for the selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueChip {
    pub label: String,
    /// What its button shows.
    pub icon: ChipIcon,
    /// How many of it there are, for a factory's items.
    pub count: u32,
    /// How far along it is, in thousandths, if started.
    pub progress: Option<u16>,
    /// What right-clicking it does: remove one.
    pub remove: UiAction,
    /// What left-clicking it does, if anything: queue one more.
    pub add: Option<UiAction>,
}

/// What a queue entry's button shows: the item's icon, or the order's glyph.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ChipIcon {
    Item(BlueprintId),
    Order(Glyph),
}

/// What shows on a builder's queued order.
fn task_icon(task: &BuildTask) -> ChipIcon {
    match *task {
        BuildTask::Build { blueprint, .. } => ChipIcon::Item(blueprint),
        BuildTask::Assist { .. } => ChipIcon::Order(Glyph::Assist),
        BuildTask::Reclaim { .. } => ChipIcon::Order(Glyph::Reclaim),
        BuildTask::Repair { .. } => ChipIcon::Order(Glyph::Repair),
        BuildTask::Capture { .. } => ChipIcon::Order(Glyph::Capture),
        BuildTask::Walk { order, .. } => ChipIcon::Order(if order.kind == OrderKind::AttackMove { Glyph::AttackMove } else { Glyph::Move }),
    }
}

/// What a builder's order is called on screen.
pub fn task_name(task: &BuildTask, db: &BlueprintDb) -> String {
    match *task {
        BuildTask::Build { blueprint, .. } => db.get(blueprint).name.clone(),
        BuildTask::Assist { .. } => "Assist".to_string(),
        BuildTask::Reclaim { .. } => "Reclaim".to_string(),
        BuildTask::Repair { .. } => "Repair".to_string(),
        BuildTask::Capture { .. } => "Capture".to_string(),
        BuildTask::Walk { order, .. } => if order.kind == OrderKind::AttackMove { "Attack-move" } else { "Move" }.to_string(),
    }
}

/// The queue for the selection: builders' orders in order, then factories' items. An order
/// that several selected units share appears once, and removing it removes it from them all.
pub fn queue_chips(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> Vec<QueueChip> {
    let mut orders: Vec<(BuildTask, Vec<SimId>)> = Vec::new();
    for &id in selected {
        let Ok(index) = snapshot.build_queues.binary_search_by_key(&id, |queue| queue.id) else {
            continue;
        };
        for task in &snapshot.build_queues[index].tasks {
            match orders.iter_mut().find(|(existing, _)| existing.is_same_order(task)) {
                Some((_, units)) => units.push(id),
                None => orders.push((*task, vec![id])),
            }
        }
    }
    let mut chips: Vec<QueueChip> = orders
        .into_iter()
        .enumerate()
        .map(|(position, (task, units))| {
            let progress = match task {
                BuildTask::Build { site: Some(site), .. } => find_unit(snapshot, site).map(|site| site.built_permille),
                _ => None,
            };
            let built = progress.map_or(String::new(), |permille| format!(" {}%", permille / 10));
            QueueChip { label: format!("{}. {}{built}", position + 1, task_name(&task, db)), icon: task_icon(&task), count: 1, progress, remove: UiAction::CancelTask { units, task }, add: None }
        })
        .collect();

    if let Some(menu) = factory_menu(snapshot, selected, db) {
        for (index, &(item, count)) in menu.queue.iter().enumerate() {
            // The selected factories with this item queued, with how many of it, and how long
            // their queues are.
            let holding: Vec<(SimId, usize, usize)> = menu
                .factories
                .iter()
                .filter_map(|&id| {
                    let factory = snapshot.factories.iter().find(|factory| factory.id == id)?;
                    let held = factory.queue.iter().filter(|&&queued| queued == item.0).count();
                    (held > 0).then_some((id, held, factory.queue.len()))
                })
                .collect();
            let name = &db.get(item).name;
            let label = if count > 1 { format!("{name} x{count}") } else { name.clone() };
            let progress = if index == 0 { menu.progress_permille } else { None };
            // One at a time: removed from the factory with the most of it, the last on a tie, and
            // added to the one of them with the shortest queue, the first on a tie. Not another
            // upgrade, which a factory takes only once.
            let most = holding.iter().max_by_key(|&&(_, held, _)| held).map(|&(id, ..)| id);
            let shortest = holding.iter().min_by_key(|&&(_, _, length)| length).map(|&(id, ..)| id);
            let remove = UiAction::RemoveQueued { factories: most.into_iter().collect(), blueprint: item };
            let add = shortest.filter(|_| !db.get(item).is_factory()).map(|id| UiAction::Queue { factories: vec![id], blueprint: item, selected_count: 1, count: 1 });
            chips.push(QueueChip { label, icon: ChipIcon::Item(item), count, progress, remove, add });
        }
    }
    chips
}

/// The size of a queue entry's button, in points.
const QUEUE_BUTTON: f32 = 34.0;

/// The queue, as a row of buttons with how many and how far along, always one row high, empty
/// or not, so that queuing the first item does not move the bar: long queues scroll sideways.
fn queue_chip_row(ui: &mut egui::Ui, chips: &[QueueChip], db: &BlueprintDb, actions: &mut Vec<UiAction>) {
    ui.horizontal(|ui| {
        ui.set_min_height(QUEUE_BUTTON);
        ui.spacing_mut().item_spacing.x = 3.0;
        if chips.is_empty() {
            ui.label(egui::RichText::new("Queue empty").color(hud::DIM_TEXT).size(12.0));
            return;
        }
        egui::ScrollArea::horizontal().id_source("queue row").show(ui, |ui| {
            ui.horizontal(|ui| {
                for chip in chips {
                    let button = hud::square_button(ui, Vec2::splat(QUEUE_BUTTON), false, true, |painter, rect| {
                        match chip.icon {
                            ChipIcon::Item(blueprint) => unit_art(painter, rect, db, &db.get(blueprint).key),
                            ChipIcon::Order(glyph) => hud::draw_glyph(painter, rect.shrink(3.0), glyph),
                        }
                        if chip.count > 1 {
                            hud::outlined_text(painter, rect.right_bottom() + Vec2::new(-1.0, 1.0), Align2::RIGHT_BOTTOM, &chip.count.to_string(), FontId::new(13.0, egui::FontFamily::Name(BOLD.into())), Color32::WHITE);
                        }
                        if let Some(progress) = chip.progress {
                            let bar = Rect::from_min_max(Pos2::new(rect.left() + 1.0, rect.bottom() - 4.0), Pos2::new(rect.right() - 1.0, rect.bottom() - 1.0));
                            hud::meter(painter, bar, f32::from(progress) / 1000.0, BUILDING);
                        }
                    });
                    let hint = if chip.add.is_some() { "Left-click to queue one more, right-click to remove one" } else { "Right-click to remove" };
                    let button = button.on_hover_text(format!("{}\n{hint}", chip.label));
                    if button.secondary_clicked() {
                        actions.push(chip.remove.clone());
                    } else if button.clicked() {
                        actions.extend(chip.add.clone());
                    }
                }
            });
        });
    });
}

/// One tier of one kind of factory in the quick select bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickTier {
    pub label: String,
    pub units: Vec<SimId>,
}

/// One kind of factory in the quick select bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickFamily {
    pub name: String,
    pub tiers: Vec<QuickTier>,
}

/// What the quick select bar offers right now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuickSelect {
    pub commander: Option<SimId>,
    pub families: Vec<QuickFamily>,
}

/// "land_factory_t2" is the kind "Land Factory". Later kinds of factory get their own rows.
fn factory_family_name(key: &str) -> String {
    let base = key.rsplit_once("_t").filter(|(_, tier)| tier.parse::<u8>().is_ok()).map_or(key, |(base, _)| base);
    base.split('_')
        .map(|word| {
            let mut letters = word.chars();
            letters.next().map_or(String::new(), |first| first.to_uppercase().chain(letters).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Sorts a team's commander and factories into the bar's rows. Tiers and kinds with
/// nothing in them do not appear. Factories still being built count, as they take orders.
pub fn quick_select_groups(snapshot: &SimSnapshot, team: Team, db: &BlueprintDb) -> QuickSelect {
    let mut groups = QuickSelect::default();
    let mut families: BTreeMap<String, BTreeMap<u8, Vec<SimId>>> = BTreeMap::new();
    for unit in snapshot.units.iter().filter(|unit| unit.team == team && unit.blueprint != u16::MAX) {
        let blueprint = db.get(BlueprintId(unit.blueprint));
        if blueprint.is_commander() {
            groups.commander.get_or_insert(unit.id);
        } else if blueprint.is_factory() && blueprint.is_structure() {
            families.entry(factory_family_name(&blueprint.key)).or_default().entry(blueprint.tier).or_default().push(unit.id);
        }
    }
    groups.families = families
        .into_iter()
        .map(|(name, tiers)| QuickFamily {
            name,
            tiers: tiers.into_iter().map(|(tier, units)| QuickTier { label: format!("T{tier}  x{}", units.len()), units }).collect(),
        })
        .collect();
    groups
}

/// The one of these units closest to a point on the map, with the lowest id winning a tie.
pub fn closest_unit(snapshot: &SimSnapshot, units: &[SimId], point: [f64; 2]) -> Option<SimId> {
    let distance = |id: SimId| {
        find_unit(snapshot, id).map(|unit| (f64::from(unit.x) / 4096.0 - point[0]).hypot(f64::from(unit.y) / 4096.0 - point[1]))
    };
    let mut best: Option<(f64, SimId)> = None;
    for &id in units {
        if let Some(away) = distance(id) {
            if best.is_none_or(|(closest, _)| away < closest) {
                best = Some((away, id));
            }
        }
    }
    best.map(|(_, id)| id)
}

/// What the quick select bar remembers between frames.
#[derive(Clone, Debug, Default)]
pub struct QuickSelectState {
    expanded: BTreeSet<String>,
    last_click: Option<(String, f64)>,
}

impl QuickSelectState {
    /// Opens a kind of factory, as a click on it would.
    pub fn expand(&mut self, family: &str) {
        self.expanded.insert(family.to_string());
    }

    /// Returns whether this click makes a double-click on the same row.
    fn register_click(&mut self, row: &str, now: f64) -> bool {
        let double = self.last_click.as_ref().is_some_and(|(last, at)| last == row && now - at <= QUICK_DOUBLE_CLICK_SECONDS);
        // A third click starts over, instead of counting as another double-click.
        self.last_click = if double { None } else { Some((row.to_string(), now)) };
        double
    }
}

/// The size of the quick select bar's buttons, in points.
const QUICK_BUTTON: f32 = 48.0;
/// The height of a tier's button, under its kind of factory.
const QUICK_TIER_HEIGHT: f32 = 24.0;

/// Where a quick select square's idle badge sits, in its top right corner, for this many idle.
fn idle_badge_rect(button: Rect, idle: usize) -> Rect {
    let width = if idle > 9 { 40.0 } else { 34.0 };
    Rect::from_min_size(Pos2::new(button.right() - width - 1.0, button.top() + 1.0), Vec2::new(width, 13.0))
}

/// The idle badge: a red hatched tag saying how many have nothing to do.
fn draw_idle_badge(painter: &egui::Painter, rect: Rect, idle: usize) {
    hud::hatched(painter, rect, IDLE, IDLE_STRIPE);
    painter.rect_stroke(rect, 1.0, Stroke::new(1.0_f32, IDLE_EDGE));
    painter.text(rect.center(), Align2::CENTER_CENTER, format!("{idle} IDLE"), FontId::new(9.0, egui::FontFamily::Name(BOLD.into())), Color32::WHITE);
}

/// A big square with a unit's symbol, a count in its corner, and how many are idle.
fn quick_square(ui: &mut egui::Ui, db: &BlueprintDb, key: &str, count: Option<usize>, chosen: bool, health: Option<u16>, idle: usize) -> egui::Response {
    hud::square_button(ui, Vec2::splat(QUICK_BUTTON), chosen, true, |painter, rect| {
        unit_art(painter, rect.shrink(1.0), db, key);
        if idle > 0 {
            draw_idle_badge(painter, idle_badge_rect(rect.expand(2.0), idle), idle);
        }
        if let Some(count) = count {
            hud::outlined_text(painter, rect.right_bottom() + Vec2::new(-2.0, 1.0), Align2::RIGHT_BOTTOM, &count.to_string(), FontId::new(16.0, egui::FontFamily::Name(BOLD.into())), Color32::WHITE);
        }
        if let Some(permille) = health {
            let bar = Rect::from_min_max(Pos2::new(rect.left() + 2.0, rect.bottom() - 5.0), Pos2::new(rect.right() - 2.0, rect.bottom() - 1.0));
            hud::meter(painter, bar, f32::from(permille) / 1000.0, HEALTHY);
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn quick_rows(
    ui: &mut egui::Ui,
    snapshot: &SimSnapshot,
    db: &BlueprintDb,
    groups: &QuickSelect,
    _color: Color32,
    selected: &[SimId],
    view_center: [f64; 2],
    state: &mut QuickSelectState,
    actions: &mut Vec<UiAction>,
) {
    ui.spacing_mut().item_spacing = Vec2::splat(4.0);
    let (shift, now) = ui.input(|input| (input.modifiers.shift, input.time));
    let key_of = |id: SimId| find_unit(snapshot, id).filter(|unit| unit.blueprint != u16::MAX).map_or_else(String::new, |unit| db.get(BlueprintId(unit.blueprint)).key.clone());
    if let Some(commander) = groups.commander {
        let health = find_unit(snapshot, commander).map(|unit| unit.health_permille);
        if quick_square(ui, db, &key_of(commander), None, selected.contains(&commander), health, 0).on_hover_text("Commander: click to select. The view stays where it is.").clicked() {
            actions.push(UiAction::QuickSelect { units: vec![commander], add: shift });
        }
    }
    for family in &groups.families {
        let open = state.expanded.contains(&family.name);
        let total: usize = family.tiers.iter().map(|tier| tier.units.len()).sum();
        // The family's lowest tier, which the portrait shows.
        let key = family.tiers.first().and_then(|tier| tier.units.first()).map_or_else(|| family.name.to_lowercase().replace(' ', "_"), |id| key_of(*id));
        let idle: Vec<SimId> = family.tiers.iter().flat_map(|tier| &tier.units).copied().filter(|id| snapshot.idle_factories.binary_search(id).is_ok()).collect();
        let shown = if open { "hide" } else { "show" };
        let hint = if idle.is_empty() {
            format!("{}: {total}. Click to {shown} the tech levels.", family.name)
        } else {
            format!("{}: {total}, {} with nothing to build. Click the red tag to select those, or elsewhere to {shown} the tech levels.", family.name, idle.len())
        };
        let square = quick_square(ui, db, &key, Some(total), open, None, idle.len());
        let on_badge = !idle.is_empty() && square.interact_pointer_pos().is_some_and(|at| idle_badge_rect(square.rect, idle.len()).expand(2.0).contains(at));
        if square.on_hover_text(hint).clicked() {
            if on_badge {
                actions.push(UiAction::QuickSelect { units: idle, add: shift });
            } else if open {
                state.expanded.remove(&family.name);
            } else {
                state.expanded.insert(family.name.clone());
            }
        }
        if !open {
            continue;
        }
        for tier in &family.tiers {
            let any_selected = tier.units.iter().any(|id| selected.contains(id));
            let hint = "Click for the closest to your view. Double-click for all.";
            let button = hud::square_button(ui, Vec2::new(QUICK_BUTTON, QUICK_TIER_HEIGHT), any_selected, true, |painter, rect| {
                painter.text(rect.center(), Align2::CENTER_CENTER, tier.label.replace(" x", " \u{d7}"), FontId::proportional(12.0), hud::TEXT);
            });
            if button.on_hover_text(hint).clicked() {
                let row = format!("{} {}", family.name, tier.label.split(' ').next().unwrap_or(""));
                let units = if state.register_click(&row, now) {
                    tier.units.clone()
                } else {
                    closest_unit(snapshot, &tier.units, view_center).into_iter().collect()
                };
                actions.push(UiAction::QuickSelect { units, add: shift });
            }
        }
    }
}

/// The column on the right, under the score panel: select the commander or factories from
/// anywhere, without moving the view.
#[allow(clippy::too_many_arguments)]
pub fn quick_select_bar(
    ctx: &egui::Context,
    snapshot: &SimSnapshot,
    team: Team,
    db: &BlueprintDb,
    selected: &[SimId],
    view_center: [f64; 2],
    state: &mut QuickSelectState,
    actions: &mut Vec<UiAction>,
) {
    let groups = quick_select_groups(snapshot, team, db);
    if groups.commander.is_none() && groups.families.is_empty() {
        return;
    }
    let top = ctx.data(|data| data.get_temp::<f32>(egui::Id::new(BELOW_SCORE))).unwrap_or(QUICK_SELECT_TOP);
    egui::Area::new(egui::Id::new("quick select")).anchor(Align2::RIGHT_TOP, Vec2::new(-8.0, top)).show(ctx, |ui| {
        quick_rows(ui, snapshot, db, &groups, team_color(team), selected, view_center, state, actions);
    });
}

/// How far down the screen the quick select column starts, until the score panel says where it
/// ends.
const QUICK_SELECT_TOP: f32 = 132.0;
const BELOW_SCORE: &str = "below score";

/// Remembers where the score panel ends, so that the quick select column sits under it.
fn note_score_bottom(ctx: &egui::Context, bottom: f32) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(BELOW_SCORE), bottom + 6.0));
}

/// A label that never wraps. A popup near the middle of the screen has little room by default,
/// which would otherwise squeeze its text into a tall, narrow column.
fn one_line_label(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) {
    ui.add(egui::Label::new(text).extend());
}

/// What a control group's row says: its number, and what is in it.
pub fn control_group_label(group: usize, members: &[SimId], snapshot: &SimSnapshot, db: &BlueprintDb) -> String {
    let short_name = |unit: &UnitView| {
        let blueprint = db.get(BlueprintId(unit.blueprint));
        if blueprint.is_commander() { "Commander".to_string() } else { blueprint.name.clone() }
    };
    let mut names = members.iter().filter_map(|id| find_unit(snapshot, *id)).filter(|unit| unit.blueprint != u16::MAX).map(short_name);
    let first = names.next();
    let what = match first {
        Some(name) if names.all(|other| other == name) => {
            if members.len() == 1 { name } else { format!("{name} x{}", members.len()) }
        }
        _ => format!("{} units", members.len()),
    };
    format!("{group}: {what}")
}

fn control_group_rows(ui: &mut egui::Ui, groups: &[(usize, Vec<SimId>)], snapshot: &SimSnapshot, db: &BlueprintDb, _color: Color32, selected: &[SimId], actions: &mut Vec<UiAction>) {
    let shift = ui.input(|input| input.modifiers.shift);
    ui.spacing_mut().item_spacing = Vec2::splat(4.0);
    ui.horizontal(|ui| {
        for (group, members) in groups {
            let key = members.first().and_then(|id| find_unit(snapshot, *id)).filter(|unit| unit.blueprint != u16::MAX).map_or("", |unit| db.get(BlueprintId(unit.blueprint)).key.as_str());
            let any_selected = members.iter().any(|id| selected.contains(id));
            let label = control_group_label(*group, members, snapshot, db);
            let hint = format!("{label}\nClick or press {group} to select. Hold Shift to add. Ctrl + {group} with nothing selected clears it.");
            let button = hud::square_button(ui, Vec2::new(CONTROL_GROUP_WIDTH, CONTROL_GROUP_HEIGHT), any_selected, true, |painter, rect| {
                unit_art(painter, Rect::from_center_size(rect.center() + Vec2::new(0.0, 6.0), Vec2::splat(rect.width() - 2.0)), db, key);
                let bold = |size| FontId::new(size, egui::FontFamily::Name(BOLD.into()));
                hud::outlined_text(painter, rect.left_top() + Vec2::new(3.0, 0.0), Align2::LEFT_TOP, &group.to_string(), bold(16.0), Color32::WHITE);
                hud::outlined_text(painter, rect.right_bottom() + Vec2::new(-2.0, 1.0), Align2::RIGHT_BOTTOM, &members.len().to_string(), bold(12.0), hud::TEXT);
            });
            if button.on_hover_text(hint).clicked() {
                actions.push(UiAction::QuickSelect { units: members.clone(), add: shift });
            }
        }
    });
}

const CONTROL_GROUP_WIDTH: f32 = 46.0;
const CONTROL_GROUP_HEIGHT: f32 = 56.0;

/// The squares under the economy panel: the saved control groups, and what is in them.
pub fn control_group_bar(ctx: &egui::Context, groups: &[(usize, Vec<SimId>)], snapshot: &SimSnapshot, team: Team, db: &BlueprintDb, selected: &[SimId], actions: &mut Vec<UiAction>) {
    if groups.is_empty() {
        return;
    }
    let top = crate::minimap::below_minimap(ctx).unwrap_or(CONTROL_GROUPS_TOP);
    egui::Area::new(egui::Id::new("control groups")).anchor(Align2::LEFT_TOP, Vec2::new(8.0, top)).show(ctx, |ui| {
        control_group_rows(ui, groups, snapshot, db, team_color(team), selected, actions);
    });
}

/// How far down the control groups start, clear of the economy panel, until the minimap says
/// where it ends.
const CONTROL_GROUPS_TOP: f32 = 96.0;

/// The text alert that goes with the "commander under attack" sound.
pub fn commander_alert(ctx: &egui::Context, strength: f32) {
    if strength <= 0.0 {
        return;
    }
    egui::Area::new(egui::Id::new("commander alert"))
        .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 150.0))
        .interactable(false)
        .show(ctx, |ui| {
            let text = egui::RichText::new("Commander under attack").size(22.0).strong().color(STALLED.gamma_multiply(strength));
            egui::Frame::popup(ui.style()).multiply_with_opacity(strength).show(ui, |ui| {
                one_line_label(ui, text);
            });
        });
}

/// Materiel a team has standing, weighted by health, as the balance harness scores a duel.
pub fn materiel_standing(snapshot: &SimSnapshot, blueprints: &BlueprintDb, team: Team) -> f64 {
    snapshot
        .units
        .iter()
        .filter(|unit| unit.team == team)
        .map(|unit| blueprints.get(BlueprintId(unit.blueprint)).mass.to_num::<f64>() * f64::from(unit.health_permille) / 1000.0)
        .sum()
}

/// A playtest scenario's score: each side's share of its materiel still standing, weighted by
/// health, and the harness's score for Blue, the difference. Red's hides in the fog of war, as
/// what the player cannot see cannot be counted.
pub fn scenario_score(ctx: &egui::Context, snapshot: &SimSnapshot, blueprints: &BlueprintDb, start: [f64; 2], fog: bool) {
    let share = |team: Team| materiel_standing(snapshot, blueprints, team) / start[team.index()].max(1.0);
    let blue = share(Team::Blue);
    let text = if fog {
        format!("Blue keeps {:.0}% of its materiel  ·  Red's is hidden by the fog of war", blue * 100.0)
    } else {
        let red = share(Team::Red);
        format!("Blue keeps {:.0}%  ·  Red keeps {:.0}%  ·  score {:+.2}", blue * 100.0, red * 100.0, blue - red)
    };
    egui::Area::new(egui::Id::new("scenario score")).anchor(Align2::CENTER_TOP, Vec2::new(0.0, 48.0)).interactable(false).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            one_line_label(ui, egui::RichText::new(text).size(15.0));
        });
    });
}

/// Nothing in the game takes typing. Without this, Tab would give a button keyboard focus,
/// and it would then swallow every key, so that hotkeys seem to stop working.
pub fn release_keyboard_focus(ctx: &egui::Context) {
    ctx.memory_mut(|memory| {
        if let Some(focused) = memory.focused() {
            memory.surrender_focus(focused);
        }
    });
}

/// The match clock, as hours, minutes, and seconds.
pub fn clock_text(tick: u32) -> String {
    let seconds = tick / 10;
    format!("{:02}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60)
}

/// How wide the economy panel's contents are, in points.
const ECONOMY_WIDTH: f32 = 340.0;
/// How tall one resource's row is.
const ECONOMY_ROW: f32 = 30.0;

/// One resource's row: its mark, how full its store is, the net rate in large figures, and income
/// over spending at the end. Lit red while it is what stalls building.
#[allow(clippy::too_many_arguments)]
fn economy_row(ui: &mut egui::Ui, energy: bool, stored: i64, capacity: i64, income: i64, demand: i64, reclaim: (i64, i64), stalling: bool, hint: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ECONOMY_WIDTH, ECONOMY_ROW), egui::Sense::hover());
    let painter = ui.painter();
    if stalling {
        let pulse = 0.5 + 0.5 * (ui.input(|input| input.time) * 4.0).sin() as f32;
        painter.rect_filled(rect.expand2(Vec2::new(4.0, 1.0)), 2.0, hud::LOSS.gamma_multiply(0.18 + 0.12 * pulse));
    }
    let color = if energy { hud::ENERGY } else { hud::MASS };
    hud::resource_mark(painter, Pos2::new(rect.left() + 9.0, rect.center().y - 1.0), 8.5, energy);
    let bar = Rect::from_min_size(Pos2::new(rect.left() + 24.0, rect.top() + 4.0), Vec2::new(118.0, 6.0));
    let share = if capacity > 0 { stored as f32 / capacity as f32 } else { 0.0 };
    hud::meter(painter, bar, share, color);
    let small = FontId::proportional(11.0);
    painter.text(Pos2::new(bar.left(), bar.bottom() + 2.0), Align2::LEFT_TOP, stored.to_string(), small.clone(), hud::TEXT);
    painter.text(Pos2::new(bar.right(), bar.bottom() + 2.0), Align2::RIGHT_TOP, capacity.to_string(), small.clone(), hud::DIM_TEXT);
    let net = income - demand;
    let net_color = if net < 0 { hud::LOSS } else if net == 0 { hud::TEXT } else { hud::GAIN };
    painter.text(Pos2::new(bar.right() + 8.0, rect.center().y), Align2::LEFT_CENTER, hud::signed(net), FontId::proportional(22.0), net_color);
    // Reclaim: what it brings in now, over all it has brought in.
    let (rate, total) = reclaim;
    painter.text(Pos2::new(rect.right() - 54.0, rect.top() + 1.0), Align2::RIGHT_TOP, format!("+{rate}"), small.clone(), hud::DIM_TEXT);
    painter.text(Pos2::new(rect.right() - 54.0, rect.bottom() - 1.0), Align2::RIGHT_BOTTOM, hud::compact(total), small.clone(), hud::DIM_TEXT);
    painter.text(Pos2::new(rect.right(), rect.top() + 1.0), Align2::RIGHT_TOP, format!("+{income}"), small.clone(), hud::GAIN);
    painter.text(Pos2::new(rect.right(), rect.bottom() - 1.0), Align2::RIGHT_BOTTOM, format!("-{demand}"), small, hud::LOSS);
    if !hint {
        return response;
    }
    response.on_hover_text(economy_summary(energy, stored, capacity, income, demand, reclaim))
}

/// What hovering over a resource says about it, before what it is spent on.
fn economy_summary(energy: bool, stored: i64, capacity: i64, income: i64, demand: i64, (rate, total): (i64, i64)) -> String {
    let name = if energy { "Energy" } else { "Materiel" };
    format!(
        "{name}: {stored} stored of {capacity}.\nNet {} a second: {income} coming in, {demand} going out.\nReclaim: {rate} a second now, {total} in all.",
        hud::signed(income - demand)
    )
}

/// Both resources, one row each, and what a stall slows building to. `title` names whose
/// economy it is, when watching. With `hints`, hovering a row says what it shows; without, the
/// breakdown of what spends it says so instead.
fn economy_rows(ui: &mut egui::Ui, economy: &EconomyView, title: Option<(&str, Color32)>, hints: bool) -> [egui::Response; 2] {
    ui.spacing_mut().item_spacing.y = 2.0;
    if let Some((name, color)) = title {
        ui.label(egui::RichText::new(name).color(color).strong().size(12.0));
    }
    let stalled = economy.satisfaction_permille < 1000;
    let rows = [
        economy_row(ui, false, economy.mass, economy.mass_capacity, economy.mass_income, economy.mass_demand, (economy.mass_reclaim, economy.mass_reclaimed), stalled && economy.mass == 0, hints),
        economy_row(ui, true, economy.energy, economy.energy_capacity, economy.energy_income, economy.energy_demand, (economy.energy_reclaim, economy.energy_reclaimed), stalled && economy.energy == 0, hints),
    ];
    if stalled {
        ui.label(egui::RichText::new(format!("Stalled: building at {}%", economy.satisfaction_permille / 10)).color(STALLED).size(12.0));
    }
    rows
}

/// Everything of one kind that spends a resource, and how much it would at full speed, in tenths
/// a second.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpendingGroup {
    pub name: String,
    pub units: Vec<SimId>,
    pub tenths: i32,
}

/// What spends one resource, by kind of unit, the most first. Kinds spending the same stay in
/// blueprint order.
pub fn spending_groups(snapshot: &SimSnapshot, db: &BlueprintDb, energy: bool) -> Vec<SpendingGroup> {
    let mut kinds: BTreeMap<u16, (Vec<SimId>, i32)> = BTreeMap::new();
    for spending in &snapshot.spending {
        let amount = if energy { spending.energy } else { spending.mass };
        let Some(unit) = find_unit(snapshot, spending.id).filter(|unit| amount > 0 && unit.blueprint != u16::MAX) else {
            continue;
        };
        let kind = kinds.entry(unit.blueprint).or_default();
        kind.0.push(spending.id);
        kind.1 += amount;
    }
    let mut groups: Vec<SpendingGroup> = kinds
        .into_iter()
        .map(|(blueprint, (units, tenths))| {
            let name = &db.get(BlueprintId(blueprint)).name;
            let name = if units.len() > 1 { format!("{name} \u{d7}{}", units.len()) } else { name.clone() };
            SpendingGroup { name, units, tenths }
        })
        .collect();
    groups.sort_by_key(|group| std::cmp::Reverse(group.tenths));
    groups
}

/// A rate in tenths a second as the breakdown shows it: whole, unless it is under ten.
pub fn breakdown_amount(tenths: i32) -> String {
    if tenths < 100 {
        per_second(tenths)
    } else {
        ((tenths + 5) / 10).to_string()
    }
}

/// Which resource's breakdown is open, `true` for energy: a row opens its own, and the one open
/// stays so while the pointer is on the panel between rows, or on the breakdown itself.
pub const fn breakdown_shown(open: Option<bool>, row: Option<bool>, on_panel: bool, on_breakdown: bool) -> Option<bool> {
    match row {
        Some(_) => row,
        None if on_panel || on_breakdown => open,
        None => None,
    }
}

/// How many kinds the breakdown lists, and how wide it is.
const BREAKDOWN_KINDS: usize = 6;
const BREAKDOWN_WIDTH: f32 = 300.0;
const BREAKDOWN: &str = "economy breakdown";

/// Beside the economy panel while a resource is hovered: what it is spent on, by kind, the most
/// first. Clicking a kind selects those units.
fn economy_breakdown(ctx: &egui::Context, snapshot: &SimSnapshot, db: &BlueprintDb, panel: Rect, rows: &[egui::Response; 2], actions: &mut Vec<UiAction>) {
    let id = egui::Id::new(BREAKDOWN);
    let (open, shown_at) = ctx.data(|data| data.get_temp::<(Option<bool>, Option<Rect>)>(id)).unwrap_or_default();
    let pointer = ctx.pointer_hover_pos();
    let on = |rect: Rect| pointer.is_some_and(|at| rect.expand(10.0).contains(at));
    let row = rows.iter().position(egui::Response::hovered).map(|index| index == 1);
    let Some(energy) = breakdown_shown(open, row, on(panel), shown_at.is_some_and(on)) else {
        ctx.data_mut(|data| data.insert_temp(id, (None::<bool>, None::<Rect>)));
        return;
    };
    let top = rows[usize::from(energy)].rect.top() - 8.0;
    let shown = egui::Area::new(egui::Id::new("economy breakdown panel"))
        .order(egui::Order::Foreground)
        .fixed_pos(Pos2::new(panel.right() + 4.0, top))
        .constrain(true)
        .show(ctx, |ui| hud::framed(ui, Vec2::new(10.0, 8.0), |ui| breakdown_contents(ui, snapshot, db, energy, actions)));
    ctx.data_mut(|data| data.insert_temp(id, (Some(energy), Some(shown.response.rect))));
}

fn breakdown_contents(ui: &mut egui::Ui, snapshot: &SimSnapshot, db: &BlueprintDb, energy: bool, actions: &mut Vec<UiAction>) {
    ui.set_width(BREAKDOWN_WIDTH);
    ui.spacing_mut().item_spacing.y = 2.0;
    let economy = &snapshot.economy;
    let (stored, capacity, income, demand, reclaim) = if energy {
        (economy.energy, economy.energy_capacity, economy.energy_income, economy.energy_demand, (economy.energy_reclaim, economy.energy_reclaimed))
    } else {
        (economy.mass, economy.mass_capacity, economy.mass_income, economy.mass_demand, (economy.mass_reclaim, economy.mass_reclaimed))
    };
    ui.label(egui::RichText::new(economy_summary(energy, stored, capacity, income, demand, reclaim)).size(12.0).color(hud::DIM_TEXT));
    ui.add_space(4.0);
    let title = if energy { "ENERGY GOES TO" } else { "MATERIEL GOES TO" };
    ui.label(egui::RichText::new(title).size(11.0).strong().color(hud::DIM_TEXT));
    let groups = spending_groups(snapshot, db, energy);
    if groups.is_empty() {
        ui.label(egui::RichText::new("Nothing is spending it.").size(12.0).color(hud::TEXT));
    }
    let color = if energy { hud::ENERGY } else { hud::MASS };
    let most = groups.first().map_or(1, |group| group.tenths.max(1));
    let shift = ui.input(|input| input.modifiers.shift);
    for group in groups.iter().take(BREAKDOWN_KINDS) {
        let (rect, response) = ui.allocate_exact_size(Vec2::new(BREAKDOWN_WIDTH, 20.0), egui::Sense::click());
        let painter = ui.painter();
        if response.hovered() {
            painter.rect_filled(rect, 2.0, hud::GLOW.gamma_multiply(0.16));
        }
        let name = Rect::from_min_max(rect.left_top(), Pos2::new(rect.right() - 120.0, rect.bottom()));
        painter.with_clip_rect(name).text(Pos2::new(name.left() + 6.0, rect.center().y), Align2::LEFT_CENTER, &group.name, FontId::proportional(13.0), hud::TEXT);
        let bar = Rect::from_min_size(Pos2::new(rect.right() - 112.0, rect.center().y - 3.0), Vec2::new(70.0, 6.0));
        hud::meter(painter, bar, group.tenths as f32 / most as f32, color);
        painter.text(Pos2::new(rect.right() - 6.0, rect.center().y), Align2::RIGHT_CENTER, breakdown_amount(group.tenths), FontId::proportional(12.0), hud::TEXT);
        if response.clicked() {
            actions.push(UiAction::QuickSelect { units: group.units.clone(), add: shift });
        }
    }
    if groups.len() > BREAKDOWN_KINDS {
        ui.label(egui::RichText::new(format!("and {} more kinds", groups.len() - BREAKDOWN_KINDS)).size(11.0).color(hud::DIM_TEXT));
    }
    ui.add_space(2.0);
    // Only an empty store holds building back.
    let short = if stored == 0 { demand - income - reclaim.0 } else { 0 };
    let footer = if short > 0 { format!("Click a kind to select it. Short by {short} a second.") } else { "Click a kind to select it.".to_string() };
    ui.label(egui::RichText::new(footer).size(11.0).color(if short > 0 { hud::LOSS } else { hud::DIM_TEXT }));
}

/// A team's row in the score panel: its color and name, then its figures in columns, each
/// right-aligned under the header's: units, mass income, army value, and how it is doing.
fn score_row(ui: &mut egui::Ui, color: Color32, name: &str, figures: [String; 3], state: (&str, Color32)) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(SCORE_WIDTH, 17.0), egui::Sense::hover());
    let painter = ui.painter();
    let swatch = Rect::from_center_size(Pos2::new(rect.left() + 6.0, rect.center().y), Vec2::splat(9.0));
    painter.rect(swatch, 1.0, color, Stroke::new(1.0_f32, Color32::from_black_alpha(160)));
    painter.text(Pos2::new(rect.left() + 16.0, rect.center().y), Align2::LEFT_CENTER, name, FontId::proportional(13.0), hud::TEXT);
    for (figure, column) in figures.iter().zip(SCORE_COLUMNS) {
        painter.text(Pos2::new(rect.left() + column, rect.center().y), Align2::RIGHT_CENTER, figure, FontId::proportional(12.0), hud::TEXT);
    }
    painter.text(Pos2::new(rect.right(), rect.center().y), Align2::RIGHT_CENTER, state.0, FontId::proportional(12.0), state.1);
}

/// The score panel's column headings, over the figures of `score_row`.
fn score_columns(ui: &mut egui::Ui) {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(SCORE_WIDTH, 14.0), egui::Sense::hover());
    let painter = ui.painter();
    for (heading, column) in ["Units", "Materiel/s", "Army"].iter().zip(SCORE_COLUMNS) {
        painter.text(Pos2::new(rect.left() + column, rect.center().y), Align2::RIGHT_CENTER, heading, FontId::proportional(10.5), hud::DIM_TEXT);
    }
    response.on_hover_text("Units: how many it has that you can see. Materiel/s: its materiel income. Army: the materiel its mobile units cost. A team out of sight shows only what you can see of it.");
}

/// Where each figure of a score row ends, from the row's left.
const SCORE_COLUMNS: [f32; 3] = [150.0, 196.0, 240.0];

const SCORE_WIDTH: f32 = 300.0;

/// A team's figures for its score row: units that can be seen, its mass income if known, and
/// the mass its mobile units that can be seen cost.
fn team_figures(snapshot: &SimSnapshot, team: Team, db: &BlueprintDb, income: Option<i64>) -> [String; 3] {
    let units: Vec<&UnitView> = snapshot.units.iter().filter(|unit| unit.team == team && unit.blueprint != u16::MAX).collect();
    let army: i64 = units.iter().filter(|unit| !unit.is_structure).map(|unit| db.get(BlueprintId(unit.blueprint)).mass.to_num::<i64>()).sum();
    [units.len().to_string(), income.map_or_else(|| "?".to_string(), |income| format!("+{income}")), hud::compact(army)]
}

/// The score panel's first line: the clock, and on the right `right`.
fn score_header(ui: &mut egui::Ui, tick: u32, right: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(SCORE_WIDTH, 18.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.circle_stroke(Pos2::new(rect.left() + 6.0, rect.center().y), 5.5, Stroke::new(1.4_f32, hud::CLOCK));
    painter.line_segment([Pos2::new(rect.left() + 6.0, rect.center().y), Pos2::new(rect.left() + 6.0, rect.center().y - 3.5)], Stroke::new(1.4_f32, hud::CLOCK));
    painter.text(Pos2::new(rect.left() + 16.0, rect.center().y), Align2::LEFT_CENTER, clock_text(tick), FontId::proportional(13.0), hud::CLOCK);
    painter.text(Pos2::new(rect.right(), rect.center().y), Align2::RIGHT_CENTER, right, FontId::proportional(12.0), hud::DIM_TEXT);
}

/// A thin line across a panel.
fn panel_rule(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(SCORE_WIDTH, 5.0), egui::Sense::hover());
    ui.painter().line_segment([rect.left_center(), rect.right_center()], Stroke::new(1.0_f32, hud::RIM.gamma_multiply(0.5)));
}

/// "Playing", or how the match ended for a team.
fn team_state(result: Option<MatchResult>, team: Team) -> (&'static str, Color32) {
    match result {
        Some(MatchResult::Winner(winner)) if winner == team => ("Victorious", hud::GAIN),
        Some(_) => ("Defeated", hud::DIM_TEXT),
        None => ("Playing", hud::TEXT),
    }
}

fn paused_banner(ctx: &egui::Context) {
    egui::Area::new(egui::Id::new("paused")).anchor(Align2::CENTER_TOP, Vec2::new(0.0, 136.0)).interactable(false).show(ctx, |ui| {
        ui.label(egui::RichText::new("PAUSED").size(32.0).strong().color(Color32::from_rgb(255, 230, 140)));
    });
}

/// The panels along the top of the screen: the economy at the top left, game controls in the
/// middle, and the clock, the players, and the map at the top right.
#[allow(clippy::too_many_arguments)]
pub fn resource_bar(ctx: &egui::Context, snapshot: &SimSnapshot, db: &BlueprintDb, map: &str, sound: Option<&str>, paused: bool, ranges: &mut RangeToggles, actions: &mut Vec<UiAction>) {
    if paused {
        paused_banner(ctx);
    }
    let (rows, economy) = hud::panel_rect(ctx, "economy", Align2::LEFT_TOP, Vec2::new(4.0, 4.0), |ui| economy_rows(ui, &snapshot.economy, None, false));
    crate::minimap::note_economy_bottom(ctx, economy.bottom());
    economy_breakdown(ctx, snapshot, db, economy, &rows, actions);
    hud::panel(ctx, "game controls", Align2::CENTER_TOP, Vec2::new(0.0, 4.0), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (glyph, hint) = if paused { (Glyph::Resume, "[Ctrl+P] Resume\nRestart the game clock.") } else { (Glyph::Pause, "[Ctrl+P] Pause\nStop the game clock. Orders given while paused are carried out on resuming.") };
            if glyph_button(ui, glyph, "", paused, hint).clicked() {
                actions.push(UiAction::TogglePause);
            }
            if let Some(sound) = sound {
                hud::square_button(ui, Vec2::splat(ORDER_BUTTON), false, true, |painter, rect| hud::draw_glyph(painter, rect.shrink(2.0), Glyph::Sound)).on_hover_text(sound);
            }
            range_menu(ui, ranges);
        });
    });
    let ((), score) = hud::panel_rect(ctx, "score", Align2::RIGHT_TOP, Vec2::new(-4.0, 4.0), |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let mine = snapshot.units.iter().filter(|unit| unit.team == Team::Blue && unit.blueprint != u16::MAX).count();
        score_header(ui, snapshot.tick, &format!("{mine} units"));
        panel_rule(ui);
        score_columns(ui);
        score_row(ui, BLUE, "Blue (you)", team_figures(snapshot, Team::Blue, db, Some(snapshot.economy.mass_income)), team_state(snapshot.match_result, Team::Blue));
        score_row(ui, RED, "Red (AI)", team_figures(snapshot, Team::Red, db, None), team_state(snapshot.match_result, Team::Red));
        panel_rule(ui);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(format!("Map: {map}")).size(12.0).color(hud::DIM_TEXT));
        });
    });
    note_score_bottom(ctx, score.bottom());
}

/// The height of the command bar along the bottom, in points.
pub const COMMAND_BAR_HEIGHT: f32 = 108.0;
/// The width of the order box at the command bar's left, for eight buttons a row.
const ORDER_BOX_WIDTH: f32 = 8.0 * ORDER_BUTTON + 7.0 * 4.0 + 20.0;
/// The size of a tab at the left of the command bar's main area: three stack in its height.
const TAB_WIDTH: f32 = 30.0;
const TAB_HEIGHT: f32 = 26.0;
/// The width of the selection's details panel, above the order box.
const INFO_WIDTH: f32 = ORDER_BOX_WIDTH - 22.0;

/// The selection, grouped by kind, in the order each kind was first selected.
pub fn selection_groups(snapshot: &SimSnapshot, selected: &[SimId]) -> Vec<(u16, Vec<SimId>)> {
    let mut groups: Vec<(u16, Vec<SimId>)> = Vec::new();
    for unit in selected.iter().filter_map(|id| find_unit(snapshot, *id)).filter(|unit| unit.blueprint != u16::MAX) {
        match groups.iter_mut().find(|(blueprint, _)| *blueprint == unit.blueprint) {
            Some((_, units)) => units.push(unit.id),
            None => groups.push((unit.blueprint, vec![unit.id])),
        }
    }
    groups
}

/// What is selected, in a panel above the order box: its symbol, name, health, what it makes and
/// spends, and anything under way, such as an upgrade.
fn selection_info(ui: &mut egui::Ui, snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb, menu: Option<&FactoryMenu>) {
    let units: Vec<&UnitView> = selected.iter().filter_map(|id| find_unit(snapshot, *id)).collect();
    let Some(first) = units.first() else {
        return;
    };
    ui.set_width(INFO_WIDTH);
    ui.spacing_mut().item_spacing.y = 3.0;
    if first.blueprint == u16::MAX {
        ui.label(format!("{} units selected", units.len()));
        return;
    }
    let unit = db.get(BlueprintId(first.blueprint));
    let same_kind = units.iter().all(|other| other.blueprint == first.blueprint);
    ui.horizontal(|ui| {
        let (portrait, _) = ui.allocate_exact_size(Vec2::splat(52.0), egui::Sense::hover());
        for shape in hud::face_shapes(portrait, false, false, true) {
            ui.painter().add(shape);
        }
        unit_art(ui.painter(), portrait.shrink(1.0), db, &unit.key);
        // How many enemies it has destroyed, or the selection all together.
        let kills: u32 = units.iter().map(|unit| kills_of(snapshot, unit.id)).sum();
        kills_badge(ui.painter(), portrait, kills);
        ui.interact(portrait, ui.id().with("kills"), egui::Sense::hover())
            .on_hover_text(if units.len() == 1 { format!("Destroyed {kills} enemy units") } else { format!("Destroyed {kills} enemy units between them") });
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let width = ui.available_width() - if menu.is_some_and(|menu| !menu.queue.is_empty()) { 52.0 } else { 0.0 };
            let title = match (units.len(), same_kind) {
                (1, _) => unit.name.clone(),
                (count, true) => format!("{} \u{d7}{count}", unit.name),
                (count, false) => format!("{count} units"),
            };
            ui.label(egui::RichText::new(title).strong().size(14.0).color(hud::TEXT));
            // Health, as a bar with the figures on it for one unit, or the average for several.
            let permille = units.iter().map(|unit| u32::from(unit.health_permille)).sum::<u32>() / units.len() as u32;
            let (bar, _) = ui.allocate_exact_size(Vec2::new(width.max(80.0), 13.0), egui::Sense::hover());
            hud::meter(ui.painter(), bar, permille as f32 / 1000.0, HEALTHY);
            let text = if units.len() == 1 {
                let most = unit.health.to_num::<i64>();
                format!("{} / {most}", most * i64::from(permille) / 1000)
            } else {
                format!("{}%", permille / 10)
            };
            hud::outlined_text(ui.painter(), bar.center(), Align2::CENTER_CENTER, &text, FontId::proportional(11.0), Color32::WHITE);
            if units.len() == 1 {
                let rank = rank_of(snapshot, first.id);
                let mut notes: Vec<String> = Vec::new();
                if rank > 0 {
                    notes.push(format!("veteran rank {rank}"));
                }
                notes.extend(shield_status(snapshot, first.id));
                notes.extend(stock_status(snapshot, first.id, db));
                notes.extend(intel_status(snapshot, first.id));
                notes.extend(cargo_status(snapshot, first.id));
                if !notes.is_empty() {
                    ui.label(egui::RichText::new(notes.join(", ")).size(11.0).color(hud::DIM_TEXT));
                }
            }
            // What it makes or uses, and its build power, each by its mark.
            let net = selection_net(snapshot, selected);
            let power = selection_build_power(snapshot, selected, db);
            if net.is_some() || power > 0 {
                let rates = selection_rates(snapshot, selected, db);
                let hint = [rates.output, rates.spending].into_iter().filter(|line| !line.is_empty()).collect::<Vec<_>>().join("\n");
                let (row, _) = ui.allocate_exact_size(Vec2::new(width.max(80.0), 16.0), egui::Sense::hover());
                let painter = ui.painter();
                let font = FontId::proportional(12.0);
                let mut x = row.left() + 7.0;
                let figure = |painter: &egui::Painter, x: &mut f32, text: String, color: Color32| {
                    let galley = painter.layout_no_wrap(text, font.clone(), color);
                    let width = galley.size().x;
                    painter.galley(Pos2::new(*x, row.center().y - galley.size().y / 2.0), galley, color);
                    *x += width + 12.0;
                };
                if let Some((mass, energy)) = net {
                    hud::resource_mark(painter, Pos2::new(x, row.center().y), 6.0, false);
                    x += 9.0;
                    figure(painter, &mut x, signed_tenths(mass), if mass < 0 { hud::LOSS } else { hud::GAIN });
                    hud::resource_mark(painter, Pos2::new(x, row.center().y), 6.0, true);
                    x += 9.0;
                    figure(painter, &mut x, signed_tenths(energy), if energy < 0 { hud::LOSS } else { hud::GAIN });
                }
                if power > 0 {
                    hud::draw_glyph(painter, Rect::from_center_size(Pos2::new(x + 2.0, row.center().y), Vec2::splat(15.0)), Glyph::Repair);
                    x += 11.0;
                    figure(painter, &mut x, power.to_string(), hud::TEXT);
                }
                if !hint.is_empty() {
                    ui.interact(row, ui.id().with("selection rates"), egui::Sense::hover()).on_hover_text(hint);
                }
            }
        });
        // What the factories are making, and how far along.
        if let Some((item, _)) = menu.and_then(|menu| menu.queue.first()) {
            ui.vertical(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(44.0), egui::Sense::hover());
                for shape in hud::face_shapes(rect, false, false, true) {
                    ui.painter().add(shape);
                }
                unit_art(ui.painter(), rect.shrink(1.0), db, &db.get(*item).key);
                let progress = menu.and_then(|menu| menu.progress_permille).unwrap_or(0);
                ui.label(egui::RichText::new(format!("{}%", progress / 10)).size(12.0).color(hud::TEXT));
            });
        }
    });

    for &(id, permille) in snapshot.upgrades.iter().filter(|(id, _)| selected.contains(id)) {
        if let Some(unit) = find_unit(snapshot, id) {
            let into = db.get(BlueprintId(unit.blueprint)).upgrades_to.map_or("", |into| db.get(into).name.as_str());
            ui.colored_label(UPGRADING, format!("Upgrading to {into}: {}%", permille / 10));
        }
    }
    for &(id, enhancement, permille) in snapshot.enhancing.iter().filter(|(id, _, _)| selected.contains(id)) {
        if find_unit(snapshot, id).is_some() {
            ui.colored_label(SPENDING, format!("Enhancing: {} {}%", db.enhancement(EnhancementId(enhancement)).name, permille / 10));
        }
    }
    for &(_, _, permille) in snapshot.teleports.iter().filter(|(id, _, _)| selected.contains(id)) {
        ui.colored_label(SPENDING, format!("Teleport charging: {}%", permille / 10));
    }
    for &(_, ticks) in snapshot.self_destructs.iter().filter(|(id, _)| selected.contains(id)) {
        let seconds = (i64::from(ticks) + TICKS_PER_SECOND - 1) / TICKS_PER_SECOND;
        ui.colored_label(Color32::from_rgb(255, 90, 70), format!("Self-destruct in {seconds} s (Ctrl+K to cancel)"));
    }
    if selected.iter().any(|id| snapshot.paused.binary_search(id).is_ok()) {
        ui.colored_label(Color32::YELLOW, "Paused");
    }
    if let Some(menu) = menu {
        if let Some(note) = construction_note(menu.unfinished, menu.factories.len()) {
            ui.label(egui::RichText::new(note).size(11.0).color(hud::DIM_TEXT));
        }
        if menu.queue.is_empty() {
            // A missile submarine has a queue for its missiles, but no rally point: it moves.
            let moves = menu.factories.iter().any(|id| find_unit(snapshot, *id).is_some_and(|unit| unit.blueprint != u16::MAX && !db.get(BlueprintId(unit.blueprint)).is_structure()));
            let hint = if moves { "Queue is empty." } else { "Queue is empty. Right-click the ground to set a rally point." };
            ui.label(egui::RichText::new(hint).size(11.0).color(hud::DIM_TEXT));
        }
    }
}

/// The toggles after the orders: pause, cloak, upgrade, and a factory's repeat and clear.
fn toggle_buttons(ui: &mut egui::Ui, snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb, menu: Option<&FactoryMenu>, actions: &mut Vec<UiAction>) {
    let pause = pause_action(snapshot, selected, db);
    if let Some(action) = &pause {
        let UiAction::SetPaused { paused, .. } = action else {
            unreachable!("pause_action only returns a pause");
        };
        let (glyph, name) = if *paused { (Glyph::Pause, "Pause") } else { (Glyph::Resume, "Resume") };
        let hint = format!("[{}] {name}\nStops spending and making progress, keeping the queue and what is done so far.", hotkeys::PAUSE);
        if glyph_button(ui, glyph, &hotkeys::PAUSE.to_string(), !*paused, &hint).clicked() {
            actions.push(action.clone());
        }
    }
    if let Some(action) = cloak_action(snapshot, selected) {
        let UiAction::SetCloak { on, .. } = &action else {
            unreachable!("cloak_action only returns a cloak");
        };
        // O pauses first, when there is something to pause.
        let key = if pause.is_none() { hotkeys::PAUSE.to_string() } else { String::new() };
        let name = if *on { "Cloak" } else { "Uncloak" };
        let hint = format!("{name}\nHidden from sight and radar, except from enemies within 100 or with deep scan on it. Costs energy while on.");
        if glyph_button(ui, Glyph::Cloak, &key, !*on, &hint).clicked() {
            actions.push(action);
        }
    }
    if let Some(action) = surface_action(snapshot, selected, db) {
        let UiAction::SetSurfaced { surfaced, .. } = &action else {
            unreachable!("surface_action only returns a surfacing");
        };
        let (glyph, hint) = if *surfaced {
            (Glyph::Surface, "Surface\nBring them up onto the water: deck guns fire, and they are seen and fired on like ships.")
        } else {
            (Glyph::Dive, "Dive\nTake them back down, where only torpedoes reach them and only sonar finds them.")
        };
        if glyph_button(ui, glyph, "", !*surfaced, hint).clicked() {
            actions.push(action);
        }
    }
    let key = hotkeys::UPGRADE.to_string();
    if let Some((upgrade, able)) = menu.and_then(|menu| menu.upgrade.as_ref()) {
        let menu = menu.expect("an upgrade comes from a factory menu");
        let partial = if able.len() < menu.factories.len() { format!("{} of {} factories can build this.\n", able.len(), menu.factories.len()) } else { String::new() };
        let hint = format!("[{key}] Upgrade to T{}\n{partial}{}", db.get(*upgrade).tier, item_details(db, *upgrade));
        if glyph_button(ui, Glyph::Upgrade, &key, false, &hint).clicked() {
            actions.push(queue_action(menu, able, *upgrade));
        }
    } else {
        let upgradable = upgradable_units(snapshot, selected, db);
        if let Some(first) = upgradable.first().and_then(|id| find_unit(snapshot, *id)) {
            let into = db.get(db.get(BlueprintId(first.blueprint)).upgrades_to.expect("only upgradable units are listed"));
            let hint = format!(
                "[{key}] Upgrade: {}\n{} materiel  {} energy\nA commander stands still while it upgrades, and a new order cancels it.",
                into.name,
                into.mass.to_num::<i64>(),
                into.energy.to_num::<i64>()
            );
            if glyph_button(ui, Glyph::Upgrade, &key, false, &hint).clicked() {
                actions.push(UiAction::Upgrade { units: upgradable });
            }
        }
    }
    if let Some(menu) = menu {
        let mixed = if menu.repeat == RepeatState::Mixed { "\nSome of the selected factories repeat and some do not." } else { "" };
        let hint = format!("[{}] Repeat\nBuild the queue over and over.{mixed}", hotkeys::REPEAT);
        if glyph_button(ui, Glyph::Repeat, &hotkeys::REPEAT.to_string(), menu.repeat == RepeatState::On, &hint).clicked() {
            actions.push(repeat_action(menu));
        }
        let hint = format!("[{}] Clear queue", hotkeys::CLEAR_QUEUE);
        if glyph_button(ui, Glyph::ClearQueue, &hotkeys::CLEAR_QUEUE.to_string(), false, &hint).clicked() {
            actions.push(clear_action(menu));
        }
    }
}

/// The selection's kinds, as icons with how many of each: clicking one selects only those.
fn selection_group_bar(ui: &mut egui::Ui, snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb, actions: &mut Vec<UiAction>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (blueprint, units) in selection_groups(snapshot, selected) {
            let unit = db.get(BlueprintId(blueprint));
            let button = hud::square_button(ui, Vec2::splat(BAR_BUTTON), false, true, |painter, rect| {
                unit_art(painter, rect, db, &unit.key);
                hud::outlined_text(painter, rect.right_bottom() + Vec2::new(-2.0, 1.0), Align2::RIGHT_BOTTOM, &units.len().to_string(), FontId::new(15.0, egui::FontFamily::Name(BOLD.into())), Color32::WHITE);
            });
            if button.on_hover_text(format!("{} \u{d7}{}\nClick to select only these.", unit.name, units.len())).clicked() {
                actions.push(UiAction::QuickSelect { units, add: false });
            }
        }
    });
}

/// The bar along the bottom: orders and toggles at the left, then what the selection can build,
/// or what it is made of, over its queue. Above the orders, a panel says what is selected.
pub fn command_panel(ctx: &egui::Context, snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb, armed: Option<ArmedOrder>, actions: &mut Vec<UiAction>) {
    let menu = factory_menu(snapshot, selected, db);
    let known = selected.iter().filter_map(|id| find_unit(snapshot, *id)).any(|unit| unit.blueprint != u16::MAX);
    if !selected.is_empty() {
        egui::Area::new(egui::Id::new("selection info")).anchor(Align2::LEFT_BOTTOM, Vec2::new(4.0, -(COMMAND_BAR_HEIGHT + 2.0))).show(ctx, |ui| {
            hud::framed(ui, Vec2::new(11.0, 9.0), |ui| selection_info(ui, snapshot, selected, db, menu.as_ref()));
        });
    }
    egui::TopBottomPanel::bottom("commands").frame(egui::Frame::none()).show_separator_line(false).resizable(false).exact_height(COMMAND_BAR_HEIGHT).show(ctx, |ui| {
        let full = ui.max_rect();
        let orders = Rect::from_min_size(full.min + Vec2::new(4.0, 2.0), Vec2::new(ORDER_BOX_WIDTH, full.height() - 6.0));
        let main = Rect::from_min_max(Pos2::new(orders.right() + 6.0, orders.top()), Pos2::new(full.right() - 4.0, orders.bottom()));
        for shape in hud::panel_shapes(orders).into_iter().chain(hud::panel_shapes(main)) {
            ui.painter().add(shape);
        }
        ui.allocate_ui_at_rect(orders.shrink2(Vec2::new(10.0, 10.0)), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(4.0);
                if has_mobile_units(snapshot, selected) {
                    let offers = OrderOffers {
                        discharge: has_discharge(snapshot, selected, db),
                        weapons: has_weapons(snapshot, selected, db),
                        teleport: !teleporters(snapshot, selected, db).is_empty(),
                        capture: has_mobile_builders(snapshot, selected, db),
                        unload: !loaded_transports(snapshot, selected).is_empty(),
                    };
                    unit_order_buttons(ui, armed, &offers, actions);
                }
                toggle_buttons(ui, snapshot, selected, db, menu.as_ref(), actions);
            });
        });
        ui.allocate_ui_at_rect(main.shrink2(Vec2::new(10.0, 8.0)), |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            if !known {
                ui.label(egui::RichText::new(if selected.is_empty() { "Nothing selected" } else { "Selected units out of sight" }).color(hud::DIM_TEXT));
                return;
            }
            let options = builder_options(snapshot, selected, db);
            let tabs = command_tabs(menu.is_some() || !options.is_empty(), !enhancement_offers(snapshot, selected, db).is_empty());
            let memory = egui::Id::new(COMMAND_TAB_MEMORY);
            let tab = shown_tab(ui.ctx().data(|data| data.get_temp::<CommandTab>(memory)), &tabs);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                // The tabs, stacked, when there is more than one.
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for &each in &tabs {
                        let (glyph, hint) = match each {
                            CommandTab::Build => (Glyph::Repair, "Build: what the selection can build"),
                            CommandTab::Selection => (Glyph::Formation, "Selection: what is selected, by kind. Click a kind to select only those."),
                            CommandTab::Enhance => (Glyph::Upgrade, "Enhance: the commander's enhancements"),
                        };
                        let button = hud::square_button(ui, Vec2::new(TAB_WIDTH, TAB_HEIGHT), each == tab, true, |painter, rect| hud::draw_glyph(painter, rect.shrink(1.0), glyph));
                        if button.on_hover_text(hint).clicked() {
                            ui.ctx().data_mut(|data| data.insert_temp(memory, each));
                        }
                    }
                });
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    match tab {
                        // One bar: a factory's, if one is selected, else the builders'.
                        CommandTab::Build => match &menu {
                            Some(menu) => factory_controls(ui, menu, db, actions),
                            None => {
                                if let Some(BarClick::Left(option)) = build_bar(ui, BUILDER_TIER_MEMORY, &options, db, |_| String::new(), |_| String::new()) {
                                    actions.push(UiAction::BeginPlacement(option));
                                }
                            }
                        },
                        CommandTab::Selection => selection_group_bar(ui, snapshot, selected, db, actions),
                        CommandTab::Enhance => enhancement_bar(ui, snapshot, selected, db, actions),
                    }
                    let chips = queue_chips(snapshot, selected, db);
                    if menu.is_some() || !options.is_empty() || !chips.is_empty() {
                        queue_chip_row(ui, &chips, db, actions);
                    }
                });
            });
        });
    });
}

/// Whether selected factories are set to repeat.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RepeatState {
    Off,
    On,
    Mixed,
}

/// What the selected factories can do together. Unfinished factories take orders too,
/// and start on them once they are built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactoryMenu {
    pub factories: Vec<SimId>,
    /// Everything at least one factory can build, with the factories able to build it.
    pub options: Vec<(BlueprintId, Vec<SimId>)>,
    /// The upgrade, with the factories able to start it.
    pub upgrade: Option<(BlueprintId, Vec<SimId>)>,
    pub repeat: RepeatState,
    /// The combined queue, in order of first appearance, with how many of each.
    pub queue: Vec<(BlueprintId, u32)>,
    /// Average progress on items under way, in thousandths, if any are.
    pub progress_permille: Option<u16>,
    /// How many of the factories are still under construction.
    pub unfinished: usize,
}

/// Works out the shared menu for the selected factories. `None` if no factory is selected.
pub fn factory_menu(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> Option<FactoryMenu> {
    let mut menu = FactoryMenu {
        factories: Vec::new(),
        options: Vec::new(),
        upgrade: None,
        repeat: RepeatState::Off,
        queue: Vec::new(),
        progress_permille: None,
        unfinished: 0,
    };
    let (mut repeating, mut progress_total, mut under_way) = (0usize, 0u32, 0u32);
    for &id in selected {
        let Some(unit) = find_unit(snapshot, id).filter(|unit| unit.blueprint != u16::MAX) else {
            continue;
        };
        let blueprint = db.get(BlueprintId(unit.blueprint));
        let Some(factory) = snapshot.factories.iter().find(|factory| factory.id == id).filter(|_| blueprint.is_factory()) else {
            continue;
        };
        let finished = unit.built_permille == 1000;
        menu.factories.push(id);
        menu.unfinished += usize::from(!finished);
        repeating += usize::from(factory.repeat);
        // With an upgrade queued, it can also queue what the upgrade will let it build. A support
        // factory builds nothing above its headquarters' tier.
        let queued_upgrade = blueprint.upgrades_to.filter(|upgrade| factory.queue.contains(&upgrade.0));
        let after_upgrade = queued_upgrade.iter().flat_map(|upgrade| &db.get(*upgrade).builds);
        let allowed = match (blueprint.support, blueprint.factory_kind) {
            (true, Some(kind)) => headquarters_tier(snapshot, unit.team, kind, db),
            _ => u8::MAX,
        };
        for &option in blueprint.builds.iter().chain(after_upgrade).filter(|option| db.get(**option).tier <= allowed) {
            match menu.options.iter_mut().find(|(existing, _)| *existing == option) {
                Some((_, able)) if !able.contains(&id) => able.push(id),
                Some(_) => {}
                None => menu.options.push((option, vec![id])),
            }
        }
        // Factories of different tiers upgrade to different things, so the button is for the
        // lowest tier selected, and only the factories that upgrade to the same thing take it.
        if let Some(upgrade) = blueprint.upgrades_to.filter(|upgrade| !factory.queue.contains(&upgrade.0) && db.get(*upgrade).tier <= allowed) {
            match &mut menu.upgrade {
                Some((offered, able)) if *offered == upgrade => able.push(id),
                Some((offered, _)) if db.get(*offered).tier <= db.get(upgrade).tier => {}
                _ => menu.upgrade = Some((upgrade, vec![id])),
            }
        }
        for &item in &factory.queue {
            let item = BlueprintId(item);
            match menu.queue.iter_mut().find(|(existing, _)| *existing == item) {
                Some((_, count)) => *count += 1,
                None => menu.queue.push((item, 1)),
            }
        }
        if finished && !factory.queue.is_empty() {
            progress_total += u32::from(factory.progress_permille);
            under_way += 1;
        }
    }
    if menu.factories.is_empty() {
        return None;
    }
    menu.repeat = match repeating {
        0 => RepeatState::Off,
        count if count == menu.factories.len() => RepeatState::On,
        _ => RepeatState::Mixed,
    };
    menu.progress_permille = (under_way > 0).then(|| (progress_total / under_way) as u16);
    Some(menu)
}

/// Says when queued units will start, if any selected factory is still being built.
pub fn construction_note(unfinished: usize, total: usize) -> Option<String> {
    match unfinished {
        0 => None,
        _ if unfinished == total => Some("Under construction: the queue starts once built.".to_string()),
        _ => Some(format!("{unfinished} of {total} under construction: their queues start once built.")),
    }
}

/// A count of factories for confirmation text.
pub fn factory_count(count: usize) -> String {
    match count {
        1 => "1 factory".to_string(),
        count => format!("{count} factories"),
    }
}

/// Text confirming a queue order, saying how many factories took it.
pub fn queue_label(name: &str, given: usize, selected: usize) -> String {
    match (given, selected) {
        (_, 1) => format!("{name} queued"),
        (given, selected) if given == selected => format!("{name}: all {selected} factories"),
        (given, selected) => format!("{name}: {given} of {selected} factories"),
    }
}

/// Whether the selected launchers are building this missile one after another.
pub fn auto_building(menu: &FactoryMenu, missile: BlueprintId) -> bool {
    menu.repeat == RepeatState::On && menu.queue.iter().any(|(item, _)| *item == missile)
}

/// Right-clicking a missile: build them one after another, until right-clicked again.
pub fn auto_build_action(menu: &FactoryMenu, able: &[SimId], missile: BlueprintId) -> UiAction {
    if auto_building(menu, missile) {
        return UiAction::SetRepeat { factories: able.to_vec(), repeat: false };
    }
    let mut actions = Vec::new();
    if !menu.queue.iter().any(|(item, _)| *item == missile) {
        actions.push(queue_action(menu, able, missile));
    }
    actions.push(UiAction::SetRepeat { factories: able.to_vec(), repeat: true });
    UiAction::Batch(actions)
}

fn factory_controls(ui: &mut egui::Ui, menu: &FactoryMenu, db: &BlueprintDb, actions: &mut Vec<UiAction>) {
    let total = menu.factories.len();
    let partial = |able: usize| if able < total { format!("{able} of {total} factories can build this.\n") } else { String::new() };
    let shift = ui.input(|input| input.modifiers.shift);
    let options: Vec<BlueprintId> = menu.options.iter().map(|(option, _)| *option).collect();
    let able = |option: BlueprintId| menu.options.iter().find(|(existing, _)| *existing == option).map_or(&[][..], |(_, able)| able.as_slice());
    let missile = |option: BlueprintId| db.get(option).class == UnitClass::Ammo;
    let hint = |option: BlueprintId| {
        let auto = if missile(option) { " Right-click to build them one after another, and again to stop." } else { "" };
        format!("{}Hold Shift to queue {SHIFT_QUEUE_COUNT}.{auto}", partial(able(option).len()))
    };
    let suffix = |option: BlueprintId| if missile(option) && auto_building(menu, option) { " (auto)".to_string() } else { String::new() };
    match build_bar(ui, FACTORY_TIER_MEMORY, &options, db, hint, suffix) {
        Some(BarClick::Left(option)) => actions.push(with_shift(queue_action(menu, able(option), option), shift, db)),
        Some(BarClick::Right(option)) if missile(option) => actions.push(auto_build_action(menu, able(option), option)),
        _ => {}
    }
}

pub fn match_end_window(ctx: &egui::Context, result: MatchResult, player: Team, actions: &mut Vec<UiAction>) {
    let title = match result {
        MatchResult::Winner(team) if team == player => "Victory",
        MatchResult::Winner(_) => "Defeat",
        MatchResult::Draw => "Draw",
    };
    egui::Window::new(title)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(match title {
                "Victory" => "The enemy commander has been destroyed.",
                "Defeat" => "Your commander has been destroyed.",
                _ => "Both commanders were destroyed.",
            });
            if ui.button("Play again").clicked() {
                actions.push(UiAction::Restart);
            }
        });
}

/// What the watch bar shows of a watched match, and what it can change.
pub struct WatchState<'a> {
    /// The map's name, as players see it.
    pub map: &'a str,
    pub seed: u64,
    /// Whose sight the match is seen through: `None` for everything.
    pub view: Option<Team>,
    pub speed: u32,
    pub paused: bool,
}

/// The views a watcher can pick, in the order V steps through them.
pub const WATCH_VIEWS: [Option<Team>; 3] = [None, Some(Team::Blue), Some(Team::Red)];

/// The speeds a watcher can pick, which - and = step through.
pub const WATCH_SPEEDS: [u32; 5] = [1, 2, 4, 8, 16];

const BLUE_TEXT: Color32 = Color32::from_rgb(120, 165, 255);
const RED_TEXT: Color32 = Color32::from_rgb(255, 120, 105);

/// The view after `view`, for the V key.
pub fn next_watch_view(view: Option<Team>) -> Option<Team> {
    let index = WATCH_VIEWS.iter().position(|candidate| *candidate == view).unwrap_or(0);
    WATCH_VIEWS[(index + 1) % WATCH_VIEWS.len()]
}

/// The speed one step faster (`faster`) or slower than `speed`, staying within the choices.
pub fn step_watch_speed(speed: u32, faster: bool) -> u32 {
    let index = WATCH_SPEEDS.iter().position(|&candidate| candidate >= speed).unwrap_or(WATCH_SPEEDS.len() - 1);
    let index = if faster { (index + 1).min(WATCH_SPEEDS.len() - 1) } else { index.saturating_sub(1) };
    WATCH_SPEEDS[index]
}

fn view_name(view: Option<Team>) -> &'static str {
    match view {
        None => "Everything",
        Some(Team::Blue) => "Blue",
        Some(Team::Red) => "Red",
    }
}

/// The bars of a watched AI-vs-AI match. At the top, the match and both teams' economies, or
/// when seeing through one team's eyes, that team's economy as its player would see it. At the
/// bottom, whose sight to see through, how fast to play, and pause.
pub fn watch_bar(ctx: &egui::Context, snapshot: &SimSnapshot, db: &BlueprintDb, state: &WatchState, ranges: &mut RangeToggles, actions: &mut Vec<UiAction>) {
    if state.paused {
        paused_banner(ctx);
    }
    let team_name = |team: Team| if team == Team::Blue { "Blue" } else { "Red" };
    let team_text = |team: Team| if team == Team::Blue { BLUE_TEXT } else { RED_TEXT };
    // The economies at the top left: both, or the one seen through.
    let economies: Vec<(Team, &EconomyView)> = match (state.view, &snapshot.team_economies) {
        (None, Some(economies)) => vec![(Team::Blue, &economies[0]), (Team::Red, &economies[1])],
        (Some(team), _) => vec![(team, &snapshot.economy)],
        // The first snapshot from another view has not come yet.
        (None, None) => Vec::new(),
    };
    if !economies.is_empty() {
        let ((), panel) = hud::panel_rect(ctx, "economy", Align2::LEFT_TOP, Vec2::new(4.0, 4.0), |ui| {
            for (team, economy) in &economies {
                economy_rows(ui, economy, Some((team_name(*team), team_text(*team))), true);
            }
        });
        crate::minimap::note_economy_bottom(ctx, panel.bottom());
    }
    hud::panel(ctx, "watch controls", Align2::CENTER_TOP, Vec2::new(0.0, 4.0), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let (glyph, hint) = if state.paused { (Glyph::Resume, "[Space] Resume") } else { (Glyph::Pause, "[Space] Pause") };
            if glyph_button(ui, glyph, "", state.paused, hint).clicked() {
                actions.push(UiAction::TogglePause);
            }
            ui.add_space(6.0);
            ui.label(bold_keys("See [V]"));
            for view in WATCH_VIEWS {
                let hint = match view {
                    None => "Everything on the map, with no fog of war, and both teams' economies.".to_string(),
                    Some(team) => format!("Only what {} sees, with its fog of war and economy, as if playing as {}.", view_name(view), team_name(team)),
                };
                if ui.selectable_label(state.view == view, view_name(view)).on_hover_text(hint).clicked() && state.view != view {
                    actions.push(UiAction::SetView(view));
                }
            }
            ui.add_space(6.0);
            ui.label(bold_keys("Speed [-] [=]"));
            for speed in WATCH_SPEEDS {
                if ui.selectable_label(state.speed == speed, format!("{speed}x")).on_hover_text(format!("{speed} seconds of the match each second")).clicked() && state.speed != speed {
                    actions.push(UiAction::SetSpeed(speed));
                }
            }
            ui.add_space(6.0);
            range_menu(ui, ranges);
        });
    });
    let ((), score) = hud::panel_rect(ctx, "score", Align2::RIGHT_TOP, Vec2::new(-4.0, 4.0), |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        score_header(ui, snapshot.tick, &format!("{}x", state.speed));
        panel_rule(ui);
        score_columns(ui);
        for team in [Team::Blue, Team::Red] {
            let economy = match (&snapshot.team_economies, state.view) {
                (Some(economies), None) => Some(&economies[team as usize]),
                (_, Some(seen)) if seen == team => Some(&snapshot.economy),
                _ => None,
            };
            let figures = team_figures(snapshot, team, db, economy.map(|economy| economy.mass_income));
            score_row(ui, team_color(team), &format!("{} (AI)", team_name(team)), figures, team_state(snapshot.match_result, team));
        }
        panel_rule(ui);
        ui.label(egui::RichText::new("Observer").strong().color(Color32::from_rgb(255, 170, 60)));
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(format!("Map: {}   seed {}", state.map, state.seed)).size(12.0).color(hud::DIM_TEXT))
                .on_hover_text("The match the soak plays for this map and seed, which plays out the same every time.");
        });
    });
    note_score_bottom(ctx, score.bottom());
}

/// The end of a watched match: who won, and what to watch next. `stopped` says the match ran
/// to the soak's time limit without a winner.
pub fn watch_end_window(ctx: &egui::Context, result: Option<MatchResult>, stopped: bool, tick: u32, seed: u64, actions: &mut Vec<UiAction>) {
    let title = match result {
        Some(MatchResult::Winner(Team::Blue)) => "Blue wins",
        Some(MatchResult::Winner(Team::Red)) => "Red wins",
        Some(MatchResult::Draw) => "Draw",
        None if stopped => "Unfinished",
        None => return,
    };
    let seconds = tick / 10;
    egui::Window::new(title)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(match result {
                Some(MatchResult::Winner(_)) => format!("The other commander was destroyed at {}:{:02}.", seconds / 60, seconds % 60),
                Some(MatchResult::Draw) => format!("Both commanders were destroyed at {}:{:02}.", seconds / 60, seconds % 60),
                None => format!("No commander had fallen by {}:{:02}, where the soak stops a match.", seconds / 60, seconds % 60),
            });
            ui.horizontal(|ui| {
                if ui.button("Watch again").clicked() {
                    actions.push(UiAction::Restart);
                }
                if ui.button(format!("Watch seed {}", seed + 1)).clicked() {
                    actions.push(UiAction::WatchSeed(seed + 1));
                }
            });
        });
}

/// A small floating reminder that the next click will attack-move.
pub fn armed_order_hint(ctx: &egui::Context, order: ArmedOrder) {
    egui::Area::new(egui::Id::new("armed order hint"))
        .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 62.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                one_line_label(ui, order.hint());
            });
        });
}

fn marker_color(kind: MarkerKind) -> Color32 {
    match kind {
        MarkerKind::Move => Color32::from_rgb(110, 240, 130),
        MarkerKind::AttackMove | MarkerKind::Attack => Color32::from_rgb(255, 80, 70),
        MarkerKind::Support => Color32::from_rgb(100, 180, 255),
        MarkerKind::Rally => Color32::WHITE,
        MarkerKind::Patrol => Color32::from_rgb(255, 220, 90),
        MarkerKind::Build => Color32::from_rgb(120, 240, 220),
        MarkerKind::Rejected => Color32::from_gray(170),
    }
}

/// Draws one marker: a ring that shrinks as it fades. The shapes differ as well as the colors,
/// so that orders can be told apart without relying on color.
fn draw_marker(painter: &egui::Painter, center: Pos2, marker: &Marker) {
    let progress = marker.progress() as f32;
    let radius = 28.0 - 16.0 * progress;
    let color = marker_color(marker.kind).gamma_multiply(1.0 - progress);
    let stroke = Stroke::new(2.5_f32, color);
    match marker.kind {
        MarkerKind::Rejected => {
            painter.circle_stroke(center, radius * 0.6, stroke);
            let reach = radius * 0.42;
            painter.line_segment([center - Vec2::splat(reach), center + Vec2::splat(reach)], stroke);
        }
        MarkerKind::AttackMove => {
            // A ring with ticks pointing in, like a gun sight.
            painter.circle_stroke(center, radius, stroke);
            for direction in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
                painter.line_segment([center + direction * radius, center + direction * (radius * 0.55)], stroke);
            }
        }
        MarkerKind::Attack => {
            painter.circle_stroke(center, radius, stroke);
            painter.circle_stroke(center, radius * 0.55, stroke);
        }
        MarkerKind::Rally | MarkerKind::Patrol => {
            let corners = [Vec2::new(0.0, -radius), Vec2::new(radius, 0.0), Vec2::new(0.0, radius), Vec2::new(-radius, 0.0)];
            for index in 0..corners.len() {
                painter.line_segment([center + corners[index], center + corners[(index + 1) % corners.len()]], stroke);
            }
        }
        MarkerKind::Build => {
            painter.rect_stroke(Rect::from_center_size(center, Vec2::splat(radius * 1.6)), Rounding::ZERO, stroke);
        }
        MarkerKind::Move | MarkerKind::Support => {
            painter.circle_stroke(center, radius, stroke);
        }
    }
}

/// Shows that clicks were received: markers on the map, and a line of text below the top bar.
pub fn order_feedback(ctx: &egui::Context, camera: &Camera, snapshot: &SimSnapshot, feedback: &OrderFeedback, pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let raw = |value: i32| f64::from(value) / 4096.0;
    for marker in feedback.markers() {
        // A marker on a unit follows it, as long as the player can still see it.
        let followed = marker.target.and_then(|target| find_unit(snapshot, target));
        let position = followed.map_or(marker.position, |unit| [raw(unit.x), raw(unit.y), raw(unit.z)]);
        if let Some(pixels) = camera.world_to_screen(position) {
            draw_marker(&painter, Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point), marker);
        }
    }
    if let Some(label) = feedback.label() {
        // Stay solid for most of the time, then fade.
        let remaining = (1.0 - label.age / LABEL_SECONDS) as f32;
        let color = marker_color(label.kind).gamma_multiply((remaining * 3.0).min(1.0));
        egui::Area::new(egui::Id::new("order feedback"))
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 100.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).multiply_with_opacity((remaining * 3.0).min(1.0)).show(ui, |ui| {
                    one_line_label(ui, egui::RichText::new(&label.text).strong().color(color));
                });
            });
    }
}

/// Cuts a screen-space line down to the part inside a rectangle (Liang-Barsky). A goal far
/// off-screen can project very far away, and dashing all of that would be wasted work.
pub fn clip_segment(from: Pos2, to: Pos2, bounds: Rect) -> Option<(Pos2, Pos2)> {
    let delta = to - from;
    let (mut start, mut end) = (0.0_f32, 1.0_f32);
    for (direction, offset) in [
        (-delta.x, from.x - bounds.min.x),
        (delta.x, bounds.max.x - from.x),
        (-delta.y, from.y - bounds.min.y),
        (delta.y, bounds.max.y - from.y),
    ] {
        if direction == 0.0 {
            if offset < 0.0 {
                return None;
            }
            continue;
        }
        let share = offset / direction;
        if direction < 0.0 {
            start = start.max(share);
        } else {
            end = end.min(share);
        }
        if start > end {
            return None;
        }
    }
    Some((from + delta * start, from + delta * end))
}

/// One leg of a trail, in world coordinates.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TrailLeg {
    pub from: [f64; 3],
    pub to: [f64; 3],
    pub kind: MarkerKind,
    pub opacity: f32,
    /// The position in the unit's plan, from 1. Zero if it is not worth numbering.
    pub number: usize,
    /// Whose plan it is.
    pub unit: SimId,
    /// For the selection's moves, how many seconds until the unit gets to the leg's end.
    pub eta: Option<f64>,
}

fn trail_marker_kind(kind: PlanStepKind) -> MarkerKind {
    match kind {
        PlanStepKind::Move => MarkerKind::Move,
        PlanStepKind::AttackMove => MarkerKind::AttackMove,
        PlanStepKind::Attack | PlanStepKind::AttackGround => MarkerKind::Attack,
        PlanStepKind::Build => MarkerKind::Build,
        PlanStepKind::Support => MarkerKind::Support,
        PlanStepKind::Patrol => MarkerKind::Patrol,
    }
}

/// Works out the trails for the selected units. Each one starts at the unit, drawn where
/// the unit appears between ticks, and runs through its pending orders in sequence.
pub fn build_trails(previous: &SimSnapshot, current: &SimSnapshot, alpha: f64, selected: &[SimId]) -> Vec<TrailLeg> {
    trails_for(previous, current, alpha, selected, MAX_TRAIL_UNITS)
}

/// Trails for every other unit with pending orders, for while Shift is held.
/// `strength` runs from 1 down to 0 as they fade out afterwards.
pub fn other_units_trails(previous: &SimSnapshot, current: &SimSnapshot, alpha: f64, selected: &[SimId], strength: f32) -> Vec<TrailLeg> {
    let mut ids: Vec<SimId> = current.plans.iter().map(|plan| plan.id).collect();
    ids.extend(current.factories.iter().filter(|factory| factory.rally.is_some()).map(|factory| factory.id));
    ids.sort_unstable();
    ids.dedup();
    ids.retain(|id| !selected.contains(id));
    let mut legs = trails_for(previous, current, alpha, &ids, ALL_ORDERS_UNIT_LIMIT);
    for leg in &mut legs {
        leg.opacity *= strength;
    }
    legs
}

fn trails_for(previous: &SimSnapshot, current: &SimSnapshot, alpha: f64, selected: &[SimId], limit: usize) -> Vec<TrailLeg> {
    let raw = |value: i32| f64::from(value) / 4096.0;
    let mut legs = Vec::new();
    let mut units_drawn = 0;
    for &id in selected {
        let Some(unit) = find_unit(current, id) else {
            continue;
        };
        let now = [raw(unit.x), raw(unit.y), raw(unit.z)];
        let before = find_unit(previous, id).map_or(now, |unit| [raw(unit.x), raw(unit.y), raw(unit.z)]);
        let start: [f64; 3] = std::array::from_fn(|axis| before[axis] + (now[axis] - before[axis]) * alpha);

        if let Some(rally) = current.factories.iter().find(|factory| factory.id == id).and_then(|factory| factory.rally) {
            legs.push(TrailLeg {
                from: start,
                to: [raw(rally[0]), raw(rally[1]), raw(rally[2])],
                kind: MarkerKind::Rally,
                opacity: RALLY_OPACITY,
                number: 0,
                unit: id,
                eta: None,
            });
        }
        let Ok(index) = current.plans.binary_search_by_key(&id, |plan| plan.id) else {
            continue;
        };
        if units_drawn == limit {
            continue;
        }
        units_drawn += 1;
        let steps = &current.plans[index].steps;
        let mut from = start;
        for (position, step) in steps.iter().enumerate() {
            let to = [raw(step.x), raw(step.y), raw(step.z)];
            legs.push(TrailLeg {
                from,
                to,
                kind: trail_marker_kind(step.kind),
                opacity: TRAIL_OPACITY,
                number: if steps.len() > 1 { position + 1 } else { 0 },
                unit: id,
                eta: None,
            });
            from = to;
        }
    }
    legs
}

/// Whether a leg is the unit walking somewhere, which gets a ribbon and an arrival time.
const fn is_walk(kind: MarkerKind) -> bool {
    matches!(kind, MarkerKind::Move | MarkerKind::AttackMove | MarkerKind::Patrol)
}

/// Works out when each of the selection's units reaches the end of each of its walking legs,
/// at its top speed along straight lines: the usual estimate, which ignores
/// detours and stops along the way. Building and other work end the estimate, since how long
/// they take is not known here.
pub fn estimate_arrivals(legs: &mut [TrailLeg], snapshot: &SimSnapshot, db: &BlueprintDb) {
    let mut elapsed: Option<(SimId, f64)> = None;
    for leg in legs.iter_mut() {
        if leg.opacity != TRAIL_OPACITY {
            continue;
        }
        let speed = find_unit(snapshot, leg.unit).filter(|unit| unit.blueprint != u16::MAX).map_or(0.0, |unit| db.get(BlueprintId(unit.blueprint)).speed.to_num::<f64>() * TICKS_PER_SECOND as f64);
        let so_far = match elapsed {
            Some((unit, seconds)) if unit == leg.unit => Some(seconds),
            Some(_) | None => Some(0.0),
        };
        let Some(so_far) = so_far.filter(|_| speed > 0.0 && is_walk(leg.kind)) else {
            // Anything else stops the clock for the rest of this unit's legs.
            elapsed = Some((leg.unit, f64::INFINITY));
            continue;
        };
        let distance = (leg.to[0] - leg.from[0]).hypot(leg.to[1] - leg.from[1]);
        let at = so_far + distance / speed;
        leg.eta = at.is_finite().then_some(at);
        elapsed = Some((leg.unit, at));
    }
}

/// An arrival time in minutes and seconds.
pub fn eta_text(seconds: f64) -> String {
    let whole = seconds.round() as i64;
    format!("ETA {}:{:02}", whole / 60, whole % 60)
}

/// Draws the selected units' trails: a wide, faint ribbon along their walks, dashed lines
/// through their pending orders, a dot at each stop, numbers when there are several, and when
/// each walk arrives. Stops that several units share show one time, the slowest's.
pub fn order_trails(ctx: &egui::Context, camera: &Camera, legs: &[TrailLeg], pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let to_points = |world: [f64; 3]| {
        camera.world_to_screen(world).map(|pixels| Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point))
    };
    let bounds = ctx.screen_rect().expand(16.0);
    // The ribbons first, under everything else.
    for leg in legs.iter().filter(|leg| leg.opacity == TRAIL_OPACITY && is_walk(leg.kind)) {
        if let (Some(from), Some(to)) = (to_points(leg.from), to_points(leg.to)) {
            if let Some((start, end)) = clip_segment(from, to, bounds) {
                painter.line_segment([start, end], Stroke::new(RIBBON_POINTS, marker_color(leg.kind).gamma_multiply(RIBBON_OPACITY)));
            }
        }
    }
    let mut arrivals: BTreeMap<(i64, i64), (Pos2, f64)> = BTreeMap::new();
    for leg in legs {
        let color = marker_color(leg.kind).gamma_multiply(leg.opacity);
        let stroke = Stroke::new(2.0_f32, color);
        if let (Some(from), Some(to)) = (to_points(leg.from), to_points(leg.to)) {
            if let Some((start, end)) = clip_segment(from, to, bounds) {
                painter.extend(egui::Shape::dashed_line(&[start, end], stroke, DASH_POINTS, GAP_POINTS));
            }
        }
        let Some(end) = to_points(leg.to) else {
            continue;
        };
        if !bounds.contains(end) {
            continue;
        }
        painter.circle_filled(end, 2.5, color);
        if leg.number > 0 {
            painter.text(end + Vec2::new(6.0, -6.0), Align2::LEFT_BOTTOM, leg.number.to_string(), FontId::proportional(14.0), Color32::WHITE.gamma_multiply(leg.opacity));
        }
        if let Some(eta) = leg.eta {
            // Stops within a few points of each other on screen count as one.
            let key = ((end.x / 24.0).round() as i64, (end.y / 24.0).round() as i64);
            let slowest = arrivals.entry(key).or_insert((end, eta));
            slowest.1 = slowest.1.max(eta);
        }
    }
    for (end, eta) in arrivals.into_values() {
        hud::outlined_text(&painter, end + Vec2::new(8.0, 4.0), Align2::LEFT_TOP, &eta_text(eta), FontId::proportional(11.0), Color32::WHITE);
    }
}

/// How wide a walk's ribbon is, in points, and how strong.
const RIBBON_POINTS: f32 = 12.0;
const RIBBON_OPACITY: f32 = 0.22;

/// The outer edge of where any of these circles reach, as short straight pieces: each circle's
/// rim, less what lies inside another circle. Circles are `(center, radius)` in world units.
pub fn coverage_outline(circles: &[([f64; 2], f64)]) -> Vec<[[f64; 2]; 2]> {
    let mut pieces = Vec::new();
    for (index, &(center, radius)) in circles.iter().enumerate() {
        if radius <= 0.0 {
            continue;
        }
        // About 40 units a piece, which looks round from anywhere it is drawn.
        let count = (std::f64::consts::TAU * radius / 40.0).clamp(32.0, 512.0) as usize;
        let at = |angle: f64| [center[0] + radius * angle.cos(), center[1] + radius * angle.sin()];
        // Only a circle that overlaps this one can hide part of its rim.
        let near: Vec<(usize, ([f64; 2], f64))> =
            circles.iter().copied().enumerate().filter(|&(other, (middle, reach))| other != index && (center[0] - middle[0]).hypot(center[1] - middle[1]) < radius + reach).collect();
        let inside_another = |point: [f64; 2]| {
            near.iter().any(|&(other, (middle, reach))| {
                if other == index {
                    false
                } else if middle == center && reach == radius {
                    // Of two identical circles, only the first draws the rim they share.
                    other < index
                } else {
                    (point[0] - middle[0]).hypot(point[1] - middle[1]) < reach - 1e-6
                }
            })
        };
        for piece in 0..count {
            let (from, to) = (piece as f64 / count as f64 * std::f64::consts::TAU, (piece + 1) as f64 / count as f64 * std::f64::consts::TAU);
            if !inside_another(at((from + to) * 0.5)) {
                pieces.push([at(from), at(to)]);
            }
        }
    }
    pieces
}

/// A kind of range a unit can reach: what it shoots at, what it guards, and what it senses.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum RangeKind {
    /// Weapons that fire at units on the ground or the water.
    GroundFire,
    /// Weapons that can fire at aircraft, including those that fire at the ground too.
    AntiAir,
    Torpedoes,
    /// Artillery's inner edge, within which it cannot hit.
    TooClose,
    Shield,
    MissileDefense,
    TorpedoDefense,
    Radar,
    Sonar,
    DeepScan,
    Stealth,
}

impl RangeKind {
    pub const ALL: [RangeKind; 11] = [
        RangeKind::GroundFire,
        RangeKind::AntiAir,
        RangeKind::Torpedoes,
        RangeKind::TooClose,
        RangeKind::Shield,
        RangeKind::MissileDefense,
        RangeKind::TorpedoDefense,
        RangeKind::Radar,
        RangeKind::Sonar,
        RangeKind::DeepScan,
        RangeKind::Stealth,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            RangeKind::GroundFire => "Ground fire",
            RangeKind::AntiAir => "Anti-air",
            RangeKind::Torpedoes => "Torpedoes",
            RangeKind::TooClose => "Too close to hit",
            RangeKind::Shield => "Shield",
            RangeKind::MissileDefense => "Missile defense",
            RangeKind::TorpedoDefense => "Torpedo defense",
            RangeKind::Radar => "Radar",
            RangeKind::Sonar => "Sonar",
            RangeKind::DeepScan => "Deep scan",
            RangeKind::Stealth => "Stealth field",
        }
    }

    /// Each its own color, so that rings of different kinds can be told apart where they cross.
    pub const fn color(self) -> Color32 {
        match self {
            RangeKind::GroundFire => Color32::from_rgb(255, 212, 72),
            RangeKind::AntiAir => Color32::from_rgb(255, 120, 220),
            RangeKind::Torpedoes => Color32::from_rgb(90, 220, 190),
            RangeKind::TooClose => Color32::from_rgb(255, 96, 84),
            RangeKind::Shield => Color32::from_rgb(140, 190, 255),
            RangeKind::MissileDefense => Color32::from_rgb(255, 160, 70),
            RangeKind::TorpedoDefense => Color32::from_rgb(170, 240, 240),
            RangeKind::Radar => Color32::from_rgb(120, 230, 140),
            RangeKind::Sonar => Color32::from_rgb(80, 140, 255),
            RangeKind::DeepScan => Color32::from_rgb(200, 140, 255),
            RangeKind::Stealth => Color32::from_rgb(190, 190, 200),
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// Which kinds of range are outlined for all the viewer's units, selected or not. Radar and sonar
/// are, to begin with, so that what the army can see is always on the map.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RangeToggles([bool; RangeKind::ALL.len()]);

impl Default for RangeToggles {
    fn default() -> Self {
        let mut shown = [false; RangeKind::ALL.len()];
        shown[RangeKind::Radar.index()] = true;
        shown[RangeKind::Sonar.index()] = true;
        Self(shown)
    }
}

impl RangeToggles {
    pub const fn shown(&self, kind: RangeKind) -> bool {
        self.0[kind.index()]
    }

    pub fn set(&mut self, kind: RangeKind, shown: bool) {
        self.0[kind.index()] = shown;
    }
}

/// The ranges a unit's blueprint reaches, by kind, for placing a structure and for what is
/// selected. A weapon that fires at both the ground and aircraft reaches as both.
pub fn placement_ranges(unit: &crate::sim::blueprints::Blueprint) -> Vec<(RangeKind, f64)> {
    use crate::sim::blueprints::Targets;
    let world = |value: Fp| value.to_num::<f64>();
    let mut ranges = Vec::new();
    for gun in unit.weapon.iter().chain(&unit.extra_weapons) {
        let range = world(gun.range);
        match gun.targets {
            Targets::Air => ranges.push((RangeKind::AntiAir, range)),
            Targets::Naval => ranges.push((RangeKind::Torpedoes, range)),
            Targets::Both => ranges.extend([(RangeKind::GroundFire, range), (RangeKind::AntiAir, range)]),
            Targets::Ground => ranges.push((RangeKind::GroundFire, range)),
        }
        if gun.min_range > Fp::ZERO {
            ranges.push((RangeKind::TooClose, world(gun.min_range)));
        }
    }
    if let Some(shield) = &unit.shield {
        ranges.push((RangeKind::Shield, world(shield.radius)));
    }
    if let Some(defense) = &unit.missile_defense {
        ranges.push((RangeKind::MissileDefense, world(defense.range)));
    }
    if let Some(defense) = &unit.torpedo_defense {
        ranges.push((RangeKind::TorpedoDefense, world(defense.range)));
    }
    ranges.push((RangeKind::Radar, world(unit.radar)));
    ranges.push((RangeKind::Sonar, world(unit.sonar)));
    ranges.push((RangeKind::DeepScan, world(unit.deep_scan)));
    ranges.push((RangeKind::Stealth, world(unit.stealth_field)));
    ranges.retain(|range| range.1 > 0.0);
    // One ring for each kind and reach: a unit's two guns of one range draw once.
    let mut unique: Vec<(RangeKind, f64)> = Vec::new();
    for range in ranges {
        if !unique.iter().any(|seen| seen.0 == range.0 && (seen.1 - range.1).abs() < 0.5) {
            unique.push(range);
        }
    }
    unique
}

/// While a structure is being placed, rings on the ground for each range it will reach, around
/// where it would go, each named at its top.
pub fn draw_placement_ranges(ctx: &egui::Context, camera: &Camera, heights: &crate::render::camera::HeightField, center: [f64; 2], ranges: &[(RangeKind, f64)], pixels_per_point: f32) {
    let kinds: Vec<(RangeKind, Vec<Circle>)> = ranges.iter().map(|&(kind, radius)| (kind, vec![(center, radius)])).collect();
    draw_range_outlines(ctx, camera, heights, &kinds, RingStyle::Labelled, pixels_per_point);
}

/// The most selected units whose ranges are drawn, so that a whole army does not bury the map.
const MOST_RANGES_SHOWN: usize = 60;

/// The most rings of one kind outlined for all the viewer's units.
const MOST_OVERLAY_RINGS: usize = 600;

/// A circle on the ground: its center and radius, in world units.
pub type Circle = ([f64; 2], f64);

/// Rings grouped by the kind of range they show.
pub type RangeRings = Vec<(RangeKind, Vec<Circle>)>;

/// Adds a ring of a kind to the list, beside the others of that kind.
fn add_ring(kinds: &mut Vec<(RangeKind, Vec<Circle>)>, kind: RangeKind, circle: Circle) {
    match kinds.iter_mut().find(|(existing, _)| *existing == kind) {
        Some((_, circles)) => circles.push(circle),
        None => kinds.push((kind, vec![circle])),
    }
}

/// The rings of what is selected, mobile units and structures alike, grouped by kind.
pub fn selection_ranges(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> Vec<(RangeKind, Vec<Circle>)> {
    let mut kinds = Vec::new();
    for unit in selected.iter().filter_map(|id| find_unit(snapshot, *id)).filter(|unit| unit.blueprint != u16::MAX).take(MOST_RANGES_SHOWN) {
        let center = [f64::from(unit.x) / 4096.0, f64::from(unit.y) / 4096.0];
        for (kind, radius) in placement_ranges(db.get(BlueprintId(unit.blueprint))) {
            add_ring(&mut kinds, kind, (center, radius));
        }
    }
    kinds
}

/// The rings of every finished unit of `team` (of both teams for `None`), for the kinds toggled
/// on. Radar and deep scan come from what the viewer's sensors really reach, powered and
/// enhanced, when there is a viewer; everything else from the units' blueprints.
pub fn overlay_ranges(snapshot: &SimSnapshot, team: Option<Team>, toggles: &RangeToggles, db: &BlueprintDb) -> Vec<(RangeKind, Vec<Circle>)> {
    let world = |raw: i32| f64::from(raw) / 4096.0;
    let sensed = |kind: RangeKind| team.is_some() && matches!(kind, RangeKind::Radar | RangeKind::DeepScan);
    let mut kinds = Vec::new();
    for unit in snapshot.units.iter().filter(|unit| unit.blueprint != u16::MAX && unit.built_permille >= 1000 && team.is_none_or(|team| unit.team == team)) {
        let center = [world(unit.x), world(unit.y)];
        for (kind, radius) in placement_ranges(db.get(BlueprintId(unit.blueprint))) {
            if toggles.shown(kind) && !sensed(kind) {
                add_ring(&mut kinds, kind, (center, radius));
            }
        }
    }
    for sensor in snapshot.sensor_ranges.iter().filter(|_| team.is_some()) {
        let center = [world(sensor.center[0]), world(sensor.center[1])];
        for (kind, radius) in [(RangeKind::Radar, sensor.radar), (RangeKind::DeepScan, sensor.deep_scan)] {
            if toggles.shown(kind) && radius > 0 {
                add_ring(&mut kinds, kind, (center, world(radius)));
            }
        }
    }
    for (_, circles) in &mut kinds {
        circles.truncate(MOST_OVERLAY_RINGS);
    }
    kinds.sort_by_key(|(kind, _)| kind.index());
    kinds
}

/// How rings are drawn: bold and named up close for what is selected or being placed, or thin
/// and unnamed for what is toggled on for all units.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RingStyle {
    Labelled,
    Faint,
}

/// Rings on the ground for each kind of range, those of one kind merged into one outline around
/// the group, so that an army shows where it reaches rather than a tangle of circles.
pub fn draw_range_outlines(ctx: &egui::Context, camera: &Camera, heights: &crate::render::camera::HeightField, kinds: &[(RangeKind, Vec<Circle>)], style: RingStyle, pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let to_points = |point: [f64; 2]| {
        let world = [point[0], point[1], heights.height_at(point[0], point[1]) + 1.5];
        camera.world_to_screen(world).map(|pixels| Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point))
    };
    let (width, shadow, opacity) = match style {
        RingStyle::Labelled => (1.8_f32, 0.35, 1.0),
        RingStyle::Faint => (1.2_f32, 0.2, 0.7),
    };
    let labels = style == RingStyle::Labelled && camera.altitude() < ICON_ALTITUDE;
    let mut named: Vec<Pos2> = Vec::new();
    for (kind, circles) in kinds {
        let color = kind.color().gamma_multiply(opacity);
        for [from, to] in coverage_outline(circles) {
            if let (Some(from), Some(to)) = (to_points(from), to_points(to)) {
                painter.line_segment([from, to], Stroke::new(width + 1.7, Color32::BLACK.gamma_multiply(shadow)));
                painter.line_segment([from, to], Stroke::new(width, color));
            }
        }
        // On the ring's far side from the camera, where it is least in the way, and above any
        // other label already there, as for a gun that reaches the ground and the air alike.
        let Some(&(center, radius)) = circles.iter().max_by(|a, b| (a.0[1] + a.1).total_cmp(&(b.0[1] + b.1))) else {
            continue;
        };
        if let Some(mut top) = to_points([center[0], center[1] + radius]).filter(|_| labels) {
            top.y -= 3.0;
            while named.iter().any(|other| (other.x - top.x).abs() < 60.0 && (other.y - top.y).abs() < 13.0) {
                top.y -= 14.0;
            }
            named.push(top);
            painter.text(top, Align2::CENTER_BOTTOM, format!("{} {}", kind.name(), radius.round() as i64), FontId::proportional(12.0), kind.color());
        }
    }
}

/// The ranges toggle at the top: a button that opens a list of every kind of range, each with a
/// checkbox in its own color, to outline it for all the viewer's units.
fn range_menu(ui: &mut egui::Ui, toggles: &mut RangeToggles) {
    let id = ui.make_persistent_id("range menu");
    let open = ui.memory(|memory| memory.data.get_temp::<bool>(id).unwrap_or(false));
    let button = glyph_button(ui, Glyph::Ranges, "", open, "Ranges\nChoose which ranges to outline for all your units. What is selected always shows all of its own.");
    if button.clicked() {
        ui.memory_mut(|memory| memory.data.insert_temp(id, !open));
    }
    if !open {
        return;
    }
    let below = button.rect.left_bottom() + Vec2::new(0.0, 8.0);
    let area = egui::Area::new(id.with("list")).order(egui::Order::Foreground).fixed_pos(below).constrain(true).show(ui.ctx(), |ui| {
        hud::framed(ui, Vec2::new(11.0, 8.0), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(egui::RichText::new("Show for all units").size(12.0).color(hud::DIM_TEXT));
            for kind in RangeKind::ALL {
                let mut shown = toggles.shown(kind);
                if ui.checkbox(&mut shown, egui::RichText::new(kind.name()).color(kind.color())).changed() {
                    toggles.set(kind, shown);
                }
            }
        })
    });
    // A click anywhere else closes it.
    if ui.input(|input| input.pointer.any_click()) && !button.hovered() && !area.response.hovered() && !area.response.contains_pointer() {
        ui.memory_mut(|memory| memory.data.insert_temp(id, false));
    }
}

/// Off the map, a floor of dark panels with a few lights, fading in as the camera pulls out; and
/// along the map's edge, a glowing rim, so that the map reads as a lit table.
pub fn draw_map_frame(ctx: &egui::Context, camera: &Camera, heights: &crate::render::camera::HeightField, pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let size = (heights.samples() - 1) as f64 * heights.cell_units();
    let to_points = |x: f64, y: f64, z: f64| camera.world_to_screen([x, y, z]).map(|pixels| Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point));
    let screen = ctx.screen_rect();

    let floor = ((camera.zoom_from_closest() - 0.3) / 0.2).clamp(0.0, 1.0) as f32;
    if floor > 0.0 {
        const TILES: i32 = 10;
        const RING: i32 = 7;
        let tile = size / f64::from(TILES);
        let floor_height = -40.0;
        for ty in -RING..TILES + RING {
            for tx in -RING..TILES + RING {
                if (0..TILES).contains(&tx) && (0..TILES).contains(&ty) {
                    continue;
                }
                let inset = tile * 0.035;
                let (x0, y0) = (f64::from(tx) * tile + inset, f64::from(ty) * tile + inset);
                let (x1, y1) = (x0 + tile - 2.0 * inset, y0 + tile - 2.0 * inset);
                let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(x, y)| to_points(x, y, floor_height));
                let Some(corners) = corners.into_iter().collect::<Option<Vec<Pos2>>>() else {
                    continue;
                };
                let bounds = Rect::from_points(&corners);
                if !bounds.intersects(screen) {
                    continue;
                }
                // Every panel the same, but for a few with lights or a recess, picked from where
                // the panel is so that they stay put.
                let hash = (tx.wrapping_mul(73_856_093) ^ ty.wrapping_mul(19_349_663)).unsigned_abs();
                let fill = if hash % 7 == 0 { Color32::from_rgb(10, 12, 15) } else { Color32::from_rgb(17, 20, 25) };
                painter.add(egui::Shape::convex_polygon(corners.clone(), fill.gamma_multiply(floor), Stroke::new(1.0_f32, Color32::from_rgb(30, 36, 44).gamma_multiply(floor))));
                if hash % 5 == 0 || hash % 11 == 0 {
                    // A pair of small lights along one side.
                    let along = |share: f32, lift: f32| corners[0] + (corners[1] - corners[0]) * share + (corners[3] - corners[0]) * lift;
                    let lift = if hash % 2 == 0 { 0.18 } else { 0.82 };
                    for start in [0.36_f32, 0.52] {
                        painter.line_segment([along(start, lift), along(start + 0.1, lift)], Stroke::new(3.0_f32, Color32::from_rgb(120, 190, 230).gamma_multiply(0.55 * floor)));
                    }
                }
            }
        }
    }

    // The rim: wide and faint, then narrower and brighter, with a white core.
    const PER_EDGE: usize = 48;
    let mut rim: Vec<Option<Pos2>> = Vec::with_capacity(PER_EDGE * 4 + 1);
    let corners = [(0.0, 0.0), (size, 0.0), (size, size), (0.0, size)];
    for edge in 0..4 {
        let (from, to) = (corners[edge], corners[(edge + 1) % 4]);
        for step in 0..PER_EDGE {
            let share = step as f64 / PER_EDGE as f64;
            let (x, y) = (from.0 + (to.0 - from.0) * share, from.1 + (to.1 - from.1) * share);
            rim.push(to_points(x, y, heights.height_at(x, y) + 2.0));
        }
    }
    rim.push(rim[0]);
    let glow = Color32::from_rgb(150, 222, 255);
    for (width, strength) in [(22.0_f32, 0.07_f32), (11.0, 0.16), (5.5, 0.5), (2.4, 0.0)] {
        let color = if strength == 0.0 { Color32::from_rgb(230, 248, 255) } else { glow.gamma_multiply(strength) };
        for pair in rim.windows(2) {
            if let [Some(from), Some(to)] = pair {
                painter.line_segment([*from, *to], Stroke::new(width, color));
            }
        }
    }
}

/// Radar contacts the viewer cannot see, as small triangles that say nothing of what they are.
pub fn draw_blips(ctx: &egui::Context, camera: &Camera, blips: &[[i32; 3]], pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let color = Color32::from_rgb(255, 190, 80);
    for blip in blips {
        let world = [f64::from(blip[0]) / 4096.0, f64::from(blip[1]) / 4096.0, f64::from(blip[2]) / 4096.0];
        let Some(pixels) = camera.world_to_screen(world) else {
            continue;
        };
        let center = Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point);
        let corners = vec![center + Vec2::new(0.0, -6.0), center + Vec2::new(5.5, 4.0), center + Vec2::new(-5.5, 4.0)];
        painter.add(egui::Shape::convex_polygon(corners, color.gamma_multiply(0.35), Stroke::new(1.5_f32, color)));
    }
}

/// Flashes a line from each missile defense to the missile it shot down this tick.
pub fn draw_interceptions(ctx: &egui::Context, camera: &Camera, interceptions: &[[[i32; 3]; 2]], pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let to_points = |raw: [i32; 3]| {
        let world = [f64::from(raw[0]) / 4096.0, f64::from(raw[1]) / 4096.0, f64::from(raw[2]) / 4096.0];
        camera.world_to_screen(world).map(|pixels| Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point))
    };
    let color = Color32::from_rgb(255, 120, 90);
    for &[from, to] in interceptions {
        let (Some(from), Some(to)) = (to_points(from), to_points(to)) else {
            continue;
        };
        painter.line_segment([from, to], Stroke::new(6.0_f32, color.gamma_multiply(0.25)));
        painter.line_segment([from, to], Stroke::new(2.0_f32, Color32::WHITE));
        painter.circle_filled(to, 6.0, Color32::from_rgb(255, 230, 160));
    }
}

/// Tractor beams, from each holder to what it holds, and the storms roaming the battlefield
/// (GAME_SPECIFICATION.md §12).
pub fn draw_exotic(ctx: &egui::Context, camera: &Camera, heights: &crate::render::camera::HeightField, snapshot: &SimSnapshot, pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let to_points = |world: [f64; 3]| camera.world_to_screen(world).map(|pixels| Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point));
    let raw = |value: i32| f64::from(value) / 4096.0;
    for &[from, to] in &snapshot.tractor_beams {
        let (Some(from), Some(to)) = (to_points([raw(from[0]), raw(from[1]), raw(from[2])]), to_points([raw(to[0]), raw(to[1]), raw(to[2])])) else {
            continue;
        };
        painter.line_segment([from, to], Stroke::new(7.0_f32, TRACTOR.gamma_multiply(0.25)));
        painter.line_segment([from, to], Stroke::new(2.0_f32, TRACTOR));
    }
    let time = ctx.input(|input| input.time);
    for storm in &snapshot.storms {
        let (x, y) = (raw(storm.x), raw(storm.y));
        let ground = heights.height_at(x, y);
        let (Some(center), Some(edge)) = (to_points([x, y, ground]), to_points([x + raw(storm.radius), y, ground])) else {
            continue;
        };
        let radius = (edge - center).length().max(6.0);
        let flicker = ((time * 9.0).sin() * 0.5 + 0.5) as f32;
        painter.circle_filled(center, radius, STORM.gamma_multiply(0.18 + flicker * 0.12));
        painter.circle_stroke(center, radius, Stroke::new(2.0_f32, STORM));
    }
}

/// Rings where the viewer's teleports will land, filling as they charge.
pub fn draw_teleports(ctx: &egui::Context, camera: &Camera, heights: &crate::render::camera::HeightField, teleports: &[(SimId, [i32; 2], u16)], pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let color = Color32::from_rgb(120, 220, 255);
    for &(_, [x, y], permille) in teleports {
        let (x, y) = (f64::from(x) / 4096.0, f64::from(y) / 4096.0);
        let Some(pixels) = camera.world_to_screen([x, y, heights.height_at(x, y)]) else {
            continue;
        };
        let center = Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point);
        painter.circle_stroke(center, 16.0, Stroke::new(2.0_f32, color));
        painter.circle_filled(center, 16.0 * f32::from(permille) / 1000.0, color.gamma_multiply(0.35));
    }
}

/// Marks where an extractor can go while placing one, since deposits are small from far away.
pub fn deposit_highlights(ctx: &egui::Context, camera: &Camera, free: &[[f64; 3]], target: Option<[f64; 3]>, pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let color = marker_color(MarkerKind::Build);
    let to_points = |world: [f64; 3]| {
        camera.world_to_screen(world).map(|pixels| Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point))
    };
    for &deposit in free {
        if let Some(center) = to_points(deposit) {
            painter.circle_stroke(center, 9.0, Stroke::new(1.5_f32, color.gamma_multiply(0.7)));
        }
    }
    if let Some(center) = target.and_then(to_points) {
        painter.circle_stroke(center, 13.0, Stroke::new(2.5_f32, color));
    }
}

/// Keeps selection visible when units are too small on screen for their ground rings to show.
/// Units already showing a tactical icon are skipped, since the icon turns green instead.
pub fn distant_selection_markers(ctx: &egui::Context, camera: &Camera, snapshot: &SimSnapshot, selected: &[SimId], pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let stroke = Stroke::new(1.5_f32, Color32::from_rgb(70, 255, 100));
    let raw = |value: i32| f64::from(value) / 4096.0;
    let viewport = ctx.screen_rect().expand(8.0);
    let icons_showing = camera.altitude() >= ICON_ALTITUDE;
    for &id in selected {
        let Some(unit) = find_unit(snapshot, id) else {
            continue;
        };
        if icons_showing && unit.blueprint != u16::MAX {
            continue;
        }
        let world = [raw(unit.x), raw(unit.y), raw(unit.z)];
        let (Some(center), Some(edge)) = (camera.world_to_screen(world), camera.world_to_screen([world[0] + raw(unit.radius), world[1], world[2]])) else {
            continue;
        };
        let apparent = (edge[0] - center[0]).hypot(edge[1] - center[1]) as f32 / pixels_per_point;
        let center = Pos2::new(center[0] as f32 / pixels_per_point, center[1] as f32 / pixels_per_point);
        if apparent < MIN_SELECTION_RING_POINTS && viewport.contains(center) {
            painter.rect_stroke(Rect::from_center_size(center, Vec2::splat(9.0)), Rounding::ZERO, stroke);
        }
    }
}

/// Draws a dimmed footprint for every queued build, so that the next one can be lined up.
pub fn pending_build_outlines(ctx: &egui::Context, camera: &Camera, builds: &[PendingBuild], pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let color = marker_color(MarkerKind::Build);
    let stroke = Stroke::new(2.0_f32, color.gamma_multiply(0.9));
    let fill = color.gamma_multiply(0.25);
    for build in builds {
        let half = build.width * 0.5;
        let [x, y, z] = build.center;
        let corners = [[x - half, y - half, z], [x + half, y - half, z], [x + half, y + half, z], [x - half, y + half, z]];
        let points: Vec<Pos2> = corners
            .into_iter()
            .filter_map(|corner| camera.world_to_screen(corner))
            .map(|pixels| Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point))
            .collect();
        if points.len() == corners.len() {
            painter.add(egui::Shape::convex_polygon(points, fill, stroke));
        }
    }
}

/// The grid lines around a placement, in world coordinates at the given height. Lines
/// fall on terrain cell edges, which is where structures snap to.
pub fn placement_grid_lines(center_cell: (u32, u32), cell_units: f64, height: f64, cells_per_side: u32) -> Vec<([f64; 3], [f64; 3])> {
    let clamp = |cell: i32| f64::from(cell.clamp(0, cells_per_side as i32)) * cell_units;
    let (center_x, center_y) = (center_cell.0 as i32, center_cell.1 as i32);
    let (min_x, max_x) = (clamp(center_x - GRID_REACH_CELLS), clamp(center_x + GRID_REACH_CELLS));
    let (min_y, max_y) = (clamp(center_y - GRID_REACH_CELLS), clamp(center_y + GRID_REACH_CELLS));
    let mut lines = Vec::new();
    for offset in -GRID_REACH_CELLS..=GRID_REACH_CELLS {
        let (x, y) = (center_x + offset, center_y + offset);
        if (0..=cells_per_side as i32).contains(&x) {
            lines.push(([clamp(x), min_y, height], [clamp(x), max_y, height]));
        }
        if (0..=cells_per_side as i32).contains(&y) {
            lines.push(([min_x, clamp(y), height], [max_x, clamp(y), height]));
        }
    }
    lines
}

/// Draws a faint grid around the structure being placed, unless the camera is too far out for it to help.
pub fn placement_grid(ctx: &egui::Context, camera: &Camera, lines: &[([f64; 3], [f64; 3])], cell_units: f64, pixels_per_point: f32) {
    let to_points = |world: [f64; 3]| {
        camera.world_to_screen(world).map(|pixels| Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point))
    };
    let Some(first) = lines.first() else {
        return;
    };
    // Measure one cell along the first line, near the middle of the grid.
    let mid = |a: f64, b: f64| (a + b) * 0.5;
    let middle = [mid(first.0[0], first.1[0]), mid(first.0[1], first.1[1]), first.0[2]];
    let next = [middle[0], middle[1] + cell_units, middle[2]];
    let (Some(a), Some(b)) = (to_points(middle), to_points(next)) else {
        return;
    };
    if a.distance(b) < MIN_GRID_CELL_POINTS {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::background());
    let stroke = Stroke::new(1.0_f32, Color32::from_white_alpha(46));
    let bounds = ctx.screen_rect().expand(16.0);
    for &(from, to) in lines {
        if let (Some(from), Some(to)) = (to_points(from), to_points(to)) {
            if let Some((start, end)) = clip_segment(from, to, bounds) {
                painter.line_segment([start, end], stroke);
            }
        }
    }
}

/// A small floating reminder of the placement in progress.
pub fn placement_hint(ctx: &egui::Context, db: &BlueprintDb, blueprint: BlueprintId) {
    egui::Area::new(egui::Id::new("placement hint"))
        .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 62.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                ui.label(format!("Placing {}: left-click to build, Shift-click to queue more, Shift-drag for a row, right-click to cancel", db.get(blueprint).name));
            });
        });
}

fn team_color(team: Team) -> Color32 {
    match team {
        Team::Blue => BLUE,
        Team::Red => RED,
    }
}

/// The name under a side's commander.
pub const fn commander_name(team: Team) -> &'static str {
    match team {
        Team::Blue => "Blue",
        Team::Red => "Red",
    }
}

/// Zoomed out, a mark on every mass deposit and geothermal vent, which the land itself no
/// longer shows: green for mass, amber for vents. Cells are terrain cells.
pub fn draw_deposit_markers(ctx: &egui::Context, camera: &Camera, heights: &crate::render::camera::HeightField, deposits: &[(u32, u32)], vents: &[(u32, u32)], pixels_per_point: f32) {
    let strength = ((camera.zoom_from_closest() - 0.35) / 0.15).clamp(0.0, 1.0) as f32;
    if strength <= 0.0 {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::background());
    let bounds = ctx.screen_rect().expand(8.0);
    let cell = f64::from(crate::sim::config::TERRAIN_CELL_UNITS);
    for (cells, color) in [(deposits, Color32::from_rgb(170, 240, 70)), (vents, Color32::from_rgb(255, 196, 70))] {
        for &(x, y) in cells {
            let (wx, wy) = ((f64::from(x) + 0.5) * cell, (f64::from(y) + 0.5) * cell);
            let Some(pixels) = camera.world_to_screen([wx, wy, heights.height_at(wx, wy)]) else {
                continue;
            };
            let at = Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point);
            if !bounds.contains(at) {
                continue;
            }
            painter.circle_filled(at, 5.5, color.gamma_multiply(0.25 * strength));
            painter.circle(at, 3.2, color.gamma_multiply(strength), Stroke::new(1.0_f32, Color32::from_black_alpha((170.0 * strength) as u8)));
        }
    }
}

/// Draws a unit's icon on the map: its kind's symbol in its frame, in the team's color, or pale
/// green when selected, with a dot beneath for each tech level.
fn draw_icon(painter: &egui::Painter, center: Pos2, key: &str, tier: u8, color: Color32, selected: bool) {
    let icon = Icon::for_key(key);
    icons::draw(painter, center, icons::map_half(icon), icon, tier, if selected { Color32::from_rgb(170, 255, 170) } else { color });
}

/// How far along one of the viewer's own upgrades is, from 0 to 1: an enhancement, an upgrade in
/// place, or a factory's upgrade at the front of its queue. `None` for anything else, and for
/// every enemy unit, whose progress the viewer is never told.
pub fn upgrade_progress(snapshot: &SimSnapshot, id: SimId, db: &BlueprintDb) -> Option<f32> {
    let share = |permille: u16| f32::from(permille) / 1000.0;
    if let Some(&(_, _, permille)) = snapshot.enhancing.iter().find(|(unit, _, _)| *unit == id) {
        return Some(share(permille));
    }
    if let Some(&(_, permille)) = snapshot.upgrades.iter().find(|(unit, _)| *unit == id) {
        return Some(share(permille));
    }
    let factory = snapshot.factories.iter().find(|factory| factory.id == id)?;
    let unit = find_unit(snapshot, id)?;
    let upgrade = db.get(BlueprintId(unit.blueprint)).upgrades_to?;
    (factory.queue.first() == Some(&upgrade.0)).then(|| share(factory.progress_permille))
}

/// How far along what one of the viewer's own factories is making is, from 0 to 1: a unit, or a
/// missile in a silo, such as the strategic missile submarine's. `None` while it makes nothing,
/// is unfinished, upgrades (which has a bar of its own), or holds all the missiles it can, and
/// for every enemy, whose progress the viewer is never told.
pub fn production_progress(snapshot: &SimSnapshot, id: SimId, db: &BlueprintDb) -> Option<f32> {
    let factory = snapshot.factories.iter().find(|factory| factory.id == id)?;
    let made = db.get(BlueprintId(*factory.queue.first()?));
    if made.is_factory() || find_unit(snapshot, id)?.built_permille < 1000 {
        return None;
    }
    if made.class == UnitClass::Ammo {
        let limit = db.get(BlueprintId(find_unit(snapshot, id)?.blueprint)).max_stock;
        let stock = snapshot.stocks.iter().find(|(silo, _)| *silo == id).map_or(0, |&(_, stock)| stock);
        if limit > 0 && stock >= limit {
            return None;
        }
    }
    Some(f32::from(factory.progress_permille) / 1000.0)
}

fn draw_bar(painter: &egui::Painter, center: Pos2, width: f32, share: f32, color: Color32) {
    let back = Rect::from_center_size(center, Vec2::new(width, 3.0));
    painter.rect_filled(back, Rounding::ZERO, Color32::from_black_alpha(180));
    let fill = Rect::from_min_size(back.min, Vec2::new(width * share.clamp(0.0, 1.0), 3.0));
    painter.rect_filled(fill, Rounding::ZERO, color);
}

/// Health bars, construction progress, and tactical icons, drawn behind the panels.
pub fn world_overlay(ctx: &egui::Context, camera: &Camera, snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb, pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let show_icons = camera.altitude() >= ICON_ALTITUDE;
    let to_points = |pixels: [f64; 2]| Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point);
    let raw = |value: i32| f64::from(value) / 4096.0;
    let viewport = ctx.screen_rect();
    let time = ctx.input(|input| input.time);
    // Where idle factories' markers go, drawn after every icon so that none covers them.
    let mut idle_marks: Vec<(Pos2, f32)> = Vec::new();
    for unit in &snapshot.units {
        if unit.blueprint == u16::MAX {
            continue;
        }
        let Some(pixels) = camera.world_to_screen([raw(unit.x), raw(unit.y), raw(unit.z)]) else {
            continue;
        };
        let center = to_points(pixels);
        if !viewport.expand(24.0).contains(center) {
            continue;
        }
        let is_selected = selected.binary_search(&unit.id).is_ok();
        let blueprint = db.get(BlueprintId(unit.blueprint));
        // How far the icon reaches above and below, with its tech level dots, for what is drawn
        // around it.
        let icon = Icon::for_key(&blueprint.key);
        let icon_half = icons::map_half(icon);
        let icon_top = icon_half * icons::frame_reach(icon.frame()).0;
        let icon_bottom = icons::reach_below(icon, icon_half, blueprint.tier);
        if show_icons {
            // A structure remembered out of sight shows dimmer, as it may be gone.
            let color = if unit.remembered { team_color(unit.team).gamma_multiply(0.55) } else { team_color(unit.team) };
            draw_icon(&painter, center, &blueprint.key, blueprint.tier, color, is_selected);
        }
        // Commanders carry their side's name, underlined in its color, so they can be found.
        if blueprint.is_commander() {
            let name = commander_name(unit.team);
            let below = center + Vec2::new(0.0, if show_icons { icon_bottom + 2.0 } else { 18.0 });
            let galley = painter.layout_no_wrap(name.to_string(), FontId::proportional(12.0), Color32::WHITE);
            let width = galley.size().x;
            hud::outlined_text(&painter, below, Align2::CENTER_TOP, name, FontId::proportional(12.0), Color32::WHITE);
            let underline = below.y + galley.size().y + 1.0;
            painter.line_segment([Pos2::new(below.x - width / 2.0, underline), Pos2::new(below.x + width / 2.0, underline)], Stroke::new(1.2_f32, team_color(unit.team)));
        }
        if snapshot.paused.binary_search(&unit.id).is_ok() {
            painter.text(center, Align2::CENTER_CENTER, "PAUSED", FontId::proportional(13.0), Color32::YELLOW);
        }
        // Stunned by an EMP, on either side, as long as it is in sight.
        if snapshot.stunned.binary_search(&unit.id).is_ok() {
            painter.text(center + Vec2::new(0.0, 16.0), Align2::CENTER_CENTER, "STUNNED", FontId::proportional(12.0), STUNNED);
        }
        // Only on what is selected: on every veteran at once, the pips cluttered the battlefield.
        let rank = if is_selected { rank_of(snapshot, unit.id) } else { 0 };
        if rank > 0 {
            // One gold pip per rank, under the unit, and under its icon's tech level dots.
            let spacing = 5.0;
            let left = center.x - spacing * f32::from(rank - 1) / 2.0;
            let below = if show_icons { icon_bottom + 4.0 } else { 10.0 };
            for pip in 0..rank {
                painter.circle_filled(Pos2::new(left + spacing * f32::from(pip), center.y + below), 2.0, VETERAN);
            }
        }
        // A factory with nothing to do, under its icon, so it can be found from afar.
        if snapshot.idle_factories.binary_search(&unit.id).is_ok() {
            let below = if show_icons { icon_bottom + 3.0 } else { 12.0 };
            let width = if show_icons { (icon_half * 2.6).max(24.0) } else { 28.0 };
            idle_marks.push((center + Vec2::new(0.0, below + 2.5), width));
        }
        let hurt = unit.health_permille < 1000;
        let building = unit.built_permille < 1000;
        let upgrading = snapshot.upgrading.binary_search(&unit.id).is_ok();
        let edge = camera.world_to_screen([raw(unit.x) + raw(unit.radius), raw(unit.y), raw(unit.z)]);
        let apparent = edge.map_or(8.0, |edge| ((edge[0] - pixels[0]).hypot(edge[1] - pixels[1]) as f32 / pixels_per_point).max(8.0));
        if upgrading {
            // The same pulsing ring on every upgrade, the enemy's included, as long as it is in sight.
            let pulse = ((time * UPGRADE_PULSE_RATE).sin() * 0.5 + 0.5) as f32;
            let radius = if show_icons { icon_half * 1.9 } else { apparent * 1.35 } + pulse * 3.0;
            let alpha = (120.0 + pulse * 135.0) as u8;
            painter.circle_stroke(center, radius, Stroke::new(2.0_f32, Color32::from_rgba_unmultiplied(UPGRADING.r(), UPGRADING.g(), UPGRADING.b(), alpha)));
        }
        // How far along it is, only for the viewer's own upgrades.
        let upgrade_share = upgrade_progress(snapshot, unit.id, db).filter(|_| upgrading);
        let production_share = production_progress(snapshot, unit.id, db);
        if hurt || building || is_selected || upgrade_share.is_some() || production_share.is_some() {
            let above = if show_icons { icon_top + 4.0 } else { apparent + 4.0 };
            let width = if show_icons { icon_half * 2.8 } else { (apparent * 2.0).clamp(14.0, 70.0) };
            let share = f32::from(unit.health_permille) / 1000.0;
            let color = if share > 0.5 { HEALTHY } else if share > 0.25 { Color32::YELLOW } else { STALLED };
            draw_bar(&painter, center - Vec2::new(0.0, above), width, share, color);
            let mut next = above + 4.0;
            if building {
                draw_bar(&painter, center - Vec2::new(0.0, next), width, f32::from(unit.built_permille) / 1000.0, BUILDING);
                next += 4.0;
            }
            if let Some(share) = upgrade_share {
                draw_bar(&painter, center - Vec2::new(0.0, next), width, share, UPGRADING);
                next += 4.0;
            }
            if let Some(share) = production_share {
                draw_bar(&painter, center - Vec2::new(0.0, next), width, share, BUILDING);
            }
        }
    }
    for (middle, width) in idle_marks {
        let bar = Rect::from_center_size(middle, Vec2::new(width, 5.0));
        hud::hatched(&painter, bar, IDLE, IDLE_STRIPE);
        painter.rect_stroke(bar, 0.0, Stroke::new(1.0_f32, IDLE_EDGE));
        hud::outlined_text(&painter, Pos2::new(middle.x, bar.bottom() + 1.0), Align2::CENTER_TOP, "IDLE", FontId::new(10.0, egui::FontFamily::Name(BOLD.into())), IDLE_EDGE);
    }
    if show_icons {
        painter.text(
            Pos2::new(viewport.right() - 8.0, viewport.bottom() - COMMAND_BAR_HEIGHT - 6.0),
            Align2::RIGHT_BOTTOM,
            format!("{} visible units", snapshot.units.len()),
            FontId::proportional(12.0),
            Color32::from_white_alpha(120),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::world::{BuildQueueView, FactoryView, OrderStopView, PlanStep, UnitPlanView, UnitSpendingView};

    fn raw(value: f64) -> i32 {
        (value * 4096.0) as i32
    }

    fn unit(id: u32, x: f64) -> UnitView {
        UnitView {
            id: SimId(id),
            team: Team::Blue,
            x: raw(x),
            y: 0,
            z: 0,
            radius: raw(8.0),
            blueprint: u16::MAX,
            health_permille: 1000,
            built_permille: 1000,
            is_structure: false,
            facing: [0, 0], aim: [0, 0], extra_aims: [[0; 2]; 3],
            working: false,
            assembling: None,
            upgrade: None,
            remembered: false,
            ally: false,
        }
    }

    fn step(kind: PlanStepKind, x: f64, pending_width: f64) -> PlanStep {
        PlanStep { kind, x: raw(x), y: 0, z: 0, pending_width: raw(pending_width) }
    }

    fn snapshot(units: Vec<UnitView>, plans: Vec<UnitPlanView>) -> SimSnapshot {
        SimSnapshot { units, plans, ..SimSnapshot::default() }
    }

    fn factory_view(id: u32, queue: Vec<u16>, repeat: bool, progress_permille: u16) -> FactoryView {
        FactoryView { id: SimId(id), queue, repeat, progress_permille, rally: None }
    }

    /// One T1 factory, one T2 factory, and a T1 factory still under construction.
    fn mixed_factories(db: &BlueprintDb) -> SimSnapshot {
        let factory_unit = |id: u32, key: &str, built: u16| {
            let mut view = unit(id, 0.0);
            view.blueprint = db.id(key).0;
            view.is_structure = true;
            view.built_permille = built;
            view
        };
        let mut current = snapshot(
            vec![factory_unit(1, "land_factory_t1", 1000), factory_unit(2, "land_factory_t2", 1000), factory_unit(3, "land_factory_t1", 400)],
            vec![],
        );
        current.factories = vec![factory_view(1, vec![], false, 0), factory_view(2, vec![], false, 0), factory_view(3, vec![], false, 0)];
        current
    }

    #[test]
    fn a_shared_key_queues_each_option_in_its_own_factories() {
        let db = BlueprintDb::standard();
        let structure = |id: u32, key: &str| {
            let mut view = unit(id, 0.0);
            view.blueprint = db.id(key).0;
            view.is_structure = true;
            view
        };
        let mut current = snapshot(vec![structure(1, "land_factory_t1"), structure(2, "air_factory"), structure(3, "rig_yard"), structure(4, "tactical_missile_launcher")], vec![]);
        current.factories = (1..=4).map(|id| factory_view(id, vec![], false, 0)).collect();
        let queue = |factory: u32, key: &str, selected_count: usize| UiAction::Queue { factories: vec![SimId(factory)], blueprint: db.id(key), selected_count, count: 1 };
        let both = hotkey_action(&current, &[SimId(1), SimId(2)], &db, 'Q', BuildTiers::default(), None).expect("the first slot");
        assert_eq!(both, UiAction::Batch(vec![queue(1, "t1_engineer", 2), queue(2, "t1_air_scout", 2)]), "an engineer in the land factory and a scout in the air factory");
        let UiAction::Batch(shifted) = with_shift(both, true, &db) else { panic!("still a batch") };
        assert!(shifted.iter().all(|action| matches!(action, UiAction::Queue { count: SHIFT_QUEUE_COUNT, .. })));
        // A gateway's and a silo's only items are in their first slot too.
        assert_eq!(hotkey_action(&current, &[SimId(3)], &db, 'Q', BuildTiers::default(), None), Some(queue(3, "field_rig", 1)));
        assert_eq!(hotkey_action(&current, &[SimId(4)], &db, 'Q', BuildTiers::default(), None), Some(queue(4, "tactical_missile", 1)));
    }

    #[test]
    fn a_build_key_picks_from_the_level_the_bar_shows() {
        let db = BlueprintDb::standard();
        let mut current = commander_and_engineer(&db);
        current.enhancements = vec![(SimId(10), [u16::MAX, db.enhancement_id("t3_fabrication").0, u16::MAX])];
        let press = |letter: char, builder: Option<u8>| hotkey_action(&current, &[SimId(10)], &db, letter, BuildTiers { builder, factory: None }, None);
        let place = |key: &str| Some(UiAction::BeginPlacement(db.id(key)));
        assert_eq!(press('E', None), place("mass_extractor_t3"), "the highest level at first");
        assert_eq!(press('E', Some(1)), place("mass_extractor"));
        // The same kind of thing is on one key at every level.
        assert_eq!(press('E', Some(2)), place("mass_extractor_t2"));
        assert_eq!(press('E', Some(3)), place("mass_extractor_t3"));
        assert_eq!(press('S', Some(3)), place("power_generator_t3"));
        assert_eq!(press('Z', Some(2)), place("radar_t2"));
        assert_eq!(press('Z', Some(3)), place("deep_scan_array"));
        assert_eq!(press('D', Some(2)), place("t2_point_defense"));
        assert_eq!(press('D', Some(3)), place("t3_heavy_point_defense"));
        assert_eq!(press('W', Some(3)), place("rig_yard"), "a support factory needs a headquarters, and there is none");
        assert_eq!(press('V', Some(2)), place("field_workshop"));
        assert_eq!(press('V', Some(1)), place("wall"));
        assert_eq!(press('Q', Some(1)), None, "Q is a factory's key");
        // A bot can build nothing, so N mutes instead.
        assert_eq!(hotkey_action(&current, &[SimId(12)], &db, 'N', BuildTiers::default(), None), None);
    }

    #[test]
    fn support_factories_are_offered_once_there_is_a_headquarters_of_their_kind() {
        let db = BlueprintDb::standard();
        let mut current = commander_and_engineer(&db);
        current.enhancements = vec![(SimId(10), [u16::MAX, db.enhancement_id("t3_fabrication").0, u16::MAX])];
        let offered = |current: &SimSnapshot, key: &str| builder_options(current, &[SimId(10)], &db).contains(&db.id(key));
        assert!(!offered(&current, "land_factory_t2_support"));
        let mut headquarters = unit(20, 0.0);
        headquarters.blueprint = db.id("land_factory_t2").0;
        headquarters.is_structure = true;
        current.units.push(headquarters);
        assert!(offered(&current, "land_factory_t2_support"), "with a T2 land headquarters");
        assert!(!offered(&current, "land_factory_t3_support"), "but not T3");
        assert!(!offered(&current, "air_factory_t2_support"), "nor air");
        current.units.last_mut().unwrap().built_permille = 500;
        assert!(!offered(&current, "land_factory_t2_support"), "it must be finished");
    }

    #[test]
    fn the_tier_key_steps_through_the_levels_a_bar_has() {
        let db = BlueprintDb::standard();
        let options: Vec<BlueprintId> = ["power_generator", "shield_generator", "rig_yard"].iter().map(|key| db.id(key)).collect();
        assert_eq!(next_tier(None, &options, &db), Some(1), "from the highest, shown first, round to the lowest");
        assert_eq!(next_tier(Some(1), &options, &db), Some(2));
        assert_eq!(next_tier(Some(2), &options, &db), Some(3));
        assert_eq!(next_tier(Some(3), &options, &db), Some(1), "and round again");
        assert_eq!(next_tier(Some(1), &options[..1], &db), Some(1), "with one level, it stays");
        // The bar lists its items by slot, which is the order of their keys.
        let bar: Vec<String> = bar_items(&[db.id("radar"), db.id("mass_extractor"), db.id("land_factory_t1")], 1, &db).into_iter().map(|id| db.get(id).key.clone()).collect();
        assert_eq!(bar, ["land_factory_t1", "mass_extractor", "radar"]);
        // Grouped: factories, then resources, defenses, and intel, whatever their slots.
        let options: Vec<BlueprintId> = ["radar", "t1_point_defense", "t1_aa_turret", "power_generator", "air_factory", "mass_extractor", "land_factory_t1"].iter().map(|key| db.id(key)).collect();
        let bar: Vec<String> = bar_items(&options, 1, &db).into_iter().map(|id| db.get(id).key.clone()).collect();
        assert_eq!(bar, ["land_factory_t1", "air_factory", "mass_extractor", "power_generator", "t1_point_defense", "t1_aa_turret", "radar"]);
    }

    #[test]
    fn the_menu_lists_everything_any_factory_can_build_and_who_can() {
        let db = BlueprintDb::standard();
        let current = mixed_factories(&db);
        let menu = factory_menu(&current, &[SimId(1), SimId(2), SimId(3)], &db).unwrap();
        assert_eq!(menu.factories, [SimId(1), SimId(2), SimId(3)], "unfinished factories take orders too");
        assert_eq!(menu.unfinished, 1);
        let able = |key: &str| menu.options.iter().find(|(option, _)| *option == db.id(key)).map(|(_, able)| able.clone());
        assert_eq!(able("t1_light_walker").unwrap(), [SimId(1), SimId(2), SimId(3)]);
        assert_eq!(able("t2_heavy_tank").unwrap(), [SimId(2)], "only T2 factories can build tanks");
        assert_eq!(menu.upgrade, Some((db.id("land_factory_t2"), vec![SimId(1), SimId(3)])), "only T1 factories can upgrade");
    }

    #[test]
    fn repeat_can_be_off_on_or_mixed() {
        let db = BlueprintDb::standard();
        let mut current = mixed_factories(&db);
        let state = |current: &SimSnapshot| factory_menu(current, &[SimId(1), SimId(2)], &db).unwrap().repeat;
        assert_eq!(state(&current), RepeatState::Off);
        current.factories[0].repeat = true;
        assert_eq!(state(&current), RepeatState::Mixed);
        current.factories[1].repeat = true;
        assert_eq!(state(&current), RepeatState::On);
    }

    #[test]
    fn the_queue_is_combined_and_an_upgrade_under_way_is_not_offered_again() {
        let db = BlueprintDb::standard();
        let (bot, upgrade) = (db.id("t1_light_walker"), db.id("land_factory_t2"));
        let mut current = mixed_factories(&db);
        current.factories[0] = factory_view(1, vec![upgrade.0, bot.0], false, 200);
        current.factories[1] = factory_view(2, vec![bot.0, bot.0], false, 400);
        let menu = factory_menu(&current, &[SimId(1), SimId(2)], &db).unwrap();
        assert_eq!(menu.queue, [(upgrade, 1), (bot, 3)]);
        assert_eq!(menu.progress_permille, Some(300));
        assert_eq!(menu.upgrade, Some((db.id("land_factory_t3"), vec![SimId(2)])), "only the T2 factory, to T3");
    }

    #[test]
    fn a_factory_under_construction_has_a_menu_and_shows_its_queue() {
        let db = BlueprintDb::standard();
        let bot = db.id("t1_light_walker");
        let mut current = mixed_factories(&db);
        current.factories[2] = factory_view(3, vec![bot.0], false, 0);
        let menu = factory_menu(&current, &[SimId(3)], &db).unwrap();
        assert_eq!(menu.factories, [SimId(3)]);
        assert_eq!(menu.unfinished, 1);
        assert_eq!(menu.queue, [(bot, 1)]);
        assert_eq!(menu.progress_permille, None, "nothing is being made until it is built");
    }

    #[test]
    fn an_unfinished_factory_does_not_drag_down_the_progress_bar() {
        let db = BlueprintDb::standard();
        let bot = db.id("t1_light_walker");
        let mut current = mixed_factories(&db);
        current.factories[0] = factory_view(1, vec![bot.0], false, 600);
        current.factories[2] = factory_view(3, vec![bot.0], false, 0);
        let menu = factory_menu(&current, &[SimId(1), SimId(3)], &db).unwrap();
        assert_eq!(menu.progress_permille, Some(600));
        assert_eq!(menu.queue, [(bot, 2)]);
    }

    #[test]
    fn a_factory_with_an_upgrade_queued_offers_what_the_upgrade_will_build() {
        let db = BlueprintDb::standard();
        let (upgrade, tank, bot) = (db.id("land_factory_t2"), db.id("t2_heavy_tank"), db.id("t1_light_walker"));
        let mut current = mixed_factories(&db);
        let able = |current: &SimSnapshot, selected: &[SimId], option| {
            factory_menu(current, selected, &db).unwrap().options.iter().find(|(existing, _)| *existing == option).map(|(_, able)| able.clone())
        };
        assert_eq!(able(&current, &[SimId(1)], tank), None, "without an upgrade, no tanks");

        current.factories[0] = factory_view(1, vec![upgrade.0], false, 200);
        assert_eq!(able(&current, &[SimId(1)], tank).unwrap(), [SimId(1)]);
        assert_eq!(able(&current, &[SimId(1)], bot).unwrap(), [SimId(1)], "listed once, though both tiers can build it");
        // The tank is in the medium tank's slot, at T2.
        let expected = UiAction::Queue { factories: vec![SimId(1)], blueprint: tank, selected_count: 1, count: 1 };
        assert_eq!(hotkey_action(&current, &[SimId(1)], &db, 'C', BuildTiers { builder: None, factory: Some(2) }, None), Some(expected));

        // The other T1 factory has no upgrade queued, so it is left out.
        assert_eq!(able(&current, &[SimId(1), SimId(3)], tank).unwrap(), [SimId(1)]);
    }

    #[test]
    fn no_factory_means_no_menu() {
        let db = BlueprintDb::standard();
        assert_eq!(factory_menu(&mixed_factories(&db), &[SimId(99)], &db), None);
        assert_eq!(factory_menu(&mixed_factories(&db), &[], &db), None);
    }

    #[test]
    fn the_note_says_when_queues_will_start() {
        assert_eq!(construction_note(0, 2), None);
        assert_eq!(construction_note(1, 1).unwrap(), "Under construction: the queue starts once built.");
        assert_eq!(construction_note(2, 2).unwrap(), "Under construction: the queue starts once built.");
        assert_eq!(construction_note(1, 3).unwrap(), "1 of 3 under construction: their queues start once built.");
    }

    fn commander_and_engineer(db: &BlueprintDb) -> SimSnapshot {
        let mut commander = unit(10, 0.0);
        commander.blueprint = db.id("command_rig").0;
        let mut engineer = unit(11, 0.0);
        engineer.blueprint = db.id("t1_engineer").0;
        let mut bot = unit(12, 0.0);
        bot.blueprint = db.id("t1_light_walker").0;
        snapshot(vec![commander, engineer, bot], vec![])
    }

    #[test]
    fn pressing_a_family_key_again_steps_through_the_family_and_round() {
        let db = BlueprintDb::standard();
        let current = commander_and_engineer(&db);
        let press = |letter: char, placing: Option<&str>| hotkey_action(&current, &[SimId(11)], &db, letter, BuildTiers::default(), placing.map(|key| db.id(key)));
        let place = |key: &str| Some(UiAction::BeginPlacement(db.id(key)));
        assert_eq!(press('W', None), place("land_factory_t1"));
        assert_eq!(press('W', Some("land_factory_t1")), place("air_factory"));
        assert_eq!(press('W', Some("air_factory")), place("naval_factory"));
        assert_eq!(press('W', Some("naval_factory")), place("land_factory_t1"), "and round to the first");
        assert_eq!(press('Z', None), place("radar"));
        assert_eq!(press('Z', Some("radar")), place("t1_sonar"));
        assert_eq!(press('D', None), place("t1_point_defense"));
        assert_eq!(press('D', Some("t1_point_defense")), place("t1_aa_turret"));
        assert_eq!(press('D', Some("t1_aa_turret")), place("t1_torpedo_launcher"));
        // Placing something of another family starts this one from its first.
        assert_eq!(press('D', Some("air_factory")), place("t1_point_defense"));
        let bar: Vec<String> = bar_items(&builder_options(&current, &[SimId(11)], &db), 1, &db).iter().map(|id| db.get(*id).key.clone()).collect();
        let at = |key: &str| bar.iter().position(|item| item == key).unwrap();
        assert!(at("land_factory_t1") < at("air_factory") && at("air_factory") < at("naval_factory") && at("naval_factory") < at("mass_extractor"), "a family sits together in the bar: {bar:?}");
    }

    #[test]
    fn builder_keys_begin_placing_the_same_structures_as_the_buttons() {
        let db = BlueprintDb::standard();
        let current = commander_and_engineer(&db);
        for (letter, key) in [('W', "land_factory_t1"), ('E', "mass_extractor"), ('S', "power_generator"), ('Z', "radar"), ('D', "t1_point_defense")] {
            for selected in [vec![SimId(10)], vec![SimId(11)], vec![SimId(10), SimId(11)]] {
                assert_eq!(hotkey_action(&current, &selected, &db, letter, BuildTiers::default(), None), Some(UiAction::BeginPlacement(db.id(key))), "{letter} for {selected:?}");
            }
        }
    }

    #[test]
    fn a_commander_builds_higher_tiers_only_with_its_engineering_suites() {
        let db = BlueprintDb::standard();
        let mut current = commander_and_engineer(&db);
        let (shield, nuke) = (db.id("shield_generator"), db.id("strategic_missile_launcher"));
        assert!(!builder_options(&current, &[SimId(10)], &db).contains(&shield), "T1 only at first");
        current.enhancements = vec![(SimId(10), [u16::MAX, db.enhancement_id("t2_fabrication").0, u16::MAX])];
        let options = builder_options(&current, &[SimId(10)], &db);
        assert!(options.contains(&shield) && !options.contains(&nuke), "T2 with the T2 suite");
        current.enhancements = vec![(SimId(10), [u16::MAX, db.enhancement_id("t3_fabrication").0, u16::MAX])];
        assert!(builder_options(&current, &[SimId(10)], &db).contains(&nuke), "T3 with the T3 suite");
        // With an engineer selected too, only what both can build.
        assert!(!builder_options(&current, &[SimId(10), SimId(11)], &db).contains(&shield));
        // A suite still being fitted already offers what it unlocks, to queue behind it.
        current.enhancements = Vec::new();
        current.enhancing = vec![(SimId(10), db.enhancement_id("t2_fabrication").0, 300)];
        let options = builder_options(&current, &[SimId(10)], &db);
        assert!(options.contains(&shield) && !options.contains(&nuke), "T2 while the T2 suite is fitted");
    }

    /// Every factory, silo, and missile submarine of the viewer's that is making something shows
    /// how far along it is; one making nothing, unfinished, upgrading, or with its silo full does
    /// not, nor does an enemy's, which the viewer is not told about.
    #[test]
    fn whatever_makes_units_or_missiles_shows_how_far_along_the_current_one_is() {
        let db = BlueprintDb::standard();
        let makers: Vec<_> = db.iter().filter(|def| def.is_factory() && !def.builds.is_empty()).collect();
        assert!(makers.iter().any(|def| def.key == "t3_missile_sub"), "the missile submarine makes missiles");
        for def in makers {
            let mut maker = unit(20, 0.0);
            maker.blueprint = def.id.0;
            let made = def.builds[0];
            let mut current = snapshot(vec![maker], vec![]);
            current.factories = vec![FactoryView { id: SimId(20), queue: vec![made.0], repeat: false, progress_permille: 350, rally: None }];
            assert_eq!(production_progress(&current, SimId(20), &db), Some(0.35), "{} making {}", def.key, db.get(made).key);
            current.factories[0].queue.clear();
            assert_eq!(production_progress(&current, SimId(20), &db), None, "{} making nothing", def.key);
            current.factories[0].queue = vec![made.0];
            current.units[0].built_permille = 600;
            assert_eq!(production_progress(&current, SimId(20), &db), None, "{} unfinished", def.key);
            current.units[0].built_permille = 1000;
            if let Some(upgrade) = def.upgrades_to {
                current.factories[0].queue = vec![upgrade.0, made.0];
                assert_eq!(production_progress(&current, SimId(20), &db), None, "{} upgrading has a bar of its own", def.key);
                current.factories[0].queue = vec![made.0];
            }
            if db.get(made).class == UnitClass::Ammo && def.max_stock > 0 {
                current.stocks = vec![(SimId(20), def.max_stock)];
                assert_eq!(production_progress(&current, SimId(20), &db), None, "{} with a full silo builds no more", def.key);
                current.stocks.clear();
            }
            current.factories.clear();
            assert_eq!(production_progress(&current, SimId(20), &db), None, "{} is an enemy's", def.key);
        }
    }

    #[test]
    fn only_the_viewers_own_upgrades_show_how_far_along_they_are() {
        let db = BlueprintDb::standard();
        let mut current = commander_and_engineer(&db);
        current.enhancing = vec![(SimId(10), db.enhancement_id("t2_fabrication").0, 400)];
        current.upgrading = vec![SimId(10), SimId(12)];
        assert_eq!(upgrade_progress(&current, SimId(10), &db), Some(0.4));
        // Listed as upgrading, but with nothing more known, as for an enemy.
        assert_eq!(upgrade_progress(&current, SimId(12), &db), None);
    }

    #[test]
    fn enhancements_are_offered_slot_by_slot_once_what_they_build_on_is_there() {
        let db = BlueprintDb::standard();
        let mut current = commander_and_engineer(&db);
        assert!(enhancement_offers(&current, &[SimId(11)], &db).is_empty(), "an engineer has none");
        let rows = enhancement_offers(&current, &[SimId(10), SimId(11)], &db);
        assert_eq!(rows.iter().map(|(slot, _)| *slot).collect::<Vec<_>>(), Slot::ALL);
        let offer = |rows: &[(Slot, Vec<EnhancementOffer>)], key: &str| {
            rows.iter().flat_map(|(_, offers)| offers).find(|offer| offer.enhancement == db.enhancement_id(key)).cloned().expect("listed")
        };
        assert_eq!(offer(&rows, "t2_fabrication").units, [SimId(10)]);
        assert!(offer(&rows, "t3_fabrication").units.is_empty(), "it needs the T2 suite first");

        current.enhancements = vec![(SimId(10), [u16::MAX, db.enhancement_id("t2_fabrication").0, u16::MAX])];
        let rows = enhancement_offers(&current, &[SimId(10)], &db);
        let installed = offer(&rows, "t2_fabrication");
        assert!(installed.installed && installed.units.is_empty());
        assert_eq!(offer(&rows, "t3_fabrication").units, [SimId(10)]);
        assert_eq!(offer(&rows, "damage_control").replaces, None, "it goes in the systems bay, beside the pack");
        assert!(enhancement_description(db.enhancement(db.enhancement_id("t3_fabrication")), &db).contains("Builds on T2 Fabrication Pack"));

        // With the T3 suite, the T2 one is part of it: shown as there, and not offered again.
        current.enhancements = vec![(SimId(10), [u16::MAX, db.enhancement_id("t3_fabrication").0, db.enhancement_id("thermal_camouflage").0])];
        let rows = enhancement_offers(&current, &[SimId(10)], &db);
        for (key, included) in [("t3_fabrication", true), ("t2_fabrication", true), ("damage_control", false), ("thermal_camouflage", true), ("signature_suppression", true)] {
            let offer = offer(&rows, key);
            assert_eq!((offer.installed, offer.units.is_empty()), (included, included), "{key}");
        }
    }

    #[test]
    fn only_commanders_with_a_teleporter_can_teleport() {
        let db = BlueprintDb::standard();
        let mut current = commander_and_engineer(&db);
        assert!(teleporters(&current, &[SimId(10), SimId(11)], &db).is_empty());
        current.enhancements = vec![(SimId(10), [u16::MAX, u16::MAX, db.enhancement_id("teleporter").0])];
        assert_eq!(teleporters(&current, &[SimId(10), SimId(11)], &db), [SimId(10)]);
    }

    #[test]
    fn a_teleport_needs_a_spot_in_sight_or_on_radar() {
        let mut current = snapshot(vec![], vec![]);
        assert!(senses_spot(&current, [100.0, 100.0]), "without fog, anywhere");
        // Four cells of 80 a side, with only the first in sight.
        current.fog = Some(std::sync::Arc::new(vec![255, 90, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        assert!(senses_spot(&current, [40.0, 40.0]));
        assert!(!senses_spot(&current, [120.0, 40.0]), "explored is not in sight");
        current.sensor_ranges = vec![crate::sim::world::SensorRangeView { center: [200 * 4096, 200 * 4096], radar: 100 * 4096, deep_scan: 0 }];
        assert!(senses_spot(&current, [250.0, 250.0]), "on radar");
        assert!(!senses_spot(&current, [300.0, 300.0]));
    }

    #[test]
    fn keys_do_nothing_when_the_selection_has_no_such_option() {
        let db = BlueprintDb::standard();
        let current = commander_and_engineer(&db);
        assert_eq!(hotkey_action(&current, &[], &db, 'E', BuildTiers::default(), None), None, "nothing is selected");
        assert_eq!(hotkey_action(&current, &[SimId(12)], &db, 'E', BuildTiers::default(), None), None, "a bot cannot build");
        assert_eq!(hotkey_action(&current, &[SimId(10)], &db, 'N', BuildTiers::default(), None), None, "nothing in that slot at T1");
        assert_eq!(hotkey_action(&current, &[SimId(10)], &db, 'R', BuildTiers::default(), None), None, "a commander has no repeat");
        let factories = mixed_factories(&db);
        let tank_level = BuildTiers { builder: None, factory: Some(2) };
        let bot = UiAction::Queue { factories: vec![SimId(1)], blueprint: db.id("t1_light_walker"), selected_count: 1, count: 1 };
        assert_eq!(hotkey_action(&factories, &[SimId(1)], &db, 'E', tank_level, None), Some(bot), "a T1 factory shows T1, so no grenadier walker");
        assert_eq!(hotkey_action(&factories, &[SimId(1)], &db, 'H', BuildTiers::default(), None), None, "nothing in that slot of a factory's bar");
    }

    #[test]
    fn factory_keys_reach_every_factory_able_to_follow_them() {
        let db = BlueprintDb::standard();
        let current = mixed_factories(&db);
        let selected = [SimId(1), SimId(2), SimId(3)];
        // Each bar opens at the highest level it has; these keys are about T1.
        const T1_BARS: BuildTiers = BuildTiers { builder: Some(1), factory: Some(1) };
        let queue = |key: &str, factories: Vec<SimId>| UiAction::Queue { factories, blueprint: db.id(key), selected_count: 3, count: 1 };
        assert_eq!(hotkey_action(&current, &selected, &db, 'Q', T1_BARS, None), Some(queue("t1_engineer", selected.to_vec())));
        assert_eq!(hotkey_action(&current, &selected, &db, 'E', T1_BARS, None), Some(queue("t1_light_walker", selected.to_vec())));
        assert_eq!(hotkey_action(&current, &selected, &db, 'G', T1_BARS, None), Some(queue("t1_light_artillery", selected.to_vec())));
        assert_eq!(hotkey_action(&current, &selected, &db, 'T', T1_BARS, None), Some(queue("t1_mobile_aa", selected.to_vec())));
        let t2 = BuildTiers { builder: None, factory: Some(2) };
        assert_eq!(hotkey_action(&current, &selected, &db, 'C', t2, None), Some(queue("t2_heavy_tank", vec![SimId(2)])));
        assert_eq!(hotkey_action(&current, &selected, &db, 'E', t2, None), Some(queue("t2_grenadier_walker", vec![SimId(2)])));
        assert_eq!(hotkey_action(&current, &selected, &db, 'U', T1_BARS, None), Some(queue("land_factory_t2", vec![SimId(1), SimId(3)])), "the lowest tier goes first");
        let to_t3 = UiAction::Queue { factories: vec![SimId(2)], blueprint: db.id("land_factory_t3"), selected_count: 1, count: 1 };
        assert_eq!(hotkey_action(&current, &[SimId(2)], &db, 'U', T1_BARS, None), Some(to_t3));
        assert_eq!(hotkey_action(&current, &selected, &db, 'K', T1_BARS, None), Some(UiAction::ClearQueue { factories: selected.to_vec() }));
    }

    #[test]
    fn the_repeat_key_toggles_like_the_checkbox() {
        let db = BlueprintDb::standard();
        let mut current = mixed_factories(&db);
        let selected = [SimId(1), SimId(2)];
        let press = |current: &SimSnapshot| hotkey_action(current, &selected, &db, 'R', BuildTiers::default(), None);
        let set = |repeat: bool| Some(UiAction::SetRepeat { factories: selected.to_vec(), repeat });
        assert_eq!(press(&current), set(true));
        current.factories[0].repeat = true;
        assert_eq!(press(&current), set(true), "a mixed selection turns on for all");
        current.factories[1].repeat = true;
        assert_eq!(press(&current), set(false));
    }

    #[test]
    fn with_a_builder_and_a_factory_selected_the_factory_takes_the_build_keys() {
        let db = BlueprintDb::standard();
        let mut current = mixed_factories(&db);
        let mut engineer = unit(4, 0.0);
        engineer.blueprint = db.id("t1_engineer").0;
        current.units.push(engineer);
        let selected = [SimId(1), SimId(4)];
        let queue = |key: &str| Some(UiAction::Queue { factories: vec![SimId(1)], blueprint: db.id(key), selected_count: 1, count: 1 });
        assert_eq!(hotkey_action(&current, &selected, &db, 'E', BuildTiers::default(), None), queue("t1_light_walker"), "not the engineer's extractor");
        assert_eq!(hotkey_action(&current, &selected, &db, 'Q', BuildTiers::default(), None), queue("t1_engineer"));
        assert_eq!(hotkey_action(&current, &selected, &db, 'H', BuildTiers::default(), None), None, "the engineer's bar is not shown");
    }

    #[test]
    fn unit_order_keys_arm_their_orders_for_anything_that_can_walk() {
        let db = BlueprintDb::standard();
        let current = commander_and_engineer(&db);
        for selected in [SimId(10), SimId(11), SimId(12)] {
            for (letter, order) in [('M', ArmedOrder::Move), ('F', ArmedOrder::AttackMove), ('J', ArmedOrder::Formation), ('Y', ArmedOrder::Assist)] {
                assert_eq!(hotkey_action(&current, &[selected], &db, letter, BuildTiers::default(), None), Some(UiAction::Arm(order)), "{letter} for {selected:?}");
            }
            assert_eq!(hotkey_action(&current, &[selected], &db, 'X', BuildTiers::default(), None), Some(UiAction::Stop));
        }
        assert_eq!(hotkey_action(&current, &[], &db, 'M', BuildTiers::default(), None), None, "nothing is selected");
    }

    #[test]
    fn coverage_outline_follows_the_outer_edge_of_overlapping_circles() {
        let length = |pieces: &[[[f64; 2]; 2]]| pieces.iter().map(|[a, b]| (a[0] - b[0]).hypot(a[1] - b[1])).sum::<f64>();
        let tau = std::f64::consts::TAU;
        // One circle: its whole rim.
        let one = coverage_outline(&[([0.0, 0.0], 1_000.0)]);
        assert!((length(&one) - tau * 1_000.0).abs() < 20.0);
        // A circle inside another adds nothing.
        let nested = coverage_outline(&[([0.0, 0.0], 1_000.0), ([200.0, 0.0], 300.0)]);
        assert!((length(&nested) - length(&one)).abs() < 1.0, "the inner circle should not show");
        // Two the same draw one rim, not two.
        let twins = coverage_outline(&[([0.0, 0.0], 1_000.0), ([0.0, 0.0], 1_000.0)]);
        assert!((length(&twins) - length(&one)).abs() < 1.0);
        // Two overlapping by half a radius lose the arcs inside each other: a third of each rim.
        let pair = coverage_outline(&[([0.0, 0.0], 1_000.0), ([1_000.0, 0.0], 1_000.0)]);
        let expected = 2.0 * tau * 1_000.0 * (2.0 / 3.0);
        assert!((length(&pair) - expected).abs() < 60.0, "{} against {expected}", length(&pair));
        for [a, b] in &pair {
            let middle = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            assert!(middle[0].hypot(middle[1]) > 990.0 && (middle[0] - 1_000.0).hypot(middle[1]) > 990.0, "a piece inside the other circle");
        }
    }

    #[test]
    fn o_toggles_a_cloak_when_there_is_nothing_to_pause() {
        let db = BlueprintDb::standard();
        let mut current = commander_and_engineer(&db);
        current.cloaks = vec![(SimId(10), false, false)];
        assert_eq!(hotkey_action(&current, &[SimId(10)], &db, 'O', BuildTiers::default(), None), Some(UiAction::SetCloak { units: vec![SimId(10)], on: true }));
        current.cloaks = vec![(SimId(10), true, true)];
        assert_eq!(hotkey_action(&current, &[SimId(10)], &db, 'O', BuildTiers::default(), None), Some(UiAction::SetCloak { units: vec![SimId(10)], on: false }));
        assert_eq!(hotkey_action(&current, &[SimId(12)], &db, 'O', BuildTiers::default(), None), None, "a bot has no cloak");
    }

    #[test]
    fn a_shield_says_how_strong_it_is_or_when_it_will_be_back() {
        use crate::sim::components::Team;
        use crate::sim::world::ShieldView;
        let view = |up: bool, recharging: u32| ShieldView { id: SimId(5), team: Team::Blue, center: [0; 3], radius: 0, health_permille: 640, up, recharging };
        let with = |shield: ShieldView| SimSnapshot { shields: vec![shield], ..SimSnapshot::default() };
        assert_eq!(shield_status(&with(view(true, 0)), SimId(5)).as_deref(), Some("shield 64%"));
        assert_eq!(shield_status(&with(view(false, 291)), SimId(5)).as_deref(), Some("shield down, back in 30 s"));
        assert_eq!(shield_status(&with(view(false, 0)), SimId(5)).as_deref(), Some("shield off"));
        assert_eq!(shield_status(&with(view(true, 0)), SimId(6)), None, "only a shield generator has one");
    }

    #[test]
    fn attack_ground_is_only_offered_to_a_selection_with_a_gun() {
        let db = BlueprintDb::standard();
        let current = commander_and_engineer(&db);
        let armed: Vec<SimId> = current.units.iter().filter(|unit| db.get(BlueprintId(unit.blueprint)).weapon.is_some()).map(|unit| unit.id).collect();
        let unarmed: Vec<SimId> = current.units.iter().filter(|unit| db.get(BlueprintId(unit.blueprint)).weapon.is_none()).map(|unit| unit.id).collect();
        assert!(!armed.is_empty() && !unarmed.is_empty());
        assert_eq!(hotkey_action(&current, &armed[..1], &db, 'I', BuildTiers::default(), None), Some(UiAction::Arm(ArmedOrder::AttackGround)));
        assert_eq!(hotkey_action(&current, &unarmed[..1], &db, 'I', BuildTiers::default(), None), None);
    }

    #[test]
    fn structures_do_not_take_unit_orders_but_a_mixed_selection_does() {
        let db = BlueprintDb::standard();
        let mut current = mixed_factories(&db);
        for letter in ['M', 'F', 'J', 'Y', 'X'] {
            assert_eq!(hotkey_action(&current, &[SimId(1)], &db, letter, BuildTiers::default(), None), None, "{letter} for a factory");
        }
        let mut engineer = unit(4, 0.0);
        engineer.blueprint = db.id("t1_engineer").0;
        current.units.push(engineer);
        assert!(has_mobile_units(&current, &[SimId(1), SimId(4)]));
        assert!(!has_mobile_units(&current, &[SimId(1), SimId(2)]));
    }

    #[test]
    fn every_unit_order_says_what_to_do_next() {
        for order in ArmedOrder::ALL {
            assert!(order.hint().starts_with(order.name()), "{}", order.hint());
        }
    }

    #[test]
    fn every_button_label_shows_its_key_and_name_and_nothing_else() {
        let db = BlueprintDb::standard();
        assert_eq!(item_label(&db, db.id("mass_extractor")), "[E] Materiel Extractor");
        assert_eq!(item_label(&db, db.id("t1_light_walker")), "[E] Light Walker");
    }

    #[test]
    fn hovering_an_item_shows_its_cost_and_what_it_does() {
        let db = BlueprintDb::standard();
        let generator = item_details(&db, db.id("power_generator"));
        assert!(generator.contains("Costs 75 materiel and 750 energy"), "{generator}");
        assert!(generator.contains("Makes 20 energy a second"), "{generator}");
        let interceptor = item_details(&db, db.id("t1_interceptor"));
        assert!(interceptor.contains("at aircraft only"), "{interceptor}");
        let height = db.get(db.id("t1_interceptor")).flight.expect("interceptors fly").elevation;
        assert!(interceptor.contains(&format!("Speed 300 a second, flies at {height}")), "{interceptor}");
        let extractor = item_details(&db, db.id("mass_extractor"));
        assert!(extractor.contains("Built on a materiel deposit"), "{extractor}");
    }

    #[test]
    fn placing_shows_the_ranges_a_structure_reaches() {
        let db = BlueprintDb::standard();
        let names = |key: &str| placement_ranges(db.get(db.id(key))).into_iter().map(|range| range.0.name()).collect::<Vec<_>>();
        assert!(names("radar").contains(&"Radar"));
        assert!(names("t1_point_defense").contains(&"Ground fire"));
        assert!(names("shield_generator").contains(&"Shield"));
        assert!(names("t2_artillery").contains(&"Too close to hit"), "artillery cannot hit what is close");
        assert!(names("mass_storage").is_empty(), "a storage reaches nothing");
    }

    #[test]
    fn hotkeys_are_bold_and_the_rest_is_not() {
        let egui::WidgetText::LayoutJob(job) = bold_keys("[M] Move, then [Ctrl+C] Capture") else {
            panic!("a layout job");
        };
        let bold: Vec<&str> = job.sections.iter().filter(|section| section.format.font_id.family == egui::FontFamily::Name(BOLD.into())).map(|section| &job.text[section.byte_range.clone()]).collect();
        assert_eq!(bold, ["[M]", "[Ctrl+C]"]);
        assert_eq!(job.text, "[M] Move, then [Ctrl+C] Capture", "nothing is lost");
    }

    #[test]
    fn a_build_menu_shows_one_tech_level_remembering_the_last_one_chosen() {
        let db = BlueprintDb::standard();
        let options: Vec<BlueprintId> = ["power_generator", "mass_extractor", "shield_generator", "strategic_missile_launcher"].iter().map(|key| db.id(key)).collect();
        assert_eq!(shown_tier(None, &options, &db), Some(3), "the highest level at first");
        assert_eq!(shown_tier(Some(1), &options, &db), Some(1));
        let t1_only = &options[..2];
        assert_eq!(shown_tier(Some(3), t1_only, &db), Some(1), "a level with nothing in it falls back");
        let up_to_t2 = &options[..3];
        assert_eq!(shown_tier(None, up_to_t2, &db), Some(2), "the best the selection can build");
        assert_eq!(shown_tier(Some(1), &[], &db), None, "no items, no menu");
    }

    fn build(db: &BlueprintDb, key: &str, cell_x: u32, site: Option<SimId>) -> BuildTask {
        BuildTask::Build { blueprint: db.id(key), cell_x, cell_y: 5, site }
    }

    #[test]
    fn a_builders_orders_are_listed_in_order_with_how_far_a_started_site_has_got() {
        let db = BlueprintDb::standard();
        let mut site = unit(20, 0.0);
        site.built_permille = 450;
        let mut current = commander_and_engineer(&db);
        current.units.push(site);
        let tasks = vec![build(&db, "power_generator", 1, Some(SimId(20))), build(&db, "mass_extractor", 2, None), BuildTask::Reclaim { target: SimId(30) }];
        current.build_queues = vec![BuildQueueView { id: SimId(10), tasks: tasks.clone() }];
        let chips = queue_chips(&current, &[SimId(10)], &db);
        let labels: Vec<&str> = chips.iter().map(|chip| chip.label.as_str()).collect();
        assert_eq!(labels, ["1. Power Generator 45%", "2. Materiel Extractor", "3. Reclaim"]);
        assert_eq!(chips[1].remove, UiAction::CancelTask { units: vec![SimId(10)], task: tasks[1] });
        assert!(queue_chips(&current, &[SimId(11)], &db).is_empty(), "the engineer has no orders");
    }

    #[test]
    fn an_order_shared_by_several_builders_appears_once_and_is_removed_from_them_all() {
        let db = BlueprintDb::standard();
        let mut current = commander_and_engineer(&db);
        // One has started the site, and the other has not reached it yet.
        current.build_queues = vec![
            BuildQueueView { id: SimId(10), tasks: vec![build(&db, "power_generator", 1, Some(SimId(20)))] },
            BuildQueueView { id: SimId(11), tasks: vec![build(&db, "mass_extractor", 2, None), build(&db, "power_generator", 1, None)] },
        ];
        let chips = queue_chips(&current, &[SimId(10), SimId(11)], &db);
        assert_eq!(chips.len(), 2);
        let UiAction::CancelTask { units, .. } = &chips[0].remove else {
            panic!("a builder's order should cancel a task");
        };
        assert_eq!(units, &[SimId(10), SimId(11)]);
        let UiAction::CancelTask { units, .. } = &chips[1].remove else {
            panic!("a builder's order should cancel a task");
        };
        assert_eq!(units, &[SimId(11)]);
    }

    #[test]
    fn a_factorys_items_are_grouped_and_only_reach_factories_holding_them() {
        let db = BlueprintDb::standard();
        let (bot, tank) = (db.id("t1_light_walker"), db.id("t2_heavy_tank"));
        let mut current = mixed_factories(&db);
        current.factories[0] = factory_view(1, vec![bot.0, bot.0], false, 100);
        current.factories[1] = factory_view(2, vec![tank.0, bot.0], false, 100);
        let chips = queue_chips(&current, &[SimId(1), SimId(2)], &db);
        assert_eq!(chips.len(), 2);
        assert_eq!(chips[0].label, "Light Walker x3");
        // One at a time: off the factory with the most, onto the one with the shorter queue.
        assert_eq!(chips[0].remove, UiAction::RemoveQueued { factories: vec![SimId(1)], blueprint: bot });
        assert_eq!(chips[0].add, Some(UiAction::Queue { factories: vec![SimId(1)], blueprint: bot, selected_count: 1, count: 1 }));
        assert_eq!(chips[1].label, "Heavy Tank");
        assert_eq!(chips[1].remove, UiAction::RemoveQueued { factories: vec![SimId(2)], blueprint: tank });
        assert_eq!(chips[1].add, Some(UiAction::Queue { factories: vec![SimId(2)], blueprint: tank, selected_count: 1, count: 1 }));
    }

    /// Runs real egui input through the queue row, and returns what came out.
    fn click_first_chip(button: egui::PointerButton) -> (Vec<UiAction>, bool) {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        let db = BlueprintDb::standard();
        let chips = [QueueChip { label: "1. Materiel Extractor".to_string(), icon: ChipIcon::Item(db.id("mass_extractor")), count: 1, progress: None, remove: UiAction::Restart, add: Some(UiAction::TogglePause) }];
        // On the first chip, which starts the row.
        let at = egui::pos2(24.0, 24.0);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let input = |events: Vec<egui::Event>| egui::RawInput { screen_rect: Some(screen), events, ..egui::RawInput::default() };
        let press = |pressed: bool| egui::Event::PointerButton { pos: at, button, pressed, modifiers: egui::Modifiers::NONE };
        let mut actions = Vec::new();
        let mut run = |events: Vec<egui::Event>| {
            let _ = ctx.run(input(events), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| queue_chip_row(ui, &chips, &db, &mut actions));
            });
        };
        run(vec![]);
        run(vec![egui::Event::PointerMoved(at)]);
        run(vec![]);
        let over_panel = ctx.wants_pointer_input();
        run(vec![press(true)]);
        run(vec![press(false)]);
        run(vec![]);
        (actions, over_panel)
    }

    #[test]
    fn the_selection_panel_shows_net_rates_and_build_power() {
        let db = BlueprintDb::standard();
        let mut current = commander_and_engineer(&db);
        current.rates = vec![
            UnitRatesView { id: SimId(10), mass_income: 10, energy_income: 200, mass_use: 0, energy_use: 0 },
            UnitRatesView { id: SimId(11), mass_income: 0, energy_income: 0, mass_use: 151, energy_use: 30 },
        ];
        assert_eq!(selection_net(&current, &[SimId(10), SimId(11)]), Some((-141, 170)));
        assert_eq!(selection_net(&current, &[SimId(12)]), None, "the bot reports no rates");
        let power = |key: &str| db.get(db.id(key)).build_power.to_num::<i64>();
        assert_eq!(selection_build_power(&current, &[SimId(10), SimId(11), SimId(12)], &db), power("command_rig") + power("t1_engineer"));
        assert_eq!(signed_tenths(-141), "-14.1");
        assert_eq!(signed_tenths(25), "+2.5");
        assert_eq!(signed_tenths(0), "+0.0");
    }

    #[test]
    fn a_teams_score_counts_what_can_be_seen_and_hides_an_unknown_income() {
        let db = BlueprintDb::standard();
        let current = base_with_factories(&db);
        let [units, income, army] = team_figures(&current, Team::Blue, &db, Some(26));
        assert_eq!((units.as_str(), income.as_str()), ("6", "+26"));
        assert_eq!(army, db.get(db.id("command_rig")).mass.to_num::<i64>().to_string(), "only mobile units count towards the army");
        assert_eq!(team_figures(&current, Team::Red, &db, None)[1], "?");
    }

    #[test]
    fn walks_get_arrival_times_that_add_up_and_stop_at_other_work() {
        let db = BlueprintDb::standard();
        let mut bot = unit(1, 0.0);
        bot.blueprint = db.id("t1_light_walker").0;
        let plan = UnitPlanView {
            id: SimId(1),
            steps: vec![step(PlanStepKind::Move, 100.0, 0.0), step(PlanStepKind::AttackMove, 300.0, 0.0), step(PlanStepKind::Build, 400.0, 20.0), step(PlanStepKind::Move, 500.0, 0.0)],
        };
        let current = snapshot(vec![bot], vec![plan]);
        let mut legs = build_trails(&current, &current, 1.0, &[SimId(1)]);
        estimate_arrivals(&mut legs, &current, &db);
        let speed = db.get(db.id("t1_light_walker")).speed.to_num::<f64>() * 10.0;
        let etas: Vec<Option<f64>> = legs.iter().map(|leg| leg.eta).collect();
        assert!((etas[0].unwrap() - 100.0 / speed).abs() < 1e-9);
        assert!((etas[1].unwrap() - 300.0 / speed).abs() < 1e-9, "times add up along the way");
        assert_eq!(etas[2..], [None, None], "how long building takes is not known, so the clock stops");
        assert_eq!(eta_text(24.4), "ETA 0:24");
        assert_eq!(eta_text(75.0), "ETA 1:15");
    }

    #[test]
    fn the_command_bar_offers_build_selection_and_enhance_tabs_as_they_apply() {
        assert_eq!(command_tabs(true, true), [CommandTab::Build, CommandTab::Selection, CommandTab::Enhance]);
        assert_eq!(command_tabs(false, false), [CommandTab::Selection]);
        let tabs = command_tabs(true, false);
        assert_eq!(shown_tab(None, &tabs), CommandTab::Build, "building first, when the selection can build");
        assert_eq!(shown_tab(Some(CommandTab::Selection), &tabs), CommandTab::Selection);
        assert_eq!(shown_tab(Some(CommandTab::Enhance), &tabs), CommandTab::Build, "a tab this selection lacks falls back to its first");
    }

    #[test]
    fn the_surface_toggle_brings_subs_up_and_takes_them_down_once_all_are_up() {
        let db = BlueprintDb::standard();
        let mut current = commander_and_engineer(&db);
        let mut sub = unit(20, 0.0);
        sub.blueprint = db.id("t1_attack_sub").0;
        let mut other = unit(21, 0.0);
        other.blueprint = db.id("t1_attack_sub").0;
        current.units.extend([sub, other]);
        assert_eq!(surface_action(&current, &[SimId(10)], &db), None, "only for submarines");
        assert_eq!(surface_action(&current, &[SimId(20), SimId(21), SimId(10)], &db), Some(UiAction::SetSurfaced { units: vec![SimId(20), SimId(21)], surfaced: true }));
        current.surfaced = vec![SimId(20)];
        assert_eq!(surface_action(&current, &[SimId(20), SimId(21)], &db), Some(UiAction::SetSurfaced { units: vec![SimId(20), SimId(21)], surfaced: true }), "not all up yet");
        current.surfaced = vec![SimId(20), SimId(21)];
        assert_eq!(surface_action(&current, &[SimId(20), SimId(21)], &db), Some(UiAction::SetSurfaced { units: vec![SimId(20), SimId(21)], surfaced: false }));
    }

    #[test]
    fn the_clock_reads_hours_minutes_and_seconds() {
        assert_eq!(clock_text(0), "00:00:00");
        assert_eq!(clock_text(3_090), "00:05:09");
        assert_eq!(clock_text(37_250), "01:02:05");
    }

    #[test]
    fn the_selection_is_grouped_by_kind_in_the_order_first_selected() {
        let db = BlueprintDb::standard();
        let mut current = commander_and_engineer(&db);
        let mut second_bot = unit(13, 0.0);
        second_bot.blueprint = db.id("t1_light_walker").0;
        current.units.push(second_bot);
        let groups = selection_groups(&current, &[SimId(12), SimId(10), SimId(13), SimId(99)]);
        let bot = db.id("t1_light_walker").0;
        assert_eq!(groups, [(bot, vec![SimId(12), SimId(13)]), (db.id("command_rig").0, vec![SimId(10)])], "unknown units are left out");
    }

    #[test]
    fn vision_is_washed_pale_more_strongly_zoomed_out() {
        use crate::render::pipeline::strategic_share;
        assert_eq!(strategic_share(0.0), 0.35, "up close, a third as strong, so the ground keeps most of its color");
        assert_eq!(strategic_share(0.2), 0.35);
        assert_eq!(strategic_share(1.0), 1.0);
        let steps: Vec<f32> = (0..=20).map(|step| strategic_share(f64::from(step) / 20.0)).collect();
        assert!(steps.windows(2).all(|pair| pair[0] <= pair[1]), "it only grows as the camera pulls out");
    }

    /// Clicks the first order button through real egui input.
    #[test]
    fn clicking_an_order_button_arms_it_and_the_armed_order_stays_lit() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        let offers = OrderOffers { discharge: false, weapons: true, teleport: false, capture: false, unload: false };
        let at = egui::pos2(8.0 + ORDER_BUTTON / 2.0, 8.0 + ORDER_BUTTON / 2.0);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let mut actions = Vec::new();
        for events in [vec![], vec![egui::Event::PointerMoved(at)], vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE }], vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE }]] {
            let input = egui::RawInput { screen_rect: Some(screen), events, ..egui::RawInput::default() };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| ui.horizontal(|ui| unit_order_buttons(ui, Some(ArmedOrder::Patrol), &offers, &mut actions)));
            });
        }
        assert_eq!(actions, [UiAction::Arm(ArmedOrder::Move)]);
    }

    #[test]
    fn right_clicking_a_queue_item_removes_one_and_left_clicking_adds_one() {
        let (actions, over_panel) = click_first_chip(egui::PointerButton::Secondary);
        assert_eq!(actions, [UiAction::Restart], "a right-click should remove one");
        assert!(over_panel, "the interface must claim the click, or it would also order units to move");
        assert_eq!(click_first_chip(egui::PointerButton::Primary).0, [UiAction::TogglePause], "a left-click should queue one more");
    }

    /// Whether a button's popup shows once the pointer has rested on it for `rest` seconds.
    fn popup_shows_after(rest: f64, styled: bool) -> bool {
        let ctx = egui::Context::default();
        if styled {
            install_style(&ctx);
        }
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let mut shown = false;
        // Frames at 60 per second, from ten seconds in, since egui counts a popup as recently
        // shown at the very start. The pointer sweeps in from the left for six frames, ending on
        // the button, then rests there.
        const SWEEP: u32 = 6;
        let frames = SWEEP + (rest * 60.0).round() as u32;
        for frame in 0..=frames {
            let events = if frame <= SWEEP { vec![egui::Event::PointerMoved(egui::pos2(30.0 - 8.0 * (SWEEP - frame) as f32, 20.0))] } else { vec![] };
            let input = egui::RawInput { screen_rect: Some(screen), events, time: Some(10.0 + f64::from(frame) / 60.0), ..egui::RawInput::default() };
            shown = false;
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let _ = ui.add_sized([46.0, 46.0], egui::Button::new("B")).on_hover_ui(|ui| {
                        shown = true;
                        ui.label("popup");
                    });
                });
            });
        }
        shown
    }

    #[test]
    fn a_build_button_popup_shows_after_a_short_rest_on_it() {
        assert!(!popup_shows_after(0.0, true), "a popup should not flash up as the pointer sweeps past");
        assert!(popup_shows_after(0.15, true), "the popup should show within a sixth of a second");
        assert!(!popup_shows_after(0.15, false), "egui's own delay is longer, so this test would not catch a missing style");
    }

    #[test]
    fn ranges_say_what_each_weapon_shoots_at_and_include_sensors() {
        let db = BlueprintDb::standard();
        let names = |key: &str| placement_ranges(db.get(db.id(key))).iter().map(|range| range.0.name()).collect::<Vec<_>>();
        assert!(names("t1_interceptor").contains(&"Anti-air"));
        assert!(names("t2_fighter_bomber").contains(&"Radar"), "the fighter-bomber's radar shows");
        assert!(names("t1_attack_sub").contains(&"Torpedoes"));
        assert!(names("t2_heavy_tank").contains(&"Ground fire"));
        assert!(names("radar").contains(&"Radar"));
        assert!(names("stealth_field_generator").contains(&"Stealth field"));
        assert!(names("command_rig").contains(&"Deep scan"));
    }

    /// Every unit's every weapon rings as what it shoots at: ground fire, anti-air, or both for a
    /// gun that fires at either, and torpedoes; and every sense and field it has rings too.
    #[test]
    fn every_unit_rings_each_weapon_by_what_it_shoots_at_and_each_sense() {
        use crate::sim::blueprints::Targets;
        let db = BlueprintDb::standard();
        for (index, unit) in db.iter().enumerate() {
            let ranges = placement_ranges(unit);
            let has = |kind: RangeKind, reach: Fp| ranges.iter().any(|&(shown, radius)| shown == kind && (radius - reach.to_num::<f64>()).abs() < 0.5);
            for gun in unit.weapon.iter().chain(&unit.extra_weapons).filter(|gun| gun.range > Fp::ZERO) {
                let kinds: &[RangeKind] = match gun.targets {
                    Targets::Ground => &[RangeKind::GroundFire],
                    Targets::Air => &[RangeKind::AntiAir],
                    Targets::Both => &[RangeKind::GroundFire, RangeKind::AntiAir],
                    Targets::Naval => &[RangeKind::Torpedoes],
                };
                for &kind in kinds {
                    assert!(has(kind, gun.range), "{} (#{index}): no {} ring at {}", unit.key, kind.name(), gun.range);
                }
            }
            for (kind, reach) in [(RangeKind::Radar, unit.radar), (RangeKind::Sonar, unit.sonar), (RangeKind::DeepScan, unit.deep_scan), (RangeKind::Stealth, unit.stealth_field)] {
                assert_eq!(has(kind, reach), reach > Fp::ZERO, "{}: {} ring", unit.key, kind.name());
            }
            let anti_air = unit.weapon.iter().chain(&unit.extra_weapons).any(|gun| gun.range > Fp::ZERO && matches!(gun.targets, Targets::Air | Targets::Both));
            assert_eq!(ranges.iter().any(|range| range.0 == RangeKind::AntiAir), anti_air, "{}: an anti-air ring only for what can fire at aircraft", unit.key);
        }
    }

    #[test]
    fn only_radar_and_sonar_are_outlined_for_all_units_to_begin_with() {
        let toggles = RangeToggles::default();
        let shown: Vec<_> = RangeKind::ALL.into_iter().filter(|&kind| toggles.shown(kind)).collect();
        assert_eq!(shown, [RangeKind::Radar, RangeKind::Sonar]);
        let colors: std::collections::BTreeSet<_> = RangeKind::ALL.iter().map(|kind| kind.color().to_array()).collect();
        assert_eq!(colors.len(), RangeKind::ALL.len(), "each kind has its own color");
    }

    /// The overlay rings the viewer's finished units for each kind toggled on, and not the enemy's
    /// or what is still being built; radar comes from what the viewer's sensors really reach.
    #[test]
    fn the_overlay_rings_the_viewers_finished_units_for_the_kinds_toggled_on() {
        let db = BlueprintDb::standard();
        let placed = |id: u32, key: &str, x: f64, team: Team, built: u16| {
            let mut view = unit(id, x);
            view.blueprint = db.id(key).0;
            view.team = team;
            view.built_permille = built;
            view
        };
        let mut current = snapshot(
            vec![
                placed(1, "t1_aa_turret", 100.0, Team::Blue, 1000),
                placed(2, "t1_aa_turret", 300.0, Team::Red, 1000),
                placed(3, "t1_aa_turret", 500.0, Team::Blue, 400),
                placed(4, "t2_heavy_tank", 700.0, Team::Blue, 1000),
            ],
            Vec::new(),
        );
        current.sensor_ranges = vec![crate::sim::world::SensorRangeView { center: [raw(900.0), 0], radar: raw(1_234.0), deep_scan: 0 }];
        let kinds = |toggles: &RangeToggles, team: Option<Team>| overlay_ranges(&current, team, toggles, &db);
        let mut toggles = RangeToggles::default();
        let radar = kinds(&toggles, Some(Team::Blue));
        assert_eq!(radar.iter().map(|kind| kind.0).collect::<Vec<_>>(), [RangeKind::Radar], "the radar shows, from the sensors seen");
        assert_eq!(radar[0].1, [([900.0, 0.0], 1_234.0)]);
        toggles.set(RangeKind::AntiAir, true);
        toggles.set(RangeKind::Radar, false);
        let anti_air = kinds(&toggles, Some(Team::Blue));
        let centers: Vec<f64> = anti_air.iter().find(|kind| kind.0 == RangeKind::AntiAir).unwrap().1.iter().map(|circle| circle.0[0]).collect();
        assert_eq!(centers, [100.0], "only the viewer's finished anti-air");
        let everyone = kinds(&toggles, None);
        assert_eq!(everyone.iter().find(|kind| kind.0 == RangeKind::AntiAir).unwrap().1.len(), 2, "with no viewer, both teams'");
        assert!(kinds(&RangeToggles([false; RangeKind::ALL.len()]), Some(Team::Blue)).is_empty());
    }

    /// A gun that fires at both the ground and aircraft rings as both, and a selection rings every
    /// kind its units reach.
    #[test]
    fn a_gun_for_ground_and_air_rings_as_both_and_a_selection_rings_every_kind() {
        let db = BlueprintDb::standard();
        let mut both = db.get(db.id("t2_heavy_tank")).clone();
        both.weapon.as_mut().unwrap().targets = crate::sim::blueprints::Targets::Both;
        let kinds: Vec<RangeKind> = placement_ranges(&both).into_iter().map(|range| range.0).collect();
        assert!(kinds.contains(&RangeKind::GroundFire) && kinds.contains(&RangeKind::AntiAir), "{kinds:?}");
        let mut tank = unit(1, 50.0);
        tank.blueprint = db.id("t2_heavy_tank").0;
        let mut turret = unit(2, 90.0);
        turret.blueprint = db.id("t1_aa_turret").0;
        let rings = selection_ranges(&snapshot(vec![tank, turret], Vec::new()), &[SimId(1), SimId(2)], &db);
        assert!(rings.iter().any(|kind| kind.0 == RangeKind::GroundFire) && rings.iter().any(|kind| kind.0 == RangeKind::AntiAir), "{rings:?}");
    }

    #[test]
    fn the_watch_keys_step_through_views_and_speeds_within_bounds() {
        assert_eq!(next_watch_view(None), Some(Team::Blue));
        assert_eq!(next_watch_view(Some(Team::Blue)), Some(Team::Red));
        assert_eq!(next_watch_view(Some(Team::Red)), None);
        assert_eq!(step_watch_speed(1, true), 2);
        assert_eq!(step_watch_speed(16, true), 16);
        assert_eq!(step_watch_speed(1, false), 1);
        assert_eq!(step_watch_speed(8, false), 4);
        assert_eq!(*WATCH_SPEEDS.last().expect("speeds"), crate::driver::MAX_SPEED);
    }

    fn base_with_factories(db: &BlueprintDb) -> SimSnapshot {
        let raw = |value: f64| (value * 4096.0) as i32;
        let make = |id: u32, key: &str, x: f64, built: u16, team: Team| {
            let mut view = unit(id, x);
            view.x = raw(x);
            view.blueprint = db.id(key).0;
            view.is_structure = key != "command_rig";
            view.built_permille = built;
            view.team = team;
            view
        };
        snapshot(
            vec![
                make(1, "command_rig", 0.0, 1000, Team::Blue),
                make(2, "land_factory_t1", 100.0, 1000, Team::Blue),
                make(3, "land_factory_t1", 900.0, 400, Team::Blue),
                make(4, "land_factory_t2", 500.0, 1000, Team::Blue),
                make(5, "land_factory_t2", 700.0, 1000, Team::Blue),
                make(6, "power_generator", 50.0, 1000, Team::Blue),
                make(7, "land_factory_t1", 300.0, 1000, Team::Red),
                make(8, "command_rig", 300.0, 1000, Team::Red),
            ],
            vec![],
        )
    }

    #[test]
    fn the_bar_lists_your_commander_and_factories_by_kind_and_tier() {
        let db = BlueprintDb::standard();
        let groups = quick_select_groups(&base_with_factories(&db), Team::Blue, &db);
        assert_eq!(groups.commander, Some(SimId(1)));
        assert_eq!(groups.families.len(), 1);
        assert_eq!(groups.families[0].name, "Land Factory");
        let tiers = &groups.families[0].tiers;
        assert_eq!(tiers.len(), 2);
        assert_eq!((tiers[0].label.as_str(), tiers[0].units.as_slice()), ("T1  x2", [SimId(2), SimId(3)].as_slice()), "an unfinished one counts");
        assert_eq!((tiers[1].label.as_str(), tiers[1].units.as_slice()), ("T2  x2", [SimId(4), SimId(5)].as_slice()));
    }

    #[test]
    fn empty_tiers_and_kinds_are_left_out() {
        let db = BlueprintDb::standard();
        let mut current = base_with_factories(&db);
        current.units.retain(|unit| unit.id != SimId(4) && unit.id != SimId(5));
        let groups = quick_select_groups(&current, Team::Blue, &db);
        assert_eq!(groups.families[0].tiers.len(), 1, "no T2 factories");
        current.units.retain(|unit| unit.team == Team::Blue && !db.get(BlueprintId(unit.blueprint)).is_factory());
        assert!(quick_select_groups(&current, Team::Blue, &db).families.is_empty());
        assert_eq!(quick_select_groups(&snapshot(vec![], vec![]), Team::Blue, &db), QuickSelect::default());
    }

    #[test]
    fn kinds_of_factory_get_readable_names() {
        assert_eq!(factory_family_name("land_factory_t1"), "Land Factory");
        assert_eq!(factory_family_name("land_factory_t2"), "Land Factory");
        assert_eq!(factory_family_name("air_factory_t3"), "Air Factory");
        assert_eq!(factory_family_name("shipyard"), "Shipyard");
    }

    #[test]
    fn closest_means_closest_to_the_view_and_a_tie_goes_to_the_lowest_id() {
        let db = BlueprintDb::standard();
        let current = base_with_factories(&db);
        let t2 = [SimId(4), SimId(5)];
        assert_eq!(closest_unit(&current, &t2, [520.0, 0.0]), Some(SimId(4)));
        assert_eq!(closest_unit(&current, &t2, [680.0, 0.0]), Some(SimId(5)));
        assert_eq!(closest_unit(&current, &t2, [600.0, 0.0]), Some(SimId(4)));
        assert_eq!(closest_unit(&current, &[SimId(99)], [0.0, 0.0]), None);
    }

    /// Clicks on the bar's rows through real egui input.
    struct BarTest {
        ctx: egui::Context,
        current: SimSnapshot,
        groups: QuickSelect,
        state: QuickSelectState,
        time: f64,
    }

    impl BarTest {
        fn new() -> Self {
            let db = BlueprintDb::standard();
            let current = base_with_factories(&db);
            let groups = quick_select_groups(&current, Team::Blue, &db);
            let mut test = Self { ctx: egui::Context::default(), current, groups, state: QuickSelectState::default(), time: 10.0 };
            install_fonts(&test.ctx);
            test.frame(vec![], egui::Modifiers::NONE);
            test
        }

        fn frame(&mut self, events: Vec<egui::Event>, modifiers: egui::Modifiers) -> Vec<UiAction> {
            self.time += 0.02;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
                time: Some(self.time),
                modifiers,
                events,
                ..egui::RawInput::default()
            };
            let mut actions = Vec::new();
            let (current, groups, state) = (&self.current, &self.groups, &mut self.state);
            let _ = self.ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    quick_rows(ui, current, &BlueprintDb::standard(), groups, BLUE, &[], [680.0, 0.0], state, &mut actions);
                });
            });
            actions
        }

        /// Row 0 is the commander, row 1 the factory kind, and rows 2 and 3 its tiers: two big squares,
        /// then two short buttons, each 4 points apart, inside the panel's 8 point margin.
        fn click(&mut self, row: usize, modifiers: egui::Modifiers) -> Vec<UiAction> {
            let middles = [32.0, 84.0, 124.0, 152.0];
            self.click_at(egui::pos2(32.0, middles[row]), modifiers)
        }

        fn click_at(&mut self, at: Pos2, modifiers: egui::Modifiers) -> Vec<UiAction> {
            let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers };
            let mut actions = self.frame(vec![egui::Event::PointerMoved(at)], modifiers);
            actions.extend(self.frame(vec![press(true)], modifiers));
            actions.extend(self.frame(vec![press(false)], modifiers));
            actions.extend(self.frame(vec![], modifiers));
            actions
        }
    }

    #[test]
    fn clicking_the_commander_selects_it_and_shift_adds_it() {
        let mut bar = BarTest::new();
        assert_eq!(bar.click(0, egui::Modifiers::NONE), [UiAction::QuickSelect { units: vec![SimId(1)], add: false }]);
        bar.time += 1.0;
        assert_eq!(bar.click(0, egui::Modifiers::SHIFT), [UiAction::QuickSelect { units: vec![SimId(1)], add: true }]);
    }

    #[test]
    fn the_idle_tag_selects_the_idle_factories_and_the_rest_of_the_square_opens_the_kind() {
        // The kind's square runs from 8 to 56 across and 60 to 108 down; the tag is in its top
        // right corner.
        let corner = egui::pos2(48.0, 66.0);
        let mut bar = BarTest::new();
        assert!(bar.click_at(corner, egui::Modifiers::NONE).is_empty(), "with none idle there is no tag, so the corner opens the kind");
        assert!(bar.state.expanded.contains("Land Factory"));

        let mut bar = BarTest::new();
        bar.current.idle_factories = vec![SimId(2), SimId(5)];
        assert_eq!(bar.click_at(corner, egui::Modifiers::NONE), [UiAction::QuickSelect { units: vec![SimId(2), SimId(5)], add: false }]);
        assert!(!bar.state.expanded.contains("Land Factory"), "the tag selects without opening the kind");
        bar.time += 1.0;
        assert_eq!(bar.click_at(corner, egui::Modifiers::SHIFT), [UiAction::QuickSelect { units: vec![SimId(2), SimId(5)], add: true }]);
        bar.time += 1.0;
        assert!(bar.click(1, egui::Modifiers::NONE).is_empty(), "the rest of the square still opens it");
        assert!(bar.state.expanded.contains("Land Factory"));
    }

    #[test]
    fn spending_is_listed_by_kind_the_most_first() {
        let db = BlueprintDb::standard();
        let mut current = base_with_factories(&db);
        let spends = |id: u32, mass: i32, energy: i32| UnitSpendingView { id: SimId(id), mass, energy };
        // The commander, two T2 factories, the generator, and one unit the viewer cannot see.
        current.spending = vec![spends(1, 50, 0), spends(4, 40, 200), spends(5, 45, 210), spends(6, 0, 30), spends(99, 500, 500)];
        let listed = |energy: bool| spending_groups(&current, &db, energy).into_iter().map(|group| (group.name, group.units, group.tenths)).collect::<Vec<_>>();
        let t2 = format!("{} \u{d7}2", db.get(db.id("land_factory_t2")).name);
        let name = |key: &str| db.get(db.id(key)).name.clone();
        assert_eq!(listed(false), [(t2.clone(), vec![SimId(4), SimId(5)], 85), (name("command_rig"), vec![SimId(1)], 50)]);
        assert_eq!(listed(true), [(t2, vec![SimId(4), SimId(5)], 410), (name("power_generator"), vec![SimId(6)], 30)], "the commander spends no energy, so it is not listed");
    }

    #[test]
    fn breakdown_amounts_are_whole_unless_small() {
        for (tenths, shown) in [(0, "0.0"), (5, "0.5"), (99, "9.9"), (100, "10"), (104, "10"), (105, "11"), (1500, "150")] {
            assert_eq!(breakdown_amount(tenths), shown, "{tenths} tenths");
        }
    }

    #[test]
    fn the_breakdown_follows_the_row_hovered_and_stays_while_the_pointer_is_on_it() {
        let (mass, energy) = (Some(false), Some(true));
        // What is open, the row hovered, whether the pointer is on the panel, and on the breakdown.
        for (open, row, on_panel, on_breakdown, shown) in [
            (None, None, false, false, None),
            (None, mass, true, false, mass),
            (mass, energy, true, false, energy),
            (energy, None, true, false, energy),
            (energy, None, false, true, energy),
            (energy, None, false, false, None),
            (None, None, true, false, None),
        ] {
            assert_eq!(breakdown_shown(open, row, on_panel, on_breakdown), shown, "{open:?} open, {row:?} hovered, on the panel {on_panel}, on the breakdown {on_breakdown}");
        }
    }

    #[test]
    fn clicking_a_kind_only_opens_and_closes_it() {
        let mut bar = BarTest::new();
        assert!(bar.click(2, egui::Modifiers::NONE).is_empty(), "closed, so there is no tier row there");
        assert!(bar.click(1, egui::Modifiers::NONE).is_empty(), "opening must not change the selection");
        assert!(bar.state.expanded.contains("Land Factory"));
        bar.time += 1.0;
        assert!(bar.click(1, egui::Modifiers::NONE).is_empty());
        assert!(!bar.state.expanded.contains("Land Factory"));
    }

    #[test]
    fn clicking_a_tier_selects_the_closest_and_double_clicking_selects_all() {
        let mut bar = BarTest::new();
        bar.click(1, egui::Modifiers::NONE);
        bar.time += 1.0;
        let closest = || vec![UiAction::QuickSelect { units: vec![SimId(5)], add: false }];
        assert_eq!(bar.click(3, egui::Modifiers::NONE), closest(), "the T2 factory nearest the view");
        assert_eq!(bar.click(3, egui::Modifiers::NONE), [UiAction::QuickSelect { units: vec![SimId(4), SimId(5)], add: false }], "a second click soon after");

        // Too slow to be a double-click.
        bar.time += 1.0;
        assert_eq!(bar.click(3, egui::Modifiers::NONE), closest());
        bar.time += 1.0;
        assert_eq!(bar.click(3, egui::Modifiers::NONE), closest());
    }

    #[test]
    fn shift_adds_from_a_tier_and_clicks_on_different_tiers_are_not_a_double_click() {
        let mut bar = BarTest::new();
        bar.click(1, egui::Modifiers::NONE);
        bar.time += 1.0;
        assert_eq!(bar.click(3, egui::Modifiers::SHIFT), [UiAction::QuickSelect { units: vec![SimId(5)], add: true }]);
        assert_eq!(bar.click(3, egui::Modifiers::SHIFT), [UiAction::QuickSelect { units: vec![SimId(4), SimId(5)], add: true }]);
        bar.time += 1.0;
        assert_eq!(bar.click(2, egui::Modifiers::NONE), [UiAction::QuickSelect { units: vec![SimId(3)], add: false }]);
        assert_eq!(bar.click(3, egui::Modifiers::NONE), [UiAction::QuickSelect { units: vec![SimId(5)], add: false }]);
    }

    /// Reproduces the squeezed popup: an area with little room, as near the middle of the screen.
    #[test]
    fn popup_text_stays_on_one_line_however_little_room_it_has() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        let input = || egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0))), ..egui::RawInput::default() };
        let id = egui::Id::new("test popup");
        for _ in 0..3 {
            let _ = ctx.run(input(), |ctx| {
                egui::Area::new(id).anchor(Align2::CENTER_TOP, Vec2::new(0.0, 80.0)).show(ctx, |ui| {
                    ui.set_max_width(40.0);
                    egui::Frame::popup(ui.style()).show(ui, |ui| one_line_label(ui, "Light Artillery queued"));
                });
            });
        }
        let rect = ctx.memory(|memory| memory.area_rect(id)).expect("the popup was shown");
        assert!(rect.width() > 100.0 && rect.height() < 50.0, "the popup is {} by {}", rect.width(), rect.height());
    }

    fn walk(route: u32) -> BuildTask {
        let order = crate::sim::components::Order { kind: OrderKind::Move, route, slot: None, slot_ticks: 0, direct: false };
        BuildTask::Walk { order, started: false }
    }

    fn stop(unit: u32, task: BuildTask, x: f64) -> OrderStopView {
        OrderStopView { unit: SimId(unit), task, position: [(x * 4096.0) as i32, 0, 0] }
    }

    #[test]
    fn a_group_order_gets_one_handle_in_the_middle_of_its_spots() {
        let db = BlueprintDb::standard();
        let build = BuildTask::Build { blueprint: db.id("power_generator"), cell_x: 5, cell_y: 5, site: None };
        let mut current = snapshot(vec![], vec![]);
        current.order_stops = vec![stop(1, walk(7), 100.0), stop(2, walk(7), 200.0), stop(3, walk(8), 500.0), stop(4, build, 900.0), stop(5, build, 900.0)];
        let handles = order_handles(&current);
        assert_eq!(handles.len(), 3);
        assert_eq!(handles[0].position[0], 150.0);
        assert_eq!(handles[0].kind, MarkerKind::Move);
        assert_eq!(handles[0].order, DraggableOrder::Walk { units: vec![SimId(1), SimId(2)], task: walk(7) });
        assert_eq!(handles[1].order, DraggableOrder::Walk { units: vec![SimId(3)], task: walk(8) });
        assert_eq!(handles[2].kind, MarkerKind::Build);
        assert_eq!(handles[2].order, DraggableOrder::Build { units: vec![SimId(4), SimId(5)], task: build });
    }

    #[test]
    fn factories_sharing_a_rally_point_share_a_handle() {
        let mut current = snapshot(vec![], vec![]);
        let rally = |id: u32, x: i32| FactoryView { id: SimId(id), queue: vec![], repeat: false, progress_permille: 0, rally: Some([x, 0, 0]) };
        current.factories = vec![rally(1, 4096), rally(2, 4096), rally(3, 8192), factory_view(4, vec![], false, 0)];
        let handles = order_handles(&current);
        assert_eq!(handles.len(), 2, "the one with no rally point has no handle");
        assert_eq!(handles[0].order, DraggableOrder::Rally { factories: vec![SimId(1), SimId(2)] });
        assert_eq!(handles[1].order, DraggableOrder::Rally { factories: vec![SimId(3)] });
    }

    #[test]
    fn handles_are_easy_to_grab_but_only_near_the_icon() {
        let mut camera = Camera::new(10_240.0, [1600.0, 1000.0]);
        camera.look_at(2_000.0, 2_000.0, 300.0);
        let near = OrderHandle { position: [2_000.0, 2_000.0, 0.0], kind: MarkerKind::Move, order: DraggableOrder::Rally { factories: vec![] } };
        let beside = OrderHandle { position: [2_100.0, 2_000.0, 0.0], ..near.clone() };
        let handles = [near.clone(), beside];
        let at = camera.world_to_screen(near.position).unwrap();
        assert_eq!(handle_under_cursor(&handles, &camera, at, 1.0), Some(0));
        assert_eq!(handle_under_cursor(&handles, &camera, [at[0] + 14.0, at[1] + 8.0], 1.0), Some(0), "a near miss still grabs it");
        assert_eq!(handle_under_cursor(&handles, &camera, [at[0], at[1] + 40.0], 1.0), None);
        assert_eq!(handle_under_cursor(&handles, &camera, [at[0], at[1] + 30.0], 2.0), Some(0), "the reach follows the screen's scale");
    }

    fn rates(id: u32, mass_income: i32, energy_income: i32, mass_use: i32, energy_use: i32) -> UnitRatesView {
        UnitRatesView { id: SimId(id), mass_income, energy_income, mass_use, energy_use }
    }

    #[test]
    fn the_panel_shows_what_structures_make_and_factories_spend() {
        let db = BlueprintDb::standard();
        let mut current = mixed_factories(&db);
        let mut generator = unit(4, 0.0);
        generator.blueprint = db.id("power_generator").0;
        let mut extractor = unit(5, 0.0);
        extractor.blueprint = db.id("mass_extractor").0;
        current.units.extend([generator, extractor]);
        current.rates = vec![rates(1, 0, 0, 36, 180), rates(2, 0, 0, 0, 0), rates(4, 0, 200, 0, 0), rates(5, 25, 0, 0, 0)];

        let of = |selected: &[SimId]| selection_rates(&current, selected, &db);
        assert_eq!(of(&[SimId(5)]), SelectionRates { output: "+2.5 materiel/s".to_string(), spending: String::new() });
        assert_eq!(of(&[SimId(4)]), SelectionRates { output: "+20.0 energy/s".to_string(), spending: String::new() });
        assert_eq!(of(&[SimId(4), SimId(5)]).output, "+2.5 materiel/s, +20.0 energy/s", "several add up");
        assert_eq!(of(&[SimId(1)]), SelectionRates { output: String::new(), spending: "Using 3.6 materiel/s, 18.0 energy/s".to_string() });
        assert_eq!(of(&[SimId(2)]).spending, "Using nothing");
        assert_eq!(of(&[SimId(99)]), SelectionRates::default());
    }

    #[test]
    fn a_control_group_says_what_is_in_it() {
        let db = BlueprintDb::standard();
        let current = commander_and_engineer(&db);
        assert_eq!(control_group_label(1, &[SimId(11)], &current, &db), "1: Engineer");
        assert_eq!(control_group_label(2, &[SimId(11), SimId(11)], &current, &db), "2: Engineer x2");
        assert_eq!(control_group_label(0, &[SimId(10), SimId(12)], &current, &db), "0: 2 units");
        assert_eq!(control_group_label(3, &[SimId(10)], &current, &db), "3: Commander");
    }

    #[test]
    fn clicking_a_control_group_selects_it_and_shift_adds_it() {
        let db = BlueprintDb::standard();
        let current = commander_and_engineer(&db);
        let groups = vec![(1, vec![SimId(10), SimId(11)]), (0, vec![SimId(12)])];
        let click = |row: usize, modifiers: egui::Modifiers| {
            let ctx = egui::Context::default();
        install_fonts(&ctx);
            // The squares sit side by side, 46 points wide and 4 apart.
            let at = egui::pos2(31.0 + row as f32 * 50.0, 36.0);
            let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers };
            let mut actions = Vec::new();
            for events in [vec![], vec![egui::Event::PointerMoved(at)], vec![press(true)], vec![press(false)], vec![]] {
                let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))), modifiers, events, ..egui::RawInput::default() };
                let _ = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| control_group_rows(ui, &groups, &current, &db, BLUE, &[], &mut actions));
                });
            }
            actions
        };
        assert_eq!(click(0, egui::Modifiers::NONE), [UiAction::QuickSelect { units: vec![SimId(10), SimId(11)], add: false }]);
        assert_eq!(click(1, egui::Modifiers::SHIFT), [UiAction::QuickSelect { units: vec![SimId(12)], add: true }]);
    }

    #[test]
    fn right_clicking_a_missile_builds_them_one_after_another_until_right_clicked_again() {
        let db = BlueprintDb::standard();
        let mut silo = unit(5, 0.0);
        silo.blueprint = db.id("strategic_missile_launcher").0;
        silo.is_structure = true;
        let mut current = snapshot(vec![silo], vec![]);
        current.factories = vec![factory_view(5, vec![], false, 0)];
        let nuke = db.id("strategic_missile");
        let menu = factory_menu(&current, &[SimId(5)], &db).unwrap();
        assert!(!auto_building(&menu, nuke));
        let on = UiAction::Batch(vec![
            UiAction::Queue { factories: vec![SimId(5)], blueprint: nuke, selected_count: 1, count: 1 },
            UiAction::SetRepeat { factories: vec![SimId(5)], repeat: true },
        ]);
        assert_eq!(auto_build_action(&menu, &[SimId(5)], nuke), on);
        // Once it is building them one after another, right-clicking stops that.
        current.factories = vec![factory_view(5, vec![nuke.0], true, 0)];
        let menu = factory_menu(&current, &[SimId(5)], &db).unwrap();
        assert!(auto_building(&menu, nuke));
        assert_eq!(auto_build_action(&menu, &[SimId(5)], nuke), UiAction::SetRepeat { factories: vec![SimId(5)], repeat: false });
    }

    #[test]
    fn shift_queues_five_units_but_only_one_upgrade() {
        let db = BlueprintDb::standard();
        let current = mixed_factories(&db);
        let press = |letter: char, shift: bool| with_shift(hotkey_action(&current, &[SimId(1)], &db, letter, BuildTiers::default(), None).unwrap(), shift, &db);
        let count_of = |action: UiAction| match action {
            UiAction::Queue { count, .. } => count,
            other => panic!("expected a queue order, not {other:?}"),
        };
        assert_eq!(count_of(press('E', false)), 1);
        assert_eq!(count_of(press('E', true)), 5);
        assert_eq!(count_of(press('Q', true)), 5);
        assert_eq!(count_of(press('U', true)), 1, "a factory only upgrades once");
        let other = with_shift(UiAction::Stop, true, &db);
        assert_eq!(other, UiAction::Stop, "other orders are not affected");
    }

    #[test]
    fn shift_clicking_a_factory_button_queues_five() {
        let db = BlueprintDb::standard();
        let current = mixed_factories(&db);
        let menu = factory_menu(&current, &[SimId(1)], &db).unwrap();
        let click = |modifiers: egui::Modifiers| {
            let ctx = egui::Context::default();
        install_fonts(&ctx);
            // Just past the stacked T1, T2, and T3 toggles, on the first item of the bar.
            let at = egui::pos2(62.0, 31.0);
            let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers };
            let mut actions = Vec::new();
            for events in [vec![], vec![egui::Event::PointerMoved(at)], vec![press(true)], vec![press(false)], vec![]] {
                let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 600.0))), modifiers, events, ..egui::RawInput::default() };
                let _ = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| factory_controls(ui, &menu, &db, &mut actions));
                });
            }
            actions
        };
        let engineer = db.id("t1_engineer");
        let queue = |count| vec![UiAction::Queue { factories: vec![SimId(1)], blueprint: engineer, selected_count: 1, count }];
        assert_eq!(click(egui::Modifiers::NONE), queue(1));
        assert_eq!(click(egui::Modifiers::SHIFT), queue(5));
    }

    #[test]
    fn pause_covers_factories_and_sites_and_resumes_when_all_are_paused() {
        let db = BlueprintDb::standard();
        let mut current = mixed_factories(&db);
        let mut engineer = unit(4, 0.0);
        engineer.blueprint = db.id("t1_engineer").0;
        current.units.push(engineer);
        // 1 and 2 are finished factories, 3 is a factory still being built, 4 is an engineer.
        let everything = [SimId(1), SimId(2), SimId(3), SimId(4)];
        assert_eq!(pausable_units(&current, &everything, &db), [SimId(1), SimId(2), SimId(3)], "an engineer cannot be paused");
        let press = |current: &SimSnapshot| hotkey_action(current, &everything, &db, 'O', BuildTiers::default(), None);
        assert_eq!(press(&current), Some(UiAction::SetPaused { units: vec![SimId(1), SimId(2), SimId(3)], paused: true }));
        current.paused = vec![SimId(1)];
        assert_eq!(press(&current), Some(UiAction::SetPaused { units: vec![SimId(1), SimId(2), SimId(3)], paused: true }), "some still run, so pause them all");
        current.paused = vec![SimId(1), SimId(2), SimId(3)];
        assert_eq!(press(&current), Some(UiAction::SetPaused { units: vec![SimId(1), SimId(2), SimId(3)], paused: false }));
        assert_eq!(hotkey_action(&current, &[SimId(4)], &db, 'O', BuildTiers::default(), None), None);
    }

    #[test]
    fn only_a_selection_with_a_commander_can_discharge() {
        let db = BlueprintDb::standard();
        let current = commander_and_engineer(&db);
        assert_eq!(hotkey_action(&current, &[SimId(10)], &db, 'L', BuildTiers::default(), None), Some(UiAction::Arm(ArmedOrder::Discharge)));
        assert_eq!(hotkey_action(&current, &[SimId(10), SimId(12)], &db, 'L', BuildTiers::default(), None), Some(UiAction::Arm(ArmedOrder::Discharge)));
        assert_eq!(hotkey_action(&current, &[SimId(11), SimId(12)], &db, 'L', BuildTiers::default(), None), None, "engineers and bots have none");
    }

    #[test]
    fn keyboard_focus_never_stays_on_a_button() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        let id = egui::Id::new("button");
        ctx.memory_mut(|memory| memory.request_focus(id));
        assert!(ctx.wants_keyboard_input(), "the test needs a focused button to begin with");
        release_keyboard_focus(&ctx);
        assert!(!ctx.wants_keyboard_input(), "focus would swallow hotkeys");
    }

    #[test]
    fn queue_text_says_how_many_factories_took_the_order() {
        assert_eq!(queue_label("Bot", 1, 1), "Bot queued");
        assert_eq!(queue_label("Bot", 3, 3), "Bot: all 3 factories");
        assert_eq!(queue_label("Tank", 2, 3), "Tank: 2 of 3 factories");
        assert_eq!(factory_count(1), "1 factory");
        assert_eq!(factory_count(4), "4 factories");
    }

    #[test]
    fn placement_grid_falls_on_cell_edges_and_stays_on_the_map() {
        let lines = placement_grid_lines((50, 50), 20.0, 3.0, 512);
        assert_eq!(lines.len(), (GRID_REACH_CELLS as usize * 2 + 1) * 2);
        for (from, to) in &lines {
            for value in [from[0], from[1], to[0], to[1]] {
                assert_eq!(value % 20.0, 0.0, "{value} is not on a cell edge");
            }
            assert_eq!(from[2], 3.0);
        }
        // At the map's corner, lines that would fall outside it are dropped or cut short.
        for (from, to) in placement_grid_lines((1, 1), 20.0, 0.0, 512) {
            assert!(from[0] >= 0.0 && from[1] >= 0.0 && to[0] >= 0.0 && to[1] >= 0.0);
        }
    }

    #[test]
    fn a_builders_trail_runs_through_each_build_in_order() {
        let plan = UnitPlanView {
            id: SimId(1),
            steps: vec![step(PlanStepKind::Build, 100.0, 0.0), step(PlanStepKind::Build, 200.0, 60.0), step(PlanStepKind::Build, 300.0, 20.0)],
        };
        let current = snapshot(vec![unit(1, 0.0)], vec![plan]);
        let legs = build_trails(&current, &current, 1.0, &[SimId(1)]);
        let from: Vec<f64> = legs.iter().map(|leg| leg.from[0]).collect();
        let to: Vec<f64> = legs.iter().map(|leg| leg.to[0]).collect();
        assert_eq!(from, [0.0, 100.0, 200.0]);
        assert_eq!(to, [100.0, 200.0, 300.0]);
        let numbers: Vec<usize> = legs.iter().map(|leg| leg.number).collect();
        assert_eq!(numbers, [1, 2, 3]);
    }

    #[test]
    fn shift_view_covers_every_other_unit_and_fades() {
        let plan = |id: u32| UnitPlanView { id: SimId(id), steps: vec![step(PlanStepKind::Move, 100.0, 0.0)] };
        let factory = FactoryView { id: SimId(3), queue: vec![], repeat: false, progress_permille: 0, rally: Some([raw(500.0), 0, 0]) };
        let mut current = snapshot(vec![unit(1, 0.0), unit(2, 0.0), unit(3, 0.0), unit(4, 0.0)], vec![plan(1), plan(2)]);
        current.factories = vec![factory];
        let legs = other_units_trails(&current, &current, 1.0, &[SimId(1)], 0.5);
        assert_eq!(legs.len(), 2, "the unselected unit and the factory, but not the selected unit or the idle one");
        let walker = legs.iter().find(|leg| leg.kind == MarkerKind::Move).unwrap();
        assert!((walker.opacity - TRAIL_OPACITY * 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_single_order_is_not_numbered() {
        let plan = UnitPlanView { id: SimId(1), steps: vec![step(PlanStepKind::Move, 100.0, 0.0)] };
        let current = snapshot(vec![unit(1, 0.0)], vec![plan]);
        let legs = build_trails(&current, &current, 1.0, &[SimId(1)]);
        assert_eq!(legs[0].number, 0);
        assert_eq!(legs[0].kind, MarkerKind::Move);
    }

    #[test]
    fn only_selected_units_have_trails() {
        let plan = UnitPlanView { id: SimId(1), steps: vec![step(PlanStepKind::Move, 100.0, 0.0)] };
        let current = snapshot(vec![unit(1, 0.0)], vec![plan]);
        assert!(build_trails(&current, &current, 1.0, &[]).is_empty());
    }

    #[test]
    fn a_factorys_rally_line_is_dimmer_than_an_order() {
        let factory = FactoryView { id: SimId(2), queue: vec![], repeat: false, progress_permille: 0, rally: Some([raw(500.0), 0, 0]) };
        let plan = UnitPlanView { id: SimId(1), steps: vec![step(PlanStepKind::Move, 100.0, 0.0)] };
        let mut current = snapshot(vec![unit(1, 0.0), unit(2, 50.0)], vec![plan]);
        current.factories = vec![factory];
        let legs = build_trails(&current, &current, 1.0, &[SimId(1), SimId(2)]);
        let rally = legs.iter().find(|leg| leg.kind == MarkerKind::Rally).expect("the factory has a rally line");
        let order = legs.iter().find(|leg| leg.kind == MarkerKind::Move).unwrap();
        assert_eq!(rally.to[0], 500.0);
        assert!(rally.opacity < order.opacity);
    }

    #[test]
    fn a_trail_starts_where_the_unit_appears_between_ticks() {
        let plan = UnitPlanView { id: SimId(1), steps: vec![step(PlanStepKind::Move, 100.0, 0.0)] };
        let previous = snapshot(vec![unit(1, 0.0)], vec![]);
        let current = snapshot(vec![unit(1, 10.0)], vec![plan]);
        let legs = build_trails(&previous, &current, 0.5, &[SimId(1)]);
        assert_eq!(legs[0].from[0], 5.0);
    }

    #[test]
    fn large_selections_are_capped() {
        let units: Vec<UnitView> = (0..200).map(|id| unit(id, 0.0)).collect();
        let plans: Vec<UnitPlanView> =
            (0..200).map(|id| UnitPlanView { id: SimId(id), steps: vec![step(PlanStepKind::Move, 100.0, 0.0)] }).collect();
        let selected: Vec<SimId> = (0..200).map(SimId).collect();
        let current = snapshot(units, plans);
        assert_eq!(build_trails(&current, &current, 1.0, &selected).len(), MAX_TRAIL_UNITS);
    }

    #[test]
    fn segments_clip_to_the_screen() {
        let bounds = Rect::from_min_max(Pos2::ZERO, Pos2::new(100.0, 100.0));
        let inside = (Pos2::new(10.0, 10.0), Pos2::new(20.0, 20.0));
        assert_eq!(clip_segment(inside.0, inside.1, bounds), Some(inside));
        let across = clip_segment(Pos2::new(50.0, 50.0), Pos2::new(1_000_000.0, 50.0), bounds).unwrap();
        assert_eq!(across, (Pos2::new(50.0, 50.0), Pos2::new(100.0, 50.0)));
        assert_eq!(clip_segment(Pos2::new(200.0, 0.0), Pos2::new(300.0, 50.0), bounds), None);
    }
}
