//! Property test: no sequence of commands, valid or not, makes the engine panic, produce a
//! value that is not a number or leave full scale.

mod common;

use std::path::PathBuf;
use std::sync::OnceLock;

use common::{RATE, Rig, sine_file};
use platter_core::{Bpm, CueKind, CueName, DeckId, HotcueSlot, MAX_MARKERS};
use platter_engine::command::{Args, SPECS, Value};
use platter_engine::{
    Band, BpmScale, ClockMode, Command, CrossfaderCurve, CueStep, Direction, MoveMode, Output,
    SizeStep, SyncMode, TEMPO_RANGES,
};
use proptest::prelude::{ProptestConfig, Strategy, any, prop, prop_oneof, proptest};

/// Three files to load: two tracks and one that does not exist.
fn files() -> &'static [PathBuf; 3] {
    static FILES: OnceLock<[PathBuf; 3]> = OnceLock::new();
    FILES.get_or_init(|| {
        [
            sine_file("sweep-a", 440.0, 0.4, 0.9, RATE),
            sine_file("sweep-b", 97.0, 0.3, 0.9, 44_100),
            PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sweep-missing.wav"),
        ]
    })
}

/// Numbers of every kind: inside the ranges, around their edges, huge, and not numbers.
fn number() -> impl Strategy<Value = f64> {
    prop_oneof![
        4 => -1.5f64..2.5,
        2 => -100.0f64..30.0,
        1 => any::<f64>(),
        1 => prop::sample::select(vec![
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            0.0,
            -0.0,
            1.0,
            f64::MAX,
            f64::MIN_POSITIVE,
        ]),
    ]
}

fn deck() -> impl Strategy<Value = DeckId> {
    prop::sample::select(DeckId::ALL.to_vec())
}

fn band() -> impl Strategy<Value = Band> {
    prop::sample::select(vec![Band::Low, Band::Mid, Band::High])
}

/// The commands of the controls that M2 added.
fn later_command() -> impl Strategy<Value = Command> {
    let output = prop::sample::select(Output::ALL.to_vec());
    prop_oneof![
        (deck(), any::<bool>()).prop_map(|(deck, pressed)| Command::DeckCue { deck, pressed }),
        (deck(), any::<bool>()).prop_map(|(deck, pressed)| Command::DeckCup { deck, pressed }),
        deck().prop_map(|deck| Command::DeckTempoRangeCycle { deck }),
        (deck(), band()).prop_map(|(deck, band)| Command::MixerEqKillToggle { deck, band }),
        (deck(), number()).prop_map(|(deck, position)| Command::MixerPanSet { deck, position }),
        deck().prop_map(|deck| Command::MixerCueToggle { deck }),
        deck().prop_map(|deck| Command::MixerGainRestore { deck }),
        prop::strategy::Just(Command::MixerAutogainToggle),
        number().prop_map(|position| Command::MixerCueMixSet { position }),
        number().prop_map(|db| Command::MixerCueVolumeSet { db }),
        output.prop_map(|output| Command::MasterTestTone { output }),
        grid_command(),
        loop_command(),
        cue_command(),
    ]
}

/// The commands of the stored cues, added in M3.
fn cue_command() -> impl Strategy<Value = Command> {
    let slot = || prop::sample::select(HotcueSlot::ALL.to_vec());
    let step = prop::sample::select(vec![CueStep::Previous, CueStep::Next]);
    let kind = prop::sample::select(CueKind::ALL.to_vec());
    prop_oneof![
        4 => (deck(), slot(), any::<bool>()).prop_map(|(deck, slot, pressed)| {
            Command::DeckHotcueTrigger { deck, slot, pressed }
        }),
        1 => (deck(), slot()).prop_map(|(deck, slot)| Command::DeckHotcueDelete { deck, slot }),
        1 => (deck(), slot()).prop_map(|(deck, slot)| Command::DeckHotcueMap { deck, slot }),
        2 => deck().prop_map(|deck| Command::DeckCueStore { deck }),
        2 => (deck(), step).prop_map(|(deck, direction)| Command::DeckCueSelect { deck, direction }),
        1 => (deck(), ".{0,60}").prop_map(|(deck, name)| Command::DeckCueRename { deck, name }),
        2 => (deck(), kind).prop_map(|(deck, kind)| Command::DeckCueTypeSet { deck, kind }),
        1 => deck().prop_map(|deck| Command::DeckCueDelete { deck }),
    ]
}

