//! The effect units and the mixer effect on the control side: what each unit holds, its values,
//! which channels go through it (`docs/behavior/effects.md`). The processor keeps the
//! instances; this side keeps everything needed to build them again after a restart
//! (`docs/decisions/0033-effect-chain.md`).

use platter_core::{DeckId, ValueError};
use platter_dsp::effects::unit::{Place, SLOTS, UnitValues};
use platter_dsp::effects::{Controls, EffectKind};

pub use crate::rt::{FxRouting, UNITS};
/// The knobs and the keys of single mode.
pub const KNOBS: usize = 3;
pub const KEYS: usize = 2;

/// One of the effect units, numbered from 1 for the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FxUnit(u8);

impl FxUnit {
    pub const ALL: [Self; UNITS] = [Self(0), Self(1), Self(2), Self(3)];

    pub fn from_number(number: f64) -> Result<Self, ValueError> {
        index_of("unit", number, UNITS).map(|index| Self(index as u8))
    }

    /// The number the user sees, from 1.
    pub fn number(self) -> u8 {
        self.0 + 1
    }

    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// The index from 0 of a whole `number` between 1 and `count`.
pub(crate) fn index_of(name: &'static str, number: f64, count: usize) -> Result<usize, ValueError> {
    let max = count as f64;
    let number = ValueError::check(name, number, 1.0, max)?;
    if number.fract() == 0.0 {
        Ok(number as usize - 1)
    } else {
        Err(ValueError::OutOfRange {
            name,
            min: 1.0,
            max,
        })
    }
}

/// What a unit holds and how it is set.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitControls {
    pub values: UnitValues,
    pub routing: FxRouting,
    /// The effect of each slot of group mode.
    pub slots: [Option<EffectKind>; SLOTS],
    /// The effect of single mode.
    pub single: Option<EffectKind>,
    /// The controls kept by *Save snapshot*, per effect in the order of [`EffectKind::ALL`].
    pub snapshots: [Option<Controls>; EffectKind::ALL.len()],
}

impl Default for UnitControls {
    fn default() -> Self {
        Self {
            values: UnitValues::default(),
            routing: FxRouting::default(),
            slots: [None; SLOTS],
            single: None,
            snapshots: [None; EffectKind::ALL.len()],
        }
    }
}

impl UnitControls {
    /// The places that hold an effect, and the effect.
    pub fn places(&self) -> impl Iterator<Item = (Place, EffectKind)> + '_ {
        let group = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(index, kind)| kind.map(|kind| (Place::Group(index), kind)));
        group.chain(self.single.map(|kind| (Place::Single, kind)))
    }

    /// The controls single mode starts from with `kind`: its snapshot in this unit, or its
    /// defaults.
    pub fn start_of(&self, kind: EffectKind) -> Controls {
        EffectKind::ALL
            .iter()
            .position(|each| *each == kind)
            .and_then(|index| self.snapshots[index])
            .unwrap_or_else(|| kind.defaults())
    }
}

/// The mixer effect of one channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MixerEffectControls {
    pub effect: EffectKind,
    /// The knob, from 0 (off) to 1.
    pub amount: f64,
}

impl Default for MixerEffectControls {
    fn default() -> Self {
        Self {
            effect: EffectKind::Reverb,
            amount: 0.0,
        }
    }
}

/// The effect units and the mixer effects, as the user set them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EffectControls {
    pub units: [UnitControls; UNITS],
    /// Which units each channel goes through, deck A first.
    pub assigned: [[bool; UNITS]; DeckId::COUNT],
    pub mixer: [MixerEffectControls; DeckId::COUNT],
}

impl EffectControls {
    pub fn unit(&self, unit: FxUnit) -> &UnitControls {
        &self.units[unit.index()]
    }

    pub fn is_assigned(&self, deck: DeckId, unit: FxUnit) -> bool {
        self.assigned[deck.index()][unit.index()]
    }

    /// The decks that go through `unit`.
    pub fn decks_of(&self, unit: FxUnit) -> impl Iterator<Item = DeckId> + '_ {
        DeckId::ALL
            .into_iter()
            .filter(move |deck| self.is_assigned(*deck, unit))
    }
}

#[cfg(test)]
mod tests {
    use super::{FxRouting, FxUnit, index_of};

    #[test]
    fn units_are_numbered_from_one_to_four() {
        assert_eq!(FxUnit::from_number(1.0).unwrap().index(), 0);
        assert_eq!(FxUnit::from_number(4.0).unwrap().number(), 4);
        assert!(FxUnit::from_number(0.0).is_err());
        assert!(FxUnit::from_number(5.0).is_err());
        assert!(FxUnit::from_number(1.5).is_err());
        assert!(index_of("slot", f64::NAN, 3).is_err());
    }

    #[test]
    fn routings_round_trip() {
        for routing in FxRouting::ALL {
            assert_eq!(FxRouting::parse(routing.name()).unwrap(), routing);
        }
    }
}
