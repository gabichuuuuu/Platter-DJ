//! The stretch of key lock, from the crate timestretch, behind the trait of `platter-dsp`.
//!
//! Behaviour note: `docs/behavior/keylock.md`; the choice and its measurement:
//! `docs/decisions/0021-key-lock-stretch.md`. This is the only module that names timestretch.

use platter_core::SampleRate;
use platter_dsp::Frame;
use platter_dsp::stretch::Stretch;
use timestretch::{
    Backend, OutputStamp, Parameters, PitchRatio, PreparedSession, ProcessStatus, RatioConstraint,
    SessionConfig, SourceAccess, StreamSession, TempoRate,
};

/// Frames rendered at a time. A tempo change waits at most this long to be scheduled; the
/// stretch itself takes about 500 frames to apply one.
const CHUNK: usize = 64;
/// Steps of work one chunk may take in steady play, twice the most measured at 0.92, 1 and
/// 1.08. A chunk that needs more is one after a seek, and [`Stretch::prepare`] spreads that
/// work instead.
const STEPS_PER_CHUNK: usize = 32;
/// Smallest change of tempo or pitch worth a command to the session.
const TEMPO_EPSILON: f64 = 1e-9;
/// Bounds of the pitch over the tempo the session takes (`Parameters::validate`).
const MIN_PITCH_OVER_TEMPO: f64 = 0.25;
const MAX_PITCH_OVER_TEMPO: f64 = 32.0;
/// Frames of the track the session takes at a time (`SOURCE_CHUNK` in its source). A change
/// of pitch while it plays leaves the position it reports this many frames times
/// tempo * (1 / new pitch - 1 / old pitch) beyond the frame it plays, until the next seek:
/// measured from -12 to +12 semitones at tempos 0.94 to 1.08, to within 25 frames
/// (`docs/decisions/0028-transposition.md`).
const SOURCE_CHUNK_FRAMES: f64 = 2048.0;

/// A timestretch session for one track, at the rate of the engine.
pub struct TrackStretch {
    session: StreamSession,
    /// Interleaved output of the chunk being played.
    chunk: [f32; CHUNK * 2],
    stamp: OutputStamp,
    /// Frames of the chunk already played.
    taken: usize,
    /// Tempo and pitch last scheduled, within the bounds of the session.
    tempo: f64,
    pitch: f64,
    /// How far the position the session reports is beyond the frame it plays.
    position_error: f64,
    commands: u64,
    /// The session failed or the track ended: nothing more comes out until a seek.
    stopped: bool,
}

impl TrackStretch {
    /// The stretch of a track of `frames` frames at `track_rate`, played at `engine_rate`.
    /// `None` when timestretch does not take the format: a track below 32 kHz, for one.
    pub fn new(frames: usize, track_rate: SampleRate, engine_rate: SampleRate) -> Option<Self> {
        let session = PreparedSession::new(SessionConfig {
            channels: 2,
            source_sample_rate: track_rate.hz(),
            output_sample_rate: engine_rate.hz(),
            backend: Backend::Portable,
            source_access: SourceAccess::Seekable,
            source_frames: Some(u64::try_from(frames).ok()?),
            parameters: parameters(1.0, 1.0)?,
            // The least it takes: the deck collects each chunk at once.
            max_inflight_frames: 65_536,
            ..SessionConfig::default()
        })
        .ok()?
        .activate();
        Some(Self {
            session,
            chunk: [0.0; CHUNK * 2],
            stamp: OutputStamp::default(),
            taken: 0,
            tempo: 1.0,
            pitch: 1.0,
            position_error: 0.0,
            commands: 0,
            stopped: true,
        })
    }

    /// One step of the session, feeding it from the track when it asks.
    fn step(&mut self, track: &[f32]) {
        let report = self.session.advance(1);
        match report.status {
            ProcessStatus::NeedSource => {
                let fed = self.session.source_request().and_then(|request| {
                    let start = usize::try_from(request.start_frame).ok()?.checked_mul(2)?;
                    let end = start.checked_add(request.frames.checked_mul(2)?)?;
                    let samples = track.get(start..end)?;
                    self.session.supply(request, samples).ok()
                });
                if fed.is_none() {
                    self.stopped = true;
                }
            }
            ProcessStatus::NeedAcknowledgement => while self.session.next_applied().is_some() {},
            ProcessStatus::Fault | ProcessStatus::Finished | ProcessStatus::Rendered => {
                self.stopped = true;
            }
            ProcessStatus::Priming
            | ProcessStatus::Progress
            | ProcessStatus::NeedOutput
            | ProcessStatus::Paused => {}
        }
    }

