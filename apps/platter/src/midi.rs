//! The MIDI controllers of the settings file at work: their mappings, checked against the
//! commands when they are read, and their inputs, whose messages become actions.
//!
//! Behaviour note: `docs/behavior/controllers.md`, section *In the application*; the
//! choices: `docs/decisions/0026-midi-in-the-application.md`.

use std::collections::BTreeSet;
use std::fmt;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use platter_control::{Arg, Emit, MAX_FILE_BYTES, Mapper, Mapping, MappingError};
use platter_device_io::{Input, Messages, PortError};
use platter_engine::command::{Args, CommandError, ParamKind, Value};
use platter_ui_egui::command::Action;

/// Messages an input holds until the window takes them: a few frames of a MIDI cable at
/// full speed, about a thousand messages a second.
const QUEUE: usize = 256;
/// Messages taken from one input in one frame. At 60 frames a second this is more than a
/// cable carries, and it bounds the commands one frame sends to the engine.
pub const PER_FRAME: usize = 64;
/// Shortest time between two reports of lost messages in the log.
const REPORT_EVERY: Duration = Duration::from_secs(10);

/// Why a mapping is not used.
#[derive(Clone, Debug, PartialEq)]
pub enum Problem {
    Read(String),
    Mapping(MappingError),
    Command {
        command: String,
        error: CommandError,
    },
    OutOfRange {
        command: String,
        param: String,
        value: f64,
        min: f64,
        max: f64,
    },
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => write!(f, "cannot read the mapping file: {error}"),
            Self::Mapping(error) => error.fmt(f),
            Self::Command { command, error } => write!(f, "`{command}`: {error}"),
            Self::OutOfRange {
                command,
                param,
                value,
                min,
                max,
            } => write!(
                f,
                "`{command}`: {param} may reach {value}, outside {min} to {max}"
            ),
        }
    }
}

fn args_of(args: &platter_control::Args) -> Args {
    args.iter()
        .map(|(name, arg)| {
            let value = match arg {
                Arg::Number(number) => Value::Number(*number),
                Arg::Text(text) => Value::Text(text.clone()),
            };
            (name.clone(), value)
        })
        .collect()
}

/// The action a binding sends, without checking the ranges of its numbers.
pub fn action_of(emit: &Emit) -> Result<Action, CommandError> {
    Action::from_args(&emit.command, &args_of(&emit.args))
}

/// Checks every command the mapping may send: that it exists, takes the parameters the
/// binding gives it, and that every number is inside the range of its parameter.
pub fn check(mapping: &Mapping) -> Result<(), Problem> {
    for emit in mapping.commands() {
        action_of(&emit).map_err(|error| Problem::Command {
            command: emit.command.clone(),
            error,
        })?;
        let Some(spec) = Action::spec_of(&emit.command) else {
            continue;
        };
        for (name, arg) in &emit.args {
            let Arg::Number(value) = arg else {
                continue;
            };
            let range = spec.params.iter().find_map(|param| match param.kind {
                ParamKind::Number { min, max } if param.name == name => Some((min, max)),
                _ => None,
            });
            if let Some((min, max)) = range
                && !(min..=max).contains(value)
            {
                return Err(Problem::OutOfRange {
                    command: emit.command.clone(),
                    param: name.clone(),
                    value: *value,
                    min,
                    max,
                });
            }
        }
    }
    Ok(())
}

/// Reads a mapping file and checks it. A file larger than a mapping may be is refused
/// without reading it whole.
pub fn read(path: &Path) -> Result<Mapping, Problem> {
    let file = std::fs::File::open(path).map_err(|error| Problem::Read(error.to_string()))?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Problem::Read(error.to_string()))?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(Problem::Mapping(MappingError::TooLarge));
    }
    let source = String::from_utf8(bytes).map_err(|error| Problem::Read(error.to_string()))?;
    let mapping = Mapping::parse(&source).map_err(Problem::Mapping)?;
    check(&mapping)?;
    Ok(mapping)
}

/// A checked mapping at work: from the bytes of a message to actions.
#[derive(Debug)]
pub struct Translator {
    mapper: Mapper,
}

impl Translator {
    /// `mapping` must have passed [`check`].
    pub fn new(mapping: Mapping) -> Self {
        Self {
            mapper: Mapper::new(mapping),
        }
    }

