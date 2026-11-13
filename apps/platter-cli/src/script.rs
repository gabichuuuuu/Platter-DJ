//! The render script: a TOML file with the length of the mix and a list of timed commands.
//!
//! ```toml
//! sample_rate = 48000
//! seconds = 30.0
//!
//! [[command]]
//! at = 0.0
//! id = "deck.load"
//! deck = "a"
//! path = "first.wav"
//! ```
//!
//! Each entry names a command of the engine by its identifier and gives its parameters by
//! name. A relative path is taken from the folder of the script. The script is untrusted
//! input: everything in it is checked before the render starts.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use platter_core::{SampleRate, ValueError};
use platter_engine::command::{Args, ParamKind, Value};
use platter_engine::{Command, CommandError, RenderPlan, TimedCommand};
use serde::Deserialize;

/// A script longer than this is a mistake, not a mix.
const MAX_COMMANDS: usize = 100_000;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Script {
    sample_rate: u32,
    seconds: f64,
    #[serde(default)]
    command: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    at: f64,
    id: String,
    #[serde(flatten)]
    params: BTreeMap<String, toml::Value>,
}

#[derive(Debug)]
pub enum ScriptError {
    Syntax(String),
    Value(ValueError),
    TooManyCommands(usize),
    Command {
        /// Position of the entry in the script, from one.
        number: usize,
        error: CommandError,
    },
    ParamType {
        number: usize,
        param: String,
    },
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(message) => write!(f, "the script is not valid: {message}"),
            Self::Value(error) => error.fmt(f),
            Self::TooManyCommands(count) => {
                write!(
                    f,
                    "the script has {count} commands; the limit is {MAX_COMMANDS}"
                )
            }
            Self::Command { number, error } => write!(f, "command {number}: {error}"),
            Self::ParamType { number, param } => write!(
                f,
                "command {number}: the parameter `{param}` must be a number or a text"
            ),
        }
    }
}

impl std::error::Error for ScriptError {}

/// Reads a script. `folder` is where relative paths of `deck.load` start.
pub fn parse(text: &str, folder: &Path) -> Result<RenderPlan, ScriptError> {
    let script: Script =
        toml::from_str(text).map_err(|error| ScriptError::Syntax(error.to_string()))?;
    if script.command.len() > MAX_COMMANDS {
        return Err(ScriptError::TooManyCommands(script.command.len()));
    }
    let sample_rate = SampleRate::new(script.sample_rate).map_err(ScriptError::Value)?;
    let mut commands = Vec::with_capacity(script.command.len());
    for (index, entry) in script.command.into_iter().enumerate() {
        let number = index + 1;
        let args = arguments(&entry, folder, number)?;
        let command = Command::from_args(&entry.id, &args)
            .map_err(|error| ScriptError::Command { number, error })?;
        commands.push(TimedCommand {
            at_seconds: entry.at,
            command,
        });
    }
    Ok(RenderPlan {
        sample_rate,
        seconds: script.seconds,
        commands,
    })
}

fn arguments(entry: &Entry, folder: &Path, number: usize) -> Result<Args, ScriptError> {
    let spec = Command::spec_of(&entry.id);
    let is_path = |name: &str| {
        spec.is_some_and(|spec| {
            spec.params
                .iter()
                .any(|param| param.name == name && param.kind == ParamKind::Path)
        })
    };
    let mut args = Args::new();
    for (name, value) in &entry.params {
        let value = match value {
            toml::Value::Integer(integer) => Value::Number(*integer as f64),
            toml::Value::Float(float) => Value::Number(*float),
            toml::Value::String(text) if is_path(name) => {
                Value::Text(folder.join(text).to_string_lossy().into_owned())
            }
            toml::Value::String(text) => Value::Text(text.clone()),
            _ => {
                return Err(ScriptError::ParamType {
                    number,
                    param: name.clone(),
                });
            }
        };
        args.insert(name.clone(), value);
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use platter_core::DeckId;
    use platter_engine::Command;

    use super::{ScriptError, parse};

    const SCRIPT: &str = r#"
sample_rate = 44100
seconds = 12.5

[[command]]
at = 0
id = "deck.load"
deck = "a"
path = "tracks/first.wav"

[[command]]
at = 1.5
id = "mixer.crossfader.set"
position = 1
"#;

    #[test]
    fn reads_the_length_and_the_commands() {
        let folder = Path::new("mixes");
        let plan = parse(SCRIPT, folder).unwrap();
        assert_eq!(plan.sample_rate.hz(), 44_100);
        assert_eq!(plan.seconds, 12.5);
        assert_eq!(plan.commands.len(), 2);
        assert_eq!(plan.commands[0].at_seconds, 0.0);
        assert_eq!(
            plan.commands[0].command,
            Command::DeckLoad {
                deck: DeckId::A,
                path: folder.join("tracks/first.wav"),
            }
        );
        assert_eq!(
            plan.commands[1].command,
            Command::MixerCrossfaderSet { position: 1.0 }
        );
    }

    #[test]
    fn a_script_without_commands_is_valid() {
        let plan = parse("sample_rate = 48000\nseconds = 1.0\n", Path::new("")).unwrap();
        assert!(plan.commands.is_empty());
    }

    #[test]
    fn reports_the_entry_that_is_wrong() {
        let folder = Path::new("");
        let unknown = SCRIPT.replace("mixer.crossfader.set", "mixer.crossfader.wiggle");
        assert!(matches!(
            parse(&unknown, folder),
            Err(ScriptError::Command { number: 2, .. })
        ));
        let extra = SCRIPT.replace("position = 1", "position = 1\nspeed = 2");
        assert!(matches!(
            parse(&extra, folder),
            Err(ScriptError::Command { number: 2, .. })
        ));
        let list = SCRIPT.replace("position = 1", "position = [1, 2]");
        assert!(matches!(
            parse(&list, folder),
            Err(ScriptError::ParamType { number: 2, .. })
        ));
        let no_deck = SCRIPT.replace("deck = \"a\"\n", "");
        assert!(matches!(
            parse(&no_deck, folder),
            Err(ScriptError::Command { number: 1, .. })
        ));
    }

    #[test]
    fn rejects_a_broken_file_and_unusable_settings() {
        let folder = Path::new("");
        assert!(matches!(
            parse("seconds = ", folder),
            Err(ScriptError::Syntax(_))
        ));
        assert!(matches!(
            parse("seconds = 1.0\n", folder),
            Err(ScriptError::Syntax(_))
        ));
        assert!(matches!(
            parse("sample_rate = 48000\nseconds = 1.0\ntempo = 2\n", folder),
            Err(ScriptError::Syntax(_))
        ));
        assert!(matches!(
            parse("sample_rate = 12\nseconds = 1.0\n", folder),
            Err(ScriptError::Value(_))
        ));
        assert!(matches!(
            parse("sample_rate = -48000\nseconds = 1.0\n", folder),
            Err(ScriptError::Syntax(_))
        ));
    }
}
