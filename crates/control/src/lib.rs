//! Mappings of Platter: from the messages of MIDI controllers and the reports of HID devices to
//! commands.
//!
//! Behaviour notes: `docs/behavior/controllers.md` and `docs/behavior/hid-controllers.md`; the
//! choices:
//! `docs/decisions/0024-midi-mappings.md`. This crate does not know the commands of the
//! engine: it gives identifiers and untyped parameters, which the application turns into
//! commands and checks when it reads a mapping.

mod file;
mod message;

use std::collections::BTreeMap;

/// Longest HID report read; bytes after it are left out.
pub const MAX_REPORT_BYTES: usize = 1024;

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
    Hid(HidControl),
}

/// A part of the input reports of a HID device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HidControl {
    /// The report ID; 0 for a device that numbers no report.
    pub report: u8,
    /// From 0 at the first byte after the report ID, or at the first byte of a report without one.
    pub byte: usize,
    pub part: HidPart,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HidPart {
    /// One bit of the byte, 0 the lowest.
    Bit(u8),
    Byte,
    /// The byte and the next one, little-endian.
    Word,
}

impl HidControl {
    /// Its value in `data`, the bytes after the report ID; `None` when the report is too short.
    fn value(self, data: &[u8]) -> Option<u32> {
        let byte = u32::from(*data.get(self.byte)?);
        match self.part {
            HidPart::Bit(bit) => Some((byte >> bit) & 1),
            HidPart::Byte => Some(byte),
            HidPart::Word => Some(byte | u32::from(*data.get(self.byte + 1)?) << 8),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// Steps up are 1 to 63, steps down 127 (one) to 65.
    TwosComplement,
    /// Steps up are 65 (one) and above, steps down 63 (one) and below.
    Offset,
    /// A HID byte that counts up and down and goes round: a step is the difference from the
    /// previous report.
    Counter,
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
        target: Steps,
        encoding: Encoding,
    },
}

/// What an encoder does with its steps.
#[derive(Clone, Debug, PartialEq)]
enum Steps {
    /// One command, with the steps times `step` in `param`.
    Amount { param: String, step: f64 },
    /// One command per step, with the parameters of `up` or of `down`, at most
    /// [`MAX_REPEATS`] per message.
    Each { up: Args, down: Args },
}

/// Most commands one message of an encoder sends, one per step.
pub const MAX_REPEATS: usize = 16;

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

    /// The commands the bindings may send, in the order of the file, so the application can
    /// check them when it reads the mapping: a button with its press and release, a fader at
    /// both ends of its range, an encoder one step each way.
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
            let with = |param: &str, value: f64| {
                let mut args = args.clone();
                args.insert(param.to_owned(), Arg::Number(value));
                args
            };
            let sent = match &binding.kind {
                Kind::Button => vec![args.clone()],
                Kind::Fader {
                    param, bottom, top, ..
                } => vec![with(param, *bottom), with(param, *top)],
                Kind::Encoder {
                    target: Steps::Amount { param, step },
                    ..
                } => vec![with(param, *step), with(param, -step)],
                Kind::Encoder {
                    target: Steps::Each { up, down },
                    ..
                } => vec![merged(args, up), merged(args, down)],
            };
            emits.extend(sent.into_iter().map(|args| Emit {
                command: command.clone(),
                args,
            }));
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
    /// The last HID report of each report ID, the ID left out; 0 holds the last report whole.
    reports: BTreeMap<u8, Vec<u8>>,
}

impl Mapper {
    pub fn new(mapping: Mapping) -> Self {
        let down = vec![false; mapping.bindings.len()];
        Self {
            mapping,
            modifiers: [0; MODIFIERS],
            down,
            reports: BTreeMap::new(),
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
            act(
                binding,
                input,
                &mut self.down[index],
                &mut self.modifiers,
                before,
                &mut emits,
            );
        }
        emits
    }

    /// The commands an input report of a HID device triggers, in the order of the file: each
    /// binding whose part of the report changed since the last report with the same ID.
    /// `docs/behavior/hid-controllers.md`.
    pub fn handle_report(&mut self, report: &[u8]) -> Vec<Emit> {
        let report = &report[..report.len().min(MAX_REPORT_BYTES)];
        let id = report.first().copied();
        let before = self.modifiers;
        let mut emits = Vec::new();
        for (index, binding) in self.mapping.bindings.iter().enumerate() {
            let Control::Hid(control) = binding.control else {
                continue;
            };
            let (key, data) = match (control.report, id) {
                (0, _) => (0, report),
                (wanted, Some(id)) if wanted == id => (id, &report[1..]),
                _ => continue,
            };
            let previous = self.reports.get(&key).map(Vec::as_slice);
            let Some(input) = hid_input(control, &binding.kind, data, previous) else {
                continue;
            };
            act(
                binding,
                input,
                &mut self.down[index],
                &mut self.modifiers,
                before,
                &mut emits,
            );
        }
        // Kept whole for the bindings of report 0, and without its ID for those of its ID.
        self.reports.insert(0, report.to_vec());
        if let Some(id) = id.filter(|id| *id != 0) {
            self.reports.insert(id, report[1..].to_vec());
        }
        emits
    }
}

