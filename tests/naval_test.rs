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
    let full = world.unit_health(sub);
    // A frigate close enough for its sonar to find the sub, whose guns cannot reach it.
    spawn(&mut world, "t1_frigate", Team::Red, point(2_200, 1_000));
    run_ticks(&mut world, 200);
    assert_eq!(world.unit_health(sub), full, "shells pass over it");
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
    // Its anti-torpedo charge off too: since 2026-11-24 it stops one torpedo a second, every one a
    // lone torpedo bomber drops, and only a wing of them saturates it.
    world.ecs.get::<&mut sim_engine::sim::components::TorpedoDefense>(world.unit_entity(destroyer).unwrap()).unwrap().range = Fp::ZERO;
    let bomber = spawn(&mut world, "t2_torpedo_bomber", Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[SimCommand::Attack { units: vec![bomber], target: destroyer }]);
    // Five torpedoes of 189 every 10 seconds against 10,231: about eleven runs, as in the
    // inspiration, where one torpedo bomber is slow work against a destroyer.
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
            assert_eq!(at.z, fp_int(13), "it floats on the water, its radius above it");
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
    let sub = spawn(&mut world, "t1_attack_sub", Team::Blue, point(1_600, 1_000));
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
    // The destroyer's charge, and the escort corvette's.
    for key in ["t2_destroyer", "t2_escort_corvette"] {
        let damage_taken = |charges: bool| {
            let mut world = lake_world();
            let ship = spawn(&mut world, key, Team::Blue, point(2_000, 1_000));
            let entity = world.unit_entity(ship).unwrap();
            if let Ok(mut combat) = world.ecs.get::<&mut sim_engine::sim::components::Combat>(entity) {
                combat.range = Fp::ZERO;
            }
            if let Ok(mut extras) = world.ecs.get::<&mut sim_engine::sim::components::ExtraWeapons>(entity) {
                extras.0.clear();
            }
            if !charges {
                world.ecs.remove_one::<sim_engine::sim::components::TorpedoDefense>(entity).unwrap();
                world.torpedo_defenses.clear();
            }
            world.spawn_finished_structure(world.blueprints.id("t1_torpedo_launcher"), Team::Red, 120, 50).unwrap();
            let full = world.unit_health(ship).unwrap();
            // Short enough that the corvette, the lighter of the two, is still afloat with them.
            run_ticks(&mut world, 200);
            full - world.unit_health(ship).unwrap_or(Fp::ZERO)
        };
        let (without, with) = (damage_taken(false), damage_taken(true));
        assert!(without > Fp::ZERO, "{key}");
        assert!(with < without, "{key}'s charges stop some torpedoes: {with} against {without}");
    }
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
    // The inspiration's missile, 12,000 mass and 1,500,000 energy over 450,000 build time, takes
    // the sub's 1,080 build power almost seven minutes, so here it builds 20 times as fast, from
    // deep stores.
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

