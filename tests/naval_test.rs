//! Ships (GAME_SPECIFICATION.md §10): they sail deep water only, with routes of their own, and
//! are built by naval factories on the water.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Blueprinted, LeavingFactory, OrderKind, SimId, Team};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::terrain::{Terrain, HEIGHT_FRAC_BITS};
use sim_engine::sim::world::SimWorld;

const MAP: i32 = 5_120;
const CELLS: usize = 256;

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

/// Land 20 above the water, with a lake from x 1,400 to 3,800 across the whole map, and an
/// island in its middle, from y 2,000 to 3,000 and x 2,400 to 2,800.
fn lake_world() -> SimWorld {
    lake_world_with_fog(false)
}

fn lake_world_with_fog(fog_of_war: bool) -> SimWorld {
    let samples = CELLS + 1;
    let height = |x: usize, y: usize| {
        let (wx, wy) = (x as i32 * 20, y as i32 * 20);
        let lake = (1_400..=3_800).contains(&wx);
        let island = (2_400..=2_800).contains(&wx) && (2_000..=3_000).contains(&wy);
        if lake && !island { -40 } else { 20 }
    };
    let heights: Vec<i16> = (0..samples * samples).map(|index| (height(index % samples, index / samples) << HEIGHT_FRAC_BITS) as i16).collect();
    // Land units cannot go where any corner is under the water.
    let blocked: Vec<bool> = (0..CELLS * CELLS)
        .map(|cell| {
            let (x, y) = (cell % CELLS, cell / CELLS);
            [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)].iter().any(|&(cx, cy)| height(cx, cy) < 0)
        })
        .collect();
    let terrain = Terrain::from_heights(MAP, heights, &blocked, Fp::ZERO);
    let config = SimConfig { map_units: MAP, terrain: TerrainStyle::Flat, fog_of_war, ..SimConfig::default() };
    let mut world = SimWorld::with_terrain(config, 1, terrain);
    world.prepare_map();
    for economy in &mut world.economies {
        for value in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.base_energy_capacity, &mut economy.energy_capacity] {
            *value = Res::from_num(1_000_000);
        }
        economy.mass = Res::from_num(500_000);
        economy.energy = Res::from_num(500_000);
    }
    world
}

fn spawn(world: &mut SimWorld, key: &str, team: Team, at: Vec2Fp) -> SimId {
    world.spawn_mobile(world.blueprints.id(key), team, at)
}

fn distance(world: &SimWorld, id: SimId, to: Vec2Fp) -> i64 {
    let at = world.unit_position(id).unwrap().xy();
    let (dx, dy) = (i64::from((at.x - to.x).to_num::<i32>()), i64::from((at.y - to.y).to_num::<i32>()));
    (dx * dx + dy * dy).isqrt()
}

fn on_water(world: &SimWorld, id: SimId) -> bool {
    let at = world.unit_position(id).unwrap().xy();
    let (x, y) = world.terrain.cell_of(at);
    world.terrain.cell_is_navigable(x as u32, y as u32)
}

#[test]
fn a_ship_sails_around_an_island_and_never_leaves_the_water() {
    let mut world = lake_world();
    let ship = spawn(&mut world, "t1_frigate", Team::Blue, point(2_000, 2_500));
    let goal = point(3_200, 2_500);
    world.advance_tick(&[SimCommand::Move { units: vec![ship], goal }]);
    let arrived = (0..1_000).find(|_| {
        world.advance_tick(&[]);
        assert!(on_water(&world, ship), "the ship left the water");
        world.unit_order(ship).unwrap().kind == OrderKind::Idle
    });
    assert!(arrived.is_some(), "it should get round the island");
    assert!(distance(&world, ship, goal) < 150);
    let at = world.unit_position(ship).unwrap();
    assert_eq!(at.z, fp_int(14), "it floats on the water");
}