    /// The actions of one message, in the order of the mapping. Every command was checked
    /// with the mapping and only the numbers change, so none is refused here; the engine
    /// checks their ranges when it carries them out.
    pub fn actions(&mut self, bytes: &[u8]) -> Vec<Action> {
        self.mapper
            .handle(bytes)
            .iter()
            .filter_map(|emit| action_of(emit).ok())
            .collect()
    }
}

/// When to write the messages an input lost in the log.
#[derive(Debug, Default)]
pub struct LossReport {
    reported: (u64, u64),
    last: Option<Instant>,
}

impl LossReport {
    /// The counts of lost and too long messages to write now, if they grew since the last
    /// report and that report is old enough.
    pub fn due(&mut self, dropped: u64, oversized: u64, now: Instant) -> Option<(u64, u64)> {
        let grew = dropped > self.reported.0 || oversized > self.reported.1;
        let rested = self
            .last
            .is_none_or(|last| now.saturating_duration_since(last) >= REPORT_EVERY);
        if !(grew && rested) {
            return None;
        }
        self.reported = (dropped, oversized);
        self.last = Some(now);
        Some((dropped, oversized))
    }
}

/// An open controller: its input, its mapping and what its log has said.
#[derive(Debug)]
pub struct Controller {
    input: Input,
    messages: Messages,
    translator: Translator,
    /// Commands the engine refused once; they are not written in the log again.
    refused: BTreeSet<&'static str>,
    losses: LossReport,
}

impl Controller {
    /// Opens the input `name` for a checked mapping. `wake` is called on the thread of the
    /// port when a message arrives.
    pub fn open(
        name: &str,
        mapping: Mapping,
        wake: impl Fn() + Send + 'static,
    ) -> Result<Self, PortError> {
        let (input, messages) = platter_device_io::open(name, QUEUE, wake)?;
        Ok(Self {
            input,
            messages,
            translator: Translator::new(mapping),
            refused: BTreeSet::new(),
            losses: LossReport::default(),
        })
    }

    pub fn name(&self) -> &str {
        self.input.name()
    }

    /// The actions of the messages waiting now, at most [`PER_FRAME`] messages, how many
    /// messages were taken, mapped or not, and whether more may be waiting.
    pub fn take(&mut self) -> (Vec<Action>, usize, bool) {
        let mut actions = Vec::new();
        let mut taken = 0;
        for message in self.messages.take().take(PER_FRAME) {
            actions.extend(self.translator.actions(message.bytes()));
            taken += 1;
        }
        (actions, taken, taken == PER_FRAME)
    }

    /// Whether a refusal of `command` by the engine is the first one, and so goes in the log.
    pub fn first_refusal(&mut self, command: &'static str) -> bool {
        self.refused.insert(command)
    }

