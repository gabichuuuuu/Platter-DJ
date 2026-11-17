use std::fmt;

use crate::ValueError;

/// A deck of the mixer: `docs/behavior/decks.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeckId {
    A,
    B,
    C,
    D,
}

impl DeckId {
    pub const ALL: [Self; 4] = [Self::A, Self::B, Self::C, Self::D];
    pub const COUNT: usize = Self::ALL.len();

    /// Position of the deck in per-deck arrays.
    pub fn index(self) -> usize {
        match self {
            Self::A => 0,
            Self::B => 1,
            Self::C => 2,
            Self::D => 3,
        }
    }

    pub fn letter(self) -> char {
        match self {
            Self::A => 'A',
            Self::B => 'B',
            Self::C => 'C',
            Self::D => 'D',
        }
    }

    /// Reads the letter of a deck, in either case.
    pub fn parse(text: &str) -> Result<Self, ValueError> {
        match text {
            "a" | "A" => Ok(Self::A),
            "b" | "B" => Ok(Self::B),
            "c" | "C" => Ok(Self::C),
            "d" | "D" => Ok(Self::D),
            _ => Err(ValueError::UnknownChoice {
                name: "deck",
                expected: "a, b, c, d",
            }),
        }
    }
}

impl fmt::Display for DeckId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.letter())
    }
}

#[cfg(test)]
mod tests {
    use super::DeckId;

    #[test]
    fn letters_round_trip_and_indices_are_dense() {
        for (expected, deck) in DeckId::ALL.into_iter().enumerate() {
            assert_eq!(deck.index(), expected);
            assert_eq!(DeckId::parse(&deck.to_string()), Ok(deck));
        }
        assert_eq!(DeckId::parse("b"), Ok(DeckId::B));
        assert_eq!(DeckId::parse("d"), Ok(DeckId::D));
        assert!(DeckId::parse("e").is_err());
        assert!(DeckId::parse("").is_err());
    }
}