#[test]
fn a_ship_sent_onto_land_stops_at_the_shore() {
    let mut world = lake_world();
    let ship = spawn(&mut world, "t1_frigate", Team::Blue, point(2_000, 1_000));
    world.advance_tick(&[SimCommand::Move { units: vec![ship], goal: point(500, 1_000) }]);
    for _ in 0..400 {
        world.advance_tick(&[]);
        assert!(on_water(&world, ship));
    }
    assert!(distance(&world, ship, point(1_450, 1_000)) < 200, "it waits at the near shore");
}

#[test]
fn ships_and_land_units_ordered_together_each_take_their_own_way() {
    let mut world = lake_world();
    let ship = spawn(&mut world, "t1_frigate", Team::Blue, point(1_600, 1_000));
    let tank = spawn(&mut world, "t2_heavy_tank", Team::Blue, point(1_200, 1_000));
    world.advance_tick(&[SimCommand::Move { units: vec![ship, tank], goal: point(1_600, 4_000) }]);
    let (ship_route, tank_route) = (world.unit_order(ship).unwrap().route, world.unit_order(tank).unwrap().route);
    assert_ne!(ship_route, tank_route, "a route each");
    for _ in 0..600 {
        world.advance_tick(&[]);
        assert!(on_water(&world, ship));
        let at = world.unit_position(tank).unwrap().xy();
        assert!(world.terrain.is_passable(at), "the tank stays on land");
    }
    assert!(distance(&world, ship, point(1_600, 4_000)) < 300);
}

#[test]
fn a_naval_factory_goes_on_the_water_and_launches_its_ships_onto_it() {
    let mut world = lake_world();
    let factory_key = world.blueprints.id("naval_factory");
    assert!(world.spawn_finished_structure(factory_key, Team::Blue, 30, 50).is_none(), "not on land");
    let factory = world.spawn_finished_structure(factory_key, Team::Blue, 75, 50).expect("on deep water");
    let frigate = world.blueprints.id("t1_frigate");
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: frigate, count: 1 }]);
    // 1,400 build time at 20 build power is 70 seconds.
    let built = (0..800).find_map(|_| {
        world.advance_tick(&[]);
        world.units.iter().map(|unit| unit.0).find(|&id| world.ecs.get::<&Blueprinted>(world.unit_entity(id).unwrap()).is_ok_and(|unit| unit.0 == frigate))
    });
    let ship = built.expect("it should build a frigate");
    // It is built in the dock, then sails out of its open end onto the water.
    for _ in 0..100 {
        if world.ecs.get::<&LeavingFactory>(world.unit_entity(ship).unwrap()).is_err() {
            break;
        }
        world.advance_tick(&[]);
    }
    assert!(world.ecs.get::<&LeavingFactory>(world.unit_entity(ship).unwrap()).is_err(), "the frigate never left the dock");
    assert!(on_water(&world, ship), "launched onto the water");
    // An engineer cannot place one on land either.
    let engineer = spawn(&mut world, "t1_engineer", Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[SimCommand::Build { units: vec![engineer], blueprint: factory_key, cell_x: 40, cell_y: 40, queued: false }]);
    for _ in 0..50 {
        world.advance_tick(&[]);
    }
    assert!(world.units.iter().filter(|unit| world.ecs.get::<&Blueprinted>(unit.1).is_ok_and(|bp| bp.0 == factory_key)).count() == 1);
}

#[test]
fn a_frigate_shells_the_shore_and_shoots_at_aircraft() {
    let mut world = lake_world();
    let ship = spawn(&mut world, "t1_frigate", Team::Blue, point(1_600, 2_500));
    let generator = world.spawn_finished_structure(world.blueprints.id("power_generator"), Team::Red, 62, 125).unwrap();
    let scout = spawn(&mut world, "t1_air_scout", Team::Red, point(1_700, 2_500));
    world.advance_tick(&[SimCommand::Follow { units: vec![scout], target: ship }]);
    for _ in 0..300 {
        world.advance_tick(&[]);
    }
    assert!(world.unit_health(generator).is_none(), "its gun reaches the generator on the shore");
    assert!(world.unit_health(scout).is_none(), "and its railgun the scout");
}

