//! Stored cues and hotcues. Behaviour notes: `docs/behavior/cues.md`.

mod common;

use common::{RATE, Rig, sine_file};
use platter_core::{CueKind, Cues, DeckId, HotcueSlot};
use platter_engine::command::{Args, Value};
use platter_engine::{Command, CueStep, DeckSnapshot, SizeStep};
use platter_testkit::measure::max_step;

const A: DeckId = DeckId::A;
/// One frame of the engine, in seconds.
const FRAME: f64 = 1.0 / RATE as f64;

/// A four-second track on deck A with a grid at 120 BPM: a beat is half a second.
fn rig(name: &str) -> Rig {
    let mut rig = bare(name);
    rig.send(Command::DeckGridBpmSet {
        deck: A,
        bpm: 120.0,
    });
    rig
}

/// The same track without a grid.
fn bare(name: &str) -> Rig {
    Rig::with_deck_a(sine_file(name, 440.0, 4.0, 0.5, RATE))
}

/// Lets the processor apply what was sent and returns deck A. A playing deck gets an empty
/// block, so no time passes; a paused one gets a single frame, which is when it takes a
/// seek that waits.
fn deck(rig: &mut Rig) -> DeckSnapshot {
    let frames = usize::from(!rig.engine.snapshot().decks[0].playing);
    rig.processor.process(&mut [0.0; 2][..frames * 2], 2);
    rig.engine.poll();
    rig.engine.snapshot().decks[0].clone()
}

fn cues(rig: &mut Rig) -> Cues {
    deck(rig).cues
}

fn slot(number: u8) -> HotcueSlot {
    HotcueSlot::ALL[usize::from(number) - 1]
}

fn key(rig: &mut Rig, number: u8, pressed: bool) {
    rig.send(Command::DeckHotcueTrigger {
        deck: A,
        slot: slot(number),
        pressed,
    });
}

/// A press and a release of the key of a slot.
fn tap(rig: &mut Rig, number: u8) {
    key(rig, number, true);
    key(rig, number, false);
}

fn seek(rig: &mut Rig, seconds: f64) {
    rig.send(Command::DeckSeek { deck: A, seconds });
}

/// Stores a cue at `seconds` in an empty slot of a paused deck.
fn store_at(rig: &mut Rig, number: u8, seconds: f64) {
    seek(rig, seconds);
    tap(rig, number);
}

fn select(rig: &mut Rig, direction: CueStep) {
    rig.send(Command::DeckCueSelect { deck: A, direction });
}

fn retype(rig: &mut Rig, kind: CueKind) {
    rig.send(Command::DeckCueTypeSet { deck: A, kind });
}

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() < 1e-9
}

/// The position of the cue in a slot, when it holds one.
fn stored(cues: &Cues, number: u8) -> Option<f64> {
    cues.get(slot(number)).map(|cue| cue.position)
}

fn region(cues: &Cues, number: u8) -> Option<(f64, f64)> {
    let region = cues.get(slot(number))?.region()?;
    Some((region.start, region.end))
}

#[test]
fn a_press_on_an_empty_slot_stores_where_the_seek_before_it_landed() {
    let mut rig = bare("cue-store-press");
    seek(&mut rig, 1.25);
    tap(&mut rig, 3);
    let cues = cues(&mut rig);
    assert!(stored(&cues, 3).is_some_and(|at| close(at, 1.25)));
    assert_eq!(cues.selected(), Some(slot(3)));
    assert_eq!(cues.get(slot(3)).unwrap().kind(), CueKind::Cue);
    assert!(cues.get(slot(3)).unwrap().name.is_empty());
    assert_eq!(cues.iter().count(), 1);
    // The release after a store holds nothing: the deck stays where it was, paused.
    let deck = deck(&mut rig);
    assert!(!deck.playing);
    assert!(close(deck.position_seconds, 1.25));
}

