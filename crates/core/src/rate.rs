use crate::ValueError;

/// A sample rate in hertz, bounded so that arithmetic on it cannot overflow or divide by zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SampleRate(u32);

impl SampleRate {
    pub const MIN_HZ: u32 = 8_000;
    pub const MAX_HZ: u32 = 384_000;
    /// The rate of compact discs.
    pub const HZ_44100: Self = Self(44_100);
    /// The rate most audio devices run at.
    pub const HZ_48000: Self = Self(48_000);

    pub fn new(hz: u32) -> Result<Self, ValueError> {
        if (Self::MIN_HZ..=Self::MAX_HZ).contains(&hz) {
            Ok(Self(hz))
        } else {
            Err(ValueError::OutOfRange {
                name: "sample rate",
                min: f64::from(Self::MIN_HZ),
                max: f64::from(Self::MAX_HZ),
            })
        }
    }

    pub fn hz(self) -> u32 {
        self.0
    }

    pub fn as_f64(self) -> f64 {
        f64::from(self.0)
    }

    pub fn as_f32(self) -> f32 {
        self.0 as f32
    }
}

#[cfg(test)]
mod tests {
    use super::SampleRate;

    #[test]
    fn accepts_audio_rates_only() {
        assert_eq!(SampleRate::new(44_100).unwrap().hz(), 44_100);
        assert!(SampleRate::new(0).is_err());
        assert!(SampleRate::new(7_999).is_err());
        assert!(SampleRate::new(384_001).is_err());
    }
}
