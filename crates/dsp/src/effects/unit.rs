//! An effect unit as one channel hears it: three slots in group mode, one slot in single mode,
//! and the dry/wet mix (`docs/behavior/effects.md`). The engine keeps one [`Chain`] per channel
//! and unit; every chain of a unit reads the same [`UnitValues`]. The mixer effect of a
//! channel is a [`MixerEffect`].

use platter_core::{SampleRate, ValueError};

use super::{Controls, Effect, EffectKind, Tempo};
use crate::Frame;
use crate::smooth::Smoothed;

/// The slots of a unit in group mode.
pub const SLOTS: usize = 3;
/// How long switching an effect on or off, swapping it or changing the mode fades.
pub const SWITCH_MS: f32 = 10.0;
/// The dry/wet a unit starts from.
pub const DEFAULT_DRY_WET: f32 = 0.5;

/// The mode of a unit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnitMode {
    /// Three effects in a chain, one amount knob each.
    #[default]
    Group,
    /// One effect with three knobs and two keys.
    Single,
}

impl UnitMode {
    pub const ALL: [Self; 2] = [Self::Group, Self::Single];

    pub fn name(self) -> &'static str {
        match self {
            Self::Group => "group",
            Self::Single => "single",
        }
    }

    pub fn parse(text: &str) -> Result<Self, ValueError> {
        Self::ALL
            .into_iter()
            .find(|mode| mode.name() == text)
            .ok_or(ValueError::UnknownChoice {
                name: "effect unit mode",
                expected: "group, single",
            })
    }
}

/// The values of one slot of group mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotValues {
    pub amount: f32,
    pub on: bool,
}

impl Default for SlotValues {
    fn default() -> Self {
        Self {
            amount: super::DEFAULT_AMOUNT,
            on: false,
        }
    }
}

/// The values of single mode.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SingleValues {
    pub controls: Controls,
    pub on: bool,
}

/// The values of a unit, shared by the chains of every channel it is assigned to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitValues {
    pub mode: UnitMode,
    pub dry_wet: f32,
    pub slots: [SlotValues; SLOTS],
    pub single: SingleValues,
}

impl Default for UnitValues {
    fn default() -> Self {
        Self {
            mode: UnitMode::Group,
            dry_wet: DEFAULT_DRY_WET,
            slots: [SlotValues::default(); SLOTS],
            single: SingleValues::default(),
        }
    }
}

/// Which slot of a chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Group(usize),
    Single,
}

/// One effect place: the effect, its on fade, its smoothed knobs, and the effect it replaced
/// while that one fades out.
pub struct Slot {
    effect: Option<Box<Effect>>,
    outgoing: Option<(Box<Effect>, Smoothed)>,
    on: Smoothed,
    was_on: bool,
    knobs: [Smoothed; 3],
    rate: SampleRate,
}

impl Slot {
    pub fn new(rate: SampleRate) -> Self {
        let ramp = |value| Smoothed::new(value, rate, SWITCH_MS);
        Self {
            effect: None,
            outgoing: None,
            on: ramp(0.0),
            was_on: false,
            knobs: [ramp(0.0); 3],
            rate,
        }
    }

    pub fn kind(&self) -> Option<EffectKind> {
        self.effect.as_ref().map(|effect| effect.kind())
    }

    /// Puts `effect` in the slot. The effect it held fades out; one still fading from an
    /// earlier swap is handed back at once, to be freed outside the audio callback.
    pub fn replace(&mut self, effect: Option<Box<Effect>>) -> Option<Box<Effect>> {
        let earlier = self.outgoing.take().map(|(effect, _)| effect);
        if let Some(current) = self.effect.take() {
            let mut fade = Smoothed::new(self.on.target(), self.rate, SWITCH_MS);
            fade.set(0.0);
            self.outgoing = Some((current, fade));
        }
        self.effect = effect;
        // The new effect fades in from silence, and starts its knobs from where they are.
        self.on = Smoothed::new(0.0, self.rate, SWITCH_MS);
        self.was_on = false;
        earlier
    }

    /// The effect that has finished fading out after a swap, to be freed outside the callback.
    pub fn take_finished(&mut self) -> Option<Box<Effect>> {
        let finished = self
            .outgoing
            .as_ref()
            .is_some_and(|(_, fade)| fade.is_settled());
        if finished {
            self.outgoing.take().map(|(effect, _)| effect)
        } else {
            None
        }
    }

