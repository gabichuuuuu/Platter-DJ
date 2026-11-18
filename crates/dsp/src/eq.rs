//! Three-band equaliser with full kill, in four models that differ in the slope of their
//! band edges.
//!
//! The signal is split into three bands at 250 Hz and 2.5 kHz, each band is scaled, and the
//! bands are summed. With the three gains at unity the sum has a flat magnitude; with a gain
//! at zero the band is removed, not merely attenuated. The models:
//!
//! - [`EqModel::Smooth`]: first-order crossovers, 6 dB per octave. A first-order low-pass
//!   and high-pass add up to the input exactly.
//! - [`EqModel::Soft`]: second-order Linkwitz-Riley crossovers, 12 dB per octave, with the
//!   upper band inverted so the two add up to an all-pass.
//! - [`EqModel::Isolator`]: fourth-order Linkwitz-Riley crossovers, 24 dB per octave, the
//!   first model (`docs/decisions/0005-equaliser-topology.md`).
//! - [`EqModel::Steep`]: eighth-order Linkwitz-Riley crossovers, 48 dB per octave.
//!
//! The choice of the models is recorded in `docs/decisions/0032-mixer-models.md`.
//!
//! Crossover design: S. Linkwitz, "Active Crossover Networks for Noncoincident Drivers",
//! J. Audio Eng. Soc. 24(1), 1976 (<https://www.linkwitzlab.com/crossovers.htm>).

use platter_core::{SampleRate, ValueError};

use crate::Frame;
use crate::biquad::{BUTTERWORTH_Q, Biquad, Coefficients};
use crate::switch::{ModelSwitch, blend};

pub const LOW_MID_HZ: f64 = 250.0;
pub const MID_HIGH_HZ: f64 = 2_500.0;
/// Gain of a band with its knob fully clockwise.
pub const MAX_BOOST_DB: f32 = 12.0;
/// How long a change of model fades, in milliseconds.
pub const MODEL_FADE_MS: f32 = 10.0;
/// Quality factors of the two sections of a fourth-order Butterworth filter.
const BUTTERWORTH_4_Q: [f64; 2] = [0.541_196_100_146_197, 1.306_562_964_876_376_5];
/// The most sections a slope needs: those of the steep model.
const MAX_SECTIONS: usize = 4;

/// Index of each band in the gain array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Low = 0,
    Mid = 1,
    High = 2,
}

/// The model of the equaliser: how steep the edges of its bands are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EqModel {
    /// 6 dB per octave: wide, overlapping bands.
    Smooth,
    /// 12 dB per octave.
    Soft,
    /// 24 dB per octave.
    #[default]
    Isolator,
    /// 48 dB per octave: narrow transitions and the deepest cut outside a band.
    Steep,
}

impl EqModel {
    pub const ALL: [Self; 4] = [Self::Smooth, Self::Soft, Self::Isolator, Self::Steep];

    pub fn name(self) -> &'static str {
        match self {
            Self::Smooth => "smooth",
            Self::Soft => "soft",
            Self::Isolator => "isolator",
            Self::Steep => "steep",
        }
    }

    pub fn parse(text: &str) -> Result<Self, ValueError> {
        Self::ALL
            .into_iter()
            .find(|model| model.name() == text)
            .ok_or(ValueError::UnknownChoice {
                name: "equaliser model",
                expected: "smooth, soft, isolator, steep",
            })
    }

    fn index(self) -> usize {
        match self {
            Self::Smooth => 0,
            Self::Soft => 1,
            Self::Isolator => 2,
            Self::Steep => 3,
        }
    }
}

/// Maps a knob position to a band gain: `0.0` removes the band, `0.5` leaves it untouched
/// and `1.0` boosts it by [`MAX_BOOST_DB`]. Positions outside `0.0..=1.0` are clamped; a
/// position that is not a number leaves the band untouched.
pub fn band_gain(position: f32) -> f32 {
    if !position.is_finite() {
        return 1.0;
    }
    let position = position.clamp(0.0, 1.0);
    if position <= 0.5 {
        let fraction = position * 2.0;
        fraction * fraction
    } else {
        10f32.powf((position - 0.5) * 2.0 * MAX_BOOST_DB / 20.0)
    }
}

