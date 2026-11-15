//! The deviceless render: deterministic, and correct within a tolerance that holds on any
//! system.

// The lint configuration allows these in test functions; this whole file is test code.
#![allow(clippy::unwrap_used)]

mod common;

use std::io::Cursor;
use std::path::PathBuf;

use common::{RATE, rate, sine_file};
use platter_core::DeckId;
use platter_engine::{
    Band, Command, CrossfaderCurve, RenderError, RenderPlan, TimedCommand, WavWriter, render,
};
use platter_testkit::measure::{peak, sine_fit};
use platter_testkit::signal::channel;

fn at(at_seconds: f64, command: Command) -> TimedCommand {
    TimedCommand {
        at_seconds,
        command,
    }
}

/// A short mix that touches every kind of command.
fn busy_plan(track_a: PathBuf, track_b: PathBuf) -> RenderPlan {
    use DeckId::{A, B};
    let commands = vec![
        at(
            0.0,
            Command::DeckLoad {
                deck: A,
                path: track_a,
            },
        ),
        at(
            0.0,
            Command::DeckLoad {
                deck: B,
                path: track_b,
            },
        ),
        at(0.0, Command::MixerCrossfaderSet { position: 0.0 }),
        at(0.1, Command::DeckPlay { deck: A }),
        at(
            0.5,
            Command::DeckTempoSet {
                deck: A,
                ratio: 1.04,
            },
        ),
        at(0.6, Command::DeckPlayPause { deck: B }),
        at(
            0.7,
            Command::MixerEqSet {
                deck: A,
                band: Band::Low,
                position: 0.0,
            },
        ),
        at(0.8, Command::MixerCrossfaderSet { position: 0.5 }),
        at(
            0.9,
            Command::MixerFilterSet {
                deck: B,
                position: -0.4,
            },
        ),
        at(
            1.0,
            Command::DeckNudge {
                deck: B,
                amount: 0.03,
            },
        ),
        at(
            1.1,
            Command::DeckNudge {
                deck: B,
                amount: 0.0,
            },
        ),
        at(
            1.2,
            Command::MixerCrossfaderCurveSet {
                curve: CrossfaderCurve::Linear,
            },
        ),
        at(1.3, Command::DeckCueSet { deck: A }),
        at(1.4, Command::MixerGainSet { deck: B, db: 3.0 }),
        at(
            1.5,
            Command::DeckSeek {
                deck: B,
                seconds: 0.25,
            },
        ),
        at(
            1.6,
            Command::MixerFaderSet {
                deck: A,
                position: 0.7,
            },
        ),
        at(1.7, Command::DeckCueGoto { deck: A }),
        at(1.8, Command::MasterGainSet { db: -3.0 }),
        at(1.9, Command::MixerCrossfaderSet { position: 1.0 }),
        at(2.2, Command::DeckPause { deck: B }),
        at(2.3, Command::DeckEject { deck: A }),
    ];
    RenderPlan {
        sample_rate: rate(),
        seconds: 2.5,
        commands,
    }
}

fn render_to_wav(plan: &RenderPlan) -> Vec<u8> {
    let mut writer = WavWriter::new(Cursor::new(Vec::new()), plan.sample_rate, 2).unwrap();
    render(plan, |block| writer.write(block)).unwrap();
    writer.finish().unwrap().into_inner()
}

#[test]
fn two_runs_give_the_same_file() {
    let plan = busy_plan(
        sine_file("render-same-a", 440.0, 3.0, 0.5, RATE),
        sine_file("render-same-b", 330.0, 3.0, 0.5, 44_100),
    );
    let first = render_to_wav(&plan);
    let second = render_to_wav(&plan);
    assert_eq!(first.len(), 56 + 120_000 * 8);
    assert!(first == second, "the two renders differ");
    // The mix is not trivially silent.
    let audio = platter_decode::decode_bytes(first, Some("wav")).unwrap();
    assert!(peak(&audio.samples) > 0.3);
}

