//! Stem separation of Platter: a track into drums, bass, other and vocals.
//!
//! Behaviour note: `docs/behavior/stems.md`, *Separation*; the choices:
//! `docs/decisions/0038-stem-separation.md`. The crate holds the inference only: the transform,
//! the networks and the masks, the file of weights of Platter and the reader of the original
//! archive of the model. It takes and gives PCM; decoding and writing files are for its callers.

mod bundle;
mod fft;
mod inflate;
mod install;
pub mod net;
mod sha256;
mod spleeter;
pub mod weights;

use platter_core::Stem;

pub use install::{InstallError, SPLEETER_SHA256, SPLEETER_URL, install_spleeter, load_spleeter};
pub use spleeter::{RATE as SPLEETER_RATE, Spleeter};
pub use weights::WeightsError;

/// Why a separation did not finish.
#[derive(Debug, PartialEq, Eq)]
pub enum SeparationError {
    /// The progress callback asked to stop.
    Cancelled,
    /// The weights do not fit the network.
    Weights,
}

impl std::fmt::Display for SeparationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Cancelled => "the separation was cancelled",
            Self::Weights => "the weights of the model do not fit its network",
        })
    }
}

impl std::error::Error for SeparationError {}

/// A model that separates a track into its four stems. Spleeter is the one Platter runs
/// today; a model trained later takes its place behind this trait.
pub trait Separator: Send + Sync {
    /// The sample rate the model takes and gives.
    fn rate(&self) -> u32;

    /// Separates interleaved stereo at [`Self::rate`]. The samples of each stem go to `sink`
    /// as they are finished, in order, interleaved stereo, until each stem has as many as the
    /// input. `progress` gets the share done, from 0 to 1, and stops the work when it returns
    /// `false`.
    fn separate(
        &self,
        stereo: &[f32],
        sink: &mut dyn FnMut(Stem, &[f32]),
        progress: &mut dyn FnMut(f32) -> bool,
    ) -> Result<(), SeparationError>;
}
