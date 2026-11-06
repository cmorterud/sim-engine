//! Keys for the options in the command panel. Each option has one key whatever is selected,
//! so that a builder and a factory selected together never compete for a key.
//! Mostly on the left of the keyboard, because the other hand is on the mouse.

use winit::keyboard::KeyCode;

pub const MOVE: char = 'M';
pub const ATTACK_MOVE: char = 'F';
pub const PATROL: char = 'P';
pub const ASSIST: char = 'Y';
/// Off to the right, because every letter on the left is taken.
pub const FORMATION: char = 'J';
pub const STOP: char = 'X';
pub const PAUSE: char = 'O';
pub const OVERCHARGE: char = 'L';
pub const UPGRADE: char = 'U';
pub const REPEAT: char = 'R';
/// Away from the others, because it throws away a queue.
pub const CLEAR_QUEUE: char = 'K';

/// Letters handled on their own: panning, patrol, which also works by holding the key, and mute.
pub const RESERVED: [char; 6] = ['W', 'A', 'S', 'D', PATROL, 'N'];

/// The key for building a blueprint, by its key in the blueprint data.
pub fn blueprint_hotkey(key: &str) -> Option<char> {
    Some(match key {
        "land_factory_t1" => 'Q',
        "mass_extractor" => 'E',
        "power_generator" => 'G',
        "mass_storage" => 'T',
        "t1_point_defense" => 'C',
        "t1_engineer" => 'Z',
        "t1_light_bot" => 'B',
        "t1_light_artillery" => 'V',
        "t2_heavy_tank" => 'H',
        _ => return None,
    })
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
        KeyCode::KeyB => 'B',
        KeyCode::KeyV => 'V',
        KeyCode::KeyH => 'H',
        KeyCode::KeyM => MOVE,
        KeyCode::KeyF => ATTACK_MOVE,
        KeyCode::KeyY => ASSIST,
        KeyCode::KeyJ => FORMATION,
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

    const ALL_KEYS: [KeyCode; 18] = [
        KeyCode::KeyO,
        KeyCode::KeyL,
        KeyCode::KeyM,
        KeyCode::KeyF,
        KeyCode::KeyY,
        KeyCode::KeyX,
        KeyCode::KeyQ,
        KeyCode::KeyE,
        KeyCode::KeyG,
        KeyCode::KeyT,
        KeyCode::KeyC,
        KeyCode::KeyZ,
        KeyCode::KeyB,
        KeyCode::KeyV,
        KeyCode::KeyH,
        KeyCode::KeyU,
        KeyCode::KeyR,
        KeyCode::KeyK,
    ];

    const PANEL_LETTERS: [char; 9] = [MOVE, ATTACK_MOVE, ASSIST, STOP, PAUSE, OVERCHARGE, UPGRADE, REPEAT, CLEAR_QUEUE];

    /// Everything that can appear in a build menu, from the real data.
    fn buildable(db: &BlueprintDb) -> Vec<String> {
        let mut keys: Vec<String> = db.iter().flat_map(|unit| unit.builds.iter().map(|option| db.get(*option).key.clone())).collect();
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    #[test]
    fn everything_that_can_be_built_has_a_key() {
        let db = BlueprintDb::standard();
        let keys = buildable(&db);
        assert_eq!(keys.len(), 9);
        for key in keys {
            assert!(blueprint_hotkey(&key).is_some(), "{key} has no hotkey");
        }
    }

    #[test]
    fn no_two_options_share_a_key_or_take_one_already_in_use() {
        let db = BlueprintDb::standard();
        let mut letters: Vec<char> = buildable(&db).iter().filter_map(|key| blueprint_hotkey(key)).collect();
        letters.extend(PANEL_LETTERS);
        let count = letters.len();
        for letter in &letters {
            assert!(!RESERVED.contains(letter), "{letter} is already used");
        }
        letters.sort_unstable();
        letters.dedup();
        assert_eq!(letters.len(), count, "two options share a key");
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
        let db = BlueprintDb::standard();
        let pressable: Vec<char> = ALL_KEYS.iter().filter_map(|code| letter_for(*code)).collect();
        let mut shown: Vec<char> = buildable(&db).iter().filter_map(|key| blueprint_hotkey(key)).collect();
        shown.extend(PANEL_LETTERS);
        for letter in shown {
            assert!(pressable.contains(&letter), "{letter} is shown but cannot be pressed");
        }
        assert_eq!(letter_for(KeyCode::KeyW), None, "panning keys are not panel keys");
    }
}
