//! Automatic mixing with the fade cues: a deck crossing a fade-out cue starts its partner from
//! its fade-in cue. Behaviour note: `docs/behavior/auto-mix.md`.

#![allow(clippy::unwrap_used)]

mod common;

use common::{RATE, Rig, sine_file};
use platter_core::{CueKind, DeckId, HotcueSlot};
use platter_engine::{Command, CrossfaderSide};

const A: DeckId = DeckId::A;
const B: DeckId = DeckId::B;

/// Applies what was sent: a frame for the paused decks to take a seek, then a poll.
fn settle(rig: &mut Rig) {
    rig.processor.process(&mut [0.0; 2], 2);
    rig.engine.poll();
}

/// Stores a cue of `kind` at `seconds` in slot 1 of a paused deck.
fn store(rig: &mut Rig, deck: DeckId, seconds: f64, kind: CueKind) {
    rig.send(Command::DeckSeek { deck, seconds });
    settle(rig);
    for pressed in [true, false] {
        rig.send(Command::DeckHotcueTrigger {
            deck,
            slot: HotcueSlot::ALL[0],
            pressed,
        });
    }
    settle(rig);
    rig.send(Command::DeckCueTypeSet { deck, kind });
    settle(rig);
    let cues = rig.engine.snapshot().decks[deck.index()].cues;
    assert_eq!(cues.get(HotcueSlot::ALL[0]).unwrap().kind(), kind);
}

/// Decks A and B with a track each, A on the left of the crossfader and B on the right, A with
/// a fade-out cue at one second and B with a fade-in cue at half a second, both paused at 0.
fn two_decks(name: &str, b_kind: CueKind) -> Rig {
    let mut rig = Rig::new();
    rig.load(A, sine_file(&format!("{name}-a"), 440.0, 4.0, 0.2, RATE));
    rig.load(B, sine_file(&format!("{name}-b"), 660.0, 4.0, 0.2, RATE));
    for (deck, side) in [(A, CrossfaderSide::Left), (B, CrossfaderSide::Right)] {
        rig.send(Command::MixerCrossfaderAssign { deck, side });
    }
    store(&mut rig, A, 1.0, CueKind::FadeOut);
    store(&mut rig, B, 0.5, b_kind);
    for deck in [A, B] {
        rig.send(Command::DeckSeek { deck, seconds: 0.0 });
    }
    settle(&mut rig);
    rig
}

/// Plays `seconds` in steps of a tenth of a second, polling after each, as the window does.
fn play(rig: &mut Rig, seconds: f64) {
    for _ in 0..(seconds * 10.0).round() as usize {
        rig.run_stereo(0.1);
        rig.engine.poll();
    }
}

#[test]
fn crossing_a_fade_out_cue_starts_the_partner_from_its_fade_in_cue_and_fades_to_it() {
    let mut rig = two_decks("auto-mix-start", CueKind::FadeIn);
    rig.send(Command::GlobalFadeMarkersToggle);
    assert!(rig.engine.snapshot().fade_markers);
    rig.send(Command::DeckPlay { deck: A });
    play(&mut rig, 0.8);
    assert!(!rig.engine.snapshot().decks[B.index()].playing);

    play(&mut rig, 0.4);
    let snapshot = rig.engine.snapshot();
    let b = &snapshot.decks[B.index()];
    assert!(b.playing);
    // B started from 0.5 s at most a tenth of a second after A crossed 1 s, and A went on.
    assert!(
        (0.5..=0.8).contains(&b.position_seconds),
        "{}",
        b.position_seconds
    );
    assert!(snapshot.decks[A.index()].playing);
    assert!(
        snapshot.mixer.crossfader > 0.5,
        "{}",
        snapshot.mixer.crossfader
    );
}

#[test]
fn nothing_starts_without_the_preference_without_a_fade_in_cue_or_on_a_jump() {
    // The preference is off to begin with.
    let mut rig = two_decks("auto-mix-off", CueKind::FadeIn);
    assert!(!rig.engine.snapshot().fade_markers);
    rig.send(Command::DeckPlay { deck: A });
    play(&mut rig, 1.5);
    assert!(!rig.engine.snapshot().decks[B.index()].playing);

    // B has a cue, but not a fade-in one.
    let mut rig = two_decks("auto-mix-no-fade-in", CueKind::Cue);
    rig.send(Command::GlobalFadeMarkersToggle);
    rig.send(Command::DeckPlay { deck: A });
    play(&mut rig, 1.5);
    assert!(!rig.engine.snapshot().decks[B.index()].playing);
    assert_eq!(rig.engine.snapshot().mixer.crossfader, 0.5);

    // A seek over the fade-out cue is not playback crossing it.
    let mut rig = two_decks("auto-mix-jump", CueKind::FadeIn);
    rig.send(Command::GlobalFadeMarkersToggle);
    rig.send(Command::DeckPlay { deck: A });
    play(&mut rig, 0.3);
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 2.5,
    });
    play(&mut rig, 0.5);
    assert!(!rig.engine.snapshot().decks[B.index()].playing);
}

#[test]
fn a_partner_already_playing_is_left_alone() {
    let mut rig = two_decks("auto-mix-playing", CueKind::FadeIn);
    rig.send(Command::GlobalFadeMarkersToggle);
    rig.send(Command::DeckPlay { deck: B });
    rig.send(Command::DeckPlay { deck: A });
    play(&mut rig, 1.5);
    let snapshot = rig.engine.snapshot();
    // B plays on from where it was, not from its fade-in cue.
    assert!(snapshot.decks[B.index()].position_seconds > 1.4);
    assert_eq!(snapshot.mixer.crossfader, 0.5);
}