#[test]
fn a_cruiser_fires_its_missiles_on_its_own_beyond_its_guns() {
    let mut world = lake_world();
    let cruiser = spawn(&mut world, "t2_cruiser", Team::Blue, point(3_600, 1_000));
    // A generator on the far shore, 2,400 away: beyond its gun, within its missiles' 3,000.
    let generator = world.spawn_finished_structure(world.blueprints.id("power_generator"), Team::Red, 59, 50).unwrap();
    let gone = (0..600).any(|_| {
        world.advance_tick(&[]);
        world.unit_health(generator).is_none()
    });
    assert!(gone, "its missiles should destroy the generator");
    assert!(world.unit_health(cruiser).is_some());
}

#[test]
fn a_cruiser_shoots_down_missiles_fired_at_it() {
    let mut world = lake_world();
    spawn(&mut world, "t2_cruiser", Team::Blue, point(1_600, 1_000));
    let red = spawn(&mut world, "t2_cruiser", Team::Red, point(3_600, 1_000));
    let mut intercepted = 0;
    for _ in 0..400 {
        world.advance_tick(&[]);
        intercepted += world.snapshot_for(Some(Team::Blue)).interceptions.len();
    }
    assert!(intercepted > 0, "the close-in gun should stop some of the missiles");
    let _ = red;
}

#[test]
fn a_battleship_outranges_and_sinks_a_destroyer() {
    let mut world = lake_world();
    let battleship = spawn(&mut world, "t3_battleship", Team::Blue, point(1_600, 1_000));
    let destroyer = spawn(&mut world, "t2_destroyer", Team::Red, point(3_600, 1_000));
    let full = world.unit_health(battleship).unwrap();
    let sunk = (0..1_200).any(|_| {
        world.advance_tick(&[]);
        world.unit_health(destroyer).is_none()
    });
    assert!(sunk, "three turrets of 3,000 every 20 seconds against 7,200");
    assert_eq!(world.unit_health(battleship), Some(full), "2,000 away, beyond the destroyer's 1,200");
}

#[test]
fn naval_support_factories_need_a_naval_headquarters() {
    let mut world = lake_world();
    let engineer = spawn(&mut world, "t2_engineer", Team::Blue, point(1_350, 1_000));
    let support = world.blueprints.id("naval_factory_t2_support");
    world.advance_tick(&[SimCommand::Build { units: vec![engineer], blueprint: support, cell_x: 71, cell_y: 49, queued: false }]);
    let tasks = |world: &SimWorld| world.ecs.get::<&sim_engine::sim::components::Builder>(world.unit_entity(engineer).unwrap()).unwrap().tasks.len();
    assert_eq!(tasks(&world), 0, "no naval headquarters");
    world.spawn_finished_structure(world.blueprints.id("naval_factory_t2"), Team::Blue, 80, 100).unwrap();
    world.advance_tick(&[SimCommand::Build { units: vec![engineer], blueprint: support, cell_x: 71, cell_y: 49, queued: false }]);
    assert_eq!(tasks(&world), 1, "with one");
}

fn seen_by(world: &SimWorld, viewer: Team, id: SimId) -> bool {
    world.snapshot_for(Some(viewer)).units.iter().any(|unit| unit.id == id)
}

#[test]
fn a_submarine_is_hidden_until_sonar_finds_it() {
    let mut world = lake_world_with_fog(true);
    let sub = spawn(&mut world, "t1_attack_sub", Team::Blue, point(2_000, 1_000));
    world.spawn_mobile(world.blueprints.id("t2_heavy_tank"), Team::Red, point(1_300, 1_000));
    run_ticks(&mut world, 10);
    assert!(!seen_by(&world, Team::Red, sub), "the tank's sight does not reach under the water");
    let blips = |world: &SimWorld| world.snapshot_for(Some(Team::Red)).blips.len();
    assert_eq!(blips(&world), 0, "nor does anything show on radar");
    // A sonar 600 away, beyond its own sight, finds it as a contact.
    world.spawn_finished_structure(world.blueprints.id("t1_sonar"), Team::Red, 130, 50).unwrap();
    run_ticks(&mut world, 10);
    assert_eq!(blips(&world), 1, "a sonar contact");
    assert!(!seen_by(&world, Team::Red, sub), "but not in sight");
    // With the sonar's contact and the sight of a ship nearby, it is in full view.
    spawn(&mut world, "t1_frigate", Team::Red, point(2_200, 1_000));
    run_ticks(&mut world, 10);
    assert!(seen_by(&world, Team::Red, sub));
}

