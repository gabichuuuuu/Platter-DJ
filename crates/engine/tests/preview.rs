//! The preview player: a track in the headphones without a deck. Behaviour note:
//! `docs/behavior/browser.md`, *Preview player*.

// The helpers that write the signals and read the output are not tests themselves.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::{RATE, Rig, sine_file};
use platter_engine::{Command, Event};
use platter_testkit::signal::channel;

/// The peak of one channel of four-channel output.
fn peak(output: &[f32], index: usize) -> f32 {
    channel(output, 4, index)
        .iter()
        .fold(0.0f32, |peak, sample| peak.max(sample.abs()))
}

fn preview(rig: &mut Rig, path: std::path::PathBuf) {
    rig.send(Command::PreviewLoad { path });
    let events = rig.engine.wait_for_loads();
    assert!(events.is_empty(), "{events:?}");
}

#[test]
fn a_previewed_track_plays_in_the_headphones_and_not_on_the_master() {
    let mut rig = Rig::new();
    preview(&mut rig, sine_file("preview-sine", 440.0, 2.0, 0.5, RATE));
    let output = rig.run_channels(0.5, 4);
    assert!(peak(&output, 0) < 1e-6, "master left");
    assert!(peak(&output, 1) < 1e-6, "master right");
    assert!(peak(&output, 2) > 0.3, "headphones left");
    assert!(peak(&output, 3) > 0.3, "headphones right");
    let snapshot = rig.engine.snapshot();
    assert!(snapshot.preview.playing);
    assert!((snapshot.preview.position_seconds - 0.5).abs() < 0.02);
    assert_eq!(
        snapshot
            .preview
            .track
            .as_ref()
            .map(|track| track.seconds.round()),
        Some(2.0)
    );
    // Nothing of a deck changed.
    assert!(snapshot.decks.iter().all(|deck| deck.track.is_none()));
}

#[test]
fn play_pause_and_seek_move_the_preview() {
    let mut rig = Rig::new();
    preview(&mut rig, sine_file("preview-seek", 440.0, 4.0, 0.5, RATE));
    rig.run_channels(0.1, 4);
    rig.send(Command::PreviewPlayPause);
    // The fade out, then silence.
    rig.run_channels(0.05, 4);
    let paused = rig.run_channels(0.2, 4);
    assert!(peak(&paused, 2) < 1e-6);
    assert!(!rig.engine.snapshot().preview.playing);
    rig.send(Command::PreviewSeek { position: 0.75 });
    rig.send(Command::PreviewPlayPause);
    rig.run_channels(0.01, 4);
    let position = rig.engine.snapshot().preview.position_seconds;
    assert!((position - 3.01).abs() < 0.02, "{position}");
    assert!(
        rig.engine
            .dispatch(Command::PreviewSeek { position: 1.5 })
            .is_err()
    );
}

#[test]
fn the_preview_stops_at_the_end_of_its_track() {
    let mut rig = Rig::new();
    preview(&mut rig, sine_file("preview-end", 440.0, 0.2, 0.5, RATE));
    let output = rig.run_channels(0.5, 4);
    assert!(peak(&output, 2) > 0.3);
    assert!(!rig.engine.snapshot().preview.playing);
    let after = rig.run_channels(0.1, 4);
    assert!(peak(&after, 2) < 1e-6);
}

#[test]
fn a_file_that_is_not_audio_leaves_the_preview_as_it_was() {
    let mut rig = Rig::new();
    let path = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("preview-not.wav");
    std::fs::write(&path, b"not audio").unwrap();
    rig.send(Command::PreviewLoad { path });
    let events = rig.engine.wait_for_loads();
    assert!(
        matches!(events.as_slice(), [Event::PreviewFailed { .. }]),
        "{events:?}"
    );
    assert!(rig.engine.snapshot().preview.track.is_none());
    // Without a track the commands do nothing.
    rig.send(Command::PreviewPlayPause);
    rig.send(Command::PreviewSeek { position: 0.5 });
    assert!(!rig.engine.snapshot().preview.playing);
}
