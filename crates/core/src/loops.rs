//! The preset sizes of loops and jumps, and the stretch of a track a loop repeats.
//!
//! Behaviour notes: `docs/behavior/loops.md`.

/// One of the eleven preset sizes, from 1/32 of a beat to 32 beats, each twice the one
/// before.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LoopSize(u8);

impl LoopSize {
    pub const COUNT: usize = 11;
    pub const SMALLEST: Self = Self(0);
    pub const LARGEST: Self = Self(Self::COUNT as u8 - 1);
    pub const ONE_BEAT: Self = Self(Self::WHOLE_BEAT);
    pub const FOUR_BEATS: Self = Self(Self::WHOLE_BEAT + 2);

    /// Index of the size of one beat: five halvings lead from it to the smallest.
    const WHOLE_BEAT: u8 = 5;

    /// The size at `index` of the list, smallest first.
    pub fn from_index(index: usize) -> Option<Self> {
        (index < Self::COUNT).then_some(Self(index as u8))
    }

    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    /// The size as a fraction of beats in lowest terms: `(1, 32)` to `(32, 1)`.
    pub fn fraction(self) -> (u32, u32) {
        match self.0.checked_sub(Self::WHOLE_BEAT) {
            Some(doublings) => (1 << doublings, 1),
            None => (1, 1 << (Self::WHOLE_BEAT - self.0)),
        }
    }

    pub fn beats(self) -> f64 {
        let (beats, parts) = self.fraction();
        f64::from(beats) / f64::from(parts)
    }

    /// The size before this one, half as long.
    pub fn smaller(self) -> Option<Self> {
        self.0.checked_sub(1).map(Self)
    }

    /// The size after this one, twice as long.
    pub fn larger(self) -> Option<Self> {
        Self::from_index(self.index() + 1)
    }
}

/// The stretch of a track a loop repeats, in seconds of the track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoopRegion {
    pub start: f64,
    pub end: f64,
}

impl LoopRegion {
    pub fn seconds(self) -> f64 {
        self.end - self.start
    }
}

#[cfg(test)]
mod tests {
    use super::LoopSize;

    #[test]
    fn the_sizes_double_from_a_thirty_second_of_a_beat_to_thirty_two_beats() {
        let sizes: Vec<LoopSize> = (0..LoopSize::COUNT)
            .map(|index| LoopSize::from_index(index).unwrap())
            .collect();
        assert_eq!(sizes[0], LoopSize::SMALLEST);
        assert_eq!(sizes[0].fraction(), (1, 32));
        assert_eq!(sizes[10], LoopSize::LARGEST);
        assert_eq!(sizes[10].fraction(), (32, 1));
        for pair in sizes.windows(2) {
            assert_eq!(pair[1].beats(), 2.0 * pair[0].beats());
        }
        assert_eq!(LoopSize::ONE_BEAT.beats(), 1.0);
        assert_eq!(LoopSize::FOUR_BEATS.beats(), 4.0);
        assert_eq!(LoopSize::from_index(LoopSize::COUNT), None);
    }

    #[test]
    fn the_steps_stop_at_both_ends_of_the_list() {
        assert_eq!(LoopSize::SMALLEST.smaller(), None);
        assert_eq!(LoopSize::LARGEST.larger(), None);
        assert_eq!(LoopSize::ONE_BEAT.smaller().unwrap().fraction(), (1, 2));
        assert_eq!(LoopSize::ONE_BEAT.larger().unwrap().fraction(), (2, 1));
    }
}
