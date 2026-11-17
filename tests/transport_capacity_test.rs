//! Air transports against the unit data (GAME_SPECIFICATION.md §10): each takes on exactly its
//! stated number of small, medium, and large units, and no more, and sets them all down again.

use sim_engine::math::fp::{fp_int, Vec2Fp};
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Team, Transport};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::world::SimWorld;

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

#[test]
fn every_transport_carries_exactly_its_room_and_sets_it_all_down() {
    let db = SimWorld::new(SimConfig::default(), 1).blueprints;
    // A unit of each size: a T1 tank, a T2 tank, and a T3 walker.
    let sizes = ["t1_medium_tank", "t2_heavy_tank", "t3_heavy_walker"];
    let mut failures = Vec::new();
    let mut checked = 0;
    for def in db.iter().filter(|def| def.transport.is_some()) {
        let room = def.transport.unwrap().slots;
        for (size, rider) in sizes.iter().enumerate() {
            checked += 1;
            let config = SimConfig { map_units: 10_240, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
            let mut world = SimWorld::new(config, 1);
            world.economies[0].energy = Res::from_num(1_000_000);
            let transport = world.spawn_mobile(world.blueprints.id(&def.key), Team::Blue, point(5_000, 5_000));
            // Landed, as an idle transport is.
            for _ in 0..120 {
                world.advance_tick(&[]);
            }
            let at = world.unit_position(transport).unwrap().xy();
            let wanted = room[size] as i32 + 1;
            let riders: Vec<_> = (0..wanted).map(|index| world.spawn_mobile(world.blueprints.id(rider), Team::Blue, at + Vec2Fp::new(fp_int(150 + 40 * (index % 6)), fp_int(40 * (index / 6))))).collect();
            world.advance_tick(&[SimCommand::Load { units: riders.clone(), transport }]);
            for _ in 0..1_000 {
                world.advance_tick(&[]);
            }
            let carried = world.ecs.get::<&Transport>(world.unit_entity(transport).unwrap()).unwrap().cargo.len();
            if carried != room[size] as usize {
                failures.push(format!("{} took on {carried} {rider}s, its room is {}", def.key, room[size]));
                continue;
            }
            // And sets them all down.
            world.advance_tick(&[SimCommand::Unload { transports: vec![transport], point: point(7_000, 5_000) }]);
            for _ in 0..1_500 {
                world.advance_tick(&[]);
            }
            let left = world.ecs.get::<&Transport>(world.unit_entity(transport).unwrap()).unwrap().cargo.len();
            let down = riders.iter().filter(|&&id| world.unit_position(id).is_some_and(|at| at.x > fp_int(6_500))).count();
            if left != 0 || down != room[size] as usize {
                failures.push(format!("{}: {left} {rider}s still aboard, {down} set down", def.key));
            }
        }
    }
    assert!(checked >= 6, "only {checked} were checked");
    assert!(failures.is_empty(), "{failures:#?}");
}
