//! The beatgrid of a track: its tempo and the markers that place its beats.
//!
//! Behaviour notes: `docs/behavior/beatgrid.md`. The grid is a fixed-size value, so the
//! processor can edit it without allocating.

use crate::ValueError;

/// Tempo of a track in beats per minute.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Bpm(f64);

impl Bpm {
    pub const MIN: f64 = 20.0;
    pub const MAX: f64 = 400.0;

    pub fn new(value: f64) -> Result<Self, ValueError> {
        ValueError::check("bpm", value, Self::MIN, Self::MAX).map(Self)
    }

    pub fn value(self) -> f64 {
        self.0
    }

    /// Length of one beat in seconds.
    pub fn beat_seconds(self) -> f64 {
        60.0 / self.0
    }
}

/// Most markers a grid holds.
pub const MAX_MARKERS: usize = 128;

/// Beats a bar has; the first of each bar counted from a marker is a downbeat.
pub const BEATS_PER_BAR: i64 = 4;

/// Most beats [`BeatGrid::for_each_beat`] visits in one call, whatever span it is given.
pub const MAX_BEATS_VISITED: usize = 4_096;

/// One beat of a grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Beat {
    /// Position in seconds of the track.
    pub seconds: f64,
    /// The first beat of a bar.
    pub downbeat: bool,
    /// The beat is a marker.
    pub marker: bool,
}

/// Positions closer than this, in seconds, are the same place.
const SAME_PLACE: f64 = 0.001;

/// Tempo, markers and lock of one track. Markers are positions in seconds of the track, in
/// ascending order and at least [`SAME_PLACE`] apart. Every edit returns whether it changed
/// the grid; while the grid is locked none does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeatGrid {
    bpm: Option<Bpm>,
    markers: [f64; MAX_MARKERS],
    len: usize,
    locked: bool,
}

impl Default for BeatGrid {
    fn default() -> Self {
        Self {
            bpm: None,
            markers: [0.0; MAX_MARKERS],
            len: 0,
            locked: false,
        }
    }
}

impl BeatGrid {
    pub fn bpm(&self) -> Option<Bpm> {
        self.bpm
    }