/// Each ship's gun on a turret of its own fires from that turret, not from the ship's middle,
/// and rests facing the bow, or the stern for a turret aft: for every ship with mounted guns.
#[test]
fn every_mounted_gun_fires_from_its_own_turret() {
    use sim_engine::sim::components::MoveClass;
    let db = sim_engine::sim::blueprints::BlueprintDb::standard();
    let ships: Vec<_> = db.iter().filter(|unit| unit.movement == MoveClass::Naval && unit.weapon.iter().chain(&unit.extra_weapons).any(|weapon| weapon.mount != [Fp::ZERO; 2])).collect();
    assert!(!ships.is_empty(), "the battleship's turrets are mounted");
    for ship in ships {
        let mut world = lake_world();
        let id = spawn(&mut world, &ship.key, Team::Blue, point(1_700, 4_000));
        let weapons: Vec<_> = ship.weapon.iter().chain(&ship.extra_weapons).copied().collect();
        let aim_of = |view: &sim_engine::sim::world::UnitView, index: usize| if index == 0 { view.aim } else { view.extra_aims.get(index - 1).copied().unwrap_or([0, 0]) };
        let view = world.snapshot().units.into_iter().find(|unit| unit.id == id).unwrap();
        for (index, weapon) in weapons.iter().enumerate().filter(|(index, weapon)| weapon.turn_rate.is_some() && *index <= 3) {
            let rest = if weapon.aft { view.facing.map(|axis| -axis) } else { view.facing };
            assert_eq!(aim_of(&view, index), rest, "{}'s weapon {index} rests facing {}", ship.key, if weapon.aft { "aft" } else { "forward" });
        }
        // Something to shoot at, off to one side, so that every turret has to turn to it, and
        // within reach of every mounted gun.
        let reach = weapons.iter().filter(|weapon| weapon.mount != [Fp::ZERO; 2]).map(|weapon| weapon.range.to_num::<i32>()).min().unwrap();
        spawn(&mut world, "t2_destroyer", Team::Red, point(1_700 + (reach * 4 / 5).min(1_800), 4_000));
        let mut fired = Vec::new();
        for _ in 0..300 {
            world.advance_tick(&[]);
            let snapshot = world.snapshot();
            let me = snapshot.units.iter().find(|unit| unit.id == id).unwrap();
            // A laser's beam is fired and lands within the tick, so it is only among the landed.
            fired.extend(snapshot.projectiles.iter().chain(&snapshot.landed).filter(|shot| shot.source == id).filter_map(|shot| shot.from.map(|from| (from, me.x, me.y, me.facing))));
        }
        for (index, weapon) in weapons.iter().enumerate().filter(|(_, weapon)| weapon.mount != [Fp::ZERO; 2]) {
            let [right, forward] = weapon.mount.map(|axis| axis.to_num::<f64>());
            let from_turret = fired.iter().any(|&(from, x, y, facing)| {
                let [fx, fy] = facing.map(|axis| f64::from(axis) / 4096.0);
                let expected = [f64::from(x) / 4096.0 + right * fy + forward * fx, f64::from(y) / 4096.0 - right * fx + forward * fy];
                (f64::from(from[0]) / 4096.0 - expected[0]).hypot(f64::from(from[1]) / 4096.0 - expected[1]) < 1.0
            });
            assert!(from_turret, "{}'s weapon {index} fires from its turret at {right}, {forward}: shots left from {:?}", ship.key, fired.iter().map(|shot| shot.0).collect::<Vec<_>>());
        }
    }
}

/// A passive, very tough Red hovercraft, which rides over the water where torpedoes cannot
/// reach it: something for a ship's guns alone to shoot at.
fn tough_hovercraft(world: &mut SimWorld, at: Vec2Fp) -> SimId {
    use sim_engine::sim::components::{Combat, Health};
    let id = spawn(world, "t2_assault_hovercraft", Team::Red, at);
    let entity = world.unit_entity(id).unwrap();
    world.ecs.get::<&mut Combat>(entity).unwrap().enabled = false;
    let mut health = world.ecs.get::<&mut Health>(entity).unwrap();
    health.max = fp_int(400_000);
    health.current = health.max;
    id
}

fn health_of(world: &SimWorld, id: SimId) -> i64 {
    i64::from(world.unit_health(id).unwrap().to_num::<i32>())
}

/// A laser's beams from `shooter` that landed in the tick just run.
fn beams_landed(world: &SimWorld, shooter: SimId) -> Vec<sim_engine::sim::world::ProjectileView> {
    let snapshot = world.snapshot();
    assert!(snapshot.projectiles.iter().all(|shot| shot.kind != sim_engine::sim::world::ProjectileKind::Beam), "a beam is never left in flight at the end of a tick");
    snapshot.landed.into_iter().filter(|shot| shot.kind == sim_engine::sim::world::ProjectileKind::Beam && shot.source == shooter).collect()
}

