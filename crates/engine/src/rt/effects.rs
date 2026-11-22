//! The effect units and the mixer effects inside the processor: one chain per channel and unit,
//! fed the values of its unit, and the beat count the effects follow
//! (`docs/decisions/0033-effect-chain.md`). Everything here lives in one box, so the processor
//! stays small enough to build on any stack.

use platter_core::{DeckId, SampleRate, ValueError};
use platter_dsp::Frame;
use platter_dsp::effects::Tempo;
use platter_dsp::effects::unit::{Chain, MixerEffect, Place, SWITCH_MS, UnitValues};
use platter_dsp::smooth::Smoothed;
use rtrb::Producer;

use super::Retired;
use super::clock::Pulse;

/// The effect units the engine has.
pub const UNITS: usize = 4;

/// Where a unit sits in the channels it is assigned to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FxRouting {
    /// After the equaliser, before the filter.
    #[default]
    Insert,
    /// After the fader and the pan. Acts as insert in external mixer mode.
    PostFader,
    /// Outside the channels: fed by the FX Send input and returned to the third pair of
    /// outputs, in external mixer mode only (`docs/behavior/effects.md`, *Send*).
    Send,
}

impl FxRouting {
    pub const ALL: [Self; 3] = [Self::Insert, Self::PostFader, Self::Send];

    pub fn name(self) -> &'static str {
        match self {
            Self::Insert => "insert",
            Self::PostFader => "post_fader",
            Self::Send => "send",
        }
    }

    pub fn parse(text: &str) -> Result<Self, ValueError> {
        Self::ALL
            .into_iter()
            .find(|routing| routing.name() == text)
            .ok_or(ValueError::UnknownChoice {
                name: "routing",
                expected: "insert, post_fader, send",
            })
    }
}

/// Where in the channel strip the effects act.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stage {
    /// After the equaliser, before the filter.
    Insert,
    /// After the filter, before the fader: the mixer effect.
    Filtered,
    /// After the fader and the pan.
    PostFader,
}

struct ChannelEffects {
    chains: [Chain; UNITS],
    /// One while the channel goes through the unit, fading as it is assigned or taken out.
    assigned: [Smoothed; UNITS],
    mixer: MixerEffect,
    mixer_amount: f32,
}

struct Inner {
    values: [UnitValues; UNITS],
    routing: [FxRouting; UNITS],
    channels: [ChannelEffects; DeckId::COUNT],
    /// The chain of each unit for the FX Send: it holds the unit's effects whatever its
    /// routing, and is heard only while that routing is send.
    send: [Chain; UNITS],
    /// One while a unit is in send routing: the return fades in and out with it.
    send_gain: Smoothed,
    rate: f64,
    /// Whole beats of the clock since the processor started, and the phase seen last.
    beats: f64,
    last_phase: f64,
    tempo: Tempo,
}

pub(super) struct Effects(Box<Inner>);

impl Effects {
    pub(super) fn new(rate: SampleRate) -> Self {
        let channel = |_| ChannelEffects {
            chains: std::array::from_fn(|_| Chain::new(rate)),
            assigned: [Smoothed::new(0.0, rate, SWITCH_MS); UNITS],
            mixer: MixerEffect::new(rate),
            mixer_amount: 0.0,
        };
        Self(Box::new(Inner {
            values: [UnitValues::default(); UNITS],
            routing: [FxRouting::default(); UNITS],
            channels: std::array::from_fn(channel),
            send: std::array::from_fn(|_| Chain::new(rate)),
            send_gain: Smoothed::new(0.0, rate, SWITCH_MS),
            rate: rate.as_f64(),
            beats: 0.0,
            last_phase: 0.0,
            tempo: Tempo::new(120.0, rate, 0.0),
        }))
    }

    pub(super) fn set_values(&mut self, unit: usize, values: UnitValues) {
        if let Some(slot) = self.0.values.get_mut(unit) {
            *slot = values;
        }
    }

    pub(super) fn set_routing(&mut self, unit: usize, routing: FxRouting) {
        if let Some(slot) = self.0.routing.get_mut(unit) {
            *slot = routing;
        }
        let any = self.0.routing.contains(&FxRouting::Send);
        self.0.send_gain.set(if any { 1.0 } else { 0.0 });
    }

    pub(super) fn set_assigned(&mut self, deck: DeckId, unit: usize, on: bool) {
        if let Some(gain) = self.0.channels[deck.index()].assigned.get_mut(unit) {
            gain.set(if on { 1.0 } else { 0.0 });
        }
    }

    pub(super) fn set_mixer_amount(&mut self, deck: DeckId, amount: f32) {
        self.0.channels[deck.index()].mixer_amount = amount;
    }

    /// Puts an effect in a place of a channel's chain. Exactly one item goes back on the
    /// retired queue for this call, now or when the effect it replaced has faded out.
    pub(super) fn place(
        &mut self,
        deck: DeckId,
        unit: usize,
        place: Place,
        effect: Option<Box<platter_dsp::effects::Effect>>,
        retired: &mut Producer<Retired>,
    ) {
        let channel = &mut self.0.channels[deck.index()];
        let Some(slot) = channel
            .chains
            .get_mut(unit)
            .and_then(|chain| chain.slot_mut(place))
        else {
            retire(effect, retired);
            return;
        };
        let empty = slot.kind().is_none();
        if let Some(earlier) = slot.replace(effect) {
            retire(Some(earlier), retired);
        }
        if empty {
            retire(None, retired);
        }
    }