/// One edge of a band: up to [`MAX_SECTIONS`] sections in series.
#[derive(Clone, Copy)]
struct Slope {
    sections: [Biquad; MAX_SECTIONS],
    len: usize,
}

impl Slope {
    /// The low-pass or the high-pass edge of `model` at `hz`.
    fn new(model: EqModel, rate: SampleRate, hz: f64, low: bool) -> Self {
        let second = |q| {
            if low {
                Coefficients::low_pass(rate, hz, q)
            } else {
                Coefficients::high_pass(rate, hz, q)
            }
        };
        let first = if low {
            Coefficients::first_order_low_pass(rate, hz)
        } else {
            Coefficients::first_order_high_pass(rate, hz)
        };
        let (designs, len) = match model {
            EqModel::Smooth => ([first; MAX_SECTIONS], 1),
            EqModel::Soft => ([first; MAX_SECTIONS], 2),
            EqModel::Isolator => ([second(BUTTERWORTH_Q); MAX_SECTIONS], 2),
            EqModel::Steep => {
                let [q1, q2] = BUTTERWORTH_4_Q.map(second);
                ([q1, q2, q1, q2], 4)
            }
        };
        Self {
            sections: designs.map(Biquad::new),
            len,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        self.sections
            .iter_mut()
            .take(self.len)
            .fold(input, |signal, section| section.process(signal))
    }

    fn reset(&mut self) {
        self.sections.iter_mut().for_each(Biquad::reset);
    }
}

#[derive(Clone, Copy)]
struct ChannelSplit {
    low: Slope,
    above_low: Slope,
    mid: Slope,
    high: Slope,
    // The low band skips the second crossover, so it passes through the same pair of
    // slopes summed: an all-pass with the phase of that crossover. Without it the bands
    // would not add up flat.
    low_align_low: Slope,
    low_align_high: Slope,
    /// The sign of the high-pass edges: second-order Linkwitz-Riley edges add up to an
    /// all-pass only with one of them inverted.
    high_sign: f32,
}

impl ChannelSplit {
    fn new(model: EqModel, rate: SampleRate) -> Self {
        let slope = |hz, low| Slope::new(model, rate, hz, low);
        Self {
            low: slope(LOW_MID_HZ, true),
            above_low: slope(LOW_MID_HZ, false),
            mid: slope(MID_HIGH_HZ, true),
            high: slope(MID_HIGH_HZ, false),
            low_align_low: slope(MID_HIGH_HZ, true),
            low_align_high: slope(MID_HIGH_HZ, false),
            high_sign: if model == EqModel::Soft { -1.0 } else { 1.0 },
        }
    }

    fn process(&mut self, input: f32, gains: [f32; 3]) -> f32 {
        let sign = self.high_sign;
        let low = self.low.process(input);
        let low = self.low_align_low.process(low) + sign * self.low_align_high.process(low);
        let above_low = sign * self.above_low.process(input);
        let mid = self.mid.process(above_low);
        let high = sign * self.high.process(above_low);
        low * gains[0] + mid * gains[1] + high * gains[2]
    }

    fn reset(&mut self) {
        for slope in [
            &mut self.low,
            &mut self.above_low,
            &mut self.mid,
            &mut self.high,
            &mut self.low_align_low,
            &mut self.low_align_high,
        ] {
            slope.reset();
        }
    }
}

#[derive(Clone)]
pub struct ThreeBandEq {
    /// Left and right of each model, in the order of [`EqModel::ALL`]. On the heap: four
    /// channels of four models made the processor too large to build on the 1 MB stack of a
    /// main thread in a debug build.
    models: Box<[[ChannelSplit; 2]; EqModel::ALL.len()]>,
    switch: ModelSwitch<EqModel>,
}

impl ThreeBandEq {
    pub fn new(rate: SampleRate) -> Self {
        Self {
            models: Box::new(EqModel::ALL.map(|model| [ChannelSplit::new(model, rate); 2])),
            switch: ModelSwitch::new(EqModel::default(), rate, MODEL_FADE_MS),
        }
    }

