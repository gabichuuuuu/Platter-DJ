//! Loops, beatjumps and the moves of the MOVE page. Behaviour notes: `docs/behavior/loops.md`.

mod common;

use common::{RATE, Rig, rate, sine_file};
use platter_core::{DeckId, LoopSize};
use platter_engine::{
    Command, DeckSnapshot, Direction, Engine, FINE_MOVE_SECONDS, MoveMode, MoveSize, SizeStep,
};
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

fn region(rig: &mut Rig) -> Option<(f64, f64)> {
    deck(rig)
        .looping
        .region
        .map(|region| (region.start, region.end))
}

fn position(rig: &mut Rig) -> f64 {
    deck(rig).position_seconds
}

fn seek(rig: &mut Rig, seconds: f64) {
    rig.send(Command::DeckSeek { deck: A, seconds });
}

fn step_loop_size(rig: &mut Rig, direction: SizeStep, times: usize) {
    for _ in 0..times {
        rig.send(Command::DeckLoopSizeStep { deck: A, direction });
    }
}

fn jump(rig: &mut Rig, direction: Direction) {
    rig.send(Command::DeckBeatjump { deck: A, direction });
}

fn move_by(rig: &mut Rig, mode: MoveMode, direction: Direction) {
    rig.send(Command::DeckMoveModeSet { deck: A, mode });
    rig.send(Command::DeckMove { deck: A, direction });
}

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() < 1e-9
}

fn assert_region(rig: &mut Rig, start: f64, end: f64) {
    let region = region(rig);
    assert!(
        region.is_some_and(|(from, to)| close(from, start) && close(to, end)),
        "{region:?} is not {start}..{end}"
    );
}

#[test]
fn a_loop_starts_where_the_seek_before_it_landed_and_toggles_off() {
    let mut rig = rig("loop-toggle");
    seek(&mut rig, 1.0);
    rig.send(Command::DeckLoopToggle { deck: A });
    // Four beats at 120 BPM.
    assert_region(&mut rig, 1.0, 3.0);
    rig.send(Command::DeckLoopToggle { deck: A });
    assert_eq!(region(&mut rig), None);
}

#[test]
fn every_pass_of_a_loop_lasts_its_length() {
    let mut rig = rig("loop-passes");
    // At this speed the deck never lands on the end of the loop: every return has a
    // fraction of a frame to carry over.
    rig.send(Command::DeckTempoSet {
        deck: A,
        ratio: 1.03,
    });
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.5);
    step_loop_size(&mut rig, SizeStep::Down, 7);
    rig.send(Command::DeckLoopToggle { deck: A });
    let (start, end) = region(&mut rig).unwrap();
    assert!(close(end - start, 0.5 / 32.0));
    // Ten seconds are 10.3 s of the track: 659 passes of the loop and a fifth of one.
    rig.run(10.0);
    let position = position(&mut rig);
    assert!(
        (position - (start + 0.2 * (end - start))).abs() < 2.0 * FRAME,
        "{position} from {start}"
    );
    assert!(deck(&mut rig).playing);
}

#[test]
fn the_return_of_a_loop_does_not_click() {
    // 125 BPM puts 211.2 periods of the 440 Hz tone in a beat, so the tone is a fifth of
    // a period off at each return: a step of up to 0.59 without the crossfade.
    let mut rig = bare("loop-click");
    rig.send(Command::DeckGridBpmSet {
        deck: A,
        bpm: 125.0,
    });
    step_loop_size(&mut rig, SizeStep::Down, 2);
    seek(&mut rig, 1.0);
    rig.send(Command::DeckLoopToggle { deck: A });
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.1);
    let passes = rig.run(2.0);
    // The slope of the tone is 0.029 per frame; the crossfade adds at most 1/240 of it.
    assert!(max_step(&passes) < 0.04, "{}", max_step(&passes));
}

#[test]
fn a_loop_that_ends_with_the_track_goes_on_instead_of_stopping() {
    let mut rig = rig("loop-at-end");
    step_loop_size(&mut rig, SizeStep::Down, 1);
    seek(&mut rig, 3.0);
    rig.send(Command::DeckLoopToggle { deck: A });
    assert_region(&mut rig, 3.0, 4.0);
    rig.send(Command::DeckPlay { deck: A });
    rig.run(2.5);
    assert!(deck(&mut rig).playing);
    let position = position(&mut rig);
    assert!((position - 3.5).abs() < 2.0 * FRAME, "{position}");
    // One second later the loop would end after the track: it is not started.
    rig.send(Command::DeckLoopToggle { deck: A });
    seek(&mut rig, 3.5);
    rig.send(Command::DeckLoopToggle { deck: A });
    assert_eq!(region(&mut rig), None);
}

