//! The processor neither allocates nor frees memory, whatever it is asked to do.
//!
//! This file holds a single test because it installs its own global allocator; the count
//! is per thread, so the worker threads that decode do not disturb it.

mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::{RATE, Rig, sine_file};
use platter_core::{CueKind, DeckId, HotcueSlot, Stem};
use platter_engine::rt::RECORDER_QUEUE_SECONDS;
use platter_engine::{
    Band, BpmScale, ClockMode, Command, CrossfaderCurve, CrossfaderDirection, CrossfaderSide,
    CueStep, Direction, EffectKind, EqModel, Event, FilterModel, FxRouting, FxUnit, InputSource,
    InputTarget, LoopSource, MixerMode, MoveMode, Output, SizeStep, SyncMode, UnitMode,
};
use platter_testkit::alloc::{CountingAllocator, count};

const WAIT: Duration = Duration::from_secs(10);

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn commands() -> Vec<Command> {
    use DeckId::{A, B, C, D};
    let one = FxUnit::ALL[0];
    let two = FxUnit::ALL[1];
    vec![
        // The mix recorder takes the master through all of it, with a cut halfway.
        Command::RecorderToggle,
        // Decks C and D play beside A and B, C with key lock and D in sync.
        Command::DeckKeyLockToggle { deck: C },
        Command::DeckPlay { deck: C },
        Command::DeckSyncToggle { deck: D },
        Command::DeckPlay { deck: D },
        // Key lock on both decks, so what follows runs through the stretch: its jumps,
        // loops, sync and tempo changes, from a 48 kHz and a 44.1 kHz track.
        Command::DeckKeyLockToggle { deck: A },
        Command::DeckKeyLockToggle { deck: B },
        // The beat tick of both decks ticks in the headphones through all of it.
        Command::DeckBeatTickToggle { deck: A },
        Command::DeckBeatTickToggle { deck: B },
        // Autogain off, a restore and autogain on again: each sets the gain of a channel.
        Command::MixerAutogainToggle,
        Command::MixerGainRestore { deck: A },
        Command::MixerAutogainToggle,
        // A change of each model fades the outgoing one out: both run for a while.
        Command::MixerEqModelSet {
            model: EqModel::Steep,
        },
        Command::MixerFilterModelSet {
            model: FilterModel::Ladder,
        },
        Command::MixerFilterSet {
            deck: B,
            position: -0.4,
        },
        // External mode runs the routed path beside the mix, which always runs.
        Command::MixerModeSet {
            mode: MixerMode::External,
        },
        Command::MixerExternalEqGainToggle,
        Command::MixerExternalMasterVolumeToggle,
        Command::MixerCrossfaderAssign {
            deck: A,
            side: CrossfaderSide::Thru,
        },
        // An automatic fade runs through all of it.
        Command::MixerCrossfaderAutoFade {
            direction: CrossfaderDirection::Right,
        },
        // Effects on decks A and B, built on this side and swapped while the decks play.
        Command::MixerFxAssignToggle { deck: A, unit: one },
        Command::MixerFxAssignToggle { deck: B, unit: two },
        Command::FxSlotEffectSet {
            unit: one,
            slot: 0,
            effect: Some(EffectKind::Delay),
        },
        Command::FxSlotToggle { unit: one, slot: 0 },
        Command::FxSlotEffectSet {
            unit: one,
            slot: 1,
            effect: Some(EffectKind::Slicer),
        },
        Command::FxSlotToggle { unit: one, slot: 1 },
        Command::FxUnitModeSet {
            unit: two,
            mode: UnitMode::Single,
        },
        Command::FxSingleEffectSet {
            unit: two,
            effect: Some(EffectKind::Reverb),
        },
        Command::FxSingleToggle { unit: two },
        Command::FxUnitRoutingSet {
            unit: two,
            routing: FxRouting::PostFader,
        },
        Command::MixerEffectSelect {
            deck: A,
            effect: EffectKind::Gater,
        },
        Command::MixerEffectSet {
            deck: A,
            position: 0.6,
        },
        Command::RecorderCut,
        // A swap while the effect before it still fades out.
        Command::FxSlotEffectSet {
            unit: one,
            slot: 0,
            effect: Some(EffectKind::Flanger),
        },
        Command::DeckPlay { deck: A },
        Command::DeckPlay { deck: B },
        Command::DeckTempoSet {
            deck: A,
            ratio: 1.08,
        },
        Command::DeckNudge {
            deck: B,
            amount: -0.04,
        },
        Command::DeckCueSet { deck: A },
        Command::DeckSeek {
            deck: A,
            seconds: 1.0,
        },
        Command::DeckPlayPause { deck: A },
        Command::DeckPlayPause { deck: A },
        Command::DeckCueGoto { deck: A },
        Command::MixerGainSet { deck: A, db: 6.0 },
        Command::MixerEqSet {
            deck: B,
            band: Band::Mid,
            position: 0.0,
        },
        Command::MixerEqSet {
            deck: B,
            band: Band::High,
            position: 1.0,
        },
        Command::MixerFilterSet {
            deck: A,
            position: -0.7,
        },
        Command::MixerFilterSet {
            deck: B,
            position: 0.7,
        },
        Command::MixerFaderSet {
            deck: A,
            position: 0.3,
        },
        Command::MixerCrossfaderSet { position: 0.2 },
        Command::MixerCrossfaderCurveSet {
            curve: CrossfaderCurve::Sharp,
        },
        Command::MasterGainSet { db: 12.0 },
        // The maximiser switched in over the hot mix and changed while it limits; it stays
        // on, with the headphones kept in time with it, through the rest.
        Command::MasterLimiterToggle,
        Command::MasterMaximiserDriveSet { db: 12.0 },
        Command::MasterMaximiserCeilingSet { db: -3.0 },
        Command::MasterMaximiserReleaseSet { ms: 20.0 },
        Command::MixerEqKillToggle {
            deck: A,
            band: Band::Low,
        },
        Command::MixerPanSet {
            deck: B,
            position: -0.6,
        },
        Command::MixerCueToggle { deck: B },
        Command::MixerCueMixSet { position: 0.4 },
        Command::MixerCueVolumeSet { db: -6.0 },
        Command::MasterTestTone {
            output: Output::Headphones,
        },
        Command::MasterTestTone {
            output: Output::Master,
        },
        Command::DeckTempoRangeCycle { deck: A },
        Command::DeckCue {
            deck: A,
            pressed: true,
        },
        Command::DeckCue {
            deck: A,
            pressed: true,
        },
        Command::DeckCue {
            deck: A,
            pressed: false,
        },
        Command::DeckCup {
            deck: B,
            pressed: true,
        },
        Command::DeckCup {
            deck: B,
            pressed: false,
        },
        Command::DeckGridBpmSet {
            deck: A,
            bpm: 128.0,
        },
        Command::DeckGridMarkerSet { deck: A },
        Command::DeckGridBpmScale {
            deck: A,
            factor: BpmScale::Half,
        },
        Command::DeckGridBpmStep {
            deck: A,
            amount: 0.5,
        },
        Command::DeckGridMarkerMove {
            deck: A,
            seconds: -0.1,
        },
        Command::DeckGridMarkerPrevious { deck: A },
        Command::DeckGridMarkerNext { deck: A },
        Command::DeckGridMarkerDelete { deck: A },
        Command::DeckGridLockToggle { deck: A },
        Command::DeckGridReset { deck: A },
        Command::DeckGridBpmReset { deck: A },
        Command::DeckGridTap { deck: B },
        Command::DeckPlay { deck: A },
        Command::DeckGridBpmSet {
            deck: A,
            bpm: 400.0,
        },
        // A loop of 1/32 of a beat at 400 BPM is 225 frames: it returns to its start
        // twice in each block of 512.
        Command::DeckLoopSizeStep {
            deck: A,
            direction: SizeStep::Down,
        },
        Command::DeckLoopSizeStep {
            deck: A,
            direction: SizeStep::Down,
        },
        Command::DeckLoopSizeStep {
            deck: A,
            direction: SizeStep::Down,
        },
        Command::DeckLoopSizeStep {
            deck: A,
            direction: SizeStep::Down,
        },
        Command::DeckLoopSizeStep {
            deck: A,
            direction: SizeStep::Down,
        },
        Command::DeckLoopSizeStep {
            deck: A,
            direction: SizeStep::Down,
        },
        Command::DeckLoopSizeStep {
            deck: A,
            direction: SizeStep::Down,
        },
        Command::DeckLoopToggle { deck: A },
        Command::DeckLoopSizeStep {
            deck: A,
            direction: SizeStep::Up,
        },
        Command::DeckBeatjump {
            deck: A,
            direction: Direction::Back,
        },
        Command::DeckMoveModeSet {
            deck: A,
            mode: MoveMode::Loop,
        },
        Command::DeckMove {
            deck: A,
            direction: Direction::Forward,
        },
        Command::DeckMoveFineToggle { deck: A },
        Command::DeckMoveModeSet {
            deck: A,
            mode: MoveMode::LoopOut,
        },
        Command::DeckMove {
            deck: A,
            direction: Direction::Back,
        },
        Command::DeckMoveSizeStep {
            deck: A,
            direction: SizeStep::Up,
        },
        Command::DeckLoopToggle { deck: A },
        Command::DeckLoopIn { deck: A },
        Command::DeckLoopOut { deck: A },
        Command::DeckLoopToggle { deck: A },
        Command::DeckLoopToggle { deck: A },
        hotcue(1, true),
        hotcue(1, false),
        Command::DeckLoopToggle { deck: A },
        hotcue(2, true),
        Command::DeckCueStore { deck: A },
        Command::DeckCueTypeSet {
            deck: A,
            kind: CueKind::Loop,
        },
        Command::DeckCueRename {
            deck: A,
            name: "A name longer than the forty-eight bytes a cue keeps".to_owned(),
        },
        Command::DeckHotcueMap {
            deck: A,
            slot: HotcueSlot::ALL[7],
        },
        hotcue(1, true),
        hotcue(1, false),
        Command::DeckCueSelect {
            deck: A,
            direction: CueStep::Next,
        },
        Command::DeckCueSelect {
            deck: A,
            direction: CueStep::Previous,
        },
        Command::DeckCueDelete { deck: A },
        Command::DeckHotcueDelete {
            deck: A,
            slot: HotcueSlot::ALL[0],
        },
        // Sync: deck B follows deck A and moves onto its beat, then follows the clock. Both
        // decks start again near their beginning, so they play through what follows.
        Command::DeckSeek {
            deck: A,
            seconds: 0.2,
        },
        Command::DeckPlay { deck: A },
        Command::DeckGridBpmSet {
            deck: A,
            bpm: 120.0,
        },
        Command::DeckPlay { deck: B },
        Command::DeckGridBpmSet {
            deck: B,
            bpm: 100.0,
        },
        Command::DeckSyncToggle { deck: B },
        Command::DeckSeek {
            deck: B,
            seconds: 1.13,
        },
        Command::GlobalSyncModeSet {
            mode: SyncMode::Tempo,
        },
        Command::GlobalSyncModeSet {
            mode: SyncMode::Beat,
        },
        Command::ClockModeSet {
            mode: ClockMode::Master,
        },
        Command::ClockTempoSet { bpm: 128.0 },
        Command::ClockTempoStep { amount: -0.5 },
        Command::ClockBend { amount: 0.1 },
        Command::ClockBend { amount: 0.0 },
        Command::ClockTap,
        Command::ClockMetronomeToggle,
        // The MIDI clock, whose queue nobody reads here: it fills and the rest is dropped.
        Command::ClockMidiSendToggle,
        Command::ClockMidiRestart,
        Command::ClockSourceSet { external: true },
        Command::ClockSourceSet { external: false },
        Command::RecorderSourceSet { external: true },
        Command::RecorderSourceSet { external: false },
        Command::DeckMasterSet { deck: B },
        Command::DeckSyncToggle { deck: A },
        Command::ClockModeSet {
            mode: ClockMode::Auto,
        },
        Command::DeckSyncToggle { deck: B },
        // Snap and Quantize: a cue and a loop on the beat, a quantized jump to a hotcue, on
        // a deck that plays from near its beginning.
        Command::DeckSeek {
            deck: A,
            seconds: 0.2,
        },
        Command::DeckPlay { deck: A },
        Command::GlobalSnapToggle,
        Command::GlobalQuantizeToggle,
        Command::DeckCueSet { deck: A },
        hotcue(3, true),
        hotcue(3, false),
        Command::DeckLoopToggle { deck: A },
        hotcue(4, true),
        hotcue(4, false),
        Command::DeckLoopToggle { deck: A },
        hotcue(3, true),
        hotcue(3, false),
        hotcue(4, true),
        hotcue(4, false),
        // Flux: a loop, a hotcue and reverse as detours, and reverse inside the loop.
        Command::DeckFluxToggle { deck: A },
        Command::DeckLoopToggle { deck: A },
        Command::DeckReverse {
            deck: A,
            pressed: true,
        },
        Command::DeckReverse {
            deck: A,
            pressed: false,
        },
        Command::DeckLoopToggle { deck: A },
        hotcue(3, true),
        hotcue(3, false),
        Command::DeckReverse {
            deck: B,
            pressed: true,
        },
        Command::DeckReverse {
            deck: B,
            pressed: false,
        },
        Command::DeckFluxToggle { deck: A },
        // Transposition: a step, a set and a persist while both decks play.
        Command::DeckKeyStep {
            deck: A,
            direction: SizeStep::Up,
        },
        Command::DeckKeySet {
            deck: B,
            semitones: -2.5,
        },
        Command::DeckKeyPersist { deck: A },
        Command::DeckSyncToggle { deck: B },
        Command::DeckEject { deck: B },
        Command::DeckKeyLockToggle { deck: A },
        Command::DeckPause { deck: A },
        Command::RecorderToggle,
    ]
}

