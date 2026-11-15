//! Mappings of Platter: from the messages of MIDI controllers to commands.
//!
//! Behaviour note: `docs/behavior/controllers.md`; the choices:
//! `docs/decisions/0024-midi-mappings.md`. This crate does not know the commands of the
//! engine: it gives identifiers and untyped parameters, which the application turns into
//! commands and checks when it reads a mapping.

mod file;
mod message;

use std::collections::BTreeMap;

pub use file::{BindingProblem, MAX_BINDINGS, MAX_FILE_BYTES, MappingError};
pub use message::Message;

/// Modifiers of one mapping.
pub const MODIFIERS: usize = 8;
/// Highest value of a modifier.
pub const MAX_MODIFIER_VALUE: u8 = 7;

/// An untyped parameter, as the mapping file gives it.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Number(f64),
    Text(String),
}

pub type Args = BTreeMap<String, Arg>;

/// A command to send: its identifier and its parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct Emit {
    pub command: String,
    pub args: Args,
}

/// A control of the device. Channels count from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Note { channel: u8, note: u8 },
    ControlChange { channel: u8, number: u8 },
    PitchBend { channel: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// Steps up are 1 to 63, steps down 127 (one) to 65.
    TwosComplement,
    /// Steps up are 65 (one) and above, steps down 63 (one) and below.
    Offset,
}

