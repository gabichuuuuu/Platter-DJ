//! The maximiser of the master output: a look-ahead peak limiter with an input drive, a
//! ceiling and a release. The Limiter key of the master switches it in and out; the safety
//! limiter (`crate::limiter`) stays after it either way. Behaviour:
//! `docs/behavior/master.md`; choices and measurements: `docs/decisions/0034-maximiser.md`.
//!
//! The gain follows the method G. Luff describes in "Designing a straightforward limiter"
//! (2022), <https://signalsmith-audio.co.uk/writing/2022/limiter/>:
//!
//! 1. for each frame, the largest gain that keeps its louder side under the ceiling;
//! 2. the minimum of that gain over a window of [`LOOKAHEAD_MS`], kept with a streaming
//!    minimum (D. Lemire, "Streaming maximum-minimum filter using no more than three
//!    comparisons per element", 2006, <https://arxiv.org/abs/cs/0610046>);
//! 3. an exponential release towards unity that never rises above that minimum;
//! 4. a moving average of the released gain, as long as the window.
//!
//! The audio waits one window less one frame, so every gain in the average comes from a
//! window that holds the frame it scales: the average is never above the gain that frame
//! needs, and the output stays under the ceiling without clipping. The moving average is
//! summed in integers, so it does not drift and returns to exactly one. The ceiling applies
//! to sample peaks; the peaks between samples of the reconstructed signal can pass it.

use platter_core::SampleRate;

use crate::Frame;
use crate::smooth::Smoothed;

/// Length of the look-ahead window, which is also the delay the maximiser adds and the time
/// the gain takes to fall to a peak.
pub const LOOKAHEAD_MS: f32 = 1.5;
/// The window at the highest sample rate Platter runs at: 1.5 ms of 384 kHz.
const MAX_WINDOW: usize = 576;
/// Units of a gain in the moving average: one is `ONE`.
const ONE: u32 = 1 << 24;
/// A released gain this close to one is one, so the release ends.
const UNITY_SNAP: f32 = 1.0e-6;

/// Frames of the look-ahead window at `rate`.
fn window(rate: SampleRate) -> usize {
    let frames = (rate.as_f32() * LOOKAHEAD_MS / 1_000.0).round();
    if frames.is_finite() {
        (frames as usize).clamp(1, MAX_WINDOW)
    } else {
        1
    }
}

/// The parts that are too large for the stack of the processor.
struct Lines {
    /// Driven frames waiting for their gain, each with the ceiling it was measured against.
    delay: [[f32; 3]; MAX_WINDOW],
    /// The streaming minimum: frame numbers and needed gains, the gains rising from front
    /// to back.
    minima: [(u64, f32); MAX_WINDOW],
    /// Released gains in the moving average, in units of [`ONE`].
    released: [u32; MAX_WINDOW],
}

pub struct Maximiser {
    lines: Box<Lines>,
    window: usize,
    /// Where the next frame goes in `delay` and `released`.
    position: usize,
    /// Frames processed, which number the entries of the streaming minimum.
    frame: u64,
    /// The streaming minimum occupies `len` entries of `minima` from `head`, wrapping.
    head: usize,
    len: usize,
    /// Sum of `released`.
    sum: u64,
    /// How far below one the released gain of the last frame is. The release works on
    /// this distance, which keeps its precision as it shrinks; the gain itself would stall
    /// a few ten-thousandths below one, where its steps are smaller than half its last digit.
    fall: f32,
    /// Per-frame factor of the distance during the release.
    keep: f32,
    rate: f32,
    drive: Smoothed,
    ceiling: Smoothed,
    /// Weight of the limited signal against the input: one while on, zero while off.
    on: Smoothed,
    /// The value of `on` for the last frame.
    weight: f32,
    /// The gain applied to the last frame.
    gain: f32,
}

impl std::fmt::Debug for Maximiser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Maximiser")
            .field("window", &self.window)
            .field("gain", &self.gain)
            .finish_non_exhaustive()
    }
}

