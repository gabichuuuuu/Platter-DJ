//! Flux and reverse. Behaviour note: `docs/behavior/flux.md`.

// The helpers that write the test tracks run outside `#[test]` functions.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::PathBuf;

use common::{RATE, Rig, sine_file};
use platter_core::{DeckId, HotcueSlot};
use platter_engine::{Command, DeckSnapshot};
use platter_testkit::files::wav_pcm16;
use platter_testkit::measure::max_step;

const A: DeckId = DeckId::A;
const B: DeckId = DeckId::B;
/// A few frames, in seconds: what a position may be off by after blocks and fades.
const NEAR: f64 = 1e-3;

/// A stereo track whose samples rise steadily from -0.9 to 0.9 over `seconds`, so each sample
/// says where in the track it was read.
fn ramp_file(name: &str, seconds: f64, file_rate: u32) -> PathBuf {
    let frames = (seconds * f64::from(file_rate)).round() as usize;
    let samples: Vec<i16> = (0..frames)
        .flat_map(|frame| {
            let value = -0.9 + 1.8 * frame as f64 / frames as f64;
            let sample = (value * f64::from(i16::MAX)).round() as i16;
            [sample, sample]
        })
        .collect();
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.wav"));
    std::fs::write(&path, wav_pcm16(file_rate, 2, &samples)).unwrap();
    path
}

/// An eight-second tone on deck A with a grid at 120 BPM: a beat is half a second.
fn rig(name: &str) -> Rig {
    let mut rig = Rig::with_deck_a(sine_file(name, 440.0, 8.0, 0.5, RATE));
    rig.send(Command::DeckGridBpmSet {
        deck: A,
        bpm: 120.0,
    });
    rig
}

/// Lets the processor apply what was sent and returns a deck. A playing deck gets an empty
/// block, so no time passes; a paused one gets a single frame.
fn deck_of(rig: &mut Rig, id: DeckId) -> DeckSnapshot {
    let frames = usize::from(!rig.engine.snapshot().decks[id.index()].playing);
    rig.processor.process(&mut [0.0; 2][..frames * 2], 2);
    rig.engine.poll();
    rig.engine.snapshot().decks[id.index()].clone()
}

fn deck(rig: &mut Rig) -> DeckSnapshot {
    deck_of(rig, A)
}

fn position(rig: &mut Rig) -> f64 {
    deck(rig).position_seconds
}

fn reverse(rig: &mut Rig, pressed: bool) {
    rig.send(Command::DeckReverse { deck: A, pressed });
}

fn hotcue(rig: &mut Rig, slot: usize, pressed: bool) {
    rig.send(Command::DeckHotcueTrigger {
        deck: A,
        slot: HotcueSlot::ALL[slot],
        pressed,
    });
}

fn assert_near(found: f64, expected: f64) {
    assert!((found - expected).abs() < NEAR, "{found} is not {expected}");
}

#[test]
fn a_loop_with_flux_returns_to_where_the_track_would_be() {
    let mut rig = rig("flux-loop");
    rig.send(Command::DeckFluxToggle { deck: A });
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.25);
    rig.send(Command::DeckLoopToggle { deck: A });
    rig.run(3.0);
    let looping = deck(&mut rig);
    assert!(looping.looping.region.is_some());
    assert!(
        looping.position_seconds < 2.5,
        "{}",
        looping.position_seconds
    );
    assert_near(looping.timeline_seconds.unwrap(), 3.25);
    rig.send(Command::DeckLoopToggle { deck: A });
    let back = deck(&mut rig);
    assert_near(back.position_seconds, 3.25);
    assert_eq!(back.timeline_seconds, None);
}

#[test]
fn without_flux_a_loop_switched_off_stays_where_it_is() {
    let mut rig = rig("flux-off-loop");
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.25);
    rig.send(Command::DeckLoopToggle { deck: A });
    rig.run(3.0);
    let inside = position(&mut rig);
    rig.send(Command::DeckLoopToggle { deck: A });
    assert_near(position(&mut rig), inside);
    assert_eq!(deck(&mut rig).timeline_seconds, None);
}

