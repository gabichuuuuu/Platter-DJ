//! The effects of the effect units: `docs/behavior/effects.md`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use platter_core::SampleRate;
use platter_dsp::Frame;
use platter_dsp::effects::unit::{Chain, MixerEffect, Place, Slot, UnitMode, UnitValues};
use platter_dsp::effects::{Controls, Effect, EffectKind, Tempo};
use platter_testkit::measure::rms;

const RATE: u32 = 48_000;

fn rate() -> SampleRate {
    SampleRate::new(RATE).unwrap()
}

/// The master clock at 120 beats per minute, `frame` frames after its first beat.
fn tempo_at(frame: usize) -> Tempo {
    let samples_per_beat = RATE as f32 / 2.0;
    Tempo {
        samples_per_beat,
        beat: frame as f64 / f64::from(samples_per_beat),
    }
}

/// A test signal: two sines and a little noise-like wobble, 0.5 at most.
fn signal(frame: usize) -> Frame {
    let t = frame as f32 / RATE as f32;
    let left = 0.3 * (2.0 * std::f32::consts::PI * 220.0 * t).sin()
        + 0.2 * (2.0 * std::f32::consts::PI * 3_100.0 * t).sin();
    let right = 0.3 * (2.0 * std::f32::consts::PI * 330.0 * t).sin()
        + 0.2 * (2.0 * std::f32::consts::PI * 1_700.0 * t).sin();
    [left, right]
}

fn boxed(kind: EffectKind) -> Option<Box<Effect>> {
    Some(Box::new(Effect::new(kind, rate())))
}

#[test]
fn every_effect_stays_finite_and_bounded_while_its_controls_sweep() {
    for kind in EffectKind::ALL {
        for keys in [[false, false], [true, false], [false, true], [true, true]] {
            let mut effect = Effect::new(kind, rate());
            let mut peak = 0.0f32;
            for frame in 0..RATE as usize {
                let sweep = (frame as f32 / RATE as f32 * 3.0).fract();
                let controls = Controls {
                    knobs: [sweep, 1.0 - sweep, (sweep * 2.0).fract()],
                    keys,
                };
                let output = effect.process(signal(frame), &controls, tempo_at(frame));
                assert!(
                    output.iter().all(|sample| sample.is_finite()),
                    "{} with keys {keys:?} gave a value that is not a number",
                    kind.name()
                );
                peak = peak.max(output[0].abs()).max(output[1].abs());
            }
            assert!(
                peak < 8.0,
                "{} with keys {keys:?} reached {peak}",
                kind.name()
            );
        }
    }
}

#[test]
fn a_chain_at_dry_wet_zero_passes_its_input_bit_for_bit() {
    let mut chain = Chain::new(rate());
    for (index, kind) in [EffectKind::Delay, EffectKind::Gater, EffectKind::Bitcrusher]
        .into_iter()
        .enumerate()
    {
        let _ = chain
            .slot_mut(Place::Group(index))
            .unwrap()
            .replace(boxed(kind));
    }
    let mut values = UnitValues {
        dry_wet: 0.0,
        ..UnitValues::default()
    };
    for slot in &mut values.slots {
        slot.on = true;
    }
    // Let the dry/wet ramp reach zero first.
    for frame in 0..2_000 {
        chain.process(signal(frame), &values, tempo_at(frame));
    }
    for frame in 2_000..20_000 {
        let input = signal(frame);
        assert_eq!(chain.process(input, &values, tempo_at(frame)), input);
    }
}

#[test]
fn the_delay_echoes_half_a_beat_later_at_120_beats_per_minute() {
    let mut effect = Effect::new(EffectKind::Delay, rate());
    // Tone open, no feedback, time on the step of half a beat: 12 000 frames at 48 kHz.
    let controls = Controls {
        knobs: [0.0, 0.0, 0.3125],
        keys: [false; 2],
    };
    let mut echo = Vec::new();
    for frame in 0..20_000 {
        let input = if frame == 0 { [1.0, 1.0] } else { [0.0; 2] };
        let output = effect.process(input, &controls, tempo_at(frame));
        echo.push(output[0] - input[0]);
    }
    let loudest = echo
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .map(|(frame, _)| frame)
        .unwrap();
    assert!(
        (11_998..=12_002).contains(&loudest),
        "the echo came at frame {loudest}"
    );
}