impl Maximiser {
    /// Off, with a drive of one, a ceiling of `ceiling` and a release of `release_seconds`;
    /// parameter changes and switching ramp over `ramp_ms`.
    pub fn new(rate: SampleRate, ramp_ms: f32, ceiling: f32, release_seconds: f32) -> Self {
        let window = window(rate);
        let mut maximiser = Self {
            lines: Box::new(Lines {
                delay: [[0.0, 0.0, 1.0]; MAX_WINDOW],
                minima: [(0, 1.0); MAX_WINDOW],
                released: [ONE; MAX_WINDOW],
            }),
            window,
            position: 0,
            frame: 0,
            head: 0,
            len: 0,
            sum: window as u64 * u64::from(ONE),
            fall: 0.0,
            keep: 0.0,
            rate: rate.as_f32(),
            drive: Smoothed::new(1.0, rate, ramp_ms),
            ceiling: Smoothed::new(valid_ceiling(ceiling).unwrap_or(1.0), rate, ramp_ms),
            on: Smoothed::new(0.0, rate, ramp_ms),
            weight: 0.0,
            gain: 1.0,
        };
        maximiser.set_release(release_seconds);
        maximiser
    }

    /// Frames between a frame going in and coming out of the limited path.
    pub fn delay_frames(&self) -> usize {
        self.window - 1
    }

    /// Switches the limited path in or out, with a fade.
    pub fn set_on(&mut self, on: bool) {
        self.on.set(if on { 1.0 } else { 0.0 });
    }

    /// Gain before limiting, linear. A value that is not a positive finite number is ignored.
    pub fn set_drive(&mut self, drive: f32) {
        if drive.is_finite() && drive > 0.0 {
            self.drive.set(drive);
        }
    }

    /// Highest sample peak of the output, linear, up to full scale. A value that is not a
    /// positive finite number is ignored.
    pub fn set_ceiling(&mut self, ceiling: f32) {
        if let Some(ceiling) = valid_ceiling(ceiling) {
            self.ceiling.set(ceiling);
        }
    }

    /// Time the gain takes to close 90 % of its distance to unity once nothing needs it
    /// lower. A value that is not a finite number is ignored; others are kept between a
    /// millisecond and ten seconds.
    pub fn set_release(&mut self, seconds: f32) {
        if !seconds.is_finite() {
            return;
        }
        let frames = seconds.clamp(0.001, 10.0) * self.rate;
        self.keep = 0.1f32.powf(1.0 / frames);
    }

    /// The weight of the limited path in the frame just processed: zero while off, one
    /// while on, between them during the fade. Whatever must stay in time with the master
    /// follows it.
    pub fn weight(&self) -> f32 {
        self.weight
    }

    /// Gain the maximiser applies while on; one while off or idle.
    pub fn gain(&self) -> f32 {
        if self.on.target() > 0.0 {
            self.gain
        } else {
            1.0
        }
    }

    /// Processes one frame. While off and settled the frame passes as it is, without
    /// delay; while on, it comes out [`Self::delay_frames`] later, limited. The limited
    /// path runs either way, so it is ready the moment it is switched in.
    pub fn process(&mut self, frame: Frame) -> Frame {
        let (limited, ceiling) = self.limit(frame);
        let wet = limited.map(|sample| sample.clamp(-ceiling, ceiling));
        let weight = self.on.advance();
        self.weight = weight;
        if weight <= 0.0 {
            frame
        } else if weight >= 1.0 {
            wet
        } else {
            let dry = frame.map(|sample| if sample.is_finite() { sample } else { 0.0 });
            [
                dry[0] * (1.0 - weight) + wet[0] * weight,
                dry[1] * (1.0 - weight) + wet[1] * weight,
            ]
        }
    }

    /// The limited path for one frame, before the clamp that covers rounding, and the
    /// ceiling the frame that comes out was measured against.
    fn limit(&mut self, frame: Frame) -> (Frame, f32) {
        let drive = self.drive.advance();
        let ceiling = self.ceiling.advance();
        // A sample the drive takes past the largest number is as unusable as one that was
        // never a number: both become silence.
        let driven = frame.map(|sample| {
            let driven = sample * drive;
            if driven.is_finite() { driven } else { 0.0 }
        });
        let peak = driven[0].abs().max(driven[1].abs());
        let needed = if peak > ceiling { ceiling / peak } else { 1.0 };
        let held = self.hold(needed);

        let recovered = self.fall * self.keep;
        let recovered = if recovered < UNITY_SNAP {
            0.0
        } else {
            recovered
        };
        let released = if 1.0 - recovered > held {
            self.fall = 1.0 - held;
            held
        } else {
            self.fall = recovered;
            1.0 - recovered
        };
        // Truncation rounds down, so the average never rises above the released gains.
        let units = (released * ONE as f32) as u32;
        let lines = &mut *self.lines;
        let position = self.position;
        self.sum = self.sum - u64::from(lines.released[position]) + u64::from(units);
        lines.released[position] = units;
        let gain = (self.sum as f64 / (self.window as f64 * f64::from(ONE))) as f32;
        self.gain = gain;

        lines.delay[position] = [driven[0], driven[1], ceiling];
        let out = lines.delay[(position + 1) % self.window];
        self.position = (position + 1) % self.window;
        self.frame = self.frame.wrapping_add(1);
        ([out[0] * gain, out[1] * gain], out[2])
    }

