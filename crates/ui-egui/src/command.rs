//! The commands of the application around the engine, and the two kinds together.
//!
//! The engine has its commands (`platter_engine::Command`): play, set a fader, load a
//! track. The application has more: open the preferences, choose the audio device, move the
//! selection of the file list. Both follow the same rule: a stable identifier, a label,
//! typed parameters. Controls, keyboard shortcuts and anything that comes later build an
//! [`Action`] and hand it to the application; nothing changes state by another path.

use std::path::PathBuf;

use platter_core::{DeckId, ValueError};
use platter_engine::Command;
use platter_engine::command::{
    Args, CommandError, CommandSpec, ParamKind, ParamSpec, SPECS, Value,
};

use crate::i18n::Language;

/// A page of the preferences.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsPage {
    #[default]
    Audio,
    General,
    Mixer,
}

impl SettingsPage {
    pub const ALL: [Self; 3] = [Self::Audio, Self::General, Self::Mixer];

    pub fn name(self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::General => "general",
            Self::Mixer => "mixer",
        }
    }
}

/// A page of the advanced panel of a deck.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeckPage {
    #[default]
    Move,
    Cue,
    Grid,
}

impl DeckPage {
    pub const ALL: [Self; 3] = [Self::Move, Self::Cue, Self::Grid];

    pub fn name(self) -> &'static str {
        match self {
            Self::Move => "move",
            Self::Cue => "cue",
            Self::Grid => "grid",
        }
    }
}

/// One side of one output pair of the engine, to be placed on an output of the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteSlot {
    MasterLeft,
    MasterRight,
    HeadphonesLeft,
    HeadphonesRight,
}

impl RouteSlot {
    pub const ALL: [Self; 4] = [
        Self::MasterLeft,
        Self::MasterRight,
        Self::HeadphonesLeft,
        Self::HeadphonesRight,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::MasterLeft => "master_left",
            Self::MasterRight => "master_right",
            Self::HeadphonesLeft => "headphones_left",
            Self::HeadphonesRight => "headphones_right",
        }
    }

    /// Position in the routing of the settings: master pair first, then headphones.
    pub fn index(self) -> usize {
        match self {
            Self::MasterLeft => 0,
            Self::MasterRight => 1,
            Self::HeadphonesLeft => 2,
            Self::HeadphonesRight => 3,
        }
    }
}

/// A level meter with a clip indicator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MeterId {
    Deck(DeckId),
    Master,
}

impl MeterId {
    pub const ALL: [Self; 3] = [Self::Deck(DeckId::A), Self::Deck(DeckId::B), Self::Master];