#[test]
fn store_type_and_rename_in_a_row_act_on_the_cue_just_stored() {
    let mut rig = bare("cue-store-row");
    seek(&mut rig, 1.5);
    rig.send(Command::DeckCueSet { deck: A });
    seek(&mut rig, 3.0);
    tap(&mut rig, 1);
    // No poll between these: the slot the store takes is known to the processor alone.
    rig.send(Command::DeckCueStore { deck: A });
    retype(&mut rig, CueKind::FadeOut);
    rig.send(Command::DeckCueRename {
        deck: A,
        name: "Break".to_owned(),
    });
    let cues = cues(&mut rig);
    // The store takes the floating cue, not the play position, and the first empty slot.
    let cue = cues.get(slot(2)).unwrap();
    assert!(close(cue.position, 1.5));
    assert_eq!(cue.kind(), CueKind::FadeOut);
    assert_eq!(cue.name.as_str(), "Break");
    assert_eq!(cues.selected(), Some(slot(2)));
    let first = cues.get(slot(1)).unwrap();
    assert!(close(first.position, 3.0));
    assert_eq!(first.kind(), CueKind::Cue);
}

#[test]
fn a_press_on_a_stored_slot_of_a_playing_deck_jumps_and_plays_on() {
    let mut rig = bare("cue-jump-playing");
    store_at(&mut rig, 1, 2.0);
    seek(&mut rig, 0.0);
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.5);
    key(&mut rig, 1, true);
    let deck_now = deck(&mut rig);
    assert!(deck_now.playing);
    assert!(close(deck_now.position_seconds, 2.0));
    // The floating cue of a playing deck stays where it was.
    assert!(close(deck_now.cue_seconds, 0.0));
    let around = rig.run(0.25);
    // The slope of the tone is 0.029 per frame; a cut would step by up to its amplitude.
    assert!(max_step(&around) < 0.04, "{}", max_step(&around));
    key(&mut rig, 1, false);
    let deck_now = deck(&mut rig);
    assert!(
        deck_now.playing,
        "the release holds nothing on a playing deck"
    );
    assert!((deck_now.position_seconds - 2.25).abs() < 2.0 * FRAME);
}

#[test]
fn a_paused_deck_plays_from_the_cue_while_the_key_is_held() {
    let mut rig = bare("cue-hold");
    store_at(&mut rig, 1, 1.0);
    seek(&mut rig, 3.0);
    assert!(close(deck(&mut rig).position_seconds, 3.0));
    key(&mut rig, 1, true);
    let held = deck(&mut rig);
    assert!(held.playing);
    assert!(close(held.position_seconds, 1.0));
    assert!(close(held.cue_seconds, 1.0), "the floating cue goes there");
    let heard = rig.run(0.25);
    assert!(heard.iter().any(|sample| sample.abs() > 0.4));
    assert!((deck(&mut rig).position_seconds - 1.25).abs() < 2.0 * FRAME);
    key(&mut rig, 1, false);
    rig.run(0.05);
    let released = deck(&mut rig);
    assert!(!released.playing);
    assert!(close(released.position_seconds, 1.0));
}

#[test]
fn play_during_the_hold_keeps_the_deck_playing() {
    let mut rig = bare("cue-hold-play");
    store_at(&mut rig, 1, 1.0);
    key(&mut rig, 1, true);
    rig.run(0.1);
    rig.send(Command::DeckPlayPause { deck: A });
    key(&mut rig, 1, false);
    rig.run(0.1);
    let deck = deck(&mut rig);
    assert!(deck.playing);
    assert!((deck.position_seconds - 1.2).abs() < 2.0 * FRAME);
}

#[test]
fn a_press_while_a_loop_is_on_stores_the_loop_and_a_later_press_brings_it_back() {
    let mut rig = rig("cue-stored-loop");
    seek(&mut rig, 1.0);
    rig.send(Command::DeckLoopToggle { deck: A });
    tap(&mut rig, 2);
    let stored_cues = cues(&mut rig);
    // Four beats at 120 BPM.
    assert!(
        region(&stored_cues, 2).is_some_and(|(start, end)| close(start, 1.0) && close(end, 3.0))
    );
    assert_eq!(stored_cues.get(slot(2)).unwrap().kind(), CueKind::Loop);
    rig.send(Command::DeckLoopToggle { deck: A });
    seek(&mut rig, 0.0);
    assert_eq!(deck(&mut rig).looping.region, None);
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.25);
    key(&mut rig, 2, true);
    let back = deck(&mut rig);
    let looping = back.looping.region.unwrap();
    assert!(close(looping.start, 1.0) && close(looping.end, 3.0));
    assert!(close(back.position_seconds, 1.0));
    // Two and a half seconds later the deck is half a second into its second pass.
    rig.run(2.5);
    let position = deck(&mut rig).position_seconds;
    assert!((position - 1.5).abs() < 2.0 * FRAME, "{position}");
}