    /// Adds the gain the current frame needs to the streaming minimum and returns the
    /// minimum over the window.
    fn hold(&mut self, needed: f32) -> f32 {
        let minima = &mut self.lines.minima;
        let window = self.window as u64;
        // Entries that left the window go first, so the new one always has room. Both
        // loops are bounded by the window: the minimum never holds more entries.
        while self.len > 0 && minima[self.head].0.saturating_add(window) <= self.frame {
            self.head = (self.head + 1) % MAX_WINDOW;
            self.len -= 1;
        }
        while self.len > 0 {
            let back = (self.head + self.len - 1) % MAX_WINDOW;
            if minima[back].1 < needed {
                break;
            }
            self.len -= 1;
        }
        if self.len < MAX_WINDOW {
            minima[(self.head + self.len) % MAX_WINDOW] = (self.frame, needed);
            self.len += 1;
        }
        minima[self.head].1
    }
}

fn valid_ceiling(ceiling: f32) -> Option<f32> {
    (ceiling.is_finite() && ceiling > 0.0).then_some(ceiling.min(1.0))
}

/// Delays another signal as the maximiser delays the master, and blends it the same way,
/// so the two stay in time: the decks sent to the headphones beside the master.
pub struct Aligner {
    delay: Box<[[f32; 3]; MAX_WINDOW]>,
    window: usize,
    position: usize,
}

impl std::fmt::Debug for Aligner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Aligner")
            .field("window", &self.window)
            .finish_non_exhaustive()
    }
}

impl Aligner {
    pub fn new(rate: SampleRate) -> Self {
        Self {
            delay: Box::new([[0.0; 3]; MAX_WINDOW]),
            window: window(rate),
            position: 0,
        }
    }

    /// The input blended with itself as the maximiser delays it, by the weight
    /// [`Maximiser::weight`] gave for the same frame.
    pub fn process(&mut self, input: [f32; 3], weight: f32) -> [f32; 3] {
        self.delay[self.position] = input;
        let delayed = self.delay[(self.position + 1) % self.window];
        self.position = (self.position + 1) % self.window;
        if weight <= 0.0 {
            input
        } else if weight >= 1.0 {
            delayed
        } else {
            [0, 1, 2].map(|index| input[index] * (1.0 - weight) + delayed[index] * weight)
        }
    }
}

#[cfg(test)]
mod tests {
    use platter_core::SampleRate;
    use platter_testkit::measure::{max_step, peak};
    use platter_testkit::signal::sine;

    use super::{Aligner, MAX_WINDOW, Maximiser, window};

    const RATE: u32 = 48_000;
    const RAMP_MS: f32 = 10.0;
    /// -1 dB, the ceiling Platter starts with.
    const CEILING: f32 = 0.891_250_9;
    /// What rounding may add to the ceiling before the final clamp.
    const ROUNDING: f32 = 1.0e-6;

    fn rate() -> SampleRate {
        SampleRate::new(RATE).unwrap()
    }

    /// A maximiser switched on and settled, with a drive and a release.
    fn on(drive: f32, release_seconds: f32) -> Maximiser {
        let mut maximiser = Maximiser::new(rate(), RAMP_MS, CEILING, release_seconds);
        maximiser.set_drive(drive);
        maximiser.set_on(true);
        for _ in 0..RATE / 10 {
            maximiser.process([0.0, 0.0]);
        }
        maximiser
    }

    /// Runs the limited path on `signal`, the same on both sides, and checks that no frame
    /// passes the ceiling before the clamp; returns the left side.
    fn limited(maximiser: &mut Maximiser, signal: &[f32]) -> Vec<f32> {
        signal
            .iter()
            .map(|x| {
                let ([left, right], ceiling) = maximiser.limit([*x, -*x]);
                assert!(
                    left.abs() <= ceiling * (1.0 + ROUNDING)
                        && right.abs() <= ceiling * (1.0 + ROUNDING),
                    "{left} or {right} passes {ceiling}"
                );
                left
            })
            .collect()
    }

