//! The controller manager of the preferences: the mapping of a controller as an editable
//! document, the rows of its table, and the control a message names for learn mode.
//!
//! Behaviour note: `docs/behavior/controllers.md`, *The controller manager*.

use std::path::Path;

use platter_control::{MAX_FILE_BYTES, Mapping, Message};
use platter_core::DeckId;
use platter_engine::command::{CommandSpec, ParamKind};
use platter_ui_egui::model::{BindingRow, ControlType};

use crate::midi;

/// A mapping as the page edits it: the tables of its bindings, those waiting for Learn too.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    bindings: Vec<toml::Table>,
}

impl Document {
    /// Reads the mapping file at `path`; a file that is not there is an empty mapping.
    pub fn read(path: &Path) -> Result<Self, String> {
        let source = match std::fs::read(path) {
            Ok(bytes) if bytes.len() > MAX_FILE_BYTES => {
                return Err(format!("larger than {MAX_FILE_BYTES} bytes"));
            }
            Ok(bytes) => String::from_utf8(bytes).map_err(|error| error.to_string())?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error.to_string()),
        };
        Self::parse(&source)
    }

    pub fn parse(source: &str) -> Result<Self, String> {
        let table: toml::Table = source
            .parse()
            .map_err(|error: toml::de::Error| error.to_string())?;
        let bindings = match table.get("binding") {
            Some(toml::Value::Array(entries)) => entries
                .iter()
                .filter_map(|entry| entry.as_table().cloned())
                .collect(),
            _ => Vec::new(),
        };
        Ok(Self { bindings })
    }

    /// The text of the mapping file: the bindings that have a control.
    pub fn to_text(&self) -> String {
        let learned: Vec<toml::Value> = self
            .bindings
            .iter()
            .filter(|binding| binding.contains_key("midi"))
            .cloned()
            .map(toml::Value::Table)
            .collect();
        let mut table = toml::Table::new();
        if !learned.is_empty() {
            table.insert("binding".to_owned(), toml::Value::Array(learned));
        }
        toml::to_string(&table).unwrap_or_default()
    }

    /// The mapping the file would hold, checked as the application checks mappings.
    pub fn mapping(&self) -> Result<Mapping, String> {
        let mapping = Mapping::parse(&self.to_text()).map_err(|error| error.to_string())?;
        midi::check(&mapping).map_err(|problem| problem.to_string())?;
        Ok(mapping)
    }

    /// Writes the mapping file, only when the mapping it holds is one the application uses.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        self.mapping()?;
        std::fs::write(path, self.to_text()).map_err(|error| error.to_string())
    }

    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// The rows of the table, in the order of the file.
    pub fn rows(&self) -> Vec<BindingRow> {
        self.bindings.iter().map(row_of).collect()
    }

    /// Gives the binding at `index` the control `midi`.
    pub fn learn(&mut self, index: usize, midi: &str) -> bool {
        let Some(binding) = self.bindings.get_mut(index) else {
            return false;
        };
        binding.insert("midi".to_owned(), toml::Value::String(midi.to_owned()));
        true
    }

    /// A copy of the binding at `index`, after it and without its control.
    pub fn duplicate(&mut self, index: usize) -> Option<usize> {
        let mut copy = self.bindings.get(index)?.clone();
        copy.remove("midi");
        self.bindings.insert(index + 1, copy);
        Some(index + 1)
    }

    pub fn delete(&mut self, index: usize) -> bool {
        if index < self.bindings.len() {
            self.bindings.remove(index);
            true
        } else {
            false
        }
    }

    /// A new binding of `spec` on `deck`, at the end, waiting for its control: a button for a
    /// command whose only parameter is the deck, a fader over the range of its number
    /// otherwise.
    pub fn add(&mut self, spec: &CommandSpec, deck: DeckId) -> Option<usize> {
        let mut binding = toml::Table::new();
        binding.insert(
            "command".to_owned(),
            toml::Value::String(spec.id.to_owned()),
        );
        for param in spec.params {
            match param.kind {
                ParamKind::Deck => {
                    binding.insert(
                        "deck".to_owned(),
                        toml::Value::String(deck.letter().to_ascii_lowercase().to_string()),
                    );
                }
                ParamKind::Number { .. } => {
                    binding.insert(
                        "control".to_owned(),
                        toml::Value::String("fader".to_owned()),
                    );
                    binding.insert(
                        "param".to_owned(),
                        toml::Value::String(param.name.to_owned()),
                    );
                }
                _ => return None,
            }
        }
        self.bindings.push(binding);
        Some(self.bindings.len() - 1)
    }
}