#[derive(Clone, Debug, PartialEq)]
enum Action {
    Command {
        command: String,
        args: Args,
        /// The parameters that change when the button comes up.
        release: Option<Args>,
    },
    Modifier {
        index: usize,
        value: u8,
        hold: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Button,
    Fader {
        param: String,
        bottom: f64,
        top: f64,
        invert: bool,
    },
    Encoder {
        param: String,
        step: f64,
        encoding: Encoding,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct Binding {
    control: Control,
    kind: Kind,
    action: Action,
    /// Modifier and value that must hold, at most two.
    when: Vec<(usize, u8)>,
}

/// A mapping as read from its file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mapping {
    bindings: Vec<Binding>,
}

impl Mapping {
    pub fn parse(source: &str) -> Result<Self, MappingError> {
        file::parse(source).map(|bindings| Self { bindings })
    }

    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// The commands a binding may send, each once, in the order of the file, so the
    /// application can check them all when it reads the mapping.
    pub fn commands(&self) -> Vec<Emit> {
        let mut emits = Vec::new();
        for binding in &self.bindings {
            let Action::Command {
                command,
                args,
                release,
            } = &binding.action
            else {
                continue;
            };
            let mut press = args.clone();
            match &binding.kind {
                Kind::Button => {}
                Kind::Fader { param, bottom, .. } => {
                    press.insert(param.clone(), Arg::Number(*bottom));
                }
                Kind::Encoder { param, step, .. } => {
                    press.insert(param.clone(), Arg::Number(*step));
                }
            }
            emits.push(Emit {
                command: command.clone(),
                args: press,
            });
            if let Some(release) = release {
                emits.push(Emit {
                    command: command.clone(),
                    args: merged(args, release),
                });
            }
        }
        emits
    }
}

fn merged(args: &Args, over: &Args) -> Args {
    let mut args = args.clone();
    args.extend(
        over.iter()
            .map(|(name, value)| (name.clone(), value.clone())),
    );
    args
}

/// A mapping at work: the state of its modifiers and buttons.
#[derive(Clone, Debug)]
pub struct Mapper {
    mapping: Mapping,
    modifiers: [u8; MODIFIERS],
    /// Which buttons acted on their press and are still down.
    down: Vec<bool>,
}

impl Mapper {
    pub fn new(mapping: Mapping) -> Self {
        let down = vec![false; mapping.bindings.len()];
        Self {
            mapping,
            modifiers: [0; MODIFIERS],
            down,
        }
    }

    /// The values of the modifiers, modifier 1 first.
    pub fn modifiers(&self) -> [u8; MODIFIERS] {
        self.modifiers
    }

    /// The commands the message in `bytes` triggers, in the order of the file. Conditions are
    /// checked against the modifiers as they were before the message.
    pub fn handle(&mut self, bytes: &[u8]) -> Vec<Emit> {
        let Some(message) = Message::parse(bytes) else {
            return Vec::new();
        };
        let before = self.modifiers;
        let mut emits = Vec::new();
        for (index, binding) in self.mapping.bindings.iter().enumerate() {
            let Some(input) = input_of(binding.control, message) else {
                continue;
            };
            let holds = binding
                .when
                .iter()
                .all(|(modifier, value)| before[*modifier] == *value);
            match &binding.kind {
                Kind::Button => match input.button() {
                    Some(true) => {
                        if self.down[index] || !holds {
                            continue;
                        }
                        self.down[index] = true;
                        match &binding.action {
                            Action::Command { command, args, .. } => emits.push(Emit {
                                command: command.clone(),
                                args: args.clone(),
                            }),
                            Action::Modifier { index, value, .. } => {
                                self.modifiers[*index] = *value;
                            }
                        }
                    }
                    Some(false) => {
                        if !self.down[index] {
                            continue;
                        }
                        self.down[index] = false;
                        match &binding.action {
                            Action::Command {
                                command,
                                args,
                                release: Some(release),
                            } => emits.push(Emit {
                                command: command.clone(),
                                args: merged(args, release),
                            }),
                            Action::Modifier {
                                index, hold: true, ..
                            } => self.modifiers[*index] = 0,
                            _ => {}
                        }
                    }
                    None => {}
                },
                Kind::Fader {
                    param,
                    bottom,
                    top,
                    invert,
                } => {
                    let Some(position) = input.position().filter(|_| holds) else {
                        continue;
                    };
                    let position = if *invert { 1.0 - position } else { position };
                    emit_with(
                        &mut emits,
                        &binding.action,
                        param,
                        bottom + position * (top - bottom),
                    );
                }
                Kind::Encoder {
                    param,
                    step,
                    encoding,
                } => {
                    let steps = input
                        .raw()
                        .filter(|_| holds)
                        .map_or(0, |value| steps(value, *encoding));
                    if steps != 0 {
                        emit_with(&mut emits, &binding.action, param, f64::from(steps) * step);
                    }
                }
            }
        }
        emits
    }
}

/// Sends the command with `value` in `param`; a value that overflowed to infinity is not sent.
fn emit_with(emits: &mut Vec<Emit>, action: &Action, param: &str, value: f64) {
    if !value.is_finite() {
        return;
    }
    if let Action::Command { command, args, .. } = action {
        let mut args = args.clone();
        args.insert(param.to_owned(), Arg::Number(value));
        emits.push(Emit {
            command: command.clone(),
            args,
        });
    }
}

/// What a message carries for a control.
#[derive(Clone, Copy, Debug)]
enum Input {
    /// A note down or up.
    Note(bool),
    ControlChange(u8),
    PitchBend(u16),
}

impl Input {
    fn button(self) -> Option<bool> {
        match self {
            Self::Note(down) => Some(down),
            Self::ControlChange(value) => Some(value > 0),
            Self::PitchBend(_) => None,
        }
    }

    /// From 0 to 1.
    fn position(self) -> Option<f64> {
        match self {
            Self::Note(_) => None,
            Self::ControlChange(value) => Some(f64::from(value.min(127)) / 127.0),
            Self::PitchBend(value) => Some(bend_position(value)),
        }
    }

    fn raw(self) -> Option<u8> {
        match self {
            Self::ControlChange(value) => Some(value),
            _ => None,
        }
    }
}

fn input_of(control: Control, message: Message) -> Option<Input> {
    match (control, message) {
        (
            Control::Note { channel, note },
            Message::NoteOn {
                channel: on,
                note: pressed,
                velocity,
            },
        ) if channel == on && note == pressed => Some(Input::Note(velocity > 0)),
        (
            Control::Note { channel, note },
            Message::NoteOff {
                channel: on,
                note: released,
            },
        ) if channel == on && note == released => Some(Input::Note(false)),
        (
            Control::ControlChange { channel, number },
            Message::ControlChange {
                channel: on,
                number: moved,
                value,
            },
        ) if channel == on && number == moved => Some(Input::ControlChange(value)),
        (Control::PitchBend { channel }, Message::PitchBend { channel: on, value })
            if channel == on =>
        {
            Some(Input::PitchBend(value))
        }
        _ => None,
    }
}

/// The position of a pitch bend: its centre, 8 192, is exactly half way.
fn bend_position(value: u16) -> f64 {
    let value = f64::from(value.min(16_383));
    if value <= 8192.0 {
        value / 8192.0 * 0.5
    } else {
        0.5 + (value - 8192.0) / 8191.0 * 0.5
    }
}

fn steps(value: u8, encoding: Encoding) -> i32 {
    let value = i32::from(value);
    match encoding {
        Encoding::TwosComplement => match value {
            1..=63 => value,
            65..=127 => value - 128,
            _ => 0,
        },
        Encoding::Offset => match value {
            0 | 64 => 0,
            _ => value - 64,
        },
    }
}
