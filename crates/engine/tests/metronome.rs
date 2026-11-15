//! The metronome of the master clock. Behaviour note: `docs/behavior/sync.md`, section
//! *Metronome*.

mod common;

use common::{RATE, Rig, sine_file};
use platter_core::DeckId;
use platter_engine::{Command, Engine, SizeStep};
use platter_testkit::measure::{max_step, peak};
use platter_testkit::signal::channel;

const A: DeckId = DeckId::A;
/// A beat at 120 BPM, in frames.
const BEAT_120: usize = 24_000;

fn metronome_on() -> Rig {
    let mut rig = Rig::new();
    rig.send(Command::ClockMetronomeToggle);
    rig
}

/// The left headphone channel of a four-channel device.
fn headphones(rig: &mut Rig, seconds: f64) -> Vec<f32> {
    channel(&rig.run_channels(seconds, 4), 4, 2)
}

/// The frames where a click starts: the first sound after at least 10 ms of silence.
fn onsets(samples: &[f32]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut quiet = usize::MAX;
    for (frame, sample) in samples.iter().enumerate() {
        if *sample == 0.0 {
            quiet = quiet.saturating_add(1);
            continue;
        }
        if quiet >= 480 {
            // The click starts on the zero of its sine, one frame before its first sound.
            out.push(frame.saturating_sub(1));
        }
        quiet = 0;
    }
    out
}

fn spacing(onsets: &[usize]) -> Vec<usize> {
    onsets.windows(2).map(|pair| pair[1] - pair[0]).collect()
}

#[test]
fn the_metronome_clicks_on_every_beat_of_the_clock_on_the_headphones_only() {
    let mut rig = metronome_on();
    let output = rig.run_channels(2.2, 4);
    let left = channel(&output, 4, 2);
    let clicks = onsets(&left);
    assert_eq!(clicks.len(), 4, "{clicks:?}");
    assert!(
        spacing(&clicks).iter().all(|gap| *gap == BEAT_120),
        "{clicks:?}"
    );
    assert_eq!(left, channel(&output, 4, 3));
    for master in [0, 1] {
        assert!(
            channel(&output, 4, master)
                .iter()
                .all(|sample| *sample == 0.0)
        );
    }
    // 18 dB below full scale, and no step at either end of a click.
    let level = peak(&left);
    assert!(level > 0.12 && level <= 0.125, "{level}");
    assert!(max_step(&left) < 0.03, "{}", max_step(&left));
    let snapshot = rig.engine.snapshot();
    assert!(snapshot.clock.metronome);
}