    /// The model asked for last.
    pub fn model(&self) -> EqModel {
        self.switch.active()
    }

    /// Changes the model; the new one fades in over [`MODEL_FADE_MS`].
    pub fn set_model(&mut self, model: EqModel) {
        self.switch.request(model);
    }

    /// Processes one frame. `gains` are linear factors for low, mid and high (see [`Band`]);
    /// the caller smooths them.
    pub fn process(&mut self, frame: Frame, gains: [f32; 3]) -> Frame {
        let (active, weight, previous) = self.switch.advance();
        if let Some(started) = self.switch.take_started() {
            self.channels(started)
                .iter_mut()
                .for_each(ChannelSplit::reset);
        }
        let output = self.run(active, frame, gains);
        let Some(previous) = previous else {
            return output;
        };
        let fading = self.run(previous, frame, gains);
        [
            blend(output[0], fading[0], weight),
            blend(output[1], fading[1], weight),
        ]
    }

    fn run(&mut self, model: EqModel, frame: Frame, gains: [f32; 3]) -> Frame {
        let [left, right] = self.channels(model);
        [
            left.process(frame[0], gains),
            right.process(frame[1], gains),
        ]
    }

    fn channels(&mut self, model: EqModel) -> &mut [ChannelSplit; 2] {
        &mut self.models[model.index()]
    }

    pub fn reset(&mut self) {
        for channels in self.models.iter_mut() {
            channels.iter_mut().for_each(ChannelSplit::reset);
        }
    }
}

#[cfg(test)]
mod tests {
    use platter_core::SampleRate;
    use platter_testkit::measure::{db, max_step, rms};
    use platter_testkit::signal::sine;

    use super::{Band, EqModel, ThreeBandEq, band_gain};

    const RATE: u32 = 48_000;

    /// Steady-state gain in decibels of a sine through `model` at `rate`.
    fn model_response_db(model: EqModel, rate: u32, hz: f64, gains: [f32; 3]) -> f64 {
        let mut eq = ThreeBandEq::new(SampleRate::new(rate).unwrap());
        eq.set_model(model);
        let samples = 2 * rate as usize;
        let input = sine(hz, rate, samples, 0.5);
        let output: Vec<f32> = input
            .iter()
            .map(|x| eq.process([*x, *x], gains)[0])
            .collect();
        let settled = samples / 2;
        db(rms(&output[settled..]).max(1e-12) / rms(&input[settled..]))
    }

    fn response_db(hz: f64, gains: [f32; 3]) -> f64 {
        model_response_db(EqModel::Isolator, RATE, hz, gains)
    }

    fn without(band: Band) -> [f32; 3] {
        let mut gains = [1.0; 3];
        gains[band as usize] = 0.0;
        gains
    }

    #[test]
    fn unity_gains_give_a_flat_response() {
        for hz in [30.0, 100.0, 250.0, 700.0, 2_500.0, 6_000.0, 15_000.0] {
            let gain = response_db(hz, [1.0; 3]);
            assert!(gain.abs() < 0.05, "{hz} Hz: {gain:.3} dB");
        }
    }

    #[test]
    fn every_model_is_flat_at_unity_at_each_common_rate() {
        for model in EqModel::ALL {
            for rate in [44_100, 48_000, 96_000] {
                for hz in [30.0, 250.0, 790.0, 2_500.0, 15_000.0] {
                    let gain = model_response_db(model, rate, hz, [1.0; 3]);
                    assert!(
                        gain.abs() < 0.05,
                        "{model:?} at {rate} Hz, {hz} Hz: {gain:.3} dB"
                    );
                }
            }
        }
    }

    #[test]
    fn a_killed_band_is_removed() {
        assert!(response_db(40.0, without(Band::Low)) < -60.0);
        assert!(response_db(8_000.0, without(Band::Low)).abs() < 0.05);

        assert!(response_db(16_000.0, without(Band::High)) < -60.0);
        assert!(response_db(60.0, without(Band::High)).abs() < 0.05);

        // Halfway between the crossovers each neighbour is about 40 dB down, and the two
        // add up.
        assert!(response_db(790.0, without(Band::Mid)) < -30.0);
    }

