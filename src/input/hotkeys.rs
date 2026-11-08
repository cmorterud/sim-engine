//! Keys for the options in the command panel. Unit orders have a key each. Build items take
//! the key of their slot in the build bar, which is the same for the same kind of thing at every
//! tech level. Mostly on the left of the keyboard, because the other hand is on the mouse.

use winit::keyboard::KeyCode;

pub const MOVE: char = 'M';
pub const ATTACK_MOVE: char = 'F';
pub const PATROL: char = 'P';
pub const ASSIST: char = 'Y';
/// Off to the right, because every letter on the left is taken.
pub const FORMATION: char = 'J';
/// The last letter left, beside J.
pub const ATTACK_GROUND: char = 'I';
pub const STOP: char = 'X';
pub const PAUSE: char = 'O';
pub const OVERCHARGE: char = 'L';
pub const UPGRADE: char = 'U';
pub const REPEAT: char = 'R';
/// Away from the others, because it throws away a queue.
pub const CLEAR_QUEUE: char = 'K';

/// Letters handled on their own: panning, patrol, which also works by holding the key, and mute.
pub const RESERVED: [char; 5] = ['W', 'A', 'S', 'D', PATROL];

/// Shows the next tech level in the build bars.
pub const CYCLE_TIER: char = '`';

/// The keys of a build bar, by slot, in the order the bar shows them. Builders' and factories'
/// bars use the same keys, since only one bar is shown at a time: a factory's, if one is selected.
pub const SLOT_KEYS: [char; 20] = ['Q', 'E', 'G', 'T', 'C', 'Z', 'V', 'B', 'H', 'N', ',', '.', '/', ';', '\'', '[', ']', '\\', '-', '='];

/// Which bar an item is in: a builder's, for structures, or a factory's, for units and missiles.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Bar {
    Builder,
    Factory,
}

/// An item's place in its build bar. Each kind of thing has its own slot, the same at every
/// tech level, so that the key for an extractor, a power generator, or anti-air never changes
/// with the level shown: only one level's items are in the bar at a time.
pub fn build_slot(key: &str) -> Option<(Bar, usize)> {
    use Bar::{Builder, Factory};
    Some(match key {
        "land_factory_t1" | "land_factory_t2_support" | "land_factory_t3_support" => (Builder, 0),
        "mass_extractor" | "mass_extractor_t2" | "mass_extractor_t3" => (Builder, 1),
        "power_generator" | "power_generator_t2" | "power_generator_t3" => (Builder, 2),
        "mass_storage" => (Builder, 3),
        "t1_point_defense" | "t2_point_defense" => (Builder, 4),
        "radar" | "radar_t2" | "omni_sensor" => (Builder, 5),
        "t1_aa_turret" | "t2_flak" | "t3_sam" => (Builder, 6),
        "air_factory" | "air_factory_t2_support" | "air_factory_t3_support" => (Builder, 7),
        // Two kinds that are never at the same level may share a slot.
        "hydrocarbon_plant" | "mass_fabricator" | "mass_fabricator_t3" => (Builder, 8),
        "shield_generator" => (Builder, 9),
        "tactical_missile_launcher" | "strategic_missile_launcher" => (Builder, 10),
        "tactical_missile_defense" | "strategic_missile_defense" => (Builder, 11),
        "stealth_field_generator" => (Builder, 12),
        "air_staging" => (Builder, 13),
        "quantum_gateway" => (Builder, 14),
        "energy_storage" => (Builder, 15),
        "t2_artillery" | "t3_artillery" => (Builder, 16),
        "naval_factory" | "naval_factory_t2_support" | "naval_factory_t3_support" => (Builder, 17),
        "t1_sonar" | "t2_sonar" => (Builder, 18),
        "t1_torpedo_launcher" | "t2_torpedo_launcher" => (Builder, 19),
        // Builders and scouts, then direct fire, then artillery and bombers, then anti-air and gunships.
        "t1_engineer" | "t2_engineer" | "t3_engineer" | "support_commander" | "t1_air_scout" | "t3_spy_plane" => (Factory, 0),
        "tactical_missile" | "strategic_missile" | "anti_nuke" => (Factory, 0),
        "t1_light_bot" | "t2_heavy_tank" | "t1_interceptor" | "t2_torpedo_bomber" | "t3_air_superiority" | "t1_frigate" | "t2_destroyer" | "t3_battleship" => (Factory, 1),
        "t1_light_artillery" | "t2_amphibious_tank" | "t3_mobile_artillery" | "t1_bomber" | "t2_fighter_bomber" | "t3_strategic_bomber" | "t2_cruiser" | "t3_battlecruiser" => (Factory, 2),
        "t1_mobile_aa" | "t2_mobile_flak" | "t2_gunship" | "t3_heavy_gunship" | "t1_attack_sub" => (Factory, 3),
        _ => return None,
    })
}