#[test]
fn the_store_command_stores_the_loop_that_is_on() {
    let mut rig = rig("cue-store-loop");
    seek(&mut rig, 0.5);
    rig.send(Command::DeckLoopToggle { deck: A });
    rig.send(Command::DeckCueStore { deck: A });
    let cues = cues(&mut rig);
    assert!(region(&cues, 1).is_some_and(|(start, end)| close(start, 0.5) && close(end, 2.5)));
}

#[test]
fn previous_and_next_go_through_the_cues_in_the_order_of_the_track() {
    let mut rig = bare("cue-select");
    store_at(&mut rig, 1, 1.0);
    store_at(&mut rig, 2, 3.0);
    store_at(&mut rig, 3, 2.0);
    seek(&mut rig, 0.0);
    for (direction, position, selected) in [
        (CueStep::Next, 1.0, 1),
        (CueStep::Next, 2.0, 3),
        (CueStep::Next, 3.0, 2),
        // Nothing after the last one.
        (CueStep::Next, 3.0, 2),
        (CueStep::Previous, 2.0, 3),
        (CueStep::Previous, 1.0, 1),
        (CueStep::Previous, 1.0, 1),
    ] {
        select(&mut rig, direction);
        let deck = deck(&mut rig);
        assert!(!deck.playing);
        assert!(
            close(deck.position_seconds, position),
            "{} is not {position}",
            deck.position_seconds
        );
        assert_eq!(deck.cues.selected(), Some(slot(selected)));
    }
}

#[test]
fn two_selects_in_a_row_count_from_where_the_first_one_landed() {
    let mut rig = bare("cue-select-row");
    store_at(&mut rig, 1, 1.0);
    store_at(&mut rig, 2, 2.0);
    seek(&mut rig, 0.0);
    select(&mut rig, CueStep::Next);
    select(&mut rig, CueStep::Next);
    let deck = deck(&mut rig);
    assert!(close(deck.position_seconds, 2.0));
    assert_eq!(deck.cues.selected(), Some(slot(2)));
}

#[test]
fn a_cue_becomes_a_loop_of_the_selected_size_when_the_grid_and_the_track_allow() {
    let mut rig = bare("cue-retype");
    store_at(&mut rig, 1, 1.0);
    retype(&mut rig, CueKind::Loop);
    assert_eq!(
        cues(&mut rig).get(slot(1)).unwrap().kind(),
        CueKind::Cue,
        "no tempo, no loop"
    );
    rig.send(Command::DeckGridBpmSet {
        deck: A,
        bpm: 120.0,
    });
    retype(&mut rig, CueKind::Loop);
    let looped = cues(&mut rig);
    assert!(region(&looped, 1).is_some_and(|(start, end)| close(start, 1.0) && close(end, 3.0)));
    assert_eq!(deck(&mut rig).looping.region, None, "the deck gets no loop");
    // Another size does not change a cue that is a loop already.
    rig.send(Command::DeckLoopSizeStep {
        deck: A,
        direction: SizeStep::Down,
    });
    retype(&mut rig, CueKind::Loop);
    assert_eq!(cues(&mut rig), looped);
    retype(&mut rig, CueKind::Load);
    let plain = cues(&mut rig);
    assert_eq!(plain.get(slot(1)).unwrap().kind(), CueKind::Load);
    assert_eq!(region(&plain, 1), None);
    // Two beats from 3.5 s would end after the track.
    store_at(&mut rig, 2, 3.5);
    retype(&mut rig, CueKind::Loop);
    assert_eq!(cues(&mut rig).get(slot(2)).unwrap().kind(), CueKind::Cue);
}

#[test]
fn a_long_name_is_cut_and_an_empty_one_removes_it() {
    let mut rig = bare("cue-rename");
    store_at(&mut rig, 1, 1.0);
    rig.send(Command::DeckCueRename {
        deck: A,
        name: "x".repeat(200),
    });
    assert_eq!(
        cues(&mut rig).get(slot(1)).unwrap().name.as_str(),
        "x".repeat(48)
    );
    rig.send(Command::DeckCueRename {
        deck: A,
        name: String::new(),
    });
    assert!(cues(&mut rig).get(slot(1)).unwrap().name.is_empty());
}

