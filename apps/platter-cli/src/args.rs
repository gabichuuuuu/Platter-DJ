//! Command-line arguments, parsed by hand: two subcommands do not justify a dependency.

use std::fmt;
use std::path::PathBuf;

pub const USAGE: &str = "\
usage:
  platter-cli info <file>
  platter-cli render --script <mix.toml> --out <mix.wav> [--log <file>]

info     prints the format, duration, channels and sample rate of an audio file
render   runs the timed commands of a script through the engine and writes the mix
         as a WAV file; no sound card is used";

#[derive(Debug, PartialEq, Eq)]
pub enum Invocation {
    Help,
    Info {
        file: PathBuf,
    },
    Render {
        script: PathBuf,
        out: PathBuf,
        log: Option<PathBuf>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum ArgsError {
    MissingCommand,
    UnknownCommand(String),
    UnknownOption(String),
    MissingValue(&'static str),
    Missing(&'static str),
    Repeated(&'static str),
    Unexpected(String),
}

impl fmt::Display for ArgsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCommand => f.write_str("no command given"),
            Self::UnknownCommand(command) => write!(f, "unknown command `{command}`"),
            Self::UnknownOption(option) => write!(f, "unknown option `{option}`"),
            Self::MissingValue(option) => write!(f, "`{option}` needs a value"),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::Repeated(option) => write!(f, "`{option}` is given twice"),
            Self::Unexpected(argument) => write!(f, "unexpected argument `{argument}`"),
        }
    }
}

pub fn parse(args: &[String]) -> Result<Invocation, ArgsError> {
    let (command, rest) = args.split_first().ok_or(ArgsError::MissingCommand)?;
    match command.as_str() {
        "help" | "--help" | "-h" => Ok(Invocation::Help),
        "info" => parse_info(rest),
        "render" => parse_render(rest),
        other => Err(ArgsError::UnknownCommand(other.to_owned())),
    }
}

fn parse_info(args: &[String]) -> Result<Invocation, ArgsError> {
    match args {
        [] => Err(ArgsError::Missing("the file")),
        [file] => Ok(Invocation::Info {
            file: PathBuf::from(file),
        }),
        [_, extra, ..] => Err(ArgsError::Unexpected(extra.clone())),
    }
}

fn parse_render(args: &[String]) -> Result<Invocation, ArgsError> {
    let (mut script, mut out, mut log) = (None, None, None);
    let mut args = args.iter();
    while let Some(argument) = args.next() {
        let (name, slot): (&'static str, &mut Option<PathBuf>) = match argument.as_str() {
            "--script" => ("--script", &mut script),
            "--out" => ("--out", &mut out),
            "--log" => ("--log", &mut log),
            other if other.starts_with('-') => {
                return Err(ArgsError::UnknownOption(other.to_owned()));
            }
            other => return Err(ArgsError::Unexpected(other.to_owned())),
        };
        let value = args.next().ok_or(ArgsError::MissingValue(name))?;
        if slot.replace(PathBuf::from(value)).is_some() {
            return Err(ArgsError::Repeated(name));
        }
    }
    Ok(Invocation::Render {
        script: script.ok_or(ArgsError::Missing("`--script`"))?,
        out: out.ok_or(ArgsError::Missing("`--out`"))?,
        log,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{ArgsError, Invocation, parse};

    fn parsed(args: &[&str]) -> Result<Invocation, ArgsError> {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        parse(&args)
    }

    #[test]
    fn reads_both_commands() {
        assert_eq!(
            parsed(&["info", "track.flac"]),
            Ok(Invocation::Info {
                file: PathBuf::from("track.flac")
            })
        );
        assert_eq!(
            parsed(&["render", "--out", "mix.wav", "--script", "mix.toml"]),
            Ok(Invocation::Render {
                script: PathBuf::from("mix.toml"),
                out: PathBuf::from("mix.wav"),
                log: None,
            })
        );
        assert!(matches!(
            parsed(&["render", "--script", "a", "--out", "b", "--log", "c"]),
            Ok(Invocation::Render { log: Some(_), .. })
        ));
        assert_eq!(parsed(&["--help"]), Ok(Invocation::Help));
    }

    #[test]
    fn reports_what_is_wrong() {
        assert_eq!(parsed(&[]), Err(ArgsError::MissingCommand));
        assert_eq!(
            parsed(&["play"]),
            Err(ArgsError::UnknownCommand("play".to_owned()))
        );
        assert_eq!(parsed(&["info"]), Err(ArgsError::Missing("the file")));
        assert_eq!(
            parsed(&["info", "a", "b"]),
            Err(ArgsError::Unexpected("b".to_owned()))
        );
        assert_eq!(
            parsed(&["render", "--script", "a"]),
            Err(ArgsError::Missing("`--out`"))
        );
        assert_eq!(
            parsed(&["render", "--out"]),
            Err(ArgsError::MissingValue("--out"))
        );
        assert_eq!(
            parsed(&["render", "--out", "a", "--out", "b"]),
            Err(ArgsError::Repeated("--out"))
        );
        assert_eq!(
            parsed(&["render", "--fast"]),
            Err(ArgsError::UnknownOption("--fast".to_owned()))
        );
        assert_eq!(
            parsed(&["render", "mix.toml"]),
            Err(ArgsError::Unexpected("mix.toml".to_owned()))
        );
    }
}
