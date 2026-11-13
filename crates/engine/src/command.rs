//! The commands of the engine.
//!
//! Every action a user can take is one of these, with a stable identifier. The interface,
//! the keyboard, the CLI and, later, MIDI and MCP all build the same values, so there is one
//! path into the engine. [`SPECS`] describes each command for anything that needs to list,
//! label or build them from untyped input.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use platter_core::{DeckId, ValueError};
use platter_dsp::crossfade::CrossfaderCurve;
use platter_dsp::eq::Band;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Loads a file into a deck, in the background.
    DeckLoad {
        deck: DeckId,
        path: PathBuf,
    },
    DeckEject {
        deck: DeckId,
    },
    DeckPlay {
        deck: DeckId,
    },
    DeckPause {
        deck: DeckId,
    },
    DeckPlayPause {
        deck: DeckId,
    },
    /// Sets the cue point at the current position.
    DeckCueSet {
        deck: DeckId,
    },
    /// Returns to the cue point and pauses there.
    DeckCueGoto {
        deck: DeckId,
    },
    DeckSeek {
        deck: DeckId,
        seconds: f64,
    },
    /// Playback speed as a ratio of the original, `0.0..=2.0`.
    DeckTempoSet {
        deck: DeckId,
        ratio: f64,
    },
    /// Temporary speed offset as a fraction of the tempo, `-0.5..=0.5`; zero releases it.
    DeckNudge {
        deck: DeckId,
        amount: f64,
    },
    MixerGainSet {
        deck: DeckId,
        db: f64,
    },
    /// Knob position `0.0..=1.0`: zero removes the band, the centre leaves it untouched.
    MixerEqSet {
        deck: DeckId,
        band: Band,
        position: f64,
    },
    /// Knob position `-1.0..=1.0`: low-pass below zero, high-pass above, off at zero.
    MixerFilterSet {
        deck: DeckId,
        position: f64,
    },
    /// Fader position `0.0..=1.0`.
    MixerFaderSet {
        deck: DeckId,
        position: f64,
    },
    /// Crossfader position `0.0..=1.0`: zero is deck A alone, one is deck B alone.
    MixerCrossfaderSet {
        position: f64,
    },
    MixerCrossfaderCurveSet {
        curve: CrossfaderCurve,
    },
    MasterGainSet {
        db: f64,
    },
}