    fn db(ratio: f32) -> f32 {
        20.0 * ratio.log10()
    }

    #[test]
    fn the_window_is_a_millisecond_and_a_half_and_fits_every_rate() {
        assert_eq!(window(rate()), 72);
        assert_eq!(
            window(SampleRate::new(SampleRate::MAX_HZ).unwrap()),
            MAX_WINDOW
        );
        assert_eq!(window(SampleRate::new(SampleRate::MIN_HZ).unwrap()), 12);
    }

    /// `docs/behavior/master.md`, edge cases: off, the master passes untouched.
    #[test]
    fn off_it_passes_every_frame_untouched_and_without_delay() {
        let mut maximiser = Maximiser::new(rate(), RAMP_MS, CEILING, 0.1);
        maximiser.set_drive(4.0);
        for x in sine(1_000.0, RATE, 4_800, 3.0) {
            assert_eq!(maximiser.process([x, -x]), [x, -x]);
        }
        assert_eq!(maximiser.gain(), 1.0);
    }

    /// Under the ceiling with no drive the limited path is a delay, bit for bit.
    #[test]
    fn under_the_ceiling_it_delays_the_signal_bit_for_bit() {
        let mut maximiser = on(1.0, 0.1);
        let delay = maximiser.delay_frames();
        assert_eq!(delay, 71);
        let input = sine(1_000.0, RATE, 4_800, 0.85);
        let output: Vec<f32> = input
            .iter()
            .map(|x| maximiser.process([*x, *x])[0])
            .collect();
        assert_eq!(&output[delay..], &input[..input.len() - delay]);
        assert_eq!(maximiser.gain(), 1.0);
    }

