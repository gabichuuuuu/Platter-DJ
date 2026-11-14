//! Shared by the engine tests: synthesised tracks on disk and an engine driven by hand.

// Each test binary uses a different part of this module.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

use std::path::PathBuf;

use platter_core::{DeckId, SampleRate};
use platter_engine::{Command, Engine, Event, Processor};
use platter_testkit::files::wav_pcm16;
use platter_testkit::signal::{channel, sine, stereo, to_i16};

pub const RATE: u32 = 48_000;
const BLOCK_FRAMES: usize = 256;

pub fn rate() -> SampleRate {
    SampleRate::new(RATE).unwrap()
}

/// Writes a stereo sine as a WAV file in the temporary directory of the test run. `name`
/// must be unique among the tests, which run in parallel.
pub fn sine_file(name: &str, hz: f64, seconds: f64, amplitude: f32, file_rate: u32) -> PathBuf {
    let frames = (seconds * f64::from(file_rate)).round() as usize;
    let samples = to_i16(&stereo(&sine(hz, file_rate, frames, amplitude)));
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.wav"));
    std::fs::write(&path, wav_pcm16(file_rate, 2, &samples)).unwrap();
    path
}

/// An engine and its processor, with the test standing in for the audio device.
pub struct Rig {
    pub engine: Engine,
    pub processor: Processor,
}

impl Rig {
    pub fn new() -> Self {
        let (engine, processor) = Engine::new(rate());
        Self { engine, processor }
    }

    /// A rig with deck A alone on the master: the crossfader fully to its side.
    pub fn with_deck_a(path: PathBuf) -> Self {
        let mut rig = Self::new();
        rig.send(Command::MixerCrossfaderSet { position: 0.0 });
        rig.load(DeckId::A, path);
        rig
    }

    pub fn send(&mut self, command: Command) {
        self.engine.dispatch(command).unwrap();
    }

    pub fn load(&mut self, deck: DeckId, path: PathBuf) {
        self.send(Command::DeckLoad { deck, path });
        let events = self.engine.wait_for_loads();
        assert!(
            matches!(events.as_slice(), [Event::Loaded { .. }]),
            "{events:?}"
        );
    }

    /// Processes `seconds` of audio and returns it as interleaved stereo.
    pub fn run_stereo(&mut self, seconds: f64) -> Vec<f32> {
        self.run_channels(seconds, 2)
    }

    /// Processes `seconds` of audio for a device with `channels` outputs.
    pub fn run_channels(&mut self, seconds: f64, channels: usize) -> Vec<f32> {
        let frames = (seconds * f64::from(RATE)).round() as usize;
        let mut output = vec![0.0; frames * channels];
        for block in output.chunks_mut(BLOCK_FRAMES * channels) {
            self.processor.process(block, channels);
        }
        output
    }

    /// Processes `seconds` of audio and returns the left channel.
    pub fn run(&mut self, seconds: f64) -> Vec<f32> {
        channel(&self.run_stereo(seconds), 2, 0)
    }
}
