//! Reading a mapping file: `docs/behavior/controllers.md`, section *The file*, and the HID
//! bindings of `docs/behavior/hid-controllers.md`.

use std::fmt;

use crate::{
    Action, Arg, Args, Binding, Control, Encoding, HidControl, HidPart, Kind, MAX_MODIFIER_VALUE,
    MAX_REPORT_BYTES, MODIFIERS, Steps,
};

/// Largest mapping file read.
pub const MAX_FILE_BYTES: usize = 1 << 20;
/// Most bindings in one mapping.
pub const MAX_BINDINGS: usize = 4096;
/// Most modifier conditions on one binding.
const MAX_CONDITIONS: usize = 2;
/// Highest control change a binding may name; 120 to 127 are channel mode messages.
const MAX_CONTROL_CHANGE: u8 = 119;

#[derive(Clone, Debug, PartialEq)]
pub enum MappingError {
    TooLarge,
    Syntax(String),
    TooManyBindings,
    /// A binding that cannot be used, by its position in the file, counted from one.
    Binding {
        number: usize,
        problem: BindingProblem,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum BindingProblem {
    NotATable,
    MissingMidi,
    /// `midi` does not name a control.
    BadMidi(String),
    /// `hid` does not name a part of a report.
    BadHid(String),
    /// `control` is not `button`, `fader` or `encoder`.
    BadControl(String),
    /// A note can only be a button, and the pitch bend only a fader.
    ControlDoesNotFit,
    /// Neither `command` nor `modifier`.
    MissingCommand,
    /// `modifier` is not from 1 to 8, or `value` not from 0 to 7.
    BadModifier,
    BadCondition(String),
    TooManyConditions,
    /// A key whose value has the wrong type, or a number that is not finite.
    BadValue(String),
    /// A key that belongs to another kind of control or binding.
    NotForThisControl(String),
    MissingParam,
    /// `range` is not two finite numbers.
    BadRange,
    BadEncoding(String),
    /// A command on hold needs the parameters to send on release.
    MissingRelease,
    /// An encoder takes `param` with `step`, or `up` with `down`.
    MixedSteps,
}

impl fmt::Display for MappingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(f, "the mapping file is larger than {MAX_FILE_BYTES} bytes"),
            Self::Syntax(message) => write!(f, "the mapping file cannot be read: {message}"),
            Self::TooManyBindings => {
                write!(f, "the mapping file has more than {MAX_BINDINGS} bindings")
            }
            Self::Binding { number, problem } => {
                write!(f, "binding {number} of the mapping file: {problem}")
            }
        }
    }
}

impl fmt::Display for BindingProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotATable => f.write_str("it is not a table"),
            Self::MissingMidi => f.write_str("`midi` or `hid` is missing"),
            Self::BadMidi(text) => write!(f, "`{text}` is not a MIDI control"),
            Self::BadHid(text) => write!(f, "`{text}` is not a part of a HID report"),
            Self::BadControl(text) => write!(f, "`{text}` is not button, fader or encoder"),
            Self::ControlDoesNotFit => f.write_str(
                "a note or a bit is a button, the pitch bend or a word a fader, \
                 and an encoder a control change or a byte",
            ),
            Self::MissingCommand => f.write_str("`command` or `modifier` is missing"),
            Self::BadModifier => write!(
                f,
                "a modifier is from 1 to {MODIFIERS} and its value from 0 to {MAX_MODIFIER_VALUE}"
            ),
            Self::BadCondition(text) => write!(f, "`{text}` is not a condition such as `1 = 0`"),
            Self::TooManyConditions => {
                write!(f, "a binding has at most {MAX_CONDITIONS} conditions")
            }
            Self::BadValue(name) => write!(f, "`{name}` has a value of the wrong kind"),
            Self::NotForThisControl(name) => write!(f, "`{name}` does not apply here"),
            Self::MissingParam => f.write_str("`param`, or `up` and `down`, is missing"),
            Self::BadRange => f.write_str("`range` must be two numbers"),
            Self::BadEncoding(text) => {
                write!(f, "`{text}` is not 7f-01, 3f-41 or, on a HID byte, counter")
            }
            Self::MissingRelease => f.write_str("a command on hold needs `release`"),
            Self::MixedSteps => {
                f.write_str("an encoder takes `param` and `step`, or `up` and `down`")
            }
        }
    }
}