/// The commands of loops, jumps and moves, added in M3.
fn loop_command() -> impl Strategy<Value = Command> {
    let direction = || prop::sample::select(vec![Direction::Back, Direction::Forward]);
    let size_step = || prop::sample::select(vec![SizeStep::Down, SizeStep::Up]);
    let mode = prop::sample::select(MoveMode::ALL.to_vec());
    prop_oneof![
        2 => deck().prop_map(|deck| Command::DeckLoopToggle { deck }),
        1 => deck().prop_map(|deck| Command::DeckLoopIn { deck }),
        1 => deck().prop_map(|deck| Command::DeckLoopOut { deck }),
        2 => (deck(), size_step())
            .prop_map(|(deck, direction)| Command::DeckLoopSizeStep { deck, direction }),
        1 => (deck(), direction())
            .prop_map(|(deck, direction)| Command::DeckBeatjump { deck, direction }),
        2 => (deck(), direction()).prop_map(|(deck, direction)| Command::DeckMove { deck, direction }),
        1 => (deck(), mode).prop_map(|(deck, mode)| Command::DeckMoveModeSet { deck, mode }),
        1 => (deck(), size_step())
            .prop_map(|(deck, direction)| Command::DeckMoveSizeStep { deck, direction }),
        1 => deck().prop_map(|deck| Command::DeckMoveFineToggle { deck }),
    ]
}

/// The commands of the beatgrid, added in M3.
fn grid_command() -> impl Strategy<Value = Command> {
    let scale = prop::sample::select(vec![BpmScale::Double, BpmScale::Half]);
    prop_oneof![
        (deck(), prop_oneof![number(), 15.0f64..420.0])
            .prop_map(|(deck, bpm)| Command::DeckGridBpmSet { deck, bpm }),
        (deck(), scale).prop_map(|(deck, factor)| Command::DeckGridBpmScale { deck, factor }),
        (deck(), number()).prop_map(|(deck, amount)| Command::DeckGridBpmStep { deck, amount }),
        deck().prop_map(|deck| Command::DeckGridTap { deck }),
        deck().prop_map(|deck| Command::DeckGridMarkerSet { deck }),
        (deck(), number())
            .prop_map(|(deck, seconds)| Command::DeckGridMarkerMove { deck, seconds }),
        deck().prop_map(|deck| Command::DeckGridMarkerDelete { deck }),
        deck().prop_map(|deck| Command::DeckGridMarkerPrevious { deck }),
        deck().prop_map(|deck| Command::DeckGridMarkerNext { deck }),
        deck().prop_map(|deck| Command::DeckGridLockToggle { deck }),
        deck().prop_map(|deck| Command::DeckGridReset { deck }),
        deck().prop_map(|deck| Command::DeckGridBpmReset { deck }),
    ]
}

fn sync_command() -> impl Strategy<Value = Command> {
    let clock_mode = prop::sample::select(vec![ClockMode::Auto, ClockMode::Master]);
    let sync_mode = prop::sample::select(SyncMode::ALL.to_vec());
    prop_oneof![
        3 => deck().prop_map(|deck| Command::DeckSyncToggle { deck }),
        2 => deck().prop_map(|deck| Command::DeckKeyLockToggle { deck }),
        2 => deck().prop_map(|deck| Command::DeckBeatTickToggle { deck }),
        2 => (deck(), prop::sample::select(vec![SizeStep::Down, SizeStep::Up]))
            .prop_map(|(deck, direction)| Command::DeckKeyStep { deck, direction }),
        2 => (deck(), number()).prop_map(|(deck, semitones)| Command::DeckKeySet { deck, semitones }),
        1 => deck().prop_map(|deck| Command::DeckKeyPersist { deck }),
        2 => deck().prop_map(|deck| Command::DeckFluxToggle { deck }),
        2 => (deck(), any::<bool>()).prop_map(|(deck, pressed)| Command::DeckReverse { deck, pressed }),
        2 => deck().prop_map(|deck| Command::DeckMasterSet { deck }),
        2 => clock_mode.prop_map(|mode| Command::ClockModeSet { mode }),
        1 => (15.0f64..420.0).prop_map(|bpm| Command::ClockTempoSet { bpm }),
        1 => number().prop_map(|amount| Command::ClockTempoStep { amount }),
        1 => proptest::strategy::Just(Command::ClockTap),
        1 => proptest::strategy::Just(Command::ClockMetronomeToggle),
        1 => number().prop_map(|amount| Command::ClockBend { amount }),
        1 => sync_mode.prop_map(|mode| Command::GlobalSyncModeSet { mode }),
        1 => proptest::strategy::Just(Command::GlobalSnapToggle),
        1 => proptest::strategy::Just(Command::GlobalQuantizeToggle),
    ]
}