/// Kind of a command parameter, for anything that builds commands from untyped input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamKind {
    Deck,
    Number { min: f64, max: f64 },
    Choice(&'static [&'static str]),
    Path,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamSpec {
    pub name: &'static str,
    pub kind: ParamKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CommandSpec {
    /// Stable identifier. It never changes once released.
    pub id: &'static str,
    /// Text for menus and lists, in English; the interface translates it by identifier.
    pub label: &'static str,
    pub params: &'static [ParamSpec],
}

const DECK: ParamSpec = ParamSpec {
    name: "deck",
    kind: ParamKind::Deck,
};

const fn number(name: &'static str, min: f64, max: f64) -> ParamSpec {
    ParamSpec {
        name,
        kind: ParamKind::Number { min, max },
    }
}

pub const MAX_SEEK_SECONDS: f64 = 86_400.0;
pub const MAX_NUDGE: f64 = 0.5;
pub const MAX_CHANNEL_GAIN_DB: f64 = 12.0;
pub const MAX_MASTER_GAIN_DB: f64 = 12.0;
const MIN_GAIN_DB: f64 = -90.0;
const BANDS: &[&str] = &["low", "mid", "high"];
const CURVES: &[&str] = &["smooth", "linear", "sharp"];

/// Every command, in the order a list shows them.
pub const SPECS: &[CommandSpec] = &[
    CommandSpec {
        id: "deck.load",
        label: "Load track",
        params: &[
            DECK,
            ParamSpec {
                name: "path",
                kind: ParamKind::Path,
            },
        ],
    },
    CommandSpec {
        id: "deck.eject",
        label: "Eject track",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.play",
        label: "Play",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.pause",
        label: "Pause",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.play_pause",
        label: "Play or pause",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.cue.set",
        label: "Set cue point",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.cue.goto",
        label: "Return to cue point",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.seek",
        label: "Jump to position",
        params: &[DECK, number("seconds", 0.0, MAX_SEEK_SECONDS)],
    },
    CommandSpec {
        id: "deck.tempo.set",
        label: "Set tempo",
        params: &[DECK, number("ratio", 0.0, 2.0)],
    },
    CommandSpec {
        id: "deck.nudge",
        label: "Bend tempo",
        params: &[DECK, number("amount", -MAX_NUDGE, MAX_NUDGE)],
    },
    CommandSpec {
        id: "mixer.gain.set",
        label: "Set channel gain",
        params: &[DECK, number("db", MIN_GAIN_DB, MAX_CHANNEL_GAIN_DB)],
    },
    CommandSpec {
        id: "mixer.eq.set",
        label: "Set equaliser band",
        params: &[
            DECK,
            ParamSpec {
                name: "band",
                kind: ParamKind::Choice(BANDS),
            },
            number("position", 0.0, 1.0),
        ],
    },
    CommandSpec {
        id: "mixer.filter.set",
        label: "Set channel filter",
        params: &[DECK, number("position", -1.0, 1.0)],
    },
    CommandSpec {
        id: "mixer.fader.set",
        label: "Set channel fader",
        params: &[DECK, number("position", 0.0, 1.0)],
    },
    CommandSpec {
        id: "mixer.crossfader.set",
        label: "Set crossfader",
        params: &[number("position", 0.0, 1.0)],
    },
    CommandSpec {
        id: "mixer.crossfader.curve.set",
        label: "Set crossfader curve",
        params: &[ParamSpec {
            name: "curve",
            kind: ParamKind::Choice(CURVES),
        }],
    },
    CommandSpec {
        id: "master.gain.set",
        label: "Set main volume",
        params: &[number("db", MIN_GAIN_DB, MAX_MASTER_GAIN_DB)],
    },
];

/// An untyped parameter value, as a script, a mapping or a remote caller provides it.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Text(String),
}

/// Untyped parameters by name.
pub type Args = BTreeMap<String, Value>;

#[derive(Clone, Debug, PartialEq)]
pub enum CommandError {
    UnknownCommand(String),
    MissingParam {
        command: &'static str,
        param: &'static str,
    },
    UnknownParam {
        command: &'static str,
        param: String,
    },
    /// The parameter is text where a number is expected, or the other way round.
    WrongType {
        command: &'static str,
        param: &'static str,
    },
    Value(ValueError),
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCommand(id) => write!(f, "unknown command `{id}`"),
            Self::MissingParam { command, param } => {
                write!(f, "`{command}` needs the parameter `{param}`")
            }
            Self::UnknownParam { command, param } => {
                write!(f, "`{command}` has no parameter `{param}`")
            }
            Self::WrongType { command, param } => {
                write!(
                    f,
                    "the parameter `{param}` of `{command}` has the wrong type"
                )
            }
            Self::Value(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for CommandError {}

impl From<ValueError> for CommandError {
    fn from(error: ValueError) -> Self {
        Self::Value(error)
    }
}

/// Reads the parameters of one command, by the names its specification declares.
struct Reader<'a> {
    spec: &'static CommandSpec,
    args: &'a Args,
}

impl Reader<'_> {
    fn param(&self, param: &'static str) -> Result<&Value, CommandError> {
        self.args.get(param).ok_or(CommandError::MissingParam {
            command: self.spec.id,
            param,
        })
    }

    fn number(&self, param: &'static str) -> Result<f64, CommandError> {
        match self.param(param)? {
            Value::Number(number) => Ok(*number),
            Value::Text(_) => Err(CommandError::WrongType {
                command: self.spec.id,
                param,
            }),
        }
    }

