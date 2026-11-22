//! MIDI ports and HID devices of Platter: the inputs of the system and the messages they send,
//! the outputs the MIDI clock goes to, and the input reports of HID devices.
//!
//! Behaviour notes: `docs/behavior/controllers.md`, section *Ports*,
//! `docs/behavior/midi-clock.md` and `docs/behavior/hid-controllers.md`. The MIDI library,
//! `midir`, is confined to the `port` and `output` modules, and the HID library, `async-hid`,
//! with `futures-lite`, to the `hid` module: none of their types appears in this interface, so
//! replacing one touches those files only. What the messages and reports mean is
//! `platter-control`'s business; this crate hands them over as bytes.

mod hid;
mod output;
mod port;
mod queue;

pub use hid::{HidDevice, HidError, HidInput, MAX_REPORT_BYTES, Reports, hid_devices, open_hid};
pub use output::{Output, outputs};
pub use port::{ClockMessages, Input, PortError, inputs, open, open_clock};
pub use queue::{MAX_MESSAGE_BYTES, Messages, RawMessage};
