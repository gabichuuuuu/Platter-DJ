//! MIDI ports of Platter: the inputs of the system and the messages they send.
//!
//! Behaviour note: `docs/behavior/controllers.md`, section *Ports*. The MIDI library,
//! `midir`, is confined to the `port` module: none of its types appears in this interface,
//! so replacing it touches that file only. What the messages mean is `platter-control`'s
//! business; this crate hands them over as bytes.

mod port;
mod queue;

pub use port::{Input, PortError, inputs, open};
pub use queue::{MAX_MESSAGE_BYTES, Messages, RawMessage};
