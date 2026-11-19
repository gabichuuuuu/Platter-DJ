//! The effects of the effect units (`docs/behavior/effects.md`). Each effect is built on a
//! normal thread, with its buffers, and then runs one frame at a time without allocating.
//!
//! An effect returns its whole processed signal. The time-based ones, delay and reverb, return
//! the input with their tail added; the others return the input transformed. A [`unit::Slot`]
//! switches an effect on and off and [`unit::Chain`] mixes a unit's chain with its input.
//!
//! Every effect reads three knobs between 0 and 1 and two keys ([`Controls`]), and the tempo
//! and beat of the master clock ([`Tempo`]). The design of the chain is
//! `docs/decisions/0033-effect-chain.md`.

mod bitcrusher;
mod delay;
mod filter;
mod gater;
mod modulation;
mod reverb;
mod shared;
mod slicer;
pub mod unit;

use platter_core::{SampleRate, ValueError};

use crate::Frame;

use bitcrusher::Bitcrusher;
use delay::Delay;
use filter::Filter;
use gater::Gater;
use modulation::{Flanger, Phaser};
use reverb::Reverb;
use slicer::Slicer;

/// The longest stretch of audio an effect keeps, in seconds: four beats at 60 beats per
/// minute. At slower tempos the longest times halve until they fit.
pub const MAX_BUFFER_SECONDS: f32 = 4.0;

/// The amount a group slot starts from: the middle of the travel, where the one-knob filter
/// leaves the signal alone.
pub const DEFAULT_AMOUNT: f32 = 0.5;

/// One of the effects of a unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectKind {
    Delay,
    Reverb,
    Flanger,
    Phaser,
    Filter,
    Gater,
    Slicer,
    Bitcrusher,
}

impl EffectKind {
    pub const ALL: [Self; 8] = [
        Self::Delay,
        Self::Reverb,
        Self::Flanger,
        Self::Phaser,
        Self::Filter,
        Self::Gater,
        Self::Slicer,
        Self::Bitcrusher,
    ];

    /// The effects the mixer effect knob of a channel offers.
    pub const MIXER: [Self; 4] = [Self::Delay, Self::Reverb, Self::Gater, Self::Bitcrusher];

    pub fn name(self) -> &'static str {
        match self {
            Self::Delay => "delay",
            Self::Reverb => "reverb",
            Self::Flanger => "flanger",
            Self::Phaser => "phaser",
            Self::Filter => "filter",
            Self::Gater => "gater",
            Self::Slicer => "slicer",
            Self::Bitcrusher => "bitcrusher",
        }
    }

    pub fn parse(text: &str) -> Result<Self, ValueError> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.name() == text)
            .ok_or(ValueError::UnknownChoice {
                name: "effect",
                expected: "delay, reverb, flanger, phaser, filter, gater, slicer, bitcrusher",
            })
    }

    /// The tail of the effect rings on after it is switched off.
    pub fn has_tail(self) -> bool {
        matches!(self, Self::Delay | Self::Reverb)
    }

    /// The controls a newly selected effect starts from, in single mode.
    pub fn defaults(self) -> Controls {
        let (knobs, keys) = match self {
            Self::Delay => ([0.0, 0.5, delay::DEFAULT_TIME], [false; 2]),
            Self::Reverb => ([0.0, 0.2, 0.5], [false; 2]),
            Self::Flanger | Self::Phaser => ([0.25, 0.5, modulation::DEFAULT_RATE], [false; 2]),
            Self::Filter => ([0.0, 0.2, 0.0], [false; 2]),
            Self::Gater => ([0.0, 0.3, gater::DEFAULT_RATE], [false; 2]),
            Self::Slicer => ([1.0, 0.0, slicer::DEFAULT_LENGTH], [false; 2]),
            Self::Bitcrusher => ([0.5, 0.5, 0.0], [false; 2]),
        };
        Controls { knobs, keys }
    }

    /// What the one amount knob of a group slot drives, as the controls of single mode.
    pub fn group_controls(self, amount: f32) -> Controls {
        let amount = if amount.is_finite() {
            amount.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (knobs, keys) = match self {
            Self::Delay => ([0.0, 0.5, amount], [false; 2]),
            Self::Reverb => ([0.0, 0.2, amount], [false; 2]),
            Self::Flanger | Self::Phaser => ([0.25, 0.5, amount], [false; 2]),
            Self::Filter => ([0.0, 0.2, amount], [false, true]),
            Self::Gater => ([0.0, 0.3, amount], [false; 2]),
            Self::Slicer => ([1.0, 0.0, amount], [false; 2]),
            Self::Bitcrusher => ([amount, amount, 0.0], [false; 2]),
        };
        Controls { knobs, keys }
    }
}

