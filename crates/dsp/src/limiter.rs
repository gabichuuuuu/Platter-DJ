//! Safety limiter of the master output.
//!
//! It keeps the output inside full scale whatever the mix does. The gain drops at once when
//! a frame would exceed the ceiling and recovers slowly, so a loud passage is turned down
//! rather than clipped. It has no look-ahead: the first frame of an overshoot is scaled
//! exactly to the ceiling, which bends the waveform there. The trade-off is recorded in
//! `docs/decisions/0007-safety-limiter.md`.

use platter_core::SampleRate;

use crate::Frame;

pub const CEILING: f32 = 1.0;
const RELEASE_SECONDS: f32 = 0.2;

#[derive(Clone, Copy, Debug)]
pub struct Limiter {
    gain: f32,
    recovery: f32,
}

impl Limiter {
    pub fn new(rate: SampleRate) -> Self {
        Self {
            gain: 1.0,
            // Per-sample factor that closes 90 % of the distance to unity in the release time.
            recovery: 1.0 - 0.1f32.powf(1.0 / (RELEASE_SECONDS * rate.as_f32())),
        }
    }

    /// Processes one frame. Both channels share the gain so the stereo image does not move.
    /// A sample that is not a number becomes silence.
    pub fn process(&mut self, frame: Frame) -> Frame {
        let frame = frame.map(|sample| if sample.is_finite() { sample } else { 0.0 });
        let peak = frame[0].abs().max(frame[1].abs());
        // Near one the step falls under the resolution of `f32` and the gain would stop short
        // of it for good; it is then one.
        let next = self.gain + (1.0 - self.gain) * self.recovery;
        self.gain = if next == self.gain { 1.0 } else { next };
        if peak * self.gain > CEILING {
            self.gain = CEILING / peak;
        }
        // The clamp covers the rounding of the product.
        frame.map(|sample| (sample * self.gain).clamp(-CEILING, CEILING))
    }

    /// Gain applied to the last frame; below one while the limiter is working.
    pub fn gain(&self) -> f32 {
        self.gain
    }
}

#[cfg(test)]
mod tests {
    use platter_core::SampleRate;
    use platter_testkit::measure::peak;
    use platter_testkit::signal::sine;

    use super::{CEILING, Limiter};

    const RATE: u32 = 48_000;

    #[test]
    fn leaves_a_signal_under_the_ceiling_untouched() {
        let mut limiter = Limiter::new(SampleRate::new(RATE).unwrap());
        for x in sine(1_000.0, RATE, 4_800, 0.9) {
            assert_eq!(limiter.process([x, -x]), [x, -x]);
        }
        assert_eq!(limiter.gain(), 1.0);
    }

    #[test]
    fn holds_a_loud_signal_at_the_ceiling() {
        let mut limiter = Limiter::new(SampleRate::new(RATE).unwrap());
        let output: Vec<f32> = sine(1_000.0, RATE, 48_000, 4.0)
            .iter()
            .map(|x| limiter.process([*x, *x * 0.5])[0])
            .collect();
        assert!(peak(&output) <= CEILING);
        // Turned down, not flattened: the settled peaks sit just under the ceiling.
        assert!(peak(&output[24_000..]) > 0.95);
        assert!(limiter.gain() < 0.3);
    }

    #[test]
    fn recovers_after_the_loud_passage() {
        let mut limiter = Limiter::new(SampleRate::new(RATE).unwrap());
        limiter.process([8.0, 8.0]);
        assert!((limiter.gain() - 0.125).abs() < 1e-6);
        for _ in 0..RATE {
            limiter.process([0.1, 0.1]);
        }
        assert!(limiter.gain() > 0.999);
    }

    /// Once the loud passage is over the gain comes back to exactly one, so a quiet mix passes
    /// untouched again; the step towards one must not vanish in the rounding of `f32`.
    #[test]
    fn the_gain_comes_back_to_exactly_one() {
        let mut limiter = Limiter::new(SampleRate::new(RATE).unwrap());
        limiter.process([8.0, 8.0]);
        for _ in 0..20 * RATE {
            limiter.process([0.1, 0.1]);
        }
        assert_eq!(limiter.gain(), 1.0);
        assert_eq!(limiter.process([0.25, -0.5]), [0.25, -0.5]);
    }

    #[test]
    fn output_is_always_finite_and_inside_full_scale() {
        let mut limiter = Limiter::new(SampleRate::new(RATE).unwrap());
        for x in [
            f32::NAN,
            f32::INFINITY,
            -f32::INFINITY,
            1e30,
            -1e30,
            f32::MIN_POSITIVE,
            0.0,
        ] {
            let [left, right] = limiter.process([x, -x]);
            assert!(left.is_finite() && left.abs() <= CEILING);
            assert!(right.is_finite() && right.abs() <= CEILING);
        }
    }
}