    fn ready(&self) -> bool {
        self.session.output_ready() >= CHUNK
    }

    /// Collects the next chunk, scheduling `tempo` and `pitch` first when they changed.
    fn refill(&mut self, track: &[f32], tempo: f64, pitch: f64) -> bool {
        let Some(asked) = parameters(tempo, pitch) else {
            return false;
        };
        let (tempo, pitch) = (asked.tempo.get(), asked.pitch.get());
        if (tempo - self.tempo).abs() > TEMPO_EPSILON || (pitch - self.pitch).abs() > TEMPO_EPSILON
        {
            self.commands += 1;
            // Frame 0 is always due, so the change applies at the next boundary of the
            // session. A full queue of commands keeps the tempo before; the next chunk asks
            // again.
            if self.session.schedule(self.commands, 0, asked).is_ok() {
                // The session glides to the new pitch over about 400 ms; the position it
                // reports moves away from what it plays as it does. Taken off at once, the
                // position is off by less than the full amount until the glide ends.
                self.position_error +=
                    SOURCE_CHUNK_FRAMES * tempo * (1.0 / pitch - 1.0 / self.pitch);
                self.tempo = tempo;
                self.pitch = pitch;
            }
        }
        let mut steps = 0;
        while !self.ready() && !self.stopped && steps < STEPS_PER_CHUNK {
            self.step(track);
            steps += 1;
        }
        while self.session.next_applied().is_some() {}
        if !self.ready() {
            return false;
        }
        let Ok(stamp) = self.session.read_output(&mut self.chunk) else {
            self.stopped = true;
            return false;
        };
        self.stamp = stamp;
        self.taken = 0;
        stamp.frames > 0
    }
}

impl Stretch for TrackStretch {
    fn seek(&mut self, frame: f64, tempo: f64, pitch: f64) {
        let target = if frame.is_finite() {
            frame.max(0.0).round() as u64
        } else {
            0
        };
        // A target past the end is refused; the deck then plays without key lock.
        self.stopped = self.session.seek(target).is_err();
        // The session starts again with the parameters it last applied, and glides from them
        // to new ones over about 400 ms. Applied first, the tempo and the pitch of the deck
        // are there from the first frame: the first step after the seek applies what was
        // scheduled, before any work, and a second seek starts from them.
        let applied = self.session.applied_parameters().requested;
        let changed = (applied.tempo.get() - tempo).abs() > TEMPO_EPSILON
            || (applied.pitch.get() - pitch).abs() > TEMPO_EPSILON;
        if !self.stopped
            && changed
            && let Some(asked) = parameters(tempo, pitch)
        {
            self.commands += 1;
            if self.session.schedule(self.commands, 0, asked).is_ok() {
                self.session.advance(1);
                while self.session.next_applied().is_some() {}
                self.stopped = self.session.seek(target).is_err();
            }
        }
        let applied = self.session.applied_parameters().requested;
        self.tempo = applied.tempo.get();
        self.pitch = applied.pitch.get();
        self.position_error = 0.0;
        self.stamp = OutputStamp::default();
        self.taken = 0;
    }

    fn prepare(&mut self, track: &[f32]) -> bool {
        if !self.ready() && !self.stopped {
            self.step(track);
        }
        self.ready() && !self.stopped
    }

    fn next(&mut self, track: &[f32], tempo: f64, pitch: f64) -> Option<(Frame, f64)> {
        if self.stopped || !tempo.is_finite() || !pitch.is_finite() {
            return None;
        }
        if self.taken >= self.stamp.frames && !self.refill(track, tempo, pitch) {
            return None;
        }
        let index = self.taken;
        self.taken += 1;
        // The receipt of each frame moves the position of the session to that frame.
        let receipt = self.stamp.consumed(self.taken).ok()?;
        self.session.acknowledge(receipt).ok()?;
        let frame = [self.chunk[index * 2], self.chunk[index * 2 + 1]];
        Some((frame, self.session.position() - self.position_error))
    }
}

/// Key lock at `tempo` and `pitch`, with the tempo exact and the pitch free to move by the few
/// cents that keep it so: a synced deck has to stay on the beat. A pitch the session does not
/// take against that tempo is brought within its bounds.
fn parameters(tempo: f64, pitch: f64) -> Option<Parameters> {
    let pitch = pitch.clamp(tempo * MIN_PITCH_OVER_TEMPO, tempo * MAX_PITCH_OVER_TEMPO);
    let asked = Parameters {
        tempo: TempoRate::new(tempo).ok()?,
        pitch: PitchRatio::new(pitch).ok()?,
        constraint: RatioConstraint::PreserveDuration,
    };
    asked.validate().ok().map(|()| asked)
}
