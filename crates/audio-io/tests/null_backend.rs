//! The output driven without a sound card: block sizes, routing and the clocked output.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use platter_audio_io::{ManualStream, Routing, Source, silent};

/// A source that writes a running count: channel `c` of frame `n` holds `n + c / 10`. It
/// also records the largest block it was asked for and how many channels.
struct Counter {
    frame: usize,
    largest_block: Arc<AtomicUsize>,
    channels_seen: Arc<AtomicUsize>,
}

fn counting_source() -> (Source, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let largest = Arc::new(AtomicUsize::new(0));
    let channels_seen = Arc::new(AtomicUsize::new(0));
    let mut counter = Counter {
        frame: 0,
        largest_block: Arc::clone(&largest),
        channels_seen: Arc::clone(&channels_seen),
    };
    let source: Source = Box::new(move |output, channels| {
        counter.channels_seen.store(channels, Ordering::Relaxed);
        let frames = output.len() / channels;
        counter.largest_block.fetch_max(frames, Ordering::Relaxed);
        for frame in output.chunks_mut(channels) {
            for (channel, sample) in frame.iter_mut().enumerate() {
                *sample = counter.frame as f32 + channel as f32 / 10.0;
            }
            counter.frame += 1;
        }
    });
    (source, largest, channels_seen)
}

#[test]
fn callbacks_of_any_size_get_a_continuous_stream() {
    let (source, largest, _) = counting_source();
    let mut stream = ManualStream::new(source, 48_000, 2, Routing::default());
    let mut expected = 0usize;
    // Sizes a device may ask for: tiny, odd, the usual ones, larger than the scratch
    // buffer, and nothing at all.
    for frames in [1, 7, 64, 256, 0, 480, 1_024, 1_025, 3_000, 5, 4_096, 333] {
        let block = stream.pump(frames).to_vec();
        assert_eq!(block.len(), frames * 2);
        for frame in block.chunks(2) {
            assert_eq!(frame, [expected as f32, expected as f32 + 0.1]);
            expected += 1;
        }
    }
    // A large callback is served in pieces; the source never sees more than one piece.
    assert_eq!(largest.load(Ordering::Relaxed), 1_024);
}

#[test]
fn a_two_output_device_gets_the_master_alone() {
    let (source, _, channels_seen) = counting_source();
    let mut stream = ManualStream::new(source, 48_000, 2, Routing::default());
    let block = stream.pump(4).to_vec();
    assert_eq!(channels_seen.load(Ordering::Relaxed), 2);
    assert_eq!(block[..4], [0.0, 0.1, 1.0, 1.1]);
}

#[test]
fn a_four_output_device_gets_master_and_headphones_where_the_routing_says() {
    let (source, _, channels_seen) = counting_source();
    let routing = Routing {
        master: [2, 3],
        headphones: [1, 0],
    };
    let mut stream = ManualStream::new(source, 48_000, 6, routing);
    let block = stream.pump(2).to_vec();
    assert_eq!(channels_seen.load(Ordering::Relaxed), 4);
    // Source channels: master left .0, master right .1, headphones left .2, right .3.
    // Outputs five and six carry nothing.
    assert_eq!(block[..6], [0.3, 0.2, 0.0, 0.1, 0.0, 0.0]);
    assert_eq!(block[6..], [1.3, 1.2, 1.0, 1.1, 0.0, 0.0]);
}

#[test]
fn headphones_need_two_free_outputs() {
    assert!(Routing::default().has_headphones(4));
    assert!(!Routing::default().has_headphones(3));
    assert!(!Routing::default().has_headphones(2));
    let shared = Routing {
        master: [0, 1],
        headphones: [1, 2],
    };
    assert!(!shared.has_headphones(8));
    let same = Routing {
        master: [0, 1],
        headphones: [2, 2],
    };
    assert!(!same.has_headphones(8));
    // Without headphones the source is asked for the master only, and a master routed
    // beyond the device is dropped instead of written out of bounds.
    let (source, _, channels_seen) = counting_source();
    let routing = Routing {
        master: [0, 5],
        headphones: [2, 3],
    };
    let mut stream = ManualStream::new(source, 48_000, 3, routing);
    let block = stream.pump(1).to_vec();
    assert_eq!(channels_seen.load(Ordering::Relaxed), 2);
    assert_eq!(block, [0.0, 0.0, 0.0]);
}

#[test]
fn the_manual_stream_counts_callbacks_that_take_too_long() {
    let slow: Source = Box::new(|output, _| {
        std::thread::sleep(Duration::from_millis(20));
        output.fill(0.0);
    });
    let mut stream = ManualStream::new(slow, 48_000, 2, Routing::default());
    let stats = stream.stats();
    // 64 frames at 48 kHz last 1.3 ms; the source takes 20 ms.
    stream.pump(64);
    assert_eq!(stats.dropouts(), 1);
    assert!(stats.load() > 1.0);
    stats.reset_dropouts();
    assert_eq!(stats.dropouts(), 0);
}

#[test]
fn the_silent_output_runs_the_source_at_the_pace_of_a_device_and_gives_it_back() {
    let (source, _, _) = counting_source();
    let produced = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&produced);
    let mut inner = source;
    let counted: Source = Box::new(move |output, channels| {
        inner(output, channels);
        seen.fetch_add(output.len() / channels, Ordering::Relaxed);
    });
    let started = Instant::now();
    let output = silent(counted, 48_000).unwrap();
    assert!(!output.is_audible());
    assert_eq!(output.info().sample_rate, 48_000);
    assert!(!output.info().headphones);
    std::thread::sleep(Duration::from_millis(300));
    let mut source = output.stop().unwrap();
    let frames = produced.load(Ordering::Relaxed);
    let expected = started.elapsed().as_secs_f64() * 48_000.0;
    // A clock driven by sleep is coarse; the point is that it neither stalls nor races.
    assert!(
        (frames as f64) > expected * 0.5 && (frames as f64) < expected * 1.5,
        "{frames} frames in {expected} expected"
    );
    // The source that comes back is the same one: it goes on counting where it stopped.
    let mut block = [0.0f32; 2];
    source(&mut block, 2);
    assert_eq!(block[0] as usize, frames);
}