fn command() -> impl Strategy<Value = Command> {
    let band = band();
    let curve = prop::sample::select(CrossfaderCurve::ALL.to_vec());
    prop_oneof![
        6 => later_command(),
        4 => sync_command(),
        1 => (deck(), 0usize..3).prop_map(|(deck, file)| Command::DeckLoad {
            deck,
            path: files()[file].clone()
        }),
        1 => deck().prop_map(|deck| Command::DeckEject { deck }),
        1 => deck().prop_map(|deck| Command::DeckPlay { deck }),
        1 => deck().prop_map(|deck| Command::DeckPause { deck }),
        1 => deck().prop_map(|deck| Command::DeckPlayPause { deck }),
        1 => deck().prop_map(|deck| Command::DeckCueSet { deck }),
        1 => deck().prop_map(|deck| Command::DeckCueGoto { deck }),
        1 => (deck(), number()).prop_map(|(deck, seconds)| Command::DeckSeek { deck, seconds }),
        1 => (deck(), number()).prop_map(|(deck, ratio)| Command::DeckTempoSet { deck, ratio }),
        1 => (deck(), number()).prop_map(|(deck, amount)| Command::DeckNudge { deck, amount }),
        1 => (deck(), number()).prop_map(|(deck, db)| Command::MixerGainSet { deck, db }),
        1 => (deck(), band, number()).prop_map(|(deck, band, position)| Command::MixerEqSet {
            deck,
            band,
            position
        }),
        1 => (deck(), number()).prop_map(|(deck, position)| Command::MixerFilterSet { deck, position }),
        1 => (deck(), number()).prop_map(|(deck, position)| Command::MixerFaderSet { deck, position }),
        1 => number().prop_map(|position| Command::MixerCrossfaderSet { position }),
        1 => curve.prop_map(|curve| Command::MixerCrossfaderCurveSet { curve }),
        1 => number().prop_map(|db| Command::MasterGainSet { db }),
    ]
}

