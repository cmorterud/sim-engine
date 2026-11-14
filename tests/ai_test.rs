//! The scripted opponent: full matches, replays, and playing by the player's rules.

use sim_engine::ai::{advance_ai_vs_ai, play_match, replay_match, AiBrain, Difficulty};
use sim_engine::sim::commands::SimCommand;
use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::components::{Blueprinted, Position, Team, UnitStats};
use sim_engine::sim::scenario;
use sim_engine::sim::world::SimWorld;

const MATCH_LIMIT: u32 = 40_000;

fn count_owned(world: &SimWorld, team: Team, key: &str) -> usize {
    let wanted = world.blueprints.id(key);
    world
        .units
        .iter()
        .filter(|(_, entity)| {
            world.ecs.get::<&UnitStats>(*entity).is_ok_and(|stats| stats.team == team)
                && world.ecs.get::<&Blueprinted>(*entity).is_ok_and(|unit| unit.0 == wanted)
        })
        .count()
}

#[test]
fn ai_builds_an_economy_and_an_army() {
    let mut world = scenario::skirmish(11).world;
    let mut brain = AiBrain::new(Team::Blue, 11, world.starts);
    // Five minutes, with nobody playing Red.
    for _ in 0..3_000 {
        let commands = brain.think(&world);
        world.advance_tick(&commands);
    }
    let count = |key: &str| count_owned(&world, Team::Blue, key);
    assert!(count("land_factory_t1") + count("land_factory_t2") >= 1, "no factory");
    assert!(count("mass_extractor") >= 4, "only {} extractors", count("mass_extractor"));
    assert!(count("power_generator") >= 2, "only {} generators", count("power_generator"));
    assert!(count("t1_engineer") >= 2, "only {} engineers", count("t1_engineer"));
    assert!(count("t1_light_bot") + count("t1_light_artillery") + count("t2_heavy_tank") >= 3, "no army");
    let income = (world.economies[0].mass_income * 10).to_num::<i64>();
    assert!(income >= 8, "mass income is only {income}/s");
}

#[test]
fn ai_commands_only_its_own_units() {
    let mut world = scenario::skirmish(12).world;
    let mut brains = [AiBrain::new(Team::Blue, 12, world.starts), AiBrain::new(Team::Red, 12, world.starts)];
    for _ in 0..4_000 {
        for brain in &mut brains {
            let commands = brain.think(&world);
            for command in &commands {
                let units: Vec<_> = match command {
                    SimCommand::Build { units, .. }
                    | SimCommand::Assist { units, .. }
                    | SimCommand::Reclaim { units, .. }
                    | SimCommand::AttackMove { units, .. }
                    | SimCommand::Upgrade { units }
                    | SimCommand::Patrol { units, .. }
                    | SimCommand::SetPaused { units, .. }
                    | SimCommand::QueueMove { units, .. }
                    | SimCommand::FormationMove { units, .. }
                    | SimCommand::Attack { units, .. } => units.clone(),
                    SimCommand::Overcharge { unit, .. } => vec![*unit],
                    SimCommand::QueueUnit { factory, .. } | SimCommand::SetRally { factory, .. } | SimCommand::SetRepeat { factory, .. } => vec![*factory],
                    other => panic!("unexpected command {other:?}"),
                };
                for unit in units {
                    let entity = world.unit_entity(unit).expect("AI commands a living unit");
                    assert_eq!(world.ecs.get::<&UnitStats>(entity).unwrap().team, brain.team());
                }
            }
            world.advance_tick(&commands);
        }
    }
}

#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn ai_matches_finish_and_replay_exactly() {
    for seed in [1, 2, 3] {
        let record = play_match(seed, MATCH_LIMIT, |_| {});
        assert!(record.result.is_some(), "seed {seed} was unfinished after {} ticks", record.ticks);
        assert_eq!(replay_match(seed, &record.log), record.checksums, "seed {seed} did not replay exactly");
    }
}