    /// The counts of lost messages to write in the log now, if any.
    pub fn losses(&mut self, now: Instant) -> Option<(u64, u64)> {
        let (dropped, oversized) = (self.messages.dropped(), self.messages.oversized());
        self.losses.due(dropped, oversized, now)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use platter_control::{MAX_FILE_BYTES, Mapping, MappingError};
    use platter_core::DeckId;
    use platter_engine::Command;
    use platter_engine::command::CommandError;
    use platter_ui_egui::command::{Action, AppCommand};

    use super::{LossReport, Problem, REPORT_EVERY, Translator, check, read};

    fn mapping(source: &str) -> Mapping {
        Mapping::parse(source).unwrap()
    }

    #[test]
    fn a_mapping_whose_commands_the_application_takes_passes() {
        let source = "[[binding]]\nmidi = \"note 1 36\"\ncommand = \"deck.play_pause\"\n\
                      deck = \"a\"\n\n[[binding]]\nmidi = \"cc 1 7\"\n\
                      command = \"mixer.crossfader.set\"\nparam = \"position\"\n\n\
                      [[binding]]\nmidi = \"note 1 40\"\ncommand = \"app.settings.close\"\n";
        assert_eq!(check(&mapping(source)), Ok(()));
    }

    #[test]
    fn an_unknown_command_or_parameter_stops_the_mapping() {
        let unknown = mapping("[[binding]]\nmidi = \"note 1 36\"\ncommand = \"deck.fly\"\n");
        assert_eq!(
            check(&unknown),
            Err(Problem::Command {
                command: "deck.fly".to_owned(),
                error: CommandError::UnknownCommand("deck.fly".to_owned()),
            })
        );
        let extra = mapping(
            "[[binding]]\nmidi = \"note 1 36\"\ncommand = \"deck.play_pause\"\n\
             deck = \"a\"\nspeed = 2\n",
        );
        assert!(matches!(
            check(&extra),
            Err(Problem::Command {
                error: CommandError::UnknownParam { .. },
                ..
            })
        ));
        // A parameter given only on release is checked too.
        let release = mapping(
            "[[binding]]\nmidi = \"note 1 36\"\ncommand = \"deck.play_pause\"\n\
             deck = \"a\"\nhold = true\nrelease = { deck = \"z\" }\n",
        );
        assert!(matches!(check(&release), Err(Problem::Command { .. })));
    }

    #[test]
    fn a_fader_that_goes_past_its_parameter_stops_the_mapping() {
        let past = mapping(
            "[[binding]]\nmidi = \"cc 1 7\"\ncommand = \"mixer.crossfader.set\"\n\
             param = \"position\"\nrange = [0, 2]\n",
        );
        assert_eq!(
            check(&past),
            Err(Problem::OutOfRange {
                command: "mixer.crossfader.set".to_owned(),
                param: "position".to_owned(),
                value: 2.0,
                min: 0.0,
                max: 1.0,
            })
        );
        let fixed = mapping(
            "[[binding]]\nmidi = \"note 1 36\"\ncommand = \"mixer.crossfader.set\"\n\
             position = -0.5\n",
        );
        assert!(matches!(check(&fixed), Err(Problem::OutOfRange { .. })));
    }

    #[test]
    fn messages_become_actions_of_the_engine_and_of_the_application() {
        let mut translator = Translator::new(mapping(
            "[[binding]]\nmidi = \"note 1 36\"\ncommand = \"deck.play_pause\"\ndeck = \"b\"\n\n\
             [[binding]]\nmidi = \"cc 1 7\"\ncommand = \"mixer.crossfader.set\"\n\
             param = \"position\"\n\n[[binding]]\nmidi = \"note 1 40\"\n\
             command = \"app.settings.close\"\n",
        ));
        assert_eq!(
            translator.actions(&[0x90, 36, 100]),
            [Action::Engine(Command::DeckPlayPause { deck: DeckId::B })]
        );
        assert_eq!(
            translator.actions(&[0xB0, 7, 127]),
            [Action::Engine(Command::MixerCrossfaderSet {
                position: 1.0
            })]
        );
        assert_eq!(
            translator.actions(&[0x90, 40, 1]),
            [Action::App(AppCommand::SettingsClose)]
        );
        assert!(translator.actions(&[0x90, 41, 1]).is_empty());
    }

    #[test]
    fn a_mapping_file_too_large_is_refused_before_it_is_read_whole() {
        let folder = std::env::temp_dir().join(format!("platter-midi-test-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let large = folder.join("large.toml");
        std::fs::write(&large, "#".repeat(MAX_FILE_BYTES + 10)).unwrap();
        assert_eq!(
            read(&large).unwrap_err(),
            Problem::Mapping(MappingError::TooLarge)
        );
        let good = folder.join("good.toml");
        std::fs::write(
            &good,
            "[[binding]]\nmidi = \"note 1 36\"\ncommand = \"deck.play_pause\"\ndeck = \"a\"\n",
        )
        .unwrap();
        assert_eq!(read(&good).unwrap().len(), 1);
        assert!(matches!(
            read(&folder.join("missing.toml")),
            Err(Problem::Read(_))
        ));
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn lost_messages_are_reported_when_they_grow_and_not_too_often() {
        let start = Instant::now();
        let mut report = LossReport::default();
        assert_eq!(report.due(0, 0, start), None);
        assert_eq!(report.due(3, 0, start), Some((3, 0)));
        assert_eq!(report.due(5, 1, start + Duration::from_secs(1)), None);
        assert_eq!(report.due(5, 1, start + REPORT_EVERY), Some((5, 1)));
        assert_eq!(report.due(5, 1, start + REPORT_EVERY * 3), None);
    }
}
