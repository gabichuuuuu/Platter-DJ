//! The rules of a smart playlist: what they mean, how a row of the file holds one, and the SQL
//! that keeps the tracks passing them. Behaviour note: `docs/behavior/library.md`, *Smart
//! playlists*.

use rusqlite::types::Value;

use crate::text::contains;
use crate::{LibraryError, MAX_COLOUR, MAX_KEY, MAX_TAG_CHARS};

/// Most rules one smart playlist holds.
pub const MAX_RULES: usize = 16;

/// A tag that words are looked for in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TagField {
    Title,
    Artist,
    Album,
    Genre,
}

/// A value of a track a range applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RangeField {
    /// Beats per minute.
    Tempo,
    /// Length in seconds.
    Duration,
    /// 0 to [`crate::MAX_RATING`].
    Rating,
    PlayCount,
    /// Seconds since 1 January 1970 UTC, as [`crate::Timestamp`].
    Imported,
    /// Seconds since 1 January 1970 UTC; a track never played has none.
    LastPlayed,
}

/// One condition a track must pass to be in a smart playlist.
#[derive(Clone, Debug, PartialEq)]
pub enum Rule {
    /// Every word, separated by white space, is found in the tag, whatever its case.
    Words { field: TagField, words: String },
    /// The value lies between the bounds, both included; at least one bound is given.
    Range {
        field: RangeField,
        low: Option<f64>,
        high: Option<f64>,
    },
    /// The colour tag, 1 to [`MAX_COLOUR`].
    Colour(u8),
    /// The key, 0 to [`MAX_KEY`], numbered as [`crate::Track::key`].
    Key(u8),
    /// The file was missing at the last consistency check, or was not.
    Missing(bool),
    /// The track has been analysed, or has not.
    Analysed(bool),
}

impl Rule {
    pub(crate) fn validate(&self) -> Result<(), LibraryError> {
        let valid = match self {
            Self::Words { words, .. } => {
                !words.trim().is_empty() && words.chars().count() <= MAX_TAG_CHARS
            }
            Self::Range { low, high, .. } => {
                let finite = |bound: &Option<f64>| bound.is_none_or(f64::is_finite);
                finite(low)
                    && finite(high)
                    && match (low, high) {
                        (None, None) => false,
                        (Some(low), Some(high)) => low <= high,
                        _ => true,
                    }
            }
            Self::Colour(colour) => (1..=MAX_COLOUR).contains(colour),
            Self::Key(key) => *key <= MAX_KEY,
            Self::Missing(_) | Self::Analysed(_) => true,
        };
        if valid {
            Ok(())
        } else {
            Err(LibraryError::InvalidRule)
        }
    }

    /// The row of the `rule` table that holds it: the field code, the two numbers and the
    /// words.
    pub(crate) fn to_row(&self) -> (i64, Option<f64>, Option<f64>, Option<String>) {
        match self {
            Self::Words { field, words } => (tag_code(*field), None, None, Some(words.clone())),
            Self::Range { field, low, high } => (range_code(*field), *low, *high, None),
            Self::Colour(colour) => (COLOUR, Some(f64::from(*colour)), None, None),
            Self::Key(key) => (KEY, Some(f64::from(*key)), None, None),
            Self::Missing(set) => (MISSING, Some(flag(*set)), None, None),
            Self::Analysed(set) => (ANALYSED, Some(flag(*set)), None, None),
        }
    }

    /// The rule a row holds, or none when this version cannot read it.
    pub(crate) fn from_row(
        code: i64,
        low: Option<f64>,
        high: Option<f64>,
        words: Option<String>,
    ) -> Option<Self> {
        let small = |value: Option<f64>| {
            value
                .filter(|value| value.fract() == 0.0 && (0.0..=255.0).contains(value))
                .map(|value| value as u8)
        };
        let rule = match code {
            TITLE..=GENRE => Self::Words {
                field: tag_field(code)?,
                words: words?,
            },
            TEMPO..=LAST_PLAYED => Self::Range {
                field: range_field(code)?,
                low,
                high,
            },
            COLOUR => Self::Colour(small(low)?),
            KEY => Self::Key(small(low)?),
            MISSING => Self::Missing(small(low)? != 0),
            ANALYSED => Self::Analysed(small(low)? != 0),
            _ => return None,
        };
        rule.validate().ok().map(|()| rule)
    }

