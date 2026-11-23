//! Players and teams (GAME_SPECIFICATION.md §16, §17.2): each player has their own commander,
//! economy, and units, on a team; a player whose commander dies is out, with everything they own;
//! a team is out once all its players are.

use sim_engine::math::fp::{fp_int, Vec2Fp};
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{PlayerId, Team, UnitStats};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::world::{MatchResult, SimWorld};

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

/// A 2v2 on open ground: players 0 and 1 Blue, 2 and 3 Red, each with a commander far apart.
fn two_on_two() -> (SimWorld, [sim_engine::sim::components::SimId; 4]) {
    let config = SimConfig { map_units: 10_240, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    world.set_players(&[Team::Blue, Team::Blue, Team::Red, Team::Red]);
    let rig = world.blueprints.id("command_rig");
    let at = [point(1_500, 1_500), point(8_700, 1_500), point(1_500, 8_700), point(8_700, 8_700)];
    let commanders = [0, 1, 2, 3].map(|player| world.spawn_mobile_for(rig, PlayerId(player), at[player as usize]));
    world.start_economy(500, 2_000);
    (world, commanders)
}

/// Self-destructs commanders, as a player would, and waits out the countdown.
fn self_destruct(world: &mut SimWorld, units: &[sim_engine::sim::components::SimId]) {
    world.advance_tick(&[SimCommand::SelfDestruct { units: units.to_vec() }]);
    for _ in 0..80 {
        world.advance_tick(&[]);
    }
}

#[test]
fn each_player_owns_their_commander_and_has_their_own_economy() {
    let (mut world, commanders) = two_on_two();
    assert_eq!(world.players, vec![Team::Blue, Team::Blue, Team::Red, Team::Red]);
    for (player, &commander) in commanders.iter().enumerate() {
        assert_eq!(world.commanders[player], Some(commander));
        let stats = *world.ecs.get::<&UnitStats>(world.unit_entity(commander).unwrap()).unwrap();
        assert_eq!(stats.owner, PlayerId(player as u8));
        assert_eq!(stats.team, world.players[player]);
    }
    assert_eq!(world.first_player(Team::Red), PlayerId(2));
    // Player 1 builds a generator: it spends player 1's materiel, and no one else's.
    let generator = world.blueprints.id("power_generator");
    let before: Vec<Res> = world.economies.iter().map(|economy| economy.mass).collect();
    world.advance_tick(&[SimCommand::Build { units: vec![commanders[1]], blueprint: generator, cell_x: 430, cell_y: 80, queued: false }]);
    for _ in 0..200 {
        world.advance_tick(&[]);
    }
    let after: Vec<Res> = world.economies.iter().map(|economy| economy.mass).collect();
    assert!(after[1] < before[1], "player 1 paid: {} then {}", before[1], after[1]);
    for player in [0, 2, 3] {
        assert!(after[player] >= before[player], "player {player} paid for player 1's generator");
    }
    let built = world.units.iter().any(|&(_, entity)| {
        world.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.is_structure && stats.owner == PlayerId(1) && stats.team == Team::Blue)
    });
    assert!(built, "the generator is player 1's");
}

#[test]
fn a_player_whose_commander_dies_is_out_and_their_team_plays_on_until_all_are() {
    let (mut world, commanders) = two_on_two();
    // Player 0 has a second unit, far from the commander's blast, which goes with them.
    let tank = world.spawn_mobile_for(world.blueprints.id("t1_medium_tank"), PlayerId(0), point(5_000, 5_000));
    self_destruct(&mut world, &[commanders[0]]);
    assert!(world.eliminated[0], "player 0 is out");
    assert!(world.unit_position(tank).is_none(), "and everything they owned goes with them");
    assert!(world.unit_position(commanders[1]).is_some(), "their ally plays on");
    assert_eq!(world.match_result, None, "Blue has a player left");
    self_destruct(&mut world, &[commanders[1]]);
    assert!(world.eliminated[1]);
    assert_eq!(world.match_result, Some(MatchResult::Winner(Team::Red)), "Blue is out");
}

#[test]
fn the_last_players_of_both_teams_going_out_together_is_a_draw() {
    let (mut world, commanders) = two_on_two();
    self_destruct(&mut world, &[commanders[0], commanders[2]]);
    assert_eq!(world.match_result, None, "each team has a player left");
    self_destruct(&mut world, &[commanders[1], commanders[3]]);
    assert_eq!(world.match_result, Some(MatchResult::Draw));
}

/// A 2v2 on Saddleback, four AIs: each player builds a base of their own from their own
/// economy, the match ends with a team out, and it replays from its orders alone.
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn a_two_on_two_of_ais_plays_out_and_replays() {
    use std::sync::Arc;
    use sim_engine::ai::{play_team_match, replay_team_match};
    use sim_engine::sim::maps::saddleback;
    use sim_engine::sim::scenario::MapSource;
    let map = MapSource::Map(Arc::new(saddleback()));
    let mut factories = [false; 4];
    let record = play_team_match(&map, 2, 2, 40_000, |world| {
        if world.current_tick % 600 != 0 {
            return;
        }
        for &(_, entity) in &world.units {
            let (Ok(stats), Ok(unit)) = (world.ecs.get::<&UnitStats>(entity), world.ecs.get::<&sim_engine::sim::components::Blueprinted>(entity)) else { continue };
            if world.blueprints.get(unit.0).key.starts_with("land_factory") {
                factories[stats.owner.index()] = true;
            }
        }
    })
    .expect("Saddleback holds four players");
    assert!(factories.iter().all(|&built| built), "every player builds a factory of their own: {factories:?}");
    assert!(matches!(record.result, Some(MatchResult::Winner(_))), "a team wins: {:?} at tick {}", record.result, record.ticks);
    assert_eq!(replay_team_match(&map, 2, 2, &record.log).expect("again"), record.checksums, "it replays from its orders");
}
