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

#[test]
fn an_external_source_takes_the_tempo_and_the_beat_it_is_given() {
    // Without the external source the tempo given is not taken.
    let mut internal = Rig::new();
    internal.engine.follow_clock(126.0, 0.0).unwrap();
    internal.run_stereo(0.1);
    internal.engine.poll();
    assert_eq!(internal.engine.snapshot().clock.bpm, 120.0);
    // The frames of this one start with the clock followed.
    let mut rig = Rig::new();
    let mut feed = rig.engine.take_clock_feed().unwrap();
    rig.send(Command::ClockSourceSet { external: true });
    rig.send(Command::ClockMidiSendToggle);
    // A clock at 126 BPM whose beat starts a quarter of a second in: given before each block.
    let block = 480;
    let seconds_per_beat = 60.0 / 126.0;
    let mut frame = 0usize;
    let mut out = vec![0.0f32; block * 2];
    while frame < 4 * 48_000 {
        let time = frame as f64 / f64::from(RATE);
        let phase = ((time - 0.25) / seconds_per_beat).rem_euclid(1.0);
        rig.engine.follow_clock(126.0, phase).unwrap();
        rig.processor.process(&mut out, 2);
        frame += block;
    }
    rig.engine.poll();
    let clock = rig.engine.snapshot().clock;
    assert!(clock.external);
    assert!((clock.bpm - 126.0).abs() < 1e-9, "{}", clock.bpm);
    // After the first second, each tick sent falls on a 24th of the beat of the clock
    // followed, within a tenth of a 24th.
    let events = drain(&mut feed);
    let late: Vec<u64> = events
        .iter()
        .filter(|event| event.message == ClockMessage::Tick && event.frame > u64::from(RATE))
        .map(|event| event.frame)
        .collect();
    assert!(late.len() > 2 * TICKS_PER_BEAT as usize);
    // The jump to the beat followed restarted the clock sent: from its last Start, every 24th
    // tick is the start of a beat.
    for frame in &late {
        let beat = (*frame as f64 / f64::from(RATE) - 0.25) / seconds_per_beat;
        let ticks = beat * f64::from(TICKS_PER_BEAT);
        let off = (ticks + 0.5).rem_euclid(1.0) - 0.5;
        assert!(off.abs() < 0.1, "{off} of a 24th at frame {frame}");
    }
    let restart = events
        .iter()
        .rposition(|event| event.message == ClockMessage::Start)
        .unwrap();
    assert!(events[restart].frame > 0, "a restart after the jump");
    let beats: Vec<u64> = events[restart..]
        .iter()
        .filter(|event| event.message == ClockMessage::Tick)
        .map(|event| event.frame)
        .step_by(TICKS_PER_BEAT as usize)
        .collect();
    assert!(beats.len() >= 3);
    for frame in beats {
        let beat = (frame as f64 / f64::from(RATE) - 0.25) / seconds_per_beat;
        let off = (beat + 0.5).rem_euclid(1.0) - 0.5;
        assert!(off.abs() < 0.01, "{off} of a beat at frame {frame}");
    }
    // The tempo commands of the clock and the choice of a deck do nothing meanwhile.
    rig.send(Command::ClockTempoSet { bpm: 90.0 });
    rig.send(Command::DeckMasterSet { deck: DeckId::A });
    rig.run_stereo(0.1);
    rig.engine.poll();
    let clock = rig.engine.snapshot().clock;
    assert!((clock.bpm - 126.0).abs() < 1e-9);
    assert_eq!(clock.master, None);
    rig.send(Command::ClockSourceSet { external: false });
    rig.send(Command::ClockTempoSet { bpm: 90.0 });
    rig.run_stereo(0.1);
    rig.engine.poll();
    assert_eq!(rig.engine.snapshot().clock.bpm, 90.0);
}