    pub fn name(self) -> &'static str {
        match self {
            Self::Deck(DeckId::A) => "a",
            Self::Deck(DeckId::B) => "b",
            Self::Master => "master",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Deck(deck) => deck.index(),
            Self::Master => DeckId::COUNT,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AppCommand {
    SettingsOpen {
        page: SettingsPage,
    },
    SettingsClose,
    SettingsRestoreDefaults,
    /// `None` is the default device of the system.
    AudioDeviceSet {
        device: Option<String>,
    },
    AudioSampleRateSet {
        hz: u32,
    },
    AudioBufferSet {
        frames: u32,
    },
    /// Puts one side of an output pair on an output of the device, counted from zero.
    AudioRoutingSet {
        slot: RouteSlot,
        output: u16,
    },
    AudioDropoutsReset,
    LanguageSet {
        language: Language,
    },
    /// Opens the keyboard shortcut file in the editor of the system.
    KeymapOpen,
    /// Shows a folder in the file list.
    BrowserNavigate {
        path: PathBuf,
    },
    /// Opens or closes a folder of the tree.
    BrowserToggle {
        path: PathBuf,
    },
    /// Shows the folder that holds the current one.
    BrowserUp,
    BrowserSelect {
        index: usize,
    },
    /// Moves the selection of the file list by a number of rows.
    BrowserMove {
        delta: i32,
    },
    /// Opens the selected folder.
    BrowserActivate,
    BrowserLoadSelected {
        deck: DeckId,
    },
    WaveformZoomIn {
        deck: DeckId,
    },
    WaveformZoomOut {
        deck: DeckId,
    },
    /// Swaps remaining and elapsed time as the large reading of a deck.
    DeckTimeDisplayToggle {
        deck: DeckId,
    },
    DeckNoticeDismiss {
        deck: DeckId,
    },
    /// Shows a page of the advanced panel of a deck.
    DeckPageSet {
        deck: DeckId,
        page: DeckPage,
    },
    /// Switches the mapping mode of the hotcues of a deck: while it is on, the key of a
    /// hotcue takes the selected cue.
    DeckCueMapToggle {
        deck: DeckId,
    },
    MixerClipReset {
        meter: MeterId,
    },
}

const DECK: ParamSpec = ParamSpec {
    name: "deck",
    kind: ParamKind::Deck,
};

const PATH: ParamSpec = ParamSpec {
    name: "path",
    kind: ParamKind::Path,
};

const fn number(name: &'static str, min: f64, max: f64) -> ParamSpec {
    ParamSpec {
        name,
        kind: ParamKind::Number { min, max },
    }
}

const fn choice(name: &'static str, choices: &'static [&'static str]) -> ParamSpec {
    ParamSpec {
        name,
        kind: ParamKind::Choice(choices),
    }
}

const PAGES: &[&str] = &["audio", "general"];
const LANGUAGES: &[&str] = &["en", "es"];
const SLOTS: &[&str] = &[
    "master_left",
    "master_right",
    "headphones_left",
    "headphones_right",
];
const METERS: &[&str] = &["a", "b", "master"];
const DECK_PAGES: &[&str] = &["move", "cue", "grid"];
/// Outputs a device may have, as far as the routing goes.
pub const MAX_OUTPUTS: u16 = 64;
const MAX_ROWS: f64 = 1_000_000.0;

/// Every command of the application, in the order a list shows them.
pub const APP_SPECS: &[CommandSpec] = &[
    CommandSpec {
        id: "app.settings.open",
        label: "Open preferences",
        params: &[choice("page", PAGES)],
    },
    CommandSpec {
        id: "app.settings.close",
        label: "Close preferences",
        params: &[],
    },
    CommandSpec {
        id: "app.settings.restore_defaults",
        label: "Restore default settings",
        params: &[],
    },
    CommandSpec {
        id: "app.language.set",
        label: "Set language",
        params: &[choice("language", LANGUAGES)],
    },
    CommandSpec {
        id: "app.keymap.open",
        label: "Open the shortcut file",
        params: &[],
    },
    CommandSpec {
        id: "audio.device.set",
        label: "Set audio device",
        params: &[ParamSpec {
            name: "device",
            kind: ParamKind::Text,
        }],
    },
    CommandSpec {
        id: "audio.sample_rate.set",
        label: "Set sample rate",
        params: &[number("hz", 8_000.0, 384_000.0)],
    },
    CommandSpec {
        id: "audio.buffer.set",
        label: "Set buffer size",
        params: &[number("frames", 16.0, 16_384.0)],
    },
    CommandSpec {
        id: "audio.routing.set",
        label: "Set output routing",
        params: &[
            choice("slot", SLOTS),
            number("output", 0.0, (MAX_OUTPUTS - 1) as f64),
        ],
    },
    CommandSpec {
        id: "audio.dropouts.reset",
        label: "Reset dropout count",
        params: &[],
    },
    CommandSpec {
        id: "browser.navigate",
        label: "Show folder",
        params: &[PATH],
    },
    CommandSpec {
        id: "browser.toggle",
        label: "Open or close folder in the tree",
        params: &[PATH],
    },
    CommandSpec {
        id: "browser.up",
        label: "Show parent folder",
        params: &[],
    },
    CommandSpec {
        id: "browser.select",
        label: "Select row",
        params: &[number("index", 0.0, MAX_ROWS)],
    },
    CommandSpec {
        id: "browser.move",
        label: "Move selection",
        params: &[number("delta", -MAX_ROWS, MAX_ROWS)],
    },
    CommandSpec {
        id: "browser.activate",
        label: "Open selected folder",
        params: &[],
    },
    CommandSpec {
        id: "browser.load_selected",
        label: "Load selected track",
        params: &[DECK],
    },
    CommandSpec {
        id: "waveform.zoom.in",
        label: "Zoom in",
        params: &[DECK],
    },
    CommandSpec {
        id: "waveform.zoom.out",
        label: "Zoom out",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.time_display.toggle",
        label: "Swap remaining and elapsed time",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.notice.dismiss",
        label: "Dismiss deck message",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.page.set",
        label: "Show deck page",
        params: &[DECK, choice("page", DECK_PAGES)],
    },
    CommandSpec {
        id: "deck.cue.map",
        label: "Map cue to hotcue",
        params: &[DECK],
    },
    CommandSpec {
        id: "mixer.clip.reset",
        label: "Reset clip indicator",
        params: &[choice("meter", METERS)],
    },
];

/// Reads the parameters of one command by the names its specification declares.
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

    fn text(&self, param: &'static str) -> Result<&str, CommandError> {
        match self.param(param)? {
            Value::Text(text) => Ok(text),
            Value::Number(_) => Err(CommandError::WrongType {
                command: self.spec.id,
                param,
            }),
        }
    }

    /// A whole number inside the range the specification declares.
    fn integer(&self, param: &'static str) -> Result<i64, CommandError> {
        let Value::Number(value) = self.param(param)? else {
            return Err(CommandError::WrongType {
                command: self.spec.id,
                param,
            });
        };
        let (min, max) = self
            .spec
            .params
            .iter()
            .find_map(|spec| match spec.kind {
                ParamKind::Number { min, max } if spec.name == param => Some((min, max)),
                _ => None,
            })
            .unwrap_or((0.0, 0.0));
        let value = ValueError::check(param, *value, min, max)?;
        Ok(value.round() as i64)
    }

    fn deck(&self) -> Result<DeckId, CommandError> {
        Ok(DeckId::parse(self.text("deck")?)?)
    }

    /// One of the choices of `param`, as its position in `choices`.
    fn choice<T: Copy>(
        &self,
        param: &'static str,
        choices: &[T],
        name: impl Fn(T) -> &'static str,
        expected: &'static str,
    ) -> Result<T, CommandError> {
        let text = self.text(param)?;
        choices
            .iter()
            .copied()
            .find(|choice| name(*choice) == text)
            .ok_or(CommandError::Value(ValueError::UnknownChoice {
                name: param,
                expected,
            }))
    }
}

impl AppCommand {
    pub fn spec_of(id: &str) -> Option<&'static CommandSpec> {
        APP_SPECS.iter().find(|spec| spec.id == id)
    }

    /// Builds a command from its identifier and untyped parameters, like
    /// `platter_engine::Command::from_args`.
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
            "app.settings.open" => Self::SettingsOpen {
                page: read.choice(
                    "page",
                    &SettingsPage::ALL,
                    SettingsPage::name,
                    "audio, general",
                )?,
            },
            "app.settings.close" => Self::SettingsClose,
            "app.settings.restore_defaults" => Self::SettingsRestoreDefaults,
            "app.language.set" => Self::LanguageSet {
                language: read.choice("language", &Language::ALL, Language::code, "en, es")?,
            },
            "app.keymap.open" => Self::KeymapOpen,
            "audio.device.set" => {
                let device = read.text("device")?;
                Self::AudioDeviceSet {
                    device: (!device.is_empty()).then(|| device.to_owned()),
                }
            }
            "audio.sample_rate.set" => Self::AudioSampleRateSet {
                hz: read.integer("hz")? as u32,
            },
            "audio.buffer.set" => Self::AudioBufferSet {
                frames: read.integer("frames")? as u32,
            },
            "audio.routing.set" => Self::AudioRoutingSet {
                slot: read.choice(
                    "slot",
                    &RouteSlot::ALL,
                    RouteSlot::name,
                    "master_left, master_right, headphones_left, headphones_right",
                )?,
                output: read.integer("output")? as u16,
            },
            "audio.dropouts.reset" => Self::AudioDropoutsReset,
            other => return Self::view_from_args(other, &read),
        })
    }

    /// The commands of the file list and of what the window shows.
    fn view_from_args(id: &str, read: &Reader<'_>) -> Result<Self, CommandError> {
        Ok(match id {
            "browser.navigate" => Self::BrowserNavigate {
                path: PathBuf::from(read.text("path")?),
            },
            "browser.toggle" => Self::BrowserToggle {
                path: PathBuf::from(read.text("path")?),
            },
            "browser.up" => Self::BrowserUp,
            "browser.select" => Self::BrowserSelect {
                index: read.integer("index")? as usize,
            },
            "browser.move" => Self::BrowserMove {
                delta: read.integer("delta")? as i32,
            },
            "browser.activate" => Self::BrowserActivate,
            "browser.load_selected" => Self::BrowserLoadSelected { deck: read.deck()? },
            "waveform.zoom.in" => Self::WaveformZoomIn { deck: read.deck()? },
            "waveform.zoom.out" => Self::WaveformZoomOut { deck: read.deck()? },
            "deck.time_display.toggle" => Self::DeckTimeDisplayToggle { deck: read.deck()? },
            "deck.notice.dismiss" => Self::DeckNoticeDismiss { deck: read.deck()? },
            "deck.page.set" => Self::DeckPageSet {
                deck: read.deck()?,
                page: read.choice("page", &DeckPage::ALL, DeckPage::name, "move, cue, grid")?,
            },
            "deck.cue.map" => Self::DeckCueMapToggle { deck: read.deck()? },
            "mixer.clip.reset" => Self::MixerClipReset {
                meter: read.choice("meter", &MeterId::ALL, MeterId::name, "a, b, master")?,
            },
            other => return Err(CommandError::UnknownCommand(other.to_owned())),
        })
    }

    /// The stable identifier of the command.
    pub fn id(&self) -> &'static str {
        match self {
            Self::SettingsOpen { .. } => "app.settings.open",
            Self::SettingsClose => "app.settings.close",
            Self::SettingsRestoreDefaults => "app.settings.restore_defaults",
            Self::LanguageSet { .. } => "app.language.set",
            Self::KeymapOpen => "app.keymap.open",
            Self::AudioDeviceSet { .. } => "audio.device.set",
            Self::AudioSampleRateSet { .. } => "audio.sample_rate.set",
            Self::AudioBufferSet { .. } => "audio.buffer.set",
            Self::AudioRoutingSet { .. } => "audio.routing.set",
            Self::AudioDropoutsReset => "audio.dropouts.reset",
            Self::BrowserNavigate { .. } => "browser.navigate",
            Self::BrowserToggle { .. } => "browser.toggle",
            Self::BrowserUp => "browser.up",
            Self::BrowserSelect { .. } => "browser.select",
            Self::BrowserMove { .. } => "browser.move",
            Self::BrowserActivate => "browser.activate",
            Self::BrowserLoadSelected { .. } => "browser.load_selected",
            Self::WaveformZoomIn { .. } => "waveform.zoom.in",
            Self::WaveformZoomOut { .. } => "waveform.zoom.out",
            Self::DeckTimeDisplayToggle { .. } => "deck.time_display.toggle",
            Self::DeckNoticeDismiss { .. } => "deck.notice.dismiss",
            Self::DeckPageSet { .. } => "deck.page.set",
            Self::DeckCueMapToggle { .. } => "deck.cue.map",
            Self::MixerClipReset { .. } => "mixer.clip.reset",
        }
    }
}

