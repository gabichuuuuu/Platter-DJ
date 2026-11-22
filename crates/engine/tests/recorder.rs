//! The mix recorder: `docs/behavior/recording.md`.

#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use common::{RATE, Rig, sine_file};
use platter_core::DeckId;
use platter_decode::decode_file;
use platter_engine::{Command, EngineError, Event};

const WAIT: Duration = Duration::from_secs(10);

/// A folder of its own for a test, empty, under the temporary directory of the run.
fn folder(name: &str) -> PathBuf {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("recorder-{name}"));
    let _ = std::fs::remove_dir_all(&folder);
    let _ = std::fs::remove_file(&folder);
    folder
}

/// A rig playing a sine on deck A, alone on the master, whose recordings are named `take` in
/// `folder`.
fn rig_recording_into(folder: &Path, sine: &str) -> Rig {
    let mut rig = Rig::with_deck_a(sine_file(sine, 440.0, 3.0, 0.5, RATE));
    let stem = folder.join("take");
    rig.engine
        .set_recording_names(Arc::new(move || stem.clone()));
    rig.send(Command::DeckPlay { deck: DeckId::A });
    rig
}

/// Polls the engine until `done` holds for its snapshot, and returns the events seen.
fn poll_until(rig: &mut Rig, done: impl Fn(&platter_engine::Snapshot) -> bool) -> Vec<Event> {
    let deadline = Instant::now() + WAIT;
    let mut events = Vec::new();
    loop {
        events.extend(rig.engine.poll());
        if done(&rig.engine.snapshot()) {
            return events;
        }
        assert!(Instant::now() < deadline, "the recorder did not answer");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn recorded(events: &[Event]) -> Vec<(String, u64)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Recorded { path, frames } => {
                Some((path.file_name()?.to_string_lossy().into_owned(), *frames))
            }
            _ => None,
        })
        .collect()
}

fn samples_of(path: &Path) -> Vec<f32> {
    let audio = decode_file(path).unwrap();
    assert_eq!((audio.sample_rate.hz(), audio.channels), (RATE, 2));
    audio.samples
}

/// The note, *Keys*: a cut closes one file and opens the next with the frame after the last
/// one of the first, and the two hold exactly what the processor sent to the master.
#[test]
fn a_cut_splits_the_recording_without_losing_or_doubling_a_frame() {
    let folder = folder("cut");
    let mut rig = rig_recording_into(&folder, "recorder-cut");
    rig.run_stereo(0.1);
    rig.send(Command::RecorderToggle);
    assert!(rig.engine.snapshot().recorder.recording);
    let mut sent = rig.run_stereo(0.5);
    rig.send(Command::RecorderCut);
    // Once the second file is open, whatever comes next goes into it.
    let second = folder.join("take-2.wav");
    let mut events = poll_until(&mut rig, |snapshot| {
        snapshot.recorder.file.as_deref() == Some(second.as_path())
    });
    sent.extend(rig.run_stereo(0.3));
    rig.send(Command::RecorderToggle);
    assert!(!rig.engine.snapshot().recorder.recording);
    // The block after the stop marks the end and is not recorded.
    rig.run_stereo(0.01);
    events.extend(rig.engine.finish_recording(WAIT));

    let first = (0.5 * f64::from(RATE)) as u64;
    let rest = (0.3 * f64::from(RATE)) as u64;
    assert_eq!(
        recorded(&events),
        [
            ("take.wav".to_owned(), first),
            ("take-2.wav".to_owned(), rest)
        ]
    );
    let mut heard = samples_of(&folder.join("take.wav"));
    heard.extend(samples_of(&second));
    assert_eq!(heard.len(), sent.len());
    assert!(heard == sent, "the files differ from the output");
    assert!(
        sent.iter().any(|sample| sample.abs() > 0.1),
        "silence was recorded"
    );

    let recorder = rig.engine.snapshot().recorder;
    assert_eq!(recorder.seconds, 0.8);
    assert_eq!(recorder.dropped_frames, 0);
    assert_eq!((recorder.file, recorder.failure), (None, None));
    std::fs::remove_dir_all(&folder).unwrap();
}

/// The note, *Problems*: a folder that cannot be created stops the recording with its reason,
/// and the recorder can start again afterwards.
#[test]
fn a_folder_that_cannot_be_made_stops_the_recording_and_says_why() {
    let blocked = folder("blocked");
    std::fs::write(&blocked, b"a file where the folder should be").unwrap();
    let mut rig = rig_recording_into(&blocked, "recorder-blocked");
    rig.send(Command::RecorderToggle);
    rig.run_stereo(0.05);
    let events = poll_until(&mut rig, |snapshot| !snapshot.recorder.recording);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::RecordingFailed { .. })),
        "{events:?}"
    );
    let failure = rig.engine.snapshot().recorder.failure.unwrap();
    assert!(failure.contains("take"), "{failure}");
    rig.run_stereo(0.05);
    rig.engine.finish_recording(WAIT);
    // The queue is back: a new recording starts, and fails the same way.
    rig.send(Command::RecorderToggle);
    assert!(rig.engine.snapshot().recorder.recording);
    assert_eq!(rig.engine.snapshot().recorder.failure, None);
    rig.run_stereo(0.05);
    poll_until(&mut rig, |snapshot| snapshot.recorder.failure.is_some());
    std::fs::remove_file(&blocked).unwrap();
}

