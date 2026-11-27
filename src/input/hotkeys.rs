//! Keys for the options in the command panel. Unit orders have a key each. A builder's
//! structures come in families that share a key, such as the factories on W: the key starts
//! placing the first of the family, and pressing it again moves on to the next. A factory's
//! items take the key of their slot in its bar. Both stay the same at every tech level. Mostly
//! on the left of the keyboard, because the other hand is on the mouse; the arrow keys pan.

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
pub const DISCHARGE: char = 'L';
/// Shared with Discharge, as no commander carries missiles: with a missile launcher selected, L
/// launches.
pub const LAUNCH: char = 'L';
pub const UPGRADE: char = 'U';
pub const REPEAT: char = 'R';
/// Away from the others, because it throws away a queue.
pub const CLEAR_QUEUE: char = 'K';

/// Letters handled on their own: patrol, which also works by holding the key.
pub const RESERVED: [char; 1] = [PATROL];

/// Shows the next tech level in the build bars.
pub const CYCLE_TIER: char = '`';

/// The keys of a factory's bar, by slot, in the order the bar shows them.
pub const SLOT_KEYS: [char; 8] = ['Q', 'E', 'G', 'T', 'C', 'Z', 'V', 'B'];

/// A builder's structures, in families that share a key, in the order the bar shows them. Each
/// family lists its members in the order its key steps through them; only those of the level
/// shown are in the bar, so a key steps through the level's land, air, and naval factories, or
/// its radar and sonar.
pub const BUILDER_FAMILIES: [(char, &[&str]); 8] = [
    (
        'W',
        &[
            "land_factory_t1",
            "land_factory_t2_support",
            "land_factory_t3_support",
            "air_factory",
            "air_factory_t2_support",
            "air_factory_t3_support",
            "naval_factory",
            "naval_factory_t2_support",
            "naval_factory_t3_support",
            "rig_yard",
        ],
    ),
    ('E', &["mass_extractor", "mass_extractor_t2", "mass_extractor_t3", "mass_fabricator", "mass_fabricator_t3", "mass_storage"]),
    ('S', &["power_generator", "power_generator_t2", "power_generator_t3", "geothermal_plant", "energy_storage"]),
    ('D', &["t1_point_defense", "t2_point_defense", "t3_heavy_point_defense", "t1_aa_turret", "t2_flak", "t3_sam", "t1_torpedo_launcher", "t2_torpedo_launcher"]),
    ('A', &["t2_artillery", "t3_artillery", "tactical_missile_launcher", "strategic_missile_launcher"]),
    ('Z', &["radar", "radar_t2", "deep_scan_array", "t1_sonar", "t2_sonar", "stealth_field_generator"]),
    ('C', &["shield_generator", "t3_heavy_shield", "tactical_missile_defense", "strategic_missile_defense"]),
    ('V', &["airfield", "wall"]),
];

/// A builder's structure's family, by its place in `BUILDER_FAMILIES`, and its place in the
/// family.
pub fn builder_family(key: &str) -> Option<(usize, usize)> {
    BUILDER_FAMILIES.iter().enumerate().find_map(|(family, (_, members))| members.iter().position(|member| *member == key).map(|place| (family, place)))
}

/// Where an item sits in its bar: builders' bars by family and then place in it, factories' by
/// slot. Items with the same first number are grouped, with dividers between groups.
pub fn bar_order(key: &str) -> Option<(usize, usize)> {
    builder_family(key).or_else(|| build_slot(key).filter(|(bar, _)| *bar == Bar::Factory).map(|(_, slot)| (0, slot)))
}

/// Which bar an item is in: a builder's, for structures, or a factory's, for units and missiles.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Bar {
    Builder,
    Factory,
}

/// Which bar an item is in, and for a factory's items, its slot. A builder's structures are
/// placed by family instead (`builder_family`), and their slot is the family's.
pub fn build_slot(key: &str) -> Option<(Bar, usize)> {
    use Bar::Factory;
    if let Some((family, _)) = builder_family(key) {
        return Some((Bar::Builder, family));
    }
    Some(match key {
        // Builders and scouts, then bots, artillery and bombers, anti-air and gunships, tanks,
        // support, and the rest.
        "t1_engineer" | "t2_engineer" | "t3_engineer" | "field_rig" | "t1_air_scout" | "t3_spy_plane" => (Factory, 0),
        "tactical_missile" | "strategic_missile" | "strategic_interceptor" => (Factory, 0),
        "t1_light_walker" | "t2_grenadier_walker" | "t3_heavy_walker" | "t1_interceptor" | "t2_torpedo_bomber" | "t3_air_superiority" | "t1_frigate" | "t2_destroyer" | "t3_battleship" => (Factory, 1),
        "t1_light_artillery" | "t2_mobile_missile_launcher" | "t3_mobile_artillery" | "t1_bomber" | "t2_fighter_bomber" | "t3_strategic_bomber" | "t2_cruiser" | "t3_battlecruiser" => (Factory, 2),
        "t1_mobile_aa" | "t2_mobile_flak" | "t2_gunship" | "t3_heavy_gunship" | "t1_attack_sub" | "t2_escort_corvette" | "t3_missile_sub" => (Factory, 3),
        "t1_medium_tank" | "t2_heavy_tank" | "t3_siege_walker" | "t2_torpedo_boat" => (Factory, 4),
        "t1_land_scout" | "t2_mobile_shield" | "t1_light_transport" | "t2_transport" | "t3_heavy_transport" => (Factory, 5),
        "t2_assault_hovercraft" => (Factory, 6),
        "t2_combat_engineer" => (Factory, 7),
        _ => return None,
    })
}