    /// How far each model takes the low band down two octaves below its edge, the steeper
    /// the slope the deeper, and the mid band halfway between the edges.
    #[test]
    fn steeper_models_cut_deeper_outside_the_band_they_keep() {
        let depths = EqModel::ALL.map(|model| {
            (
                model_response_db(model, RATE, 62.5, without(Band::Low)),
                model_response_db(model, RATE, 790.0, without(Band::Mid)),
            )
        });
        // Two octaves at 6, 12, 24 and 48 dB per octave; the depth of the mid band was
        // measured.
        let expected = [
            (-11.5, -19.0),
            (-23.5, -14.0),
            (-47.0, -32.0),
            (-94.0, -70.0),
        ];
        for ((model, (low, mid)), (low_at_most, mid_at_most)) in
            EqModel::ALL.into_iter().zip(depths).zip(expected)
        {
            assert!(
                low < low_at_most && mid < mid_at_most,
                "{model:?}: low {low:.1} dB, mid {mid:.1} dB"
            );
        }
        for pair in depths.windows(2) {
            assert!(pair[1].0 < pair[0].0, "{depths:?}");
        }
    }

    #[test]
    fn all_bands_killed_is_exact_silence_in_every_model() {
        for model in EqModel::ALL {
            let mut eq = ThreeBandEq::new(SampleRate::new(RATE).unwrap());
            eq.set_model(model);
            for x in sine(1_000.0, RATE, 4_800, 1.0) {
                assert_eq!(eq.process([x, -x], [0.0; 3]), [0.0, 0.0], "{model:?}");
            }
        }
    }

    #[test]
    fn a_boosted_band_rises_by_its_gain() {
        let boost = band_gain(1.0);
        assert!((f64::from(boost) - 3.981).abs() < 0.01);
        let mut gains = [1.0; 3];
        gains[Band::Low as usize] = boost;
        assert!((response_db(40.0, gains) - 12.0).abs() < 0.2);
    }

    #[test]
    fn a_change_of_model_fades_without_a_click() {
        let mut eq = ThreeBandEq::new(SampleRate::new(RATE).unwrap());
        assert_eq!(eq.model(), EqModel::Isolator);
        let input = sine(180.0, RATE, 48_000, 0.5);
        let gains = [1.0, 0.0, 0.0];
        let mut output = Vec::new();
        for (n, x) in input.iter().enumerate() {
            if n % 4_800 == 2_400 {
                eq.set_model(EqModel::ALL[(n / 4_800) % EqModel::ALL.len()]);
            }
            output.push(eq.process([*x, *x], gains)[0]);
        }
        assert!(output.iter().all(|y| y.is_finite()));
        assert!(
            max_step(&output) < 1.5 * max_step(&input),
            "{} against {}",
            max_step(&output),
            max_step(&input)
        );
    }

    #[test]
    fn knob_positions_map_to_gains() {
        assert_eq!(band_gain(0.0), 0.0);
        assert_eq!(band_gain(0.5), 1.0);
        assert_eq!(band_gain(-3.0), 0.0);
        assert_eq!(band_gain(7.0), band_gain(1.0));
        assert_eq!(band_gain(f32::NAN), 1.0);
        assert!(band_gain(0.25) < band_gain(0.3));
    }

    #[test]
    fn model_names_round_trip() {
        for model in EqModel::ALL {
            assert_eq!(EqModel::parse(model.name()), Ok(model));
        }
        assert!(EqModel::parse("loud").is_err());
    }

    #[test]
    fn output_stays_finite() {
        for model in EqModel::ALL {
            let mut eq = ThreeBandEq::new(SampleRate::new(RATE).unwrap());
            eq.set_model(model);
            let gains = [band_gain(1.0); 3];
            for n in 0..48_000 {
                let x = if n % 2 == 0 { 1.0 } else { -1.0 };
                let [left, right] = eq.process([x, -x], gains);
                assert!(left.is_finite() && right.is_finite());
            }
        }
    }
}
