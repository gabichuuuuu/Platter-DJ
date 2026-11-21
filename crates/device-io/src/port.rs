//! The MIDI inputs of the system, through `midir`. No type of `midir` leaves this module.
//!
//! On every backend `midir` calls back once per message, from a thread of its own or of the
//! system. System exclusive, clock and active sensing messages are ignored at the port.

use std::fmt;

use midir::{ConnectErrorKind, Ignore, MidiInput, MidiInputConnection};

use crate::queue::{Messages, queue};

/// Name Platter gives itself as a client of the MIDI system.
const CLIENT: &str = "Platter";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortError {
    /// The MIDI system of the computer cannot be used.
    System,
    /// No input has that name now.
    NoSuchPort(String),
    /// The input exists but cannot be opened, for instance because another program holds it.
    Busy(String),
    /// An open port stopped taking messages, usually because its device went away.
    Gone(String),
}

impl fmt::Display for PortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System => f.write_str("the MIDI system cannot be used"),
            Self::NoSuchPort(name) => write!(f, "no MIDI port is called `{name}`"),
            Self::Busy(name) => write!(f, "the MIDI port `{name}` cannot be opened"),
            Self::Gone(name) => write!(f, "the MIDI port `{name}` went away"),
        }
    }
}

impl std::error::Error for PortError {}

fn client() -> Result<MidiInput, PortError> {
    let mut input = MidiInput::new(CLIENT).map_err(|_| PortError::System)?;
    input.ignore(Ignore::All);
    Ok(input)
}

/// The names of the MIDI inputs, in the order the system gives them. A port whose name
/// cannot be read is left out.
pub fn inputs() -> Result<Vec<String>, PortError> {
    let input = client()?;
    Ok(input
        .ports()
        .iter()
        .filter_map(|port| input.port_name(port).ok())
        .collect())
}

/// An open MIDI input. It closes when dropped.
pub struct Input {
    name: String,
    _connection: MidiInputConnection<()>,
}

impl Input {
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Debug for Input {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Input").field("name", &self.name).finish()
    }
}

/// Opens the first input called `name`. Its messages go to a queue of `capacity` messages;
/// those that arrive while it is full are counted and lost. `wake` is called, on the thread of
/// the port, after each message that enters the queue, so whoever reads it need not ask
/// again and again; it must return at once.
pub fn open(
    name: &str,
    capacity: usize,
    wake: impl Fn() + Send + 'static,
) -> Result<(Input, Messages), PortError> {
    let input = client()?;
    let port = input
        .ports()
        .into_iter()
        .find(|port| input.port_name(port).is_ok_and(|found| found == name))
        .ok_or_else(|| PortError::NoSuchPort(name.to_owned()))?;
    let (sender, messages) = queue(capacity);
    let connection = input
        .connect(
            &port,
            CLIENT,
            move |_, bytes, _| {
                if sender.deliver(bytes) {
                    wake();
                }
            },
            (),
        )
        .map_err(|error| match error.kind() {
            ConnectErrorKind::InvalidPort => PortError::NoSuchPort(name.to_owned()),
            ConnectErrorKind::Other(_) => PortError::Busy(name.to_owned()),
        })?;
    Ok((
        Input {
            name: name.to_owned(),
            _connection: connection,
        },
        messages,
    ))
}
