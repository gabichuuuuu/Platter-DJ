//! The effect units and the mixer effect in the engine: assignment, routing and restart.
//!
//! Behaviour note: `docs/behavior/effects.md`.

#![allow(clippy::unwrap_used)]

mod common;

use common::{RATE, Rig, sine_file};
use platter_core::DeckId;
use platter_engine::{Command, EffectKind, Engine, FxRouting, FxUnit};
use platter_testkit::measure::{rms, sine_fit};
use platter_testkit::signal::channel;

const LEVEL: f32 = 0.2;
const UNIT: FxUnit = FxUnit::ALL[0];

/// Unit 1 holds the filter in its first slot, switched on, fully wet, turned all the way to
/// the low-pass: it takes a 1 kHz tone down by far more than 20 dB.
fn low_pass_on_unit_one(rig: &mut Rig) {
    rig.send(Command::FxSlotEffectSet {
        unit: UNIT,
        slot: 0,
        effect: Some(EffectKind::Filter),
    });
    rig.send(Command::FxSlotAmountSet {
        unit: UNIT,
        slot: 0,
        position: 0.0,
    });
    rig.send(Command::FxSlotToggle {
        unit: UNIT,
        slot: 0,
    });
    rig.send(Command::FxUnitDryWetSet {
        unit: UNIT,
        position: 1.0,
    });
}

fn amplitude(output: &[f32], hz: f64) -> f64 {
    let left = channel(output, 2, 0);
    sine_fit(&left[left.len() / 2..], hz, f64::from(RATE)).amplitude
}

/// Decks A and B play 1 kHz and 1.5 kHz in the middle of the crossfader.
fn two_decks(name: &str) -> Rig {
    let mut rig = Rig::new();
    rig.load(
        DeckId::A,
        sine_file(&format!("{name}-a"), 1_000.0, 6.0, LEVEL, RATE),
    );
    rig.load(
        DeckId::B,
        sine_file(&format!("{name}-b"), 1_500.0, 6.0, LEVEL, RATE),
    );
    rig.send(Command::DeckPlay { deck: DeckId::A });
    rig.send(Command::DeckPlay { deck: DeckId::B });
    rig
}

#[test]
fn a_unit_acts_only_on_the_channels_assigned_to_it() {
    let mut rig = two_decks("fx-assigned");
    low_pass_on_unit_one(&mut rig);
    let before = rig.run_stereo(0.5);
    let (a, b) = (amplitude(&before, 1_000.0), amplitude(&before, 1_500.0));
    // A unit no channel goes through changes nothing.
    assert!(a > 0.05 && b > 0.05, "{a} {b}");

    rig.send(Command::MixerFxAssignToggle {
        deck: DeckId::A,
        unit: UNIT,
    });
    let after = rig.run_stereo(0.5);
    assert!(
        amplitude(&after, 1_000.0) < a / 10.0,
        "deck A was not filtered"
    );
    assert!(
        (amplitude(&after, 1_500.0) - b).abs() < 0.002,
        "deck B changed"
    );

    let snapshot = rig.engine.snapshot();
    assert!(snapshot.effects.is_assigned(DeckId::A, UNIT));
    assert!(!snapshot.effects.is_assigned(DeckId::B, UNIT));
    assert_eq!(
        snapshot.effects.unit(UNIT).slots[0],
        Some(EffectKind::Filter)
    );

    // Taken out again, deck A comes back as it was.
    rig.send(Command::MixerFxAssignToggle {
        deck: DeckId::A,
        unit: UNIT,
    });
    let back = rig.run_stereo(0.5);
    assert!((amplitude(&back, 1_000.0) - a).abs() < 0.002);
}

#[test]
fn a_delay_after_the_fader_rings_on_when_the_fader_closes_and_an_insert_does_not() {
    for (routing, rings) in [(FxRouting::Insert, false), (FxRouting::PostFader, true)] {
        let mut rig = Rig::with_deck_a(sine_file(
            &format!("fx-routing-{}", routing.name()),
            440.0,
            6.0,
            LEVEL,
            RATE,
        ));
        rig.send(Command::DeckPlay { deck: DeckId::A });
        rig.send(Command::FxUnitRoutingSet {
            unit: UNIT,
            routing,
        });
        rig.send(Command::FxSlotEffectSet {
            unit: UNIT,
            slot: 0,
            effect: Some(EffectKind::Delay),
        });
        rig.send(Command::FxSlotToggle {
            unit: UNIT,
            slot: 0,
        });
        rig.send(Command::MixerFxAssignToggle {
            deck: DeckId::A,
            unit: UNIT,
        });
        rig.run_stereo(1.0);
        rig.send(Command::MixerFaderSet {
            deck: DeckId::A,
            position: 0.0,
        });
        let closed = channel(&rig.run_stereo(0.3), 2, 0);
        let tail = rms(&closed[RATE as usize / 20..]);
        if rings {
            assert!(tail > 1e-3, "no tail after the fader: {tail}");
        } else {
            assert!(
                tail < 1e-6,
                "{} sounded with the fader closed: {tail}",
                routing.name()
            );
        }
    }
}

#[test]
fn an_engine_at_another_rate_keeps_the_effects_and_their_assignments() {
    let mut rig = two_decks("fx-restart");
    low_pass_on_unit_one(&mut rig);
    rig.send(Command::MixerFxAssignToggle {
        deck: DeckId::C,
        unit: UNIT,
    });
    rig.send(Command::MixerEffectSelect {
        deck: DeckId::C,
        effect: EffectKind::Gater,
    });
    let kept = rig.engine.snapshot().effects;

    let rate = platter_core::SampleRate::new(44_100).unwrap();
    let (mut engine, mut processor) = Engine::with_controls_of(rate, &rig.engine);
    assert_eq!(engine.snapshot().effects, kept);
    engine
        .dispatch(Command::DeckLoad {
            deck: DeckId::C,
            path: sine_file("fx-restart-c", 1_000.0, 4.0, LEVEL, 44_100),
        })
        .unwrap();
    engine.wait_for_loads();
    engine
        .dispatch(Command::MixerCrossfaderAssign {
            deck: DeckId::C,
            side: platter_engine::CrossfaderSide::Thru,
        })
        .unwrap();
    engine
        .dispatch(Command::DeckPlay { deck: DeckId::C })
        .unwrap();
    let mut output = vec![0.0f32; 44_100 * 2];
    for block in output.chunks_mut(512) {
        processor.process(block, 2);
    }
    let left = channel(&output, 2, 0);
    let heard = sine_fit(&left[22_050..], 1_000.0, 44_100.0).amplitude;
    // Deck C goes through the low-pass of unit 1 at the new rate too.
    assert!(
        heard < f64::from(LEVEL) / 10.0,
        "deck C was not filtered: {heard}"
    );
}

#[test]
fn commands_out_of_range_change_nothing() {
    let mut rig = Rig::new();
    let before = rig.engine.snapshot().effects;
    for command in [
        Command::FxUnitDryWetSet {
            unit: UNIT,
            position: 1.5,
        },
        Command::FxSlotAmountSet {
            unit: UNIT,
            slot: 0,
            position: f64::NAN,
        },
        Command::MixerEffectSet {
            deck: DeckId::A,
            position: -0.1,
        },
    ] {
        assert!(rig.engine.dispatch(command).is_err());
    }
    assert_eq!(rig.engine.snapshot().effects, before);
}
