//! The files of the user: where they live and what the settings file holds.
//!
//! Platter keeps three files in a folder of its own inside the configuration folder of the
//! user: `settings.toml`, `keymap.toml` and `platter.log`. A settings file is untrusted
//! input like any other: a value that is missing or makes no sense falls back to its
//! default, one setting at a time, and never stops the application.

use std::io::Write;
use std::path::{Path, PathBuf};

use platter_audio_io::{BUFFER_SIZES, Routing, SAMPLE_RATES};
use platter_engine::{CrossfaderCurve, MAX_FADE_BEATS, MIN_FADE_BEATS, MixerMode};
use platter_ui_egui::command::MAX_OUTPUTS;
use platter_ui_egui::i18n::Language;

pub const SETTINGS_FILE: &str = "settings.toml";
pub const KEYMAP_FILE: &str = "keymap.toml";
pub const LOG_FILE: &str = "platter.log";
/// Roots of the file selector a settings file may name.
const MAX_ROOTS: usize = 32;
/// MIDI controllers a settings file may name.
pub const MAX_CONTROLLERS: usize = 8;

/// The folder of Platter inside the configuration folder of the user, by the convention of
/// each system. `None` when the environment names no such folder.
pub fn config_dir() -> Option<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(windows) {
        var("APPDATA").map(|base| base.join("Platter"))
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|home| {
            home.join("Library")
                .join("Application Support")
                .join("Platter")
        })
    } else {
        var("XDG_CONFIG_HOME")
            .or_else(|| var("HOME").map(|home| home.join(".config")))
            .map(|base| base.join("platter"))
    }
}

/// The home folder of the user, for the file selector.
pub fn home_dir() -> Option<PathBuf> {
    let name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// A MIDI controller the settings file names: its input and its mapping file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Controller {
    /// Name of the MIDI input, as the system gives it.
    pub input: String,
    /// The mapping file; without a folder, in the folder of the settings file.
    pub mapping: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub language: Language,
    /// Name of the audio device; `None` is the default device of the system.
    pub device: Option<String>,
    pub sample_rate: u32,
    pub buffer_frames: u32,
    pub routing: Routing,
    pub crossfader_curve: CrossfaderCurve,
    /// The keys show their tooltips.
    pub tooltips: bool,
    /// The settings of the Mixer page: `docs/behavior/settings.md`.
    pub mixer_mode: MixerMode,
    pub autogain: bool,
    pub external_eq_gain: bool,
    pub external_master_volume: bool,
    /// Beats of an automatic fade across the whole travel of the crossfader.
    pub fade_beats: u32,
    /// The folder the file selector showed last.
    pub folder: Option<PathBuf>,
    /// Folders shown as roots of the file selector beside the home folder, such as a
    /// drive with music. The user writes them in the settings file.
    pub roots: Vec<PathBuf>,
    /// MIDI controllers, until the controller manager lets the user choose them
    /// (`docs/behavior/controllers.md`, section *In the application*).
    pub midi: Vec<Controller>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::English,
            device: None,
            sample_rate: 48_000,
            buffer_frames: 256,
            routing: Routing::default(),
            crossfader_curve: CrossfaderCurve::default(),
            tooltips: true,
            mixer_mode: MixerMode::Internal,
            autogain: true,
            external_eq_gain: false,
            external_master_volume: false,
            fade_beats: 8,
            folder: None,
            roots: Vec::new(),
            midi: Vec::new(),
        }
    }
}