#[test]
fn a_delay_rings_on_after_its_slot_switches_off_and_a_flanger_does_not() {
    for (kind, rings) in [(EffectKind::Delay, true), (EffectKind::Flanger, false)] {
        let mut slot = Slot::new(rate());
        let _ = slot.replace(boxed(kind));
        let controls = kind.defaults();
        for frame in 0..24_000 {
            slot.process(signal(frame), &controls, true, tempo_at(frame));
        }
        // Switched off, with silence coming in.
        let mut after = Vec::new();
        for frame in 24_000..48_000 {
            let output = slot.process([0.0; 2], &controls, false, tempo_at(frame));
            after.push(output[0]);
        }
        let tail = rms(&after[2_000..]);
        if rings {
            assert!(tail > 1e-3, "{} left no tail: {tail}", kind.name());
        } else {
            assert_eq!(tail, 0.0, "{} kept sounding", kind.name());
        }
    }
}

#[test]
fn an_effect_switched_off_passes_its_input_exactly_unless_it_rings() {
    for kind in EffectKind::ALL.into_iter().filter(|kind| !kind.has_tail()) {
        let mut slot = Slot::new(rate());
        let _ = slot.replace(boxed(kind));
        let controls = kind.defaults();
        for frame in 0..4_000 {
            slot.process(signal(frame), &controls, true, tempo_at(frame));
        }
        for frame in 4_000..5_000 {
            slot.process(signal(frame), &controls, false, tempo_at(frame));
        }
        for frame in 5_000..10_000 {
            let input = signal(frame);
            assert_eq!(
                slot.process(input, &controls, false, tempo_at(frame)),
                input,
                "{} changed the signal while off",
                kind.name()
            );
        }
    }
}

#[test]
fn a_value_that_is_not_a_number_clears_the_effect_and_gives_silence() {
    for kind in EffectKind::ALL {
        let mut effect = Effect::new(kind, rate());
        let controls = kind.defaults();
        for frame in 0..1_000 {
            effect.process(signal(frame), &controls, tempo_at(frame));
        }
        assert_eq!(
            effect.process([f32::NAN, 0.0], &controls, tempo_at(1_000)),
            [0.0; 2]
        );
        let after = effect.process([0.0; 2], &controls, tempo_at(1_001));
        assert!(after.iter().all(|sample| sample.is_finite()));
    }
}

#[test]
fn the_gater_opens_on_every_step_and_closes_before_the_next() {
    let mut effect = Effect::new(EffectKind::Gater, rate());
    // No noise, the shortest shape, a gate every quarter of a beat: 6 000 frames.
    let controls = Controls {
        knobs: [0.0, 0.0, 0.625],
        keys: [false; 2],
    };
    let mut levels = Vec::new();
    for frame in 0..24_000 {
        levels.push(effect.process([1.0, 1.0], &controls, tempo_at(frame))[0]);
    }
    // The shortest shape is a click: open within a millisecond, half closed soon after.
    let opened = |start: usize| {
        levels[start..start + 200]
            .iter()
            .copied()
            .fold(0.0, f32::max)
    };
    for start in (0..24_000).step_by(6_000) {
        assert!(opened(start) > 0.6, "closed after the step at {start}");
        assert!(
            levels[start + 5_900] < 0.01,
            "open before the step after {start}"
        );
    }
}

#[test]
fn the_middle_of_the_one_knob_filter_leaves_the_signal_alone() {
    let mut effect = Effect::new(EffectKind::Filter, rate());
    let controls = Controls {
        knobs: [0.7, 0.9, 0.5],
        keys: [false, true],
    };
    for frame in 0..4_000 {
        let input = signal(frame);
        assert_eq!(effect.process(input, &controls, tempo_at(frame)), input);
    }
}

#[test]
fn the_bitcrusher_at_one_bit_leaves_three_levels() {
    let mut effect = Effect::new(EffectKind::Bitcrusher, rate());
    let controls = Controls {
        knobs: [1.0, 0.0, 0.0],
        keys: [false; 2],
    };
    for frame in 0..4_000 {
        let output = effect.process(signal(frame), &controls, tempo_at(frame));
        for sample in output {
            assert!(
                [-1.0, 0.0, 1.0].contains(&sample),
                "{sample} is not one of the levels of one bit"
            );
        }
    }
}