    /// Puts an effect in a place of the send chain of a unit; as [`Self::place`].
    pub(super) fn place_send(
        &mut self,
        unit: usize,
        place: Place,
        effect: Option<Box<platter_dsp::effects::Effect>>,
        retired: &mut Producer<Retired>,
    ) {
        let Some(slot) = self
            .0
            .send
            .get_mut(unit)
            .and_then(|chain| chain.slot_mut(place))
        else {
            retire(effect, retired);
            return;
        };
        let empty = slot.kind().is_none();
        if let Some(earlier) = slot.replace(effect) {
            retire(Some(earlier), retired);
        }
        if empty {
            retire(None, retired);
        }
    }

    /// The return of the FX Send: `frame` through the units in send routing, in the order
    /// 1, 2, 3, 4; silence while no unit is in send routing.
    pub(super) fn process_send(&mut self, frame: Frame) -> Frame {
        let inner = &mut *self.0;
        let gain = inner.send_gain.advance();
        if gain == 0.0 && inner.send_gain.is_settled() {
            return [0.0; 2];
        }
        let tempo = inner.tempo;
        let mut signal = frame;
        for unit in 0..UNITS {
            if inner.routing[unit] == FxRouting::Send {
                signal = inner.send[unit].process(signal, &inner.values[unit], tempo);
            }
        }
        [signal[0] * gain, signal[1] * gain]
    }

    /// Puts an effect behind the mixer effect knob of a channel; as [`Self::place`].
    pub(super) fn mixer_place(
        &mut self,
        deck: DeckId,
        effect: Option<Box<platter_dsp::effects::Effect>>,
        retired: &mut Producer<Retired>,
    ) {
        let mixer = &mut self.0.channels[deck.index()].mixer;
        let empty = mixer.kind().is_none();
        if let Some(earlier) = mixer.replace(effect) {
            retire(Some(earlier), retired);
        }
        if empty {
            retire(None, retired);
        }
    }

    /// Advances the beat count by one frame of the clock.
    pub(super) fn follow(&mut self, pulse: Pulse) {
        let inner = &mut *self.0;
        let phase = pulse.phase;
        if phase < inner.last_phase - 0.5 {
            inner.beats += 1.0;
        } else if phase > inner.last_phase + 0.5 {
            inner.beats -= 1.0;
        }
        inner.last_phase = phase;
        let bpm = if pulse.bpm.is_finite() && pulse.bpm > 0.0 {
            pulse.bpm
        } else {
            120.0
        };
        inner.tempo = Tempo {
            samples_per_beat: (inner.rate * 60.0 / bpm) as f32,
            beat: inner.beats + phase,
        };
    }

    /// The frame of a channel after the effects of `stage`. In external mixer mode the units
    /// in post-fader routing act at the insert point.
    pub(super) fn process(
        &mut self,
        deck: DeckId,
        stage: Stage,
        frame: Frame,
        external: bool,
    ) -> Frame {
        let inner = &mut *self.0;
        let tempo = inner.tempo;
        let channel = &mut inner.channels[deck.index()];
        if stage == Stage::Filtered {
            return channel.mixer.process(frame, channel.mixer_amount, tempo);
        }
        let mut signal = frame;
        for unit in 0..UNITS {
            let acts = match (inner.routing[unit], stage) {
                (FxRouting::Insert, Stage::Insert) => true,
                (FxRouting::PostFader, Stage::Insert) => external,
                (FxRouting::PostFader, Stage::PostFader) => !external,
                _ => false,
            };
            let assigned = &mut channel.assigned[unit];
            // A chain no channel hears is skipped, once whatever it swapped out has faded.
            let silent = assigned.is_settled() && assigned.target() == 0.0;
            if !acts || (silent && channel.chains[unit].is_idle()) {
                continue;
            }
            let gain = assigned.advance();
            let processed = channel.chains[unit].process(signal, &inner.values[unit], tempo);
            signal = [
                signal[0] + gain * (processed[0] - signal[0]),
                signal[1] + gain * (processed[1] - signal[1]),
            ];
        }
        signal
    }

    /// Sends the effects that have finished fading out back to be freed. Called once a block.
    pub(super) fn collect(&mut self, retired: &mut Producer<Retired>) {
        for chain in &mut self.0.send {
            while let Some(effect) = chain.take_finished() {
                retire(Some(effect), retired);
            }
        }
        for channel in &mut self.0.channels {
            for chain in &mut channel.chains {
                while let Some(effect) = chain.take_finished() {
                    retire(Some(effect), retired);
                }
            }
            if let Some(effect) = channel.mixer.take_finished() {
                retire(Some(effect), retired);
            }
        }
    }
}

/// Hands an effect, or the mark of a swap that freed nothing, to the retired queue. The queue
/// has room for every swap in flight; were it full, leaking is the only way to keep the audio
/// thread from freeing memory.
fn retire(effect: Option<Box<platter_dsp::effects::Effect>>, retired: &mut Producer<Retired>) {
    if let Err(rtrb::PushError::Full(item)) = retired.push(Retired(None, None, effect)) {
        std::mem::forget(item);
    }
}
