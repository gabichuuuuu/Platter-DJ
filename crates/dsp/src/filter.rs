//! One-knob channel filter: low-pass when turned one way, high-pass the other, and no
//! effect at the centre. Three models share the knob and differ in slope and resonance:
//!
//! - [`FilterModel::Gentle`]: a second-order state-variable filter with a slight emphasis at
//!   the corner, 12 dB per octave, the first model (`docs/decisions/0006-channel-filter.md`).
//! - [`FilterModel::Steep`]: two state-variable filters in series with the quality factors
//!   of a fourth-order Butterworth filter: 24 dB per octave, no emphasis at the corner.
//! - [`FilterModel::Ladder`]: four one-pole stages in a loop with negative feedback, 24 dB
//!   per octave with a resonant peak at the corner.
//!
//! Every model runs in its trapezoidal (zero-delay feedback) form, which stays stable while
//! the corner moves every sample. Sources: A. Simper, "Linear Trapezoidal Integrated SVF"
//! (<https://cytomic.com/files/dsp/SvfLinearTrapOptimised2.pdf>), for the state-variable
//! filter and the trapezoidal one-pole stage; T. Stilson and J. Smith, "Analyzing the Moog
//! VCF with Considerations for Digital Implementation", ICMC 1996
//! (<https://ccrma.stanford.edu/~stilti/papers/moogvcf.pdf>), for the four-stage loop and
//! its feedback. The choice of the models is recorded in `docs/decisions/0032-mixer-models.md`.

use std::f32::consts::PI;

use platter_core::{SampleRate, ValueError};

use crate::Frame;
use crate::switch::{ModelSwitch, blend};

/// Half-width of the zone around the centre where the filter fades in.
pub const BLEND_ZONE: f32 = 0.05;
/// How long a change of model fades, in milliseconds.
pub const MODEL_FADE_MS: f32 = 10.0;
const GENTLE_Q: f32 = 0.9;
/// Quality factors of the two sections of a fourth-order Butterworth filter.
const STEEP_Q: [f32; 2] = [0.541_196_1, 1.306_563];
/// Feedback of the ladder. Four stages oscillate by themselves at 4; at 2 the peak at the
/// corner is about 8 dB above the pass band, measured at 48 kHz.
const LADDER_FEEDBACK: f32 = 2.0;
const LOW_PASS_RANGE_HZ: (f32, f32) = (20_000.0, 60.0);
const HIGH_PASS_RANGE_HZ: (f32, f32) = (20.0, 10_000.0);

/// The model of the filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilterModel {
    /// 12 dB per octave with a slight emphasis at the corner.
    #[default]
    Gentle,
    /// 24 dB per octave without emphasis.
    Steep,
    /// 24 dB per octave with a resonant peak at the corner.
    Ladder,
}

impl FilterModel {
    pub const ALL: [Self; 3] = [Self::Gentle, Self::Steep, Self::Ladder];

    pub fn name(self) -> &'static str {
        match self {
            Self::Gentle => "gentle",
            Self::Steep => "steep",
            Self::Ladder => "ladder",
        }
    }

    pub fn parse(text: &str) -> Result<Self, ValueError> {
        Self::ALL
            .into_iter()
            .find(|model| model.name() == text)
            .ok_or(ValueError::UnknownChoice {
                name: "filter model",
                expected: "gentle, steep, ladder",
            })
    }
}

/// Coefficients of a state-variable filter in its trapezoidal form.
#[derive(Clone, Copy, Default)]
struct SvfCoefficients {
    a1: f32,
    a2: f32,
    a3: f32,
    /// Damping: the inverse of the quality factor.
    k: f32,
}

