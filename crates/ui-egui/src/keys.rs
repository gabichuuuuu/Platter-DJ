//! The key of a track as the browser shows it: its notation, its Open Key number and which
//! keys mix well with another.
//!
//! Behaviour note: `docs/behavior/browser.md`, *Analysis in the background and keys*. A key
//! is the number the collection keeps: the pitch class of the tonic, 0 for C up to 11 for B,
//! plus 12 for a minor key.

use platter_engine::{Key, Mode};

/// How the browser writes a key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyNotation {
    /// `D♭ maj`, `A min`, as the design system writes them.
    #[default]
    Flats,
    /// `C♯ maj`, `A min`.
    Sharps,
    /// `1d` for C major up to `12d` round the circle of fifths, `1m` for A minor.
    OpenKey,
}

impl KeyNotation {
    pub const ALL: [Self; 3] = [Self::Flats, Self::Sharps, Self::OpenKey];

    pub fn name(self) -> &'static str {
        match self {
            Self::Flats => "flats",
            Self::Sharps => "sharps",
            Self::OpenKey => "open_key",
        }
    }
}

pub const KEY_NOTATIONS: &[&str] = &["flats", "sharps", "open_key"];

const FLATS: [&str; 12] = [
    "C", "D♭", "D", "E♭", "E", "F", "G♭", "G", "A♭", "A", "B♭", "B",
];
const SHARPS: [&str; 12] = [
    "C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B",
];

/// The number the collection keeps for `key`.
pub fn number_of(key: Key) -> u8 {
    key.tonic % 12 + if key.mode == Mode::Minor { 12 } else { 0 }
}

/// The Open Key number of a key, 1 to 12, and whether it is minor; `None` for a number that is
/// not a key.
pub fn open_key(key: u8) -> Option<(u8, bool)> {
    let minor = match key {
        0..=11 => false,
        12..=23 => true,
        _ => return None,
    };
    let tonic = key % 12;
    // Round the circle of fifths from C major, or from A minor, its relative.
    let from_c = if minor { (tonic + 3) % 12 } else { tonic };
    Some(((from_c * 7) % 12 + 1, minor))
}

/// `key` written in `notation`, with `words` for major and minor in the musical notations;
/// empty for a number that is not a key.
pub fn key_text(key: u8, notation: KeyNotation, words: (&str, &str)) -> String {
    let Some((number, minor)) = open_key(key) else {
        return String::new();
    };
    let tonic = usize::from(key % 12);
    let mode = if minor { words.1 } else { words.0 };
    match notation {
        KeyNotation::Flats => format!("{} {mode}", FLATS[tonic]),
        KeyNotation::Sharps => format!("{} {mode}", SHARPS[tonic]),
        KeyNotation::OpenKey => format!("{number}{}", if minor { 'm' } else { 'd' }),
    }
}

const ID3_NAMES: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
];

/// `key` as ID3 writes a key: the note, `b` for a flat, `m` for minor (`Dbm`).
pub fn id3_text(key: u8) -> String {
    let name = ID3_NAMES[usize::from(key % 12)];
    if key >= 12 {
        format!("{name}m")
    } else {
        name.to_owned()
    }
}

/// Whether a track in `key` mixes well with one in `reference`: the same key, the other mode
/// of the same Open Key number, or the same mode one number away.
pub fn compatible(key: u8, reference: u8) -> bool {
    let (Some((number, minor)), Some((reference_number, reference_minor))) =
        (open_key(key), open_key(reference))
    else {
        return false;
    };
    let distance = (i16::from(number) - i16::from(reference_number)).rem_euclid(12);
    matches!(
        (minor == reference_minor, distance),
        (true, 0 | 1 | 11) | (false, 0)
    )
}

/// Where a key sorts: by Open Key number, major before minor; keys after tracks without one.
pub fn sort_place(key: Option<u8>) -> u8 {
    key.and_then(open_key)
        .map_or(0, |(number, minor)| number * 2 + u8::from(minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    const C_MAJOR: u8 = 0;
    const A_MINOR: u8 = 9 + 12;

    #[test]
    fn the_open_key_numbers_go_round_the_circle_of_fifths() {
        // C, G, D, A, E, B, F♯, D♭, A♭, E♭, B♭, F major are 1d to 12d.
        let majors = [0, 7, 2, 9, 4, 11, 6, 1, 8, 3, 10, 5];
        for (index, tonic) in majors.into_iter().enumerate() {
            assert_eq!(open_key(tonic), Some((index as u8 + 1, false)), "{tonic}");
        }
        // A, E, B, F♯, C♯, G♯, E♭, B♭, F, C, G, D minor are 1m to 12m.
        let minors = [9, 4, 11, 6, 1, 8, 3, 10, 5, 0, 7, 2];
        for (index, tonic) in minors.into_iter().enumerate() {
            assert_eq!(
                open_key(tonic + 12),
                Some((index as u8 + 1, true)),
                "{tonic}"
            );
        }
        assert_eq!(open_key(24), None);
    }

    #[test]
    fn a_key_is_written_in_each_notation() {
        let words = ("maj", "min");
        assert_eq!(key_text(1, KeyNotation::Flats, words), "D♭ maj");
        assert_eq!(key_text(1, KeyNotation::Sharps, words), "C♯ maj");
        assert_eq!(key_text(1, KeyNotation::OpenKey, words), "8d");
        assert_eq!(key_text(A_MINOR, KeyNotation::Flats, words), "A min");
        assert_eq!(
            key_text(A_MINOR, KeyNotation::Sharps, ("may", "men")),
            "A men"
        );
        assert_eq!(key_text(A_MINOR, KeyNotation::OpenKey, words), "1m");
        assert_eq!(key_text(99, KeyNotation::Flats, words), "");
        assert_eq!(
            number_of(Key {
                tonic: 9,
                mode: Mode::Minor
            }),
            A_MINOR
        );
    }

    #[test]
    fn compatible_keys_are_the_same_its_relative_and_its_neighbours() {
        // C major is 1d: 1d, 1m (A minor), 12d (F major) and 2d (G major) mix with it.
        assert!(compatible(C_MAJOR, C_MAJOR));
        assert!(compatible(A_MINOR, C_MAJOR));
        assert!(compatible(5, C_MAJOR));
        assert!(compatible(7, C_MAJOR));
        // 3d (D major), 2m (E minor) and 12m (D minor) do not.
        assert!(!compatible(2, C_MAJOR));
        assert!(!compatible(4 + 12, C_MAJOR));
        assert!(!compatible(2 + 12, C_MAJOR));
        assert!(!compatible(99, C_MAJOR));
        // The relation holds both ways.
        for key in 0..24 {
            for reference in 0..24 {
                assert_eq!(compatible(key, reference), compatible(reference, key));
            }
        }
    }

    #[test]
    fn a_key_is_written_as_id3_writes_it() {
        assert_eq!(id3_text(1 + 12), "Dbm");
        assert_eq!(id3_text(A_MINOR), "Am");
        assert_eq!(id3_text(6), "Gb");
        assert_eq!(id3_text(C_MAJOR), "C");
    }

    #[test]
    fn keys_sort_by_number_then_mode() {
        let mut keys = [Some(A_MINOR), None, Some(7), Some(C_MAJOR)];
        keys.sort_by_key(|key| sort_place(*key));
        assert_eq!(keys, [None, Some(C_MAJOR), Some(A_MINOR), Some(7)]);
    }
}