#[test]
fn a_deck_back_from_reverse_is_where_and_sounds_as_one_that_played_on() {
    let mut straight = rig("flux-straight");
    straight.send(Command::DeckPlay { deck: A });
    straight.run(2.0);

    let mut detour = rig("flux-detour");
    detour.send(Command::DeckPlay { deck: A });
    detour.run(0.5);
    reverse(&mut detour, true);
    detour.run(0.5);
    assert_near(position(&mut detour), 0.0);
    reverse(&mut detour, false);
    detour.run(1.0);

    assert_near(position(&mut detour), position(&mut straight));
    // Past the crossfade of the return, both play the same frames.
    straight.run(0.01);
    detour.run(0.01);
    let (heard, expected) = (detour.run(0.05), straight.run(0.05));
    let apart = heard
        .iter()
        .zip(&expected)
        .map(|(left, right)| (left - right).abs())
        .fold(0.0f32, f32::max);
    assert!(apart < 1e-6, "{apart}");
}

#[test]
fn reverse_reads_the_track_backwards_and_turns_without_a_click() {
    let mut rig = Rig::with_deck_a(ramp_file("flux-ramp", 2.0, RATE));
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 1.5,
    });
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.1);
    reverse(&mut rig, true);
    let backwards = rig.run(0.2);
    assert_near(position(&mut rig), 1.4);
    // After the 5 ms crossfade each millisecond is lower than the one before, by the slope
    // of the ramp read backwards: 0.9 per second. Single samples are compared by the
    // millisecond because the ramp rises less than one step of 16 bits per frame.
    let after = &backwards[480..];
    let means: Vec<f32> = after
        .as_chunks::<48>()
        .0
        .iter()
        .map(|chunk| chunk.iter().sum::<f32>() / 48.0)
        .collect();
    let rises: Vec<_> = means
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[1] >= pair[0])
        .collect();
    assert!(rises.is_empty(), "{rises:?}");
    let fall = f64::from(after[0] - after[after.len() - 1]);
    assert!((fall - 0.9 * 0.19).abs() < 0.005, "{fall}");
    // The ramp moves 0.00002 per frame; a click would be a step far larger.
    assert!(max_step(&backwards) < 0.001, "{}", max_step(&backwards));
    // The release goes back to the timeline, 0.3 s on: a gap of 0.27 in the ramp, spread
    // by the 5 ms crossfade of a jump over 240 frames, about 0.0011 a frame. Cut without
    // it, the step would be the whole gap.
    reverse(&mut rig, false);
    let forward = rig.run(0.1);
    assert!(max_step(&forward) < 0.003, "{}", max_step(&forward));
    assert_near(position(&mut rig), 1.9);
}

#[test]
fn reverse_inside_a_loop_wraps_to_its_end_without_a_click() {
    let mut rig = rig("flux-reverse-loop");
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 1.0,
    });
    rig.send(Command::DeckPlay { deck: A });
    rig.send(Command::DeckLoopToggle { deck: A });
    let (start, end) = deck(&mut rig)
        .looping
        .region
        .map(|region| (region.start, region.end))
        .unwrap();
    rig.run(0.1);
    reverse(&mut rig, true);
    let passes = rig.run(end - start + 0.5);
    let at = position(&mut rig);
    assert!((start..end).contains(&at), "{at} outside {start}..{end}");
    // The slope of the tone is 0.029 per frame; the crossfade adds at most 1/240 of it.
    assert!(max_step(&passes) < 0.04, "{}", max_step(&passes));
}

#[test]
fn reverse_stays_silent_at_the_start_and_returns_to_the_timeline() {
    let mut rig = rig("flux-start");
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 0.2,
    });
    rig.send(Command::DeckPlay { deck: A });
    reverse(&mut rig, true);
    let heard = rig.run(0.5);
    assert_eq!(position(&mut rig), 0.0);
    let loud = heard[heard.len() - 9600..]
        .iter()
        .fold(0.0f32, |most, sample| most.max(sample.abs()));
    // What the filters of the channel still ring with, below -180 dB.
    assert!(loud < 1e-9, "{loud}");
    reverse(&mut rig, false);
    assert_near(position(&mut rig), 0.7);
}

