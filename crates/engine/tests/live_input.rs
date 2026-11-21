//! Live input: a deck that plays an input of the device, and the auxiliary input.
//!
//! Behaviour note: `docs/behavior/live-input.md`; design: `docs/decisions/0040-audio-input.md`.

mod common;

use common::{RATE, Rig, sine_file};
use platter_core::DeckId;
use platter_engine::command::{Args, Value};
use platter_engine::{Command, InputFeed, InputSource, InputTarget, LoopSource, MixerMode};
use platter_testkit::measure::{max_step, sine_fit};
use platter_testkit::signal::channel;

const A: DeckId = DeckId::A;
/// Frames the output asks for at a time, and the input delivers.
const BLOCK: usize = 512;

/// A device with eight inputs: a 1 kHz sine at 0.5 on the first, one of 300 Hz at 0.25 on
/// the second, the others silent.
struct Device {
    feed: InputFeed,
    frame: usize,
}

impl Device {
    fn new(feed: InputFeed) -> Self {
        Self { feed, frame: 0 }
    }

    fn deliver(&mut self) {
        let mut samples = Vec::with_capacity(BLOCK * 8);
        for index in self.frame..self.frame + BLOCK {
            let time = index as f64 / f64::from(RATE);
            samples.push((std::f64::consts::TAU * 1_000.0 * time).sin() as f32 * 0.5);
            samples.push((std::f64::consts::TAU * 300.0 * time).sin() as f32 * 0.25);
            samples.extend([0.0; 6]);
        }
        self.frame += BLOCK;
        self.feed.push(&samples, 8);
    }
}

/// Runs `seconds` of stereo output, the device delivering a block before each block.
fn run(rig: &mut Rig, device: &mut Device, seconds: f64) -> Vec<f32> {
    let blocks = (seconds * f64::from(RATE) / BLOCK as f64).ceil() as usize;
    let mut out = Vec::with_capacity(blocks * BLOCK * 2);
    let mut block = vec![0.0f32; BLOCK * 2];
    for _ in 0..blocks {
        device.deliver();
        rig.processor.process(&mut block, 2);
        out.extend_from_slice(&block);
    }
    out
}

fn amplitude(samples: &[f32], side: usize, hz: f64) -> f64 {
    sine_fit(&channel(samples, 2, side), hz, f64::from(RATE)).amplitude
}

fn route(rig: &mut Rig, target: InputTarget, input: &str) {
    rig.send(Command::InputRoutingSet {
        target,
        source: InputSource::parse(input),
    });
}

fn live(rig: &mut Rig, deck: DeckId, live: bool) {
    rig.send(Command::DeckTypeSet { deck, live });
    // The crossfader at the side of deck A gives it full gain.
    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
}

#[test]
fn a_live_deck_plays_its_input_through_its_channel() {
    let mut rig = Rig::new();
    let mut device = Device::new(rig.engine.take_input_feed().unwrap());
    route(&mut rig, InputTarget::Deck(A), "1-2");
    live(&mut rig, A, true);
    run(&mut rig, &mut device, 0.5);
    let heard = run(&mut rig, &mut device, 0.5);
    let measured = amplitude(&heard, 0, 1_000.0);
    assert!((measured - 0.5).abs() < 0.01, "{measured}");
    assert!((amplitude(&heard, 1, 300.0) - 0.25).abs() < 0.01);
    // The fader of the channel acts on it, as on a track.
    rig.send(Command::MixerFaderSet {
        deck: A,
        position: 0.0,
    });
    run(&mut rig, &mut device, 0.1);
    let faded = run(&mut rig, &mut device, 0.2);
    assert!(amplitude(&faded, 0, 1_000.0) < 1e-4);
    let snapshot = rig.engine.snapshot();
    assert!(snapshot.decks[0].controls.live);
    assert!(snapshot.input.reading);
    assert_eq!(snapshot.input.gaps, 0);
    assert!((snapshot.input.levels[0] - 0.5).abs() < 0.01);
    assert!(snapshot.input.levels[2] < 1e-6);
}

#[test]
fn a_single_input_is_heard_on_both_sides() {
    let mut rig = Rig::new();
    let mut device = Device::new(rig.engine.take_input_feed().unwrap());
    route(&mut rig, InputTarget::Deck(A), "2");
    live(&mut rig, A, true);
    run(&mut rig, &mut device, 0.5);
    let heard = run(&mut rig, &mut device, 0.5);
    assert!((amplitude(&heard, 0, 300.0) - 0.25).abs() < 0.01);
    assert!((amplitude(&heard, 1, 300.0) - 0.25).abs() < 0.01);
}