    /// Appends to `filter` the condition on the track `t`, and its values to `values`.
    pub(crate) fn push_sql(&self, filter: &mut String, values: &mut Vec<Value>) {
        match self {
            Self::Words { field, words } => {
                let column = match field {
                    TagField::Title => "t.title_key",
                    TagField::Artist => "t.artist_key",
                    TagField::Album => "t.album_key",
                    TagField::Genre => "t.genre_key",
                };
                for word in words.split_whitespace() {
                    values.push(contains(word).into());
                    let n = values.len();
                    filter.push_str(&format!(" AND {column} LIKE ?{n} ESCAPE '\\'"));
                }
            }
            Self::Range { field, low, high } => {
                let column = match field {
                    RangeField::Tempo => "t.bpm",
                    RangeField::Duration => "t.seconds",
                    RangeField::Rating => "t.rating",
                    RangeField::PlayCount => "t.play_count",
                    RangeField::Imported => "t.imported",
                    RangeField::LastPlayed => "t.last_played",
                };
                // A tempo or a length that is not positive reads as none, as for a track.
                let known = match field {
                    RangeField::Tempo | RangeField::Duration => format!("{column} > 0"),
                    _ => format!("{column} IS NOT NULL"),
                };
                filter.push_str(&format!(" AND {known}"));
                for (bound, operator) in [(low, ">="), (high, "<=")] {
                    if let Some(bound) = bound {
                        values.push((*bound).into());
                        let n = values.len();
                        filter.push_str(&format!(" AND {column} {operator} ?{n}"));
                    }
                }
            }
            Self::Colour(colour) => {
                values.push(i64::from(*colour).into());
                filter.push_str(&format!(" AND t.colour = ?{}", values.len()));
            }
            Self::Key(key) => {
                values.push(i64::from(*key).into());
                filter.push_str(&format!(" AND t.key = ?{}", values.len()));
            }
            Self::Missing(set) => {
                filter.push_str(if *set {
                    " AND t.missing <> 0"
                } else {
                    " AND t.missing = 0"
                });
            }
            Self::Analysed(set) => {
                filter.push_str(if *set {
                    " AND t.analysed <> 0"
                } else {
                    " AND t.analysed = 0"
                });
            }
        }
    }
}

const TITLE: i64 = 0;
const ARTIST: i64 = 1;
const ALBUM: i64 = 2;
const GENRE: i64 = 3;
const TEMPO: i64 = 10;
const DURATION: i64 = 11;
const RATING: i64 = 12;
const PLAY_COUNT: i64 = 13;
const IMPORTED: i64 = 14;
const LAST_PLAYED: i64 = 15;
const COLOUR: i64 = 20;
const KEY: i64 = 21;
const MISSING: i64 = 30;
const ANALYSED: i64 = 31;

fn flag(set: bool) -> f64 {
    if set { 1.0 } else { 0.0 }
}

fn tag_code(field: TagField) -> i64 {
    match field {
        TagField::Title => TITLE,
        TagField::Artist => ARTIST,
        TagField::Album => ALBUM,
        TagField::Genre => GENRE,
    }
}

fn tag_field(code: i64) -> Option<TagField> {
    Some(match code {
        TITLE => TagField::Title,
        ARTIST => TagField::Artist,
        ALBUM => TagField::Album,
        GENRE => TagField::Genre,
        _ => return None,
    })
}

fn range_code(field: RangeField) -> i64 {
    match field {
        RangeField::Tempo => TEMPO,
        RangeField::Duration => DURATION,
        RangeField::Rating => RATING,
        RangeField::PlayCount => PLAY_COUNT,
        RangeField::Imported => IMPORTED,
        RangeField::LastPlayed => LAST_PLAYED,
    }
}

fn range_field(code: i64) -> Option<RangeField> {
    Some(match code {
        TEMPO => RangeField::Tempo,
        DURATION => RangeField::Duration,
        RATING => RangeField::Rating,
        PLAY_COUNT => RangeField::PlayCount,
        IMPORTED => RangeField::Imported,
        LAST_PLAYED => RangeField::LastPlayed,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_comes_back_from_its_row() {
        let rules = [
            Rule::Words {
                field: TagField::Genre,
                words: "deep house".into(),
            },
            Rule::Range {
                field: RangeField::Tempo,
                low: Some(120.0),
                high: Some(126.5),
            },
            Rule::Range {
                field: RangeField::LastPlayed,
                low: None,
                high: Some(1_700_000_000.0),
            },
            Rule::Colour(3),
            Rule::Key(23),
            Rule::Missing(true),
            Rule::Analysed(false),
        ];
        for rule in rules {
            let (code, low, high, words) = rule.to_row();
            assert_eq!(Rule::from_row(code, low, high, words), Some(rule));
        }
    }

    #[test]
    fn rows_this_version_cannot_read_give_none() {
        assert_eq!(Rule::from_row(99, Some(1.0), None, None), None);
        assert_eq!(Rule::from_row(COLOUR, Some(9.0), None, None), None);
        assert_eq!(Rule::from_row(KEY, Some(2.5), None, None), None);
        assert_eq!(Rule::from_row(TITLE, None, None, None), None);
        assert_eq!(Rule::from_row(TEMPO, None, None, None), None);
        assert_eq!(Rule::from_row(TEMPO, Some(130.0), Some(120.0), None), None);
    }

    #[test]
    fn invalid_rules_are_refused() {
        for rule in [
            Rule::Words {
                field: TagField::Title,
                words: "  ".into(),
            },
            Rule::Range {
                field: RangeField::Duration,
                low: Some(f64::NAN),
                high: None,
            },
            Rule::Colour(0),
            Rule::Key(MAX_KEY + 1),
        ] {
            assert!(matches!(rule.validate(), Err(LibraryError::InvalidRule)));
        }
    }
}