/// Frequency of a sine from its rising zero crossings.
fn frequency(samples: &[f32]) -> f64 {
    let rising: Vec<usize> = (1..samples.len())
        .filter(|&i| samples[i - 1] < 0.0 && samples[i] >= 0.0)
        .collect();
    let span = (rising[rising.len() - 1] - rising[0]) as f64;
    (rising.len() - 1) as f64 * f64::from(RATE) / span
}

/// A deck that plays the input and the auxiliary input, then the deck back on its track.
fn input_commands() -> Vec<Command> {
    vec![
        Command::InputRoutingSet {
            target: InputTarget::Deck(DeckId::A),
            source: InputSource::parse("1-2"),
        },
        Command::DeckTypeSet {
            deck: DeckId::A,
            live: true,
        },
        Command::InputRoutingSet {
            target: InputTarget::Aux,
            source: InputSource::parse("1"),
        },
        Command::MixerAuxSet { level: 0.5 },
        // The recorders take the input: the mix recorder and a loop of it.
        Command::InputRoutingSet {
            target: InputTarget::Recorder,
            source: InputSource::parse("3-4"),
        },
        Command::RecorderSourceSet { external: true },
        Command::LoopRecorderSourceSet {
            source: LoopSource::External,
        },
        Command::DeckTypeSet {
            deck: DeckId::A,
            live: false,
        },
    ]
}