#[test]
fn switching_the_type_fades_and_keeps_the_track_paused() {
    let mut rig = Rig::new();
    let mut device = Device::new(rig.engine.take_input_feed().unwrap());
    rig.load(A, sine_file("live-track", 440.0, 10.0, 0.5, RATE));
    rig.send(Command::DeckPlay { deck: A });
    route(&mut rig, InputTarget::Deck(A), "1-2");
    run(&mut rig, &mut device, 0.5);
    live(&mut rig, A, true);
    let switch = run(&mut rig, &mut device, 0.5);
    assert!(max_step(&channel(&switch, 2, 0)) < 0.1, "the switch clicks");
    let heard = run(&mut rig, &mut device, 0.5);
    assert!(
        amplitude(&heard, 0, 440.0) < 1e-3,
        "the track is still heard"
    );
    assert!(!rig.engine.snapshot().decks[0].playing);
    // Play does nothing on a live deck.
    rig.send(Command::DeckPlay { deck: A });
    run(&mut rig, &mut device, 0.1);
    assert!(!rig.engine.snapshot().decks[0].playing);
    live(&mut rig, A, false);
    let back = run(&mut rig, &mut device, 0.5);
    assert!(amplitude(&back[BLOCK * 8..], 0, 1_000.0) < 1e-3);
    rig.send(Command::DeckPlay { deck: A });
    run(&mut rig, &mut device, 0.1);
    assert!(rig.engine.snapshot().decks[0].playing);
}

#[test]
fn a_live_deck_without_a_route_is_silent() {
    let mut rig = Rig::new();
    let mut device = Device::new(rig.engine.take_input_feed().unwrap());
    live(&mut rig, A, true);
    run(&mut rig, &mut device, 0.5);
    let heard = run(&mut rig, &mut device, 0.5);
    assert!(heard.iter().all(|sample| sample.abs() < 1e-6));
}

#[test]
fn the_auxiliary_input_joins_the_master_at_its_level_and_not_in_external_mode() {
    let mut rig = Rig::new();
    let mut device = Device::new(rig.engine.take_input_feed().unwrap());
    route(&mut rig, InputTarget::Aux, "1");
    run(&mut rig, &mut device, 0.5);
    let silent = run(&mut rig, &mut device, 0.3);
    assert!(
        amplitude(&silent, 0, 1_000.0) < 1e-4,
        "the level starts at zero"
    );
    rig.send(Command::MixerAuxSet { level: 0.5 });
    run(&mut rig, &mut device, 0.1);
    let heard = run(&mut rig, &mut device, 0.5);
    assert!((amplitude(&heard, 0, 1_000.0) - 0.25).abs() < 0.01);
    assert!((amplitude(&heard, 1, 1_000.0) - 0.25).abs() < 0.01);
    rig.send(Command::MixerModeSet {
        mode: MixerMode::External,
    });
    run(&mut rig, &mut device, 0.1);
    let external = run(&mut rig, &mut device, 0.3);
    assert!(amplitude(&external, 0, 1_000.0) < 1e-4);
}

#[test]
fn a_live_deck_is_never_the_tempo_master() {
    let mut rig = Rig::new();
    let mut device = Device::new(rig.engine.take_input_feed().unwrap());
    rig.load(A, sine_file("live-master", 440.0, 10.0, 0.5, RATE));
    rig.send(Command::DeckPlay { deck: A });
    run(&mut rig, &mut device, 0.2);
    live(&mut rig, A, true);
    run(&mut rig, &mut device, 0.2);
    assert_eq!(rig.engine.snapshot().clock.master, None);
}

#[test]
fn a_stopped_device_gives_silence_and_one_gap() {
    let mut rig = Rig::new();
    let mut device = Device::new(rig.engine.take_input_feed().unwrap());
    route(&mut rig, InputTarget::Deck(A), "1-2");
    live(&mut rig, A, true);
    run(&mut rig, &mut device, 0.5);
    let mut block = vec![0.0f32; BLOCK * 2];
    let mut tail = Vec::new();
    for _ in 0..20 {
        rig.processor.process(&mut block, 2);
        tail.extend_from_slice(&block);
    }
    assert!(max_step(&channel(&tail, 2, 0)) < 0.1, "the gap clicks");
    assert!(tail[tail.len() - BLOCK * 2..].iter().all(|s| *s == 0.0));
    rig.engine.poll();
    let snapshot = rig.engine.snapshot();
    assert_eq!(snapshot.input.gaps, 1);
    assert!(!snapshot.input.reading);
}