/// The row of the table for one binding of a mapping.
fn row_of(binding: &toml::Table) -> BindingRow {
    let text = |key: &str| binding.get(key).and_then(toml::Value::as_str);
    let integer = |key: &str| {
        binding
            .get(key)
            .and_then(toml::Value::as_integer)
            .and_then(|value| u8::try_from(value).ok())
    };
    let hold = binding
        .get("hold")
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    let midi = text("midi").map(str::to_owned);
    let control = match text("control") {
        Some("encoder") => ControlType::Encoder,
        Some("fader") => ControlType::Fader,
        Some("button") => button(hold),
        // A control change is a fader unless it says otherwise; the pitch bend is one.
        _ if midi
            .as_deref()
            .is_some_and(|midi| midi.starts_with("cc") || midi.starts_with("pitch")) =>
        {
            ControlType::Fader
        }
        _ => button(hold),
    };
    let when = binding
        .get("when")
        .and_then(toml::Value::as_array)
        .map(|conditions| {
            conditions
                .iter()
                .filter_map(|condition| {
                    let (modifier, value) = condition.as_str()?.split_once('=')?;
                    Some((modifier.trim().parse().ok()?, value.trim().parse().ok()?))
                })
                .collect()
        })
        .unwrap_or_default();
    BindingRow {
        command: text("command").map(str::to_owned),
        deck: text("deck").and_then(|deck| DeckId::parse(deck).ok()),
        control,
        when,
        sets: integer("modifier").zip(integer("value")),
        midi,
    }
}

fn button(hold: bool) -> ControlType {
    if hold {
        ControlType::ButtonHeld
    } else {
        ControlType::Button
    }
}

