//! MIDI ports of Platter: the inputs of the system and the messages they send, and the
//! outputs the MIDI clock goes to.
//!
//! Behaviour notes: `docs/behavior/controllers.md`, section *Ports*, and
//! `docs/behavior/midi-clock.md`. The MIDI library, `midir`, is confined to the `port` and
//! `output` modules: none of its types appears in this interface, so replacing it touches
//! those files only. What the messages mean is `platter-control`'s business; this crate hands
//! them over as bytes.

mod output;
mod port;
mod queue;

pub use output::{Output, outputs};
pub use port::{Input, PortError, inputs, open};
pub use queue::{MAX_MESSAGE_BYTES, Messages, RawMessage};
