//! The processor neither allocates nor frees memory, whatever it is asked to do.
//!
//! This file holds a single test because it installs its own global allocator; the count
//! is per thread, so the worker threads that decode do not disturb it.

mod common;

use common::{RATE, Rig, sine_file};
use platter_core::{CueKind, DeckId, HotcueSlot};
use platter_engine::{
    Band, BpmScale, ClockMode, Command, CrossfaderCurve, CueStep, Direction, MoveMode, Output,
    SizeStep, SyncMode,
};
use platter_testkit::alloc::{CountingAllocator, count};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn commands() -> Vec<Command> {
    use DeckId::{A, B};
    vec![
        // Key lock on both decks, so what follows runs through the stretch: its jumps,
        // loops, sync and tempo changes, from a 48 kHz and a 44.1 kHz track.
        Command::DeckKeyLockToggle { deck: A },
        Command::DeckKeyLockToggle { deck: B },
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
        Command::DeckSyncToggle { deck: B },
        Command::DeckEject { deck: B },
        Command::DeckKeyLockToggle { deck: A },
        Command::DeckPause { deck: A },
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
    rig.load(DeckId::A, sine_file("memory-a", 440.0, 3.0, 0.5, RATE));
    rig.load(DeckId::B, sine_file("memory-b", 660.0, 3.0, 0.5, 44_100));
    // Four channels, so the headphone path runs too.
    let mut block = vec![0.0f32; 512 * 4];
    let mut events = 0;
    let mut run = |rig: &mut Rig, blocks: usize| {
        for _ in 0..blocks {
            let (_, counted) = count(|| rig.processor.process(&mut block, 4));
            events += counted;
        }
    };

    for command in commands() {
        rig.send(command);
        run(&mut rig, 5);
    }
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
    // The track and its stretch go back to this thread too.
    rig.send(Command::DeckEject { deck: DeckId::A });
    for _ in 0..5 {
        let (_, counted) = count(|| rig.processor.process(&mut block, 4));
        events += counted;
    }

    assert_eq!(
        events, 0,
        "process allocated or freed memory {events} times"
    );
    // The retired audio is freed here, on the control side.
    let (_, freed) = count(|| {
        rig.engine.poll();
    });
    assert!(freed >= 1, "nothing was freed on the control side");
}
