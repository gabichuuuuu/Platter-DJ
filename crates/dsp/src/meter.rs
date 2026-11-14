//! Peak meter: rises at once, falls slowly.

use platter_core::SampleRate;

use crate::Frame;

/// Time for the reading to fall by 20 dB once the signal stops.
const FALL_SECONDS: f32 = 0.3;

#[derive(Clone, Copy, Debug)]
pub struct PeakMeter {
    levels: [f32; 2],
    decay: f32,
}

impl PeakMeter {
    pub fn new(rate: SampleRate) -> Self {
        Self {
            levels: [0.0; 2],
            decay: 0.1f32.powf(1.0 / (FALL_SECONDS * rate.as_f32())),
        }
    }

    /// Feeds one frame; each channel keeps its own reading.
    pub fn feed(&mut self, frame: Frame) {
        for (level, input) in self.levels.iter_mut().zip(frame) {
            let fallen = *level * self.decay;
            // `max` drops a NaN operand, so a bad sample cannot poison the reading.
            *level = if fallen < 1e-9 { 0.0 } else { fallen }.max(input.abs());
        }
    }

    /// Current reading of the louder channel, as a linear amplitude.
    pub fn level(&self) -> f32 {
        self.levels[0].max(self.levels[1])
    }

    /// Current reading of the left and the right channel, as linear amplitudes.
    pub fn levels(&self) -> [f32; 2] {
        self.levels
    }
}

#[cfg(test)]
mod tests {
    use platter_core::SampleRate;

    use super::PeakMeter;

    #[test]
    fn rises_at_once_and_falls_twenty_decibels_in_the_fall_time() {
        let mut meter = PeakMeter::new(SampleRate::new(48_000).unwrap());
        meter.feed([0.2, -0.8]);
        assert_eq!(meter.level(), 0.8);
        assert_eq!(meter.levels(), [0.2, 0.8]);
        for _ in 0..14_400 {
            meter.feed([0.0, 0.0]);
        }
        assert!((meter.level() - 0.08).abs() < 0.002);
        for _ in 0..480_000 {
            meter.feed([0.0, 0.0]);
        }
        assert_eq!(meter.level(), 0.0);
    }

    #[test]
    fn a_bad_sample_does_not_poison_the_reading() {
        let mut meter = PeakMeter::new(SampleRate::new(48_000).unwrap());
        meter.feed([f32::NAN, 0.5]);
        meter.feed([f32::NAN, f32::NAN]);
        assert!(meter.level().is_finite());
    }
}