/// Every laser in the unit data (the battlecruiser's two): its beam lands in the tick it is
/// fired, on one target, for exactly its damage, with no blast onto what stands beside it, and
/// it fires as often as it reloads.
#[test]
fn every_laser_hits_within_the_tick_it_fires_and_only_what_it_is_aimed_at() {
    use sim_engine::sim::blueprints::{BlueprintDb, WeaponKind};
    let db = BlueprintDb::standard();
    let armed: Vec<_> = db.iter().filter(|unit| unit.weapon.iter().chain(&unit.extra_weapons).any(|weapon| weapon.kind == WeaponKind::Laser)).collect();
    assert!(armed.iter().any(|unit| unit.key == "t3_battlecruiser"), "the battlecruiser has lasers");
    for ship in armed {
        let lasers: Vec<_> = ship.weapon.iter().chain(&ship.extra_weapons).filter(|weapon| weapon.kind == WeaponKind::Laser).collect();
        let damage = i64::from(lasers[0].damage.to_num::<i32>());
        assert!(lasers.iter().all(|laser| i64::from(laser.damage.to_num::<i32>()) == damage && laser.splash_radius == Fp::ZERO && laser.spread == Fp::ZERO), "{}'s lasers", ship.key);
        let reach = lasers.iter().map(|laser| laser.range.to_num::<i32>()).min().unwrap();
        let mut world = lake_world();
        let shooter = spawn(&mut world, &ship.key, Team::Blue, point(1_700, 4_000));
        // Two targets side by side, closer together than any shell's blast would need.
        let targets = [tough_hovercraft(&mut world, point(1_700 + reach * 3 / 5, 4_000)), tough_hovercraft(&mut world, point(1_700 + reach * 3 / 5, 4_030))];
        let ticks = 400;
        let mut fired = 0;
        for tick in 0..ticks {
            let before: Vec<i64> = targets.iter().map(|&target| health_of(&world, target)).collect();
            world.advance_tick(&[]);
            let beams = beams_landed(&world, shooter);
            let drops: Vec<i64> = targets.iter().zip(&before).map(|(&target, &before)| before - health_of(&world, target)).collect();
            assert_eq!(drops.iter().sum::<i64>(), beams.len() as i64 * damage, "{}: tick {tick}, {} beams landed and the targets lost {drops:?}", ship.key, beams.len());
            assert!(drops.iter().all(|drop| drop % damage == 0), "{}: each target is hit by whole beams, not by a blast: {drops:?}", ship.key);
            for beam in &beams {
                assert!(beam.from.is_some(), "a beam says where it was fired from, for drawing");
                let on_target = targets.iter().any(|&target| {
                    let at = world.unit_position(target).unwrap();
                    let (dx, dy) = (f64::from(beam.x) / 4096.0 - at.x.to_num::<f64>(), f64::from(beam.y) / 4096.0 - at.y.to_num::<f64>());
                    dx.hypot(dy) <= 20.0
                });
                assert!(on_target, "{}'s beam ends on its target", ship.key);
            }
            fired += beams.len();
        }
        let expected: usize = lasers.iter().map(|laser| (ticks / laser.reload_ticks) as usize).sum();
        assert!(fired >= expected - lasers.len() && fired <= expected + lasers.len(), "{} fired {fired} beams in {ticks} ticks, expected about {expected}", ship.key);
    }
}