impl std::error::Error for MappingError {}

pub(crate) fn parse(source: &str) -> Result<Vec<Binding>, MappingError> {
    if source.len() > MAX_FILE_BYTES {
        return Err(MappingError::TooLarge);
    }
    let root: toml::Table = source
        .parse()
        .map_err(|error: toml::de::Error| MappingError::Syntax(error.message().to_owned()))?;
    let list = match root.get("binding") {
        None => return Ok(Vec::new()),
        Some(toml::Value::Array(list)) => list,
        Some(_) => return Err(MappingError::Syntax("`binding` is not a list".to_owned())),
    };
    if list.len() > MAX_BINDINGS {
        return Err(MappingError::TooManyBindings);
    }
    list.iter()
        .enumerate()
        .map(|(index, value)| {
            read_binding(value).map_err(|problem| MappingError::Binding {
                number: index + 1,
                problem,
            })
        })
        .collect()
}

fn read_control(text: &str) -> Option<Control> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let number = |word: &str, top: u8| word.parse::<u8>().ok().filter(|value| *value <= top);
    let channel = |word: &str| {
        word.parse::<u8>()
            .ok()
            .filter(|value| (1..=16).contains(value))
            .map(|value| value - 1)
    };
    match words.as_slice() {
        [kind, ch, note] if kind.eq_ignore_ascii_case("note") => Some(Control::Note {
            channel: channel(ch)?,
            note: number(note, 127)?,
        }),
        [kind, ch, cc] if kind.eq_ignore_ascii_case("cc") => Some(Control::ControlChange {
            channel: channel(ch)?,
            number: number(cc, MAX_CONTROL_CHANGE)?,
        }),
        [kind, ch] if kind.eq_ignore_ascii_case("pitch") => Some(Control::PitchBend {
            channel: channel(ch)?,
        }),
        _ => None,
    }
}

/// `bit R B N`, `byte R B` or `word R B`: report `R`, byte `B` and bit `N`.
fn read_hid(text: &str) -> Option<Control> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let (kind, report, byte, bit) = match words.as_slice() {
        [kind, report, byte] => (*kind, *report, *byte, None),
        [kind, report, byte, bit] => (*kind, *report, *byte, Some(*bit)),
        _ => return None,
    };
    let report = report.parse::<u8>().ok()?;
    let byte = byte.parse::<usize>().ok()?;
    // The bytes of the control must fit in a report with its ID.
    let room = MAX_REPORT_BYTES - usize::from(report != 0);
    let (part, width) = match (kind.to_ascii_lowercase().as_str(), bit) {
        ("bit", Some(bit)) => (
            HidPart::Bit(bit.parse::<u8>().ok().filter(|bit| *bit < 8)?),
            1,
        ),
        ("byte", None) => (HidPart::Byte, 1),
        ("word", None) => (HidPart::Word, 2),
        _ => return None,
    };
    (byte + width <= room).then_some(Control::Hid(HidControl { report, byte, part }))
}

fn read_condition(text: &str) -> Option<(usize, u8)> {
    let (modifier, value) = text.split_once('=')?;
    let modifier = modifier.trim().parse::<usize>().ok()?;
    let value = value.trim().parse::<u8>().ok()?;
    ((1..=MODIFIERS).contains(&modifier) && value <= MAX_MODIFIER_VALUE)
        .then_some((modifier - 1, value))
}

fn number(value: &toml::Value) -> Option<f64> {
    match value {
        toml::Value::Integer(number) => Some(*number as f64),
        toml::Value::Float(number) => Some(*number),
        _ => None,
    }
    .filter(|number| number.is_finite())
}

fn read_args<'a>(
    pairs: impl Iterator<Item = (&'a String, &'a toml::Value)>,
) -> Result<Args, BindingProblem> {
    pairs
        .map(|(name, value)| {
            let arg = match value {
                toml::Value::String(text) => Arg::Text(text.clone()),
                other => Arg::Number(
                    number(other).ok_or_else(|| BindingProblem::BadValue(name.clone()))?,
                ),
            };
            Ok((name.clone(), arg))
        })
        .collect()
}

