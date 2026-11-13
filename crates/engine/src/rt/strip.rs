//! Channel strip of the mixer: gain, equaliser, filter, fader and meter, in that order.

use platter_core::SampleRate;
use platter_dsp::Frame;
use platter_dsp::eq::{Band, ThreeBandEq};
use platter_dsp::filter::OneKnobFilter;
use platter_dsp::meter::PeakMeter;
use platter_dsp::smooth::Smoothed;

/// Ramp of every mixer parameter: short enough to feel immediate, long enough not to click.
pub(super) const PARAMETER_RAMP_MS: f32 = 10.0;

pub(super) struct ChannelStrip {
    gain: Smoothed,
    eq_gains: [Smoothed; 3],
    eq: ThreeBandEq,
    filter_position: Smoothed,
    filter: OneKnobFilter,
    fader: Smoothed,
    meter: PeakMeter,
}

impl ChannelStrip {
    pub(super) fn new(rate: SampleRate) -> Self {
        let parameter = |value| Smoothed::new(value, rate, PARAMETER_RAMP_MS);
        Self {
            gain: parameter(1.0),
            eq_gains: [parameter(1.0); 3],
            eq: ThreeBandEq::new(rate),
            filter_position: parameter(0.0),
            filter: OneKnobFilter::new(rate),
            fader: parameter(1.0),
            meter: PeakMeter::new(rate),
        }
    }

    pub(super) fn set_gain(&mut self, linear: f32) {
        self.gain.set(linear);
    }

    pub(super) fn set_eq(&mut self, band: Band, gain: f32) {
        self.eq_gains[band as usize].set(gain);
    }

    pub(super) fn set_filter(&mut self, position: f32) {
        self.filter_position.set(position);
    }

    pub(super) fn set_fader(&mut self, gain: f32) {
        self.fader.set(gain);
    }

    pub(super) fn process(&mut self, input: Frame) -> Frame {
        let gain = self.gain.advance();
        let gains = [
            self.eq_gains[0].advance(),
            self.eq_gains[1].advance(),
            self.eq_gains[2].advance(),
        ];
        let equalised = self.eq.process([input[0] * gain, input[1] * gain], gains);
        let filtered = self
            .filter
            .process(equalised, self.filter_position.advance());
        let fader = self.fader.advance();
        let output = [filtered[0] * fader, filtered[1] * fader];
        self.meter.feed(output);
        output
    }

    pub(super) fn level(&self) -> f32 {
        self.meter.level()
    }
}