impl SvfCoefficients {
    fn new(g: f32, q: f32) -> Self {
        let k = 1.0 / q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        Self {
            a1,
            a2,
            a3: g * a2,
            k,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Svf {
    fn process(&mut self, input: f32, c: &SvfCoefficients, high_pass: bool) -> f32 {
        let v3 = input - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = flush(2.0 * v1 - self.ic1);
        self.ic2 = flush(2.0 * v2 - self.ic2);
        if high_pass { input - c.k * v1 - v2 } else { v2 }
    }
}

/// Four one-pole stages; `g` is the prewarped corner.
#[derive(Clone, Copy, Default)]
struct Ladder {
    stages: [f32; 4],
}

impl Ladder {
    fn process(&mut self, input: f32, g: f32, high_pass: bool) -> f32 {
        let big_g = g / (1.0 + g);
        let h = 1.0 / (1.0 + g);
        let k = LADDER_FEEDBACK;
        let [s1, s2, s3, s4] = self.stages;
        // The output of the last stage is a known gain times the input of the first plus a
        // part that depends on the states only; solving the loop for the input of the first
        // stage removes the delay a naive feedback would add.
        let input = input * (1.0 + k);
        let first = if high_pass {
            let free = h * (h * (h * (h * s1 + s2) + s3) + s4);
            (input + k * free) / (1.0 + k * h.powi(4))
        } else {
            let free = h * (big_g * (big_g * (big_g * s1 + s2) + s3) + s4);
            (input - k * free) / (1.0 + k * big_g.powi(4))
        };
        let mut signal = first;
        for state in &mut self.stages {
            let v = (signal - *state) * big_g;
            let low = v + *state;
            *state = flush(low + v);
            signal = if high_pass { signal - low } else { low };
        }
        signal
    }
}

/// The state of one side for every model.
#[derive(Clone, Copy, Default)]
struct Side {
    gentle: Svf,
    steep: [Svf; 2],
    ladder: Ladder,
}

impl Side {
    fn clear(&mut self, model: FilterModel) {
        match model {
            FilterModel::Gentle => self.gentle = Svf::default(),
            FilterModel::Steep => self.steep = [Svf::default(); 2],
            FilterModel::Ladder => self.ladder = Ladder::default(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct OneKnobFilter {
    rate: f32,
    position: f32,
    // Coefficients, kept between samples while the knob rests.
    g: f32,
    gentle: SvfCoefficients,
    steep: [SvfCoefficients; 2],
    high_pass: bool,
    wet: f32,
    sides: [Side; 2],
    switch: ModelSwitch<FilterModel>,
}

impl OneKnobFilter {
    pub fn new(rate: SampleRate) -> Self {
        let mut filter = Self {
            rate: rate.as_f32(),
            position: f32::NAN,
            g: 0.0,
            gentle: SvfCoefficients::default(),
            steep: [SvfCoefficients::default(); 2],
            high_pass: false,
            wet: 0.0,
            sides: [Side::default(); 2],
            switch: ModelSwitch::new(FilterModel::default(), rate, MODEL_FADE_MS),
        };
        filter.retune(0.0);
        filter
    }

    /// The model asked for last.
    pub fn model(&self) -> FilterModel {
        self.switch.active()
    }

    /// Changes the model; the new one fades in over [`MODEL_FADE_MS`].
    pub fn set_model(&mut self, model: FilterModel) {
        self.switch.request(model);
    }

    /// Processes one frame with the knob at `position`: `-1.0` is the lowest low-pass,
    /// `0.0` leaves the signal untouched and `1.0` is the highest high-pass. The caller
    /// smooths the position. A position that is not a number is treated as the centre.
    pub fn process(&mut self, frame: Frame, position: f32) -> Frame {
        let position = if position.is_finite() {
            position.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        if position != self.position {
            self.retune(position);
        }
        let (active, weight, previous) = self.switch.advance();
        if let Some(started) = self.switch.take_started() {
            self.sides.iter_mut().for_each(|side| side.clear(started));
        }
        let mut output = frame;
        for (index, sample) in output.iter_mut().enumerate() {
            let mut filtered = self.run(active, index, *sample);
            if let Some(previous) = previous {
                let fading = self.run(previous, index, *sample);
                filtered = blend(filtered, fading, weight);
            }
            *sample += self.wet * (filtered - *sample);
        }
        output
    }

    fn run(&mut self, model: FilterModel, side: usize, input: f32) -> f32 {
        let high_pass = self.high_pass;
        let Some(state) = self.sides.get_mut(side) else {
            return input;
        };
        match model {
            FilterModel::Gentle => state.gentle.process(input, &self.gentle, high_pass),
            FilterModel::Steep => {
                let [first, second] = &mut state.steep;
                let once = first.process(input, &self.steep[0], high_pass);
                second.process(once, &self.steep[1], high_pass)
            }
            FilterModel::Ladder => state.ladder.process(input, self.g, high_pass),
        }
    }

    pub fn reset(&mut self) {
        self.sides = [Side::default(); 2];
    }

    fn retune(&mut self, position: f32) {
        self.position = position;
        self.high_pass = position > 0.0;
        let travel = position.abs();
        self.wet = (travel / BLEND_ZONE).min(1.0);
        let (from, to) = if self.high_pass {
            HIGH_PASS_RANGE_HZ
        } else {
            LOW_PASS_RANGE_HZ
        };
        // Exponential sweep: equal knob travel moves the corner by equal musical intervals.
        let hz = (from * (to / from).powf(travel)).clamp(10.0, 0.45 * self.rate);
        self.g = (PI * hz / self.rate).tan();
        self.gentle = SvfCoefficients::new(self.g, GENTLE_Q);
        self.steep = STEEP_Q.map(|q| SvfCoefficients::new(self.g, q));
    }
}

/// Keeps a decaying state out of the denormal range, where arithmetic becomes slow.
fn flush(value: f32) -> f32 {
    if value.abs() < 1e-25 { 0.0 } else { value }
}

#[cfg(test)]
mod tests {
    use platter_core::SampleRate;
    use platter_testkit::measure::{db, max_step, rms};
    use platter_testkit::signal::sine;

    use super::{FilterModel, OneKnobFilter};

    const RATE: u32 = 48_000;

    fn model_response_db(model: FilterModel, rate: u32, hz: f64, position: f32) -> f64 {
        let mut filter = OneKnobFilter::new(SampleRate::new(rate).unwrap());
        filter.set_model(model);
        let samples = rate as usize;
        let input = sine(hz, rate, samples, 0.5);
        let output: Vec<f32> = input
            .iter()
            .map(|x| filter.process([*x, *x], position)[1])
            .collect();
        let settled = samples / 2;
        db(rms(&output[settled..]).max(1e-12) / rms(&input[settled..]))
    }

    fn response_db(hz: f64, position: f32) -> f64 {
        model_response_db(FilterModel::Gentle, RATE, hz, position)
    }

    #[test]
    fn the_centre_leaves_the_signal_untouched_in_every_model() {
        for model in FilterModel::ALL {
            let mut filter = OneKnobFilter::new(SampleRate::new(RATE).unwrap());
            filter.set_model(model);
            for x in sine(3_000.0, RATE, 4_800, 0.8) {
                assert_eq!(filter.process([x, -x], 0.0), [x, -x], "{model:?}");
            }
        }
    }

    #[test]
    fn turning_left_is_a_low_pass() {
        assert!(response_db(100.0, -0.5).abs() < 1.0);
        assert!(response_db(10_000.0, -0.5) < -30.0);
        // Fully left the corner sits at 60 Hz.
        assert!(response_db(1_000.0, -1.0) < -40.0);
    }

    #[test]
    fn turning_right_is_a_high_pass() {
        assert!(response_db(10_000.0, 0.5).abs() < 1.0);
        assert!(response_db(40.0, 0.5) < -30.0);
        assert!(response_db(500.0, 1.0) < -40.0);
    }

    /// Half way to the left the corner sits near 1.1 kHz: the pass band, the octave from one
    /// to two octaves above the corner, and the peak around the corner of each model, at
    /// each common rate.
    #[test]
    fn each_model_has_its_slope_and_its_peak() {
        for rate in [44_100, 48_000, 96_000] {
            for model in FilterModel::ALL {
                let response = |hz| model_response_db(model, rate, hz, -0.5);
                let pass = response(100.0);
                let slope = response(2_200.0) - response(4_400.0);
                let peak = (0..16)
                    .map(|step| response(600.0 * 1.05f64.powi(step)))
                    .fold(f64::MIN, f64::max);
                let (slope_range, peak_range) = match model {
                    FilterModel::Gentle => (11.0..14.0, 0.0..1.0),
                    FilterModel::Steep => (22.0..26.0, -1.0..0.5),
                    FilterModel::Ladder => (19.0..26.0, 6.5..9.0),
                };
                assert!(
                    pass.abs() < 0.5 && slope_range.contains(&slope) && peak_range.contains(&peak),
                    "{model:?} at {rate} Hz: pass {pass:.2}, octave {slope:.1}, peak {peak:.2}"
                );
            }
        }
    }

    #[test]
    fn the_high_pass_of_every_model_mirrors_its_low_pass() {
        for model in FilterModel::ALL {
            let pass = model_response_db(model, RATE, 12_000.0, 0.5);
            let stop = model_response_db(model, RATE, 40.0, 0.5);
            assert!(
                pass.abs() < 0.5 && stop < -30.0,
                "{model:?}: {pass:.2} {stop:.1}"
            );
        }
    }

    #[test]
    fn sweeping_the_knob_neither_clicks_nor_blows_up_in_any_model() {
        for model in FilterModel::ALL {
            let mut filter = OneKnobFilter::new(SampleRate::new(RATE).unwrap());
            filter.set_model(model);
            let input = sine(200.0, RATE, 96_000, 0.5);
            let output: Vec<f32> = input
                .iter()
                .enumerate()
                .map(|(n, x)| {
                    let position = (n as f32 / 96_000.0) * 2.0 - 1.0;
                    filter.process([*x, *x], position)[0]
                })
                .collect();
            // The ladder peaks by its resonance where the corner crosses the tone.
            let ceiling = if model == FilterModel::Ladder {
                1.5
            } else {
                1.0
            };
            assert!(
                output.iter().all(|y| y.is_finite() && y.abs() < ceiling),
                "{model:?}"
            );
            assert!(max_step(&output) < 4.0 * max_step(&input), "{model:?}");
        }
    }

    #[test]
    fn a_change_of_model_fades_without_a_click() {
        let mut filter = OneKnobFilter::new(SampleRate::new(RATE).unwrap());
        assert_eq!(filter.model(), FilterModel::Gentle);
        let input = sine(300.0, RATE, 48_000, 0.5);
        let mut output = Vec::new();
        for (n, x) in input.iter().enumerate() {
            if n % 4_800 == 2_400 {
                filter.set_model(FilterModel::ALL[(n / 4_800) % FilterModel::ALL.len()]);
            }
            output.push(filter.process([*x, *x], -0.4)[0]);
        }
        assert!(output.iter().all(|y| y.is_finite()));
        assert!(
            max_step(&output) < 2.0 * max_step(&input),
            "{} against {}",
            max_step(&output),
            max_step(&input)
        );
    }

    #[test]
    fn positions_that_are_not_numbers_act_as_the_centre() {
        let mut filter = OneKnobFilter::new(SampleRate::new(RATE).unwrap());
        assert_eq!(filter.process([0.25, 0.5], f32::NAN), [0.25, 0.5]);
        assert_eq!(filter.process([0.25, 0.5], f32::INFINITY), [0.25, 0.5]);
    }

    #[test]
    fn model_names_round_trip() {
        for model in FilterModel::ALL {
            assert_eq!(FilterModel::parse(model.name()), Ok(model));
        }
        assert!(FilterModel::parse("comb").is_err());
    }
}