fn hotcue(number: usize, pressed: bool) -> Command {
    Command::DeckHotcueTrigger {
        deck: DeckId::A,
        slot: HotcueSlot::ALL[number - 1],
        pressed,
    }
}

#[test]
fn process_neither_allocates_nor_frees() {
    // First prove the count works: without the allocator installed it would always be zero.
    let (_, seen) = count(|| drop(std::hint::black_box(vec![0u8; 64])));
    assert!(seen >= 2, "the counting allocator is not active");

    let mut rig = Rig::new();
    let recordings = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("memory-recordings");
    let _ = std::fs::remove_dir_all(&recordings);
    // While `held` is on, the worker of a recording waits before it takes anything.
    let held = Arc::new(AtomicBool::new(false));
    let waiting = Arc::clone(&held);
    let stem = recordings.join("take");
    rig.engine.set_recording_names(Arc::new(move || {
        while waiting.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(1));
        }
        stem.clone()
    }));
    rig.load(DeckId::A, sine_file("memory-a", 440.0, 3.0, 0.5, RATE));
    rig.load(DeckId::B, sine_file("memory-b", 660.0, 3.0, 0.5, 44_100));
    rig.load(
        DeckId::C,
        sine_file("memory-lower-c", 550.0, 3.0, 0.25, RATE),
    );
    rig.load(
        DeckId::D,
        sine_file("memory-lower-d", 770.0, 3.0, 0.25, 44_100),
    );
    // Four channels, so the headphone path runs too.
    let mut block = vec![0.0f32; 512 * 4];
    let mut events = 0;
    // The input device delivers a block of two inputs before each block of the output, so
    // the reader of the input runs through all of it.
    let mut feed = rig
        .engine
        .take_input_feed()
        .expect("the feed of a new engine");
    let input: Vec<f32> = (0..512 * 2)
        .map(|i| ((i / 2) as f32 * 0.05).sin() * 0.3)
        .collect();
    let mut run = |rig: &mut Rig, blocks: usize| {
        for _ in 0..blocks {
            feed.push(&input, 2);
            let (_, counted) = count(|| rig.processor.process(&mut block, 4));
            events += counted;
        }
    };

    for command in commands().into_iter().chain(input_commands()) {
        rig.send(command);
        run(&mut rig, 5);
    }
    let recorded = rig.engine.finish_recording(WAIT);
    assert_eq!(
        recorded
            .iter()
            .filter(|event| matches!(event, Event::Recorded { frames, .. } if *frames > 0))
            .count(),
        2,
        "{recorded:?}"
    );
    // A recording whose worker cannot take anything yet: the queue fills up and the frames
    // that find no room are dropped, not waited for.
    held.store(true, Ordering::Relaxed);
    rig.send(Command::RecorderToggle);
    let full = RECORDER_QUEUE_SECONDS as f64 + 0.5;
    run(&mut rig, (full * f64::from(RATE) / 512.0).ceil() as usize);
    assert!(rig.engine.snapshot().recorder.dropped_frames > 0);
    held.store(false, Ordering::Relaxed);
    rig.send(Command::RecorderToggle);
    run(&mut rig, 2);
    rig.engine.finish_recording(WAIT);
    // The loop recorder: a pass of a bar, an overdub, undo and redo, an overdub that copies
    // the loop while an undo waits for the copy, then delete, each with its fades.
    rig.send(Command::MixerModeSet {
        mode: MixerMode::Internal,
    });
    rig.send(Command::DeckPlay { deck: DeckId::A });
    rig.send(Command::LoopRecorderSizeSet { bars: 1.0 });
    rig.send(Command::LoopRecorderSourceSet {
        source: LoopSource::Deck(DeckId::A),
    });
    rig.send(Command::LoopRecorderRecord);
    run(&mut rig, 260);
    for (command, blocks) in [
        (Command::LoopRecorderRecord, 100),
        (Command::LoopRecorderRecord, 5),
        (Command::LoopRecorderUndo, 10),
        (Command::LoopRecorderRedo, 10),
        (Command::LoopRecorderRecord, 1),
        (Command::LoopRecorderUndo, 60),
        (Command::LoopRecorderPlay, 5),
        (Command::LoopRecorderPlay, 5),
        (Command::LoopRecorderDryWetSet { amount: 0.8 }, 5),
        (Command::LoopRecorderDelete, 5),
    ] {
        rig.send(command);
        run(&mut rig, blocks);
    }
    rig.send(Command::DeckPause { deck: DeckId::A });
    // A stem deck: its four stems arrive with their stretches, play through key lock with
    // their controls moving, and go back when a plain track replaces them while it plays.
    let stems =
        Stem::ALL.map(|stem| sine_file(&format!("memory-stem-{stem}"), 330.0, 2.0, 0.2, RATE));
    rig.send(Command::DeckLoadStems {
        deck: DeckId::B,
        path: sine_file("memory-stem-track", 330.0, 2.0, 0.5, RATE),
        stems: Box::new(stems),
    });
    rig.engine.wait_for_loads();
    rig.send(Command::DeckPlay { deck: DeckId::B });
    rig.send(Command::DeckKeyLockToggle { deck: DeckId::B });
    rig.send(Command::DeckTempoSet {
        deck: DeckId::B,
        ratio: 1.05,
    });
    run(&mut rig, 20);
    for stem in Stem::ALL {
        rig.send(Command::DeckStemFilterToggle {
            deck: DeckId::B,
            stem,
        });
        rig.send(Command::DeckStemSendSet {
            deck: DeckId::B,
            stem,
            amount: 0.3,
        });
        rig.send(Command::DeckStemSendToggle {
            deck: DeckId::B,
            stem,
        });
        rig.send(Command::DeckStemMuteToggle {
            deck: DeckId::B,
            stem,
        });
        run(&mut rig, 3);
    }
    rig.send(Command::DeckBeatjump {
        deck: DeckId::B,
        direction: Direction::Forward,
    });
    run(&mut rig, 10);
    rig.load(
        DeckId::B,
        sine_file("memory-after-stems", 220.0, 1.0, 0.5, RATE),
    );
    run(&mut rig, 10);
    rig.send(Command::DeckEject { deck: DeckId::B });
    run(&mut rig, 5);
    // Replacing a track while it plays is the case where a buffer changes hands: the old
    // one must travel back to this thread instead of being freed by the processor.
    rig.send(Command::DeckPlay { deck: DeckId::A });
    run(&mut rig, 5);
    rig.load(DeckId::A, sine_file("memory-c", 220.0, 1.0, 0.5, RATE));
    rig.send(Command::DeckPlay { deck: DeckId::A });
    // Long enough to run past the end of the new track.
    run(&mut rig, 120);
    rig.send(Command::DeckEject { deck: DeckId::A });
    run(&mut rig, 5);

    // The preview player takes a track, plays it, seeks, pauses and takes another: its old
    // audio too goes back through the retired queue.
    for (name, seconds) in [("memory-preview-a", 1.0), ("memory-preview-b", 0.2)] {
        rig.send(Command::PreviewLoad {
            path: sine_file(name, 330.0, seconds, 0.5, 44_100),
        });
        rig.engine.wait_for_loads();
        run(&mut rig, 10);
        rig.send(Command::PreviewSeek { position: 0.5 });
        run(&mut rig, 5);
        rig.send(Command::PreviewPlayPause);
        run(&mut rig, 5);
        rig.send(Command::PreviewPlayPause);
        // Long enough to run past the end of the short track.
        run(&mut rig, 30);
    }

    // Key lock engaged for long enough to hold, with no jump: its pitch shows that the
    // stretch played what was counted.
    rig.load(DeckId::A, sine_file("memory-d", 440.0, 3.0, 0.5, RATE));
    rig.send(Command::DeckKeyLockToggle { deck: DeckId::A });
    rig.send(Command::DeckTempoSet {
        deck: DeckId::A,
        ratio: 1.08,
    });
    rig.send(Command::DeckPlay { deck: DeckId::A });
    let mut heard = Vec::new();
    for _ in 0..60 {
        let (_, counted) = count(|| rig.processor.process(&mut block, 4));
        events += counted;
        heard.extend(block.iter().step_by(4).copied());
    }
    let hz = frequency(&heard[RATE as usize / 10..]);
    assert!(
        (1200.0 * (hz / 440.0).log2()).abs() < 5.0,
        "key lock did not hold: {hz:.2} Hz"
    );
    // Transposition: a change while the stretch plays, then one while paused, which the
    // stretch applies before its seek. Their pitch shows the stretch played them.
    let mut transposed = |rig: &mut Rig, semitones: f64| {
        let mut heard = Vec::new();
        for _ in 0..60 {
            let (_, counted) = count(|| rig.processor.process(&mut block, 4));
            events += counted;
            heard.extend(block.iter().step_by(4).copied());
        }
        let hz = frequency(&heard[RATE as usize / 2..]);
        let expected = 440.0 * (semitones / 12.0).exp2();
        assert!(
            (1200.0 * (hz / expected).log2()).abs() < 5.0,
            "the shift of {semitones} did not hold: {hz:.2} Hz"
        );
    };
    for _ in 0..2 {
        rig.send(Command::DeckKeyStep {
            deck: DeckId::A,
            direction: SizeStep::Up,
        });
    }
    transposed(&mut rig, 2.0);
    rig.send(Command::DeckPause { deck: DeckId::A });
    rig.send(Command::DeckKeySet {
        deck: DeckId::A,
        semitones: -2.0,
    });
    rig.send(Command::DeckPlay { deck: DeckId::A });
    transposed(&mut rig, -2.0);
    // The track and its stretch go back to this thread too.
    rig.send(Command::DeckEject { deck: DeckId::A });
    for _ in 0..5 {
        let (_, counted) = count(|| rig.processor.process(&mut block, 4));
        events += counted;
    }

    // Key lock on worker threads, run between the blocks: the audio thread only takes what the
    // workers rendered, through a seek and a change of tempo. Its pitch shows it was stretched.
    rig.engine.set_key_lock_pumped();
    rig.load(DeckId::A, sine_file("memory-e", 440.0, 4.0, 0.5, RATE));
    // The shift of the key the deck kept from above is taken back.
    rig.send(Command::DeckKeySet {
        deck: DeckId::A,
        semitones: 0.0,
    });
    rig.send(Command::DeckPlay { deck: DeckId::A });
    let mut pumped = |rig: &mut Rig, blocks: usize| {
        let mut heard = Vec::new();
        for _ in 0..blocks {
            let (_, counted) = count(|| rig.processor.process(&mut block, 4));
            events += counted;
            rig.engine.pump_key_lock();
            heard.extend(block.iter().step_by(4).copied());
        }
        heard
    };
    pumped(&mut rig, 40);
    rig.send(Command::DeckTempoSet {
        deck: DeckId::A,
        ratio: 1.04,
    });
    let heard = pumped(&mut rig, 80);
    let hz = frequency(&heard[heard.len() / 2..]);
    assert!(
        (1200.0 * (hz / 440.0).log2()).abs() < 5.0,
        "key lock on worker threads did not hold: {hz:.2} Hz"
    );
    assert_eq!(rig.engine.key_lock_starved(), 0);
    rig.send(Command::DeckEject { deck: DeckId::A });
    pumped(&mut rig, 5);

    assert_eq!(
        events, 0,
        "process allocated or freed memory {events} times"
    );
    // The retired audio is freed here, on the control side.
    let (_, freed) = count(|| {
        rig.engine.poll();
    });
    assert!(freed >= 1, "nothing was freed on the control side");
    std::fs::remove_dir_all(&recordings).unwrap();
}
