//! The processor neither allocates nor frees memory, whatever it is asked to do.
//!
//! This file holds a single test because it installs its own global allocator; the count
//! is per thread, so the worker threads that decode do not disturb it.

mod common;

use common::{RATE, Rig, sine_file};
use platter_core::DeckId;
use platter_engine::{Band, Command, CrossfaderCurve, Output};
use platter_testkit::alloc::{CountingAllocator, count};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn commands() -> Vec<Command> {
    use DeckId::{A, B};
    vec![
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
        Command::DeckPlay { deck: A },
        Command::DeckEject { deck: B },
        Command::DeckPause { deck: A },
    ]
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