    #[test]
    fn a_full_scale_square_with_twelve_decibels_of_drive_stays_under_the_ceiling() {
        let mut maximiser = on(3.981, 0.1);
        let square: Vec<f32> = (0..RATE)
            .map(|i| if (i / 24) % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let output = limited(&mut maximiser, &square);
        // A square has no peaks between its levels: the settled output sits at the ceiling.
        let settled = peak(&output[RATE as usize / 2..]);
        assert!(settled > CEILING * 0.999, "{settled}");
    }

    #[test]
    fn a_single_sample_impulse_and_a_burst_shorter_than_the_window_stay_under_it() {
        let mut maximiser = on(1.0, 0.1);
        let mut signal = vec![0.0f32; 4_800];
        signal[1_000] = 8.0;
        for sample in &mut signal[2_000..2_030] {
            *sample = -5.0;
        }
        // A step out of silence, held.
        for sample in &mut signal[3_000..] {
            *sample = 2.0;
        }
        let output = limited(&mut maximiser, &signal);
        assert!(peak(&output) <= CEILING * (1.0 + super::UNITY_SNAP));
        // The gain is already down when the impulse comes out: it is scaled, not clipped.
        let delay = maximiser.delay_frames();
        assert!((output[1_000 + delay] - CEILING).abs() < 1.0e-5);
    }

    /// `docs/behavior/master.md`: the gain reduction of a steady sine.
    #[test]
    fn a_loud_sine_comes_out_with_its_peaks_just_under_the_ceiling() {
        for hz in [50.0, 1_000.0, 5_000.0] {
            let mut maximiser = on(1.0, 0.1);
            let output = limited(&mut maximiser, &sine(hz, RATE, RATE as usize, 2.0));
            let settled = peak(&output[RATE as usize / 2..]);
            let under = db(CEILING) - db(settled);
            assert!((0.0..0.5).contains(&under), "{hz} Hz: {under:.3} dB under");
            let reduction = db(maximiser.gain());
            assert!(reduction < -6.0, "{hz} Hz: {reduction:.2} dB");
        }
    }

    /// `docs/behavior/master.md`: the release closes 90 % of the reduction in the release
    /// time, after the window holds the gain down and its average follows: a window and a
    /// half, 2.25 ms, from the last loud frame.
    #[test]
    fn the_gain_recovers_in_the_release_time() {
        for release in [0.05, 0.1, 0.5] {
            let mut maximiser = on(1.0, release);
            let burst = vec![4.0f32; RATE as usize / 10];
            limited(&mut maximiser, &burst);
            let reduced = maximiser.gain();
            assert!((reduced - CEILING / 4.0).abs() < 1.0e-4, "{reduced}");
            let target = 1.0 - 0.1 * (1.0 - reduced);
            let mut frames = 0usize;
            while maximiser.gain() < target {
                maximiser.limit([0.0, 0.0]);
                frames += 1;
                assert!(frames < RATE as usize * 2);
            }
            let seconds = frames as f32 / RATE as f32;
            assert!(
                (seconds - release - 0.002_25).abs() < 0.000_5,
                "release {release}: {seconds:.4} s"
            );
        }
    }

    #[test]
    fn after_a_long_loud_passage_the_gain_returns_to_exactly_one() {
        let mut maximiser = on(2.0, 0.05);
        limited(&mut maximiser, &sine(80.0, RATE, RATE as usize * 20, 1.0));
        assert!(maximiser.gain() < 0.6);
        let quiet = sine(1_000.0, RATE, RATE as usize, 0.1);
        let output: Vec<f32> = quiet
            .iter()
            .map(|x| maximiser.process([*x, *x])[0])
            .collect();
        assert_eq!(maximiser.gain(), 1.0);
        let delay = maximiser.delay_frames();
        let tail = quiet.len() - RATE as usize / 2;
        for (out, x) in output[tail + delay..].iter().zip(&quiet[tail..]) {
            assert_eq!(*out, *x * 2.0);
        }
    }

    #[test]
    fn a_lower_ceiling_holds_for_the_frames_measured_against_it() {
        let mut maximiser = on(1.0, 0.1);
        let mut signal = sine(440.0, RATE, RATE as usize, 1.5);
        let mut frames = 0;
        for x in &mut signal {
            let ([left, _], ceiling) = maximiser.limit([*x, *x]);
            assert!(left.abs() <= ceiling * (1.0 + ROUNDING));
            *x = left;
            frames += 1;
            if frames == RATE as usize / 4 {
                maximiser.set_ceiling(0.25);
            }
        }
        assert!(peak(&signal[RATE as usize / 2..]) <= 0.25 * (1.0 + ROUNDING));
    }

    /// `docs/behavior/master.md`: switching fades over the ramp, so it does not click.
    #[test]
    fn switching_on_and_off_fades_without_a_step() {
        let input = sine(1_000.0, RATE, RATE as usize, 0.5);
        let own_step = max_step(&input);
        let mut maximiser = Maximiser::new(rate(), RAMP_MS, CEILING, 0.1);
        let output: Vec<f32> = input
            .iter()
            .enumerate()
            .map(|(i, x)| {
                if i == RATE as usize / 4 {
                    maximiser.set_on(true);
                }
                if i == RATE as usize * 3 / 4 {
                    maximiser.set_on(false);
                }
                maximiser.process([*x, *x])[0]
            })
            .collect();
        let step = max_step(&output);
        assert!(step < own_step * 1.1, "{step} against {own_step}");
    }

    #[test]
    fn whatever_comes_in_the_output_is_finite_and_inside_the_ceiling() {
        let mut maximiser = on(3.981, 0.01);
        for x in [
            f32::NAN,
            f32::INFINITY,
            -f32::INFINITY,
            // Finite, but past the largest number once driven.
            f32::MAX,
            1e30,
            -1e30,
            0.0,
            1.0,
        ] {
            for _ in 0..100 {
                let [left, right] = maximiser.process([x, -x]);
                assert!(left.is_finite() && left.abs() <= CEILING);
                assert!(right.is_finite() && right.abs() <= CEILING);
            }
        }
    }

    #[test]
    fn parameters_that_are_not_numbers_are_ignored() {
        let mut maximiser = on(1.0, 0.1);
        maximiser.set_drive(f32::NAN);
        maximiser.set_drive(-1.0);
        maximiser.set_ceiling(f32::INFINITY);
        maximiser.set_ceiling(0.0);
        maximiser.set_release(f32::NAN);
        let output = limited(&mut maximiser, &sine(1_000.0, RATE, 4_800, 0.5));
        assert!(peak(&output) <= 0.5);
    }

    #[test]
    fn the_aligner_keeps_the_delay_of_the_maximiser() {
        let mut maximiser = on(1.0, 0.1);
        let mut aligner = Aligner::new(rate());
        let input = sine(500.0, RATE, 2_000, 0.5);
        for x in input {
            let [master, _] = maximiser.process([x, x]);
            let [aligned, _, _] = aligner.process([x, 0.0, 0.0], maximiser.weight());
            assert_eq!(master, aligned);
        }
    }
}