/// The control a message comes from, as a mapping names it, for learn mode: a note going
/// down, a control change or a pitch bend. A note coming up names nothing.
pub fn learned(bytes: &[u8]) -> Option<String> {
    match Message::parse(bytes)? {
        Message::NoteOn {
            channel,
            note,
            velocity,
        } if velocity > 0 => Some(format!("note {} {note}", channel + 1)),
        Message::ControlChange {
            channel, number, ..
        } => Some(format!("cc {} {number}", channel + 1)),
        Message::PitchBend { channel, .. } => Some(format!("pitch {}", channel + 1)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use platter_engine::command::SPECS;

    use super::*;

    const SOURCE: &str = r#"
[[binding]]
midi = "note 1 36"
command = "deck.play_pause"
deck = "a"

[[binding]]
midi = "cc 1 7"
command = "mixer.fader.set"
deck = "a"
param = "position"

[[binding]]
midi = "note 1 40"
modifier = 1
value = 1
hold = true

[[binding]]
midi = "cc 1 20"
command = "deck.beatjump"
deck = "a"
control = "encoder"
up = { direction = "forward" }
down = { direction = "back" }
when = ["1 = 1"]
"#;

    fn spec(id: &str) -> &'static CommandSpec {
        SPECS.iter().find(|spec| spec.id == id).unwrap()
    }

    #[test]
    fn the_rows_say_what_each_binding_is() {
        let document = Document::parse(SOURCE).unwrap();
        let rows = document.rows();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].command.as_deref(), Some("deck.play_pause"));
        assert_eq!(rows[0].deck, Some(DeckId::A));
        assert_eq!(rows[0].control, ControlType::Button);
        assert_eq!(rows[1].control, ControlType::Fader);
        assert_eq!(rows[2].command, None);
        assert_eq!(rows[2].sets, Some((1, 1)));
        assert_eq!(rows[2].control, ControlType::ButtonHeld);
        assert_eq!(rows[3].control, ControlType::Encoder);
        assert_eq!(rows[3].when, [(1, 1)]);
        assert_eq!(rows[3].midi.as_deref(), Some("cc 1 20"));
    }

    #[test]
    fn edits_are_written_only_once_a_binding_has_its_control() {
        let mut document = Document::parse(SOURCE).unwrap();
        // A new button waits for Learn: the file does not hold it yet.
        let added = document.add(spec("deck.sync.toggle"), DeckId::B).unwrap();
        assert_eq!(added, 4);
        assert_eq!(document.rows()[4].midi, None);
        assert_eq!(document.rows()[4].deck, Some(DeckId::B));
        assert_eq!(Document::parse(&document.to_text()).unwrap().len(), 4);
        assert!(document.learn(added, "note 2 50"));
        let written = Document::parse(&document.to_text()).unwrap();
        assert_eq!(written.len(), 5);
        assert_eq!(written.rows()[4].midi.as_deref(), Some("note 2 50"));
        assert!(
            document.mapping().is_ok(),
            "{:?}
{}",
            document.mapping(),
            document.to_text()
        );

        // A fader takes the number of its command as its parameter.
        let fader = document
            .add(spec("mixer.crossfader.set"), DeckId::A)
            .unwrap();
        assert_eq!(document.rows()[fader].control, ControlType::Fader);
        document.learn(fader, "cc 1 8");
        assert!(document.mapping().is_ok(), "{:?}", document.mapping());

        // A copy goes after its original, without a control; Delete removes a row.
        let copy = document.duplicate(0).unwrap();
        assert_eq!(copy, 1);
        assert_eq!(document.rows()[1].midi, None);
        assert_eq!(
            document.rows()[1].command.as_deref(),
            Some("deck.play_pause")
        );
        assert!(document.delete(1));
        assert!(!document.delete(99));
        assert_eq!(document.duplicate(99), None);
    }

    #[test]
    fn a_command_with_other_parameters_cannot_be_added() {
        let mut document = Document::default();
        assert_eq!(document.add(spec("deck.load"), DeckId::A), None);
        assert_eq!(document.len(), 0);
    }

    #[test]
    fn a_mapping_with_a_mistake_is_not_written() {
        let folder = std::env::temp_dir()
            .join("platter-app-tests")
            .join(format!("manager-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("mapping.toml");
        std::fs::write(&path, SOURCE).unwrap();
        let mut document = Document::read(&path).unwrap();
        // A tempo out of the range of its command.
        document.learn(0, "note 1 36");
        document.bindings[0].insert(
            "command".to_owned(),
            toml::Value::String("deck.tempo.set".to_owned()),
        );
        document.bindings[0].insert("ratio".to_owned(), toml::Value::Float(9.0));
        assert!(document.write(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SOURCE);
        // A file that is not there is an empty mapping.
        assert_eq!(Document::read(&folder.join("none.toml")).unwrap().len(), 0);
    }

    #[test]
    fn learn_takes_the_control_a_message_comes_from() {
        assert_eq!(learned(&[0x90, 36, 100]).as_deref(), Some("note 1 36"));
        assert_eq!(learned(&[0x91, 36, 0]), None);
        assert_eq!(learned(&[0x80, 36, 0]), None);
        assert_eq!(learned(&[0xB2, 14, 64]).as_deref(), Some("cc 3 14"));
        assert_eq!(learned(&[0xE0, 0, 64]).as_deref(), Some("pitch 1"));
        assert_eq!(learned(&[0xF8]), None);
    }
}
