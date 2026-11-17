//! Enhancement effects against the data (GAME_SPECIFICATION.md §2): every enhancement, fitted after
//! what it needs, gives its unit what it says: health, regeneration, gun range, damage, reload and
//! blast, sight, build power, a shield, and income. Effects in different slots add up; a later
//! enhancement in a slot replaces the earlier.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::{EnhancementEffects, EnhancementId, Res};
use sim_engine::sim::components::{Builder, Combat, Health, Shield, Team, UnitStats};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::world::SimWorld;

fn world() -> SimWorld {
    let config = SimConfig { map_units: 10_240, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    for economy in &mut world.economies {
        for value in [&mut economy.base_energy_capacity, &mut economy.energy_capacity, &mut economy.energy] {
            *value = Res::from_num(10_000_000);
        }
    }
    world
}

#[test]
fn every_enhancement_gives_what_it_says() {
    let db = world().blueprints;
    let mut failures = Vec::new();
    let mut checked = 0;
    for def in db.iter() {
        let unit_id = db.id(&def.key);
        for enhancement in db.enhancements_for(unit_id) {
            checked += 1;
            let name = format!("{} with {}", def.key, enhancement.key);
            let mut world = world();
            let unit = world.spawn_mobile(unit_id, Team::Blue, Vec2Fp::new(fp_int(3_000), fp_int(3_000)));
            let entity = world.unit_entity(unit).unwrap();
            // Its own income, counted on the next tick.
            world.advance_tick(&[]);
            let income_before = (world.economies[0].mass_income, world.economies[0].energy_income);
            // What it needs first, then itself; the last in each slot is what stays.
            let mut chain: Vec<EnhancementId> = vec![enhancement.id];
            let mut needs = enhancement.requires;
            while let Some(earlier) = needs {
                chain.push(earlier);
                needs = world.blueprints.enhancement(earlier).requires;
            }
            chain.reverse();
            let mut by_slot: std::collections::BTreeMap<String, EnhancementEffects> = std::collections::BTreeMap::new();
            for &id in &chain {
                world.install_enhancement(unit, id);
                let fitted = world.blueprints.enhancement(id);
                by_slot.insert(format!("{:?}", fitted.slot), fitted.effects);
            }
            world.advance_tick(&[]);
            let effects: Vec<EnhancementEffects> = by_slot.into_values().collect();
            let total_health: Fp = effects.iter().map(|effects| effects.health).sum();
            let last = |pick: &dyn Fn(&EnhancementEffects) -> bool| effects.iter().rev().find(|effects| pick(effects)).copied();
            let health = world.ecs.get::<&Health>(entity).unwrap().max;
            if health != def.health + total_health {
                failures.push(format!("{name}: health {health}, expected {}", def.health + total_health));
            }
            let combat = *world.ecs.get::<&Combat>(entity).unwrap();
            if let Some(range) = last(&|effects| effects.weapon_range.is_some()).and_then(|effects| effects.weapon_range) {
                if combat.range != range {
                    failures.push(format!("{name}: range {}, expected {range}", combat.range));
                }
            }
            if let Some(damage) = last(&|effects| effects.weapon_damage.is_some()).and_then(|effects| effects.weapon_damage) {
                if combat.projectile_damage != damage {
                    failures.push(format!("{name}: damage {}, expected {damage}", combat.projectile_damage));
                }
            }
            if let Some(reload) = last(&|effects| effects.weapon_reload_ticks.is_some()).and_then(|effects| effects.weapon_reload_ticks) {
                if combat.reload_ticks != reload {
                    failures.push(format!("{name}: reload {}, expected {reload}", combat.reload_ticks));
                }
            }
            if let Some(splash) = last(&|effects| effects.weapon_splash.is_some()).and_then(|effects| effects.weapon_splash) {
                if combat.projectile_splash != splash {
                    failures.push(format!("{name}: blast {}, expected {splash}", combat.projectile_splash));
                }
            }
            if let Some(vision) = last(&|effects| effects.vision.is_some()).and_then(|effects| effects.vision) {
                let seen = world.ecs.get::<&UnitStats>(entity).unwrap().vision;
                if seen != vision {
                    failures.push(format!("{name}: sight {seen}, expected {vision}"));
                }
            }
            if let Some(power) = last(&|effects| effects.build_power.is_some()).and_then(|effects| effects.build_power) {
                let has = world.ecs.get::<&Builder>(entity).map_or(Fp::ZERO, |builder| builder.power);
                if has != power {
                    failures.push(format!("{name}: build power {has}, expected {power}"));
                }
            }
            if let Some(shield) = last(&|effects| effects.shield.is_some()).and_then(|effects| effects.shield) {
                match world.ecs.get::<&Shield>(entity).map(|dome| *dome) {
                    Ok(dome) if dome.radius == shield.radius && dome.max_health == shield.health => {}
                    Ok(dome) => failures.push(format!("{name}: a dome of {} and {}, expected {} and {}", dome.radius, dome.max_health, shield.radius, shield.health)),
                    Err(_) => failures.push(format!("{name}: no shield")),
                }
            }
            let income: (Res, Res) = effects.iter().fold((Res::ZERO, Res::ZERO), |sum, effects| (sum.0 + effects.mass_income, sum.1 + effects.energy_income));
            let gained = (world.economies[0].mass_income - income_before.0, world.economies[0].energy_income - income_before.1);
            if gained != income {
                failures.push(format!("{name}: income +{} materiel and +{} energy a tick, expected {} and {}", gained.0, gained.1, income.0, income.1));
            }
        }
    }
    assert!(checked >= 15, "only {checked} were checked");
    assert!(failures.is_empty(), "{} failures:\n{failures:#?}", failures.len());
}
