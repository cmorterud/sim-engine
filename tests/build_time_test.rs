//! Build times against the unit data: everything is built at its build power's rate, its build
//! time in seconds at one point of build power, so ten times the build time over the power, in
//! ticks (GAME_SPECIFICATION.md §5.1).

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::{Blueprint, Res};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Builder, Construction, Factory, SimId, Team};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::terrain::{Terrain, HEIGHT_FRAC_BITS};
use sim_engine::sim::world::SimWorld;

const MAP: i32 = 10_240;
/// The lake, for naval factories: x from this on.
const LAKE_X: i32 = 6_000;

/// Flat land, with a lake on the east side, and more materiel and energy than anything costs.
fn world() -> SimWorld {
    let cells = (MAP / 20) as usize;
    let samples = cells + 1;
    let height = |x: usize, _y: usize| if x as i32 * 20 >= LAKE_X { -40 } else { 20 };
    let heights: Vec<i16> = (0..samples * samples).map(|index| (height(index % samples, index / samples) << HEIGHT_FRAC_BITS) as i16).collect();
    let blocked: Vec<bool> = (0..cells * cells)
        .map(|cell| {
            let (x, y) = (cell % cells, cell / cells);
            [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)].iter().any(|&(cx, cy)| height(cx, cy) < 0)
        })
        .collect();
    let terrain = Terrain::from_heights(MAP, heights, &blocked, Fp::ZERO);
    let config = SimConfig { map_units: MAP, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::with_terrain(config, 1, terrain);
    world.prepare_map();
    for economy in &mut world.economies {
        for value in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.base_energy_capacity, &mut economy.energy_capacity] {
            *value = Res::from_num(100_000_000);
        }
        economy.mass = Res::from_num(50_000_000);
        economy.energy = Res::from_num(50_000_000);
    }
    world
}

/// A support factory is built, and builds, only while its team has a headquarters of its kind and
/// tier: puts one down, out of the way, for one.
fn headquarters_for(world: &mut SimWorld, key: &str, on_water: bool) {
    if let Some(headquarters) = key.strip_suffix("_support") {
        let blueprint = world.blueprints.id(headquarters);
        let cell = if on_water { (LAKE_X as u32 / 20 + 120, 300) } else { (200, 300) };
        world.spawn_finished_structure(blueprint, Team::Blue, cell.0, cell.1).unwrap_or_else(|| panic!("no room for {headquarters}"));
    }
}

/// Missiles a silo has built and holds.
fn stock(world: &SimWorld, id: SimId) -> u32 {
    world.unit_entity(id).and_then(|entity| world.ecs.get::<&sim_engine::sim::components::Launcher>(entity).ok().map(|launcher| launcher.stock)).unwrap_or(0)
}

/// Progress a tick at this power, a share of the whole.
fn rate(def: &Blueprint, power: Fp) -> f64 {
    power.to_num::<f64>() / (def.build_time.to_num::<f64>() * 10.0)
}