#[test]
fn the_mix_matches_what_the_commands_describe() {
    use DeckId::{A, B};
    let plan = RenderPlan {
        sample_rate: rate(),
        seconds: 3.0,
        commands: vec![
            // The levels below are those of the files.
            at(0.0, Command::MixerAutogainToggle),
            at(
                0.0,
                Command::DeckLoad {
                    deck: A,
                    path: sine_file("render-mix-a", 440.0, 4.0, 0.5, RATE),
                },
            ),
            at(
                0.0,
                Command::DeckLoad {
                    deck: B,
                    path: sine_file("render-mix-b", 1_000.0, 4.0, 0.5, 44_100),
                },
            ),
            at(0.0, Command::MixerCrossfaderSet { position: 0.0 }),
            at(0.0, Command::DeckPlay { deck: A }),
            at(1.0, Command::DeckPlay { deck: B }),
            at(1.0, Command::MixerCrossfaderSet { position: 0.5 }),
            at(2.0, Command::MixerCrossfaderSet { position: 1.0 }),
        ],
    };
    let mut output = Vec::new();
    let report = render(&plan, |block| {
        output.extend_from_slice(block);
        Ok(())
    })
    .unwrap();
    assert_eq!(report.frames, 144_000);
    assert_eq!(output.len(), 288_000);
    assert!(report.peak <= 1.0 && report.peak > 0.5);
    assert_eq!(report.lowest_limiter_gain, 1.0);

    let left = channel(&output, 2, 0);
    assert_eq!(left, channel(&output, 2, 1));
    let amplitudes = |from: usize, to: usize| {
        let window = &left[from..to];
        (
            sine_fit(window, 440.0, f64::from(RATE)).amplitude,
            sine_fit(window, 1_000.0, f64::from(RATE)).amplitude,
        )
    };
    // Tolerances wide enough for any floating-point implementation, narrow enough to
    // catch a wrong gain or a wrong pitch.
    let (a, b) = amplitudes(9_600, 48_000);
    assert!(
        (a - 0.5).abs() < 0.005 && b < 0.001,
        "first second: {a} {b}"
    );
    let (a, b) = amplitudes(57_600, 96_000);
    assert!(
        (a - 0.3536).abs() < 0.005 && (b - 0.3536).abs() < 0.005,
        "second second: {a} {b}"
    );
    let (a, b) = amplitudes(105_600, 144_000);
    assert!(
        a < 0.001 && (b - 0.5).abs() < 0.005,
        "third second: {a} {b}"
    );
}

#[test]
fn commands_apply_at_their_exact_frame() {
    let plan = RenderPlan {
        sample_rate: rate(),
        seconds: 0.5,
        commands: vec![
            at(0.25, Command::DeckPlay { deck: DeckId::A }),
            at(
                0.0,
                Command::DeckLoad {
                    deck: DeckId::A,
                    path: sine_file("render-frame", 440.0, 1.0, 0.5, RATE),
                },
            ),
        ],
    };
    let mut output = Vec::new();
    render(&plan, |block| {
        output.extend_from_slice(block);
        Ok(())
    })
    .unwrap();
    let left = channel(&output, 2, 0);
    // Play lands on frame 12 000; the sine starts at phase zero, so the first frame that
    // can differ from zero is the one after.
    assert!(left[..=12_000].iter().all(|sample| *sample == 0.0));
    assert!(left[12_001..12_010].iter().any(|sample| *sample != 0.0));
}

#[test]
fn problems_in_the_plan_are_errors() {
    let play = Command::DeckPlay { deck: DeckId::A };
    let plan = |seconds: f64, commands: Vec<TimedCommand>| RenderPlan {
        sample_rate: rate(),
        seconds,
        commands,
    };
    let run = |plan: RenderPlan| render(&plan, |_| Ok(()));

    for seconds in [f64::NAN, -1.0, f64::INFINITY, 1e12] {
        assert!(matches!(
            run(plan(seconds, vec![])),
            Err(RenderError::Value(_))
        ));
    }
    assert!(matches!(
        run(plan(1.0, vec![at(f64::NAN, play.clone())])),
        Err(RenderError::Value(_))
    ));
    assert!(matches!(
        run(plan(1.0, vec![at(-0.5, play)])),
        Err(RenderError::Value(_))
    ));
    let bad_tempo = Command::DeckTempoSet {
        deck: DeckId::A,
        ratio: 9.0,
    };
    assert!(matches!(
        run(plan(1.0, vec![at(0.5, bad_tempo)])),
        Err(RenderError::Command {
            id: "deck.tempo.set",
            ..
        })
    ));
    let missing = Command::DeckLoad {
        deck: DeckId::B,
        path: PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("render-missing.wav"),
    };
    assert!(matches!(
        run(plan(1.0, vec![at(0.0, missing)])),
        Err(RenderError::Load {
            deck: DeckId::B,
            ..
        })
    ));
    let failing_sink = render(&plan(1.0, vec![]), |_| {
        Err(std::io::Error::other("disk full"))
    });
    assert!(matches!(failing_sink, Err(RenderError::Output(_))));
}

#[test]
fn an_empty_plan_renders_silence() {
    let plan = RenderPlan {
        sample_rate: rate(),
        seconds: 0.25,
        commands: vec![],
    };
    let mut output = Vec::new();
    let report = render(&plan, |block| {
        output.extend_from_slice(block);
        Ok(())
    })
    .unwrap();
    assert_eq!(output, vec![0.0; 24_000]);
    assert_eq!(report.peak, 0.0);
}
