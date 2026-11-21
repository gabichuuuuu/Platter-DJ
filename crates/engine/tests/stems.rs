//! The stem deck and the controls of each stem. Behaviour note: `docs/behavior/stems.md`.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::{RATE, Rig, sine_file};
use platter_core::{DeckId, Stem};
use platter_engine::command::{Args, Value};
use platter_engine::{Command, Event};
use platter_testkit::measure::rms;

const A: DeckId = DeckId::A;
/// The level of a sine of amplitude 0.5.
const FULL: f64 = 0.353_553;

/// A track of a 440 Hz sine and its stems: the drums are the same sine, the other three are
/// silent, so the stems add up to the track.
fn stem_files(name: &str, drums_rate: u32) -> (PathBuf, Box<[PathBuf; Stem::COUNT]>) {
    let track = sine_file(&format!("{name}-track"), 440.0, 6.0, 0.5, RATE);
    let stems = Stem::ALL.map(|stem| {
        let (amplitude, rate) = if stem == Stem::Drums {
            (0.5, drums_rate)
        } else {
            (0.0, RATE)
        };
        sine_file(&format!("{name}-{stem}"), 440.0, 6.0, amplitude, rate)
    });
    (track, Box::new(stems))
}

/// Deck A alone on the master, playing the stems of the track.
fn stem_deck(name: &str) -> Rig {
    let mut rig = Rig::new();
    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
    let (path, stems) = stem_files(name, RATE);
    rig.send(Command::DeckLoadStems {
        deck: A,
        path,
        stems,
    });
    let events = rig.engine.wait_for_loads();
    assert!(
        matches!(events.as_slice(), [Event::Loaded { .. }]),
        "{events:?}"
    );
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.05);
    rig
}

fn level(rig: &mut Rig) -> f64 {
    rig.run(0.05);
    rms(&rig.run(0.5))
}

fn drums(command: impl Fn(DeckId, Stem) -> Command) -> Command {
    command(A, Stem::Drums)
}

#[test]
fn a_stem_deck_sounds_like_its_track_until_a_stem_is_changed() {
    let mut rig = stem_deck("stems-sum");
    assert!(rig.engine.snapshot().decks[0].stem_peaks.is_some());
    let full = level(&mut rig);
    assert!((full - FULL).abs() < 0.01, "rms {full}");

    rig.send(drums(|deck, stem| Command::DeckStemVolumeSet {
        deck,
        stem,
        volume: 0.5,
    }));
    let half = level(&mut rig);
    assert!((half - FULL / 2.0).abs() < 0.01, "rms {half}");
    rig.send(drums(|deck, stem| Command::DeckStemMuteToggle {
        deck,
        stem,
    }));
    assert!(level(&mut rig) < 1e-4);
    // Muted, the stem comes back at its level.
    rig.send(drums(|deck, stem| Command::DeckStemMuteToggle {
        deck,
        stem,
    }));
    assert!((level(&mut rig) - half).abs() < 0.01);
    let controls = rig.engine.snapshot().decks[0].controls.stems[0];
    assert!((controls.volume - 0.5).abs() < 1e-6 && !controls.muted);
}

#[test]
fn what_a_stem_does_not_send_still_goes_through_the_channel() {
    let mut rig = stem_deck("stems-send");
    let full = level(&mut rig);
    rig.send(drums(|deck, stem| Command::DeckStemSendSet {
        deck,
        stem,
        amount: 0.0,
    }));
    rig.send(drums(|deck, stem| Command::DeckStemSendToggle {
        deck,
        stem,
    }));
    assert!((level(&mut rig) - full).abs() < 0.005);
    // The fader of the channel acts on the part around the effects too.
    rig.send(Command::MixerFaderSet {
        deck: A,
        position: 0.0,
    });
    assert!(level(&mut rig) < 1e-4);
}

#[test]
fn the_filter_of_a_stem_acts_only_while_it_is_on() {
    let mut rig = stem_deck("stems-filter");
    let full = level(&mut rig);
    rig.send(drums(|deck, stem| Command::DeckStemFilterSet {
        deck,
        stem,
        position: -1.0,
    }));
    assert!(
        (level(&mut rig) - full).abs() < 0.005,
        "off, the knob does nothing"
    );
    rig.send(drums(|deck, stem| Command::DeckStemFilterToggle {
        deck,
        stem,
    }));
    let filtered = level(&mut rig);
    assert!(filtered < full * 0.5, "{filtered} {full}");
}