/// An order with its unit ids left out and its places turned into Blue's side of the map.
fn mirrored_order(world: &SimWorld, team: Team, command: &SimCommand) -> String {
    let map = scenario::SKIRMISH_MAP_UNITS;
    let last = world.terrain.cells_per_side() as i32 - 1;
    let point = |point: Vec2Fp| {
        let (x, y) = (point.x.to_num::<i32>(), point.y.to_num::<i32>());
        if team == Team::Red { (map - x, map - y) } else { (x, y) }
    };
    match command {
        SimCommand::Build { units, blueprint, cell_x, cell_y, .. } => {
            let size = i32::from(world.blueprints.get(*blueprint).footprint);
            let (x, y) = (*cell_x as i32, *cell_y as i32);
            let cell = if team == Team::Red { (last - x - size + 1, last - y - size + 1) } else { (x, y) };
            format!("build {} x{} at {cell:?}", blueprint.0, units.len())
        }
        SimCommand::AttackMove { units, goal } => format!("attack-move x{} to {:?}", units.len(), point(*goal)),
        SimCommand::FormationMove { units, goal, attack, queued } => format!("formation x{} to {:?} {attack} {queued}", units.len(), point(*goal)),
        SimCommand::SetRally { point: rally, .. } => format!("rally {:?}", point(*rally)),
        SimCommand::QueueUnit { blueprint, count, .. } => format!("queue {} x{count}", blueprint.0),
        SimCommand::Assist { units, .. } => format!("assist x{}", units.len()),
        SimCommand::Reclaim { units, .. } => format!("reclaim x{}", units.len()),
        SimCommand::Upgrade { units } => format!("upgrade x{}", units.len()),
        SimCommand::SetPaused { units, paused } => format!("pause {paused} x{}", units.len()),
        SimCommand::Patrol { units, points } => format!("patrol x{} {:?}", units.len(), points.iter().map(|at| point(*at)).collect::<Vec<_>>()),
        SimCommand::QueueMove { units, goal, attack } => format!("queue-move x{} to {:?} {attack}", units.len(), point(*goal)),
        SimCommand::SetRepeat { repeat, .. } => format!("repeat {repeat}"),
        SimCommand::Attack { units, .. } => format!("attack x{}", units.len()),
        SimCommand::Overcharge { .. } => "overcharge".to_string(),
        other => panic!("unexpected command {other:?}"),
    }
}

/// Each unit's blueprint and position in whole units, turned into Blue's side of the map.
fn mirrored_units(world: &SimWorld, team: Team) -> Vec<(u16, i32, i32)> {
    let map = scenario::SKIRMISH_MAP_UNITS;
    let mut units: Vec<_> = world
        .units
        .iter()
        .filter(|(_, entity)| world.ecs.get::<&UnitStats>(*entity).is_ok_and(|stats| stats.team == team))
        .map(|(_, entity)| {
            let mut position = world.ecs.get::<&Position>(*entity).unwrap().0.xy();
            if team == Team::Red {
                position = Vec2Fp::new(fp_int(map) - position.x, fp_int(map) - position.y);
            }
            (world.ecs.get::<&Blueprinted>(*entity).unwrap().0 .0, position.x.to_num::<i32>(), position.y.to_num::<i32>())
        })
        .collect();
    units.sort_unstable();
    units
}