/// Every factory builds every unit it offers at its build power's rate, and those quick enough to
/// wait for come out when that says.
#[test]
fn every_factory_builds_each_unit_at_its_rate() {
    let mut failures = Vec::new();
    let mut checked = 0;
    let db = world().blueprints;
    for factory_def in db.iter().filter(|def| def.is_factory() && def.is_structure()) {
        for &item in &factory_def.builds {
            let item_def = db.get(item).clone();
            if item_def.is_structure() {
                continue;
            }
            checked += 1;
            let mut world = world();
            headquarters_for(&mut world, &factory_def.key, factory_def.on_water);
            let cell: (u32, u32) = if factory_def.on_water { (LAKE_X as u32 / 20 + 40, 200) } else { (100, 200) };
            let Some(factory) = world.spawn_finished_structure(world.blueprints.id(&factory_def.key), Team::Blue, cell.0, cell.1) else {
                failures.push(format!("{}: could not be placed", factory_def.key));
                continue;
            };
            let entity = world.unit_entity(factory).unwrap();
            let power = world.ecs.get::<&Factory>(entity).unwrap().power;
            // A unit comes out, or a missile goes into a silo's stock.
            let made = |world: &SimWorld| (world.units.len(), stock(world, factory));
            let before = made(&world);
            world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: item, count: 1 }]);
            let name = format!("{} building {}", factory_def.key, item_def.key);
            // Starts, then goes at its rate.
            let started = (0..20).find(|_| {
                world.advance_tick(&[]);
                world.ecs.get::<&Factory>(entity).is_ok_and(|factory| factory.progress > Res::ZERO)
            });
            if started.is_none() {
                failures.push(format!("{name}: never started"));
                continue;
            }
            let expected_ticks = 1.0 / rate(&item_def, power);
            if expected_ticks <= 600.0 {
                let done = (0..(expected_ticks as u32 + 30)).find(|_| {
                    world.advance_tick(&[]);
                    let now = made(&world);
                    now.0 > before.0 || now.1 > before.1
                });
                match done {
                    None => failures.push(format!("{name}: not out after {:.0} ticks", expected_ticks + 30.0)),
                    Some(ticks) if (f64::from(ticks + 2) - expected_ticks).abs() > expected_ticks * 0.01 + 3.0 => {
                        failures.push(format!("{name}: out after about {} ticks, expected {expected_ticks:.0}", ticks + 2));
                    }
                    _ => {}
                }
            } else {
                let start = world.ecs.get::<&Factory>(entity).unwrap().progress;
                for _ in 0..100 {
                    world.advance_tick(&[]);
                }
                let gained = (world.ecs.get::<&Factory>(entity).unwrap().progress - start).to_num::<f64>() / 100.0;
                let expected = rate(&item_def, power);
                if (gained - expected).abs() > expected * 0.01 {
                    failures.push(format!("{name}: {gained:.6} a tick, expected {expected:.6}"));
                }
            }
        }
    }
    assert!(checked >= 40, "only {checked} factory items were checked");
    assert!(failures.is_empty(), "{} failures:\n{failures:#?}", failures.len());
}