    pub fn process(&mut self, input: Frame, controls: &Controls, on: bool, tempo: Tempo) -> Frame {
        self.on.set(if on { 1.0 } else { 0.0 });
        let gain = self.on.advance();
        let mut smoothed = *controls;
        for (knob, target) in self.knobs.iter_mut().zip(smoothed.knobs.iter_mut()) {
            knob.set(*target);
            *target = knob.advance();
        }
        let mut output = input;
        if let Some(effect) = &mut self.effect {
            if on != self.was_on {
                effect.switched(on);
                self.was_on = on;
            }
            if effect.kind().has_tail() {
                // The input closes as the effect switches off; the tail rings on.
                let fed = [input[0] * gain, input[1] * gain];
                let processed = effect.process(fed, &smoothed, tempo);
                for side in 0..2 {
                    output[side] += processed[side] - fed[side];
                }
            } else {
                let processed = effect.process(input, &smoothed, tempo);
                for side in 0..2 {
                    output[side] += gain * (processed[side] - input[side]);
                }
            }
        }
        if let Some((effect, fade)) = &mut self.outgoing {
            let weight = fade.advance();
            let processed = effect.process(input, &smoothed, tempo);
            for side in 0..2 {
                output[side] += weight * (processed[side] - input[side]);
            }
        }
        output
    }
}

/// A unit as one channel goes through it.
pub struct Chain {
    group: [Slot; SLOTS],
    single: Slot,
    dry_wet: Smoothed,
    /// Zero in group mode, one in single mode, between the two while the mode changes.
    single_weight: Smoothed,
}

impl Chain {
    pub fn new(rate: SampleRate) -> Self {
        Self {
            group: std::array::from_fn(|_| Slot::new(rate)),
            single: Slot::new(rate),
            dry_wet: Smoothed::new(DEFAULT_DRY_WET, rate, SWITCH_MS),
            single_weight: Smoothed::new(0.0, rate, SWITCH_MS),
        }
    }

    /// The slot at `place`; `None` for a group slot out of range.
    pub fn slot_mut(&mut self, place: Place) -> Option<&mut Slot> {
        match place {
            Place::Group(index) => self.group.get_mut(index),
            Place::Single => Some(&mut self.single),
        }
    }

    /// No slot has an effect fading out after a swap.
    pub fn is_idle(&self) -> bool {
        self.group
            .iter()
            .chain(std::iter::once(&self.single))
            .all(|slot| slot.outgoing.is_none())
    }

    /// An effect that has finished fading out, to be freed outside the callback.
    pub fn take_finished(&mut self) -> Option<Box<Effect>> {
        self.group
            .iter_mut()
            .chain(std::iter::once(&mut self.single))
            .find_map(Slot::take_finished)
    }

    pub fn process(&mut self, input: Frame, values: &UnitValues, tempo: Tempo) -> Frame {
        self.dry_wet.set(values.dry_wet);
        self.single_weight.set(match values.mode {
            UnitMode::Group => 0.0,
            UnitMode::Single => 1.0,
        });
        let dry_wet = self.dry_wet.advance();
        let weight = self.single_weight.advance();
        let mut wet = [0.0; 2];
        if weight < 1.0 {
            let mut signal = input;
            for (slot, slot_values) in self.group.iter_mut().zip(values.slots.iter()) {
                let controls = slot.kind().map_or_else(Controls::default, |kind| {
                    kind.group_controls(slot_values.amount)
                });
                signal = slot.process(signal, &controls, slot_values.on, tempo);
            }
            for side in 0..2 {
                wet[side] += (1.0 - weight) * signal[side];
            }
        }
        if weight > 0.0 {
            let single = &values.single;
            let signal = self
                .single
                .process(input, &single.controls, single.on, tempo);
            for side in 0..2 {
                wet[side] += weight * signal[side];
            }
        }
        [
            input[0] + dry_wet * (wet[0] - input[0]),
            input[1] + dry_wet * (wet[1] - input[1]),
        ]
    }
}

/// The one-knob effect of a channel: the knob drives the effect as a group slot does and sets
/// how much of it is heard. At zero it does nothing.
pub struct MixerEffect {
    slot: Slot,
    amount: Smoothed,
}

impl MixerEffect {
    pub fn new(rate: SampleRate) -> Self {
        Self {
            slot: Slot::new(rate),
            amount: Smoothed::new(0.0, rate, SWITCH_MS),
        }
    }

    pub fn kind(&self) -> Option<EffectKind> {
        self.slot.kind()
    }

    /// Puts `effect` behind the knob; see [`Slot::replace`].
    pub fn replace(&mut self, effect: Option<Box<Effect>>) -> Option<Box<Effect>> {
        self.slot.replace(effect)
    }

    pub fn take_finished(&mut self) -> Option<Box<Effect>> {
        self.slot.take_finished()
    }

    pub fn process(&mut self, input: Frame, amount: f32, tempo: Tempo) -> Frame {
        let amount = if amount.is_finite() {
            amount.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.amount.set(amount);
        let heard = self.amount.advance();
        let controls = self
            .slot
            .kind()
            .map_or_else(Controls::default, |kind| kind.group_controls(heard));
        let processed = self.slot.process(input, &controls, amount > 0.0, tempo);
        [
            input[0] + heard * (processed[0] - input[0]),
            input[1] + heard * (processed[1] - input[1]),
        ]
    }
}