    pub fn markers(&self) -> &[f64] {
        self.markers.get(..self.len).unwrap_or(&[])
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    pub fn set_locked(&mut self, locked: bool) {
        self.locked = locked;
    }

    /// Sets the tempo. A grid without markers also gets one at the start of the track.
    pub fn set_bpm(&mut self, bpm: Bpm) -> bool {
        if self.locked {
            return false;
        }
        let added = self.len == 0 && self.add_marker(0.0);
        let changed = self.bpm != Some(bpm);
        self.bpm = Some(bpm);
        added || changed
    }

    /// Multiplies the tempo by `factor`. Nothing changes without a tempo or when the result
    /// leaves the accepted range.
    pub fn scale_bpm(&mut self, factor: f64) -> bool {
        self.replace_bpm(|bpm| bpm.value() * factor)
    }

    /// Adds `amount` to the tempo, under the same conditions as [`Self::scale_bpm`].
    pub fn step_bpm(&mut self, amount: f64) -> bool {
        self.replace_bpm(|bpm| bpm.value() + amount)
    }

    fn replace_bpm(&mut self, change: impl FnOnce(Bpm) -> f64) -> bool {
        match self.bpm.map(change).map(Bpm::new) {
            Some(Ok(bpm)) => self.set_bpm(bpm),
            _ => false,
        }
    }

    /// Adds a marker at `at` seconds, unless one is already there or the grid is full.
    pub fn add_marker(&mut self, at: f64) -> bool {
        if self.locked || !at.is_finite() || at < 0.0 || self.len >= MAX_MARKERS {
            return false;
        }
        let markers = self.markers();
        if markers
            .iter()
            .any(|marker| (marker - at).abs() < SAME_PLACE)
        {
            return false;
        }
        let index = markers.partition_point(|marker| *marker < at);
        self.markers.copy_within(index..self.len, index + 1);
        self.markers[index] = at;
        self.len += 1;
        true
    }

    /// Deletes the marker on or before `at` seconds.
    pub fn delete_marker(&mut self, at: f64) -> bool {
        let Some(index) = self.on_or_before(at) else {
            return false;
        };
        if self.locked {
            return false;
        }
        self.markers.copy_within(index + 1..self.len, index);
        self.len -= 1;
        true
    }

    /// Moves the marker on or before `at` seconds by `by` seconds. It stays between zero and
    /// `end` and at least [`SAME_PLACE`] from its neighbours.
    pub fn move_marker(&mut self, at: f64, by: f64, end: f64) -> bool {
        let Some(index) = self.on_or_before(at) else {
            return false;
        };
        if self.locked || !by.is_finite() || !end.is_finite() {
            return false;
        }
        let low = match index.checked_sub(1) {
            Some(before) => self.markers[before] + SAME_PLACE,
            None => 0.0,
        };
        let high = if index + 1 < self.len {
            self.markers[index + 1] - SAME_PLACE
        } else {
            end
        };
        if low > high {
            return false;
        }
        let moved = (self.markers[index] + by).clamp(low, high);
        let changed = moved != self.markers[index];
        self.markers[index] = moved;
        changed
    }

    /// The last marker before `at` seconds, skipping one the position stands on.
    pub fn previous_marker(&self, at: f64) -> Option<f64> {
        self.markers()
            .iter()
            .rev()
            .find(|marker| **marker <= at - SAME_PLACE)
            .copied()
    }

    /// The first marker after `at` seconds, skipping one the position stands on.
    pub fn next_marker(&self, at: f64) -> Option<f64> {
        self.markers()
            .iter()
            .find(|marker| **marker >= at + SAME_PLACE)
            .copied()
    }

    /// Calls `visit` for each beat from `from` to `to` seconds, in order. Each marker starts
    /// a run of beats at the tempo of the grid up to the next marker; the beats before the
    /// first marker count back from it. A grid without tempo or markers has no beats.
    pub fn for_each_beat(&self, from: f64, to: f64, mut visit: impl FnMut(Beat)) {
        let (Some(bpm), Some(first)) = (self.bpm, self.markers().first()) else {
            return;
        };
        if !from.is_finite() || !to.is_finite() || from > to {
            return;
        }
        let beat = bpm.beat_seconds();
        let mut visited = 0;
        let markers = self.markers();
        for (index, marker) in markers.iter().enumerate() {
            // The run of the first marker reaches back to the start of the span.
            let start = if index == 0 {
                from.min(*first)
            } else {
                *marker
            };
            let end = markers.get(index + 1).copied().unwrap_or(f64::INFINITY);
            let low = from.max(start);
            let high = to.min(end);
            if low > high {
                continue;
            }
            let first_beat = ((low - marker) / beat).ceil() as i64;
            let last_beat = ((high - marker) / beat).floor() as i64;
            for count in first_beat..=last_beat {
                let seconds = marker + count as f64 * beat;
                // The next marker starts its own run.
                if seconds >= end || visited >= MAX_BEATS_VISITED {
                    break;
                }
                visit(Beat {
                    seconds,
                    downbeat: count.rem_euclid(BEATS_PER_BAR) == 0,
                    marker: count == 0,
                });
                visited += 1;
            }
        }
    }

    /// Index of the marker a position stands on, or of the last one before it.
    fn on_or_before(&self, at: f64) -> Option<usize> {
        if !at.is_finite() {
            return None;
        }
        let count = self
            .markers()
            .partition_point(|marker| *marker < at + SAME_PLACE);
        count.checked_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use super::{BeatGrid, Bpm, MAX_MARKERS};

    fn bpm(value: f64) -> Bpm {
        Bpm::new(value).unwrap()
    }

    fn grid_with(markers: &[f64]) -> BeatGrid {
        let mut grid = BeatGrid::default();
        for marker in markers {
            assert!(grid.add_marker(*marker));
        }
        grid
    }

    #[test]
    fn the_tempo_accepts_twenty_to_four_hundred() {
        assert!(Bpm::new(20.0).is_ok());
        assert!(Bpm::new(400.0).is_ok());
        assert!(Bpm::new(19.99).is_err());
        assert!(Bpm::new(400.01).is_err());
        assert!(Bpm::new(f64::NAN).is_err());
        assert_eq!(bpm(120.0).beat_seconds(), 0.5);
    }

    #[test]
    fn a_tempo_on_an_empty_grid_puts_a_marker_at_the_start() {
        let mut grid = BeatGrid::default();
        assert!(grid.set_bpm(bpm(128.0)));
        assert_eq!(grid.markers(), &[0.0]);
        assert!(!grid.set_bpm(bpm(128.0)));

        let mut grid = grid_with(&[1.5]);
        grid.set_bpm(bpm(128.0));
        assert_eq!(grid.markers(), &[1.5]);
    }

    #[test]
    fn the_same_tempo_on_a_grid_without_markers_still_changes_it() {
        let mut grid = BeatGrid::default();
        grid.set_bpm(bpm(128.0));
        assert!(grid.delete_marker(0.0));
        assert!(
            grid.set_bpm(bpm(128.0)),
            "the marker at the start is a change"
        );
        assert_eq!(grid.markers(), &[0.0]);
    }

    #[test]
    fn double_halve_and_step_stay_in_range() {
        let mut grid = BeatGrid::default();
        assert!(!grid.scale_bpm(2.0), "no tempo yet");
        assert!(!grid.step_bpm(1.0), "no tempo yet");
        grid.set_bpm(bpm(150.0));
        assert!(grid.scale_bpm(2.0));
        assert_eq!(grid.bpm(), Some(bpm(300.0)));
        assert!(!grid.scale_bpm(2.0), "600 is out of range");
        assert_eq!(grid.bpm(), Some(bpm(300.0)));
        assert!(grid.scale_bpm(0.5));
        assert!(grid.step_bpm(-0.5));
        assert_eq!(grid.bpm(), Some(bpm(149.5)));
        grid.set_bpm(bpm(20.0));
        assert!(!grid.step_bpm(-0.01));
        assert!(!grid.scale_bpm(f64::NAN));
    }

    #[test]
    fn markers_stay_sorted_and_apart() {
        let mut grid = grid_with(&[4.0, 1.0, 2.5]);
        assert_eq!(grid.markers(), &[1.0, 2.5, 4.0]);
        assert!(!grid.add_marker(2.5004), "within a millisecond of a marker");
        assert!(grid.add_marker(2.502));
        assert!(!grid.add_marker(-1.0));
        assert!(!grid.add_marker(f64::INFINITY));
        assert_eq!(grid.markers(), &[1.0, 2.5, 2.502, 4.0]);
    }

    #[test]
    fn a_full_grid_takes_no_more_markers() {
        let mut grid = BeatGrid::default();
        for index in 0..MAX_MARKERS {
            assert!(grid.add_marker(index as f64));
        }
        assert!(!grid.add_marker(1_000.0));
        assert_eq!(grid.markers().len(), MAX_MARKERS);
    }

    #[test]
    fn delete_takes_the_marker_on_or_before_the_position() {
        let mut grid = grid_with(&[1.0, 2.0, 3.0]);
        assert!(!grid.delete_marker(0.5), "before the first marker");
        assert!(grid.delete_marker(2.0005), "on the marker at 2");
        assert_eq!(grid.markers(), &[1.0, 3.0]);
        assert!(grid.delete_marker(2.9));
        assert_eq!(grid.markers(), &[3.0]);
        assert!(!grid.delete_marker(f64::NAN));
    }

    #[test]
    fn a_moved_marker_stays_between_its_neighbours_and_in_the_track() {
        let mut grid = grid_with(&[1.0, 2.0, 3.0]);
        assert!(grid.move_marker(2.5, 0.25, 10.0));
        assert_eq!(grid.markers(), &[1.0, 2.25, 3.0]);
        assert!(grid.move_marker(2.5, 5.0, 10.0));
        assert_eq!(grid.markers()[1], 3.0 - 0.001);
        assert!(grid.move_marker(1.0, -5.0, 10.0));
        assert_eq!(grid.markers()[0], 0.0);
        assert!(grid.move_marker(9.0, 20.0, 10.0));
        assert_eq!(grid.markers()[2], 10.0);
        assert!(!grid.move_marker(10.0, 1.0, 10.0), "already at the end");
        assert!(!BeatGrid::default().move_marker(1.0, 0.1, 10.0));
    }

    #[test]
    fn previous_and_next_skip_the_marker_under_the_position() {
        let grid = grid_with(&[1.0, 2.0, 3.0]);
        assert_eq!(grid.previous_marker(2.0), Some(1.0));
        assert_eq!(grid.previous_marker(2.4), Some(2.0));
        assert_eq!(grid.next_marker(2.0), Some(3.0));
        assert_eq!(grid.next_marker(3.0), None);
        assert_eq!(grid.previous_marker(1.0), None);
        assert_eq!(grid.next_marker(f64::NAN), None);
    }

    fn beats(grid: &BeatGrid, from: f64, to: f64) -> Vec<(f64, bool, bool)> {
        let mut seen = Vec::new();
        grid.for_each_beat(from, to, |beat| {
            seen.push((beat.seconds, beat.downbeat, beat.marker));
        });
        seen
    }

    #[test]
    fn beats_run_from_each_marker_at_the_one_tempo() {
        let mut grid = grid_with(&[1.0, 3.2]);
        grid.set_bpm(bpm(120.0));
        assert_eq!(
            beats(&grid, 0.0, 4.0),
            [
                (0.0, false, false),
                (0.5, false, false),
                (1.0, true, true),
                (1.5, false, false),
                (2.0, false, false),
                (2.5, false, false),
                (3.0, true, false),
                // The second marker restarts the count.
                (3.2, true, true),
                (3.7, false, false),
            ]
        );
        // Counting back from the first marker, the bar before it starts at -1.0.
        assert_eq!(beats(&grid, -1.0, -1.0), [(-1.0, true, false)]);
    }

    #[test]
    fn no_tempo_no_markers_or_a_bad_span_give_no_beats() {
        let mut grid = BeatGrid::default();
        grid.set_bpm(bpm(120.0));
        assert!(grid.delete_marker(0.0));
        assert!(beats(&grid, 0.0, 10.0).is_empty());
        assert!(beats(&grid_with(&[0.0]), 0.0, 10.0).is_empty());
        let mut grid = grid_with(&[0.0]);
        grid.set_bpm(bpm(120.0));
        assert!(beats(&grid, 5.0, 1.0).is_empty());
        assert!(beats(&grid, f64::NAN, 1.0).is_empty());
    }

    #[test]
    fn a_huge_span_visits_a_bounded_number_of_beats() {
        let mut grid = grid_with(&[0.0]);
        grid.set_bpm(bpm(400.0));
        assert_eq!(beats(&grid, 0.0, 1.0e9).len(), super::MAX_BEATS_VISITED);
    }

    #[test]
    fn a_locked_grid_changes_only_its_lock() {
        let mut grid = grid_with(&[1.0, 2.0]);
        grid.set_bpm(bpm(120.0));
        grid.set_locked(true);
        let before = grid;
        assert!(!grid.set_bpm(bpm(90.0)));
        assert!(!grid.scale_bpm(2.0));
        assert!(!grid.step_bpm(1.0));
        assert!(!grid.add_marker(5.0));
        assert!(!grid.delete_marker(2.0));
        assert!(!grid.move_marker(2.0, 0.1, 10.0));
        assert_eq!(grid, before);
        assert_eq!(grid.next_marker(1.0), Some(2.0));
        grid.set_locked(false);
        assert!(grid.add_marker(5.0));
    }
}
