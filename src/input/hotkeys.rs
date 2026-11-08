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

/// Function keys, for options added after every letter and punctuation key was taken. Each
/// stands for its key as a character no keyboard types.
pub const F1: char = '\u{E001}';
pub const F2: char = '\u{E002}';
pub const F3: char = '\u{E003}';
pub const F4: char = '\u{E004}';
pub const F5: char = '\u{E005}';
pub const F6: char = '\u{E006}';

/// How a key is shown on a button: the letter, or the function key's name.
pub fn key_name(letter: char) -> String {
    match letter {
        F1 => "F1".to_string(),
        F2 => "F2".to_string(),
        F3 => "F3".to_string(),
        F4 => "F4".to_string(),
        F5 => "F5".to_string(),
        F6 => "F6".to_string(),
        other => other.to_string(),
    }
}

/// The key for building a blueprint, by its key in the blueprint data.
pub fn blueprint_hotkey(key: &str) -> Option<char> {
    Some(match key {
        "land_factory_t1" => 'Q',
        "mass_extractor" => 'E',
        "power_generator" => 'G',
        "mass_storage" => 'T',
        "t1_point_defense" => 'C',
        // Every letter is taken, so the key above Tab, beside 1.
        "shield_generator" => '`',
        // Missiles go on the keys right of M, and the silos' missiles beside 0.
        "tactical_missile_launcher" => ',',
        "tactical_missile_defense" => '.',
        "strategic_missile_launcher" => '/',
        "strategic_missile_defense" => ';',
        // Building a missile is the same key in every silo, as no silo builds two kinds.
        "strategic_missile" | "tactical_missile" => '-',
        "radar" => '[',
        "stealth_field_generator" => ']',
        "t2_engineer" => '\'',
        "t3_engineer" => '\\',
        "anti_nuke" => '=',
        // N mutes the sound when nothing selected can build a gateway.
        "quantum_gateway" => 'N',
        "hydrocarbon_plant" => F1,
        "mass_fabricator" => F2,
        "mass_fabricator_t3" => F3,
        "air_factory" => F4,
        "t1_aa_turret" => F5,
        "t1_mobile_aa" => F6,
        // The gateway's support commander and the air factory's aircraft share the land
        // factory's keys: a key queues every option on it in the selected factories' menus.
        "t1_engineer" | "support_commander" | "t1_air_scout" => 'Z',
        "t1_light_bot" | "t1_interceptor" => 'B',
        "t1_light_artillery" | "t1_bomber" => 'V',
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
        KeyCode::Backquote => '`',
        KeyCode::Comma => ',',
        KeyCode::Period => '.',
        KeyCode::Slash => '/',
        KeyCode::Semicolon => ';',
        KeyCode::Minus => '-',
        KeyCode::Equal => '=',
        KeyCode::Quote => '\'',
        KeyCode::BracketLeft => '[',
        KeyCode::BracketRight => ']',
        KeyCode::Backslash => '\\',
        KeyCode::KeyZ => 'Z',
        KeyCode::KeyN => 'N',
        KeyCode::F1 => F1,
        KeyCode::F2 => F2,
        KeyCode::F3 => F3,
        KeyCode::F4 => F4,
        KeyCode::F5 => F5,
        KeyCode::F6 => F6,
        KeyCode::KeyB => 'B',
        KeyCode::KeyV => 'V',
        KeyCode::KeyH => 'H',
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

    const ALL_KEYS: [KeyCode; 38] = [
        KeyCode::KeyN,
        KeyCode::F1,
        KeyCode::F2,
        KeyCode::F3,
        KeyCode::F4,
        KeyCode::F5,
        KeyCode::F6,
        KeyCode::BracketLeft,
        KeyCode::BracketRight,
        KeyCode::Backquote,
        KeyCode::Quote,
        KeyCode::Backslash,
        KeyCode::Comma,
        KeyCode::Period,
        KeyCode::Slash,
        KeyCode::Semicolon,
        KeyCode::Minus,
        KeyCode::Equal,
        KeyCode::KeyJ,
        KeyCode::KeyI,
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

    const PANEL_LETTERS: [char; 11] = [MOVE, ATTACK_MOVE, FORMATION, ATTACK_GROUND, ASSIST, STOP, PAUSE, OVERCHARGE, UPGRADE, REPEAT, CLEAR_QUEUE];

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
        assert_eq!(keys.len(), 32);
        for key in keys {
            assert!(blueprint_hotkey(&key).is_some(), "{key} has no hotkey");
        }
    }

    #[test]
    fn no_two_options_share_a_key_or_take_one_already_in_use() {
        let db = BlueprintDb::standard();
        // Factory items may share a key, as a key queues all of them: the two silos' missiles,
        // and the engineer and the support commander. Structures never do, nor panel orders.
        let shared = ["tactical_missile", "support_commander", "t1_air_scout", "t1_interceptor", "t1_bomber"];
        let mut letters: Vec<char> = buildable(&db).iter().filter(|key| !shared.contains(&key.as_str())).filter_map(|key| blueprint_hotkey(key)).collect();
        assert_eq!(blueprint_hotkey("tactical_missile"), blueprint_hotkey("strategic_missile"));
        assert_eq!(blueprint_hotkey("support_commander"), blueprint_hotkey("t1_engineer"));
        assert_eq!(blueprint_hotkey("t1_air_scout"), blueprint_hotkey("t1_engineer"));
        assert_eq!(blueprint_hotkey("t1_interceptor"), blueprint_hotkey("t1_light_bot"));
        assert_eq!(blueprint_hotkey("t1_bomber"), blueprint_hotkey("t1_light_artillery"));
        letters.extend(PANEL_LETTERS);
        let count = letters.len();
        for letter in &letters {
            assert!(!RESERVED.contains(letter), "{letter} is already used");
        }
        letters.sort_unstable();
        letters.dedup();
        assert_eq!(letters.len(), count, "two options share a key");
        // And no one menu has two options on one key.
        for unit in db.iter() {
            let mut menu: Vec<char> = unit.builds.iter().filter_map(|option| blueprint_hotkey(&db.get(*option).key)).collect();
            let count = menu.len();
            menu.sort_unstable();
            menu.dedup();
            assert_eq!(menu.len(), count, "two options in {}'s menu share a key", unit.key);
        }
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