/// The three knobs, between 0 and 1, and the two keys of an effect.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Controls {
    pub knobs: [f32; 3],
    pub keys: [bool; 2],
}

/// Where the master clock is: the length of a beat and the position, in beats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tempo {
    pub samples_per_beat: f32,
    pub beat: f64,
}

impl Tempo {
    /// The tempo of `bpm` at `rate`, at beat `beat`. A tempo that is not a positive number
    /// reads as 120 beats per minute.
    pub fn new(bpm: f64, rate: SampleRate, beat: f64) -> Self {
        let bpm = if bpm.is_finite() && bpm > 0.0 {
            bpm
        } else {
            120.0
        };
        Self {
            samples_per_beat: (rate.as_f64() * 60.0 / bpm) as f32,
            beat: if beat.is_finite() { beat } else { 0.0 },
        }
    }
}

enum State {
    Delay(Delay),
    Reverb(Reverb),
    Flanger(Flanger),
    Phaser(Phaser),
    Filter(Filter),
    Gater(Gater),
    Slicer(Slicer),
    Bitcrusher(Bitcrusher),
}

/// One effect with its state. Built on a normal thread: the buffers are allocated here.
pub struct Effect {
    kind: EffectKind,
    state: State,
}

impl std::fmt::Debug for Effect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Effect").field("kind", &self.kind).finish()
    }
}

impl Effect {
    pub fn new(kind: EffectKind, rate: SampleRate) -> Self {
        let state = match kind {
            EffectKind::Delay => State::Delay(Delay::new(rate)),
            EffectKind::Reverb => State::Reverb(Reverb::new(rate)),
            EffectKind::Flanger => State::Flanger(Flanger::new(rate)),
            EffectKind::Phaser => State::Phaser(Phaser::new(rate)),
            EffectKind::Filter => State::Filter(Filter::new(rate)),
            EffectKind::Gater => State::Gater(Gater::new(rate)),
            EffectKind::Slicer => State::Slicer(Slicer::new(rate)),
            EffectKind::Bitcrusher => State::Bitcrusher(Bitcrusher::new(rate)),
        };
        Self { kind, state }
    }

    pub fn kind(&self) -> EffectKind {
        self.kind
    }

    /// Processes one frame. An input that is not a finite number clears the effect and gives
    /// silence; an output that is not one clears it and gives the input back.
    pub fn process(&mut self, input: Frame, controls: &Controls, tempo: Tempo) -> Frame {
        if !input.iter().all(|sample| sample.is_finite()) {
            self.reset();
            return [0.0; 2];
        }
        let output = match &mut self.state {
            State::Delay(effect) => effect.process(input, controls, tempo),
            State::Reverb(effect) => effect.process(input, controls),
            State::Flanger(effect) => effect.process(input, controls, tempo),
            State::Phaser(effect) => effect.process(input, controls, tempo),
            State::Filter(effect) => effect.process(input, controls),
            State::Gater(effect) => effect.process(input, controls, tempo),
            State::Slicer(effect) => effect.process(input, controls, tempo),
            State::Bitcrusher(effect) => effect.process(input, controls),
        };
        if output.iter().all(|sample| sample.is_finite()) {
            output
        } else {
            self.reset();
            input
        }
    }

    /// Tells the effect it was switched on or off. The slicer captures a bar when it is
    /// switched on.
    pub fn switched(&mut self, on: bool) {
        if let State::Slicer(effect) = &mut self.state {
            effect.switched(on);
        }
    }

    /// Clears every buffer and every running state.
    pub fn reset(&mut self) {
        match &mut self.state {
            State::Delay(effect) => effect.reset(),
            State::Reverb(effect) => effect.reset(),
            State::Flanger(effect) => effect.reset(),
            State::Phaser(effect) => effect.reset(),
            State::Filter(effect) => effect.reset(),
            State::Gater(effect) => effect.reset(),
            State::Slicer(effect) => effect.reset(),
            State::Bitcrusher(effect) => effect.reset(),
        }
    }
}
