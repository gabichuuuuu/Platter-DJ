//! The MIDI clock the master clock sends: on which frames its messages fall.
//!
//! Behaviour note: `docs/behavior/midi-clock.md`; design: `docs/decisions/0041-midi-clock.md`.

mod common;

use common::{RATE, Rig, sine_file};
use platter_core::DeckId;
use platter_engine::Command;
use platter_engine::midi_clock::{ClockEvent, ClockFeed, ClockMessage, TICKS_PER_BEAT};

fn drain(feed: &mut ClockFeed) -> Vec<ClockEvent> {
    std::iter::from_fn(|| feed.pop()).collect()
}

fn ticks(events: &[ClockEvent]) -> Vec<u64> {
    events
        .iter()
        .filter(|event| event.message == ClockMessage::Tick)
        .map(|event| event.frame)
        .collect()
}

/// The frames between ticks, over the ticks after the first beat.
fn spacing(events: &[ClockEvent]) -> (f64, f64) {
    let frames = ticks(events);
    let gaps: Vec<f64> = frames
        .windows(2)
        .skip(TICKS_PER_BEAT as usize)
        .map(|pair| (pair[1] - pair[0]) as f64)
        .collect();
    let mean = gaps.iter().sum::<f64>() / gaps.len() as f64;
    let worst = gaps
        .iter()
        .map(|gap| (gap - mean).abs())
        .fold(0.0, f64::max);
    (mean, worst)
}

#[test]
fn the_clock_sends_twenty_four_ticks_to_each_beat_of_its_tempo() {
    for bpm in [120.0, 128.0, 174.0] {
        let mut rig = Rig::new();
        let mut feed = rig.engine.take_clock_feed().unwrap();
        assert!(
            rig.engine.take_clock_feed().is_none(),
            "one feed per engine"
        );
        rig.send(Command::ClockTempoSet { bpm });
        rig.send(Command::ClockMidiSendToggle);
        rig.run_stereo(4.0);
        let events = drain(&mut feed);
        assert_eq!(events[0].message, ClockMessage::Start);
        assert_eq!(events[1].message, ClockMessage::Tick);
        assert_eq!(events[0].frame, events[1].frame);
        let (mean, worst) = spacing(&events);
        let expected = f64::from(RATE) * 60.0 / bpm / f64::from(TICKS_PER_BEAT);
        assert!((mean - expected).abs() < 0.05, "{bpm} BPM: {mean} frames");
        // Each tick on the first frame of its 24th: never more than a frame off.
        assert!(worst <= 1.0, "{bpm} BPM: {worst} frames off");
        let beats = (ticks(&events).len() as f64) / f64::from(TICKS_PER_BEAT);
        let heard = 4.0 * bpm / 60.0;
        assert!((beats - heard).abs() <= 1.0, "{beats} beats in {heard}");
        rig.engine.poll();
        let clock = rig.engine.snapshot().clock;
        assert!(clock.midi_clock && clock.midi_clock_running);
    }
}

#[test]
fn a_bend_of_the_clock_is_sent_with_it() {
    let mut rig = Rig::new();
    let mut feed = rig.engine.take_clock_feed().unwrap();
    rig.send(Command::ClockMidiSendToggle);
    rig.send(Command::ClockBend { amount: 0.04 });
    rig.run_stereo(4.0);
    let (mean, _) = spacing(&drain(&mut feed));
    let expected = f64::from(RATE) * 60.0 / (120.0 * 1.04) / f64::from(TICKS_PER_BEAT);
    assert!((mean - expected).abs() < 0.1, "{mean} frames");
}

#[test]
fn the_beats_of_a_deck_that_is_the_tempo_master_are_sent() {
    let mut rig = Rig::new();
    let mut feed = rig.engine.take_clock_feed().unwrap();
    rig.load(DeckId::A, sine_file("clock-master", 440.0, 20.0, 0.5, RATE));
    rig.send(Command::DeckGridBpmSet {
        deck: DeckId::A,
        bpm: 125.0,
    });
    rig.send(Command::DeckPlay { deck: DeckId::A });
    rig.send(Command::ClockMidiSendToggle);
    rig.run_stereo(4.0);
    rig.engine.poll();
    assert_eq!(rig.engine.snapshot().clock.master, Some(DeckId::A));
    let (mean, _) = spacing(&drain(&mut feed));
    let expected = f64::from(RATE) * 60.0 / 125.0 / f64::from(TICKS_PER_BEAT);
    assert!((mean - expected).abs() < 0.1, "{mean} frames");
}

#[test]
fn stopping_sends_stop_and_the_setting_survives_a_new_engine() {
    let mut rig = Rig::new();
    let mut feed = rig.engine.take_clock_feed().unwrap();
    rig.send(Command::ClockMidiSendToggle);
    rig.run_stereo(1.0);
    rig.send(Command::ClockMidiSendToggle);
    rig.run_stereo(1.0);
    let events = drain(&mut feed);
    assert_eq!(
        events.last().map(|event| event.message),
        Some(ClockMessage::Stop)
    );
    rig.send(Command::ClockMidiSendToggle);
    let (engine, _) = platter_engine::Engine::with_controls_of(common::rate(), &rig.engine);
    assert!(engine.snapshot().clock.midi_clock);
    assert!(Command::from_args("clock.midi.restart", &Default::default()).is_ok());
}
