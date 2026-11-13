//! Parameter smoothing. A control that jumps would click; every parameter that reaches the
//! audio path moves along a short ramp instead.

use platter_core::SampleRate;

/// A value that moves to its target in a straight line over a fixed number of samples.
#[derive(Clone, Copy, Debug)]
pub struct Smoothed {
    current: f32,
    target: f32,
    step: f32,
    remaining: u32,
    ramp_samples: u32,
}

impl Smoothed {
    /// Starts at `value` with ramps of `milliseconds`.
    pub fn new(value: f32, rate: SampleRate, milliseconds: f32) -> Self {
        let samples = (rate.as_f32() * milliseconds / 1_000.0).round();
        Self {
            current: value,
            target: value,
            step: 0.0,
            remaining: 0,
            ramp_samples: if samples.is_finite() {
                samples.clamp(1.0, 1_000_000.0) as u32
            } else {
                1
            },
        }
    }

    /// Starts a ramp to `target`. A target that is not a number is ignored.
    pub fn set(&mut self, target: f32) {
        if !target.is_finite() || target == self.target {
            return;
        }
        self.target = target;
        self.remaining = self.ramp_samples;
        self.step = (target - self.current) / self.ramp_samples as f32;
    }

    /// Advances one sample and returns the value for it.
    pub fn advance(&mut self) -> f32 {
        if self.remaining == 0 {
            return self.target;
        }
        self.remaining -= 1;
        if self.remaining == 0 {
            // Land exactly on the target; accumulated steps would miss it by rounding.
            self.current = self.target;
        } else {
            self.current += self.step;
        }
        self.current
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn is_settled(&self) -> bool {
        self.remaining == 0
    }
}

#[cfg(test)]
mod tests {
    use platter_core::SampleRate;

    use super::Smoothed;

    #[test]
    fn ramps_in_equal_steps_and_lands_on_the_target() {
        let rate = SampleRate::new(48_000).unwrap();
        let mut value = Smoothed::new(0.0, rate, 1.0);
        value.set(1.0);
        let ramp: Vec<f32> = (0..48).map(|_| value.advance()).collect();
        assert!(ramp.windows(2).all(|pair| pair[1] > pair[0]));
        assert!((ramp[0] - 1.0 / 48.0).abs() < 1e-6);
        assert_eq!(ramp[47], 1.0);
        assert!(value.is_settled());
        assert_eq!(value.advance(), 1.0);
    }

    #[test]
    fn a_new_target_mid_ramp_continues_from_the_current_value() {
        let rate = SampleRate::new(48_000).unwrap();
        let mut value = Smoothed::new(0.0, rate, 1.0);
        value.set(1.0);
        let halfway = (0..24).map(|_| value.advance()).last().unwrap();
        value.set(0.0);
        let after = value.advance();
        assert!(after < halfway && after > halfway - 0.05);
    }

    #[test]
    fn ignores_targets_that_are_not_numbers() {
        let rate = SampleRate::new(48_000).unwrap();
        let mut value = Smoothed::new(0.5, rate, f32::NAN);
        value.set(f32::NAN);
        value.set(f32::INFINITY);
        assert_eq!(value.advance(), 0.5);
        assert_eq!(value.target(), 0.5);
    }
}