#[test]
fn the_slicer_repeats_the_slice_that_was_playing_when_it_was_switched_on() {
    let mut slot = Slot::new(rate());
    let _ = slot.replace(boxed(EffectKind::Slicer));
    // Full mix, no rotation, slices of one beat: 24 000 frames.
    let controls = Controls {
        knobs: [1.0, 0.0, 0.5],
        keys: [false; 2],
    };
    // A rising ramp marks every frame of the bar.
    let ramp = |frame: usize| {
        let value = frame as f32 / 100_000.0;
        [value, value]
    };
    for frame in 0..96_000 {
        slot.process(ramp(frame), &controls, false, tempo_at(frame));
    }
    let mut output = Vec::new();
    for frame in 96_000..168_000 {
        output.push(slot.process(ramp(frame), &controls, true, tempo_at(frame))[0]);
    }
    // After the fade in, each beat repeats the one before it, away from the slice edges.
    for frame in [30_000, 40_000] {
        let a = output[frame];
        let b = output[frame + 24_000];
        assert!((a - b).abs() < 1e-6, "frame {frame}: {a} then {b}");
    }
    assert!(output[30_000] < ramp(96_000 + 30_000)[0]);
}

#[test]
fn a_swapped_effect_fades_out_and_comes_back_to_be_freed() {
    let mut slot = Slot::new(rate());
    assert!(slot.replace(boxed(EffectKind::Reverb)).is_none());
    assert!(slot.replace(boxed(EffectKind::Delay)).is_none());
    assert_eq!(slot.kind(), Some(EffectKind::Delay));
    let controls = EffectKind::Delay.defaults();
    let mut freed = None;
    for frame in 0..1_000 {
        slot.process(signal(frame), &controls, true, tempo_at(frame));
        if let Some(effect) = slot.take_finished() {
            freed = Some((frame, effect.kind()));
            break;
        }
    }
    let (frame, kind) = freed.expect("the reverb never came back");
    assert_eq!(kind, EffectKind::Reverb);
    assert!(frame <= 480, "it took {frame} frames");
}

#[test]
fn the_single_mode_of_a_chain_plays_its_own_effect() {
    let mut chain = Chain::new(rate());
    let _ = chain
        .slot_mut(Place::Single)
        .unwrap()
        .replace(boxed(EffectKind::Gater));
    let mut values = UnitValues {
        mode: UnitMode::Single,
        dry_wet: 1.0,
        ..UnitValues::default()
    };
    values.single.controls = Controls {
        knobs: [0.0, 0.0, 0.625],
        keys: [false; 2],
    };
    values.single.on = true;
    let mut levels = Vec::new();
    for frame in 0..12_000 {
        levels.push(chain.process([1.0, 1.0], &values, tempo_at(frame))[0]);
    }
    let opened = levels[6_000..6_200].iter().copied().fold(0.0, f32::max);
    assert!(
        opened > 0.6,
        "the gate of single mode did not open: {opened}"
    );
    assert!(levels[11_900] < 0.01);
}

#[test]
fn the_mixer_effect_at_zero_does_nothing() {
    let mut mixer = MixerEffect::new(rate());
    let _ = mixer.replace(boxed(EffectKind::Reverb));
    for frame in 0..4_000 {
        let input = signal(frame);
        assert_eq!(mixer.process(input, 0.0, tempo_at(frame)), input);
    }
    let mut turned = Vec::new();
    for frame in 4_000..30_000 {
        let input = signal(frame);
        let output = mixer.process(input, 0.8, tempo_at(frame));
        turned.push(output[0] - input[0]);
    }
    assert!(rms(&turned[10_000..]) > 1e-3);
}

#[test]
fn names_round_trip() {
    for kind in EffectKind::ALL {
        assert_eq!(EffectKind::parse(kind.name()).unwrap(), kind);
    }
    for mode in UnitMode::ALL {
        assert_eq!(UnitMode::parse(mode.name()).unwrap(), mode);
    }
    assert!(EffectKind::parse("Delay").is_err());
}