/// A laser is a straight beam: an enemy dome stops it short of what is under it, and it is not
/// fired at all across ground that rises into its line, where a shell would arc over.
#[test]
fn a_dome_stops_a_laser_and_rising_ground_blocks_it() {
    // A dome: the escort corvette's, with a hovercraft under it.
    let mut world = lake_world();
    let shooter = spawn(&mut world, "t3_battlecruiser", Team::Blue, point(1_700, 4_000));
    let corvette = spawn(&mut world, "t2_escort_corvette", Team::Red, point(2_700, 4_000));
    let sheltered = tough_hovercraft(&mut world, point(2_650, 4_000));
    let before = health_of(&world, sheltered);
    // For as long as the dome can stand what the lasers do to it: they wear it down, as shells do.
    let db = sim_engine::sim::blueprints::BlueprintDb::standard();
    let dome_health = db.get(db.id("t2_escort_corvette")).shield.expect("the corvette has a dome").health.to_num::<f64>();
    let beam_damage = db.get(db.id("t3_battlecruiser")).weapon.expect("the battlecruiser has lasers").damage.to_num::<f64>();
    let mut beams = Vec::new();
    for _ in 0..200 {
        if (beams.len() + 2) as f64 * beam_damage > dome_health * 0.8 {
            break;
        }
        world.advance_tick(&[]);
        beams.extend(beams_landed(&world, shooter));
    }
    assert!(beams.len() >= 2, "the battlecruiser fires at what it can see: {} beams", beams.len());
    assert_eq!(health_of(&world, sheltered), before, "the dome takes the beams");
    let dome = world.unit_position(corvette).unwrap();
    for beam in &beams {
        let (dx, dy) = (f64::from(beam.x) / 4096.0 - dome.x.to_num::<f64>(), f64::from(beam.y) / 4096.0 - dome.y.to_num::<f64>());
        assert!(dx.hypot(dy) > 300.0, "a beam ends on the dome, {:.0} from its middle, not inside it", dx.hypot(dy));
    }

    // The island, 20 above the water, between the ship and a target close behind it: the line
    // from the ship's guns, high on its hull, down to the target runs into the far shore.
    let mut world = lake_world();
    let shooter = spawn(&mut world, "t3_battlecruiser", Team::Blue, point(1_800, 2_500));
    let hidden = tough_hovercraft(&mut world, point(2_880, 2_500));
    let before = health_of(&world, hidden);
    let mut beams = 0;
    for _ in 0..200 {
        world.advance_tick(&[]);
        beams += beams_landed(&world, shooter).len();
    }
    assert_eq!((beams, health_of(&world, hidden)), (0, before), "no beam is fired into the island");
}

/// The big guns at sea (2026-11-25): the battleship's three turrets are one gun three times over,
/// and fire as one broadside, its main gun and its two broadside guns; the battlecruiser's two
/// lasers have the shorter reach, on less health. Until then a battleship turret was set by
/// damage a second against the artillery, and each laser did twice one; the battleship's
/// broadside is now set by its own targets ([broadsides] and [kill_times]).
#[test]
fn the_battleships_turrets_fire_as_one_broadside_and_outrange_the_battlecruisers_lasers() {
    use sim_engine::sim::blueprints::{BlueprintDb, Targets, WeaponDef, WeaponKind};
    let db = BlueprintDb::standard();
    let guns = |key: &str, kind: WeaponKind| -> Vec<WeaponDef> {
        let unit = db.get(db.id(key));
        unit.weapon.iter().chain(&unit.extra_weapons).filter(|weapon| weapon.kind == kind && weapon.targets == Targets::Ground).copied().collect()
    };
    let (lasers, turrets) = (guns("t3_battlecruiser", WeaponKind::Laser), guns("t3_battleship", WeaponKind::Shell));
    assert_eq!((lasers.len(), turrets.len()), (2, 3));
    let same = |a: &WeaponDef, b: &WeaponDef| (a.damage, a.reload_ticks, a.range, a.projectile_speed) == (b.damage, b.reload_ticks, b.range, b.projectile_speed);
    assert!(turrets.iter().all(|turret| same(turret, &turrets[0])), "the battleship's turrets differ: {turrets:?}");
    assert!(!turrets[0].broadside && turrets[1..].iter().all(|turret| turret.broadside), "the main gun leads, and the other two fire in its broadside");
    let (cruiser, ship) = (db.get(db.id("t3_battlecruiser")), db.get(db.id("t3_battleship")));
    assert!(lasers[0].range < turrets[0].range && cruiser.health < ship.health, "the battlecruiser has the shorter reach and the lower health");
}