/// What one binding does with what its control carries. Conditions are checked against the
/// modifiers `before` the message or report.
fn act(
    binding: &Binding,
    input: Input,
    down: &mut bool,
    modifiers: &mut [u8; MODIFIERS],
    before: [u8; MODIFIERS],
    emits: &mut Vec<Emit>,
) {
    let holds = binding
        .when
        .iter()
        .all(|(modifier, value)| before[*modifier] == *value);
    match &binding.kind {
        Kind::Button => match input.button() {
            Some(true) => {
                if *down || !holds {
                    return;
                }
                *down = true;
                match &binding.action {
                    Action::Command { command, args, .. } => emits.push(Emit {
                        command: command.clone(),
                        args: args.clone(),
                    }),
                    Action::Modifier { index, value, .. } => modifiers[*index] = *value,
                }
            }
            Some(false) => {
                if !*down {
                    return;
                }
                *down = false;
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
                    } => modifiers[*index] = 0,
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
                return;
            };
            let position = if *invert { 1.0 - position } else { position };
            emit_with(
                emits,
                &binding.action,
                param,
                bottom + position * (top - bottom),
            );
        }
        Kind::Encoder { target, encoding } => {
            let steps = if holds { input.steps(*encoding) } else { 0 };
            match target {
                _ if steps == 0 => {}
                Steps::Amount { param, step } => {
                    emit_with(emits, &binding.action, param, f64::from(steps) * step);
                }
                Steps::Each { up, down } => {
                    if let Action::Command { command, args, .. } = &binding.action {
                        let way = merged(args, if steps > 0 { up } else { down });
                        let count = (steps.unsigned_abs() as usize).min(MAX_REPEATS);
                        emits.extend((0..count).map(|_| Emit {
                            command: command.clone(),
                            args: way.clone(),
                        }));
                    }
                }
            }
        }
    }
}

/// What a report carries for a HID control: `None` when the report is too short, when its part
/// did not change, or for the first report of an encoder counter, which only sets where it is.
/// An encoder that sends steps acts on every report that carries some.
fn hid_input(
    control: HidControl,
    kind: &Kind,
    data: &[u8],
    previous: Option<&[u8]>,
) -> Option<Input> {
    let value = control.value(data)?;
    let old = previous.and_then(|previous| control.value(previous));
    match kind {
        Kind::Encoder {
            encoding: Encoding::Counter,
            ..
        } => {
            // A byte: the difference the short way round, from -128 to 127.
            let delta = i32::from((value as u8).wrapping_sub(old? as u8) as i8);
            (delta != 0).then_some(Input::Delta(delta))
        }
        Kind::Encoder { .. } => Some(Input::HidByte(value as u8)),
        // The first report counts as a change from all zeros.
        _ if old.unwrap_or(0) == value => None,
        _ => Some(match control.part {
            HidPart::Bit(_) => Input::HidButton(value == 1),
            HidPart::Byte => Input::HidByte(value as u8),
            HidPart::Word => Input::HidWord(value as u16),
        }),
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

/// What a message or a report carries for a control.
#[derive(Clone, Copy, Debug)]
enum Input {
    /// A note down or up.
    Note(bool),
    ControlChange(u8),
    PitchBend(u16),
    /// A bit of a HID report: set or clear.
    HidButton(bool),
    HidByte(u8),
    HidWord(u16),
    /// The steps of a HID counter since the report before.
    Delta(i32),
}

impl Input {
    fn button(self) -> Option<bool> {
        match self {
            Self::Note(down) | Self::HidButton(down) => Some(down),
            Self::ControlChange(value) | Self::HidByte(value) => Some(value > 0),
            Self::PitchBend(_) | Self::HidWord(_) | Self::Delta(_) => None,
        }
    }

    /// From 0 to 1.
    fn position(self) -> Option<f64> {
        match self {
            Self::Note(_) | Self::HidButton(_) | Self::Delta(_) => None,
            Self::ControlChange(value) => Some(f64::from(value.min(127)) / 127.0),
            Self::PitchBend(value) => Some(bend_position(value)),
            Self::HidByte(value) => Some(f64::from(value) / 255.0),
            Self::HidWord(value) => Some(f64::from(value) / 65_535.0),
        }
    }

    /// The steps of an encoder: up is positive.
    fn steps(self, encoding: Encoding) -> i32 {
        match self {
            Self::ControlChange(value) => steps(value, encoding),
            Self::HidByte(value) => byte_steps(value, encoding),
            Self::Delta(steps) => steps,
            _ => 0,
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
        Encoding::Counter => 0,
    }
}

/// The steps of a HID byte that sends steps: the same encodings as a control change, over eight
/// bits.
fn byte_steps(value: u8, encoding: Encoding) -> i32 {
    let value = i32::from(value);
    match encoding {
        Encoding::TwosComplement => match value {
            1..=127 => value,
            129..=255 => value - 256,
            _ => 0,
        },
        Encoding::Offset => match value {
            0 | 128 => 0,
            _ => value - 128,
        },
        Encoding::Counter => 0,
    }
}