fn run_ticks(world: &mut SimWorld, ticks: u32) {
    for _ in 0..ticks {
        world.advance_tick(&[]);
    }
}

#[test]
fn only_torpedoes_hurt_a_submarine() {
    let mut world = lake_world();
    let sub = spawn(&mut world, "t1_attack_sub", Team::Blue, point(2_000, 1_000));
    world.ecs.get::<&mut sim_engine::sim::components::Combat>(world.unit_entity(sub).unwrap()).unwrap().range = Fp::ZERO;
    // A frigate close enough for its sonar to find the sub, whose guns cannot reach it.
    spawn(&mut world, "t1_frigate", Team::Red, point(2_200, 1_000));
    run_ticks(&mut world, 200);
    assert_eq!(world.unit_health(sub), Some(fp_int(800)), "shells pass over it");
    // A destroyer's torpedoes do.
    spawn(&mut world, "t2_destroyer", Team::Red, point(2_300, 1_100));
    let sunk = (0..600).any(|_| {
        world.advance_tick(&[]);
        world.unit_health(sub).is_none()
    });
    assert!(sunk, "the destroyer's torpedoes should sink it");
}

#[test]
fn a_submarine_sinks_ships_and_leaves_land_alone() {
    let mut world = lake_world();
    spawn(&mut world, "t1_attack_sub", Team::Blue, point(1_500, 1_000));
    let tank = spawn(&mut world, "t2_heavy_tank", Team::Red, point(1_300, 1_000));
    let frigate = spawn(&mut world, "t1_frigate", Team::Red, point(1_900, 1_000));
    world.ecs.get::<&mut sim_engine::sim::components::Combat>(world.unit_entity(tank).unwrap()).unwrap().range = Fp::ZERO;
    let full = world.unit_health(tank).unwrap();
    let sunk = (0..800).any(|_| {
        world.advance_tick(&[]);
        world.unit_health(frigate).is_none()
    });
    assert!(sunk, "its torpedoes should sink the frigate");
    assert_eq!(world.unit_health(tank), Some(full), "torpedoes do not reach land");
}

#[test]
fn a_torpedo_bomber_sinks_a_ship() {
    let mut world = lake_world();
    let destroyer = spawn(&mut world, "t2_destroyer", Team::Red, point(2_000, 1_000));
    let combat = |world: &mut SimWorld, id: SimId| world.ecs.get::<&mut sim_engine::sim::components::ExtraWeapons>(world.unit_entity(id).unwrap()).unwrap().0.clear();
    combat(&mut world, destroyer);
    let bomber = spawn(&mut world, "t2_torpedo_bomber", Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[SimCommand::Attack { units: vec![bomber], target: destroyer }]);
    // Five torpedoes of 150 every 10 seconds, less the one its charge stops, against 7,200: about
    // twelve runs, as in FAF, where one torpedo bomber is slow work against a destroyer.
    let sunk = (0..1_800).any(|_| {
        world.advance_tick(&[]);
        world.unit_health(destroyer).is_none()
    });
    assert!(sunk, "a torpedo bomber should sink an undefended destroyer within three minutes");
}

#[test]
fn sonar_and_torpedo_launchers_go_on_the_water() {
    let mut world = lake_world();
    for (key, cell_y) in [("t1_sonar", 30), ("t1_torpedo_launcher", 40), ("t2_torpedo_launcher", 50)] {
        let blueprint = world.blueprints.id(key);
        assert!(world.spawn_finished_structure(blueprint, Team::Blue, 30, cell_y).is_none(), "{key} on land");
        assert!(world.spawn_finished_structure(blueprint, Team::Blue, 90, cell_y).is_some(), "{key} on water");
    }
}