/// Every structure is built at its builder's rate: each by the first builder that can build it,
/// alone, and the quick ones finish when that says.
#[test]
fn every_structure_is_built_at_its_builders_rate() {
    let mut failures = Vec::new();
    let mut checked = 0;
    let db = world().blueprints;
    let builders: Vec<Blueprint> = db.iter().filter(|def| !def.is_structure() && def.build_power > Fp::ZERO && !def.builds.is_empty()).cloned().collect();
    for structure in db.iter().filter(|def| def.is_structure()) {
        let Some(builder_def) = builders.iter().find(|builder| builder.builds.contains(&db.id(&structure.key))) else {
            continue;
        };
        checked += 1;
        let name = format!("{} building {}", builder_def.key, structure.key);
        let mut world = world();
        headquarters_for(&mut world, &structure.key, structure.on_water);
        let (cell_x, cell_y, at): (u32, u32, Vec2Fp) = if structure.on_water { (LAKE_X as u32 / 20 + 30, 200, Vec2Fp::new(fp_int(LAKE_X - 100), fp_int(4_000))) } else { (100, 200, Vec2Fp::new(fp_int(1_800), fp_int(4_000))) };
        let builder = world.spawn_mobile(world.blueprints.id(&builder_def.key), Team::Blue, at);
        // Commanders and field rigs reach the higher tiers through enhancements.
        for enhancement in ["t2_fabrication", "t3_fabrication"] {
            if let Some(id) = world.blueprints.find_enhancement(enhancement) {
                if world.blueprints.enhancement(id).units.contains(&world.blueprints.id(&builder_def.key)) {
                    world.install_enhancement(builder, id);
                }
            }
        }
        if structure.requires_deposit {
            world.deposits.push((cell_x, cell_y));
        }
        if structure.requires_vent {
            let half = u32::from(structure.footprint) / 2;
            world.vents.push((cell_x + half, cell_y + half));
        }
        let power = world.ecs.get::<&Builder>(world.unit_entity(builder).unwrap()).unwrap().power;
        world.advance_tick(&[SimCommand::Build { units: vec![builder], blueprint: world.blueprints.id(&structure.key), cell_x, cell_y, queued: false }]);
        // Walks there and starts it.
        let site = |world: &SimWorld| -> Option<SimId> {
            world.ecs.get::<&Builder>(world.unit_entity(builder).unwrap()).ok().and_then(|builder| match builder.tasks.first() {
                Some(sim_engine::sim::components::BuildTask::Build { site, .. }) => *site,
                _ => None,
            })
        };
        let progress = |world: &SimWorld, site: SimId| world.unit_entity(site).and_then(|entity| world.ecs.get::<&Construction>(entity).ok().map(|construction| construction.progress));
        let mut started = None;
        for _ in 0..1_500 {
            world.advance_tick(&[]);
            if let Some(found) = site(&world).filter(|&found| progress(&world, found).is_some_and(|done| done > Res::ZERO)) {
                started = Some(found);
                break;
            }
        }
        let Some(found) = started else {
            failures.push(format!("{name}: never started"));
            continue;
        };
        let expected = rate(structure, power);
        let start = progress(&world, found).unwrap();
        let ticks = 50;
        for _ in 0..ticks {
            world.advance_tick(&[]);
        }
        match progress(&world, found) {
            Some(now) => {
                let gained = (now - start).to_num::<f64>() / f64::from(ticks);
                if (gained - expected).abs() > expected * 0.01 {
                    failures.push(format!("{name}: {gained:.6} a tick, expected {expected:.6} at {power} power"));
                }
            }
            // Finished within the 50 ticks: it should have been that quick.
            None => {
                if expected * f64::from(ticks) < 0.99 - start.to_num::<f64>() {
                    failures.push(format!("{name}: finished sooner than {expected:.6} a tick allows"));
                }
            }
        }
    }
    assert!(checked >= 35, "only {checked} structures were checked");
    assert!(failures.is_empty(), "{} failures:\n{failures:#?}", failures.len());
}

/// Every enhancement is fitted at its unit's build power's rate, from its own build time.
#[test]
fn every_enhancement_is_fitted_at_its_rate() {
    use sim_engine::sim::components::Enhancing;
    let mut failures = Vec::new();
    let mut checked = 0;
    let db = world().blueprints;
    for def in db.iter() {
        let unit_id = db.id(&def.key);
        for enhancement in db.enhancements_for(unit_id) {
            checked += 1;
            let name = format!("{} fitting {}", def.key, enhancement.key);
            let mut world = world();
            let unit = world.spawn_mobile(unit_id, Team::Blue, Vec2Fp::new(fp_int(2_000), fp_int(4_000)));
            // What it needs fitted first.
            let mut chain = Vec::new();
            let mut needs = enhancement.requires;
            while let Some(earlier) = needs {
                chain.push(earlier);
                needs = world.blueprints.enhancement(earlier).requires;
            }
            for &earlier in chain.iter().rev() {
                world.install_enhancement(unit, earlier);
            }
            let entity = world.unit_entity(unit).unwrap();
            let power = world.ecs.get::<&Builder>(entity).map_or(def.build_power, |builder| builder.power);
            world.advance_tick(&[SimCommand::Enhance { units: vec![unit], enhancement: enhancement.id }]);
            let progress = |world: &SimWorld| world.ecs.get::<&Enhancing>(entity).ok().map(|enhancing| enhancing.progress);
            let Some(start) = (0..20).find_map(|_| {
                world.advance_tick(&[]);
                progress(&world).filter(|done| *done > Res::ZERO)
            }) else {
                failures.push(format!("{name}: never started"));
                continue;
            };
            let ticks = 50;
            for _ in 0..ticks {
                world.advance_tick(&[]);
            }
            let expected = power.to_num::<f64>() / (enhancement.build_time.to_num::<f64>() * 10.0);
            match progress(&world) {
                Some(now) => {
                    let gained = (now - start).to_num::<f64>() / f64::from(ticks);
                    if (gained - expected).abs() > expected * 0.01 {
                        failures.push(format!("{name}: {gained:.6} a tick, expected {expected:.6} at {power} power"));
                    }
                }
                None if expected * f64::from(ticks) < 0.99 - start.to_num::<f64>() => failures.push(format!("{name}: done sooner than its rate allows")),
                None => {}
            }
        }
    }
    assert!(checked >= 15, "only {checked} enhancements were checked");
    assert!(failures.is_empty(), "{} failures:\n{failures:#?}", failures.len());
}