/// Something a control or a shortcut asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Engine(Command),
    App(AppCommand),
}

impl From<Command> for Action {
    fn from(command: Command) -> Self {
        Self::Engine(command)
    }
}

impl From<AppCommand> for Action {
    fn from(command: AppCommand) -> Self {
        Self::App(command)
    }
}

impl Action {
    pub fn id(&self) -> &'static str {
        match self {
            Self::Engine(command) => command.id(),
            Self::App(command) => command.id(),
        }
    }

    /// The specification of a command of either kind.
    pub fn spec_of(id: &str) -> Option<&'static CommandSpec> {
        Command::spec_of(id).or_else(|| AppCommand::spec_of(id))
    }

    /// Every command of the engine and of the application.
    pub fn specs() -> impl Iterator<Item = &'static CommandSpec> {
        SPECS.iter().chain(APP_SPECS)
    }

    /// Builds an action of either kind from an identifier and untyped parameters.
    pub fn from_args(id: &str, args: &Args) -> Result<Self, CommandError> {
        if Command::spec_of(id).is_some() {
            Command::from_args(id, args).map(Self::Engine)
        } else {
            AppCommand::from_args(id, args).map(Self::App)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use platter_engine::command::{Args, CommandError, CommandSpec, ParamKind, Value};

    use super::{APP_SPECS, Action, AppCommand};

    /// Valid arguments for a specification: the first choice, the lower bound, deck A.
    fn sample_args(spec: &CommandSpec) -> Args {
        spec.params
            .iter()
            .map(|param| {
                let value = match param.kind {
                    ParamKind::Deck => Value::Text("a".to_owned()),
                    ParamKind::Number { min, .. } => Value::Number(min),
                    ParamKind::Choice(choices) => Value::Text(choices[0].to_owned()),
                    ParamKind::Path => Value::Text("music".to_owned()),
                    ParamKind::Text => Value::Text("device".to_owned()),
                };
                (param.name.to_owned(), value)
            })
            .collect()
    }

    #[test]
    fn identifiers_are_unique_across_the_engine_and_the_application() {
        let ids: BTreeSet<&str> = Action::specs().map(|spec| spec.id).collect();
        assert_eq!(ids.len(), Action::specs().count());
        for spec in APP_SPECS {
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
    fn every_specification_builds_its_command_and_needs_every_parameter() {
        for spec in Action::specs() {
            let args = sample_args(spec);
            let action = Action::from_args(spec.id, &args).unwrap();
            assert_eq!(action.id(), spec.id);
            for param in spec.params {
                let mut missing = args.clone();
                missing.remove(param.name);
                assert_eq!(
                    Action::from_args(spec.id, &missing),
                    Err(CommandError::MissingParam {
                        command: spec.id,
                        param: param.name
                    })
                );
            }
        }
    }

    #[test]
    fn values_outside_what_a_command_accepts_are_rejected() {
        let mut args = Args::new();
        args.insert("hz".to_owned(), Value::Number(12.0));
        assert!(matches!(
            AppCommand::from_args("audio.sample_rate.set", &args),
            Err(CommandError::Value(_))
        ));
        args.clear();
        args.insert("page".to_owned(), Value::Text("network".to_owned()));
        assert!(matches!(
            AppCommand::from_args("app.settings.open", &args),
            Err(CommandError::Value(_))
        ));
        args.insert("extra".to_owned(), Value::Number(1.0));
        assert!(matches!(
            AppCommand::from_args("app.settings.open", &args),
            Err(CommandError::UnknownParam { .. })
        ));
        assert!(matches!(
            Action::from_args("app.explode", &Args::new()),
            Err(CommandError::UnknownCommand(_))
        ));
    }

    #[test]
    fn an_empty_device_name_is_the_default_device() {
        let mut args = Args::new();
        args.insert("device".to_owned(), Value::Text(String::new()));
        assert_eq!(
            AppCommand::from_args("audio.device.set", &args),
            Ok(AppCommand::AudioDeviceSet { device: None })
        );
    }
}