#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn ai_teams_play_mirror_images_until_they_meet() {
    // The map turns into itself about its center, and both AIs think on the same tick. Until
    // the armies meet, Red should play Blue's game turned around. Anything else gives one side
    // a better base, a faster start, or a shorter walk.
    //
    // Fixed-point rounding is not symmetric under negation, so the sides drift apart a little:
    // a unit a fraction of a unit off, or a builder in range a tick sooner.
    const TOLERANCE: i32 = 10;
    let matches = |red: &[(u16, i32, i32)], blue: &[(u16, i32, i32)]| {
        let mut partnered = vec![false; blue.len()];
        red.len() == blue.len()
            && red.iter().all(|unit| {
                let partner = blue.iter().enumerate().position(|(index, other)| {
                    !partnered[index] && other.0 == unit.0 && (other.1 - unit.1).abs() <= TOLERANCE && (other.2 - unit.2).abs() <= TOLERANCE
                });
                partner.inspect(|&index| partnered[index] = true).is_some()
            })
    };
    for seed in [1, 2, 3, 4, 5, 6, 7, 8] {
        let mut world = scenario::skirmish(seed).world;
        let mut brains = [AiBrain::new(Team::Blue, seed, world.starts), AiBrain::new(Team::Red, seed, world.starts)];
        let (mut blue_units, mut red_units) = (Vec::new(), Vec::new());
        // Five minutes, well before the first wave sets out.
        for tick in 0..3_000 {
            let blue = brains[0].think(&world);
            let red = brains[1].think(&world);
            let orders = |team: Team, commands: &[SimCommand]| commands.iter().map(|command| mirrored_order(&world, team, command)).collect::<Vec<_>>();
            assert_eq!(orders(Team::Red, &red), orders(Team::Blue, &blue), "seed {seed}: orders differ at tick {tick}");
            let mut commands = blue;
            commands.extend(red);
            world.advance_tick(&commands);
            blue_units.push(mirrored_units(&world, Team::Blue));
            red_units.push(mirrored_units(&world, Team::Red));
            // Equal-cost paths can bend differently on each side, so walks far from home drift.
            if (2..1_200).contains(&tick) {
                let then = tick - 1;
                let near_in_time = (then - 1..=then + 1).any(|blue| matches(&red_units[then], &blue_units[blue]));
                assert!(near_in_time, "seed {seed}: at tick {then}, Red has {:?}\nwhere Blue has {:?}", red_units[then], blue_units[then]);
            }
        }
    }
}

#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn ai_cannot_see_through_fog() {
    // Red's start is known, but nothing that happens there may change Blue's orders
    // until Blue can see it. Two worlds differ only in a hidden Red structure, far from any Blue sight.
    let run = |extra_structure: bool| {
        let mut world = scenario::skirmish(21).world;
        if extra_structure {
            let generator = world.blueprints.id("power_generator");
            let cells = world.terrain.cells_per_side() as u32;
            world.spawn_finished_structure(generator, Team::Red, cells - 30, cells - 30).unwrap();
        }
        let mut brain = AiBrain::new(Team::Blue, 21, world.starts);
        let mut orders = Vec::new();
        for _ in 0..1_200 {
            let commands = brain.think(&world);
            // Ids shift when anything extra spawns, so compare what was ordered, not which id got it.
            for command in &commands {
                match command {
                    SimCommand::Build { blueprint, cell_x, cell_y, .. } => orders.push((blueprint.0, *cell_x, *cell_y)),
                    SimCommand::QueueUnit { blueprint, .. } => orders.push((blueprint.0, 0, 0)),
                    _ => {}
                }
            }
            world.advance_tick(&commands);
        }
        orders
    };
    assert_eq!(run(false), run(true), "Blue's orders changed because of something it could not see");
}

