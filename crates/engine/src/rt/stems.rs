//! The controls of the four stems of a stem deck: volume and mute, filter and effect send.
//!
//! Behaviour note: `docs/behavior/stems.md`. What a stem does not send goes around every
//! effect of the channel, so the deck gives the channel two signals: the part that goes
//! through the effects and the part that goes around them.

use platter_core::{SampleRate, Stem};
use platter_dsp::Frame;
use platter_dsp::filter::{FilterModel, OneKnobFilter};
use platter_dsp::smooth::Smoothed;

use super::strip::PARAMETER_RAMP_MS;

/// How one stem is set. While the filter is off the stem is not filtered, whatever the knob;
/// while the send is off the stem goes fully to the effects, as a track does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StemSettings {
    /// Level of the stem, `0.0..=1.0`.
    pub volume: f32,
    pub muted: bool,
    /// Knob position `-1.0..=1.0`: low-pass below zero, high-pass above.
    pub filter: f32,
    pub filter_on: bool,
    /// Share of the stem the effects get while the send is on, `0.0..=1.0`.
    pub send: f32,
    pub send_on: bool,
}

impl Default for StemSettings {
    fn default() -> Self {
        Self {
            volume: 1.0,
            muted: false,
            filter: 0.0,
            filter_on: false,
            send: 1.0,
            send_on: false,
        }
    }
}

/// What a deck plays for one frame: the part the effects of the channel get, and the part
/// that goes around them. A track deck has nothing around.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Played {
    pub(super) sent: Frame,
    pub(super) around: Frame,
}

pub(super) struct StemMix {
    volume: [Smoothed; Stem::COUNT],
    filter_position: [Smoothed; Stem::COUNT],
    filters: [OneKnobFilter; Stem::COUNT],
    send: [Smoothed; Stem::COUNT],
}

impl StemMix {
    pub(super) fn new(rate: SampleRate) -> Self {
        let parameter = |value| Smoothed::new(value, rate, PARAMETER_RAMP_MS);
        Self {
            volume: [parameter(1.0); Stem::COUNT],
            filter_position: [parameter(0.0); Stem::COUNT],
            filters: [(); Stem::COUNT].map(|()| OneKnobFilter::new(rate)),
            send: [parameter(1.0); Stem::COUNT],
        }
    }

    pub(super) fn set(&mut self, stem: Stem, settings: StemSettings) {
        let index = stem.index();
        let finite = |value: f32, low: f32, high: f32, default: f32| {
            if value.is_finite() {
                value.clamp(low, high)
            } else {
                default
            }
        };
        let volume = finite(settings.volume, 0.0, 1.0, 1.0);
        self.volume[index].set(if settings.muted { 0.0 } else { volume });
        let filter = finite(settings.filter, -1.0, 1.0, 0.0);
        self.filter_position[index].set(if settings.filter_on { filter } else { 0.0 });
        let send = finite(settings.send, 0.0, 1.0, 1.0);
        self.send[index].set(if settings.send_on { send } else { 1.0 });
    }

    pub(super) fn set_filter_model(&mut self, model: FilterModel) {
        for filter in &mut self.filters {
            filter.set_model(model);
        }
    }

    /// Mixes the four stems of one frame into what goes through the effects and what goes
    /// around them.
    pub(super) fn process(&mut self, parts: [Frame; Stem::COUNT]) -> Played {
        let mut played = Played::default();
        for (index, part) in parts.into_iter().enumerate() {
            let volume = self.volume[index].advance();
            let position = self.filter_position[index].advance();
            let filtered =
                self.filters[index].process([part[0] * volume, part[1] * volume], position);
            let send = self.send[index].advance();
            for (side, sample) in filtered.into_iter().enumerate() {
                played.sent[side] += sample * send;
                played.around[side] += sample * (1.0 - send);
            }
        }
        played
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle(mix: &mut StemMix, parts: [Frame; Stem::COUNT]) -> Played {
        let mut played = Played::default();
        for _ in 0..4800 {
            played = mix.process(parts);
        }
        played
    }

    fn rate() -> SampleRate {
        SampleRate::HZ_48000
    }

    #[test]
    fn by_default_the_stems_add_up_and_all_go_to_the_effects() {
        let mut mix = StemMix::new(rate());
        let played = settle(&mut mix, [[0.1, 0.2]; Stem::COUNT]);
        assert!((played.sent[0] - 0.4).abs() < 1e-5 && (played.sent[1] - 0.8).abs() < 1e-5);
        assert_eq!(played.around, [0.0, 0.0]);
    }

    #[test]
    fn a_muted_stem_is_silent_and_a_lowered_send_goes_around_the_effects() {
        let mut mix = StemMix::new(rate());
        let quiet = StemSettings {
            muted: true,
            ..StemSettings::default()
        };
        mix.set(Stem::Drums, quiet);
        let around = StemSettings {
            send: 0.25,
            send_on: true,
            ..StemSettings::default()
        };
        mix.set(Stem::Vocals, around);
        // The send knob means nothing while the send is off.
        let off = StemSettings {
            send: 0.0,
            ..StemSettings::default()
        };
        mix.set(Stem::Bass, off);
        let parts = [[1.0, 1.0], [0.0, 0.0], [0.0, 0.0], [0.5, 0.5]];
        let played = settle(&mut mix, parts);
        assert!((played.sent[0] - 0.125).abs() < 1e-5, "{played:?}");
        assert!((played.around[0] - 0.375).abs() < 1e-5, "{played:?}");
    }
}
