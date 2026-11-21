//! Channel strip of the mixer: gain, equaliser with a cut per band, filter, fader, balance
//! and meter, in that order. The signal for pre-listening is taken before the fader.
//! What the stems of a stem deck do not send to the effects takes a second path through
//! the gain, the equaliser and the filter, around every effect: `docs/behavior/stems.md`.

use platter_core::SampleRate;
use platter_dsp::Frame;
use platter_dsp::balance::balance_gains;
use platter_dsp::eq::{Band, EqModel, ThreeBandEq};
use platter_dsp::filter::{FilterModel, OneKnobFilter};
use platter_dsp::meter::PeakMeter;
use platter_dsp::smooth::Smoothed;

use super::effects::Stage;
use super::stems::Played;

/// Ramp of every mixer parameter: short enough to feel immediate, long enough not to click.
pub(super) const PARAMETER_RAMP_MS: f32 = 10.0;

/// What a strip produces for one frame.
pub(super) struct StripFrame {
    /// After the fader and the balance: what goes to the crossfader.
    pub(super) post: Frame,
    /// Before the fader: what pre-listening hears.
    pub(super) pre: Frame,
    /// After the gain and the equaliser only: what external mixer mode can send.
    pub(super) equalised: Frame,
    /// After the effects of the insert point.
    pub(super) inserted: Frame,
}

pub(super) struct ChannelStrip {
    gain: Smoothed,
    /// Gain each band knob asks for; a cut overrides it without forgetting it.
    eq_knobs: [f32; 3],
    eq_cuts: [bool; 3],
    eq_gains: [Smoothed; 3],
    eq: ThreeBandEq,
    filter_position: Smoothed,
    filter: OneKnobFilter,
    /// The path around the effects, used only while the deck plays stems. Boxed so the
    /// processor stays small enough to be built on the stack of any thread.
    around: Box<(ThreeBandEq, OneKnobFilter)>,
    fader: Smoothed,
    balance: [Smoothed; 2],
    meter: PeakMeter,
}

impl ChannelStrip {
    pub(super) fn new(rate: SampleRate) -> Self {
        let parameter = |value| Smoothed::new(value, rate, PARAMETER_RAMP_MS);
        Self {
            gain: parameter(1.0),
            eq_knobs: [1.0; 3],
            eq_cuts: [false; 3],
            eq_gains: [parameter(1.0); 3],
            eq: ThreeBandEq::new(rate),
            filter_position: parameter(0.0),
            filter: OneKnobFilter::new(rate),
            around: Box::new((ThreeBandEq::new(rate), OneKnobFilter::new(rate))),
            fader: parameter(1.0),
            balance: [parameter(1.0); 2],
            meter: PeakMeter::new(rate),
        }
    }

    pub(super) fn set_gain(&mut self, linear: f32) {
        self.gain.set(linear);
    }

    pub(super) fn set_eq(&mut self, band: Band, gain: f32) {
        self.eq_knobs[band as usize] = gain;
        self.retarget_band(band);
    }

    pub(super) fn set_eq_cut(&mut self, band: Band, cut: bool) {
        self.eq_cuts[band as usize] = cut;
        self.retarget_band(band);
    }

    fn retarget_band(&mut self, band: Band) {
        let index = band as usize;
        let gain = if self.eq_cuts[index] {
            0.0
        } else {
            self.eq_knobs[index]
        };
        self.eq_gains[index].set(gain);
    }

    pub(super) fn set_eq_model(&mut self, model: EqModel) {
        self.eq.set_model(model);
        self.around.0.set_model(model);
    }

    pub(super) fn set_filter_model(&mut self, model: FilterModel) {
        self.filter.set_model(model);
        self.around.1.set_model(model);
    }

    pub(super) fn set_filter(&mut self, position: f32) {
        self.filter_position.set(position);
    }

    pub(super) fn set_fader(&mut self, gain: f32) {
        self.fader.set(gain);
    }

    pub(super) fn set_balance(&mut self, position: f32) {
        let (left, right) = balance_gains(position);
        self.balance[0].set(left);
        self.balance[1].set(right);
    }

    /// Processes one frame; `effects` gives the frame after the effects of each stage.
    pub(super) fn process(
        &mut self,
        input: Played,
        stems: bool,
        mut effects: impl FnMut(Stage, Frame) -> Frame,
    ) -> StripFrame {
        let gain = self.gain.advance();
        let gains = [
            self.eq_gains[0].advance(),
            self.eq_gains[1].advance(),
            self.eq_gains[2].advance(),
        ];
        let equalised = self
            .eq
            .process([input.sent[0] * gain, input.sent[1] * gain], gains);
        let inserted = effects(Stage::Insert, equalised);
        let position = self.filter_position.advance();
        let filtered = self.filter.process(inserted, position);
        let pre = effects(Stage::Filtered, filtered);
        let (around_equalised, around) = if stems {
            let (eq, filter) = &mut *self.around;
            let equalised = eq.process([input.around[0] * gain, input.around[1] * gain], gains);
            (equalised, filter.process(equalised, position))
        } else {
            ([0.0; 2], [0.0; 2])
        };
        let fader = self.fader.advance();
        let balance = [self.balance[0].advance(), self.balance[1].advance()];
        let post = effects(
            Stage::PostFader,
            [pre[0] * fader * balance[0], pre[1] * fader * balance[1]],
        );
        StripFrame {
            post: [0, 1].map(|side| post[side] + around[side] * fader * balance[side]),
            pre: [0, 1].map(|side| pre[side] + around[side]),
            equalised: [0, 1].map(|side| equalised[side] + around_equalised[side]),
            inserted: [0, 1].map(|side| inserted[side] + around_equalised[side]),
        }
    }

    /// Feeds the meter with what the channel sends out: the processor knows which signal
    /// that is.
    pub(super) fn feed_meter(&mut self, frame: Frame) {
        self.meter.feed(frame);
    }

    /// Peak level of the left and the right side of what the channel sends out.
    pub(super) fn levels(&self) -> [f32; 2] {
        self.meter.levels()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_goes_around_the_effects_skips_them_and_keeps_the_fader() {
        let mut strip = ChannelStrip::new(SampleRate::HZ_48000);
        let input = Played {
            sent: [0.5, 0.5],
            around: [0.25, 0.25],
        };
        // Effects that silence what they get leave only the part around them.
        let mut frame = strip.process(input, true, |_, _| [0.0; 2]);
        for _ in 0..4800 {
            frame = strip.process(input, true, |_, _| [0.0; 2]);
        }
        assert!((frame.post[0] - 0.25).abs() < 1e-3, "{:?}", frame.post);
        assert!((frame.pre[1] - 0.25).abs() < 1e-3, "{:?}", frame.pre);
        // A track deck has nothing around.
        let track = strip.process(input, false, |_, _| [0.0; 2]);
        assert_eq!(track.post, [0.0; 2]);
        strip.set_fader(0.0);
        for _ in 0..4800 {
            frame = strip.process(input, true, |_, frame| frame);
        }
        assert!(frame.post[0].abs() < 1e-6);
    }
}
