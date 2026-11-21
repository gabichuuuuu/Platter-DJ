//! The four stems of a track: `docs/behavior/stems.md`.

use std::fmt;

use crate::ValueError;

/// A part of a track that a stem deck plays apart from the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stem {
    Drums,
    Bass,
    Other,
    Vocals,
}

impl Stem {
    /// In the order of the lanes, of the files of a set and of per-stem arrays.
    pub const ALL: [Self; 4] = [Self::Drums, Self::Bass, Self::Other, Self::Vocals];
    pub const COUNT: usize = Self::ALL.len();
    /// The names [`Self::parse`] reads, in the order of [`Self::ALL`].
    pub const NAMES: [&'static str; 4] = ["drums", "bass", "other", "vocals"];

    /// Position of the stem in per-stem arrays.
    pub fn index(self) -> usize {
        match self {
            Self::Drums => 0,
            Self::Bass => 1,
            Self::Other => 2,
            Self::Vocals => 3,
        }
    }

    pub fn name(self) -> &'static str {
        Self::NAMES[self.index()]
    }

    /// Reads the name of a stem, in any case.
    pub fn parse(text: &str) -> Result<Self, ValueError> {
        Self::ALL
            .into_iter()
            .find(|stem| stem.name().eq_ignore_ascii_case(text))
            .ok_or(ValueError::UnknownChoice {
                name: "stem",
                expected: "drums, bass, other, vocals",
            })
    }
}

impl fmt::Display for Stem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::Stem;

    #[test]
    fn names_round_trip_in_any_case_and_indices_are_dense() {
        for (expected, stem) in Stem::ALL.into_iter().enumerate() {
            assert_eq!(stem.index(), expected);
            assert_eq!(Stem::parse(stem.name()), Ok(stem));
            assert_eq!(Stem::parse(&stem.name().to_uppercase()), Ok(stem));
        }
        assert!(Stem::parse("piano").is_err());
    }
}