fn value() -> impl Strategy<Value = Value> {
    prop_oneof![
        number().prop_map(Value::Number),
        "[a-z./ ]{0,8}".prop_map(Value::Text),
        prop::sample::select(vec![
            "a",
            "b",
            "low",
            "high",
            "sharp",
            "press",
            "release",
            "master",
            "headphones",
            "double",
            "half",
            "back",
            "forward",
            "down",
            "up",
            "beatjump",
            "loop",
            "loop_in",
            "loop_out",
            "previous",
            "next",
            "cue",
            "fade_in",
            "fade_out",
            "load",
            "auto",
            "tempo",
            "beat",
        ])
        .prop_map(|text| Value::Text(text.to_owned())),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    #[test]
    fn no_command_sequence_breaks_the_output(
        steps in prop::collection::vec((command(), 0usize..1_500, 1usize..7), 1..40),
    ) {
        let mut rig = Rig::new();
        let mut block = Vec::new();
        for (command, frames, channels) in steps {
            // Invalid commands are refused; that is an outcome, not a failure.
            let _ = rig.engine.dispatch(command);
            rig.engine.wait_for_loads();
            block.clear();
            block.resize(frames * channels, f32::NAN);
            rig.processor.process(&mut block, channels);
            for sample in &block {
                assert!(sample.is_finite() && sample.abs() <= 1.0, "{sample}");
            }
        }
        // An empty block: the processor reports what the last commands left.
        rig.processor.process(&mut [], 2);
        rig.engine.poll();
        let snapshot = rig.engine.snapshot();
        for deck in &snapshot.decks {
            let markers = deck.grid.markers();
            assert!(markers.len() <= MAX_MARKERS);
            assert!(markers.iter().all(|marker| marker.is_finite() && *marker >= 0.0));
            assert!(markers.windows(2).all(|pair| pair[0] < pair[1]), "{markers:?}");
            if let Some(bpm) = deck.grid.bpm() {
                assert!((Bpm::MIN..=Bpm::MAX).contains(&bpm.value()));
            }
            assert!(deck.position_seconds.is_finite() && deck.position_seconds >= 0.0);
            let seconds = deck.track.as_ref().map_or(0.0, |track| track.seconds);
            if let Some(region) = deck.looping.region {
                assert!(deck.looping.in_point.is_none());
                assert!(region.start >= 0.0 && region.start < region.end, "{region:?}");
                // A frame of slack: the length of the file is rounded to its frames.
                assert!(region.end <= seconds + 1e-4, "{region:?} in {seconds}");
            }
            if let Some(at) = deck.looping.in_point {
                assert!((0.0..=seconds + 1e-4).contains(&at), "{at} in {seconds}");
            }
            for (_, cue) in deck.cues.iter() {
                assert!((0.0..=seconds + 1e-4).contains(&cue.position), "{cue:?}");
                assert!(cue.name.as_str().len() <= CueName::MAX_BYTES);
                assert_eq!(cue.region().is_some(), cue.kind() == CueKind::Loop);
                if let Some(region) = cue.region() {
                    assert!(region.start < region.end && region.end <= seconds + 1e-4, "{cue:?}");
                }
            }
            if let Some(slot) = deck.cues.selected() {
                assert!(deck.cues.get(slot).is_some(), "an empty slot is selected");
            }
            assert!(deck.cue_seconds.is_finite());
            assert!(deck.levels.iter().all(|level| level.is_finite()));
            // deck.tempo.set takes the whole span of a tempo whatever the range of the fader
            // is, so only that span holds here; the range pulls the tempo in when it changes.
            assert!(TEMPO_RANGES.contains(&deck.controls.tempo_range));
            assert!((0.0..=2.0).contains(&deck.controls.tempo_ratio));
            assert!((0.0..=2.0).contains(&deck.speed), "{}", deck.speed);
            if let Some(phase) = deck.phase {
                assert!((-0.5..=0.5).contains(&phase), "{phase}");
                assert!(deck.grid.bpm().is_some());
            }
        }
        let clock = &snapshot.clock;
        assert!(clock.bpm.is_finite() && clock.bpm >= 0.0, "{}", clock.bpm);
        if let Some(master) = clock.master {
            let deck = &snapshot.decks[master.index()];
            assert!(deck.grid.bpm().is_some(), "a tempo master without a tempo");
        } else {
            // A bend is at most half the tempo either way.
            assert!((Bpm::MIN * 0.5..=Bpm::MAX * 1.5).contains(&clock.bpm), "{}", clock.bpm);
        }
        assert!(snapshot.master_levels.iter().all(|level| level.is_finite() && *level <= 1.0));
    }

    #[test]
    fn untyped_input_builds_a_command_or_an_error(
        spec in prop::sample::select(SPECS.to_vec()),
        use_real_id in any::<bool>(),
        other_id in "[a-z._]{0,24}",
        values in prop::collection::vec(value(), 0..5),
        extra in prop::option::of(("[a-z]{1,6}", value())),
    ) {
        let mut args: Args = spec
            .params
            .iter()
            .zip(values)
            .map(|(param, value)| (param.name.to_owned(), value))
            .collect();
        if let Some((name, value)) = extra {
            args.insert(name, value);
        }
        let id = if use_real_id { spec.id } else { other_id.as_str() };
        if let Ok(command) = Command::from_args(id, &args) {
            assert_eq!(command.id(), id);
            let mut rig = Rig::new();
            let _ = rig.engine.dispatch(command);
            rig.engine.wait_for_loads();
        }
    }
}