/// The key for an item, from its slot.
pub fn blueprint_hotkey(key: &str) -> Option<char> {
    build_slot(key).map(|(_, slot)| SLOT_KEYS[slot])
}

/// How a key is shown on a button.
pub fn key_name(letter: char) -> String {
    letter.to_string()
}

/// The letter of a key that is used for command panel options.
pub const fn letter_for(code: KeyCode) -> Option<char> {
    Some(match code {
        KeyCode::KeyQ => 'Q',
        KeyCode::KeyE => 'E',
        KeyCode::KeyG => 'G',
        KeyCode::KeyT => 'T',
        KeyCode::KeyC => 'C',
        KeyCode::KeyZ => 'Z',
        KeyCode::KeyV => 'V',
        KeyCode::KeyB => 'B',
        KeyCode::KeyH => 'H',
        KeyCode::KeyN => 'N',
        KeyCode::Comma => ',',
        KeyCode::Period => '.',
        KeyCode::Slash => '/',
        KeyCode::Semicolon => ';',
        KeyCode::Quote => '\'',
        KeyCode::BracketLeft => '[',
        KeyCode::BracketRight => ']',
        KeyCode::Backslash => '\\',
        KeyCode::Minus => '-',
        KeyCode::Equal => '=',
        KeyCode::Backquote => CYCLE_TIER,
        KeyCode::KeyM => MOVE,
        KeyCode::KeyF => ATTACK_MOVE,
        KeyCode::KeyY => ASSIST,
        KeyCode::KeyJ => FORMATION,
        KeyCode::KeyI => ATTACK_GROUND,
        KeyCode::KeyX => STOP,
        KeyCode::KeyO => PAUSE,
        KeyCode::KeyL => OVERCHARGE,
        KeyCode::KeyU => UPGRADE,
        KeyCode::KeyR => REPEAT,
        KeyCode::KeyK => CLEAR_QUEUE,
        _ => return None,
    })
}