/// Every ship's dome stays beneath every aircraft that attacks what is under it, hull and all, so
/// that none flies inside a dome and fires from under it: its fire must come down through the dome
/// (2026-11-24). Read from the data, for every such ship and aircraft.
#[test]
fn ships_domes_stay_beneath_the_aircraft_that_attack_them() {
    use sim_engine::sim::blueprints::{BlueprintDb, Targets};
    use sim_engine::sim::components::MoveClass;
    let db = BlueprintDb::standard();
    let attacks_below = |unit: &sim_engine::sim::blueprints::Blueprint| unit.weapon.iter().chain(&unit.extra_weapons).any(|weapon| weapon.targets != Targets::Air);
    let mut checked = 0;
    for ship in db.iter().filter(|unit| unit.move_class() == MoveClass::Naval) {
        let Some(dome) = ship.shield else { continue };
        for aircraft in db.iter().filter(|unit| attacks_below(unit)) {
            let Some(flight) = aircraft.flight else { continue };
            assert!(flight.elevation - aircraft.radius > dome.height, "{} flies at {}, inside {}'s dome {} high", aircraft.key, flight.elevation, ship.key, dome.height);
            checked += 1;
        }
    }
    assert!(checked >= 5, "only {checked} pairs checked");
}

/// A gunship that hovers over a destroyer under an escort corvette's dome, as gunships hover over
/// what they attack, fires down into the dome, which takes it all, and not into the destroyer: the
/// dome once rose to its full radius, over the gunship, which fired from inside it.
#[test]
fn a_gunship_over_a_corvettes_dome_fires_into_the_dome() {
    let mut world = lake_world();
    let corvette = spawn(&mut world, "t2_escort_corvette", Team::Blue, point(2_000, 1_000));
    let destroyer = spawn(&mut world, "t2_destroyer", Team::Blue, point(2_080, 1_000));
    // The fleet holds its anti-air, so that the gunship lives to keep firing.
    for id in [corvette, destroyer] {
        let entity = world.unit_entity(id).unwrap();
        if let Ok(mut extra) = world.ecs.get::<&mut sim_engine::sim::components::ExtraWeapons>(entity) {
            extra.0.clear();
        }
        world.ecs.get::<&mut sim_engine::sim::components::Combat>(entity).unwrap().enabled = false;
    }
    let gunship = spawn(&mut world, "t2_gunship", Team::Red, point(1_700, 1_000));
    world.advance_tick(&[SimCommand::Attack { units: vec![gunship], target: destroyer }]);
    let full = world.unit_health(destroyer).unwrap();
    let dome = |world: &SimWorld| world.ecs.get::<&sim_engine::sim::components::Shield>(world.unit_entity(corvette).unwrap()).unwrap().health;
    let before = dome(&world);
    run_ticks(&mut world, 300);
    assert!(dome(&world) < before, "the gunship's fire should strike the dome");
    assert_eq!(world.unit_health(destroyer), Some(full), "and none of it reach the destroyer");
}

/// A destroyer's torpedoes go into the water at once, as a submarine's do, not from its guns'
/// height: from the deck they flew to the water as shells, and were drawn so (2026-11-25).
#[test]
fn a_destroyers_torpedoes_start_under_the_water() {
    let mut world = lake_world();
    let destroyer = spawn(&mut world, "t2_destroyer", Team::Blue, point(2_000, 1_000));
    let sub = spawn(&mut world, "t1_attack_sub", Team::Red, point(2_700, 1_000));
    world.ecs.get::<&mut sim_engine::sim::components::Combat>(world.unit_entity(sub).unwrap()).unwrap().enabled = false;
    let water = world.terrain.water_level();
    let mut launched = 0;
    for _ in 0..200 {
        world.advance_tick(&[]);
        for &(_, entity) in &world.projectiles {
            let torpedo = world.ecs.get::<&sim_engine::sim::components::Homing>(entity).is_ok_and(|homing| homing.underwater);
            let ours = world.ecs.get::<&sim_engine::sim::components::Projectile>(entity).is_ok_and(|projectile| projectile.source == destroyer);
            if torpedo && ours {
                let z = world.ecs.get::<&sim_engine::sim::components::Position>(entity).unwrap().0.z;
                assert!(z < water, "a torpedo at {} above the water at {}", z.to_num::<f64>(), water.to_num::<f64>());
                launched += 1;
            }
        }
    }
    assert!(launched > 0, "the destroyer fired no torpedoes, so nothing was tested");
}

