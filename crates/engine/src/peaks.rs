//! Waveform peaks of a track in three frequency bands, computed once when the track loads.
//!
//! The interface draws its waveforms from this data and never looks at the audio itself.
//! The bands are split at the corners of the equaliser, so what the waveform shows as low,
//! mid and high is what the three knobs of the mixer act on. Computing it is slow work and
//! belongs on a worker thread.

use platter_core::SampleRate;
use platter_dsp::biquad::{BUTTERWORTH_Q, Biquad, Coefficients};
use platter_dsp::eq::{LOW_MID_HZ, MID_HIGH_HZ};

/// Columns per second of the detailed data. At the closest zoom of the scrolling waveform a
/// column is about as wide as a pixel.
const COLUMNS_PER_SECOND: f64 = 400.0;

/// Peak of the low, the mid and the high band in one column, where 255 is full scale.
pub type Column = [u8; 3];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WaveformPeaks {
    columns: Vec<Column>,
    overview: Vec<Column>,
    seconds_per_column: f64,
}

/// Two second-order sections in a row: a slope of 24 dB per octave.
struct Steep([Biquad; 2]);

impl Steep {
    fn new(coefficients: Coefficients) -> Self {
        Self([Biquad::new(coefficients); 2])
    }

    fn process(&mut self, input: f32) -> f32 {
        let [first, second] = &mut self.0;
        second.process(first.process(input))
    }
}

impl WaveformPeaks {
    /// Columns of the overview: enough for the widest overview the interface draws.
    pub const OVERVIEW_COLUMNS: usize = 1_024;

    /// Computes the peaks of interleaved stereo audio at `rate`.
    pub fn compute(stereo: &[f32], rate: SampleRate) -> Self {
        let frames_per_column = (rate.as_f64() / COLUMNS_PER_SECOND).round().max(1.0) as usize;
        let low_pass = |hz| Coefficients::low_pass(rate, hz, BUTTERWORTH_Q);
        let high_pass = |hz| Coefficients::high_pass(rate, hz, BUTTERWORTH_Q);
        let mut low = Steep::new(low_pass(LOW_MID_HZ));
        let mut mid = (
            Steep::new(high_pass(LOW_MID_HZ)),
            Steep::new(low_pass(MID_HIGH_HZ)),
        );
        let mut high = Steep::new(high_pass(MID_HIGH_HZ));

        let frames = stereo.len() / 2;
        let mut columns = Vec::with_capacity(frames.div_ceil(frames_per_column));
        for column in stereo.chunks(frames_per_column * 2) {
            let mut peaks = [0.0f32; 3];
            for frame in column.as_chunks::<2>().0 {
                let mono = 0.5 * (frame[0] + frame[1]);
                let bands = [
                    low.process(mono),
                    mid.1.process(mid.0.process(mono)),
                    high.process(mono),
                ];
                for (peak, band) in peaks.iter_mut().zip(bands) {
                    // `max` drops a NaN operand, so a bad sample cannot poison a column.
                    *peak = peak.max(band.abs());
                }
            }
            columns.push(peaks.map(quantise));
        }
        let overview = reduce(&columns, Self::OVERVIEW_COLUMNS);
        Self {
            columns,
            overview,
            seconds_per_column: frames_per_column as f64 / rate.as_f64(),
        }
    }

    /// Builds the peaks from columns computed elsewhere, one every `seconds_per_column`.
    /// The capture mode of the application uses it to show a waveform without audio.
    pub fn from_columns(columns: Vec<Column>, seconds_per_column: f64) -> Self {
        let overview = reduce(&columns, Self::OVERVIEW_COLUMNS);
        let seconds_per_column = if seconds_per_column.is_finite() && seconds_per_column > 0.0 {
            seconds_per_column
        } else {
            1.0 / COLUMNS_PER_SECOND
        };
        Self {
            columns,
            overview,
            seconds_per_column,
        }
    }

    /// The detailed data: one column every [`Self::seconds_per_column`].
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    pub fn seconds_per_column(&self) -> f64 {
        self.seconds_per_column
    }

    /// The whole track in at most [`Self::OVERVIEW_COLUMNS`] columns of equal length.
    pub fn overview(&self) -> &[Column] {
        &self.overview
    }
}