#[test]
fn late_in_a_long_game_the_commander_joins_a_full_size_wave() {
    let wave_with = |minute: u32| {
        let mut world = scenario::skirmish(41).world;
        let home = scenario::skirmish_start(Team::Blue);
        let tank = world.blueprints.id("t2_heavy_tank");
        for index in 0..50 {
            world.spawn_mobile(tank, Team::Blue, Vec2Fp::new(home.x + fp_int(300 + index % 10 * 20), home.y + fp_int(300 + index / 10 * 20)));
        }
        world.current_tick = minute * 600;
        let orders = AiBrain::new(Team::Blue, 41, world.starts).think(&world);
        let wave = orders.iter().find_map(|command| match command {
            SimCommand::AttackMove { units, .. } => Some(units.clone()),
            _ => None,
        });
        let wave = wave.expect("a full-size wave sets out");
        (wave.len(), wave.contains(&world.commanders[Team::Blue.index()].unwrap()))
    };
    assert_eq!(wave_with(20), (50, false), "the commander stays home early on");
    assert_eq!(wave_with(36), (51, true), "and joins the wave after 35 minutes");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn ai_climbs_the_tech_tree_and_upgrades_its_economy() {
    let mut world = scenario::skirmish(11).world;
    let mut brain = AiBrain::new(Team::Blue, 11, world.starts);
    // 25 minutes, with nobody playing Red, whose commander is kept alive so the match goes on.
    for _ in 0..15_000 {
        let commands = brain.think(&world);
        world.advance_tick(&commands);
        let red = world.unit_entity(world.commanders[Team::Red.index()].unwrap()).unwrap();
        let mut health = world.ecs.get::<&mut sim_engine::sim::components::Health>(red).unwrap();
        health.current = health.max;
    }
    let count = |key: &str| count_owned(&world, Team::Blue, key);
    assert!(count("land_factory_t3") >= 1, "no T3 factory");
    assert!(count("t2_engineer") >= 1 && count("t3_engineer") >= 1, "no higher-tier engineers");
    assert!(count("mass_extractor_t2") + count("mass_extractor_t3") >= 6, "only {} upgraded extractors", count("mass_extractor_t2") + count("mass_extractor_t3"));
    assert!(count("power_generator_t2") + count("power_generator_t3") >= 1, "no higher-tier generators");
    assert!(count("t2_heavy_tank") >= 5 && count("t3_mobile_artillery") >= 1, "the army is not keeping up with the tech");
    let income = (world.economies[0].mass_income * 10).to_num::<i64>();
    assert!(income >= 60, "mass income is only {income}/s");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn ai_takes_to_the_air_and_answers_enemy_aircraft_with_anti_air() {
    let mut world = scenario::skirmish(11).world;
    let mut brain = AiBrain::new(Team::Blue, 11, world.starts);
    let keep_red_alive = |world: &mut SimWorld| {
        let red = world.unit_entity(world.commanders[Team::Red.index()].unwrap()).unwrap();
        let mut health = world.ecs.get::<&mut sim_engine::sim::components::Health>(red).unwrap();
        health.current = health.max;
    };
    for _ in 0..9_000 {
        let commands = brain.think(&world);
        world.advance_tick(&commands);
        keep_red_alive(&mut world);
    }
    let count = |world: &SimWorld, key: &str| count_owned(world, Team::Blue, key);
    let air_factories = count(&world, "air_factory") + count(&world, "air_factory_t2") + count(&world, "air_factory_t3");
    assert!(air_factories >= 1, "no air factory after 15 minutes");
    let aircraft = count(&world, "t1_interceptor") + count(&world, "t1_bomber") + count(&world, "t2_gunship");
    assert!(aircraft >= 3, "only {aircraft} aircraft");
    let anti_air = |world: &SimWorld| ["t1_aa_turret", "t2_flak", "t3_sam", "t1_mobile_aa", "t2_mobile_flak"].iter().map(|key| count(world, key)).sum::<usize>();
    assert_eq!(anti_air(&world), 0, "no anti-air before any enemy aircraft are seen");
    // Red bombers circle over Blue's base, where Blue sees them.
    let home = scenario::skirmish_start(Team::Blue);
    let bomber = world.blueprints.id("t1_bomber");
    for index in 0..3 {
        let raider = world.spawn_mobile(bomber, Team::Red, Vec2Fp::new(home.x + fp_int(200 + index * 40), home.y + fp_int(200)));
        world.advance_tick(&[SimCommand::Patrol { units: vec![raider], points: vec![Vec2Fp::new(home.x + fp_int(200), home.y + fp_int(200)), Vec2Fp::new(home.x + fp_int(400), home.y + fp_int(300))] }]);
    }
    for _ in 0..3_000 {
        let commands = brain.think(&world);
        world.advance_tick(&commands);
        keep_red_alive(&mut world);
    }
    assert!(anti_air(&world) >= 2, "only {} anti-air after enemy aircraft were seen", anti_air(&world));
}

#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn ai_puts_up_radar_shields_and_missiles_and_overcharges_what_comes_close() {
    let mut world = scenario::skirmish(11).world;
    let mut brain = AiBrain::new(Team::Blue, 11, world.starts);
    let keep_red_alive = |world: &mut SimWorld| {
        let red = world.unit_entity(world.commanders[Team::Red.index()].unwrap()).unwrap();
        let mut health = world.ecs.get::<&mut sim_engine::sim::components::Health>(red).unwrap();
        health.current = health.max;
    };
    // 25 minutes with nobody playing Red, as in the tech tree test.
    for _ in 0..15_000 {
        let commands = brain.think(&world);
        world.advance_tick(&commands);
        keep_red_alive(&mut world);
    }
    let count = |world: &SimWorld, key: &str| count_owned(world, Team::Blue, key);
    assert!(count(&world, "radar") + count(&world, "radar_t2") >= 1, "no radar");
    assert!(count(&world, "shield_generator") >= 1, "no shield generator");
    assert!(count(&world, "tactical_missile_defense") >= 1, "no tactical missile defense");
    assert_eq!(count(&world, "tactical_missile_launcher"), 1, "not one tactical missile launcher");

    // A Red generator in sight of Blue's base draws a missile, and a Red tank by the commander
    // an Overcharge.
    let home = scenario::skirmish_start(Team::Blue);
    let home_cell = (home.x.to_num::<i32>() / 20, home.y.to_num::<i32>() / 20);
    let generator = world.blueprints.id("power_generator");
    let target = (0..40)
        .find_map(|step| world.spawn_finished_structure(generator, Team::Red, (home_cell.0 + 25 + step % 8) as u32, (home_cell.1 + 25 + step / 8) as u32))
        .expect("room for a Red generator");
    // Kept standing, like Red's commander, so that the base's other guns do not end it first.
    let keep_standing = |world: &mut SimWorld| {
        if let Some(entity) = world.unit_entity(target) {
            let mut health = world.ecs.get::<&mut sim_engine::sim::components::Health>(entity).unwrap();
            health.current = health.max;
        }
    };
    let commander = world.commanders[Team::Blue.index()].unwrap();
    let at = world.unit_position(commander).unwrap().xy();
    let tank = world.spawn_mobile(world.blueprints.id("t2_heavy_tank"), Team::Red, Vec2Fp::new(at.x + fp_int(300), at.y));
    let (mut fired_at, mut overcharged) = (false, false);
    let launcher = world.units.iter().find(|(_, entity)| {
        world.ecs.get::<&Blueprinted>(*entity).is_ok_and(|unit| unit.0 == world.blueprints.id("tactical_missile_launcher"))
    }).map(|&(id, _)| id).unwrap();
    for _ in 0..600 {
        let commands = brain.think(&world);
        overcharged |= commands.iter().any(|command| matches!(command, SimCommand::Overcharge { target, .. } if *target == tank));
        fired_at |= commands.iter().any(|command| matches!(command, SimCommand::Attack { units, target: aimed } if units == &vec![launcher] && *aimed == target));
        world.advance_tick(&commands);
        keep_red_alive(&mut world);
        keep_standing(&mut world);
    }
    assert!(fired_at, "the missile launcher never fired at the Red generator");
    assert!(overcharged, "the commander never overcharged the Red tank");
}

#[test]
fn the_commander_never_overcharges_the_enemy_commander() {
    // The shot would kill it, and its reactor's blast would take Blue's commander too: a draw.
    let mut world = scenario::skirmish(5).world;
    let mut brain = AiBrain::new(Team::Blue, 5, world.starts);
    let (blue, red) = (world.commanders[0].unwrap(), world.commanders[1].unwrap());
    let at = world.unit_position(blue).unwrap().xy();
    let red_entity = world.unit_entity(red).unwrap();
    world.ecs.get::<&mut sim_engine::sim::components::Position>(red_entity).unwrap().0 = world.unit_position(blue).unwrap() + sim_engine::math::fp::Vec3Fp::new(fp_int(350), Fp::ZERO, Fp::ZERO);
    let _ = at;
    let mut offered_something_else = false;
    for tick in 0..300 {
        let economy = &mut world.economies[0];
        economy.energy_capacity = economy.energy_capacity.max(sim_engine::sim::blueprints::Res::from_num(10_000));
        economy.energy = economy.energy_capacity;
        let commands = brain.think(&world);
        for command in &commands {
            if let SimCommand::Overcharge { target, .. } = command {
                assert_ne!(*target, red, "overcharged the enemy commander at tick {tick}");
            }
        }
        // With Red's commander gone home, a Red tank beside it is fair game, which shows the
        // commander was free to overcharge all along.
        if tick == 150 {
            let red_entity = world.unit_entity(red).unwrap();
            let start = scenario::skirmish_start(Team::Red);
            world.ecs.get::<&mut sim_engine::sim::components::Position>(red_entity).unwrap().0 = sim_engine::math::fp::Vec3Fp::new(start.x, start.y, Fp::ZERO);
            let tank = world.blueprints.id("t2_heavy_tank");
            world.spawn_mobile(tank, Team::Red, Vec2Fp::new(at.x - fp_int(300), at.y));
        }
        offered_something_else |= tick > 150 && commands.iter().any(|command| matches!(command, SimCommand::Overcharge { .. }));
        world.advance_tick(&commands);
        let health = |id| world.unit_entity(id).map(|entity| { let mut h = world.ecs.get::<&mut sim_engine::sim::components::Health>(entity).unwrap(); h.current = h.max; });
        health(red);
        health(blue);
    }
    assert!(offered_something_else, "the commander never overcharged the tank either");
}

#[test]
fn the_army_is_never_sent_at_an_enemy_commander_under_the_water() {
    // Tanks cannot hit what is on the seabed. Two commanders that followed their waves there
    // once stalked each other until the match ran out of time.
    let map = scenario::MapSource::Imported(std::sync::Arc::new(sim_engine::sim::maps::twin_passes()));
    let mut world = map.scenario(3).world;
    let mut brain = AiBrain::new(Team::Blue, 3, world.starts);
    let home = world.starts[0];
    let water = world.terrain.water_level();
    let cells = world.terrain.cells_per_side() as i32;
    let deep = (0..cells * cells)
        .map(|index| Vec2Fp::new(fp_int((index % cells) * 20 + 10), fp_int((index / cells) * 20 + 10)))
        .filter(|&at| world.terrain.height_at(at) + fp_int(40) < water)
        .min_by_key(|at| at.distance_squared(&home))
        .expect("twin passes has a sea");
    let red = world.commanders[1].unwrap();
    let seabed = sim_engine::math::fp::Vec3Fp::new(deep.x, deep.y, world.terrain.height_at(deep));
    // A Blue frigate over it sees it, and a dozen tanks at home make a wave.
    world.spawn_mobile(world.blueprints.id("t1_frigate"), Team::Blue, deep);
    let tank = world.blueprints.id("t2_heavy_tank");
    for index in 0..12 {
        world.spawn_mobile(tank, Team::Blue, Vec2Fp::new(home.x + fp_int(200 + 40 * (index % 4)), home.y + fp_int(200 + 40 * (index / 4))));
    }
    let place_red = |world: &mut SimWorld| {
        let entity = world.unit_entity(red).unwrap();
        world.ecs.get::<&mut sim_engine::sim::components::Position>(entity).unwrap().0 = seabed;
    };
    // Long enough for the frigate's sight to take in the commander before the AI looks.
    for _ in 0..20 {
        place_red(&mut world);
        world.advance_tick(&[]);
    }
    let mut waves = 0;
    for tick in 0..400 {
        let entity = world.unit_entity(red).unwrap();
        world.ecs.get::<&mut sim_engine::sim::components::Position>(entity).unwrap().0 = seabed;
        let commands = brain.think(&world);
        for command in &commands {
            // The tanks' waves: the frigate is the fleet's, and goes where ships go.
            if let SimCommand::AttackMove { units, goal } = command.clone() {
                if units.len() < 10 {
                    continue;
                }
                waves += 1;
                assert!(goal.distance_squared(&deep) > sim_engine::math::fp::wide_mul(fp_int(300), fp_int(300)), "a wave was sent at the commander on the seabed at tick {tick}");
            }
        }
        world.advance_tick(&commands);
    }
    assert!(waves >= 1, "no wave set out, so nothing was tested");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn on_a_map_with_a_sea_the_ai_builds_a_navy_and_none_on_a_dry_one() {
    let map = scenario::MapSource::Imported(std::sync::Arc::new(sim_engine::sim::maps::twin_passes()));
    let mut world = map.scenario(3).world;
    let mut brain = AiBrain::new(Team::Blue, 3, world.starts);
    // 20 minutes, with nobody playing Red.
    for _ in 0..12_000 {
        let commands = brain.think(&world);
        world.advance_tick(&commands);
        let red = world.unit_entity(world.commanders[Team::Red.index()].unwrap()).unwrap();
        let mut health = world.ecs.get::<&mut sim_engine::sim::components::Health>(red).unwrap();
        health.current = health.max;
    }
    let count = |world: &SimWorld, keys: &[&str]| keys.iter().map(|key| count_owned(world, Team::Blue, key)).sum::<usize>();
    let yards = ["naval_factory", "naval_factory_t2", "naval_factory_t3", "naval_factory_t2_support", "naval_factory_t3_support"];
    let ships = ["t1_frigate", "t1_attack_sub", "t2_destroyer", "t2_cruiser", "t2_shield_boat", "t3_battleship", "t3_battlecruiser"];
    assert!(count(&world, &yards) >= 1, "no naval factory on the sea's shore");
    assert!(count(&world, &ships) >= 2, "only {} ships", count(&world, &ships));

    // The skirmish map has no water, so no shipyard is ever planned there.
    let mut world = scenario::skirmish(3).world;
    let mut brain = AiBrain::new(Team::Blue, 3, world.starts);
    for _ in 0..12_000 {
        let commands = brain.think(&world);
        world.advance_tick(&commands);
        let red = world.unit_entity(world.commanders[Team::Red.index()].unwrap()).unwrap();
        let mut health = world.ecs.get::<&mut sim_engine::sim::components::Health>(red).unwrap();
        health.current = health.max;
    }
    assert_eq!(count(&world, &yards), 0);
}

#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn after_losing_every_generator_the_ai_pauses_its_extractors_and_rebuilds_its_power() {
    let mut world = scenario::skirmish(11).world;
    let mut brain = AiBrain::new(Team::Blue, 11, world.starts);
    let keep_red_alive = |world: &mut SimWorld| {
        let red = world.unit_entity(world.commanders[Team::Red.index()].unwrap()).unwrap();
        let mut health = world.ecs.get::<&mut sim_engine::sim::components::Health>(red).unwrap();
        health.current = health.max;
    };
    for _ in 0..9_000 {
        let commands = brain.think(&world);
        world.advance_tick(&commands);
        keep_red_alive(&mut world);
    }
    // Finished ones: a site still being built cannot self-destruct, and makes nothing.
    let generators = |world: &SimWorld| {
        let kinds: Vec<_> = ["power_generator", "power_generator_t2", "power_generator_t3", "hydrocarbon_plant"].iter().map(|key| world.blueprints.id(key)).collect();
        world
            .units
            .iter()
            .filter(|(_, entity)| {
                world.ecs.get::<&UnitStats>(*entity).is_ok_and(|stats| stats.team == Team::Blue)
                    && world.ecs.get::<&Blueprinted>(*entity).is_ok_and(|unit| kinds.contains(&unit.0))
                    && world.ecs.get::<&sim_engine::sim::components::Construction>(*entity).is_err()
            })
            .count()
    };
    assert!(generators(&world) >= 4);
    // Raiders take out every generator at once, and the energy in store with them.
    let wanted: Vec<_> = ["power_generator", "power_generator_t2", "power_generator_t3", "hydrocarbon_plant"].iter().map(|key| world.blueprints.id(key)).collect();
    let doomed: Vec<_> = world
        .units
        .iter()
        .filter(|(_, entity)| world.ecs.get::<&UnitStats>(*entity).is_ok_and(|stats| stats.team == Team::Blue) && world.ecs.get::<&Blueprinted>(*entity).is_ok_and(|unit| wanted.contains(&unit.0)))
        .map(|(id, _)| *id)
        .collect();
    world.advance_tick(&[SimCommand::SelfDestruct { units: doomed }]);
    for _ in 0..60 {
        world.advance_tick(&[]);
    }
    assert_eq!(generators(&world), 0);
    world.economies[0].energy = sim_engine::sim::blueprints::Res::ZERO;
    let (mut paused, mut resumed) = (false, false);
    for _ in 0..6_000 {
        let commands = brain.think(&world);
        for command in &commands {
            if let SimCommand::SetPaused { paused: on, .. } = command {
                paused |= *on;
                resumed |= paused && !*on;
            }
        }
        world.advance_tick(&commands);
        keep_red_alive(&mut world);
    }
    assert!(paused, "its extractors were paused, so that energy went into generators");
    assert!(resumed, "and started again once energy was back");
    assert!(generators(&world) >= 2, "only {} generators rebuilt", generators(&world));
}

/// Mobile units a team has: its army, aircraft, ships, engineers, and commander.
fn mobile_units(world: &SimWorld, team: Team) -> usize {
    world.snapshot().units.iter().filter(|unit| unit.team == team && !unit.is_structure).count()
}

/// The player's default opponent must be easy to beat: it keeps a small army, so that by twenty
/// minutes a normal AI, on either side, fields half as many again.
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn an_easy_ai_keeps_a_small_army() {
    // 16 land units, 4 aircraft, 3 ships, 3 engineers, and the commander.
    const EASY_MOST: usize = 27;
    for seed in [1, 2, 3] {
        for easy in [Team::Blue, Team::Red] {
            let mut world = scenario::skirmish(seed).world;
            let difficulty = |team: Team| if team == easy { Difficulty::Easy } else { Difficulty::Normal };
            let mut brains = [Team::Blue, Team::Red].map(|team| AiBrain::with_difficulty(team, seed, world.starts, difficulty(team)));
            while world.current_tick < 12_000 && world.match_result.is_none() {
                advance_ai_vs_ai(&mut world, &mut brains);
                if world.current_tick.is_multiple_of(600) {
                    assert!(mobile_units(&world, easy) <= EASY_MOST, "seed {seed}: easy {easy:?} has {} units at tick {}", mobile_units(&world, easy), world.current_tick);
                }
            }
            let (weak, strong) = (mobile_units(&world, easy), mobile_units(&world, easy.enemy()));
            assert!(strong * 2 >= weak * 3, "seed {seed}: normal has {strong} units to easy {easy:?}'s {weak}");
        }
    }
}

/// Easy is still a game: it builds a base and an army, and attacks, but not in the first
/// twelve minutes.
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn an_easy_ai_builds_and_attacks_but_not_at_once() {
    let mut world = scenario::skirmish(4).world;
    let mut brain = AiBrain::with_difficulty(Team::Blue, 4, world.starts, Difficulty::Easy);
    let mut first_attack = None;
    // Twenty minutes, against nobody, so that nothing calls it to defend.
    for _ in 0..12_000 {
        let commands = brain.think(&world);
        if first_attack.is_none() && commands.iter().any(|command| matches!(command, SimCommand::AttackMove { .. })) {
            first_attack = Some(world.current_tick);
        }
        world.advance_tick(&commands);
    }
    let factories: usize = ["land_factory_t1", "land_factory_t2", "land_factory_t3", "land_factory_t2_support", "land_factory_t3_support"].iter().map(|key| count_owned(&world, Team::Blue, key)).sum();
    assert!(factories >= 1, "it built a factory");
    let extractors: usize = ["mass_extractor", "mass_extractor_t2", "mass_extractor_t3"].iter().map(|key| count_owned(&world, Team::Blue, key)).sum();
    assert!(extractors >= 4, "it took deposits: {extractors}");
    let first_attack = first_attack.expect("it attacked");
    assert!(first_attack >= 7_200, "it attacked at tick {first_attack}, before twelve minutes");
}