#[test]
fn an_engineer_floats_out_on_the_water_and_builds_a_naval_factory_far_from_shore() {
    let mut world = lake_world();
    let engineer = spawn(&mut world, "t1_engineer", Team::Blue, point(1_000, 1_000));
    let factory_key = world.blueprints.id("naval_factory");
    // Cell 100 is x 2,000, 600 out from the shore at 1,400: far beyond an engineer's reach of 100.
    world.advance_tick(&[SimCommand::Build { units: vec![engineer], blueprint: factory_key, cell_x: 100, cell_y: 50, queued: false }]);
    let mut floated = false;
    let built = (0..3_000).find(|_| {
        world.advance_tick(&[]);
        if on_water(&world, engineer) {
            assert_eq!(world.unit_position(engineer).unwrap().z, fp_int(8), "it floats on the water, its radius up");
            floated = true;
        }
        world.units.iter().any(|unit| {
            world.ecs.get::<&Blueprinted>(unit.1).is_ok_and(|bp| bp.0 == factory_key)
                && world.ecs.get::<&sim_engine::sim::components::Construction>(unit.1).is_err()
        })
    });
    assert!(floated, "it went out on the water");
    assert!(built.is_some(), "and finished the factory there");
}

#[test]
fn a_hovercraft_crosses_land_and_water_alike() {
    let mut world = lake_world();
    let hovercraft = spawn(&mut world, "t2_assault_hovercraft", Team::Blue, point(1_000, 1_000));
    let goal = point(4_200, 1_000);
    world.advance_tick(&[SimCommand::Move { units: vec![hovercraft], goal }]);
    let mut floated = false;
    let arrived = (0..1_200).find(|_| {
        world.advance_tick(&[]);
        let at = world.unit_position(hovercraft).unwrap();
        if on_water(&world, hovercraft) {
            assert_eq!(at.z, fp_int(11), "it floats on the water");
            floated = true;
        }
        world.unit_order(hovercraft).unwrap().kind == OrderKind::Idle
    });
    assert!(arrived.is_some() && floated, "straight across the lake");
    assert!(distance(&world, hovercraft, goal) < 200);
}

#[test]
fn a_commander_walks_the_seabed_hidden_and_silent() {
    let mut world = lake_world_with_fog(true);
    let commander = spawn(&mut world, "command_rig", Team::Blue, point(1_200, 1_000));
    // A hovercraft on the water beside its path, which sees it but has no sonar.
    let tank = spawn(&mut world, "t2_assault_hovercraft", Team::Red, point(2_600, 1_250));
    world.ecs.get::<&mut sim_engine::sim::components::Combat>(world.unit_entity(tank).unwrap()).unwrap().range = Fp::ZERO;
    world.advance_tick(&[SimCommand::Move { units: vec![commander], goal: point(2_600, 1_000) }]);
    let full = world.unit_health(tank).unwrap();
    let mut was_under = false;
    for _ in 0..300 {
        world.advance_tick(&[]);
        let at = world.unit_position(commander).unwrap();
        if at.z < Fp::ZERO {
            was_under = true;
            assert!(!seen_by(&world, Team::Red, commander), "hidden under the water");
            assert_eq!(world.unit_health(tank), Some(full), "and it cannot fire from there");
        }
    }
    assert!(was_under, "it should walk along the bottom");
}