fn quantise(peak: f32) -> u8 {
    (peak.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Shrinks `columns` to at most `target` columns, each the peak of the ones it covers.
fn reduce(columns: &[Column], target: usize) -> Vec<Column> {
    if columns.len() <= target {
        return columns.to_vec();
    }
    (0..target)
        .map(|index| {
            let start = index * columns.len() / target;
            let end = ((index + 1) * columns.len() / target).max(start + 1);
            columns[start..end.min(columns.len())]
                .iter()
                .fold([0u8; 3], |peak, column| {
                    [
                        peak[0].max(column[0]),
                        peak[1].max(column[1]),
                        peak[2].max(column[2]),
                    ]
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use platter_core::SampleRate;
    use platter_testkit::signal::{sine, stereo};

    use super::WaveformPeaks;

    const RATE: u32 = 44_100;

    fn peaks_of_sine(hz: f64, seconds: f64) -> WaveformPeaks {
        let frames = (seconds * f64::from(RATE)) as usize;
        let audio = stereo(&sine(hz, RATE, frames, 0.8));
        WaveformPeaks::compute(&audio, SampleRate::new(RATE).unwrap())
    }

    /// The peak of each band over the second half of the data, away from the start of the
    /// filters. A column is shorter than a cycle of a low tone, so one alone may miss its
    /// crest.
    fn settled(peaks: &WaveformPeaks) -> [u8; 3] {
        let half = &peaks.columns()[peaks.columns().len() / 2..];
        [0, 1, 2].map(|band| half.iter().map(|column| column[band]).max().unwrap())
    }

    #[test]
    fn each_band_holds_the_tone_that_belongs_to_it() {
        // 0.8 of full scale is 204 of 255.
        let [low, mid, high] = settled(&peaks_of_sine(60.0, 1.0));
        assert!(low > 190 && mid < 20 && high == 0, "{low} {mid} {high}");
        let [low, mid, high] = settled(&peaks_of_sine(1_000.0, 1.0));
        assert!(mid > 190 && low < 20 && high < 20, "{low} {mid} {high}");
        let [low, mid, high] = settled(&peaks_of_sine(9_000.0, 1.0));
        assert!(high > 190 && mid < 20 && low == 0, "{low} {mid} {high}");
    }

    #[test]
    fn columns_cover_the_whole_track_at_a_fixed_pace() {
        let peaks = peaks_of_sine(440.0, 1.0);
        // 44 100 Hz at 400 columns per second: 110 frames per column, rounded up at the end.
        assert_eq!(peaks.columns().len(), 401);
        assert!((peaks.seconds_per_column() - 110.0 / 44_100.0).abs() < 1e-12);
        assert_eq!(peaks.overview(), peaks.columns());
    }

    #[test]
    fn the_overview_keeps_the_peaks_of_a_long_track() {
        let frames = RATE as usize * 6;
        let mut audio = vec![0.0f32; frames * 2];
        // One loud stretch of 20 ms in the fifth second, in the mid band.
        let burst = sine(1_000.0, RATE, 882, 0.9);
        let start = RATE as usize * 4;
        for (offset, sample) in burst.iter().enumerate() {
            audio[(start + offset) * 2] = *sample;
            audio[(start + offset) * 2 + 1] = *sample;
        }
        let peaks = WaveformPeaks::compute(&audio, SampleRate::new(RATE).unwrap());
        assert_eq!(peaks.overview().len(), WaveformPeaks::OVERVIEW_COLUMNS);
        let loudest = |columns: &[[u8; 3]]| columns.iter().map(|column| column[1]).max();
        assert_eq!(loudest(peaks.overview()), loudest(peaks.columns()));
        // The burst sits two thirds of the way in, and nowhere else.
        let position = peaks
            .overview()
            .iter()
            .position(|column| column[1] > 100)
            .unwrap();
        let expected = WaveformPeaks::OVERVIEW_COLUMNS * 4 / 6;
        assert!(position.abs_diff(expected) <= 2, "{position}");
        assert!(peaks.overview()[..expected - 8].iter().all(|c| c[1] == 0));
    }

    #[test]
    fn peaks_built_from_columns_get_their_overview() {
        let columns: Vec<[u8; 3]> = (0..4_096u32)
            .map(|index| [(index % 251) as u8, 7, 0])
            .collect();
        let peaks = WaveformPeaks::from_columns(columns.clone(), 0.0025);
        assert_eq!(peaks.columns(), columns.as_slice());
        assert_eq!(peaks.seconds_per_column(), 0.0025);
        assert_eq!(peaks.overview().len(), WaveformPeaks::OVERVIEW_COLUMNS);
        assert_eq!(peaks.overview()[0], [3, 7, 0]);
        // A pace that is not a positive number falls back to the pace of computed peaks.
        let peaks = WaveformPeaks::from_columns(Vec::new(), f64::NAN);
        assert_eq!(peaks.seconds_per_column(), 1.0 / 400.0);
    }

    #[test]
    fn silence_and_empty_audio_give_empty_peaks() {
        let rate = SampleRate::new(RATE).unwrap();
        let peaks = WaveformPeaks::compute(&vec![0.0; 8_820], rate);
        assert!(peaks.columns().iter().all(|column| *column == [0; 3]));
        let peaks = WaveformPeaks::compute(&[], rate);
        assert!(peaks.columns().is_empty() && peaks.overview().is_empty());
    }

    #[test]
    fn samples_that_are_not_numbers_or_exceed_full_scale_stay_in_range() {
        let rate = SampleRate::new(RATE).unwrap();
        let mut audio = stereo(&sine(1_000.0, RATE, 4_410, 4.0));
        audio[100] = f32::NAN;
        audio[201] = f32::INFINITY;
        // Reaching the end without a panic is the point; every column is a valid byte.
        let peaks = WaveformPeaks::compute(&audio, rate);
        assert_eq!(peaks.columns().len(), 41);
    }
}
