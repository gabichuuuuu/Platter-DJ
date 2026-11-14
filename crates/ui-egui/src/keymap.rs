//! Keyboard shortcuts: a table from key chords to commands, read from a TOML file the user
//! can edit.
//!
//! ```toml
//! [[binding]]
//! keys = "Shift + Left"
//! command = "browser.load_selected"
//! deck = "a"
//! ```
//!
//! Every key of a binding other than `keys`, `command` and `release` is a parameter of the
//! command. A command that acts while held (`deck.cue`, `deck.cup`) gets its `action`
//! parameter from the key itself: `press` when it goes down, `release` when it comes up. A
//! binding may also name the parameters to send on release in a `release` table; the tempo
//! bend uses it to return to zero.

use std::fmt;

use egui::{Key, Modifiers};
use platter_engine::command::{Args, CommandError, ParamKind, Value};

use crate::command::Action;

/// Bindings a file may hold. A real keymap has a few dozen.
const MAX_BINDINGS: usize = 1_024;
const ACTION_PARAM: &str = "action";

/// A key with the modifiers held with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: Key,
    pub shift: bool,
    /// Control, or Command on macOS.
    pub command: bool,
    pub alt: bool,
}

impl Chord {
    pub const fn plain(key: Key) -> Self {
        Self {
            key,
            shift: false,
            command: false,
            alt: false,
        }
    }

    /// Reads a chord written as `Shift + Left` or `Ctrl+,`; names are those of the keys in
    /// English, without regard to case.
    pub fn parse(text: &str) -> Option<Self> {
        let mut chord = None;
        let (mut shift, mut command, mut alt) = (false, false, false);
        for part in text.split('+').map(str::trim) {
            match part.to_ascii_lowercase().as_str() {
                "shift" => shift = true,
                "ctrl" | "control" | "cmd" | "command" => command = true,
                "alt" | "option" => alt = true,
                _ if chord.is_some() => return None,
                _ => chord = Some(key_from_name(part)?),
            }
        }
        Some(Self {
            key: chord?,
            shift,
            command,
            alt,
        })
    }

    fn matches(&self, key: Key, modifiers: &Modifiers) -> bool {
        self.key == key
            && self.shift == modifiers.shift
            && self.command == modifiers.command
            && self.alt == modifiers.alt
    }

    /// The parts of the chord in the order they are written: modifiers, then the key.
    pub fn parts(&self) -> Vec<&'static str> {
        let mut parts = Vec::new();
        if self.command {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        parts.push(self.key.name());
        parts
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.parts().join(" + "))
    }
}

fn key_from_name(name: &str) -> Option<Key> {
    // The names egui gives its keys, compared without regard to case, plus the characters
    // people type for the punctuation keys.
    Key::from_name(name).or_else(|| {
        Key::ALL
            .iter()
            .copied()
            .find(|key| key.name().eq_ignore_ascii_case(name) || key.symbol_or_name() == name)
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub chord: Chord,
    pub command: &'static str,
    /// Parameters as the file gives them, without the one the key itself provides.
    pub args: Args,
    /// Parameters to send when the key comes up.
    pub release: Option<Args>,
}

impl Binding {
    /// Whether the command acts while the key is held.
    fn is_held(&self) -> bool {
        self.release.is_some() || has_action_param(self.command)
    }

    fn action(&self, pressed: bool) -> Option<Action> {
        let mut args = match (&self.release, pressed) {
            (Some(release), false) => release.clone(),
            _ => self.args.clone(),
        };
        if has_action_param(self.command) {
            let action = if pressed { "press" } else { "release" };
            args.insert(ACTION_PARAM.to_owned(), Value::Text(action.to_owned()));
        } else if !pressed && self.release.is_none() {
            return None;
        }
        Action::from_args(self.command, &args).ok()
    }
}

fn has_action_param(command: &str) -> bool {
    Action::spec_of(command).is_some_and(|spec| {
        spec.params
            .iter()
            .any(|param| param.name == ACTION_PARAM && matches!(param.kind, ParamKind::Choice(_)))
    })
}

#[derive(Clone, Debug, PartialEq)]
pub enum KeymapError {
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
    MissingKeys,
    UnknownKeys(String),
    MissingCommand,
    BadValue(String),
    Command(CommandError),
}

impl fmt::Display for KeymapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(message) => write!(f, "the shortcut file cannot be read: {message}"),
            Self::TooManyBindings => {
                write!(f, "the shortcut file has more than {MAX_BINDINGS} bindings")
            }
            Self::Binding { number, problem } => {
                write!(f, "binding {number} of the shortcut file: ")?;
                match problem {
                    BindingProblem::NotATable => f.write_str("it is not a table"),
                    BindingProblem::MissingKeys => f.write_str("`keys` is missing"),
                    BindingProblem::UnknownKeys(keys) => write!(f, "`{keys}` is not a key chord"),
                    BindingProblem::MissingCommand => f.write_str("`command` is missing"),
                    BindingProblem::BadValue(name) => {
                        write!(f, "`{name}` must be a number or a text")
                    }
                    BindingProblem::Command(error) => error.fmt(f),
                }
            }
        }
    }
}