#[test]
fn deleting_empties_the_slot_and_edits_without_a_selection_do_nothing() {
    let mut rig = bare("cue-delete");
    store_at(&mut rig, 1, 1.0);
    store_at(&mut rig, 2, 2.0);
    rig.send(Command::DeckCueDelete { deck: A });
    let after = cues(&mut rig);
    assert_eq!(stored(&after, 2), None);
    assert_eq!(after.selected(), None);
    rig.send(Command::DeckCueDelete { deck: A });
    retype(&mut rig, CueKind::Load);
    rig.send(Command::DeckCueRename {
        deck: A,
        name: "Nobody".to_owned(),
    });
    rig.send(Command::DeckHotcueMap {
        deck: A,
        slot: slot(7),
    });
    assert_eq!(cues(&mut rig), after);
    rig.send(Command::DeckHotcueDelete {
        deck: A,
        slot: slot(1),
    });
    assert_eq!(cues(&mut rig), Cues::default());
}

#[test]
fn mapping_moves_the_selected_cue_and_swaps_with_a_taken_slot() {
    let mut rig = bare("cue-map");
    store_at(&mut rig, 1, 1.0);
    store_at(&mut rig, 2, 2.0);
    rig.send(Command::DeckHotcueMap {
        deck: A,
        slot: slot(5),
    });
    let moved = cues(&mut rig);
    assert_eq!(stored(&moved, 2), None);
    assert!(stored(&moved, 5).is_some_and(|at| close(at, 2.0)));
    assert_eq!(moved.selected(), Some(slot(5)));
    rig.send(Command::DeckHotcueMap {
        deck: A,
        slot: slot(1),
    });
    let swapped = cues(&mut rig);
    assert!(stored(&swapped, 1).is_some_and(|at| close(at, 2.0)));
    assert!(stored(&swapped, 5).is_some_and(|at| close(at, 1.0)));
    assert_eq!(swapped.selected(), Some(slot(1)));
}

#[test]
fn a_store_with_the_eight_slots_taken_does_nothing() {
    let mut rig = bare("cue-full");
    for number in 1..=8 {
        store_at(&mut rig, number, f64::from(number) * 0.25);
    }
    let full = cues(&mut rig);
    assert_eq!(full.iter().count(), 8);
    rig.send(Command::DeckCueStore { deck: A });
    assert_eq!(cues(&mut rig), full);
}

#[test]
fn a_new_track_has_no_cues_and_takes_the_ones_stored_while_it_arrives() {
    let mut rig = bare("cue-swap-old");
    store_at(&mut rig, 1, 1.0);
    store_at(&mut rig, 2, 2.0);
    assert_eq!(cues(&mut rig).iter().count(), 2);
    rig.load(A, sine_file("cue-swap-new", 220.0, 2.0, 0.5, 44_100));
    // Sent before the processor has taken the new track in.
    store_at(&mut rig, 4, 0.5);
    let cues = cues(&mut rig);
    assert_eq!(cues.iter().count(), 1);
    assert!(stored(&cues, 4).is_some_and(|at| close(at, 0.5)));
    rig.send(Command::DeckEject { deck: A });
    assert_eq!(self::cues(&mut rig), Cues::default());
}

#[test]
fn a_track_of_another_sample_rate_keeps_its_cues_in_seconds() {
    let mut rig = Rig::with_deck_a(sine_file("cue-other-rate", 440.0, 4.0, 0.5, 44_100));
    store_at(&mut rig, 1, 2.5);
    seek(&mut rig, 0.0);
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.25);
    key(&mut rig, 1, true);
    let deck = deck(&mut rig);
    assert!(
        close(deck.position_seconds, 2.5),
        "{}",
        deck.position_seconds
    );
    assert!(stored(&deck.cues, 1).is_some_and(|at| close(at, 2.5)));
}

#[test]
fn an_empty_deck_stores_nothing() {
    let mut rig = Rig::new();
    tap(&mut rig, 1);
    rig.send(Command::DeckCueStore { deck: A });
    select(&mut rig, CueStep::Next);
    let deck = deck(&mut rig);
    assert_eq!(deck.cues, Cues::default());
    assert!(!deck.playing);
}

#[test]
fn a_slot_outside_one_to_eight_is_rejected() {
    for bad in [0.0, 9.0, 2.5, f64::NAN] {
        let mut args = Args::new();
        args.insert("deck".to_owned(), Value::Text("a".to_owned()));
        args.insert("slot".to_owned(), Value::Number(bad));
        assert!(
            Command::from_args("deck.hotcue.delete", &args).is_err(),
            "{bad}"
        );
    }
}
