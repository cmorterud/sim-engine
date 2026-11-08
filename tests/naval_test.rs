//! Ships (GAME_SPECIFICATION.md §10): they sail deep water only, with routes of their own, and
//! are built by naval factories on the water.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Blueprinted, OrderKind, SimId, Team};
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
    let config = SimConfig { map_units: MAP, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
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
    assert!(intercepted > 0, "the Phalanx should stop some of the missiles");
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