    fn text(&self, param: &'static str) -> Result<&str, CommandError> {
        match self.param(param)? {
            Value::Text(text) => Ok(text),
            Value::Number(_) => Err(CommandError::WrongType {
                command: self.spec.id,
                param,
            }),
        }
    }

    fn deck(&self) -> Result<DeckId, CommandError> {
        Ok(DeckId::parse(self.text("deck")?)?)
    }

    fn band(&self) -> Result<Band, CommandError> {
        match self.text("band")? {
            "low" => Ok(Band::Low),
            "mid" => Ok(Band::Mid),
            "high" => Ok(Band::High),
            _ => Err(CommandError::Value(ValueError::UnknownChoice {
                name: "band",
                expected: "low, mid, high",
            })),
        }
    }
}

impl Command {
    /// The specification of a command identifier.
    pub fn spec_of(id: &str) -> Option<&'static CommandSpec> {
        SPECS.iter().find(|spec| spec.id == id)
    }

    /// Builds a command from its identifier and untyped parameters. Parameters the command
    /// does not declare are an error, so a typing mistake in a script does not pass
    /// unnoticed. Ranges are checked when the command is dispatched.
    pub fn from_args(id: &str, args: &Args) -> Result<Self, CommandError> {
        let spec = Self::spec_of(id).ok_or_else(|| CommandError::UnknownCommand(id.to_owned()))?;
        if let Some(extra) = args
            .keys()
            .find(|name| spec.params.iter().all(|param| param.name != name.as_str()))
        {
            return Err(CommandError::UnknownParam {
                command: spec.id,
                param: extra.clone(),
            });
        }
        let read = Reader { spec, args };
        Ok(match spec.id {
            "deck.load" => Self::DeckLoad {
                deck: read.deck()?,
                path: PathBuf::from(read.text("path")?),
            },
            "deck.eject" => Self::DeckEject { deck: read.deck()? },
            "deck.play" => Self::DeckPlay { deck: read.deck()? },
            "deck.pause" => Self::DeckPause { deck: read.deck()? },
            "deck.play_pause" => Self::DeckPlayPause { deck: read.deck()? },
            "deck.cue.set" => Self::DeckCueSet { deck: read.deck()? },
            "deck.cue.goto" => Self::DeckCueGoto { deck: read.deck()? },
            "deck.seek" => Self::DeckSeek {
                deck: read.deck()?,
                seconds: read.number("seconds")?,
            },
            "deck.tempo.set" => Self::DeckTempoSet {
                deck: read.deck()?,
                ratio: read.number("ratio")?,
            },
            "deck.nudge" => Self::DeckNudge {
                deck: read.deck()?,
                amount: read.number("amount")?,
            },
            "mixer.gain.set" => Self::MixerGainSet {
                deck: read.deck()?,
                db: read.number("db")?,
            },
            "mixer.eq.set" => Self::MixerEqSet {
                deck: read.deck()?,
                band: read.band()?,
                position: read.number("position")?,
            },
            "mixer.filter.set" => Self::MixerFilterSet {
                deck: read.deck()?,
                position: read.number("position")?,
            },
            "mixer.fader.set" => Self::MixerFaderSet {
                deck: read.deck()?,
                position: read.number("position")?,
            },
            "mixer.crossfader.set" => Self::MixerCrossfaderSet {
                position: read.number("position")?,
            },
            "mixer.crossfader.curve.set" => Self::MixerCrossfaderCurveSet {
                curve: CrossfaderCurve::parse(read.text("curve")?)?,
            },
            "master.gain.set" => Self::MasterGainSet {
                db: read.number("db")?,
            },
            other => return Err(CommandError::UnknownCommand(other.to_owned())),
        })
    }

    /// The stable identifier of the command.
    pub fn id(&self) -> &'static str {
        match self {
            Self::DeckLoad { .. } => "deck.load",
            Self::DeckEject { .. } => "deck.eject",
            Self::DeckPlay { .. } => "deck.play",
            Self::DeckPause { .. } => "deck.pause",
            Self::DeckPlayPause { .. } => "deck.play_pause",
            Self::DeckCueSet { .. } => "deck.cue.set",
            Self::DeckCueGoto { .. } => "deck.cue.goto",
            Self::DeckSeek { .. } => "deck.seek",
            Self::DeckTempoSet { .. } => "deck.tempo.set",
            Self::DeckNudge { .. } => "deck.nudge",
            Self::MixerGainSet { .. } => "mixer.gain.set",
            Self::MixerEqSet { .. } => "mixer.eq.set",
            Self::MixerFilterSet { .. } => "mixer.filter.set",
            Self::MixerFaderSet { .. } => "mixer.fader.set",
            Self::MixerCrossfaderSet { .. } => "mixer.crossfader.set",
            Self::MixerCrossfaderCurveSet { .. } => "mixer.crossfader.curve.set",
            Self::MasterGainSet { .. } => "master.gain.set",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use platter_core::DeckId;

    use super::{Args, Command, CommandError, ParamKind, SPECS, Value};

    /// Valid arguments for a specification: the first choice, the lower bound, deck A.
    fn sample_args(spec: &super::CommandSpec) -> Args {
        spec.params
            .iter()
            .map(|param| {
                let value = match param.kind {
                    ParamKind::Deck => Value::Text("a".to_owned()),
                    ParamKind::Number { min, .. } => Value::Number(min),
                    ParamKind::Choice(choices) => Value::Text(choices[0].to_owned()),
                    ParamKind::Path => Value::Text("track.wav".to_owned()),
                };
                (param.name.to_owned(), value)
            })
            .collect()
    }

    #[test]
    fn identifiers_are_unique_and_well_formed() {
        let ids: BTreeSet<&str> = SPECS.iter().map(|spec| spec.id).collect();
        assert_eq!(ids.len(), SPECS.len());
        for spec in SPECS {
            assert!(
                spec.id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '.' || c == '_'),
                "{}",
                spec.id
            );
            assert!(!spec.label.is_empty());
        }
    }

    #[test]
    fn every_specification_builds_its_command() {
        for spec in SPECS {
            let command = Command::from_args(spec.id, &sample_args(spec)).unwrap();
            assert_eq!(command.id(), spec.id);
        }
    }

    #[test]
    fn every_declared_parameter_is_required() {
        for spec in SPECS {
            for param in spec.params {
                let mut args = sample_args(spec);
                args.remove(param.name);
                assert_eq!(
                    Command::from_args(spec.id, &args),
                    Err(CommandError::MissingParam {
                        command: spec.id,
                        param: param.name
                    })
                );
            }
        }
    }

    #[test]
    fn rejects_what_a_specification_does_not_declare() {
        let spec = Command::spec_of("deck.play").unwrap();
        let mut args = sample_args(spec);
        args.insert("speed".to_owned(), Value::Number(1.0));
        assert!(matches!(
            Command::from_args("deck.play", &args),
            Err(CommandError::UnknownParam { .. })
        ));
        assert!(matches!(
            Command::from_args("deck.explode", &Args::new()),
            Err(CommandError::UnknownCommand(_))
        ));
    }

    #[test]
    fn rejects_values_of_the_wrong_type_or_choice() {
        let mut args = Args::new();
        args.insert("deck".to_owned(), Value::Number(1.0));
        assert!(matches!(
            Command::from_args("deck.play", &args),
            Err(CommandError::WrongType { .. })
        ));
        args.insert("deck".to_owned(), Value::Text("z".to_owned()));
        assert!(matches!(
            Command::from_args("deck.play", &args),
            Err(CommandError::Value(_))
        ));
        args.insert("deck".to_owned(), Value::Text("b".to_owned()));
        assert_eq!(
            Command::from_args("deck.play", &args),
            Ok(Command::DeckPlay { deck: DeckId::B })
        );
    }
}
