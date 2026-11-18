//! The files of the user: where they live and what the settings file holds.
//!
//! Platter keeps three files in a folder of its own inside the configuration folder of the
//! user: `settings.toml`, `keymap.toml` and `platter.log`. A settings file is untrusted
//! input like any other: a value that is missing or makes no sense falls back to its
//! default, one setting at a time, and never stops the application.

use std::io::Write;
use std::path::{Path, PathBuf};

use platter_audio_io::{BUFFER_SIZES, Routing, SAMPLE_RATES};
use platter_engine::{
    CrossfaderCurve, MAX_FADE_BEATS, MIN_FADE_BEATS, MixerMode, SyncMode, TempoRange,
};
use platter_ui_egui::collection::TagWriting;
use platter_ui_egui::command::{DeckLayout, DeckSize, ListColumn, MAX_OUTPUTS};
use platter_ui_egui::i18n::Language;
use platter_ui_egui::keys::KeyNotation;
use platter_ui_egui::played::{DEFAULT_PLAYTIME, MAX_PLAYTIME, MIN_PLAYTIME};

pub const SETTINGS_FILE: &str = "settings.toml";
pub const KEYMAP_FILE: &str = "keymap.toml";
pub const LOG_FILE: &str = "platter.log";
/// The library: collection, playlists and history (`docs/behavior/browser.md`).
pub const LIBRARY_FILE: &str = "library.sqlite";
/// The folder beside the library where its backups go.
pub const BACKUP_FOLDER: &str = "Backup";
/// Roots of the file selector a settings file may name.
const MAX_ROOTS: usize = 32;
/// MIDI controllers a settings file may name.
pub const MAX_CONTROLLERS: usize = 8;
/// Most music folders the settings keep.
pub const MAX_MUSIC_FOLDERS: usize = 64;
/// The ranges of the tempo faders, in percent, as `deck.tempo.range.set` takes them.
pub const TEMPO_RANGE_PERCENTS: [u32; 3] = [8, 16, 50];

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