#[test]
fn the_metronome_starts_off_and_switches_off_again() {
    let mut rig = Rig::new();
    assert!(!rig.engine.snapshot().clock.metronome);
    assert!(
        headphones(&mut rig, 1.2)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    rig.send(Command::ClockMetronomeToggle);
    rig.send(Command::ClockMetronomeToggle);
    assert!(!rig.engine.snapshot().clock.metronome);
    assert!(
        headphones(&mut rig, 1.2)
            .iter()
            .all(|sample| *sample == 0.0)
    );
}

#[test]
fn a_device_with_two_outputs_does_not_hear_the_metronome() {
    let mut rig = metronome_on();
    for channels in [2, 3] {
        let output = rig.run_channels(1.2, channels);
        assert!(output.iter().all(|sample| *sample == 0.0), "{channels}");
    }
}

#[test]
fn the_headphone_volume_sets_the_level_of_the_clicks() {
    let mut rig = metronome_on();
    rig.send(Command::MixerCueVolumeSet { db: -6.0 });
    let level = f64::from(peak(&headphones(&mut rig, 1.2)));
    let expected = 0.125 * 10f64.powf(-6.0 / 20.0);
    assert!(level <= expected && level > expected * 0.95, "{level}");
}

#[test]
fn the_clicks_end_before_the_next_beat_at_the_fastest_tempo() {
    let mut rig = metronome_on();
    rig.send(Command::ClockTempoSet { bpm: 400.0 });
    let left = headphones(&mut rig, 1.0);
    let clicks = onsets(&left);
    // 150 ms apart; the onsets are only found after a silence, so each click has ended.
    assert!(clicks.len() >= 6, "{clicks:?}");
    assert!(
        spacing(&clicks).iter().all(|gap| *gap == 7_200),
        "{clicks:?}"
    );
}

/// Deck A playing a track with a grid at 100 BPM, the tempo master.
fn leading_deck(name: &str) -> Rig {
    let mut rig = metronome_on();
    rig.load(A, sine_file(name, 200.0, 8.0, 0.4, RATE));
    rig.send(Command::DeckGridBpmSet {
        deck: A,
        bpm: 100.0,
    });
    rig.send(Command::DeckPlay { deck: A });
    rig
}

#[test]
fn the_clicks_fall_on_the_beats_of_the_deck_that_is_the_tempo_master() {
    let mut rig = leading_deck("metronome-lead");
    rig.run_channels(0.1, 4);
    assert_eq!(rig.engine.snapshot().clock.master, Some(A));
    // Deck A is not pre-listened, so the headphones carry the clicks alone.
    let clicks = onsets(&headphones(&mut rig, 2.5));
    assert_eq!(clicks.len(), 4, "{clicks:?}");
    assert!(
        spacing(&clicks).iter().all(|gap| *gap == 28_800),
        "{clicks:?}"
    );
}

#[test]
fn a_jump_is_taken_the_shorter_way_round_the_beat() {
    // At 100 BPM a beat lasts 0.6 s: at 1.44 s the deck is 0.4 into its third beat.
    let mut rig = leading_deck("metronome-jump");
    rig.run_channels(1.44, 4);
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 1.26,
    });
    // From 0.1 into the beat, the next one is 0.5 s away.
    let clicks = onsets(&headphones(&mut rig, 0.55));
    assert_eq!(clicks.len(), 1, "{clicks:?}");
    assert!(clicks[0] > 23_000, "{clicks:?}");

    // From 0.8 into the third beat over the start of the fourth, to 0.1 into it: a click at
    // once, then the fifth beat 0.54 s later.
    let mut rig = leading_deck("metronome-forward");
    rig.run_channels(1.68, 4);
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 1.86,
    });
    let clicks = onsets(&headphones(&mut rig, 0.6));
    assert_eq!(clicks.len(), 2, "{clicks:?}");
    assert!(clicks[0] < 960, "{clicks:?}");

    // A long jump forward, from 0.4 into the third beat to 0.2 into the fourth, is taken the
    // short way, backwards: no click until the fifth beat.
    let mut rig = leading_deck("metronome-long");
    rig.run_channels(1.44, 4);
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 1.92,
    });
    let clicks = onsets(&headphones(&mut rig, 0.55));
    assert_eq!(clicks.len(), 1, "{clicks:?}");
    assert!(clicks[0] > 22_000, "{clicks:?}");
}

#[test]
fn a_clock_at_zero_beats_per_minute_does_not_click() {
    // A tempo master with a tempo of zero holds the clock still.
    let mut rig = leading_deck("metronome-still");
    rig.run_channels(0.1, 4);
    rig.send(Command::DeckTempoSet {
        deck: A,
        ratio: 0.0,
    });
    rig.run_channels(0.2, 4);
    assert!(
        headphones(&mut rig, 1.5)
            .iter()
            .all(|sample| *sample == 0.0)
    );
}

#[test]
fn a_new_processor_keeps_the_metronome_on() {
    let rig = metronome_on();
    let (mut engine, mut processor) = Engine::with_controls_of(common::rate(), &rig.engine);
    assert!(engine.snapshot().clock.metronome);
    let mut output = vec![0.0; 4 * 60_000];
    for block in output.chunks_mut(4 * 256) {
        processor.process(block, 4);
    }
    // 1.25 s at 120 BPM: the beats at 0.5 s and 1 s.
    assert_eq!(onsets(&channel(&output, 4, 2)).len(), 2);
    engine.dispatch(Command::ClockMetronomeToggle).unwrap();
    assert!(!engine.snapshot().clock.metronome);
}

#[test]
fn a_tempo_master_in_a_loop_shorter_than_a_beat_leaves_the_clock_clicking_on_its_beats() {
    let mut rig = leading_deck("metronome-loop");
    rig.run_channels(1.0, 4);
    for _ in 0..3 {
        rig.send(Command::DeckLoopSizeStep {
            deck: A,
            direction: SizeStep::Down,
        });
    }
    rig.send(Command::DeckLoopToggle { deck: A });
    rig.run_channels(0.1, 4);
    rig.engine.poll();
    let looping = rig.engine.snapshot().decks[0].looping.region;
    assert!(looping.is_some_and(|region| region.end - region.start < 0.6));
    // The clock counts on at 100 BPM: a click every 0.6 s.
    let clicks = onsets(&headphones(&mut rig, 2.5));
    assert_eq!(clicks.len(), 4, "{clicks:?}");
    assert!(
        spacing(&clicks).iter().all(|gap| *gap == 28_800),
        "{clicks:?}"
    );
}