#[test]
fn in_and_out_make_a_loop_while_the_deck_plays() {
    let mut rig = bare("loop-in-out");
    rig.send(Command::DeckLoopOut { deck: A });
    assert_eq!(region(&mut rig), None, "no in point yet");
    rig.send(Command::DeckPlay { deck: A });
    rig.run(1.0);
    rig.send(Command::DeckLoopIn { deck: A });
    let waiting = deck(&mut rig).looping;
    assert!(
        waiting
            .in_point
            .is_some_and(|at| (at - 1.0).abs() < 2.0 * FRAME)
    );
    assert_eq!(waiting.region, None);
    rig.run(0.3);
    rig.send(Command::DeckLoopOut { deck: A });
    let (start, end) = region(&mut rig).unwrap();
    assert!((start - 1.0).abs() < 2.0 * FRAME && close(end - start, 0.3));
    assert_eq!(deck(&mut rig).looping.in_point, None);
    // The deck went back to the in point when the loop closed.
    rig.run(0.7);
    let position = position(&mut rig);
    assert!((position - (start + 0.1)).abs() < 2.0 * FRAME, "{position}");
    // Out inside the loop shortens it; In switches it off and waits again.
    rig.send(Command::DeckLoopOut { deck: A });
    let (_, shorter) = region(&mut rig).unwrap();
    assert!((shorter - (start + 0.1)).abs() < 2.0 * FRAME);
    rig.send(Command::DeckLoopIn { deck: A });
    assert_eq!(region(&mut rig), None);
}

#[test]
fn a_new_size_reaches_the_loop_that_is_on() {
    let mut rig = rig("loop-resize");
    assert_eq!(deck(&mut rig).controls.loop_size, LoopSize::FOUR_BEATS);
    seek(&mut rig, 1.0);
    rig.send(Command::DeckLoopToggle { deck: A });
    seek(&mut rig, 2.8);
    step_loop_size(&mut rig, SizeStep::Down, 2);
    assert_region(&mut rig, 1.0, 1.5);
    // The deck was 1.8 s into the loop: three laps of the new half second and 0.3 more.
    assert!(close(position(&mut rig), 1.3));
    assert_eq!(deck(&mut rig).controls.loop_size, LoopSize::ONE_BEAT);
    // Eight beats do not fit before the end of the four-second track: the selection moves
    // on and the loop stays at four.
    step_loop_size(&mut rig, SizeStep::Up, 3);
    assert_region(&mut rig, 1.0, 3.0);
    assert_eq!(deck(&mut rig).controls.loop_size.fraction(), (8, 1));
    step_loop_size(&mut rig, SizeStep::Up, 9);
    assert_eq!(deck(&mut rig).controls.loop_size, LoopSize::LARGEST);
    step_loop_size(&mut rig, SizeStep::Down, 20);
    assert_eq!(deck(&mut rig).controls.loop_size, LoopSize::SMALLEST);
    assert_region(&mut rig, 1.0, 1.0 + 0.5 / 32.0);
}

#[test]
fn a_beatjump_keeps_the_time_while_the_deck_plays() {
    let mut rig = rig("jump-playing");
    rig.send(Command::DeckPlay { deck: A });
    rig.run(1.0);
    jump(&mut rig, Direction::Forward);
    rig.run(0.25);
    let position = position(&mut rig);
    assert!((position - 1.75).abs() < 2.0 * FRAME, "{position}");
    jump(&mut rig, Direction::Back);
    jump(&mut rig, Direction::Back);
    rig.run(0.25);
    let position = self::position(&mut rig);
    assert!((position - 1.0).abs() < 2.0 * FRAME, "{position}");
}

#[test]
fn a_beatjump_does_not_click() {
    // A beat at 125 BPM is not a whole number of periods of the tone.
    let mut rig = bare("jump-click");
    rig.send(Command::DeckGridBpmSet {
        deck: A,
        bpm: 125.0,
    });
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.5);
    jump(&mut rig, Direction::Forward);
    let around = rig.run(0.2);
    assert!(max_step(&around) < 0.04, "{}", max_step(&around));
}