#[derive(Clone, Debug, PartialEq)]
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
    /// The window goes to the whole screen as Platter starts.
    pub fullscreen_at_start: bool,
    /// The range every tempo fader gets, in percent: 8, 16 or 50.
    pub tempo_range: u32,
    /// The size of decks A and B.
    pub deck_size: DeckSize,
    /// Two decks or four.
    pub deck_layout: DeckLayout,
    /// The settings of the Mixer page: `docs/behavior/settings.md`.
    pub mixer_mode: MixerMode,
    /// The sync mode of the Decks page.
    pub sync_mode: SyncMode,
    /// How the browser writes keys, from the Decks page.
    pub key_notation: KeyNotation,
    /// The tempos the analysis looks in, from the Decks page.
    pub bpm_range: TempoRange,
    /// The rules of loading of the Decks page: a track with a load cue starts at it, and a
    /// playing deck takes no track.
    pub load_marker: bool,
    pub load_stopped_only: bool,
    /// The music folders of the Library page, whose music is imported at start while
    /// `import_at_start` says so; the collection is checked at start while `check_at_start`
    /// says so.
    pub music_folders: Vec<PathBuf>,
    pub import_at_start: bool,
    /// The columns the list of the browser shows besides the title.
    pub columns: Vec<ListColumn>,
    pub check_at_start: bool,
    /// What Platter writes into the files, from the Library page.
    pub tag_writing: TagWriting,
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
    /// Seconds a deck plays a track before it counts as played
    /// (`docs/behavior/browser.md`, *Played state*).
    pub min_playtime: u32,
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
            fullscreen_at_start: false,
            tempo_range: 8,
            deck_size: DeckSize::Full,
            deck_layout: DeckLayout::Two,
            mixer_mode: MixerMode::Internal,
            sync_mode: SyncMode::Beat,
            key_notation: KeyNotation::default(),
            bpm_range: TempoRange::default(),
            load_marker: true,
            load_stopped_only: false,
            music_folders: Vec::new(),
            import_at_start: true,
            columns: ListColumn::SHOWN_AT_FIRST.to_vec(),
            check_at_start: false,
            tag_writing: TagWriting::default(),
            autogain: true,
            external_eq_gain: false,
            external_master_volume: false,
            fade_beats: 8,
            folder: None,
            roots: Vec::new(),
            min_playtime: DEFAULT_PLAYTIME,
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
            fullscreen_at_start: flag(Some(&root), "fullscreen_at_start")
                .unwrap_or(defaults.fullscreen_at_start),
            tempo_range: number(table("decks"), "tempo_range")
                .filter(|percent| TEMPO_RANGE_PERCENTS.contains(percent))
                .unwrap_or(defaults.tempo_range),
            deck_size: text(table("decks"), "size")
                .and_then(|name| DeckSize::ALL.into_iter().find(|size| size.name() == name))
                .unwrap_or(defaults.deck_size),
            deck_layout: text(table("decks"), "layout")
                .and_then(|name| {
                    DeckLayout::ALL
                        .into_iter()
                        .find(|layout| layout.name() == name)
                })
                .unwrap_or(defaults.deck_layout),
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
            sync_mode: text(table("decks"), "sync_mode")
                .and_then(|name| SyncMode::ALL.into_iter().find(|mode| mode.name() == name))
                .unwrap_or(defaults.sync_mode),
            key_notation: text(table("decks"), "key_notation")
                .and_then(|name| {
                    KeyNotation::ALL
                        .into_iter()
                        .find(|notation| notation.name() == name)
                })
                .unwrap_or(defaults.key_notation),
            bpm_range: number(table("decks"), "bpm_min")
                .zip(number(table("decks"), "bpm_max"))
                .and_then(|(min, max)| TempoRange::new(f64::from(min), f64::from(max)).ok())
                .unwrap_or(defaults.bpm_range),
            load_marker: flag(table("decks"), "load_marker").unwrap_or(defaults.load_marker),
            load_stopped_only: flag(table("decks"), "stopped_only")
                .unwrap_or(defaults.load_stopped_only),
            columns: table("browser")
                .and_then(|table| table.get("columns"))
                .and_then(toml::Value::as_array)
                .map_or_else(
                    || defaults.columns.clone(),
                    |names| {
                        ListColumn::ALL
                            .into_iter()
                            .filter(|column| {
                                names
                                    .iter()
                                    .any(|name| name.as_str() == Some(column.name()))
                            })
                            .collect()
                    },
                ),
            music_folders: table("browser")
                .and_then(|table| table.get("music_folders"))
                .and_then(toml::Value::as_array)
                .map(|folders| {
                    folders
                        .iter()
                        .filter_map(toml::Value::as_str)
                        .filter(|folder| !folder.is_empty())
                        .map(PathBuf::from)
                        .take(MAX_MUSIC_FOLDERS)
                        .collect()
                })
                .unwrap_or_default(),
            import_at_start: flag(table("browser"), "import_at_start")
                .unwrap_or(defaults.import_at_start),
            check_at_start: flag(table("browser"), "check_at_start")
                .unwrap_or(defaults.check_at_start),
            tag_writing: text(table("browser"), "tag_writing")
                .and_then(|name| TagWriting::ALL.into_iter().find(|mode| mode.name() == name))
                .unwrap_or(defaults.tag_writing),
            autogain: flag(table("mixer"), "autogain").unwrap_or(defaults.autogain),
            external_eq_gain: flag(table("mixer"), "external_eq_gain")
                .unwrap_or(defaults.external_eq_gain),
            external_master_volume: flag(table("mixer"), "external_master_volume")
                .unwrap_or(defaults.external_master_volume),
            fade_beats: number(table("mixer"), "fade_beats")
                .filter(|beats| (MIN_FADE_BEATS..=MAX_FADE_BEATS).contains(&f64::from(*beats)))
                .unwrap_or(defaults.fade_beats),
            min_playtime: number(table("browser"), "min_playtime")
                .filter(|seconds| (MIN_PLAYTIME..=MAX_PLAYTIME).contains(seconds))
                .unwrap_or(defaults.min_playtime),
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
        let mut decks = toml::Table::new();
        decks.insert(
            "sync_mode".to_owned(),
            toml::Value::String(self.sync_mode.name().to_owned()),
        );
        decks.insert(
            "key_notation".to_owned(),
            toml::Value::String(self.key_notation.name().to_owned()),
        );
        decks.insert("tempo_range".to_owned(), integer(self.tempo_range));
        decks.insert(
            "size".to_owned(),
            toml::Value::String(self.deck_size.name().to_owned()),
        );
        decks.insert(
            "layout".to_owned(),
            toml::Value::String(self.deck_layout.name().to_owned()),
        );
        // The ranges offered are whole tempos.
        decks.insert(
            "bpm_min".to_owned(),
            toml::Value::Integer(self.bpm_range.min().round() as i64),
        );
        decks.insert(
            "bpm_max".to_owned(),
            toml::Value::Integer(self.bpm_range.max().round() as i64),
        );
        decks.insert(
            "load_marker".to_owned(),
            toml::Value::Boolean(self.load_marker),
        );
        decks.insert(
            "stopped_only".to_owned(),
            toml::Value::Boolean(self.load_stopped_only),
        );
        let mut root = toml::Table::new();
        root.insert(
            "language".to_owned(),
            toml::Value::String(self.language.code().to_owned()),
        );
        root.insert("tooltips".to_owned(), toml::Value::Boolean(self.tooltips));
        root.insert(
            "fullscreen_at_start".to_owned(),
            toml::Value::Boolean(self.fullscreen_at_start),
        );
        root.insert("audio".to_owned(), toml::Value::Table(audio));
        root.insert("mixer".to_owned(), toml::Value::Table(mixer));
        root.insert("decks".to_owned(), toml::Value::Table(decks));
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
        browser.insert("min_playtime".to_owned(), integer(self.min_playtime));
        browser.insert(
            "columns".to_owned(),
            toml::Value::Array(
                self.columns
                    .iter()
                    .map(|column| toml::Value::String(column.name().to_owned()))
                    .collect(),
            ),
        );
        browser.insert(
            "music_folders".to_owned(),
            toml::Value::Array(
                self.music_folders
                    .iter()
                    .filter_map(|folder| folder.to_str())
                    .map(|folder| toml::Value::String(folder.to_owned()))
                    .collect(),
            ),
        );
        browser.insert(
            "import_at_start".to_owned(),
            toml::Value::Boolean(self.import_at_start),
        );
        browser.insert(
            "check_at_start".to_owned(),
            toml::Value::Boolean(self.check_at_start),
        );
        browser.insert(
            "tag_writing".to_owned(),
            toml::Value::String(self.tag_writing.name().to_owned()),
        );
        root.insert("browser".to_owned(), toml::Value::Table(browser));
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
    use platter_engine::{CrossfaderCurve, MixerMode, SyncMode, TempoRange};
    use platter_ui_egui::collection::TagWriting;
    use platter_ui_egui::command::{DeckLayout, DeckSize, ListColumn};
    use platter_ui_egui::i18n::Language;
    use platter_ui_egui::keys::KeyNotation;

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
            fullscreen_at_start: true,
            tempo_range: 16,
            deck_size: DeckSize::Compact,
            deck_layout: DeckLayout::Four,
            mixer_mode: MixerMode::External,
            sync_mode: SyncMode::Tempo,
            key_notation: KeyNotation::Sharps,
            bpm_range: TempoRange::new(90.0, 180.0).unwrap(),
            load_marker: false,
            load_stopped_only: true,
            music_folders: vec![PathBuf::from("Music"), PathBuf::from("Sets with ñ")],
            import_at_start: false,
            columns: vec![ListColumn::Genre, ListColumn::Comment],
            check_at_start: true,
            tag_writing: TagWriting::PlatterOnly,
            autogain: false,
            external_eq_gain: true,
            external_master_volume: true,
            fade_beats: 32,
            min_playtime: 45,
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
    fn the_minimum_play_time_stays_between_five_seconds_and_ten_minutes() {
        for (written, read) in [
            ("4", 30),
            ("5", 5),
            ("600", 600),
            ("601", 30),
            ("\"60\"", 30),
        ] {
            let source = format!(
                "[browser]
min_playtime = {written}
"
            );
            assert_eq!(Settings::parse(&source).min_playtime, read, "{written}");
        }
    }

    #[test]
    fn the_sync_mode_is_kept_under_decks_and_an_unknown_one_is_the_default() {
        let tempo = Settings {
            sync_mode: SyncMode::Tempo,
            ..Settings::default()
        };
        let written = tempo.to_toml();
        assert!(written.contains("[decks]"), "{written}");
        assert_eq!(Settings::parse(&written).sync_mode, SyncMode::Tempo);
        let unknown = Settings::parse("[decks]\nsync_mode = \"phase\"\n");
        assert_eq!(unknown.sync_mode, SyncMode::Beat);
        let open_key = Settings {
            key_notation: KeyNotation::OpenKey,
            ..Settings::default()
        };
        assert_eq!(
            Settings::parse(&open_key.to_toml()).key_notation,
            KeyNotation::OpenKey
        );
        let unknown = Settings::parse("[decks]\nkey_notation = \"camelot\"\n");
        assert_eq!(unknown.key_notation, KeyNotation::Flats);
        // A range the analysis cannot use, or half a range, is the default.
        for written in [
            "[decks]\nbpm_min = 100\nbpm_max = 120\n",
            "[decks]\nbpm_min = 10\nbpm_max = 180\n",
            "[decks]\nbpm_min = 80\n",
        ] {
            assert_eq!(Settings::parse(written).bpm_range, TempoRange::default());
        }
        // Unknown names are left out; an empty list hides every column but the title.
        let columns = Settings::parse("[browser]\ncolumns = [\"rating\", \"stars\", \"artist\"]\n");
        assert_eq!(columns.columns, [ListColumn::Artist, ListColumn::Rating]);
        assert!(
            Settings::parse("[browser]\ncolumns = []\n")
                .columns
                .is_empty()
        );
        assert_eq!(Settings::default().columns, ListColumn::SHOWN_AT_FIRST);
        let unknown = Settings::parse("[decks]\nlayout = \"six\"\n");
        assert_eq!(unknown.deck_layout, DeckLayout::Two);
        let unknown = Settings::parse("[decks]\ntempo_range = 10\n");
        assert_eq!(unknown.tempo_range, 8);
        let written = Settings::parse("[decks]\nbpm_min = 85\nbpm_max = 175\n");
        assert_eq!(written.bpm_range, TempoRange::new(85.0, 175.0).unwrap());
        let nothing = Settings {
            tag_writing: TagWriting::Nothing,
            ..Settings::default()
        };
        assert_eq!(
            Settings::parse(&nothing.to_toml()).tag_writing,
            TagWriting::Nothing
        );
        assert_eq!(Settings::default().tag_writing, TagWriting::Everything);
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
