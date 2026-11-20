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

/// The tempo of a marker that has none of its own.
const NO_TEMPO: u16 = 0;

/// The tempo of a marker is kept in hundredths of a beat per minute: 400 BPM is 40,000.
const TEMPO_STEPS_PER_BPM: f64 = 100.0;

/// The tempo of a marker as it is kept, to the nearest hundredth of a beat per minute.
fn kept_tempo(bpm: Bpm) -> u16 {
    // Within 20 to 400 BPM the result fits: at most 40,000.
    (bpm.value() * TEMPO_STEPS_PER_BPM).round() as u16
}

/// The tempo a marker holds of its own, if it holds one.
fn own_tempo(tempo: u16) -> Option<Bpm> {
    (tempo != NO_TEMPO)
        .then(|| Bpm::new(f64::from(tempo) / TEMPO_STEPS_PER_BPM).ok())
        .flatten()
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
    /// The tempo of each marker up to the next one, in hundredths of a beat per minute, or
    /// [`NO_TEMPO`] for a marker with the tempo of the grid. Small numbers keep the grid
    /// small: it is copied often, on the stack too.
    tempos: [u16; MAX_MARKERS],
    len: usize,
    locked: bool,
}

impl Default for BeatGrid {
    fn default() -> Self {
        Self {
            bpm: None,
            markers: [0.0; MAX_MARKERS],
            tempos: [NO_TEMPO; MAX_MARKERS],
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

    /// The tempo each marker holds of its own, in the order of [`Self::markers`].
    pub fn marker_tempos(&self) -> impl Iterator<Item = Option<Bpm>> + '_ {
        self.tempos
            .get(..self.len)
            .unwrap_or(&[])
            .iter()
            .map(|tempo| own_tempo(*tempo))
    }

    /// The tempo of the run of beats that holds a position of `at` seconds: that of its
    /// marker, or of the grid. Before the first marker, that of the first marker.
    pub fn bpm_at(&self, at: f64) -> Option<Bpm> {
        let bpm = self.bpm?;
        if !at.is_finite() || self.len == 0 {
            return Some(bpm);
        }
        let run = self.markers().partition_point(|marker| *marker <= at);
        Some(self.tempo_of(run.saturating_sub(1)).unwrap_or(bpm))
    }

    /// The tempo of the run of the marker at `index`.
    fn tempo_of(&self, index: usize) -> Option<Bpm> {
        self.tempos
            .get(index)
            .and_then(|tempo| own_tempo(*tempo))
            .or(self.bpm)
    }

    /// Gives the marker on or before `at` seconds a tempo of its own up to the next marker,
    /// or with `None` the tempo of the grid again, to the nearest hundredth of a beat per
    /// minute. A grid without a tempo takes none.
    pub fn set_marker_bpm(&mut self, at: f64, bpm: Option<Bpm>) -> bool {
        let Some(index) = self.on_or_before(at) else {
            return false;
        };
        if self.locked || self.bpm.is_none() {
            return false;
        }
        let tempo = bpm.map_or(NO_TEMPO, kept_tempo);
        let changed = self.tempos[index] != tempo;
        self.tempos[index] = tempo;
        changed
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

    /// Multiplies the tempo by `factor`, with the tempos of the markers that have their own.
    /// Nothing changes without a tempo or when a result leaves the accepted range.
    pub fn scale_bpm(&mut self, factor: f64) -> bool {
        self.replace_bpm(|bpm| bpm.value() * factor)
    }

    /// Adds `amount` to the tempo, under the same conditions as [`Self::scale_bpm`].
    pub fn step_bpm(&mut self, amount: f64) -> bool {
        self.replace_bpm(|bpm| bpm.value() + amount)
    }

    fn replace_bpm(&mut self, change: impl Fn(Bpm) -> f64) -> bool {
        let Some(Ok(bpm)) = self.bpm.map(&change).map(Bpm::new) else {
            return false;
        };
        let mut tempos = self.tempos;
        for tempo in tempos.iter_mut().take(self.len) {
            if let Some(own) = own_tempo(*tempo) {
                let Ok(changed) = Bpm::new(change(own)) else {
                    return false;
                };
                *tempo = kept_tempo(changed);
            }
        }
        if self.locked {
            return false;
        }
        let changed = tempos != self.tempos;
        self.tempos = tempos;
        self.set_bpm(bpm) || changed
    }

    /// No tempo and no markers.
    pub fn is_empty(&self) -> bool {
        self.bpm.is_none() && self.len == 0
    }

    /// Replaces the tempo and the markers with `bpm` and one marker at `marker` seconds, or
    /// with neither when `found` is `None`, as a reset to the result of an analysis does.
    pub fn replace(&mut self, found: Option<(Bpm, f64)>) -> bool {
        if self.locked {
            return false;
        }
        let before = *self;
        self.bpm = None;
        self.len = 0;
        self.tempos = [NO_TEMPO; MAX_MARKERS];
        if let Some((bpm, marker)) = found {
            self.add_marker(marker);
            self.bpm = Some(bpm);
        }
        *self != before
    }

    /// Sets the tempo and, on a grid without markers, a marker at `marker` seconds rather
    /// than at the start of the track.
    pub fn set_bpm_from(&mut self, bpm: Bpm, marker: f64) -> bool {
        if self.locked {
            return false;
        }
        let added = self.len == 0 && self.add_marker(marker);
        added | self.set_bpm(bpm)
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
        self.tempos.copy_within(index..self.len, index + 1);
        // A marker set inside a run keeps the tempo of that run; one set before the first
        // marker, that of the first, which its beats followed.
        self.tempos[index] = match index.checked_sub(1) {
            Some(before) => self.tempos[before],
            None if self.len > 0 => self.tempos[1],
            None => NO_TEMPO,
        };
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
        self.tempos.copy_within(index + 1..self.len, index);
        self.len -= 1;
        self.tempos[self.len] = NO_TEMPO;
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
    /// a run of beats at its tempo, or that of the grid, up to the next marker; the beats before the
    /// first marker count back from it. A grid without tempo or markers has no beats.
    pub fn for_each_beat(&self, from: f64, to: f64, mut visit: impl FnMut(Beat)) {
        let (Some(_), Some(first)) = (self.bpm, self.markers().first()) else {
            return;
        };
        if !from.is_finite() || !to.is_finite() || from > to {
            return;
        }
        let mut visited = 0;
        let markers = self.markers();
        for (index, marker) in markers.iter().enumerate() {
            let Some(beat) = self.tempo_of(index).map(Bpm::beat_seconds) else {
                return;
            };
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

    /// How far into its beat a position of `at` seconds is, from zero up to one. The beats
    /// are those of [`Self::for_each_beat`]: counted from the last marker on or before the
    /// position, or back from the first one. A grid without tempo or markers has no beats.
    pub fn phase_at(&self, at: f64) -> Option<f64> {
        let markers = self.markers();
        if !at.is_finite() {
            return None;
        }
        let run = markers
            .partition_point(|marker| *marker <= at)
            .saturating_sub(1);
        let marker = markers.get(run)?;
        let bpm = self.tempo_of(run)?;
        let phase = ((at - marker) / bpm.beat_seconds()).rem_euclid(1.0);
        // The remainder of a tiny negative number rounds up to one.
        Some(if phase < 1.0 { phase } else { 0.0 })
    }

    /// The beat nearest to a position of `at` seconds, in the run of beats the position is
    /// in, between zero and `end` seconds. Between two beats exactly, the later one. A grid
    /// without tempo or markers has no beats.
    pub fn nearest_beat(&self, at: f64, end: f64) -> Option<f64> {
        let beat = self.bpm_at(at)?.beat_seconds();
        let phase = self.phase_at(at)?;
        let (_, run_end) = self.run_at(at)?;
        if !end.is_finite() {
            return None;
        }
        let before = at - phase * beat;
        // The next marker is a beat too, and starts a run of its own.
        let after = (before + beat).min(run_end);
        let inside = |seconds: f64| (0.0..=end).contains(&seconds);
        match (inside(before), inside(after)) {
            // A nanosecond either way is a tie: rounding must not pick the earlier beat.
            (true, true) if at - before < after - at - 1e-9 => Some(before),
            (_, true) => Some(after),
            (true, false) => Some(before),
            (false, false) => None,
        }
    }

    /// Where the run of beats that holds a position of `at` seconds starts and ends: from
    /// its marker to the next one. The first run has no start and the last one no end.
    pub fn run_at(&self, at: f64) -> Option<(f64, f64)> {
        let markers = self.markers();
        if self.bpm.is_none() || markers.is_empty() || !at.is_finite() {
            return None;
        }
        let run = markers.partition_point(|marker| *marker <= at).max(1);
        let start = if run == 1 {
            f64::NEG_INFINITY
        } else {
            markers.get(run - 1).copied()?
        };
        Some((start, markers.get(run).copied().unwrap_or(f64::INFINITY)))
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
    fn the_phase_counts_from_the_marker_of_each_run() {
        assert_eq!(BeatGrid::default().phase_at(1.0), None);
        assert_eq!(grid_with(&[1.0]).phase_at(1.0), None, "no tempo");
        let mut grid = grid_with(&[1.0, 2.2]);
        grid.set_bpm(bpm(120.0));
        let phase = |at: f64| grid.phase_at(at).unwrap();
        assert_eq!(phase(1.0), 0.0);
        assert!((phase(1.25) - 0.5).abs() < 1e-12);
        assert!(
            (phase(0.875) - 0.75).abs() < 1e-12,
            "back from the first marker"
        );
        assert!(
            (phase(2.1) - 0.2).abs() < 1e-12,
            "still the run of the first marker"
        );
        assert_eq!(phase(2.2), 0.0, "the second marker starts its own run");
        assert!((phase(2.45) - 0.5).abs() < 1e-12);
        assert!((0.0..1.0).contains(&phase(1.0 - 1e-18)));
        assert_eq!(grid.phase_at(f64::NAN), None);
    }

    #[test]
    fn the_nearest_beat_stays_in_the_run_and_in_the_track() {
        assert_eq!(grid_with(&[1.0]).nearest_beat(1.2, 10.0), None, "no tempo");
        let mut grid = grid_with(&[0.1, 2.2]);
        grid.set_bpm(bpm(120.0));
        let near = |at: f64| grid.nearest_beat(at, 10.0).unwrap();
        assert!((near(1.3) - 1.1).abs() < 1e-12);
        assert!((near(1.4) - 1.6).abs() < 1e-12);
        assert!(
            (near(0.85) - 1.1).abs() < 1e-12,
            "the later one between two"
        );
        assert!((near(2.0) - 2.1).abs() < 1e-12);
        assert!(
            (near(2.18) - 2.2).abs() < 1e-12,
            "the next marker is the next beat"
        );
        assert!((near(2.3) - 2.2).abs() < 1e-12);
        assert!((near(0.05) - 0.1).abs() < 1e-12, "no beat before the track");
        assert!((grid.nearest_beat(9.95, 9.9).unwrap() - 9.7).abs() < 1e-12);
        assert_eq!(grid.nearest_beat(f64::NAN, 10.0), None);
    }

    #[test]
    fn a_run_goes_from_its_marker_to_the_next() {
        assert_eq!(grid_with(&[1.0]).run_at(1.0), None, "no tempo");
        let mut grid = grid_with(&[1.0, 2.2]);
        grid.set_bpm(bpm(120.0));
        assert_eq!(grid.run_at(0.5), Some((f64::NEG_INFINITY, 2.2)));
        assert_eq!(grid.run_at(2.1), Some((f64::NEG_INFINITY, 2.2)));
        assert_eq!(grid.run_at(2.2), Some((2.2, f64::INFINITY)));
        assert_eq!(grid.run_at(f64::NAN), None);
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
        assert!(!grid.replace(None));
        assert!(!grid.set_bpm_from(bpm(90.0), 0.5));
        assert_eq!(grid, before);
        assert_eq!(grid.next_marker(1.0), Some(2.0));
        grid.set_locked(false);
        assert!(grid.add_marker(5.0));
    }

    #[test]
    fn a_replaced_grid_has_one_marker_or_none() {
        let mut grid = grid_with(&[1.0, 2.0, 3.0]);
        grid.set_bpm(bpm(120.0));
        assert!(grid.replace(Some((bpm(128.0), 0.25))));
        assert_eq!(grid.bpm(), Some(bpm(128.0)));
        assert_eq!(grid.markers(), &[0.25]);
        assert!(!grid.replace(Some((bpm(128.0), 0.25))));
        assert!(grid.replace(None));
        assert!(grid.is_empty());
        assert!(!grid.replace(None));
    }

    #[test]
    fn a_tempo_set_from_a_beat_places_the_marker_there_only_without_markers() {
        let mut grid = BeatGrid::default();
        assert!(grid.is_empty());
        assert!(grid.set_bpm_from(bpm(124.0), 0.3));
        assert_eq!(grid.markers(), &[0.3]);
        assert!(grid.set_bpm_from(bpm(125.0), 0.7));
        assert_eq!(grid.markers(), &[0.3]);
        assert_eq!(grid.bpm(), Some(bpm(125.0)));
        assert!(!grid.set_bpm_from(bpm(125.0), 0.7));
        assert!(!grid.is_empty());
    }

    /// A grid at 120 BPM with markers at 0 and 10 seconds, the second at 100 BPM.
    fn two_tempos() -> BeatGrid {
        let mut grid = BeatGrid::default();
        assert!(grid.set_bpm(bpm(120.0)));
        assert!(grid.add_marker(10.0));
        assert!(grid.set_marker_bpm(10.0, Some(bpm(100.0))));
        grid
    }

    #[test]
    fn each_marker_sets_its_tempo_up_to_the_next_one() {
        let grid = two_tempos();
        assert_eq!(grid.bpm(), Some(bpm(120.0)));
        assert_eq!(grid.bpm_at(-1.0), Some(bpm(120.0)));
        assert_eq!(grid.bpm_at(9.99), Some(bpm(120.0)));
        assert_eq!(grid.bpm_at(10.0), Some(bpm(100.0)));
        assert_eq!(grid.bpm_at(30.0), Some(bpm(100.0)));
        assert_eq!(
            grid.marker_tempos().collect::<Vec<_>>(),
            [None, Some(bpm(100.0))]
        );

        // Beats every half second up to 10 s, then every 0.6 s from the marker.
        let mut beats = Vec::new();
        grid.for_each_beat(9.0, 11.5, |beat| beats.push(beat.seconds));
        assert_eq!(beats, [9.0, 9.5, 10.0, 10.6, 11.2]);
        assert!((grid.phase_at(10.3).unwrap() - 0.5).abs() < 1e-9);
        assert!((grid.nearest_beat(10.5, 60.0).unwrap() - 10.6).abs() < 1e-9);
        assert_eq!(grid.run_at(10.3), Some((10.0, f64::INFINITY)));
    }

    #[test]
    fn markers_set_deleted_and_scaled_carry_their_tempos() {
        let mut grid = two_tempos();
        // A marker inside the second run keeps its tempo; deleting it gives the run back.
        assert!(grid.add_marker(20.0));
        assert_eq!(grid.bpm_at(25.0), Some(bpm(100.0)));
        assert!(grid.set_marker_bpm(20.0, Some(bpm(90.0))));
        assert!(grid.delete_marker(20.0));
        assert_eq!(grid.bpm_at(25.0), Some(bpm(100.0)));
        assert_eq!(
            grid.marker_tempos().collect::<Vec<_>>(),
            [None, Some(bpm(100.0))]
        );

        // Doubling and stepping act on every tempo, or on none when one leaves the range.
        assert!(grid.scale_bpm(2.0));
        assert_eq!(grid.bpm_at(0.0), Some(bpm(240.0)));
        assert_eq!(grid.bpm_at(10.0), Some(bpm(200.0)));
        assert!(!grid.scale_bpm(2.0));
        assert_eq!(grid.bpm_at(10.0), Some(bpm(200.0)));
        assert!(grid.step_bpm(-10.0));
        assert_eq!(grid.bpm_at(10.0), Some(bpm(190.0)));

        // Back to the tempo of the grid, and nothing on a locked grid or one without tempo.
        assert!(grid.set_marker_bpm(10.0, None));
        assert_eq!(grid.bpm_at(10.0), Some(bpm(230.0)));
        grid.set_locked(true);
        assert!(!grid.set_marker_bpm(10.0, Some(bpm(100.0))));
        let mut bare = BeatGrid::default();
        assert!(bare.add_marker(1.0));
        assert!(!bare.set_marker_bpm(1.0, Some(bpm(100.0))));

        // A reset forgets the tempos of the markers.
        let mut grid = two_tempos();
        assert!(grid.replace(Some((bpm(128.0), 0.0))));
        assert!(grid.add_marker(10.0));
        assert_eq!(grid.bpm_at(10.0), Some(bpm(128.0)));
    }
}
