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
    stem_files_of(name, drums_rate, 6.0)
}

fn stem_files_of(
    name: &str,
    drums_rate: u32,
    seconds: f64,
) -> (PathBuf, Box<[PathBuf; Stem::COUNT]>) {
    let track = sine_file(&format!("{name}-track"), 440.0, seconds, 0.5, RATE);
    let stems = Stem::ALL.map(|stem| {
        let (amplitude, rate) = if stem == Stem::Drums {
            (0.5, drums_rate)
        } else {
            (0.0, RATE)
        };
        sine_file(&format!("{name}-{stem}"), 440.0, seconds, amplitude, rate)
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

/// How one case of [`cost_of_stem_decks`] plays.
struct Case {
    name: &'static str,
    decks: usize,
    stems: bool,
    key_lock: bool,
    /// Seconds of audio played between the start of one deck and the next.
    offset: f64,
    /// Key lock runs on worker threads, and the blocks come at the pace of a device.
    threads: bool,
}

/// The time spent on each block of 256 frames, the period of a low-latency device, as a share
/// of that period, sorted.
fn block_costs(case: &Case, seconds: f64) -> (Vec<f64>, Vec<usize>, usize) {
    let mut rig = Rig::new();
    if case.threads {
        assert!(rig.engine.set_key_lock_threads(true).is_ok());
    }
    for deck in DeckId::ALL.into_iter().take(case.decks) {
        // Longer than what is measured, so no deck reaches the end of its track.
        let (path, files) = stem_files_of(&format!("stems-cost-{deck}"), RATE, seconds + 5.0);
        rig.send(if case.stems {
            Command::DeckLoadStems {
                deck,
                path,
                stems: files,
            }
        } else {
            Command::DeckLoad { deck, path }
        });
        rig.engine.wait_for_loads();
        rig.send(Command::DeckTempoSet { deck, ratio: 1.05 });
        if case.key_lock {
            rig.send(Command::DeckKeyLockToggle { deck });
        }
        rig.send(Command::DeckPlay { deck });
        rig.run(case.offset);
    }
    rig.run(0.5);
    let mut block = [0.0f32; 512];
    let period = 256.0 / f64::from(RATE);
    let blocks = (seconds / period).round() as usize;
    let begin = std::time::Instant::now();
    let costs: Vec<f64> = (0..blocks)
        .map(|index| {
            if case.threads {
                // A device asks for the next block only once the last one has played.
                let due = begin + std::time::Duration::from_secs_f64(period * index as f64);
                while std::time::Instant::now() < due {
                    std::hint::spin_loop();
                }
            }
            let start = std::time::Instant::now();
            rig.processor.process(&mut block, 2);
            start.elapsed().as_secs_f64() / period
        })
        .collect();
    let starved = rig.engine.key_lock_starved();
    let late = costs
        .iter()
        .enumerate()
        .filter(|(_, cost)| **cost > 1.0)
        .map(|(index, _)| index)
        .collect();
    let mut sorted = costs;
    sorted.sort_by(f64::total_cmp);
    (sorted, late, starved)
}

/// The cost of playing at a tempo of 1.05 in a few cases. A measurement, not a check: run with
/// `cargo test -p platter-engine --release --test stems cost -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn cost_of_stem_decks() {
    let cases = [
        Case {
            name: "4 track decks, no key lock",
            decks: 4,
            stems: false,
            key_lock: false,
            offset: 0.0,
            threads: false,
        },
        Case {
            name: "4 stem decks, no key lock",
            decks: 4,
            stems: true,
            key_lock: false,
            offset: 0.0,
            threads: false,
        },
        Case {
            name: "1 track deck, key lock",
            decks: 1,
            stems: false,
            key_lock: true,
            offset: 0.0,
            threads: false,
        },
        Case {
            name: "1 stem deck, key lock",
            decks: 1,
            stems: true,
            key_lock: true,
            offset: 0.0,
            threads: false,
        },
        Case {
            name: "4 track decks, key lock",
            decks: 4,
            stems: false,
            key_lock: true,
            offset: 0.0,
            threads: false,
        },
        Case {
            name: "4 stem decks, key lock",
            decks: 4,
            stems: true,
            key_lock: true,
            offset: 0.0,
            threads: false,
        },
        Case {
            name: "4 stem decks, key lock, staggered",
            decks: 4,
            stems: true,
            key_lock: true,
            offset: 0.013,
            threads: false,
        },
        Case {
            name: "4 stem decks, key lock, threads",
            decks: 4,
            stems: true,
            key_lock: true,
            offset: 0.0,
            threads: true,
        },
        Case {
            name: "1 stem deck, key lock, threads",
            decks: 1,
            stems: true,
            key_lock: true,
            offset: 0.0,
            threads: true,
        },
    ];
    for case in &cases {
        let (sorted, late, starved) = block_costs(case, 20.0);
        let count = sorted.len();
        let at = |share: f64| {
            sorted
                .get((count as f64 * share) as usize)
                .copied()
                .unwrap_or(0.0)
        };
        let mean = sorted.iter().sum::<f64>() / count as f64;
        let gaps: Vec<usize> = late
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .take(12)
            .collect();
        println!(
            "{:36} mean {:5.1} %  median {:4.0} %  99 % {:4.0} %  99.9 % {:4.0} %  worst {:4.0} %               late {:4} of {count}  starved {starved}  gaps {gaps:?}",
            case.name,
            mean * 100.0,
            at(0.5) * 100.0,
            at(0.99) * 100.0,
            at(0.999) * 100.0,
            sorted.last().copied().unwrap_or(0.0) * 100.0,
            late.len(),
        );
    }
}

/// `docs/behavior/keylock.md`, *Worker threads*: the four stretches of a stem deck on worker
/// threads stay together through changes of tempo. Each stem holds a quarter of one sine, so
/// stems that drifted apart would no longer add up to it.
#[test]
fn stems_on_worker_threads_stay_together_through_changes_of_tempo() {
    let mut rig = Rig::new();
    rig.engine.set_key_lock_pumped();
    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
    let track = sine_file("stems-threads-track", 440.0, 12.0, 0.5, RATE);
    let stems = Box::new(
        Stem::ALL.map(|stem| sine_file(&format!("stems-threads-{stem}"), 440.0, 12.0, 0.125, RATE)),
    );
    rig.send(Command::DeckLoadStems {
        deck: A,
        path: track,
        stems,
    });
    rig.engine.wait_for_loads();
    rig.send(Command::DeckKeyLockToggle { deck: A });
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.3);
    let mut heard = Vec::new();
    for step in 0..30 {
        let ratio = 1.0 + 0.002 * f64::from(step % 10);
        rig.send(Command::DeckTempoSet { deck: A, ratio });
        heard.extend(rig.run(0.1));
    }
    let level = rms(&heard);
    assert!((level - FULL).abs() < 0.01, "rms {level}");
    assert_eq!(rig.engine.key_lock_starved(), 0);
}
