//! The MIDI outputs of the system, through `midir`, for the MIDI clock. No type of `midir`
//! leaves this module.
//!
//! Behaviour note: `docs/behavior/midi-clock.md`; design: `docs/decisions/0041-midi-clock.md`.

use std::fmt;

use midir::{ConnectErrorKind, MidiOutput, MidiOutputConnection};

use crate::port::PortError;

/// Name Platter gives itself as a client of the MIDI system.
const CLIENT: &str = "Platter";

fn client() -> Result<MidiOutput, PortError> {
    MidiOutput::new(CLIENT).map_err(|_| PortError::System)
}

/// The names of the MIDI outputs, in the order the system gives them. A port whose name
/// cannot be read is left out.
pub fn outputs() -> Result<Vec<String>, PortError> {
    let output = client()?;
    Ok(output
        .ports()
        .iter()
        .filter_map(|port| output.port_name(port).ok())
        .collect())
}

/// An open MIDI output. It closes when dropped.
pub struct Output {
    name: String,
    connection: MidiOutputConnection,
}

impl Output {
    /// Opens the first output called `name`.
    pub fn open(name: &str) -> Result<Self, PortError> {
        let output = client()?;
        let port = output
            .ports()
            .into_iter()
            .find(|port| output.port_name(port).is_ok_and(|found| found == name))
            .ok_or_else(|| PortError::NoSuchPort(name.to_owned()))?;
        let connection = output
            .connect(&port, CLIENT)
            .map_err(|error| match error.kind() {
                ConnectErrorKind::InvalidPort => PortError::NoSuchPort(name.to_owned()),
                ConnectErrorKind::Other(_) => PortError::Busy(name.to_owned()),
            })?;
        Ok(Self {
            name: name.to_owned(),
            connection,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sends one message. A port that refuses it has usually gone away.
    pub fn send(&mut self, message: &[u8]) -> Result<(), PortError> {
        self.connection
            .send(message)
            .map_err(|_| PortError::Gone(self.name.clone()))
    }
}

impl fmt::Debug for Output {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Output").field("name", &self.name).finish()
    }
}
