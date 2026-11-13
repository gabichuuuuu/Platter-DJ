use crate::ValueError;

/// Playback speed as a ratio of the original speed. `1.0` is the original; `0.0` is a full
/// stop, which the widest tempo range reaches.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Tempo(f64);

impl Tempo {
    pub const ORIGINAL: Self = Self(1.0);
    pub const MIN_RATIO: f64 = 0.0;
    pub const MAX_RATIO: f64 = 2.0;

    pub fn from_ratio(ratio: f64) -> Result<Self, ValueError> {
        ValueError::check("tempo", ratio, Self::MIN_RATIO, Self::MAX_RATIO).map(Self)
    }

    pub fn ratio(self) -> f64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::Tempo;

    #[test]
    fn accepts_from_full_stop_to_double_speed() {
        assert_eq!(Tempo::from_ratio(1.0), Ok(Tempo::ORIGINAL));
        assert!(Tempo::from_ratio(0.0).is_ok());
        assert!(Tempo::from_ratio(2.0).is_ok());
        assert!(Tempo::from_ratio(2.01).is_err());
        assert!(Tempo::from_ratio(-0.5).is_err());
        assert!(Tempo::from_ratio(f64::NAN).is_err());
    }
}