/// Keys of a binding that are not parameters of its command.
const BINDING_KEYS: [&str; 14] = [
    "midi", "hid", "command", "control", "hold", "release", "when", "param", "range", "invert",
    "step", "encoding", "up", "down",
];
const BUTTON_KEYS: [&str; 2] = ["hold", "release"];
const FADER_KEYS: [&str; 3] = ["param", "range", "invert"];
const ENCODER_KEYS: [&str; 5] = ["param", "step", "encoding", "up", "down"];

fn read_binding(value: &toml::Value) -> Result<Binding, BindingProblem> {
    let table = value.as_table().ok_or(BindingProblem::NotATable)?;
    let text = |key: &str| -> Result<Option<&str>, BindingProblem> {
        match table.get(key) {
            None => Ok(None),
            Some(toml::Value::String(text)) => Ok(Some(text)),
            Some(_) => Err(BindingProblem::BadValue(key.to_owned())),
        }
    };
    let flag = |key: &str| -> Result<bool, BindingProblem> {
        match table.get(key) {
            None => Ok(false),
            Some(toml::Value::Boolean(flag)) => Ok(*flag),
            Some(_) => Err(BindingProblem::BadValue(key.to_owned())),
        }
    };
    let control = match (text("midi")?, text("hid")?) {
        (Some(_), Some(_)) => return Err(BindingProblem::NotForThisControl("hid".to_owned())),
        (Some(midi), None) => {
            read_control(midi).ok_or_else(|| BindingProblem::BadMidi(midi.to_owned()))?
        }
        (None, Some(hid)) => read_hid(hid).ok_or_else(|| BindingProblem::BadHid(hid.to_owned()))?,
        (None, None) => return Err(BindingProblem::MissingMidi),
    };
    let declared = text("control")?;
    let bit = |control| {
        matches!(
            control,
            Control::Hid(HidControl {
                part: HidPart::Bit(_),
                ..
            })
        )
    };
    let word = |control| {
        matches!(
            control,
            Control::Hid(HidControl {
                part: HidPart::Word,
                ..
            })
        )
    };
    let kind_name = match (declared, control) {
        (Some(name), _) => name,
        (None, Control::Note { .. }) => "button",
        (None, control) if bit(control) => "button",
        (None, _) => "fader",
    };
    let fits = match (kind_name, control) {
        ("button", Control::PitchBend { .. }) => false,
        ("button", control) => !word(control),
        ("fader", Control::Note { .. }) => false,
        ("fader", control) => !bit(control),
        ("encoder", Control::ControlChange { .. }) => true,
        (
            "encoder",
            Control::Hid(HidControl {
                part: HidPart::Byte,
                ..
            }),
        ) => true,
        ("encoder", _) => false,
        (other, _) => return Err(BindingProblem::BadControl(other.to_owned())),
    };
    if !fits {
        return Err(BindingProblem::ControlDoesNotFit);
    }
    let own_keys: &[&str] = match kind_name {
        "button" => &BUTTON_KEYS,
        "fader" => &FADER_KEYS,
        _ => &ENCODER_KEYS,
    };
    let foreign = [BUTTON_KEYS.as_slice(), &FADER_KEYS, &ENCODER_KEYS]
        .concat()
        .into_iter()
        .find(|key| table.contains_key(*key) && !own_keys.contains(key));
    if let Some(key) = foreign {
        return Err(BindingProblem::NotForThisControl(key.to_owned()));
    }
    let when = match table.get("when") {
        None => Vec::new(),
        Some(toml::Value::Array(list)) => {
            if list.len() > MAX_CONDITIONS {
                return Err(BindingProblem::TooManyConditions);
            }
            list.iter()
                .map(|item| {
                    let text = item.as_str().unwrap_or_default();
                    read_condition(text)
                        .ok_or_else(|| BindingProblem::BadCondition(text.to_owned()))
                })
                .collect::<Result<_, _>>()?
        }
        Some(_) => return Err(BindingProblem::BadValue("when".to_owned())),
    };
    let hold = flag("hold")?;

    let Some(command) = text("command")? else {
        return read_modifier(table, control, kind_name, hold, when);
    };
    if table.contains_key("modifier") {
        return Err(BindingProblem::NotForThisControl("modifier".to_owned()));
    }
    let table_of = |key: &str| -> Result<Option<Args>, BindingProblem> {
        match table.get(key) {
            None => Ok(None),
            Some(toml::Value::Table(inner)) => read_args(inner.iter()).map(Some),
            Some(_) => Err(BindingProblem::BadValue(key.to_owned())),
        }
    };
    let kind = match kind_name {
        "button" => Kind::Button,
        "fader" => {
            let (bottom, top) = match table.get("range") {
                None => (0.0, 1.0),
                Some(toml::Value::Array(pair)) => match pair.as_slice() {
                    [bottom, top] => (
                        number(bottom).ok_or(BindingProblem::BadRange)?,
                        number(top).ok_or(BindingProblem::BadRange)?,
                    ),
                    _ => return Err(BindingProblem::BadRange),
                },
                Some(_) => return Err(BindingProblem::BadRange),
            };
            Kind::Fader {
                param: text("param")?
                    .ok_or(BindingProblem::MissingParam)?
                    .to_owned(),
                bottom,
                top,
                invert: flag("invert")?,
            }
        }
        _ => Kind::Encoder {
            target: match (text("param")?, table_of("up")?, table_of("down")?) {
                (Some(param), None, None) => Steps::Amount {
                    param: param.to_owned(),
                    step: match table.get("step") {
                        None => 1.0,
                        Some(value) => number(value)
                            .ok_or_else(|| BindingProblem::BadValue("step".to_owned()))?,
                    },
                },
                (None, Some(up), Some(down)) if !table.contains_key("step") => {
                    Steps::Each { up, down }
                }
                (None, None, None) => return Err(BindingProblem::MissingParam),
                _ => return Err(BindingProblem::MixedSteps),
            },
            encoding: match text("encoding")? {
                None | Some("7f-01") => Encoding::TwosComplement,
                Some("3f-41") => Encoding::Offset,
                Some("counter") if matches!(control, Control::Hid(_)) => Encoding::Counter,
                Some(other) => return Err(BindingProblem::BadEncoding(other.to_owned())),
            },
        },
    };
    let args = read_args(
        table
            .iter()
            .filter(|(name, _)| !BINDING_KEYS.contains(&name.as_str())),
    )?;
    let release = match table.get("release") {
        None => None,
        Some(toml::Value::Table(release)) => Some(read_args(release.iter())?),
        Some(_) => return Err(BindingProblem::BadValue("release".to_owned())),
    };
    match (hold, &release) {
        (true, None) => return Err(BindingProblem::MissingRelease),
        (false, Some(_)) => return Err(BindingProblem::NotForThisControl("release".to_owned())),
        _ => {}
    }
    Ok(Binding {
        control,
        kind,
        action: Action::Command {
            command: command.to_owned(),
            args,
            release,
        },
        when,
    })
}