/// Every structure that upgrades in place, extractors, radars, generators, and factories, does so
/// at its own build power's rate, from what it becomes's build time.
#[test]
fn every_structure_upgrades_at_its_rate() {
    use sim_engine::sim::components::Upgrading;
    let mut failures = Vec::new();
    let mut checked = 0;
    let db = world().blueprints;
    for def in db.iter().filter(|def| def.is_structure()) {
        let Some(into) = def.upgrades_to else { continue };
        checked += 1;
        let into_def = db.get(into).clone();
        let name = format!("{} upgrading to {}", def.key, into_def.key);
        let mut world = world();
        // A support factory goes up a tier only with a headquarters of the tier it goes to.
        headquarters_for(&mut world, &into_def.key, def.on_water);
        let cell: (u32, u32) = if def.on_water { (LAKE_X as u32 / 20 + 40, 200) } else { (100, 200) };
        if def.requires_deposit {
            world.deposits.push(cell);
        }
        let Some(id) = world.spawn_finished_structure(world.blueprints.id(&def.key), Team::Blue, cell.0, cell.1) else {
            failures.push(format!("{name}: could not be placed"));
            continue;
        };
        let entity = world.unit_entity(id).unwrap();
        // A factory's upgrade is an item in its queue; the rest are told to upgrade.
        let is_factory = world.ecs.get::<&Factory>(entity).is_ok();
        let command = if is_factory { SimCommand::QueueUnit { factory: id, blueprint: into, count: 1 } } else { SimCommand::Upgrade { units: vec![id] } };
        let power = if is_factory { world.ecs.get::<&Factory>(entity).unwrap().power } else { def.build_power };
        world.advance_tick(&[command]);
        let progress = |world: &SimWorld| {
            if is_factory {
                world.ecs.get::<&Factory>(entity).ok().map(|factory| factory.progress).filter(|_| world.ecs.get::<&sim_engine::sim::components::Blueprinted>(entity).is_ok_and(|unit| unit.0 != into))
            } else {
                world.ecs.get::<&Upgrading>(entity).ok().map(|upgrading| upgrading.progress)
            }
        };
        let Some(start) = (0..20).find_map(|_| {
            world.advance_tick(&[]);
            progress(&world).filter(|done| *done > Res::ZERO)
        }) else {
            failures.push(format!("{name}: never started"));
            continue;
        };
        let ticks = 50;
        for _ in 0..ticks {
            world.advance_tick(&[]);
        }
        let expected = power.to_num::<f64>() / (into_def.build_time.to_num::<f64>() * 10.0);
        match progress(&world) {
            Some(now) => {
                let gained = (now - start).to_num::<f64>() / f64::from(ticks);
                if (gained - expected).abs() > expected * 0.01 {
                    failures.push(format!("{name}: {gained:.6} a tick, expected {expected:.6} at {power} power"));
                }
            }
            None if expected * f64::from(ticks) < 0.99 - start.to_num::<f64>() => failures.push(format!("{name}: done sooner than its rate allows")),
            None => {}
        }
    }
    assert!(checked >= 8, "only {checked} upgrades were checked");
    assert!(failures.is_empty(), "{} failures:\n{failures:#?}", failures.len());
}