/// The note, *Edge cases*: no folder, Cut with nothing recorded, Rec while finishing.
#[test]
fn rec_is_refused_without_a_folder_and_while_the_last_recording_finishes() {
    let mut rig = Rig::new();
    assert!(matches!(
        rig.engine.dispatch(Command::RecorderToggle),
        Err(EngineError::NoRecordingFolder)
    ));
    rig.send(Command::RecorderCut);
    assert_eq!(
        rig.engine.snapshot().recorder,
        platter_engine::RecorderSnapshot::default()
    );

    let folder = folder("busy");
    let mut rig = rig_recording_into(&folder, "recorder-busy");
    rig.send(Command::RecorderToggle);
    rig.run_stereo(0.05);
    rig.send(Command::RecorderToggle);
    // The processor has not marked the end yet.
    assert!(matches!(
        rig.engine.dispatch(Command::RecorderToggle),
        Err(EngineError::Busy)
    ));
    rig.run_stereo(0.01);
    let events = rig.engine.finish_recording(WAIT);
    assert_eq!(recorded(&events), [("take.wav".to_owned(), 2_400)]);
    rig.send(Command::RecorderToggle);
    assert!(rig.engine.snapshot().recorder.recording);
    rig.send(Command::RecorderToggle);
    rig.run_stereo(0.01);
    let events = rig.engine.finish_recording(WAIT);
    assert_eq!(recorded(&events), [("take-2.wav".to_owned(), 0)]);
    std::fs::remove_dir_all(&folder).unwrap();
}

/// The note, *Edge cases*: an engine dropped while it records, as a change of sample rate
/// drops it, still finishes its file with what the processor sent.
#[test]
fn a_dropped_engine_still_finishes_its_file() {
    let folder = folder("dropped");
    let mut rig = rig_recording_into(&folder, "recorder-dropped");
    rig.send(Command::RecorderToggle);
    let sent = rig.run_stereo(0.2);
    let Rig { engine, processor } = rig;
    drop(engine);
    let file = folder.join("take.wav");
    let deadline = Instant::now() + WAIT;
    // The worker finishes the header last.
    while !decode_file(&file).is_ok_and(|audio| audio.samples.len() >= sent.len()) {
        assert!(Instant::now() < deadline, "the file was not finished");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(samples_of(&file) == sent);
    drop(processor);
    std::fs::remove_dir_all(&folder).unwrap();
}

/// The note, *Stop*: the application stops the recording when it closes, while the device
/// still plays, and waits for the file.
#[test]
fn closing_finishes_the_file_while_the_device_plays() {
    let folder = folder("closing");
    let rig = rig_recording_into(&folder, "recorder-closing");
    let Rig {
        mut engine,
        mut processor,
    } = rig;
    engine.dispatch(Command::RecorderToggle).unwrap();
    let playing = AtomicBool::new(true);
    let blocks = AtomicUsize::new(0);
    let events = std::thread::scope(|scope| {
        scope.spawn(|| {
            let mut block = vec![0.0f32; 256 * 2];
            while playing.load(Ordering::Relaxed) {
                processor.process(&mut block, 2);
                blocks.fetch_add(1, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        // Some blocks reach the recording before it is stopped, however slow the machine:
        // a fixed wait left the file empty on a busy runner.
        let deadline = Instant::now() + WAIT;
        while blocks.load(Ordering::Relaxed) < 20 {
            assert!(Instant::now() < deadline, "the device did not play");
            std::thread::sleep(Duration::from_millis(2));
        }
        let events = engine.finish_recording(WAIT);
        playing.store(false, Ordering::Relaxed);
        events
    });
    let files = recorded(&events);
    assert_eq!(files.len(), 1, "{events:?}");
    assert!(files[0].1 > 0);
    assert!(!engine.snapshot().recorder.recording);
    let frames = samples_of(&folder.join("take.wav")).len() as u64 / 2;
    assert_eq!(frames, files[0].1);
    std::fs::remove_dir_all(&folder).unwrap();
}

/// The note, *Split* and *Preferences*: the split size set before a recording closes its
/// first file at that size and goes on in the next one without losing a frame.
#[test]
fn the_split_size_set_closes_a_file_at_that_size() {
    let folder = folder("split-size");
    let mut rig = rig_recording_into(&folder, "recorder-split-size");
    rig.engine
        .set_recording_split(platter_engine::MIN_SPLIT_BYTES);
    rig.send(Command::RecorderToggle);
    let sent = rig.run_stereo(2.8);
    rig.send(Command::RecorderToggle);
    rig.run_stereo(0.01);
    let events = rig.engine.finish_recording(WAIT);

    // Eight bytes a frame: two channels of 32-bit floats.
    let split = platter_engine::MIN_SPLIT_BYTES / 8;
    let total = (sent.len() / 2) as u64;
    assert_eq!(
        recorded(&events),
        [
            ("take.wav".to_owned(), split),
            ("take-2.wav".to_owned(), total - split)
        ]
    );
    std::fs::remove_dir_all(&folder).unwrap();
}