#[test]
fn jumps_follow_the_seek_before_them_and_stay_in_the_track() {
    let mut rig = rig("jump-settled");
    seek(&mut rig, 2.0);
    jump(&mut rig, Direction::Forward);
    jump(&mut rig, Direction::Forward);
    assert!(close(position(&mut rig), 3.0));
    rig.send(Command::DeckMoveSizeStep {
        deck: A,
        direction: SizeStep::Up,
    });
    rig.send(Command::DeckMoveSizeStep {
        deck: A,
        direction: SizeStep::Up,
    });
    jump(&mut rig, Direction::Forward);
    assert!(
        close(position(&mut rig), 3.0),
        "four beats would leave the track"
    );
    jump(&mut rig, Direction::Back);
    assert!(close(position(&mut rig), 1.0));
    jump(&mut rig, Direction::Back);
    assert!(
        close(position(&mut rig), 1.0),
        "and so would four beats back"
    );
}

#[test]
fn the_move_size_goes_from_the_presets_to_the_length_of_the_loop() {
    let mut rig = rig("move-size");
    let step = |rig: &mut Rig, direction, times| {
        for _ in 0..times {
            rig.send(Command::DeckMoveSizeStep { deck: A, direction });
        }
    };
    assert_eq!(
        deck(&mut rig).controls.move_size,
        MoveSize::Beats(LoopSize::ONE_BEAT)
    );
    step(&mut rig, SizeStep::Up, 5);
    assert_eq!(
        deck(&mut rig).controls.move_size,
        MoveSize::Beats(LoopSize::LARGEST)
    );
    step(&mut rig, SizeStep::Up, 3);
    assert_eq!(deck(&mut rig).controls.move_size, MoveSize::Loop);
    // Without a loop the step is the selected loop size, four beats.
    jump(&mut rig, Direction::Forward);
    assert!(close(position(&mut rig), 2.0));
    // With one it is its length, here the 0.3 s between In and Out.
    seek(&mut rig, 1.0);
    rig.send(Command::DeckLoopIn { deck: A });
    seek(&mut rig, 1.3);
    rig.send(Command::DeckLoopOut { deck: A });
    assert!(close(position(&mut rig), 1.0));
    move_by(&mut rig, MoveMode::Loop, Direction::Forward);
    assert_region(&mut rig, 1.3, 1.6);
    assert!(close(position(&mut rig), 1.3));
    step(&mut rig, SizeStep::Down, 20);
    assert_eq!(
        deck(&mut rig).controls.move_size,
        MoveSize::Beats(LoopSize::SMALLEST)
    );
}

#[test]
fn the_moves_act_on_what_the_mode_says() {
    let mut rig = rig("move-modes");
    seek(&mut rig, 1.0);
    rig.send(Command::DeckLoopToggle { deck: A });
    // The default mode is the beatjump: the loop stays and the deck leaves it.
    rig.send(Command::DeckMove {
        deck: A,
        direction: Direction::Back,
    });
    assert_region(&mut rig, 1.0, 3.0);
    assert!(close(position(&mut rig), 0.5));
    move_by(&mut rig, MoveMode::Loop, Direction::Forward);
    assert_region(&mut rig, 1.5, 3.5);
    assert!(
        close(position(&mut rig), 0.5),
        "the deck was not inside the loop"
    );
    move_by(&mut rig, MoveMode::LoopIn, Direction::Forward);
    assert_region(&mut rig, 2.0, 3.5);
    move_by(&mut rig, MoveMode::LoopOut, Direction::Forward);
    assert_region(&mut rig, 2.0, 4.0);
    rig.send(Command::DeckMove {
        deck: A,
        direction: Direction::Forward,
    });
    assert_region(&mut rig, 2.0, 4.0);
    assert_eq!(deck(&mut rig).controls.move_mode, MoveMode::LoopOut);
    // The end moves back behind the deck, which returns into the loop.
    seek(&mut rig, 3.75);
    move_by(&mut rig, MoveMode::LoopOut, Direction::Back);
    assert_region(&mut rig, 2.0, 3.5);
    assert!(close(position(&mut rig), 2.25));
    // Without a loop the loop modes move nothing.
    rig.send(Command::DeckLoopToggle { deck: A });
    move_by(&mut rig, MoveMode::Loop, Direction::Forward);
    assert_eq!(region(&mut rig), None);
    assert!(close(position(&mut rig), 2.25));
}