#[test]
fn the_controls_survive_a_new_engine() {
    let mut rig = Rig::new();
    route(&mut rig, InputTarget::Aux, "3-4");
    rig.send(Command::MixerAuxSet { level: 0.7 });
    live(&mut rig, DeckId::C, true);
    let rate = common::rate();
    let (engine, _) = platter_engine::Engine::with_controls_of(rate, &rig.engine);
    let snapshot = engine.snapshot();
    assert_eq!(
        snapshot.mixer.input_routes[InputTarget::Aux.index()],
        InputSource::parse("3-4")
    );
    assert_eq!(snapshot.mixer.aux_level, 0.7);
    assert!(snapshot.decks[2].controls.live);
}

#[test]
fn the_commands_parse_from_text() {
    let parse = |id: &str, pairs: &[(&str, &str)]| {
        let args: Args = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), Value::Text((*value).to_owned())))
            .collect();
        Command::from_args(id, &args)
    };
    assert!(matches!(
        parse("deck.type.set", &[("deck", "b"), ("type", "live")]),
        Ok(Command::DeckTypeSet {
            deck: DeckId::B,
            live: true
        })
    ));
    assert!(matches!(
        parse("input.routing.set", &[("target", "aux"), ("input", "none")]),
        Ok(Command::InputRoutingSet {
            target: InputTarget::Aux,
            source: None
        })
    ));
    assert!(matches!(
        parse(
            "input.routing.set",
            &[("target", "deck_d"), ("input", "7-8")]
        ),
        Ok(Command::InputRoutingSet {
            target: InputTarget::Deck(DeckId::D),
            source: Some(InputSource::Pair(6))
        })
    ));
    assert!(parse("input.routing.set", &[("target", "aux"), ("input", "2-3")]).is_err());
    assert!(parse("deck.type.set", &[("deck", "a"), ("type", "remix")]).is_err());
    let mut rig = Rig::new();
    assert!(
        rig.engine
            .dispatch(Command::MixerAuxSet { level: 1.5 })
            .is_err()
    );
}

#[test]
fn the_loop_recorder_records_the_input_routed_to_the_recorders() {
    let mut rig = Rig::new();
    let mut device = Device::new(rig.engine.take_input_feed().unwrap());
    route(&mut rig, InputTarget::Recorder, "1-2");
    rig.send(Command::LoopRecorderSizeSet { bars: 1.0 });
    rig.send(Command::LoopRecorderSourceSet {
        source: LoopSource::External,
    });
    // In external mixer mode the loop would play into a master nobody hears: Rec is refused
    // for the input too.
    rig.send(Command::MixerModeSet {
        mode: MixerMode::External,
    });
    assert!(rig.engine.dispatch(Command::LoopRecorderRecord).is_err());
    rig.send(Command::MixerModeSet {
        mode: MixerMode::Internal,
    });
    run(&mut rig, &mut device, 0.3);
    rig.send(Command::LoopRecorderRecord);
    // The pass waits for the beat and lasts a bar of two seconds at 120 BPM.
    run(&mut rig, &mut device, 2.6);
    // Nothing routed any more: what sounds is the loop of the input.
    route(&mut rig, InputTarget::Recorder, "none");
    run(&mut rig, &mut device, 0.1);
    let heard = run(&mut rig, &mut device, 1.0);
    let measured = amplitude(&heard, 0, 1_000.0);
    assert!((measured - 0.5).abs() < 0.02, "{measured}");
    assert!((amplitude(&heard, 1, 300.0) - 0.25).abs() < 0.02);
}

#[test]
fn the_mix_recorder_takes_its_source_and_keeps_it_for_a_new_engine() {
    let mut rig = Rig::new();
    assert!(!rig.engine.snapshot().recorder.external);
    rig.send(Command::RecorderSourceSet { external: true });
    assert!(rig.engine.snapshot().recorder.external);
    let (engine, _) = platter_engine::Engine::with_controls_of(common::rate(), &rig.engine);
    assert!(engine.snapshot().recorder.external);
    let args: Args = [("source".to_owned(), Value::Text("external".to_owned()))]
        .into_iter()
        .collect();
    assert!(matches!(
        Command::from_args("recorder.source.set", &args),
        Ok(Command::RecorderSourceSet { external: true })
    ));
}
