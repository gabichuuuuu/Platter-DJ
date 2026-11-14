//! Channel strip of the mixer: gain, equaliser with a cut per band, filter, fader, balance
//! and meter, in that order. The signal for pre-listening is taken before the fader.

use platter_core::SampleRate;
use platter_dsp::Frame;
use platter_dsp::balance::balance_gains;
use platter_dsp::eq::{Band, ThreeBandEq};
use platter_dsp::filter::OneKnobFilter;
use platter_dsp::meter::PeakMeter;
use platter_dsp::smooth::Smoothed;

/// Ramp of every mixer parameter: short enough to feel immediate, long enough not to click.
pub(super) const PARAMETER_RAMP_MS: f32 = 10.0;

/// What a strip produces for one frame.
pub(super) struct StripFrame {
    /// After the fader and the balance: what goes to the crossfader.
    pub(super) post: Frame,
    /// Before the fader: what pre-listening hears.
    pub(super) pre: Frame,
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

    pub(super) fn process(&mut self, input: Frame) -> StripFrame {
        let gain = self.gain.advance();
        let gains = [
            self.eq_gains[0].advance(),
            self.eq_gains[1].advance(),
            self.eq_gains[2].advance(),
        ];
        let equalised = self.eq.process([input[0] * gain, input[1] * gain], gains);
        let pre = self
            .filter
            .process(equalised, self.filter_position.advance());
        let fader = self.fader.advance();
        let post = [
            pre[0] * fader * self.balance[0].advance(),
            pre[1] * fader * self.balance[1].advance(),
        ];
        self.meter.feed(post);
        StripFrame { post, pre }
    }

    /// Post-fader peak level of the left and the right side.
    pub(super) fn levels(&self) -> [f32; 2] {
        self.meter.levels()
    }
}