/// The key for an item: its family's for a builder's structure, its slot's for a factory's item.
pub fn blueprint_hotkey(key: &str) -> Option<char> {
    match build_slot(key)? {
        (Bar::Builder, family) => Some(BUILDER_FAMILIES[family].0),
        (Bar::Factory, slot) => Some(SLOT_KEYS[slot]),
    }
}

/// How a key is shown on a button.
pub fn key_name(letter: char) -> String {
    letter.to_string()
}

/// The letter of a key that is used for command panel options.
pub const fn letter_for(code: KeyCode) -> Option<char> {
    Some(match code {
        KeyCode::KeyQ => 'Q',
        KeyCode::KeyW => 'W',
        KeyCode::KeyA => 'A',
        KeyCode::KeyS => 'S',
        KeyCode::KeyD => 'D',
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
        KeyCode::KeyL => DISCHARGE,
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

    const ALL_KEYS: [KeyCode; 36] = [
        KeyCode::KeyQ,
        KeyCode::KeyW,
        KeyCode::KeyA,
        KeyCode::KeyS,
        KeyCode::KeyD,
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

    const PANEL_LETTERS: [char; 12] = [MOVE, ATTACK_MOVE, FORMATION, ATTACK_GROUND, ASSIST, STOP, PAUSE, DISCHARGE, UPGRADE, REPEAT, CLEAR_QUEUE, CYCLE_TIER];

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
    fn no_two_items_in_a_factorys_bar_at_one_level_share_a_slot() {
        let db = BlueprintDb::standard();
        for unit in db.iter().filter(|unit| unit.builds.iter().any(|option| !db.get(*option).is_structure())) {
            let mut taken: Vec<(u8, usize)> = unit.builds.iter().map(|option| (db.get(*option).tier, build_slot(&db.get(*option).key).unwrap().1)).collect();
            let count = taken.len();
            taken.sort_unstable();
            taken.dedup();
            assert_eq!(taken.len(), count, "two items in {}'s bar share a slot at one level", unit.key);
        }
    }

    #[test]
    fn related_structures_share_a_key_as_one_family() {
        let same = |keys: &[&str], letter: char| {
            for key in keys {
                assert_eq!(blueprint_hotkey(key), Some(letter), "{key}");
            }
        };
        same(&["land_factory_t1", "air_factory", "naval_factory", "land_factory_t2_support", "naval_factory_t3_support"], 'W');
        same(&["radar", "t1_sonar", "radar_t2", "t2_sonar", "deep_scan_array"], 'Z');
        same(&["t1_point_defense", "t1_aa_turret", "t2_point_defense", "t2_flak", "t3_heavy_point_defense", "t3_sam"], 'D');
        same(&["mass_extractor", "mass_storage"], 'E');
        same(&["power_generator", "geothermal_plant", "energy_storage"], 'S');
        // Land comes first, so the key starts on a land factory.
        assert!(builder_family("land_factory_t1").unwrap().1 < builder_family("air_factory").unwrap().1);
        assert!(builder_family("radar").unwrap().1 < builder_family("t1_sonar").unwrap().1);
        assert!(builder_family("t1_point_defense").unwrap().1 < builder_family("t1_aa_turret").unwrap().1);
    }

    #[test]
    fn each_structure_is_in_one_family_and_each_family_has_its_own_key() {
        let mut seen: Vec<&str> = BUILDER_FAMILIES.iter().flat_map(|(_, members)| members.iter().copied()).collect();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count, "a structure is in two families");
        let mut letters: Vec<char> = BUILDER_FAMILIES.iter().map(|(letter, _)| *letter).collect();
        letters.sort_unstable();
        letters.dedup();
        assert_eq!(letters.len(), BUILDER_FAMILIES.len(), "two families share a key");
    }

    #[test]
    fn the_same_kind_of_thing_keeps_its_key_at_every_level() {
        for kinds in [
            &["t1_aa_turret", "t2_flak", "t3_sam"][..],
            &["mass_extractor", "mass_extractor_t2", "mass_extractor_t3"],
            &["power_generator", "power_generator_t2", "power_generator_t3"],
            &["radar", "radar_t2", "deep_scan_array"],
            &["t1_point_defense", "t2_point_defense", "t3_heavy_point_defense"],
            &["shield_generator", "t3_heavy_shield"],
            &["t1_medium_tank", "t2_heavy_tank", "t3_siege_walker"],
            &["t1_land_scout", "t2_mobile_shield"],
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
            &["t1_light_walker", "t2_grenadier_walker", "t3_heavy_walker"],
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
        // A factory's common units keep the keys they always had.
        for (key, letter) in [("t1_engineer", 'Q'), ("t1_light_walker", 'E'), ("t1_light_artillery", 'G'), ("t1_mobile_aa", 'T'), ("t1_medium_tank", 'C')] {
            assert_eq!(blueprint_hotkey(key), Some(letter), "{key}");
        }
    }

    #[test]
    fn slot_keys_never_take_a_key_already_in_use() {
        for letter in SLOT_KEYS.iter().chain(BUILDER_FAMILIES.iter().map(|(letter, _)| letter)) {
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
        for letter in SLOT_KEYS.iter().chain(&PANEL_LETTERS).chain(BUILDER_FAMILIES.iter().map(|(letter, _)| letter)) {
            assert!(pressable.contains(letter), "{letter} is shown but cannot be pressed");
        }
        assert_eq!(letter_for(KeyCode::ArrowUp), None, "panning keys are not panel keys");
    }
}
