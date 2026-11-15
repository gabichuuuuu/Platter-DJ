//! What a key-locked deck asks of a time stretch: the track at a tempo, at its own pitch.
//!
//! Behaviour note: `docs/behavior/keylock.md`; the choice of the stretch behind it:
//! `docs/decisions/0021-key-lock-stretch.md`.

use crate::Frame;

/// Plays one track at a tempo without changing its pitch. It is built for that track off the
/// audio thread and from then on runs inside the audio callback: no call allocates, locks or
/// takes unbounded time.
///
/// `track` is the whole track as interleaved stereo samples, the same slice on every call.
pub trait Stretch: Send {
    /// Starts again at `frame` of the track. Nothing is ready until [`Self::prepare`] has done
    /// the work for it.
    fn seek(&mut self, frame: f64);

    /// Does one bounded step of the work the next frame needs and says whether it is ready.
    fn prepare(&mut self, track: &[f32]) -> bool;

    /// The next frame at `tempo`, a ratio of the original speed, with the frame of the track
    /// it plays. `None` when it is not ready, past the end of the track or at a tempo it
    /// cannot play.
    fn next(&mut self, track: &[f32], tempo: f64) -> Option<(Frame, f64)>;
}