#[test]
fn a_commander_on_an_attack_move_under_the_water_walks_on_past_what_it_cannot_shoot() {
    // On an attack-move, a unit stops for a target. A commander on the seabed cannot fire, so
    // it took the frigate as one anyway, and stood under the water for the rest of the match.
    let mut world = lake_world();
    let commander = spawn(&mut world, "command_rig", Team::Blue, point(1_200, 1_000));
    let frigate = spawn(&mut world, "t1_frigate", Team::Red, point(2_000, 1_250));
    world.ecs.get::<&mut sim_engine::sim::components::Combat>(world.unit_entity(frigate).unwrap()).unwrap().range = Fp::ZERO;
    let goal = point(2_600, 1_000);
    world.advance_tick(&[SimCommand::AttackMove { units: vec![commander], goal }]);
    let mut was_under = false;
    for _ in 0..600 {
        world.advance_tick(&[]);
        was_under |= world.unit_position(commander).unwrap().z < Fp::ZERO;
    }
    assert!(was_under, "it should cross along the bottom");
    assert!(distance(&world, commander, goal) < 200, "it stopped under the water, {} from its goal", distance(&world, commander, goal));
}

#[test]
fn a_submarine_surfaces_to_use_its_deck_gun_and_can_be_shelled_there_until_it_dives() {
    // Near the shore, with a Red light bot on the beach.
    let mut world = lake_world();
    let sub = spawn(&mut world, "t1_attack_sub", Team::Blue, point(1_700, 1_000));
    let bot = spawn(&mut world, "t1_light_walker", Team::Red, point(1_250, 1_000));
    world.ecs.get::<&mut sim_engine::sim::components::Combat>(world.unit_entity(bot).unwrap()).unwrap().range = Fp::ZERO;
    let bot_health = world.unit_health(bot).unwrap();
    for _ in 0..100 {
        world.advance_tick(&[]);
    }
    assert!(world.unit_position(sub).unwrap().z < Fp::ZERO);
    assert_eq!(world.unit_health(bot), Some(bot_health), "submerged, its deck gun is silent");

    world.advance_tick(&[SimCommand::SetSurfaced { units: vec![sub], surfaced: true }]);
    assert_eq!(world.surfaced, vec![sub]);
    for _ in 0..100 {
        world.advance_tick(&[]);
    }
    assert!(world.unit_position(sub).unwrap().z >= Fp::ZERO, "on the surface it rides on the water");
    assert!(world.unit_health(bot).is_none_or(|health| health < bot_health), "surfaced, its deck gun fires at the shore");

    // A Red frigate comes up. The sub's weapons and the frigate's anti-air are left out, to see
    // the frigate's shells alone.
    world.ecs.get::<&mut sim_engine::sim::components::Combat>(world.unit_entity(sub).unwrap()).unwrap().range = Fp::ZERO;
    let frigate = spawn(&mut world, "t1_frigate", Team::Red, point(1_900, 1_000));
    world.ecs.get::<&mut sim_engine::sim::components::ExtraWeapons>(world.unit_entity(frigate).unwrap()).unwrap().0.clear();
    let full = world.unit_health(sub).unwrap();
    for _ in 0..100 {
        world.advance_tick(&[]);
    }
    assert!(world.unit_health(sub).unwrap() < full, "surfaced, the frigate's shells reach it");

    world.advance_tick(&[SimCommand::SetSurfaced { units: vec![sub], surfaced: false }]);
    for _ in 0..20 {
        world.advance_tick(&[]);
    }
    let hurt = world.unit_health(sub).unwrap();
    for _ in 0..100 {
        world.advance_tick(&[]);
    }
    assert!(world.unit_position(sub).unwrap().z < Fp::ZERO, "it went back down");
    assert_eq!(world.unit_health(sub), Some(hurt), "and shells pass over it again");
    assert!(world.surfaced.is_empty());
}

/// The water is 40 deep here, far more than any unit is tall, so a shell skimming the surface
/// is far above the seabed and must still be checked against the ships on it.
#[test]
fn shells_hit_ships_over_deep_water() {
    let mut world = lake_world();
    let blue = spawn(&mut world, "t1_frigate", Team::Blue, point(1_700, 1_000));
    world.ecs.get::<&mut sim_engine::sim::components::Combat>(world.unit_entity(blue).unwrap()).unwrap().range = Fp::ZERO;
    world.ecs.get::<&mut sim_engine::sim::components::ExtraWeapons>(world.unit_entity(blue).unwrap()).unwrap().0.clear();
    let red = spawn(&mut world, "t1_frigate", Team::Red, point(1_900, 1_000));
    world.ecs.get::<&mut sim_engine::sim::components::ExtraWeapons>(world.unit_entity(red).unwrap()).unwrap().0.clear();
    let full = world.unit_health(blue).unwrap();
    for _ in 0..100 {
        world.advance_tick(&[]);
    }
    assert!(world.unit_health(blue).unwrap() < full, "the frigate's shells strike the other frigate");
}