#[test]
fn a_hotcue_held_with_flux_returns_on_release_and_without_flux_stays() {
    for flux in [true, false] {
        let mut rig = rig(if flux {
            "flux-hotcue"
        } else {
            "flux-hotcue-off"
        });
        rig.send(Command::DeckSeek {
            deck: A,
            seconds: 0.5,
        });
        hotcue(&mut rig, 0, true);
        hotcue(&mut rig, 0, false);
        rig.send(Command::DeckSeek {
            deck: A,
            seconds: 2.0,
        });
        if flux {
            rig.send(Command::DeckFluxToggle { deck: A });
        }
        rig.send(Command::DeckPlay { deck: A });
        rig.run(0.1);
        hotcue(&mut rig, 0, true);
        assert_near(position(&mut rig), 0.5);
        rig.run(1.0);
        hotcue(&mut rig, 0, false);
        let expected = if flux { 3.1 } else { 1.5 };
        assert_near(position(&mut rig), expected);
    }
}

#[test]
fn a_seek_or_flux_switched_off_ends_the_timeline_without_moving() {
    let mut rig = rig("flux-end");
    rig.send(Command::DeckFluxToggle { deck: A });
    rig.send(Command::DeckPlay { deck: A });
    rig.send(Command::DeckLoopToggle { deck: A });
    rig.run(1.0);
    assert!(deck(&mut rig).timeline_seconds.is_some());
    rig.send(Command::DeckFluxToggle { deck: A });
    assert_eq!(deck(&mut rig).timeline_seconds, None);
    let inside = position(&mut rig);
    rig.send(Command::DeckLoopToggle { deck: A });
    assert_near(position(&mut rig), inside);

    rig.send(Command::DeckFluxToggle { deck: A });
    rig.send(Command::DeckLoopToggle { deck: A });
    rig.run(1.0);
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 6.0,
    });
    assert_eq!(deck(&mut rig).timeline_seconds, None);
}

#[test]
fn a_timeline_past_the_end_brings_the_deck_to_the_end() {
    let mut rig = rig("flux-end-of-track");
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 7.5,
    });
    rig.send(Command::DeckPlay { deck: A });
    reverse(&mut rig, true);
    rig.run(1.0);
    assert_near(deck(&mut rig).timeline_seconds.unwrap(), 8.0);
    reverse(&mut rig, false);
    rig.run(0.01);
    let end = deck(&mut rig);
    assert!(!end.playing);
    assert_near(end.position_seconds, 8.0);
}

#[test]
fn a_reversed_tempo_master_keeps_its_follower_on_the_beat() {
    let mut rig = rig("flux-master-a");
    rig.load(B, sine_file("flux-master-b", 330.0, 8.0, 0.5, RATE));
    rig.send(Command::DeckGridBpmSet {
        deck: B,
        bpm: 120.0,
    });
    rig.send(Command::DeckSyncToggle { deck: B });
    rig.send(Command::DeckPlay { deck: A });
    rig.send(Command::DeckPlay { deck: B });
    rig.run(0.5);
    reverse(&mut rig, true);
    for _ in 0..4 {
        rig.run(0.25);
        let phase = deck_of(&mut rig, B).phase.unwrap();
        assert!(phase.abs() < 0.01, "{phase}");
    }
    reverse(&mut rig, false);
    rig.run(0.25);
    for id in [A, B] {
        let phase = deck_of(&mut rig, id).phase.unwrap();
        assert!(phase.abs() < 0.01, "{id:?}: {phase}");
    }
}

#[test]
fn reverse_on_a_44_1_khz_track_counts_the_time_of_the_track() {
    let mut rig = Rig::with_deck_a(sine_file("flux-44k", 440.0, 4.0, 0.5, 44_100));
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 2.0,
    });
    rig.send(Command::DeckPlay { deck: A });
    reverse(&mut rig, true);
    rig.run(1.0);
    assert_near(position(&mut rig), 1.0);
    assert_near(deck(&mut rig).timeline_seconds.unwrap(), 3.0);
    reverse(&mut rig, false);
    assert_near(position(&mut rig), 3.0);
}

#[test]
fn flux_stays_with_the_deck_for_the_next_track_and_shows_in_the_snapshot() {
    let mut rig = rig("flux-setting");
    rig.send(Command::DeckFluxToggle { deck: A });
    reverse(&mut rig, true);
    let held = deck(&mut rig);
    assert!(held.flux && held.reverse);
    reverse(&mut rig, false);
    rig.load(A, sine_file("flux-setting-next", 440.0, 2.0, 0.5, RATE));
    let next = deck(&mut rig);
    assert!(next.flux && !next.reverse);
    assert!(!deck_of(&mut rig, B).flux);
}
