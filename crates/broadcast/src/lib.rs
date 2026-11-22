//! Broadcasting of Platter: the mix as an Ogg Opus stream for a streaming server the user
//! configures. This crate turns stereo audio at the rate of the engine into the bytes of the
//! stream; sending them is not its business yet.
//!
//! Behaviour note: `docs/behavior/broadcasting.md`; the choices:
//! `docs/decisions/0046-broadcast-encoding.md`. The Opus library is confined to the module
//! `stream`: none of its types appears in this interface.

mod ogg;
mod resample;
mod stream;

pub use stream::{BITRATES, EncodeError, OPUS_RATE, OpusStream};