#[test]
fn only_torpedoes_reach_a_commander_under_the_water() {
    let mut world = lake_world();
    let commander = spawn(&mut world, "command_rig", Team::Blue, point(2_000, 1_000));
    world.ecs.get::<&mut sim_engine::sim::components::Combat>(world.unit_entity(commander).unwrap()).unwrap().range = Fp::ZERO;
    let full = world.unit_health(commander).unwrap();
    spawn(&mut world, "t1_frigate", Team::Red, point(2_250, 1_000));
    run_ticks(&mut world, 200);
    assert_eq!(world.unit_health(commander), Some(full), "shells pass over it");
    spawn(&mut world, "t1_attack_sub", Team::Red, point(2_300, 1_100));
    run_ticks(&mut world, 200);
    assert!(world.unit_health(commander).unwrap() < full, "torpedoes do not");
}

#[test]
fn a_group_of_every_kind_gets_a_route_for_each() {
    let mut world = lake_world();
    let tank = spawn(&mut world, "t2_heavy_tank", Team::Blue, point(1_200, 1_000));
    let hovercraft = spawn(&mut world, "t2_assault_hovercraft", Team::Blue, point(1_250, 1_000));
    let frigate = spawn(&mut world, "t1_frigate", Team::Blue, point(1_600, 1_000));
    world.advance_tick(&[SimCommand::Move { units: vec![tank, hovercraft, frigate], goal: point(1_600, 4_000) }]);
    let routes: Vec<u32> = [tank, hovercraft, frigate].iter().map(|&id| world.unit_order(id).unwrap().route).collect();
    assert!(routes[0] != routes[1] && routes[1] != routes[2] && routes[0] != routes[2], "{routes:?}");
}

#[test]
fn a_escort_corvettes_dome_moves_with_it_and_stops_shells() {
    let mut world = lake_world();
    let boat = spawn(&mut world, "t2_escort_corvette", Team::Blue, point(2_000, 1_000));
    let frigate = spawn(&mut world, "t1_frigate", Team::Blue, point(2_050, 1_000));
    world.ecs.get::<&mut sim_engine::sim::components::Combat>(world.unit_entity(frigate).unwrap()).unwrap().range = Fp::ZERO;
    world.ecs.get::<&mut sim_engine::sim::components::ExtraWeapons>(world.unit_entity(frigate).unwrap()).unwrap().0.clear();
    // A destroyer outside the dome, with only its gun.
    let enemy = spawn(&mut world, "t2_destroyer", Team::Red, point(3_000, 1_000));
    world.ecs.get::<&mut sim_engine::sim::components::ExtraWeapons>(world.unit_entity(enemy).unwrap()).unwrap().0.clear();
    let full = world.unit_health(frigate).unwrap();
    run_ticks(&mut world, 200);
    let shield = |world: &SimWorld| world.snapshot_for(Some(Team::Blue)).shields.iter().find(|view| view.id == boat).map(|view| (view.health_permille, view.center));
    let (health, start) = shield(&world).expect("its dome");
    assert!(health < 1_000, "the dome takes the shells");
    assert_eq!(world.unit_health(frigate), Some(full), "and the frigate under it none");
    // It sails away, and the dome goes with it.
    world.advance_tick(&[SimCommand::Move { units: vec![boat], goal: point(2_000, 1_600) }]);
    run_ticks(&mut world, 150);
    let (_, later) = shield(&world).expect("its dome");
    assert!(later[1] - start[1] > fp_int(300).to_bits(), "the dome moved with the boat");
}

