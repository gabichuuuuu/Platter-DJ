use crate::ValueError;

/// A linear amplitude factor. It is never negative and never above [`Gain::MAX_DB`].
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Gain(f32);

impl Gain {
    pub const SILENCE: Self = Self(0.0);
    pub const UNITY: Self = Self(1.0);
    /// Decibel values at or below this one are silence.
    pub const MIN_DB: f64 = -90.0;
    pub const MAX_DB: f64 = 24.0;

    /// Converts decibels to a factor. Values at or below [`Gain::MIN_DB`], including negative
    /// infinity, are silence.
    pub fn from_db(db: f64) -> Result<Self, ValueError> {
        if db == f64::NEG_INFINITY {
            return Ok(Self::SILENCE);
        }
        if !db.is_finite() {
            return Err(ValueError::NotFinite { name: "gain" });
        }
        if db > Self::MAX_DB {
            return Err(ValueError::OutOfRange {
                name: "gain",
                min: Self::MIN_DB,
                max: Self::MAX_DB,
            });
        }
        if db <= Self::MIN_DB {
            return Ok(Self::SILENCE);
        }
        Ok(Self(10f64.powf(db / 20.0) as f32))
    }

    /// Accepts a factor between silence and unity, as a fader position gives it.
    pub fn from_fraction(value: f64) -> Result<Self, ValueError> {
        ValueError::check("level", value, 0.0, 1.0).map(|value| Self(value as f32))
    }

    pub fn linear(self) -> f32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::Gain;

    #[test]
    fn converts_decibels() {
        assert_eq!(Gain::from_db(0.0), Ok(Gain::UNITY));
        let minus_six = Gain::from_db(-6.0).unwrap().linear();
        assert!((minus_six - 0.501_187).abs() < 1e-5);
        assert_eq!(Gain::from_db(-90.0), Ok(Gain::SILENCE));
        assert_eq!(Gain::from_db(f64::NEG_INFINITY), Ok(Gain::SILENCE));
    }

    #[test]
    fn rejects_values_that_cannot_be_a_gain() {
        assert!(Gain::from_db(24.1).is_err());
        assert!(Gain::from_db(f64::NAN).is_err());
        assert!(Gain::from_db(f64::INFINITY).is_err());
        assert!(Gain::from_fraction(1.01).is_err());
        assert!(Gain::from_fraction(-0.1).is_err());
        assert!(Gain::from_fraction(f64::NAN).is_err());
    }
}