/// A binding that sets a modifier: a button with `modifier` and `value` and nothing else.
fn read_modifier(
    table: &toml::Table,
    control: Control,
    kind_name: &str,
    hold: bool,
    when: Vec<(usize, u8)>,
) -> Result<Binding, BindingProblem> {
    let (Some(modifier), Some(value)) = (table.get("modifier"), table.get("value")) else {
        return Err(BindingProblem::MissingCommand);
    };
    if kind_name != "button" {
        return Err(BindingProblem::ControlDoesNotFit);
    }
    let allowed = [
        "midi", "hid", "control", "hold", "when", "modifier", "value",
    ];
    if let Some(key) = table.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(BindingProblem::NotForThisControl(key.clone()));
    }
    let modifier = modifier
        .as_integer()
        .filter(|modifier| (1..=MODIFIERS as i64).contains(modifier))
        .ok_or(BindingProblem::BadModifier)?;
    let value = value
        .as_integer()
        .filter(|value| (0..=i64::from(MAX_MODIFIER_VALUE)).contains(value))
        .ok_or(BindingProblem::BadModifier)?;
    Ok(Binding {
        control,
        kind: Kind::Button,
        action: Action::Modifier {
            index: (modifier - 1) as usize,
            value: value as u8,
            hold,
        },
        when,
    })
}