#[test]
fn anti_torpedo_charges_shoot_torpedoes_down() {
    let damage_taken = |charges: bool| {
        let mut world = lake_world();
        let destroyer = spawn(&mut world, "t2_destroyer", Team::Blue, point(2_000, 1_000));
        let entity = world.unit_entity(destroyer).unwrap();
        world.ecs.get::<&mut sim_engine::sim::components::Combat>(entity).unwrap().range = Fp::ZERO;
        world.ecs.get::<&mut sim_engine::sim::components::ExtraWeapons>(entity).unwrap().0.clear();
        if !charges {
            world.ecs.remove_one::<sim_engine::sim::components::TorpedoDefense>(entity).unwrap();
            world.torpedo_defenses.clear();
        }
        world.spawn_finished_structure(world.blueprints.id("t1_torpedo_launcher"), Team::Red, 120, 50).unwrap();
        let full = world.unit_health(destroyer).unwrap();
        run_ticks(&mut world, 400);
        full - world.unit_health(destroyer).unwrap_or(Fp::ZERO)
    };
    let (without, with) = (damage_taken(false), damage_taken(true));
    assert!(without > Fp::ZERO);
    assert!(with < without, "the charges stop some torpedoes: {with} against {without}");
}

fn stock(world: &SimWorld, id: SimId) -> u32 {
    world.ecs.get::<&sim_engine::sim::components::Launcher>(world.unit_entity(id).unwrap()).unwrap().stock
}

#[test]
fn a_missile_sub_builds_nuclear_missiles_only_on_order_and_fires_cruise_missiles_on_its_own() {
    let mut world = lake_world();
    let sub = spawn(&mut world, "t3_missile_sub", Team::Blue, point(3_600, 1_000));
    // A generator on the far shore, 2,400 away, in reach of its cruise missiles.
    let generator = world.spawn_finished_structure(world.blueprints.id("power_generator"), Team::Red, 59, 50).unwrap();
    let gone = (0..600).any(|_| {
        world.advance_tick(&[]);
        world.unit_health(generator).is_none()
    });
    assert!(gone, "its cruise missiles fire on their own, and cost nothing");
    assert_eq!(stock(&world, sub), 0, "no nuclear missile without an order to build one");
    let nuke = world.blueprints.id("strategic_missile");
    // FAF's missile, 12,000 mass and 1,500,000 energy over 450,000 build time, takes the sub's
    // 1,080 build power almost seven minutes, so here it builds 20 times as fast, from deep stores.
    {
        let entity = world.unit_entity(sub).unwrap();
        world.ecs.get::<&mut sim_engine::sim::components::Factory>(entity).unwrap().power = fp_int(22_500);
        let economy = &mut world.economies[Team::Blue.index()];
        economy.base_mass_capacity = Res::from_num(1_000_000);
        economy.mass_capacity = economy.base_mass_capacity;
        economy.mass = economy.mass_capacity;
        economy.base_energy_capacity = Res::from_num(100_000_000);
        economy.energy_capacity = economy.base_energy_capacity;
        economy.energy = economy.energy_capacity;
    }
    world.advance_tick(&[SimCommand::QueueUnit { factory: sub, blueprint: nuke, count: 1 }]);
    // 450,000 build time at 22,500 build power is 20 seconds.
    let built = (0..2_100).any(|_| {
        world.advance_tick(&[]);
        stock(&world, sub) == 1
    });
    assert!(built, "it builds one when told to");
    run_ticks(&mut world, 2_100);
    assert_eq!(stock(&world, sub), 1, "and only the one");
    // Building one after another, it stops at three.
    world.advance_tick(&[SimCommand::QueueUnit { factory: sub, blueprint: nuke, count: 1 }]);
    world.advance_tick(&[SimCommand::SetRepeat { factory: sub, repeat: true }]);
    run_ticks(&mut world, 6_500);
    assert_eq!(stock(&world, sub), 3, "up to three");
}
