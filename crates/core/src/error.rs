use std::fmt;

/// A value that came from outside (a file, a command parameter) and cannot be used.
#[derive(Clone, Debug, PartialEq)]
pub enum ValueError {
    /// The value is NaN or infinite.
    NotFinite { name: &'static str },
    /// The value is finite but outside the accepted range.
    OutOfRange {
        name: &'static str,
        min: f64,
        max: f64,
    },
    /// The text names none of the accepted choices.
    UnknownChoice {
        name: &'static str,
        expected: &'static str,
    },
}

impl ValueError {
    /// Accepts a finite value inside `min..=max`.
    pub fn check(name: &'static str, value: f64, min: f64, max: f64) -> Result<f64, Self> {
        if !value.is_finite() {
            Err(Self::NotFinite { name })
        } else if value < min || value > max {
            Err(Self::OutOfRange { name, min, max })
        } else {
            Ok(value)
        }
    }
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFinite { name } => write!(f, "{name} is not a finite number"),
            Self::OutOfRange { name, min, max } => {
                write!(f, "{name} must be between {min} and {max}")
            }
            Self::UnknownChoice { name, expected } => {
                write!(f, "{name} must be one of: {expected}")
            }
        }
    }
}

impl std::error::Error for ValueError {}

#[cfg(test)]
mod tests {
    use super::ValueError;

    #[test]
    fn check_accepts_the_bounds_and_rejects_the_rest() {
        assert_eq!(ValueError::check("x", 1.0, 0.0, 1.0), Ok(1.0));
        assert_eq!(ValueError::check("x", 0.0, 0.0, 1.0), Ok(0.0));
        assert!(matches!(
            ValueError::check("x", 1.5, 0.0, 1.0),
            Err(ValueError::OutOfRange { .. })
        ));
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                ValueError::check("x", bad, 0.0, 1.0),
                Err(ValueError::NotFinite { name: "x" })
            );
        }
    }
}