/// The control group on a number key, from 0 to 9.
pub const fn group_for(code: KeyCode) -> Option<usize> {
    Some(match code {
        KeyCode::Digit0 => 0,
        KeyCode::Digit1 => 1,
        KeyCode::Digit2 => 2,
        KeyCode::Digit3 => 3,
        KeyCode::Digit4 => 4,
        KeyCode::Digit5 => 5,
        KeyCode::Digit6 => 6,
        KeyCode::Digit7 => 7,
        KeyCode::Digit8 => 8,
        KeyCode::Digit9 => 9,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::blueprints::BlueprintDb;

    const ALL_KEYS: [KeyCode; 32] = [
        KeyCode::KeyQ,
        KeyCode::KeyE,
        KeyCode::KeyG,
        KeyCode::KeyT,
        KeyCode::KeyC,
        KeyCode::KeyZ,
        KeyCode::KeyV,
        KeyCode::KeyB,
        KeyCode::KeyH,
        KeyCode::KeyN,
        KeyCode::Comma,
        KeyCode::Period,
        KeyCode::Slash,
        KeyCode::Semicolon,
        KeyCode::Quote,
        KeyCode::BracketLeft,
        KeyCode::BracketRight,
        KeyCode::Backslash,
        KeyCode::Minus,
        KeyCode::Equal,
        KeyCode::Backquote,
        KeyCode::KeyM,
        KeyCode::KeyF,
        KeyCode::KeyY,
        KeyCode::KeyJ,
        KeyCode::KeyI,
        KeyCode::KeyX,
        KeyCode::KeyO,
        KeyCode::KeyL,
        KeyCode::KeyU,
        KeyCode::KeyR,
        KeyCode::KeyK,
    ];

    const PANEL_LETTERS: [char; 12] = [MOVE, ATTACK_MOVE, FORMATION, ATTACK_GROUND, ASSIST, STOP, PAUSE, OVERCHARGE, UPGRADE, REPEAT, CLEAR_QUEUE, CYCLE_TIER];

    /// Everything that can appear in a build menu, from the real data.
    fn buildable(db: &BlueprintDb) -> Vec<String> {
        let mut keys: Vec<String> = db.iter().flat_map(|unit| unit.builds.iter().map(|option| db.get(*option).key.clone())).collect();
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    #[test]
    fn everything_that_can_be_built_has_a_slot_in_the_right_bar() {
        let db = BlueprintDb::standard();
        for key in buildable(&db) {
            let (bar, _) = build_slot(&key).unwrap_or_else(|| panic!("{key} has no slot"));
            let expected = if db.get(db.id(&key)).is_structure() { Bar::Builder } else { Bar::Factory };
            assert_eq!(bar, expected, "{key} is in the wrong bar");
        }
    }

    #[test]
    fn no_two_items_in_one_bar_at_one_level_share_a_slot() {
        let db = BlueprintDb::standard();
        for unit in db.iter() {
            let mut taken: Vec<(u8, usize)> = unit.builds.iter().map(|option| (db.get(*option).tier, build_slot(&db.get(*option).key).unwrap().1)).collect();
            let count = taken.len();
            taken.sort_unstable();
            taken.dedup();
            assert_eq!(taken.len(), count, "two items in {}'s bar share a slot at one level", unit.key);
        }
    }

    #[test]
    fn the_same_kind_of_thing_keeps_its_key_at_every_level() {
        for kinds in [
            &["t1_aa_turret", "t2_flak", "t3_sam"][..],
            &["mass_extractor", "mass_extractor_t2", "mass_extractor_t3"],
            &["power_generator", "power_generator_t2", "power_generator_t3"],
            &["radar", "radar_t2", "omni_sensor"],
            &["t1_point_defense", "t2_point_defense"],
            &["t2_artillery", "t3_artillery"],
            &["t1_light_artillery", "t3_mobile_artillery"],
            &["land_factory_t1", "land_factory_t2_support", "land_factory_t3_support"],
            &["air_factory", "air_factory_t2_support", "air_factory_t3_support"],
            &["naval_factory", "naval_factory_t2_support", "naval_factory_t3_support"],
            &["t1_sonar", "t2_sonar"],
            &["t1_torpedo_launcher", "t2_torpedo_launcher"],
            &["t1_frigate", "t2_destroyer", "t3_battleship"],
            &["t2_cruiser", "t3_battlecruiser"],
            &["mass_fabricator", "mass_fabricator_t3"],
            &["tactical_missile_launcher", "strategic_missile_launcher"],
            &["tactical_missile_defense", "strategic_missile_defense"],
            &["t1_engineer", "t2_engineer", "t3_engineer"],
            &["t1_light_bot", "t2_heavy_tank"],
            &["t1_mobile_aa", "t2_mobile_flak"],
            &["t1_air_scout", "t3_spy_plane"],
            &["t1_interceptor", "t3_air_superiority"],
            &["t1_bomber", "t2_fighter_bomber", "t3_strategic_bomber"],
            &["t2_gunship", "t3_heavy_gunship"],
        ] {
            let first = blueprint_hotkey(kinds[0]);
            for kind in kinds {
                assert_eq!(blueprint_hotkey(kind), first, "{kind}");
            }
        }
        // The common structures keep the keys they always had.
        for (key, letter) in [("land_factory_t1", 'Q'), ("mass_extractor", 'E'), ("power_generator", 'G'), ("mass_storage", 'T'), ("t1_point_defense", 'C')] {
            assert_eq!(blueprint_hotkey(key), Some(letter), "{key}");
        }
    }

    #[test]
    fn slot_keys_never_take_a_key_already_in_use() {
        for letter in &SLOT_KEYS {
            assert!(!RESERVED.contains(letter) && !PANEL_LETTERS.contains(letter), "{letter} is already used");
        }
        let mut keys = SLOT_KEYS.to_vec();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), SLOT_KEYS.len(), "two slots share a key");
        // Every item fits.
        let db = BlueprintDb::standard();
        assert!(buildable(&db).iter().all(|key| build_slot(key).is_some_and(|(_, slot)| slot < SLOT_KEYS.len())));
    }

    #[test]
    fn every_number_key_has_its_own_control_group() {
        let digits = [KeyCode::Digit0, KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9];
        for (expected, code) in digits.into_iter().enumerate() {
            assert_eq!(group_for(code), Some(expected));
        }
        assert_eq!(group_for(KeyCode::KeyQ), None);
    }

    #[test]
    fn every_key_shown_can_really_be_pressed() {
        let pressable: Vec<char> = ALL_KEYS.iter().filter_map(|code| letter_for(*code)).collect();
        for letter in SLOT_KEYS.iter().chain(&PANEL_LETTERS) {
            assert!(pressable.contains(letter), "{letter} is shown but cannot be pressed");
        }
        assert_eq!(letter_for(KeyCode::KeyW), None, "panning keys are not panel keys");

    }
}