impl Settings {
    /// Reads settings from the text of a settings file. What cannot be used is replaced by
    /// its default.
    pub fn parse(source: &str) -> Self {
        let defaults = Self::default();
        let Ok(root) = source.parse::<toml::Table>() else {
            return defaults;
        };
        let table = |name: &str| root.get(name).and_then(toml::Value::as_table);
        let text =
            |table: Option<&toml::Table>, name: &str| table?.get(name)?.as_str().map(str::to_owned);
        let number = |table: Option<&toml::Table>, name: &str| {
            u32::try_from(table?.get(name)?.as_integer()?).ok()
        };
        let flag = |table: Option<&toml::Table>, name: &str| table?.get(name)?.as_bool();
        let audio = table("audio");
        let routing = audio
            .and_then(|audio| audio.get("routing"))
            .and_then(toml::Value::as_table);
        let output = |name: &str, default: u16| {
            number(routing, name)
                .and_then(|output| u16::try_from(output).ok())
                .filter(|output| *output < MAX_OUTPUTS)
                .unwrap_or(default)
        };
        Self {
            language: text(Some(&root), "language")
                .and_then(|code| Language::from_code(&code))
                .unwrap_or(defaults.language),
            tooltips: flag(Some(&root), "tooltips").unwrap_or(defaults.tooltips),
            device: text(audio, "device").filter(|name| !name.is_empty()),
            sample_rate: number(audio, "sample_rate")
                .filter(|rate| SAMPLE_RATES.contains(rate))
                .unwrap_or(defaults.sample_rate),
            buffer_frames: number(audio, "buffer_frames")
                .filter(|frames| BUFFER_SIZES.contains(frames))
                .unwrap_or(defaults.buffer_frames),
            routing: Routing {
                master: [
                    output("master_left", defaults.routing.master[0]),
                    output("master_right", defaults.routing.master[1]),
                ],
                headphones: [
                    output("headphones_left", defaults.routing.headphones[0]),
                    output("headphones_right", defaults.routing.headphones[1]),
                ],
            },
            crossfader_curve: text(table("mixer"), "crossfader_curve")
                .and_then(|name| CrossfaderCurve::parse(&name).ok())
                .unwrap_or(defaults.crossfader_curve),
            mixer_mode: text(table("mixer"), "mode")
                .and_then(|name| MixerMode::ALL.into_iter().find(|mode| mode.name() == name))
                .unwrap_or(defaults.mixer_mode),
            autogain: flag(table("mixer"), "autogain").unwrap_or(defaults.autogain),
            external_eq_gain: flag(table("mixer"), "external_eq_gain")
                .unwrap_or(defaults.external_eq_gain),
            external_master_volume: flag(table("mixer"), "external_master_volume")
                .unwrap_or(defaults.external_master_volume),
            fade_beats: number(table("mixer"), "fade_beats")
                .filter(|beats| (MIN_FADE_BEATS..=MAX_FADE_BEATS).contains(&f64::from(*beats)))
                .unwrap_or(defaults.fade_beats),
            folder: text(table("browser"), "folder")
                .filter(|folder| !folder.is_empty())
                .map(PathBuf::from),
            roots: table("browser")
                .and_then(|browser| browser.get("roots"))
                .and_then(toml::Value::as_array)
                .map(|roots| {
                    roots
                        .iter()
                        .filter_map(toml::Value::as_str)
                        .filter(|root| !root.is_empty())
                        .take(MAX_ROOTS)
                        .map(PathBuf::from)
                        .collect()
                })
                .unwrap_or_default(),
            midi: root
                .get("midi")
                .and_then(toml::Value::as_array)
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(toml::Value::as_table)
                        .filter_map(|entry| {
                            let input = text(Some(entry), "input")?;
                            let mapping = text(Some(entry), "mapping")?;
                            (!input.is_empty() && !mapping.is_empty()).then(|| Controller {
                                input,
                                mapping: PathBuf::from(mapping),
                            })
                        })
                        .take(MAX_CONTROLLERS)
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// The text of the settings file for these settings.
    pub fn to_toml(&self) -> String {
        let integer = |value: u32| toml::Value::Integer(i64::from(value));
        let mut routing = toml::Table::new();
        for (name, output) in [
            ("master_left", self.routing.master[0]),
            ("master_right", self.routing.master[1]),
            ("headphones_left", self.routing.headphones[0]),
            ("headphones_right", self.routing.headphones[1]),
        ] {
            routing.insert(name.to_owned(), integer(u32::from(output)));
        }
        let mut audio = toml::Table::new();
        if let Some(device) = &self.device {
            audio.insert("device".to_owned(), toml::Value::String(device.clone()));
        }
        audio.insert("sample_rate".to_owned(), integer(self.sample_rate));
        audio.insert("buffer_frames".to_owned(), integer(self.buffer_frames));
        audio.insert("routing".to_owned(), toml::Value::Table(routing));
        let mut mixer = toml::Table::new();
        mixer.insert(
            "crossfader_curve".to_owned(),
            toml::Value::String(self.crossfader_curve.name().to_owned()),
        );
        mixer.insert(
            "mode".to_owned(),
            toml::Value::String(self.mixer_mode.name().to_owned()),
        );
        for (name, on) in [
            ("autogain", self.autogain),
            ("external_eq_gain", self.external_eq_gain),
            ("external_master_volume", self.external_master_volume),
        ] {
            mixer.insert(name.to_owned(), toml::Value::Boolean(on));
        }
        mixer.insert("fade_beats".to_owned(), integer(self.fade_beats));
        let mut root = toml::Table::new();
        root.insert(
            "language".to_owned(),
            toml::Value::String(self.language.code().to_owned()),
        );
        root.insert("tooltips".to_owned(), toml::Value::Boolean(self.tooltips));
        root.insert("audio".to_owned(), toml::Value::Table(audio));
        root.insert("mixer".to_owned(), toml::Value::Table(mixer));
        let mut browser = toml::Table::new();
        if let Some(folder) = self.folder.as_ref().and_then(|folder| folder.to_str()) {
            browser.insert("folder".to_owned(), toml::Value::String(folder.to_owned()));
        }
        let roots: Vec<toml::Value> = self
            .roots
            .iter()
            .filter_map(|root| root.to_str())
            .map(|root| toml::Value::String(root.to_owned()))
            .collect();
        if !roots.is_empty() {
            browser.insert("roots".to_owned(), toml::Value::Array(roots));
        }
        if !browser.is_empty() {
            root.insert("browser".to_owned(), toml::Value::Table(browser));
        }
        let midi: Vec<toml::Value> = self
            .midi
            .iter()
            .filter_map(|controller| {
                let mut entry = toml::Table::new();
                entry.insert(
                    "input".to_owned(),
                    toml::Value::String(controller.input.clone()),
                );
                entry.insert(
                    "mapping".to_owned(),
                    toml::Value::String(controller.mapping.to_str()?.to_owned()),
                );
                Some(toml::Value::Table(entry))
            })
            .collect();
        if !midi.is_empty() {
            root.insert("midi".to_owned(), toml::Value::Array(midi));
        }
        root.to_string()
    }

    /// Reads the settings file; a file that is missing or cannot be read gives the
    /// defaults.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .map_or_else(|_| Self::default(), |source| Self::parse(&source))
    }

    /// Writes the settings file: to a temporary file first, then renamed over the old one,
    /// so a crash in between leaves the old file whole.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let temporary = path.with_extension("toml.tmp");
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(self.to_toml().as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use platter_audio_io::Routing;
    use platter_engine::{CrossfaderCurve, MixerMode};
    use platter_ui_egui::i18n::Language;

    use super::{Controller, MAX_CONTROLLERS, Settings};

    #[test]
    fn settings_survive_a_round_trip_through_their_file() {
        let settings = Settings {
            language: Language::Spanish,
            device: Some("Interface with \"quotes\" and ñ".to_owned()),
            sample_rate: 44_100,
            buffer_frames: 512,
            routing: Routing {
                master: [2, 3],
                headphones: [0, 1],
            },
            crossfader_curve: CrossfaderCurve::Sharp,
            tooltips: false,
            mixer_mode: MixerMode::External,
            autogain: false,
            external_eq_gain: true,
            external_master_volume: true,
            fade_beats: 32,
            folder: Some(PathBuf::from("music").join("sets")),
            roots: vec![PathBuf::from("records"), PathBuf::from("more records")],
            midi: vec![
                Controller {
                    input: "Example Controller".to_owned(),
                    mapping: PathBuf::from("example.toml"),
                },
                Controller {
                    input: "Second \"Pad\" 2".to_owned(),
                    mapping: PathBuf::from("mappings").join("pad.toml"),
                },
            ],
        };
        assert_eq!(Settings::parse(&settings.to_toml()), settings);
        assert_eq!(
            Settings::parse(&Settings::default().to_toml()),
            Settings::default()
        );
    }

    #[test]
    fn what_cannot_be_used_falls_back_to_its_default_one_setting_at_a_time() {
        let source = "language = \"fr\"\n[audio]\ndevice = \"\"\nsample_rate = 12345\n\
                      buffer_frames = 512\n[audio.routing]\nmaster_left = 900\nmaster_right = 5\n\
                      [mixer]\ncrossfader_curve = 7\nmode = \"hardware\"\nautogain = \"no\"\n\
                      external_eq_gain = true\nfade_beats = 65\n";
        let settings = Settings::parse(source);
        let defaults = Settings::default();
        assert_eq!(settings.language, defaults.language);
        assert_eq!(settings.device, None);
        assert_eq!(settings.sample_rate, defaults.sample_rate);
        // The values that make sense are kept.
        assert_eq!(settings.buffer_frames, 512);
        assert_eq!(settings.routing.master, [0, 5]);
        assert_eq!(settings.routing.headphones, [2, 3]);
        assert_eq!(settings.crossfader_curve, defaults.crossfader_curve);
        assert_eq!(settings.mixer_mode, defaults.mixer_mode);
        assert_eq!(settings.autogain, defaults.autogain);
        assert!(settings.external_eq_gain);
        assert_eq!(settings.fade_beats, defaults.fade_beats);
    }

    #[test]
    fn controllers_without_an_input_or_a_mapping_are_left_out_and_the_rest_bounded() {
        let mut source = String::from(
            "midi = [{ input = \"\", mapping = \"a.toml\" }, { input = \"Pad\" }, 7,              { input = \"Pad\", mapping = 3 }]
",
        );
        assert!(Settings::parse(&source).midi.is_empty());
        source = (0..MAX_CONTROLLERS + 3)
            .map(|index| {
                format!(
                    "[[midi]]
input = \"Pad {index}\"
mapping = \"pad.toml\"
"
                )
            })
            .collect();
        let midi = Settings::parse(&source).midi;
        assert_eq!(midi.len(), MAX_CONTROLLERS);
        assert_eq!(midi[0].input, "Pad 0");
        // A file whose `midi` is not a list keeps the other settings.
        let settings = Settings::parse(
            "language = \"es\"
midi = \"Pad\"
",
        );
        assert_eq!(settings.language, Language::Spanish);
        assert!(settings.midi.is_empty());
    }

    #[test]
    fn a_file_that_is_not_toml_or_is_missing_gives_the_defaults() {
        assert_eq!(Settings::parse("[[[ not toml"), Settings::default());
        assert_eq!(Settings::parse(""), Settings::default());
        let missing = PathBuf::from("no-such-settings.toml");
        assert_eq!(Settings::load(&missing), Settings::default());
    }

    #[test]
    fn saving_writes_the_file_and_leaves_no_temporary_one() {
        let folder =
            std::env::temp_dir().join(format!("platter-settings-test-{}", std::process::id()));
        let path = folder.join("nested").join("settings.toml");
        let settings = Settings {
            buffer_frames: 128,
            ..Settings::default()
        };
        settings.save(&path).unwrap();
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        assert!(!path.with_extension("toml.tmp").exists());
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
