//! Income and death blasts against the unit data: every structure that makes materiel or energy
//! adds what it says to its team's income, and everything that blows up when destroyed hurts
//! what is within its blast, and nothing beyond it (GAME_SPECIFICATION.md §3, §6).

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::{Blueprint, Res};
use sim_engine::sim::components::{Health, SimId, Team};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::world::SimWorld;

fn world() -> SimWorld {
    let config = SimConfig { map_units: 10_240, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    for economy in &mut world.economies {
        for value in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.base_energy_capacity, &mut economy.energy_capacity] {
            *value = Res::from_num(10_000_000);
        }
        economy.mass = Res::from_num(5_000_000);
        economy.energy = Res::from_num(5_000_000);
    }
    world
}

/// A finished structure, standing on a deposit or vent if it needs one.
fn place(world: &mut SimWorld, def: &Blueprint, team: Team, cell: (u32, u32)) -> SimId {
    if def.requires_deposit {
        world.deposits.push(cell);
    }
    if def.requires_vent {
        let half = u32::from(def.footprint) / 2;
        world.vents.push((cell.0 + half, cell.1 + half));
    }
    world.spawn_finished_structure(world.blueprints.id(&def.key), team, cell.0, cell.1).unwrap_or_else(|| panic!("no room for {}", def.key))
}

#[test]
fn every_structure_adds_its_stated_income() {
    let db = world().blueprints;
    let mut failures = Vec::new();
    let mut checked = 0;
    for def in db.iter().filter(|def| def.is_structure() && (def.mass_income > Res::ZERO || def.energy_income > Res::ZERO)) {
        checked += 1;
        let mut world = world();
        let before = (world.economies[0].mass_income, world.economies[0].energy_income);
        place(&mut world, def, Team::Blue, (200, 200));
        for _ in 0..5 {
            world.advance_tick(&[]);
        }
        let economy = &world.economies[0];
        let gained = (economy.mass_income - before.0, economy.energy_income - before.1);
        if gained != (def.mass_income, def.energy_income) {
            failures.push(format!("{}: income +{} materiel and +{} energy a tick, stated {} and {}", def.key, gained.0, gained.1, def.mass_income, def.energy_income));
        }
    }
    assert!(checked >= 8, "only {checked} were checked");
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn everything_that_blows_up_hurts_within_its_blast_and_not_beyond() {
    let db = world().blueprints;
    let mut failures = Vec::new();
    let mut checked = 0;
    for def in db.iter().filter(|def| def.death_explosion.is_some()) {
        let blast = def.death_explosion.unwrap();
        checked += 1;
        let mut world = world();
        let center = Vec2Fp::new(fp_int(4_000), fp_int(4_000));
        let doomed = if def.is_structure() {
            let half = u32::from(def.footprint) / 2;
            place(&mut world, def, Team::Blue, (200 - half, 200 - half))
        } else {
            world.spawn_mobile(world.blueprints.id(&def.key), Team::Blue, center)
        };
        let at = world.unit_position(doomed).unwrap().xy();
        // Enemies just inside and just outside the blast, to the north and south, out of the way.
        let probe_radius = db.get(db.id("t1_engineer")).radius;
        let inside_at = Vec2Fp::new(at.x, at.y + blast.radius + probe_radius - fp_int(4));
        let outside_at = Vec2Fp::new(at.x, at.y - blast.radius - probe_radius - fp_int(4));
        let probe = |world: &mut SimWorld, at: Vec2Fp| {
            let id = world.spawn_mobile(world.blueprints.id("t1_engineer"), Team::Red, at);
            let entity = world.unit_entity(id).unwrap();
            let mut health = world.ecs.get::<&mut Health>(entity).unwrap();
            health.max = fp_int(400_000);
            health.current = health.max;
            id
        };
        let (inside, outside) = (probe(&mut world, inside_at), probe(&mut world, outside_at));
        // It is destroyed while running.
        world.advance_tick(&[]);
        world.advance_tick(&[sim_engine::sim::commands::SimCommand::SelfDestruct { units: vec![doomed] }]);
        let mut shown = Vec::new();
        for _ in 0..80 {
            world.advance_tick(&[]);
            shown.extend(world.snapshot().blasts);
            if world.unit_position(doomed).is_none() {
                break;
            }
        }
        if world.unit_position(doomed).is_some() {
            failures.push(format!("{}: could not be destroyed", def.key));
            continue;
        }
        // And the renderer is told, so that it draws an explosion as wide as the blast.
        if !shown.iter().any(|view| view.radius == blast.radius.to_bits() && (view.x, view.y) == (at.x.to_bits(), at.y.to_bits())) {
            failures.push(format!("{}: its blast was not in the snapshot: {shown:?}", def.key));
        }
        for _ in 0..3 {
            world.advance_tick(&[]);
        }
        let taken = |world: &SimWorld, id: SimId| fp_int(400_000) - world.unit_health(id).unwrap_or(Fp::ZERO);
        let (near, far) = (taken(&world, inside), taken(&world, outside));
        if near < blast.damage {
            failures.push(format!("{}: a unit just inside its {} blast took {near}, not {}", def.key, blast.radius, blast.damage));
        }
        if far > Fp::ZERO {
            failures.push(format!("{}: a unit just outside its {} blast took {far}", def.key, blast.radius));
        }
    }
    assert!(checked >= 5, "only {checked} were checked");
    assert!(failures.is_empty(), "{failures:#?}");
}