/// An anti-torpedo charge drops a decoy into the water, which an incoming torpedo turns to and
/// strikes instead of the ship (2026-11-25): a destroyer, whose charge is ready every 10 seconds,
/// takes nothing from a submarine's first torpedo, and a decoy shows in the water; with its charge
/// off, the torpedo hits it.
#[test]
fn an_anti_torpedo_charge_lures_torpedoes_onto_decoys() {
    for charged in [true, false] {
        let mut world = lake_world();
        let destroyer = spawn(&mut world, "t2_destroyer", Team::Blue, point(2_000, 1_000));
        let entity = world.unit_entity(destroyer).unwrap();
        world.ecs.get::<&mut sim_engine::sim::components::Combat>(entity).unwrap().enabled = false;
        world.ecs.get::<&mut sim_engine::sim::components::ExtraWeapons>(entity).unwrap().0.clear();
        if !charged {
            world.ecs.get::<&mut sim_engine::sim::components::TorpedoDefense>(entity).unwrap().range = Fp::ZERO;
        }
        let sub = spawn(&mut world, "t1_attack_sub", Team::Red, point(2_350, 1_000));
        let full = world.unit_health(destroyer).unwrap();
        let (mut decoys_seen, mut fired, mut landed) = (false, false, false);
        // Until the first torpedo has struck something: the charge is ready for it, not the next.
        for _ in 0..200 {
            world.advance_tick(&[]);
            decoys_seen |= !world.snapshot().decoys.is_empty();
            let in_flight = world.snapshot().projectiles.iter().any(|projectile| projectile.source == sub);
            fired |= in_flight;
            if fired && !in_flight {
                landed = true;
                break;
            }
        }
        assert!(landed, "the submarine's first torpedo never landed");
        let health = world.unit_health(destroyer).unwrap();
        if charged {
            assert!(decoys_seen, "no decoy was dropped");
            assert_eq!(health, full, "a torpedo reached the destroyer past its charge");
        } else {
            assert!(!decoys_seen, "a decoy with the charge off");
            assert!(health < full, "with its charge off, the torpedoes should hit the destroyer");
        }
    }
}

