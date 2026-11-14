//! Balance between the left and the right side of a stereo channel.
//!
//! Behaviour note: `docs/behavior/mixer.md`.

/// Gains of the left and the right side for a knob position in `-1.0..=1.0`. At the centre
/// both sides pass untouched; turning to one side fades the other out in a straight line,
/// down to silence at the end of the travel. The side the knob points at never gets louder,
/// so the control cannot push a channel into clipping.
pub fn balance_gains(position: f32) -> (f32, f32) {
    if !position.is_finite() {
        return (1.0, 1.0);
    }
    let position = position.clamp(-1.0, 1.0);
    (1.0 - position.max(0.0), 1.0 + position.min(0.0))
}

#[cfg(test)]
mod tests {
    use super::balance_gains;

    #[test]
    fn the_centre_leaves_both_sides_untouched() {
        assert_eq!(balance_gains(0.0), (1.0, 1.0));
    }

    #[test]
    fn each_end_silences_the_other_side() {
        assert_eq!(balance_gains(-1.0), (1.0, 0.0));
        assert_eq!(balance_gains(1.0), (0.0, 1.0));
    }

    #[test]
    fn the_far_side_fades_in_a_straight_line() {
        assert_eq!(balance_gains(0.5), (0.5, 1.0));
        assert_eq!(balance_gains(-0.25), (1.0, 0.75));
    }

    #[test]
    fn values_outside_the_travel_or_not_numbers_are_safe() {
        assert_eq!(balance_gains(7.0), (0.0, 1.0));
        assert_eq!(balance_gains(-7.0), (1.0, 0.0));
        assert_eq!(balance_gains(f32::NAN), (1.0, 1.0));
    }
}
