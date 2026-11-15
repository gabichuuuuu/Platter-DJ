//! Crossfader curves: how the position of the crossfader becomes a gain for each side.

use std::f32::consts::FRAC_PI_2;

use platter_core::ValueError;

/// Fraction of the travel at each end over which the sharp curve opens a side fully.
const SHARP_TRAVEL: f32 = 0.1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CrossfaderCurve {
    /// Equal power: the summed loudness of two unrelated tracks stays level across the
    /// travel. Each side is 3 dB down at the centre.
    #[default]
    Smooth,
    /// Straight lines: each side is 6 dB down at the centre.
    Linear,
    /// Both sides at full level except near the ends, for cutting.
    Sharp,
}

impl CrossfaderCurve {
    pub const ALL: [Self; 3] = [Self::Smooth, Self::Linear, Self::Sharp];

    pub fn name(self) -> &'static str {
        match self {
            Self::Smooth => "smooth",
            Self::Linear => "linear",
            Self::Sharp => "sharp",
        }
    }

    pub fn parse(text: &str) -> Result<Self, ValueError> {
        Self::ALL
            .into_iter()
            .find(|curve| curve.name() == text)
            .ok_or(ValueError::UnknownChoice {
                name: "crossfader curve",
                expected: "smooth, linear, sharp",
            })
    }

    /// Gains of the left side and the right side for a position between `0.0` (fully left)
    /// and `1.0` (fully right). Positions outside the travel are clamped; a position that
    /// is not a number is the centre.
    pub fn gains(self, position: f32) -> (f32, f32) {
        let x = if position.is_finite() {
            position.clamp(0.0, 1.0)
        } else {
            0.5
        };
        match self {
            // At the ends the cosine and sine round to almost zero, not zero; a closed side
            // must be silent.
            Self::Smooth if x == 0.0 => (1.0, 0.0),
            Self::Smooth if x == 1.0 => (0.0, 1.0),
            Self::Smooth => ((x * FRAC_PI_2).cos(), (x * FRAC_PI_2).sin()),
            Self::Linear => (1.0 - x, x),
            Self::Sharp => (
                ((1.0 - x) / SHARP_TRAVEL).min(1.0),
                (x / SHARP_TRAVEL).min(1.0),
            ),
        }
    }
}

/// The side of the crossfader a mixer channel is assigned to: `docs/behavior/mixer.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossfaderSide {
    Left,
    /// The channel bypasses the crossfader.
    Thru,
    Right,
}

impl CrossfaderSide {
    pub const ALL: [Self; 3] = [Self::Left, Self::Thru, Self::Right];

    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Thru => "thru",
            Self::Right => "right",
        }
    }

    pub fn parse(text: &str) -> Result<Self, ValueError> {
        Self::ALL
            .into_iter()
            .find(|side| side.name() == text)
            .ok_or(ValueError::UnknownChoice {
                name: "crossfader side",
                expected: "left, thru, right",
            })
    }

    /// The gain of a channel on this side, from the gains of the left and the right side.
    pub fn gain(self, (left, right): (f32, f32)) -> f32 {
        match self {
            Self::Left => left,
            Self::Thru => 1.0,
            Self::Right => right,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CrossfaderCurve, CrossfaderSide};

    #[test]
    fn a_side_takes_the_gain_of_its_side_and_thru_passes() {
        for curve in CrossfaderCurve::ALL {
            for position in [0.0, 0.3, 0.5, 1.0] {
                let gains = curve.gains(position);
                assert_eq!(CrossfaderSide::Left.gain(gains), gains.0);
                assert_eq!(CrossfaderSide::Right.gain(gains), gains.1);
                assert_eq!(CrossfaderSide::Thru.gain(gains), 1.0);
            }
        }
        for side in CrossfaderSide::ALL {
            assert_eq!(CrossfaderSide::parse(side.name()), Ok(side));
        }
        assert!(CrossfaderSide::parse("a").is_err());
    }

    #[test]
    fn every_curve_closes_the_far_side_at_the_ends() {
        for curve in CrossfaderCurve::ALL {
            assert_eq!(curve.gains(0.0), (1.0, 0.0), "{curve:?}");
            assert_eq!(curve.gains(1.0), (0.0, 1.0), "{curve:?}");
            assert_eq!(curve.gains(-4.0), curve.gains(0.0));
            assert_eq!(curve.gains(9.0), curve.gains(1.0));
            assert_eq!(curve.gains(f32::NAN), curve.gains(0.5));
        }
    }

    #[test]
    fn the_centre_matches_each_curve() {
        let (left, right) = CrossfaderCurve::Smooth.gains(0.5);
        assert!((left * left + right * right - 1.0).abs() < 1e-6);
        assert_eq!(CrossfaderCurve::Linear.gains(0.5), (0.5, 0.5));
        assert_eq!(CrossfaderCurve::Sharp.gains(0.5), (1.0, 1.0));
    }

    #[test]
    fn the_smooth_curve_keeps_the_power_level() {
        for step in 0..=20 {
            let (left, right) = CrossfaderCurve::Smooth.gains(step as f32 / 20.0);
            assert!((left * left + right * right - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn names_round_trip() {
        for curve in CrossfaderCurve::ALL {
            assert_eq!(CrossfaderCurve::parse(curve.name()), Ok(curve));
        }
        assert!(CrossfaderCurve::parse("scratch").is_err());
    }
}