#[test]
fn a_load_puts_the_stems_back_to_their_defaults() {
    let mut rig = stem_deck("stems-reset");
    rig.send(drums(|deck, stem| Command::DeckStemMuteToggle {
        deck,
        stem,
    }));
    assert!(level(&mut rig) < 1e-4);
    let (path, stems) = stem_files("stems-reset-again", RATE);
    rig.send(Command::DeckLoadStems {
        deck: A,
        path,
        stems,
    });
    rig.engine.wait_for_loads();
    rig.send(Command::DeckPlay { deck: A });
    assert!((level(&mut rig) - FULL).abs() < 0.01);
    assert!(!rig.engine.snapshot().decks[0].controls.stems[0].muted);
}

#[test]
fn key_lock_plays_the_stems_together_at_another_tempo() {
    let mut rig = stem_deck("stems-lock");
    rig.send(Command::DeckKeyLockToggle { deck: A });
    rig.send(Command::DeckTempoSet {
        deck: A,
        ratio: 1.08,
    });
    rig.run(0.3);
    let locked = level(&mut rig);
    assert!((locked - FULL).abs() < 0.03, "rms {locked}");
    rig.send(drums(|deck, stem| Command::DeckStemMuteToggle {
        deck,
        stem,
    }));
    assert!(level(&mut rig) < 1e-3);
}

#[test]
fn a_stem_at_another_rate_is_brought_to_that_of_the_track() {
    let mut rig = Rig::new();
    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
    // The drums at 44.1 kHz, as the separation writes them, under a track at 48 kHz.
    let (path, stems) = stem_files("stems-rate", 44_100);
    rig.send(Command::DeckLoadStems {
        deck: A,
        path,
        stems,
    });
    let events = rig.engine.wait_for_loads();
    assert!(
        matches!(events.as_slice(), [Event::Loaded { .. }]),
        "{events:?}"
    );
    assert!(rig.engine.snapshot().decks[0].stem_peaks.is_some());
    rig.send(Command::DeckPlay { deck: A });
    assert!((level(&mut rig) - FULL).abs() < 0.01);
}

#[test]
fn stems_that_cannot_be_read_leave_the_track_playing_alone() {
    let mut rig = Rig::new();
    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
    let (path, mut stems) = stem_files("stems-missing", RATE);
    stems[1] = PathBuf::from("no-such-bass.wav");
    rig.send(Command::DeckLoadStems {
        deck: A,
        path,
        stems,
    });
    let events = rig.engine.wait_for_loads();
    assert!(
        matches!(
            events.as_slice(),
            [Event::Loaded { .. }, Event::StemsFailed { deck: A, .. }]
        ),
        "{events:?}"
    );
    assert!(rig.engine.snapshot().decks[0].stem_peaks.is_none());
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.05);
    // The controls of the stems do nothing to a track.
    rig.send(drums(|deck, stem| Command::DeckStemMuteToggle {
        deck,
        stem,
    }));
    assert!((level(&mut rig) - FULL).abs() < 0.01);
}

#[test]
fn stem_commands_are_read_by_name_and_checked() {
    let args = |stem: &str| {
        let mut args = Args::new();
        args.insert("deck".to_owned(), Value::Text("b".to_owned()));
        args.insert("stem".to_owned(), Value::Text(stem.to_owned()));
        args.insert("volume".to_owned(), Value::Number(0.25));
        args
    };
    assert_eq!(
        Command::from_args("deck.stem.volume.set", &args("Vocals")).ok(),
        Some(Command::DeckStemVolumeSet {
            deck: DeckId::B,
            stem: Stem::Vocals,
            volume: 0.25,
        })
    );
    assert!(Command::from_args("deck.stem.volume.set", &args("piano")).is_err());
    let mut rig = Rig::new();
    assert!(
        rig.engine
            .dispatch(Command::DeckStemSendSet {
                deck: A,
                stem: Stem::Bass,
                amount: 1.5,
            })
            .is_err()
    );
}

#[test]
fn a_load_takes_the_stems_the_collection_links_unless_it_asks_for_the_track() {
    let mut rig = Rig::new();
    let (path, stems) = stem_files("stems-linked", RATE);
    let linked = path.clone();
    rig.engine.set_stem_recall(Arc::new(move |asked: &Path| {
        (asked == linked).then(|| (*stems).clone())
    }));
    rig.send(Command::DeckLoad {
        deck: A,
        path: path.clone(),
    });
    rig.engine.wait_for_loads();
    assert!(rig.engine.snapshot().decks[0].stem_peaks.is_some());
    rig.send(Command::DeckLoadTrack { deck: A, path });
    rig.engine.wait_for_loads();
    let snapshot = rig.engine.snapshot();
    assert!(snapshot.decks[0].track.is_some() && snapshot.decks[0].stem_peaks.is_none());
}