#[test]
fn the_fine_steps_are_five_milliseconds_and_need_no_tempo() {
    let mut rig = bare("move-fine");
    seek(&mut rig, 1.0);
    jump(&mut rig, Direction::Forward);
    rig.send(Command::DeckLoopToggle { deck: A });
    assert!(close(position(&mut rig), 1.0), "no tempo, no beats");
    assert_eq!(region(&mut rig), None);
    rig.send(Command::DeckMoveFineToggle { deck: A });
    assert!(deck(&mut rig).controls.move_fine);
    jump(&mut rig, Direction::Forward);
    assert!(close(position(&mut rig), 1.0 + FINE_MOVE_SECONDS));
    rig.send(Command::DeckLoopIn { deck: A });
    seek(&mut rig, 2.0);
    rig.send(Command::DeckLoopOut { deck: A });
    move_by(&mut rig, MoveMode::LoopOut, Direction::Back);
    assert_region(&mut rig, 1.005, 1.995);
    rig.send(Command::DeckMoveFineToggle { deck: A });
    assert!(!deck(&mut rig).controls.move_fine);
}

#[test]
fn a_deck_that_seeks_past_its_loop_plays_on() {
    let mut rig = rig("loop-left");
    step_loop_size(&mut rig, SizeStep::Down, 2);
    seek(&mut rig, 1.0);
    rig.send(Command::DeckLoopToggle { deck: A });
    seek(&mut rig, 2.0);
    rig.send(Command::DeckPlay { deck: A });
    rig.run(1.0);
    // The seek fades out and in, so the second is not exact; the loop did not catch it.
    assert!(position(&mut rig) > 2.9);
    assert_region(&mut rig, 1.0, 1.5);
    // From before the loop the deck plays into it and stays.
    seek(&mut rig, 0.8);
    rig.run(1.5);
    let position = position(&mut rig);
    assert!((1.0..1.5).contains(&position), "{position}");
}

#[test]
fn a_new_track_has_no_loop_and_keeps_the_selections() {
    let mut rig = rig("loop-swap-old");
    step_loop_size(&mut rig, SizeStep::Down, 1);
    rig.send(Command::DeckMoveModeSet {
        deck: A,
        mode: MoveMode::LoopIn,
    });
    rig.send(Command::DeckMoveFineToggle { deck: A });
    rig.send(Command::DeckLoopToggle { deck: A });
    assert_region(&mut rig, 0.0, 1.0);

    rig.load(A, sine_file("loop-swap-new", 220.0, 2.0, 0.5, 44_100));
    // Sent before the processor has taken the new track: it belongs to the new one.
    rig.send(Command::DeckLoopIn { deck: A });
    let deck = deck(&mut rig);
    assert_eq!(deck.looping.region, None);
    assert_eq!(deck.looping.in_point, Some(0.0));
    assert_eq!(deck.controls.loop_size.fraction(), (2, 1));
    assert_eq!(deck.controls.move_mode, MoveMode::LoopIn);
    assert!(deck.controls.move_fine);

    rig.send(Command::DeckEject { deck: A });
    assert_eq!(self::deck(&mut rig).looping.in_point, None);
}

#[test]
fn the_selections_survive_a_change_of_sample_rate() {
    let mut rig = Rig::new();
    step_loop_size(&mut rig, SizeStep::Up, 1);
    rig.send(Command::DeckMoveModeSet {
        deck: A,
        mode: MoveMode::LoopOut,
    });
    rig.send(Command::DeckMoveSizeStep {
        deck: A,
        direction: SizeStep::Down,
    });
    rig.send(Command::DeckMoveFineToggle { deck: A });
    let before = rig.engine.snapshot().decks[0].controls.clone();
    let (engine, _processor) = Engine::with_controls_of(rate(), &rig.engine);
    assert_eq!(engine.snapshot().decks[0].controls, before);
    assert_eq!(before.loop_size.fraction(), (8, 1));
}

#[test]
fn an_empty_deck_has_nothing_to_loop_or_jump_in() {
    let mut rig = Rig::new();
    rig.send(Command::DeckLoopIn { deck: A });
    rig.send(Command::DeckLoopOut { deck: A });
    rig.send(Command::DeckLoopToggle { deck: A });
    jump(&mut rig, Direction::Forward);
    move_by(&mut rig, MoveMode::Loop, Direction::Back);
    let deck = deck(&mut rig);
    assert_eq!(deck.looping, Default::default());
    assert_eq!(deck.position_seconds, 0.0);
    assert_eq!(deck.controls.move_mode, MoveMode::Loop);
}