impl std::error::Error for KeymapError {}

fn read_args<'a>(
    pairs: impl Iterator<Item = (&'a String, &'a toml::Value)>,
) -> Result<Args, BindingProblem> {
    pairs
        .map(|(name, value)| {
            let value = match value {
                toml::Value::String(text) => Value::Text(text.clone()),
                toml::Value::Integer(number) => Value::Number(*number as f64),
                toml::Value::Float(number) => Value::Number(*number),
                _ => return Err(BindingProblem::BadValue(name.clone())),
            };
            Ok((name.clone(), value))
        })
        .collect()
}

fn read_binding(value: &toml::Value) -> Result<Binding, BindingProblem> {
    let table = value.as_table().ok_or(BindingProblem::NotATable)?;
    let keys = table
        .get("keys")
        .and_then(toml::Value::as_str)
        .ok_or(BindingProblem::MissingKeys)?;
    let chord = Chord::parse(keys).ok_or_else(|| BindingProblem::UnknownKeys(keys.to_owned()))?;
    let command = table
        .get("command")
        .and_then(toml::Value::as_str)
        .ok_or(BindingProblem::MissingCommand)?;
    let spec = Action::spec_of(command)
        .ok_or_else(|| BindingProblem::Command(CommandError::UnknownCommand(command.to_owned())))?;
    let reserved = ["keys", "command", "release"];
    let args = read_args(
        table
            .iter()
            .filter(|(name, _)| !reserved.contains(&name.as_str())),
    )?;
    let release = match table.get("release") {
        None => None,
        Some(toml::Value::Table(release)) => Some(read_args(release.iter())?),
        Some(_) => return Err(BindingProblem::BadValue("release".to_owned())),
    };
    let binding = Binding {
        chord,
        command: spec.id,
        args,
        release,
    };
    // Build both actions once, so a wrong parameter is reported when the file is read
    // instead of being ignored when the key is pressed.
    for pressed in [true, false] {
        if !pressed && !binding.is_held() {
            continue;
        }
        let mut args = match (&binding.release, pressed) {
            (Some(release), false) => release.clone(),
            _ => binding.args.clone(),
        };
        if has_action_param(binding.command) {
            args.insert(ACTION_PARAM.to_owned(), Value::Text("press".to_owned()));
        }
        Action::from_args(binding.command, &args).map_err(BindingProblem::Command)?;
    }
    Ok(binding)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Keymap {
    bindings: Vec<Binding>,
}

/// The shortcuts of a fresh installation, in the format of the file.
pub const DEFAULT_KEYMAP: &str = include_str!("../keymap.default.toml");

impl Keymap {
    /// The shortcuts compiled into the binary. They are checked by a test, so reading them
    /// cannot fail; if it ever did the application would start without shortcuts.
    pub fn defaults() -> Self {
        Self::parse(DEFAULT_KEYMAP).unwrap_or_default()
    }

    pub fn parse(source: &str) -> Result<Self, KeymapError> {
        let root: toml::Table = source
            .parse()
            .map_err(|error: toml::de::Error| KeymapError::Syntax(error.message().to_owned()))?;
        let list = match root.get("binding") {
            None => return Ok(Self::default()),
            Some(toml::Value::Array(list)) => list,
            Some(_) => return Err(KeymapError::Syntax("`binding` is not a list".to_owned())),
        };
        if list.len() > MAX_BINDINGS {
            return Err(KeymapError::TooManyBindings);
        }
        let bindings = list
            .iter()
            .enumerate()
            .map(|(index, value)| {
                read_binding(value).map_err(|problem| KeymapError::Binding {
                    number: index + 1,
                    problem,
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { bindings })
    }

    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// Whether some shortcut triggers `command`.
    pub fn has_command(&self, command: &str) -> bool {
        self.bindings
            .iter()
            .any(|binding| binding.command == command)
    }
}

/// Which held bindings are down, so each release is sent once and only after its press.
#[derive(Clone, Debug, Default)]
pub struct KeymapState {
    held: Vec<usize>,
}

impl KeymapState {
    /// Turns the key events of a frame into actions. `claimed` tells whether the focused
    /// control uses a key itself; such a key does not trigger a shortcut.
    pub fn actions(
        &mut self,
        keymap: &Keymap,
        events: &[egui::Event],
        claimed: impl Fn(Key, &Modifiers) -> bool,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        for event in events {
            let egui::Event::Key {
                key,
                pressed,
                repeat,
                modifiers,
                ..
            } = event
            else {
                continue;
            };
            if *repeat {
                continue;
            }
            if *pressed {
                if claimed(*key, modifiers) {
                    continue;
                }
                for (index, binding) in keymap.bindings.iter().enumerate() {
                    if !binding.chord.matches(*key, modifiers) {
                        continue;
                    }
                    if binding.is_held() {
                        if self.held.contains(&index) {
                            continue;
                        }
                        self.held.push(index);
                    }
                    actions.extend(binding.action(true));
                }
            } else {
                // A release matches by key alone: the modifier may have come up first.
                self.held.retain(|index| match keymap.bindings.get(*index) {
                    Some(binding) if binding.chord.key == *key => {
                        actions.extend(binding.action(false));
                        false
                    }
                    Some(_) => true,
                    None => false,
                });
            }
        }
        actions
    }

    /// Releases everything that is held: the window lost the keyboard, or the keymap was
    /// replaced, and the key-up events will not arrive.
    pub fn release_all(&mut self, keymap: &Keymap) -> Vec<Action> {
        self.held
            .drain(..)
            .filter_map(|index| keymap.bindings.get(index)?.action(false))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use egui::{Event, Key, Modifiers};
    use platter_core::DeckId;
    use platter_engine::Command;

    use super::{BindingProblem, Chord, Keymap, KeymapError, KeymapState};
    use crate::command::{Action, AppCommand};

    fn key(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        }
    }

    fn never(_: Key, _: &Modifiers) -> bool {
        false
    }

    #[test]
    fn chords_are_read_and_written_the_same_way() {
        let chord = Chord::parse("shift + left").unwrap();
        assert_eq!(
            (chord.key, chord.shift, chord.command),
            (Key::ArrowLeft, true, false)
        );
        assert_eq!(chord.to_string(), "Shift + Left");
        assert_eq!(Chord::parse("Ctrl+,").unwrap().key, Key::Comma);
        assert_eq!(Chord::parse("Z"), Some(Chord::plain(Key::Z)));
        assert_eq!(
            Chord::parse("Ctrl + Alt + Shift + F5").unwrap().to_string(),
            "Ctrl + Alt + Shift + F5"
        );
        for bad in ["", "Shift", "A + B", "Hyper + A", "NoSuchKey"] {
            assert_eq!(Chord::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_default_shortcuts_are_valid_and_cover_both_decks() {
        let keymap = Keymap::parse(super::DEFAULT_KEYMAP).unwrap();
        assert_eq!(keymap, Keymap::defaults());
        assert!(keymap.bindings().len() >= 20);
        for command in [
            "deck.play_pause",
            "deck.cue",
            "deck.cup",
            "browser.load_selected",
        ] {
            let decks: Vec<_> = keymap
                .bindings()
                .iter()
                .filter(|binding| binding.command == command)
                .map(|binding| binding.args.get("deck").cloned())
                .collect();
            assert_eq!(decks.len(), 2, "{command}");
            assert_ne!(decks[0], decks[1], "{command}");
        }
        // No chord is bound twice.
        for (index, binding) in keymap.bindings().iter().enumerate() {
            assert!(
                keymap.bindings()[..index]
                    .iter()
                    .all(|other| other.chord != binding.chord),
                "{}",
                binding.chord
            );
        }
    }

    #[test]
    fn a_key_press_emits_the_command_of_its_binding() {
        let keymap = Keymap::parse(
            "[[binding]]\nkeys = \"Z\"\ncommand = \"deck.play_pause\"\ndeck = \"a\"\n\n\
             [[binding]]\nkeys = \"Shift + Right\"\ncommand = \"browser.load_selected\"\ndeck = \"b\"\n",
        )
        .unwrap();
        let mut state = KeymapState::default();
        let events = [
            key(Key::Z, true, Modifiers::NONE),
            key(Key::Z, false, Modifiers::NONE),
            key(Key::ArrowRight, true, Modifiers::SHIFT),
            // Without the modifier the chord does not match.
            key(Key::ArrowRight, true, Modifiers::NONE),
        ];
        assert_eq!(
            state.actions(&keymap, &events, never),
            [
                Action::Engine(Command::DeckPlayPause { deck: DeckId::A }),
                Action::App(AppCommand::BrowserLoadSelected { deck: DeckId::B }),
            ]
        );
    }

    #[test]
    fn a_binding_of_a_hotcue_sends_the_press_and_the_release_of_its_slot() {
        let keymap = Keymap::parse(
            "[[binding]]
keys = \"G\"
command = \"deck.hotcue.trigger\"
deck = \"b\"
slot = 3
",
        )
        .unwrap();
        let mut state = KeymapState::default();
        let events = [
            key(Key::G, true, Modifiers::NONE),
            key(Key::G, false, Modifiers::NONE),
        ];
        let trigger = |pressed| {
            Action::Engine(Command::DeckHotcueTrigger {
                deck: DeckId::B,
                slot: platter_core::HotcueSlot::ALL[2],
                pressed,
            })
        };
        assert_eq!(
            state.actions(&keymap, &events, never),
            [trigger(true), trigger(false)]
        );
    }

    #[test]
    fn held_commands_send_press_and_release_once_each() {
        let keymap = Keymap::parse(
            "[[binding]]\nkeys = \"X\"\ncommand = \"deck.cue\"\ndeck = \"a\"\n\n\
             [[binding]]\nkeys = \"Shift + S\"\ncommand = \"deck.nudge\"\ndeck = \"a\"\namount = -0.04\nrelease = { deck = \"a\", amount = 0.0 }\n",
        )
        .unwrap();
        let mut state = KeymapState::default();
        let mut repeat = key(Key::X, true, Modifiers::NONE);
        if let Event::Key { repeat, .. } = &mut repeat {
            *repeat = true;
        }
        let events = [
            key(Key::X, true, Modifiers::NONE),
            repeat,
            key(Key::X, true, Modifiers::NONE),
            key(Key::S, true, Modifiers::SHIFT),
            // Shift comes up before the key: the release still arrives.
            key(Key::S, false, Modifiers::NONE),
            key(Key::X, false, Modifiers::NONE),
            key(Key::X, false, Modifiers::NONE),
        ];
        let deck = DeckId::A;
        assert_eq!(
            state.actions(&keymap, &events, never),
            [
                Action::Engine(Command::DeckCue {
                    deck,
                    pressed: true
                }),
                Action::Engine(Command::DeckNudge {
                    deck,
                    amount: -0.04
                }),
                Action::Engine(Command::DeckNudge { deck, amount: 0.0 }),
                Action::Engine(Command::DeckCue {
                    deck,
                    pressed: false
                }),
            ]
        );
    }

    #[test]
    fn a_key_the_focused_control_uses_triggers_no_shortcut() {
        let keymap =
            Keymap::parse("[[binding]]\nkeys = \"Space\"\ncommand = \"deck.cup\"\ndeck = \"a\"\n")
                .unwrap();
        let mut state = KeymapState::default();
        let events = [
            key(Key::Space, true, Modifiers::NONE),
            key(Key::Space, false, Modifiers::NONE),
        ];
        let claimed = |key: Key, _: &Modifiers| key == Key::Space;
        // Neither the press nor a release without its press.
        assert_eq!(state.actions(&keymap, &events, claimed), []);
    }

    #[test]
    fn losing_the_keyboard_releases_what_is_held() {
        let keymap =
            Keymap::parse("[[binding]]\nkeys = \"C\"\ncommand = \"deck.cup\"\ndeck = \"b\"\n")
                .unwrap();
        let mut state = KeymapState::default();
        state.actions(&keymap, &[key(Key::C, true, Modifiers::NONE)], never);
        let deck = DeckId::B;
        assert_eq!(
            state.release_all(&keymap),
            [Action::Engine(Command::DeckCup {
                deck,
                pressed: false
            })]
        );
        assert_eq!(state.release_all(&keymap), []);
    }

    #[test]
    fn a_file_with_a_mistake_names_the_binding() {
        let problem = |source: &str| match Keymap::parse(source) {
            Err(KeymapError::Binding { number, problem }) => (number, problem),
            other => panic!("{other:?}"),
        };
        let good = "[[binding]]\nkeys = \"Z\"\ncommand = \"deck.play_pause\"\ndeck = \"a\"\n\n";
        assert_eq!(
            problem(&format!("{good}[[binding]]\ncommand = \"deck.play\"\n")),
            (2, BindingProblem::MissingKeys)
        );
        assert_eq!(
            problem("[[binding]]\nkeys = \"Hyper + Q\"\ncommand = \"deck.play\"\n"),
            (1, BindingProblem::UnknownKeys("Hyper + Q".to_owned()))
        );
        assert_eq!(
            problem("[[binding]]\nkeys = \"Q\"\n"),
            (1, BindingProblem::MissingCommand)
        );
        assert!(matches!(
            problem("[[binding]]\nkeys = \"Q\"\ncommand = \"deck.fly\"\n"),
            (1, BindingProblem::Command(_))
        ));
        // A parameter the command does not have, a missing one, and a value of a kind a
        // parameter cannot be.
        for body in ["deck = \"a\"\nspeed = 2", "", "deck = true"] {
            let source = format!("[[binding]]\nkeys = \"Q\"\ncommand = \"deck.play\"\n{body}\n");
            assert!(Keymap::parse(&source).is_err(), "{body}");
        }
        assert!(matches!(
            Keymap::parse("binding = 3"),
            Err(KeymapError::Syntax(_))
        ));
        assert!(matches!(
            Keymap::parse("[[binding"),
            Err(KeymapError::Syntax(_))
        ));
        assert_eq!(Keymap::parse(""), Ok(Keymap::default()));
    }
}
