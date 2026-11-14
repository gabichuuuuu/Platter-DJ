//! Tempo from taps: the TAP button of the GRID page.
//!
//! Behaviour notes: `docs/behavior/beatgrid.md`, section Tap. The times come from the caller,
//! so the tests give them directly instead of waiting.

use std::time::{Duration, Instant};

use platter_core::Bpm;

/// Taps needed before a tempo comes out.
const TAPS_FOR_TEMPO: usize = 4;
/// Intervals the mean covers at most.
const INTERVALS: usize = 8;
/// A pause longer than this starts a new count.
const RESTART_AFTER: Duration = Duration::from_secs(2);

#[derive(Debug, Default)]
pub(crate) struct TapTempo {
    last: Option<Instant>,
    /// The latest intervals, oldest first once the ring has wrapped.
    intervals: [Duration; INTERVALS],
    /// Intervals recorded since the count started.
    count: usize,
}

impl TapTempo {
    /// Takes one tap at `now` and returns the tempo once there are enough taps.
    pub(crate) fn tap(&mut self, now: Instant) -> Option<Bpm> {
        let since = self.last.and_then(|last| now.checked_duration_since(last));
        self.last = Some(now);
        let Some(interval) = since.filter(|interval| *interval <= RESTART_AFTER) else {
            self.count = 0;
            return None;
        };
        self.intervals[self.count % INTERVALS] = interval;
        self.count = self.count.saturating_add(1);
        if self.count + 1 < TAPS_FOR_TEMPO {
            return None;
        }
        let used = self.count.min(INTERVALS);
        let total: Duration = self.intervals.iter().take(used).sum();
        let mean = total.as_secs_f64() / used as f64;
        if mean <= 0.0 {
            return None;
        }
        Bpm::new(60.0 / mean).ok()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::TapTempo;

    fn taps(tap: &mut TapTempo, start: Instant, gaps_ms: &[u64]) -> Vec<Option<f64>> {
        let mut now = start;
        let mut results = vec![tap.tap(now).map(|bpm| bpm.value())];
        for gap in gaps_ms {
            now += Duration::from_millis(*gap);
            results.push(tap.tap(now).map(|bpm| bpm.value()));
        }
        results
    }

    #[test]
    fn the_fourth_tap_gives_the_tempo() {
        let mut tap = TapTempo::default();
        let results = taps(&mut tap, Instant::now(), &[500, 500, 500, 500]);
        assert_eq!(results, [None, None, None, Some(120.0), Some(120.0)]);
    }

    #[test]
    fn the_mean_covers_the_last_eight_intervals() {
        let mut tap = TapTempo::default();
        // Eight slow intervals, then eight at 120 BPM: only the last eight count.
        let mut gaps = vec![1_000; 8];
        gaps.extend([500; 8]);
        let results = taps(&mut tap, Instant::now(), &gaps);
        assert_eq!(results.last().copied().flatten(), Some(120.0));
        assert_eq!(results[3], Some(60.0));
    }

    #[test]
    fn a_long_pause_starts_again() {
        let mut tap = TapTempo::default();
        let results = taps(&mut tap, Instant::now(), &[500, 500, 2_500, 400, 400, 400]);
        assert_eq!(results, [None, None, None, None, None, None, Some(150.0)]);
    }

    #[test]
    fn a_tempo_out_of_range_comes_out_as_nothing() {
        let mut tap = TapTempo::default();
        // 100 ms apart is 600 BPM.
        let results = taps(&mut tap, Instant::now(), &[100, 100, 100]);
        assert_eq!(results.last().copied().flatten(), None);
        // Taps at the same instant give no interval to divide by.
        let mut tap = TapTempo::default();
        assert!(
            taps(&mut tap, Instant::now(), &[0, 0, 0])
                .iter()
                .all(Option::is_none)
        );
    }
}