/// A torpedo leaves at about a ship's speed, a third of its top speed, gathers speed to it over two
/// seconds, as the inspiration's do, and steers all the way (2026-11-25): it catches a frigate, the
/// fastest ship, sailing straight away at full speed from the edge of a submarine's range.
#[test]
fn a_torpedo_speeds_up_after_launch_and_runs_down_a_fleeing_frigate() {
    let mut world = lake_world();
    let sub = spawn(&mut world, "t1_attack_sub", Team::Red, point(1_800, 600));
    let frigate = spawn(&mut world, "t1_frigate", Team::Blue, point(1_800, 900));
    for id in [sub, frigate] {
        world.ecs.get::<&mut sim_engine::sim::components::Combat>(world.unit_entity(id).unwrap()).unwrap().enabled = false;
    }
    let def = world.blueprints.get(world.blueprints.id("t1_attack_sub"));
    let (top, range) = (def.weapon.unwrap().projectile_speed.to_num::<f64>(), def.weapon.unwrap().range.to_num::<f64>());
    world.advance_tick(&[SimCommand::Move { units: vec![frigate], goal: point(1_800, 4_900) }]);
    // At full speed, and just inside the submarine's range, before it may fire.
    let gap = |world: &SimWorld| world.unit_position(frigate).unwrap().xy().distance_squared(&world.unit_position(sub).unwrap().xy()).to_num::<f64>().sqrt();
    while gap(&world) < range - 40.0 {
        world.advance_tick(&[]);
    }
    world.ecs.get::<&mut sim_engine::sim::components::Combat>(world.unit_entity(sub).unwrap()).unwrap().enabled = true;
    let full = world.unit_health(frigate).unwrap();
    let mut first: Option<(SimId, Vec<f64>, (f64, f64))> = None;
    for _ in 0..200 {
        world.advance_tick(&[]);
        if let Some((id, speeds, last)) = &mut first {
            if let Some(&(_, entity)) = world.projectiles.iter().find(|(other, _)| other == id) {
                let at = world.ecs.get::<&sim_engine::sim::components::Position>(entity).unwrap().0;
                let (x, y) = (at.x.to_num::<f64>(), at.y.to_num::<f64>());
                speeds.push(((x - last.0).powi(2) + (y - last.1).powi(2)).sqrt());
                *last = (x, y);
            }
        } else if let Some(&(id, entity)) = world.projectiles.iter().find(|&&(_, entity)| world.ecs.get::<&sim_engine::sim::components::Homing>(entity).is_ok_and(|homing| homing.underwater)) {
            let at = world.ecs.get::<&sim_engine::sim::components::Position>(entity).unwrap().0;
            first = Some((id, Vec::new(), (at.x.to_num::<f64>(), at.y.to_num::<f64>())));
        }
    }
    let (_, speeds, _) = first.expect("the submarine fired no torpedo");
    assert!(speeds.len() > 22, "the torpedo ran only {} ticks", speeds.len());
    assert!(speeds[0] < top * 0.6, "it left at {:.1}, near its top speed {top}", speeds[0]);
    assert!(speeds[10] < top - 1.0, "a second on, it already ran at {:.1}, its top speed: {speeds:.1?}", speeds[10]);
    assert!((speeds[20] - top).abs() < 0.5, "two seconds on, it ran at {:.1}, not {top}: {speeds:.1?}", speeds[20]);
    assert!(world.unit_health(frigate).unwrap() < full, "the torpedoes never caught the frigate");
}

/// A torpedo bomber's torpedo homes from the bomber straight down onto its ship, closing on it every
/// tick, and does not fall into the water to run in from there (the user's call, 2026-11-25: a
/// fall from the bomber's height carried it past the ship).
#[test]
fn a_torpedo_bombers_torpedo_comes_down_on_its_ship() {
    let mut world = lake_world();
    let destroyer = spawn(&mut world, "t2_destroyer", Team::Red, point(2_000, 1_000));
    let entity = world.unit_entity(destroyer).unwrap();
    world.ecs.get::<&mut sim_engine::sim::components::ExtraWeapons>(entity).unwrap().0.clear();
    world.ecs.get::<&mut sim_engine::sim::components::TorpedoDefense>(entity).unwrap().range = Fp::ZERO;
    let bomber = spawn(&mut world, "t2_torpedo_bomber", Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[SimCommand::Attack { units: vec![bomber], target: destroyer }]);
    let full = world.unit_health(destroyer).unwrap();
    let mut first: Option<SimId> = None;
    let mut gaps: Vec<f64> = Vec::new();
    for _ in 0..600 {
        world.advance_tick(&[]);
        if first.is_none() {
            first = world.projectiles.iter().find(|&&(_, entity)| world.ecs.get::<&sim_engine::sim::components::Homing>(entity).is_ok_and(|homing| homing.underwater)).map(|&(id, _)| id);
        }
        let Some(id) = first else { continue };
        let Some(&(_, entity)) = world.projectiles.iter().find(|(other, _)| *other == id) else { break };
        let at = world.ecs.get::<&sim_engine::sim::components::Position>(entity).unwrap().0;
        let ship = world.unit_position(destroyer).unwrap();
        gaps.push(at.distance_squared(&ship).to_num::<f64>().sqrt());
    }
    assert!(gaps.len() > 2, "no torpedo was dropped");
    assert!(gaps.windows(2).all(|pair| pair[1] < pair[0]), "it did not close on the ship every tick: {gaps:.0?}");
    assert!(world.unit_health(destroyer).unwrap() < full, "the torpedo never hit");
}
