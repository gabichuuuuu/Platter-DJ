//! What Platter keeps of a track between sessions, its beatgrid and its stored cues, and the
//! text that holds them in the collection and in the tags of the file.
//!
//! Behaviour note: `docs/behavior/library.md`, *Cues, grid and analysis* and *The text of a
//! track*.

use std::fmt::Write as _;

use crate::{BeatGrid, Bpm, Cue, CueKind, CueName, Cues, HotcueSlot, MAX_MARKERS};

/// First line of the text, with the version this code writes and reads.
const HEADER: &str = "platter-track 1";
/// Longest text read, in bytes.
pub const MAX_TRACK_TEXT: usize = 64 * 1024;
/// Latest place in a track a position or a length may name, in seconds: a day.
const LATEST: f64 = 86_400.0;

/// The beatgrid and the stored cues of a track, without the selection.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrackMemory {
    pub grid: BeatGrid,
    pub cues: Cues,
}

/// Why a text is not a track memory Platter reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrackTextError {
    TooLong,
    /// The first line names another format, or a version this code does not know.
    Header,
    /// The line, counted from 1, cannot be read.
    Line(usize),
}

impl std::fmt::Display for TrackTextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLong => write!(
                f,
                "the text of the track is longer than {MAX_TRACK_TEXT} bytes"
            ),
            Self::Header => write!(f, "the text is not a track text this version reads"),
            Self::Line(line) => write!(f, "line {line} of the text of the track cannot be read"),
        }
    }
}

impl std::error::Error for TrackTextError {}

impl TrackMemory {
    /// Whether there is nothing to keep: no tempo, no marker, no lock and no cue.
    pub fn is_empty(&self) -> bool {
        self.grid.is_empty() && !self.grid.is_locked() && self.cues.iter().next().is_none()
    }

    /// The text of the memory, which [`Self::parse`] reads back to the same value.
    pub fn to_text(&self) -> String {
        let mut text = String::from(HEADER);
        text.push('\n');
        // Writing to a String does not fail.
        if let Some(bpm) = self.grid.bpm() {
            let _ = writeln!(text, "bpm {}", bpm.value());
        }
        if self.grid.is_locked() {
            text.push_str("lock\n");
        }
        for marker in self.grid.markers() {
            let _ = writeln!(text, "marker {marker}");
        }
        for (slot, cue) in self.cues.iter() {
            let _ = writeln!(
                text,
                "cue {} {} {} {} {}",
                slot.number(),
                cue.kind().name(),
                cue.position,
                cue.length(),
                cue.name.as_str()
            );
        }
        text
    }

    /// Reads a text [`Self::to_text`] wrote. Anything else is refused whole.
    pub fn parse(text: &str) -> Result<Self, TrackTextError> {
        if text.len() > MAX_TRACK_TEXT {
            return Err(TrackTextError::TooLong);
        }
        let mut lines = text.lines();
        if lines.next().map(str::trim_end) != Some(HEADER) {
            return Err(TrackTextError::Header);
        }
        let mut bpm = None;
        let mut locked = false;
        // Each marker with the line it came from.
        let mut markers: Vec<(f64, usize)> = Vec::new();
        let mut slots: [Option<Cue>; HotcueSlot::COUNT] = [None; HotcueSlot::COUNT];
        for (index, line) in lines.enumerate() {
            // The header is line 1.
            let bad = TrackTextError::Line(index + 2);
            if line.trim().is_empty() {
                continue;
            }
            let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
            match word {
                "bpm" if bpm.is_none() => {
                    bpm = Some(Bpm::new(number(rest).ok_or(bad.clone())?).map_err(|_| bad)?);
                }
                "lock" if !locked && rest.is_empty() => locked = true,
                "marker" if markers.len() < MAX_MARKERS => {
                    markers.push((place(rest).ok_or(bad)?, index + 2));
                }
                "cue" => {
                    let (slot, cue) = cue_line(rest).ok_or(bad.clone())?;
                    let place = slots.get_mut(slot.index()).ok_or(bad.clone())?;
                    if place.is_some() {
                        return Err(bad);
                    }
                    *place = Some(cue);
                }
                _ => return Err(bad),
            }
        }
        let mut grid = BeatGrid::default();
        for (marker, line) in markers {
            // A marker on the place of another is refused by the grid.
            if !grid.add_marker(marker) {
                return Err(TrackTextError::Line(line));
            }
        }
        if let Some(bpm) = bpm {
            grid.set_bpm(bpm);
        }
        grid.set_locked(locked);
        Ok(Self {
            grid,
            cues: Cues::from_slots(slots),
        })
    }
}

/// A finite number written with a full stop.
fn number(text: &str) -> Option<f64> {
    let value: f64 = text.trim().parse().ok()?;
    value.is_finite().then_some(value)
}

/// A place in a track, in seconds.
fn place(text: &str) -> Option<f64> {
    number(text).filter(|value| (0.0..=LATEST).contains(value))
}

fn cue_line(rest: &str) -> Option<(HotcueSlot, Cue)> {
    let mut parts = rest.splitn(5, ' ');
    let slot: u8 = parts.next()?.parse().ok()?;
    let slot = HotcueSlot::from_number(f64::from(slot)).ok()?;
    let kind = parts.next()?;
    let kind = CueKind::ALL
        .into_iter()
        .find(|candidate| candidate.name() == kind)?;
    let position = place(parts.next()?)?;
    let length = place(parts.next()?)?;
    let name = CueName::new(parts.next().unwrap_or(""));
    Some((slot, Cue::of_kind(position, kind, length, name)?))
}
