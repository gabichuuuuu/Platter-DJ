//! The application: it owns the engine, the audio output, the settings and the file
//! selector, builds the view of each frame and carries out the commands the interface
//! answers with.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::ViewportCommand;
use platter_audio_io::{
    AudioError, BUFFER_SIZES, DeviceInfo, DeviceKind, Output, SAMPLE_RATES, Source, StreamRequest,
};
use platter_core::log::{Level, Logger};
use platter_core::{DeckId, SampleRate};
use platter_engine::command::{Args, CommandSpec, SPECS};
use platter_engine::{
    Command, Engine, EngineError, Event, FxUnit, PlatterAnalyser, Processor, Recall,
    RecordingNames, StemRecall, TempoRange,
};
use platter_library::{Library, LibraryError, SqliteLibrary, Timestamp, back_up};
use platter_mcp::json::Value as McpValue;
use platter_mcp::{Host, Server};
use platter_ui_egui::collection::Collection;
use platter_ui_egui::command::{Action, AppCommand, EffectUnits, fx_panel_of};
use platter_ui_egui::files::{FileBrowser, Root};
use platter_ui_egui::i18n::Language;
use platter_ui_egui::keymap::{DEFAULT_KEYMAP, Keymap};
use platter_ui_egui::layout::{Layouts, Row as LayoutRow, Zone};
use platter_ui_egui::model::{
    AnalysisStatus, AudioProblem, AudioState, AudioView, ControllerManager, ControllerRow,
    DeckNotice, DeviceView, MidiView, OutputKind, SettingsView, Shown, StreamView, View,
};
use platter_ui_egui::played::{MAX_PLAYTIME, MIN_PLAYTIME, PlayClock};
use platter_ui_egui::shell::{DESIGN_WIDTH, Shell};

use crate::background;
use crate::config::{
    self, BACKUP_FOLDER, Controller, KEYMAP_FILE, LIBRARY_FILE, LOG_FILE, MAX_CONTROLLERS,
    MAX_MUSIC_FOLDERS, SETTINGS_FILE, Settings, TEMPO_RANGE_PERCENTS,
};
use crate::keeping::{Keeping, SETTLE};
use crate::manager::Document;
use crate::midi;
use crate::starting::{Next, Starting};

/// How often the devices are looked for again while none is playing.
const DEVICE_RETRY: Duration = Duration::from_secs(3);
/// How often an idle window is drawn, for the readings that change on their own.
const IDLE_REPAINT: Duration = Duration::from_millis(100);
/// How long the MIDI indicator lights after a message (`docs/behavior/interface.md`).
const MIDI_LIGHT: Duration = Duration::from_millis(150);
/// Most mapping files the Controllers page offers to import from one folder.
const MAX_IMPORTABLE: usize = 64;

/// What the controller manager of the preferences keeps between frames.
struct Manager {
    selected: Option<usize>,
    binding: Option<usize>,
    learning: bool,
    target: DeckId,
    /// The mapping of each controller of the settings, as the page edits it.
    documents: Vec<Document>,
    /// Why each controller of the settings is not open, if it is not.
    problems: Vec<Option<String>>,
    /// Learn gave an assignment its control: the mapping is written after the messages.
    pending_write: bool,
}

impl Default for Manager {
    fn default() -> Self {
        Self {
            selected: None,
            binding: None,
            learning: false,
            target: DeckId::A,
            documents: Vec::new(),
            problems: Vec::new(),
            pending_write: false,
        }
    }
}

/// Longest wait at closing for the collection to write what the decks learned last.
const EXIT_WAIT: Duration = Duration::from_secs(2);
/// A level below this is silence for the purpose of deciding how often to draw.
const QUIET_LEVEL: f32 = 1e-4;

fn source_of(mut processor: Processor) -> Source {
    Box::new(move |output, channels| processor.process(output, channels))
}

fn problem_of(error: &AudioError) -> AudioProblem {
    match error {
        AudioError::NoDevice => AudioProblem::NoDevice,
        AudioError::DeviceNotFound(_) => AudioProblem::DeviceMissing,
        AudioError::Unsupported(_) => AudioProblem::Unsupported,
        AudioError::Busy => AudioProblem::Busy,
        AudioError::Backend(_) => AudioProblem::Failed,
    }
}

fn rate_of(hz: u32) -> SampleRate {
    SampleRate::new(hz).unwrap_or(SampleRate::HZ_48000)
}

/// The roots of the file selector: the home folder, the folders the settings file names
/// and, where the system has one tree for everything, its root.
///
/// The drives of a Windows machine are not looked for. Finding them without a system call
/// the project cannot make means asking for every letter in turn, and a letter that stands
/// for a network share blocks for a long time when the share does not answer. A drive is
/// reached by naming it in the settings file or by dropping a file from it on a deck.
/// Reads what the collection in `file` kept for a track; it runs on the worker that loads
/// the track. A library that cannot be read keeps nothing.
fn recall_from(file: PathBuf) -> Recall {
    Arc::new(move |path: &Path| {
        SqliteLibrary::open(&file)
            .ok()?
            .memory_at(path)
            .ok()
            .flatten()
    })
}

/// Reads the stems the collection in `file` links to a track; it runs on the worker that loads
/// the track. A library that cannot be read links none: `docs/behavior/stems.md`.
fn stem_recall_from(file: PathBuf) -> StemRecall {
    Arc::new(move |path: &Path| {
        let library = SqliteLibrary::open(&file).ok()?;
        let track = library.track_at(path).ok()??;
        library.stems(track.id).ok().flatten()
    })
}

/// Names each recording in `folder` by the moment it starts, in UTC, as the backups of the
/// collection are: `mix-20261118-214105` (`docs/behavior/recording.md`, *Files*).
/// Bytes in a MiB, the unit of the split size of the Recording page.
const MIB: u64 = 1024 * 1024;

/// The names of the recordings: `prefix`, a dash and the moment the recording starts, in UTC
/// (`docs/behavior/recording.md`, *Files*).
fn recording_names(folder: PathBuf, prefix: String) -> RecordingNames {
    Arc::new(move || {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
            });
        let (year, month, day, hour, minute, second) = Timestamp(now).utc();
        folder.join(format!(
            "{prefix}-{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}"
        ))
    })
}

fn roots(home_name: &str, extra: &[PathBuf]) -> Vec<Root> {
    let mut roots = Vec::new();
    if let Some(home) = config::home_dir() {
        roots.push(Root {
            name: home_name.to_owned(),
            path: home,
        });
    }
    for path in extra {
        roots.push(Root {
            name: path.to_string_lossy().into_owned(),
            path: path.clone(),
        });
    }
    if !cfg!(windows) {
        roots.push(Root {
            name: "/".to_owned(),
            path: PathBuf::from("/"),
        });
    }
    roots
}

pub struct PlatterApp {
    /// The MCP server, when the application was started with `--mcp`.
    mcp: Option<(Receiver<crate::mcp::Request>, Server)>,
    /// Installing the separation model and separating tracks into stems.
    separation: crate::separation::Jobs,
    /// The folder of the files of the user; without one nothing is saved.
    folder: Option<PathBuf>,
    /// Where the mix recorder writes when the user has chosen no folder.
    default_recordings: Option<PathBuf>,
    logger: Option<Logger>,
    settings: Settings,
    keymap: Keymap,
    shell: Shell,
    engine: Engine,
    /// Sample rate the engine was built for.
    engine_rate: u32,
    output: Option<Output>,
    problem: Option<AudioProblem>,
    devices: Vec<DeviceInfo>,
    last_device_scan: Instant,
    browser: FileBrowser,
    notices: [Option<DeckNotice>; DeckId::COUNT],
    /// The window covers the whole screen, as the window last said.
    fullscreen: bool,
    /// The unit each panel of the effect row shows, with four units shown.
    fx_panels: [FxUnit; 2],
    /// Full screen asked for by a command, sent to the window in the next frame.
    fullscreen_wanted: Option<bool>,
    /// The MIDI controllers of the settings file that could be opened.
    /// The controllers whose input is open, with their place in the settings.
    controllers: Vec<(usize, midi::Controller)>,
    /// The controller manager of the preferences.
    manager: Manager,
    /// The MIDI inputs of the system, as the last look found them.
    midi_inputs: Vec<String>,
    /// The mapping files in the folder the explorer shows, which the page offers to import.
    importable: Vec<PathBuf>,
    /// The context of the window, which the MIDI inputs wake.
    context: eframe::egui::Context,
    /// When the last MIDI message came, from any controller.
    last_midi: Option<Instant>,
    /// When each deck has played its track long enough for it to count as played.
    play_clock: PlayClock,
    last_play_check: Instant,
    /// Why the backup of the library made when it opened failed, if it did.
    backup_failure: std::sync::mpsc::Receiver<String>,
    /// The track each deck had at the last frame, to tell the browser which one loaded last.
    deck_tracks: [Option<PathBuf>; DeckId::COUNT],
    /// What the decks learned of their tracks, on its way to the collection.
    keeping: Keeping,
    /// The music folder the Library page shows.
    music_folder: Option<usize>,
    /// What the library still does at start; `None` once it is done.
    starting: Option<Starting>,
    /// What the window shares with the analysis of the collection in the background.
    background: background::Shared,
    background_reports: std::sync::mpsc::Receiver<background::Report>,
}

impl PlatterApp {
    /// `context` is the context of the window, woken by the MIDI inputs.
    pub fn new(
        shell: Shell,
        settings: Settings,
        folder: Option<PathBuf>,
        context: eframe::egui::Context,
    ) -> Self {
        let logger = folder.as_ref().and_then(|folder| {
            std::fs::create_dir_all(folder).ok()?;
            Logger::to_file(&folder.join(LOG_FILE)).ok()
        });
        let home_name = shell.texts().get("browser.home").to_owned();
        let mut browser = FileBrowser::new(roots(&home_name, &settings.roots));
        browser.set_columns(&settings.columns);
        let library = folder.as_ref().map(|folder| folder.join(LIBRARY_FILE));
        let recall = library.clone().map(recall_from);
        let stem_recall = library.clone().map(stem_recall_from);
        let background = background::Shared::default();
        background.range.set(settings.bpm_range);
        let (background_sender, background_reports) = std::sync::mpsc::channel();
        if let Some(file) = library.clone() {
            // Without a worker the collection is simply not analysed in the background.
            let _ = background::start(file, background.clone(), background_sender);
        }
        let (backup_sender, backup_failure) = std::sync::mpsc::channel();
        browser.attach(Collection::start(move || {
            let path = library
                .ok_or_else(|| LibraryError::NotAFolder(std::io::ErrorKind::NotFound.into()))?;
            // The file as the last run left it, before this run writes to it.
            if let Some(backups) = path.parent().map(|folder| folder.join(BACKUP_FOLDER)) {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |since| {
                        i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
                    });
                if let Err(error) = back_up(&path, &backups, Timestamp(now)) {
                    // The window may be gone already; then nobody is left to tell.
                    let _ = backup_sender.send(error.to_string());
                }
            }
            Ok(Box::new(SqliteLibrary::open(&path)?) as Box<dyn Library>)
        }));
        // The folder shown last is not looked at here: if it is gone, or on a drive that
        // does not answer, the worker that reads it finds out without holding the window.
        let start = settings.folder.clone().or_else(|| {
            let home = config::home_dir()?;
            let music = home.join("Music");
            Some(if music.is_dir() { music } else { home })
        });
        if let Some(start) = start {
            browser.navigate(start);
        }
        let engine_rate = settings.sample_rate;
        let (mut engine, processor) = Engine::with_analyser(
            rate_of(engine_rate),
            Arc::new(PlatterAnalyser::with_range(background.range.clone())),
        );
        if let Some(recall) = recall {
            engine.set_recall(recall);
        }
        if let Some(recall) = stem_recall {
            engine.set_stem_recall(recall);
        }
        let home = config::home_dir();
        let default_recordings = config::recordings_folder(home.as_deref(), folder.as_deref());
        let recordings = settings
            .recording_folder
            .clone()
            .or_else(|| default_recordings.clone());
        browser.set_recordings(recordings.clone());
        if let Some(recordings) = recordings {
            engine.set_recording_names(recording_names(
                recordings,
                settings.recording_prefix.clone(),
            ));
        }
        engine.set_recording_split(u64::from(settings.recording_split) * MIB);
        // A fresh queue has room for it, and the settings keep the decay in range.
        let _ = engine.set_loop_decay(f64::from(settings.loop_decay) / 100.0);
        // Without its workers, key lock runs on the audio thread as it always could.
        let _ = engine.set_key_lock_threads(settings.key_lock_threads);
        let settings_min_playtime = settings.min_playtime;
        let separation = crate::separation::Jobs::new(folder.clone());
        let mut app = Self {
            folder,
            default_recordings,
            logger,
            settings,
            keymap: Keymap::defaults(),
            shell,
            engine,
            engine_rate,
            output: None,
            problem: None,
            devices: Vec::new(),
            last_device_scan: Instant::now(),
            mcp: None,
            separation,
            browser,
            notices: [None; DeckId::COUNT],
            fullscreen: false,
            fx_panels: [FxUnit::ALL[0], FxUnit::ALL[1]],
            fullscreen_wanted: None,
            controllers: Vec::new(),
            manager: Manager::default(),
            midi_inputs: Vec::new(),
            importable: Vec::new(),
            context: context.clone(),
            last_midi: None,
            play_clock: PlayClock::new(settings_min_playtime),
            last_play_check: Instant::now(),
            deck_tracks: Default::default(),
            keeping: Keeping::default(),
            background,
            background_reports,
            music_folder: None,
            starting: None,
            backup_failure,
        };
        app.log(
            Level::Info,
            &format!("Platter DJ {} started", env!("CARGO_PKG_VERSION")),
        );
        app.load_keymap();
        app.apply_tag_writing();
        app.log_key_lock_priority();
        app.fullscreen_wanted = app.settings.fullscreen_at_start.then_some(true);
        app.starting = Some(Starting::new(
            &app.settings.music_folders,
            app.settings.import_at_start,
            app.settings.check_at_start,
        ));
        app.open_audio(Some(source_of(processor)));
        app.open_controllers();
        let curve = app.settings.crossfader_curve;
        app.dispatch(Command::MixerCrossfaderCurveSet { curve }.into());
        app.apply_mixer_settings();
        let effects = app.settings.effects.clone();
        if let Err(error) = app.engine.restore_effects(&effects) {
            app.log(
                Level::Warning,
                &format!("the effects of the settings file: {error}"),
            );
        }
        app
    }

    fn log(&self, level: Level, message: &str) {
        if let Some(logger) = &self.logger {
            logger.line(level, message);
        }
    }

    /// What the mix recorder reported: `docs/behavior/recording.md`.
    fn log_recording(&self, event: &Event) {
        match event {
            Event::Recorded { path, frames } => self.log(
                Level::Info,
                &format!("recording: {frames} frames in {}", path.display()),
            ),
            Event::RecordingFailed { message } => {
                self.log(Level::Warning, &format!("recording: {message}"));
            }
            _ => {}
        }
    }

    /// Applies `change` to the layouts and, when it changed them, gives the window the
    /// arrangement of the active one and keeps it: `docs/behavior/interface.md`, *Layouts*.
    fn change_layout(&mut self, change: impl FnOnce(&mut Layouts) -> bool) {
        if change(&mut self.settings.layouts) {
            let arrangement = self.settings.layouts.arrangement();
            self.settings.deck_layout = arrangement.decks;
            self.settings.deck_size = arrangement.size;
            self.save_settings();
        }
    }

    /// The name the active layout shows, in the language of the interface.
    fn layout_name(&self) -> String {
        let layouts = &self.settings.layouts;
        match layouts.rows().get(layouts.active) {
            Some(LayoutRow::BuiltIn(built_in)) => {
                self.shell.texts().get(built_in.text()).to_owned()
            }
            Some(LayoutRow::Personal(name)) => name.clone(),
            None => String::new(),
        }
    }

    /// The folder in use for the recordings: the one chosen, or the default.
    fn recordings(&self) -> Option<PathBuf> {
        self.settings
            .recording_folder
            .clone()
            .or_else(|| self.default_recordings.clone())
    }

    /// Gives the preferences of the Recording page to the engine and to the tree, for the next
    /// recording, and keeps them.
    /// Runs key lock where the settings say, for the tracks loaded from then on.
    fn apply_key_lock_threads(&mut self) {
        let on = self.settings.key_lock_threads;
        if let Err(error) = self.engine.set_key_lock_threads(on) {
            self.log(
                Level::Warning,
                &format!("key lock stays on the audio thread: {error}"),
            );
        }
        self.log_key_lock_priority();
    }

    /// Says in the log when a worker of key lock could not get the priority of audio: it may
    /// then fall behind under load.
    fn log_key_lock_priority(&mut self) {
        if let Some((raised, workers)) = self.engine.key_lock_threads()
            && raised < workers
        {
            self.log(
                Level::Warning,
                &format!(
                    "{} of {workers} key lock workers run without the priority of audio",
                    workers - raised
                ),
            );
        }
    }

    fn apply_loop_decay(&mut self) {
        let decay = f64::from(self.settings.loop_decay) / 100.0;
        if let Err(error) = self.engine.set_loop_decay(decay) {
            self.log(
                Level::Warning,
                &format!("the overdub decay was not applied: {error}"),
            );
        }
    }

    fn apply_recorder(&mut self) {
        let recordings = self.recordings();
        self.browser.set_recordings(recordings.clone());
        if let Some(recordings) = recordings {
            self.engine.set_recording_names(recording_names(
                recordings,
                self.settings.recording_prefix.clone(),
            ));
        }
        self.engine
            .set_recording_split(u64::from(self.settings.recording_split) * MIB);
        self.save_settings();
    }

    fn save_settings(&self) {
        let Some(folder) = &self.folder else {
            return;
        };
        if let Err(error) = self.settings.save(&folder.join(SETTINGS_FILE)) {
            self.log(Level::Error, &format!("cannot save the settings: {error}"));
        }
    }

    /// Reads the shortcut file, writing the default one the first time. A file with a
    /// mistake is reported in the log and the default shortcuts are used.
    fn load_keymap(&mut self) {
        let Some(path) = self.folder.as_ref().map(|folder| folder.join(KEYMAP_FILE)) else {
            return;
        };
        if !path.exists()
            && let Err(error) = std::fs::write(&path, DEFAULT_KEYMAP)
        {
            self.log(
                Level::Warning,
                &format!("cannot write the shortcut file: {error}"),
            );
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            return;
        };
        match Keymap::parse(&source) {
            Ok(keymap) => self.keymap = keymap,
            Err(error) => self.log(
                Level::Warning,
                &format!("{error}; using the default shortcuts"),
            ),
        }
    }

    /// Reads the mapping of each MIDI controller of the settings and opens its input. A
    /// controller whose mapping has a mistake, or whose input cannot be opened, is written in
    /// the log and left off.
    fn open_controllers(&mut self) {
        // The old inputs close before the same ones open again.
        self.controllers.clear();
        let entries = self.settings.midi.clone();
        self.manager.documents.clear();
        self.manager.problems.clear();
        for (index, entry) in entries.iter().enumerate() {
            let path = self.mapping_path(&entry.mapping);
            self.manager
                .documents
                .push(Document::read(&path).unwrap_or_default());
            let mapping = match midi::read(&path) {
                Ok(mapping) => mapping,
                Err(problem) => {
                    self.log(
                        Level::Warning,
                        &format!("MIDI mapping {}: {problem}", entry.mapping.display()),
                    );
                    self.manager.problems.push(Some(problem.to_string()));
                    continue;
                }
            };
            let context = self.context.clone();
            match midi::Controller::open(&entry.input, mapping, move || context.request_repaint()) {
                Ok(controller) => {
                    self.log(
                        Level::Info,
                        &format!("MIDI input `{}` opened", controller.name()),
                    );
                    self.controllers.push((index, controller));
                    self.manager.problems.push(None);
                }
                Err(error) => {
                    self.log(Level::Warning, &format!("MIDI: {error}"));
                    self.manager.problems.push(Some(error.to_string()));
                }
            }
        }
        self.manager.selected = self
            .manager
            .selected
            .filter(|index| *index < entries.len())
            .or((!entries.is_empty()).then_some(0));
    }

    /// What the library does when Platter starts, once it is open: the import of each music
    /// folder in turn, then the check of the collection with the maintenance open.
    fn start_library(&mut self) {
        let Some(starting) = self.starting.as_mut() else {
            return;
        };
        match starting.next(
            self.browser.library_available(),
            self.browser.collection_busy(),
        ) {
            Next::Wait => {}
            Next::Done => self.starting = None,
            Next::Send(commands) => {
                for command in commands {
                    self.dispatch_app(command);
                }
            }
        }
    }

    /// Looks for the MIDI inputs of the system and the mapping files of the folder the
    /// explorer shows, for the Controllers page.
    fn scan_midi(&mut self) {
        self.midi_inputs = platter_device_io::inputs().unwrap_or_default();
        self.importable = self
            .browser
            .current()
            .and_then(|folder| std::fs::read_dir(folder).ok())
            .map(|entries| {
                let mut files: Vec<PathBuf> = entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| {
                        path.extension()
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("toml"))
                    })
                    .take(MAX_IMPORTABLE)
                    .collect();
                files.sort();
                files
            })
            .unwrap_or_default();
    }

    /// Where a mapping file named in the settings is: without a folder, in that of the
    /// settings.
    fn mapping_path(&self, mapping: &Path) -> PathBuf {
        match &self.folder {
            Some(folder) if mapping.is_relative() => folder.join(mapping),
            _ => mapping.to_path_buf(),
        }
    }

    /// Writes the mapping of the selected controller as the page left it, and opens the
    /// controllers again with it; a mapping with a mistake is not written.
    fn write_mapping(&mut self) {
        let Some(index) = self.manager.selected else {
            return;
        };
        let (Some(entry), Some(document)) = (
            self.settings.midi.get(index).cloned(),
            self.manager.documents.get(index).cloned(),
        ) else {
            return;
        };
        let path = self.mapping_path(&entry.mapping);
        if let Err(problem) = document.write(&path) {
            self.log(
                Level::Warning,
                &format!("MIDI mapping {}: {problem}", entry.mapping.display()),
            );
            if let Some(slot) = self.manager.problems.get_mut(index) {
                *slot = Some(problem);
            }
            return;
        }
        let (selected, binding) = (self.manager.selected, self.manager.binding);
        self.open_controllers();
        // What the page was editing stays, unwritten assignments included.
        if let Some(slot) = self.manager.documents.get_mut(index) {
            *slot = document;
        }
        self.manager.selected = selected;
        self.manager.binding = binding;
    }

    /// Carries out a command of the controller manager.
    fn manage(&mut self, command: AppCommand) {
        let selected = self.manager.selected;
        let document = selected.and_then(|index| self.manager.documents.get_mut(index));
        match command {
            AppCommand::MidiControllerAdd => {
                let used: Vec<&String> = self
                    .settings
                    .midi
                    .iter()
                    .map(|entry| &entry.input)
                    .collect();
                let Some(input) = self
                    .midi_inputs
                    .iter()
                    .find(|input| !used.contains(input))
                    .or_else(|| self.midi_inputs.first())
                    .cloned()
                else {
                    return;
                };
                if self.settings.midi.len() >= MAX_CONTROLLERS {
                    return;
                }
                let mapping = (1..)
                    .map(|number| PathBuf::from(format!("controller-{number}.toml")))
                    .find(|name| {
                        !self.mapping_path(name).exists()
                            && self
                                .settings
                                .midi
                                .iter()
                                .all(|entry| entry.mapping != *name)
                    })
                    .unwrap_or_else(|| PathBuf::from("controller.toml"));
                if let Err(error) = std::fs::write(self.mapping_path(&mapping), "") {
                    self.log(Level::Warning, &format!("MIDI mapping: {error}"));
                    return;
                }
                self.settings.midi.push(Controller { input, mapping });
                self.save_settings();
                self.manager.selected = Some(self.settings.midi.len() - 1);
                self.manager.binding = None;
                self.open_controllers();
            }
            AppCommand::MidiControllerRemove { index } => {
                if index < self.settings.midi.len() {
                    self.settings.midi.remove(index);
                    self.save_settings();
                    self.manager.selected = None;
                    self.manager.binding = None;
                    self.manager.learning = false;
                    self.open_controllers();
                }
            }
            AppCommand::MidiControllerSelect { index } => {
                if index < self.settings.midi.len() {
                    self.manager.selected = Some(index);
                    self.manager.binding = None;
                    self.manager.learning = false;
                }
            }
            AppCommand::MidiInputSet { index, input } => {
                if let Some(entry) = self.settings.midi.get_mut(index) {
                    entry.input = input;
                    self.save_settings();
                    self.open_controllers();
                }
            }
            AppCommand::MidiMappingImport { index, path } => self.import_mapping(index, &path),
            AppCommand::MidiMappingExport { index, folder } => self.export_mapping(index, &folder),
            AppCommand::MidiBindingSelect { index } => {
                if document.is_some_and(|document| index < document.len()) {
                    self.manager.binding = Some(index);
                }
            }
            AppCommand::MidiBindingAdd { command } => {
                let spec = SPECS.iter().find(|spec| spec.id == command);
                if let (Some(document), Some(spec)) = (document, spec)
                    && let Some(added) = document.add(spec, self.manager.target)
                {
                    // It waits for its control: Learn gives it one.
                    self.manager.binding = Some(added);
                    self.manager.learning = true;
                }
            }
            AppCommand::MidiBindingDuplicate => {
                if let (Some(document), Some(binding)) = (document, self.manager.binding)
                    && let Some(copy) = document.duplicate(binding)
                {
                    self.manager.binding = Some(copy);
                }
            }
            AppCommand::MidiBindingDelete => {
                if let (Some(document), Some(binding)) = (document, self.manager.binding)
                    && document.delete(binding)
                {
                    self.manager.binding = None;
                    self.write_mapping();
                }
            }
            AppCommand::MidiLearnToggle => self.manager.learning = !self.manager.learning,
            AppCommand::MidiTargetSet { deck } => self.manager.target = deck,
            _ => {}
        }
    }

    /// Copies the mapping file at `path` into the folder of the settings and gives it to the
    /// controller at `index`; a mapping with a mistake is refused.
    fn import_mapping(&mut self, index: usize, path: &Path) {
        if index >= self.settings.midi.len() {
            return;
        }
        let document = match Document::read(path).and_then(|document| {
            document.mapping()?;
            Ok(document)
        }) {
            Ok(document) => document,
            Err(problem) => {
                self.log(
                    Level::Warning,
                    &format!("MIDI mapping {}: {problem}", path.display()),
                );
                return;
            }
        };
        let stem = path.file_stem().map_or_else(
            || "mapping".to_owned(),
            |stem| stem.to_string_lossy().into_owned(),
        );
        // A name of its own in the folder of the settings: a file there is never replaced.
        let name = std::iter::once(format!("{stem}.toml"))
            .chain((2..).map(|number| format!("{stem}-{number}.toml")))
            .map(PathBuf::from)
            .find(|name| !self.mapping_path(name).exists())
            .unwrap_or_else(|| PathBuf::from("mapping.toml"));
        if let Err(problem) = document.write(&self.mapping_path(&name)) {
            self.log(Level::Warning, &format!("MIDI mapping: {problem}"));
            return;
        }
        if let Some(entry) = self.settings.midi.get_mut(index) {
            entry.mapping = name;
        }
        self.save_settings();
        self.open_controllers();
    }

    /// Writes the mapping of the controller at `index` into `folder`, never over a file.
    fn export_mapping(&mut self, index: usize, folder: &Path) {
        let (Some(entry), Some(document)) = (
            self.settings.midi.get(index).cloned(),
            self.manager.documents.get(index),
        ) else {
            return;
        };
        let Some(name) = entry.mapping.file_name() else {
            return;
        };
        let target = folder.join(name);
        let written = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .and_then(|mut file| {
                use std::io::Write as _;
                file.write_all(document.to_text().as_bytes())
            });
        match written {
            Ok(()) => self.log(
                Level::Info,
                &format!("MIDI mapping exported to {}", target.display()),
            ),
            Err(error) => self.log(
                Level::Warning,
                &format!("MIDI mapping {}: {error}", target.display()),
            ),
        }
    }

    /// The view of the controller manager for the Controllers page.
    fn manager_view(&self) -> ControllerManager {
        let open: Vec<usize> = self.controllers.iter().map(|(index, _)| *index).collect();
        ControllerManager {
            controllers: self
                .settings
                .midi
                .iter()
                .enumerate()
                .map(|(index, entry)| ControllerRow {
                    input: entry.input.clone(),
                    mapping: entry
                        .mapping
                        .file_name()
                        .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
                    open: open.contains(&index),
                    problem: self.manager.problems.get(index).cloned().flatten(),
                    bindings: self
                        .manager
                        .documents
                        .get(index)
                        .map(Document::rows)
                        .unwrap_or_default(),
                })
                .collect(),
            inputs: self.midi_inputs.clone(),
            selected: self.manager.selected,
            binding: self.manager.binding,
            learning: self.manager.learning,
            target: self.manager.target,
            importable: self.importable.clone(),
        }
    }

    /// Carries out the messages the MIDI controllers sent since the last frame. Returns
    /// whether some may still be waiting.
    fn take_midi(&mut self) -> bool {
        let mut controllers = std::mem::take(&mut self.controllers);
        let mut more = false;
        for (index, controller) in &mut controllers {
            if self.manager.learning && self.manager.selected == Some(*index) {
                // While Learn is on, the messages of the controller name controls instead
                // of acting.
                if let Some(midi) = controller.take_learned() {
                    self.last_midi = Some(Instant::now());
                    let binding = self.manager.binding;
                    if let (Some(document), Some(binding)) =
                        (self.manager.documents.get_mut(*index), binding)
                        && document.learn(binding, &midi)
                    {
                        self.manager.pending_write = true;
                    }
                }
                continue;
            }
            let (actions, taken, left) = controller.take();
            more |= left;
            if taken > 0 {
                self.last_midi = Some(Instant::now());
            }
            for action in actions {
                match action {
                    Action::Engine(command) => {
                        let id = command.id();
                        if let Err(error) = self.run_engine(command)
                            && controller.first_refusal(id)
                        {
                            self.log(
                                Level::Warning,
                                &format!(
                                    "MIDI `{}`: {id}: {error}; not written again",
                                    controller.name()
                                ),
                            );
                        }
                    }
                    Action::App(command) => self.dispatch_app(command),
                }
            }
            if let Some((dropped, oversized)) = controller.losses(Instant::now()) {
                self.log(
                    Level::Warning,
                    &format!(
                        "MIDI `{}`: {dropped} messages lost so far, {oversized} too long",
                        controller.name()
                    ),
                );
            }
        }
        self.controllers = controllers;
        if std::mem::take(&mut self.manager.pending_write) {
            self.write_mapping();
        }
        more
    }

    /// Builds an engine at the sample rate of the settings with the controls where they
    /// stand, and loads again the tracks the decks held. Returns its processor.
    fn rebuild_engine(&mut self) -> Processor {
        // The tracks come back from the collection: what changed last goes there first.
        for (path, kept) in self.keeping.flush() {
            self.browser.keep(path, kept);
        }
        let snapshot = self.engine.snapshot();
        let rate = self.settings.sample_rate;
        let (engine, processor) = Engine::with_controls_of(rate_of(rate), &self.engine);
        self.engine = engine;
        self.engine_rate = rate;
        for deck in DeckId::ALL {
            let held = &snapshot.decks[deck.index()];
            if let Some(track) = &held.track {
                let path = track.path.clone();
                // A track loaded without its stems comes back without them.
                let command = if held.stem_peaks.is_some() {
                    Command::DeckLoad { deck, path }
                } else {
                    Command::DeckLoadTrack { deck, path }
                };
                self.dispatch(command.into());
            }
        }
        processor
    }

    /// Opens the audio output the settings ask for. `source` is the processor of the
    /// engine when the caller still has it; without one, or when the sample rate of the
    /// settings is not the one the engine runs at, the engine is built again. When no
    /// device can be opened the engine runs on the silent output and the interface says
    /// why.
    fn open_audio(&mut self, source: Option<Source>) {
        self.devices = platter_audio_io::devices();
        self.last_device_scan = Instant::now();
        let request = StreamRequest {
            device: self.settings.device.clone(),
            sample_rate: self.settings.sample_rate,
            buffer_frames: self.settings.buffer_frames,
            routing: self.settings.routing,
        };
        let source = match source {
            Some(source) if self.engine_rate == request.sample_rate => source,
            _ => source_of(self.rebuild_engine()),
        };
        let (output, problem) = match platter_audio_io::prepare(&request) {
            Ok(prepared) => match prepared.start(source) {
                Ok(output) => (Some(output), None),
                Err(failed) => {
                    self.log(Level::Error, &format!("audio: {}", failed.error));
                    let source = match failed.source {
                        Some(source) => source,
                        None => source_of(self.rebuild_engine()),
                    };
                    (self.silent(source), Some(problem_of(&failed.error)))
                }
            },
            Err(error) => {
                self.log(Level::Warning, &format!("audio: {error}"));
                (self.silent(source), Some(problem_of(&error)))
            }
        };
        if let Some(output) = &output {
            let info = output.info();
            self.log(
                Level::Info,
                &format!(
                    "audio: {} Hz, {} channels, {} frames per buffer, playing through a device: {}",
                    info.sample_rate,
                    info.channels,
                    info.buffer_frames,
                    output.is_audible()
                ),
            );
        }
        self.output = output;
        self.problem = problem;
    }

    fn silent(&self, source: Source) -> Option<Output> {
        match platter_audio_io::silent(source, self.settings.sample_rate) {
            Ok(output) => Some(output),
            Err(error) => {
                self.log(Level::Error, &format!("audio: {error}"));
                None
            }
        }
    }

    /// Stops the output, keeping the processor when the audio system gives it back, and
    /// opens the output again with the current settings.
    fn reopen_audio(&mut self) {
        let source = self.output.take().and_then(Output::stop);
        self.open_audio(source);
    }

    /// Carries out one command. This is the only way anything in the application changes.
    pub fn dispatch(&mut self, action: Action) {
        match action {
            Action::Engine(command) => {
                let id = command.id();
                if let Err(error) = self.run_engine(command) {
                    self.log(Level::Warning, &format!("{id}: {error}"));
                }
            }
            Action::App(command) => self.dispatch_app(command),
        }
    }

    fn dispatch_engine(&mut self, command: Command) {
        let id = command.id();
        if let Err(error) = self.engine.dispatch(command) {
            self.log(Level::Warning, &format!("{id}: {error}"));
        }
    }

    /// Brings the mixer of the engine to the settings of the Mixer page.
    fn apply_mixer_settings(&mut self) {
        let mixer = self.engine.snapshot().mixer;
        let settings = &self.settings;
        let mut commands = vec![
            Command::MixerModeSet {
                mode: settings.mixer_mode,
            },
            Command::MixerCrossfaderFadeBeatsSet {
                beats: f64::from(settings.fade_beats),
            },
            Command::MixerEqModelSet {
                model: settings.eq_model,
            },
            Command::MixerFilterModelSet {
                model: settings.filter_model,
            },
            Command::GlobalSyncModeSet {
                mode: settings.sync_mode,
            },
            Command::MasterMaximiserDriveSet {
                db: settings.maximiser.drive_db,
            },
            Command::MasterMaximiserCeilingSet {
                db: settings.maximiser.ceiling_db,
            },
            Command::MasterMaximiserReleaseSet {
                ms: settings.maximiser.release_ms,
            },
        ];
        commands.extend(DeckId::ALL.map(|deck| Command::DeckTempoRangeSet {
            deck,
            percent: settings.tempo_range,
        }));
        for (now, wanted, toggle) in [
            (
                mixer.autogain,
                settings.autogain,
                Command::MixerAutogainToggle,
            ),
            (
                mixer.external_eq_gain,
                settings.external_eq_gain,
                Command::MixerExternalEqGainToggle,
            ),
            (
                mixer.external_master_volume,
                settings.external_master_volume,
                Command::MixerExternalMasterVolumeToggle,
            ),
            (
                self.engine.snapshot().load_marker,
                settings.load_marker,
                Command::GlobalLoadMarkerToggle,
            ),
            (
                self.engine.snapshot().fade_markers,
                settings.fade_markers,
                Command::GlobalFadeMarkersToggle,
            ),
            (
                mixer.maximiser.on,
                settings.maximiser.on,
                Command::MasterLimiterToggle,
            ),
        ] {
            if now != wanted {
                commands.push(toggle);
            }
        }
        for command in commands {
            self.dispatch_engine(command);
        }
    }

    /// Keeps the settings of the Decks and Mixer pages as the engine has them, whatever
    /// changed them: the page, a shortcut or a controller.
    fn keep_mixer_settings(&mut self) {
        let snapshot = self.engine.snapshot();
        let mixer = snapshot.mixer;
        let kept = Settings {
            sync_mode: snapshot.clock.sync_mode,
            mixer_mode: mixer.mode,
            eq_model: mixer.eq_model,
            filter_model: mixer.filter_model,
            autogain: mixer.autogain,
            external_eq_gain: mixer.external_eq_gain,
            external_master_volume: mixer.external_master_volume,
            // A length set by a command between whole beats is saved to the nearest one.
            fade_beats: mixer.fade_beats.round() as u32,
            maximiser: mixer.maximiser,
            load_marker: snapshot.load_marker,
            fade_markers: snapshot.fade_markers,
            ..self.settings.clone()
        };
        if kept != self.settings {
            self.settings = kept;
            self.save_settings();
        }
    }

    /// Answers the messages of the MCP client: `docs/behavior/mcp.md`.
    pub fn serve_mcp(&mut self, requests: Receiver<crate::mcp::Request>) {
        let server = Server {
            name: "platter".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        };
        self.mcp = Some((requests, server));
    }

    fn answer_mcp(&mut self) {
        let Some((requests, server)) = self.mcp.take() else {
            return;
        };
        while let Ok(request) = requests.try_recv() {
            let answer = server.handle(&request.line, &mut McpHost(self));
            // The reader may have given up waiting; then nobody takes the answer.
            let _ = request.reply.send(answer);
        }
        self.mcp = Some((requests, server));
    }

    fn run_engine(&mut self, command: Command) -> Result<(), EngineError> {
        match &command {
            // The rules of loading of the Decks page: a playing deck takes no track.
            Command::DeckLoad { deck, path }
            | Command::DeckLoadTrack { deck, path }
            | Command::DeckLoadStems { deck, path, .. }
                if self.settings.load_stopped_only
                    && self.engine.snapshot().decks[deck.index()].playing =>
            {
                self.log(
                    Level::Info,
                    &format!(
                        "deck {} plays: {} is not loaded",
                        deck.letter(),
                        path.display()
                    ),
                );
                return Ok(());
            }
            Command::DeckLoad { deck, .. }
            | Command::DeckLoadTrack { deck, .. }
            | Command::DeckLoadStems { deck, .. } => self.notices[deck.index()] = None,
            Command::MixerCrossfaderCurveSet { curve }
                if self.settings.crossfader_curve != *curve =>
            {
                self.settings.crossfader_curve = *curve;
                self.save_settings();
            }
            _ => {}
        }
        let mixer_setting = matches!(
            command,
            Command::MixerModeSet { .. }
                | Command::MixerAutogainToggle
                | Command::MixerExternalEqGainToggle
                | Command::MixerExternalMasterVolumeToggle
                | Command::MixerCrossfaderFadeBeatsSet { .. }
                | Command::MixerEqModelSet { .. }
                | Command::MixerFilterModelSet { .. }
                | Command::GlobalSyncModeSet { .. }
                | Command::GlobalLoadMarkerToggle
                | Command::GlobalFadeMarkersToggle
                | Command::MasterLimiterToggle
                | Command::MasterMaximiserDriveSet { .. }
                | Command::MasterMaximiserCeilingSet { .. }
                | Command::MasterMaximiserReleaseSet { .. }
        );
        let id = command.id();
        let effect = id.starts_with("fx.")
            || id.starts_with("mixer.fx_assign")
            || id.starts_with("mixer.effect");
        // A knob is saved at exit rather than at every step of a drag.
        let continuous = matches!(
            id,
            "fx.unit.dry_wet.set"
                | "fx.slot.amount.set"
                | "fx.single.knob.set"
                | "mixer.effect.set"
        );
        self.engine.dispatch(command)?;
        if mixer_setting {
            self.keep_mixer_settings();
        }
        if effect {
            self.settings.effects = crate::effects_config::kept(&self.engine.snapshot().effects);
            if !continuous {
                self.save_settings();
            }
        }
        Ok(())
    }

    fn dispatch_app(&mut self, command: AppCommand) {
        self.shell.apply(&command);
        let folder_before = self.browser.current().map(PathBuf::from);
        if let Some(follow_up) = self.browser.apply(&command) {
            self.dispatch(follow_up);
        }
        let folder = self.browser.current().map(PathBuf::from);
        if folder != folder_before {
            self.settings.folder = folder;
            self.save_settings();
        }
        match command {
            AppCommand::BrowserStemsGenerate => {
                if let Some((track, path, name)) = self.browser.selected_collection_track()
                    && let Err(why) = self.separation.start(track, path, name)
                {
                    self.log(Level::Warning, &format!("stems: {why}"));
                }
            }
            AppCommand::BrowserStemsCancel => self.separation.cancel(),
            AppCommand::StemsModelInstall { ref path } => match self.separation.install(path) {
                Ok(()) => self.log(Level::Info, "stems: installing the separation model"),
                Err(why) => self.log(Level::Warning, &format!("stems: {why}")),
            },
            AppCommand::SettingsOpen { .. } => {
                // The page lists the devices: look for them now, not when it was last open.
                self.devices = platter_audio_io::devices();
                self.last_device_scan = Instant::now();
                self.scan_midi();
            }
            AppCommand::MidiControllerAdd
            | AppCommand::MidiControllerRemove { .. }
            | AppCommand::MidiControllerSelect { .. }
            | AppCommand::MidiInputSet { .. }
            | AppCommand::MidiMappingImport { .. }
            | AppCommand::MidiMappingExport { .. }
            | AppCommand::MidiBindingSelect { .. }
            | AppCommand::MidiBindingAdd { .. }
            | AppCommand::MidiBindingDuplicate
            | AppCommand::MidiBindingDelete
            | AppCommand::MidiLearnToggle
            | AppCommand::MidiTargetSet { .. } => {
                self.manage(command);
                self.scan_midi();
            }
            AppCommand::AudioDeviceSet { device } => {
                self.change_audio(|settings| settings.device = device)
            }
            AppCommand::AudioSampleRateSet { hz } => {
                if SAMPLE_RATES.contains(&hz) {
                    self.change_audio(|settings| settings.sample_rate = hz);
                }
            }
            AppCommand::AudioBufferSet { frames } => {
                if BUFFER_SIZES.contains(&frames) {
                    self.change_audio(|settings| settings.buffer_frames = frames);
                }
            }
            AppCommand::AudioRoutingSet { slot, output } => self.change_audio(|settings| {
                let routing = &mut settings.routing;
                match slot.index() {
                    0 => routing.master[0] = output,
                    1 => routing.master[1] = output,
                    2 => routing.headphones[0] = output,
                    _ => routing.headphones[1] = output,
                }
            }),
            AppCommand::AudioDropoutsReset => {
                if let Some(output) = &self.output {
                    output.stats().reset_dropouts();
                }
            }
            AppCommand::LanguageSet { language } => {
                if self.set_language(language) {
                    self.settings.language = language;
                    self.save_settings();
                }
            }
            AppCommand::SettingsRestoreDefaults => self.restore_defaults(),
            AppCommand::KeymapOpen => self.open_keymap(),
            AppCommand::FullscreenToggle => self.fullscreen_wanted = Some(!self.fullscreen),
            AppCommand::FullscreenAtStartToggle => {
                self.settings.fullscreen_at_start = !self.settings.fullscreen_at_start;
                self.save_settings();
            }
            AppCommand::DeckSizeSet { size } => {
                self.change_layout(|layouts| layouts.change(|arrangement| arrangement.size = size));
            }
            AppCommand::DeckLayoutSet { layout } => {
                self.change_layout(|layouts| {
                    layouts.change(|arrangement| arrangement.decks = layout)
                });
            }
            AppCommand::LayoutSelect { index } => {
                self.change_layout(|layouts| layouts.select(index));
            }
            AppCommand::LayoutDuplicate => {
                let base = self.layout_name();
                self.change_layout(|layouts| layouts.duplicate(&base));
            }
            AppCommand::LayoutRename { name } => {
                self.change_layout(|layouts| layouts.rename(&name));
            }
            AppCommand::LayoutDelete => self.change_layout(Layouts::delete),
            AppCommand::LayoutMoveUp => self.change_layout(|layouts| layouts.shift(true)),
            AppCommand::LayoutMoveDown => self.change_layout(|layouts| layouts.shift(false)),
            AppCommand::LayoutZoneToggle { zone } => {
                self.change_layout(|layouts| {
                    layouts.change(|arrangement| match zone {
                        Zone::Effects => arrangement.effects = !arrangement.effects,
                        Zone::Mixer => arrangement.mixer = !arrangement.mixer,
                        Zone::Lanes => arrangement.lanes = !arrangement.lanes,
                    })
                });
            }
            AppCommand::WaveColourSet { mode } => {
                self.settings.wave_colour = mode;
                self.save_settings();
            }
            AppCommand::EffectUnitsSet { units } => {
                self.settings.effect_units = units;
                self.fx_panels = [FxUnit::ALL[0], FxUnit::ALL[1]];
                self.save_settings();
            }
            AppCommand::FxPanelShow { unit } => {
                if self.settings.effect_units == EffectUnits::Four {
                    self.fx_panels[fx_panel_of(unit)] = unit;
                }
            }
            AppCommand::TempoRangeSet { percent } => {
                if TEMPO_RANGE_PERCENTS.contains(&percent) {
                    self.settings.tempo_range = percent;
                    for deck in DeckId::ALL {
                        self.dispatch(Command::DeckTempoRangeSet { deck, percent }.into());
                    }
                    self.save_settings();
                }
            }
            AppCommand::TooltipsToggle => {
                self.settings.tooltips = !self.settings.tooltips;
                self.save_settings();
            }
            AppCommand::BrowserColumnToggle { .. } => {
                self.settings.columns = self.browser.columns().to_vec();
                self.save_settings();
            }
            AppCommand::RecorderFolderSet { path } => {
                if !path.as_os_str().is_empty() {
                    self.settings.recording_folder = Some(path);
                    self.apply_recorder();
                }
            }
            AppCommand::RecorderFolderReset => {
                self.settings.recording_folder = None;
                self.apply_recorder();
            }
            AppCommand::RecorderPrefixSet { prefix } => {
                self.settings.recording_prefix = config::recording_prefix(&prefix);
                self.apply_recorder();
            }
            AppCommand::RecorderSplitSet { megabytes } => {
                self.settings.recording_split = megabytes;
                self.apply_recorder();
            }
            AppCommand::LoopDecaySet { percent } => {
                self.settings.loop_decay = percent.min(100);
                self.apply_loop_decay();
                self.save_settings();
            }
            AppCommand::MusicFolderAdd { path } => {
                if !path.as_os_str().is_empty()
                    && !self.settings.music_folders.contains(&path)
                    && self.settings.music_folders.len() < MAX_MUSIC_FOLDERS
                {
                    self.settings.music_folders.push(path);
                    self.music_folder = Some(self.settings.music_folders.len() - 1);
                    self.save_settings();
                }
            }
            AppCommand::MusicFolderRemove { index } => {
                if index < self.settings.music_folders.len() {
                    self.settings.music_folders.remove(index);
                    self.music_folder = None;
                    self.save_settings();
                }
            }
            AppCommand::MusicFolderSelect { index } => self.music_folder = Some(index),
            AppCommand::ImportAtStartToggle => {
                self.settings.import_at_start = !self.settings.import_at_start;
                self.save_settings();
            }
            AppCommand::CheckAtStartToggle => {
                self.settings.check_at_start = !self.settings.check_at_start;
                self.save_settings();
            }
            AppCommand::LoadStoppedOnlyToggle => {
                self.settings.load_stopped_only = !self.settings.load_stopped_only;
                self.save_settings();
            }
            AppCommand::KeyLockThreadsToggle => {
                self.settings.key_lock_threads = !self.settings.key_lock_threads;
                self.apply_key_lock_threads();
                self.save_settings();
            }
            AppCommand::AnalysisRangeSet { min, max } => {
                match TempoRange::new(f64::from(min), f64::from(max)) {
                    Ok(range) => {
                        self.settings.bpm_range = range;
                        self.background.range.set(range);
                        self.save_settings();
                    }
                    Err(error) => self.log(
                        Level::Warning,
                        &format!("range of tempos {min} to {max} refused: {error}"),
                    ),
                }
            }
            AppCommand::KeyNotationSet { notation } => {
                self.settings.key_notation = notation;
                self.save_settings();
            }
            AppCommand::TagWritingSet { mode } => {
                self.settings.tag_writing = mode;
                self.apply_tag_writing();
                self.save_settings();
            }
            AppCommand::MinPlaytimeSet { seconds } => {
                self.settings.min_playtime = seconds.clamp(MIN_PLAYTIME, MAX_PLAYTIME);
                self.play_clock.set_minimum(self.settings.min_playtime);
                self.save_settings();
            }
            AppCommand::DeckNoticeDismiss { deck } => self.notices[deck.index()] = None,
            _ => {}
        }
    }

    /// Changes the audio settings, saves them and opens the output again when they differ.
    fn change_audio(&mut self, change: impl FnOnce(&mut Settings)) {
        let before = self.settings.clone();
        change(&mut self.settings);
        if self.settings != before {
            self.save_settings();
            self.reopen_audio();
        }
    }

    /// Switches the texts of the interface, and the name of the home folder in the file
    /// selector with them. Returns whether the language could be loaded.
    fn set_language(&mut self, language: Language) -> bool {
        if let Err(error) = self.shell.set_language(language) {
            self.log(Level::Error, &error.to_string());
            return false;
        }
        if let Some(home) = config::home_dir() {
            let name = self.shell.texts().get("browser.home").to_owned();
            self.browser.rename_root(&home, &name);
        }
        true
    }

    /// Tells the collection and the analysis in the background what to write into the files.
    fn apply_tag_writing(&mut self) {
        let mode = self.settings.tag_writing;
        self.browser.set_tag_writing(mode);
        if let Ok(mut shared) = self.background.tag_writing.lock() {
            *shared = mode;
        }
    }

    fn restore_defaults(&mut self) {
        // Where the file selector stands, its roots and the MIDI controllers are not settings
        // of these pages.
        self.fx_panels = [FxUnit::ALL[0], FxUnit::ALL[1]];
        self.settings = Settings {
            folder: self.settings.folder.clone(),
            roots: self.settings.roots.clone(),
            midi: self.settings.midi.clone(),
            // The personal layouts are the user's work, not settings: they stay, and Mixing
            // becomes active.
            layouts: Layouts {
                personal: self.settings.layouts.personal.clone(),
                active: 0,
            },
            ..Settings::default()
        };
        self.save_settings();
        self.play_clock.set_minimum(self.settings.min_playtime);
        self.apply_tag_writing();
        self.set_language(self.settings.language);
        let curve = self.settings.crossfader_curve;
        self.dispatch(Command::MixerCrossfaderCurveSet { curve }.into());
        self.apply_mixer_settings();
        self.apply_recorder();
        self.reopen_audio();
    }

    /// Opens the shortcut file in the program the system uses for it.
    fn open_keymap(&mut self) {
        let Some(path) = self.folder.as_ref().map(|folder| folder.join(KEYMAP_FILE)) else {
            return;
        };
        let program = if cfg!(windows) {
            "explorer"
        } else if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        if let Err(error) = std::process::Command::new(program).arg(&path).spawn() {
            self.log(
                Level::Warning,
                &format!("cannot open the shortcut file: {error}"),
            );
        }
    }

    /// Gives the decks the time since the last frame and sends the tracks that have just
    /// counted as played to the library; tells the browser of a track a deck has just loaded.
    fn count_played(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_play_check).as_secs_f64();
        self.last_play_check = now;
        let unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
            });
        let snapshot = self.engine.snapshot();
        let decks = DeckId::ALL.map(|deck| {
            let state = &snapshot.decks[deck.index()];
            (
                state.track.as_ref().map(|track| track.path.as_path()),
                state.playing,
            )
        });
        for played in self.play_clock.advance(decks, elapsed, unix) {
            self.browser.played(played);
        }
        for (known, deck) in self.deck_tracks.iter_mut().zip(&snapshot.decks) {
            let path = deck.track.as_ref().map(|track| &track.path);
            if known.as_ref() != path {
                *known = path.cloned();
                if let Some(path) = path {
                    self.browser.loaded(path.clone());
                }
            }
        }
        for (path, kept) in self.keeping.observe(&snapshot.decks, now) {
            self.browser.keep(path, kept);
        }
        self.browser.set_deck_tracks(
            snapshot
                .decks
                .each_ref()
                .map(|deck| deck.track.as_ref().map(|track| track.path.clone())),
        );
        // The analysis in the background leaves the decks alone while they load and analyse,
        // and leaves their tracks to them.
        let busy = snapshot
            .decks
            .iter()
            .any(|deck| deck.loading || deck.analysing);
        self.background.pause.store(busy, Ordering::Relaxed);
        if let Ok(mut held) = self.background.held.lock() {
            *held = snapshot
                .decks
                .iter()
                .filter_map(|deck| Some(deck.track.as_ref()?.path.clone()))
                .collect();
        }
    }

    /// Takes in what happened in the background since the last frame.
    fn poll(&mut self) {
        for why in self.browser.take_unwritten() {
            self.log(Level::Warning, &format!("cannot write the tags of {why}"));
        }
        for why in self.browser.take_unattached() {
            self.log(Level::Warning, &format!("no stems attached: {why}"));
        }
        for finished in self.separation.poll() {
            match finished {
                crate::separation::Finished::Separated { track, folder } => {
                    self.log(
                        Level::Info,
                        &format!("stems written to {}", folder.display()),
                    );
                    self.browser.attach_stems_to(track, folder);
                }
                crate::separation::Finished::Failed(why) => {
                    self.log(Level::Warning, &format!("stems: {why}"));
                }
                crate::separation::Finished::Installed => {
                    self.log(Level::Info, "stems: the separation model is installed");
                }
                crate::separation::Finished::InstallFailed(why) => {
                    self.log(
                        Level::Warning,
                        &format!("stems: the model was not installed: {why}"),
                    );
                }
            }
        }
        self.browser
            .set_separation(self.separation.status(), self.separation.model_installed());
        while let Ok(report) = self.background_reports.try_recv() {
            match report {
                background::Report::Progress { done, total } => self
                    .browser
                    .set_analysis(Some(AnalysisStatus { done, total }), None),
                background::Report::Idle => self.browser.set_analysis(None, None),
                background::Report::NotWritten(why) => {
                    self.log(Level::Warning, &format!("cannot write the tags of {why}"));
                }
                background::Report::Stopped(why) => {
                    self.log(Level::Warning, &format!("the analysis stopped: {why}"));
                    self.browser.set_analysis(None, Some(why));
                }
            }
        }
        while let Ok(error) = self.backup_failure.try_recv() {
            self.log(
                Level::Warning,
                &format!("cannot back up the library: {error}"),
            );
        }
        for event in self.engine.poll() {
            match event {
                Event::Loaded { deck, .. } => self.notices[deck.index()] = None,
                Event::LoadFailed { deck, message } => {
                    self.notices[deck.index()] = Some(DeckNotice::LoadFailed);
                    self.log(
                        Level::Warning,
                        &format!("deck {}: {message}", deck.letter()),
                    );
                }
                Event::StemsFailed { deck, message } => {
                    self.notices[deck.index()] = Some(DeckNotice::StemsFailed);
                    self.log(
                        Level::Warning,
                        &format!("deck {}: stems: {message}", deck.letter()),
                    );
                }
                Event::PreviewFailed { message } => {
                    self.log(Level::Warning, &format!("preview: {message}"));
                }
                event @ (Event::Recorded { .. } | Event::RecordingFailed { .. }) => {
                    self.log_recording(&event);
                }
            }
        }
        self.browser.poll();
        self.start_library();
        self.count_played();
        let lost = self
            .output
            .as_ref()
            .is_some_and(|output| output.stats().is_lost());
        // Without a device, look again now and then: one may have been plugged in.
        let retry = self.problem.is_some() && self.last_device_scan.elapsed() > DEVICE_RETRY;
        if lost {
            self.log(Level::Warning, "audio: the device went away");
            self.reopen_audio();
        } else if retry {
            self.last_device_scan = Instant::now();
            let devices = platter_audio_io::devices();
            // Try again when the devices changed, or when the one wanted was only busy.
            let worth_trying = devices != self.devices || self.problem == Some(AudioProblem::Busy);
            self.devices = devices;
            if worth_trying {
                self.reopen_audio();
            }
        }
    }

    fn audio_view(&self) -> AudioView {
        let stream = self
            .output
            .as_ref()
            .filter(|output| output.is_audible())
            .map(|output| {
                let info = output.info();
                StreamView {
                    kind: match info.kind {
                        DeviceKind::BuiltIn => OutputKind::BuiltIn,
                        DeviceKind::Usb => OutputKind::Usb,
                        DeviceKind::Bluetooth => OutputKind::Bluetooth,
                        DeviceKind::Other => OutputKind::Other,
                    },
                    sample_rate: info.sample_rate,
                    channels: info.channels,
                    buffer_frames: info.buffer_frames,
                    latency_ms: info.latency_ms(),
                    load: output.stats().load(),
                    dropouts: output.stats().dropouts(),
                    headphones: info.headphones,
                }
            });
        AudioView {
            state: match stream {
                Some(stream) => AudioState::Running(stream),
                None => AudioState::Stopped(self.problem.unwrap_or(AudioProblem::Failed)),
            },
            devices: self
                .devices
                .iter()
                .map(|device| DeviceView {
                    name: device.name.clone(),
                    outputs: device.outputs,
                    inputs: device.inputs,
                    sample_rates: device.sample_rates.clone(),
                })
                .collect(),
            sample_rates: SAMPLE_RATES.to_vec(),
            buffer_sizes: BUFFER_SIZES.to_vec(),
        }
    }

    fn settings_view(&self) -> SettingsView {
        let routing = self.settings.routing;
        let arrangement = self.settings.layouts.arrangement();
        SettingsView {
            device: self.settings.device.clone(),
            sample_rate: self.settings.sample_rate,
            buffer_frames: self.settings.buffer_frames,
            routing: [
                routing.master[0],
                routing.master[1],
                routing.headphones[0],
                routing.headphones[1],
            ],
            language: self.settings.language,
            fullscreen: self.fullscreen,
            tooltips: self.settings.tooltips,
            fullscreen_at_start: self.settings.fullscreen_at_start,
            tempo_range: self.settings.tempo_range,
            deck_size: arrangement.size,
            deck_layout: arrangement.decks,
            layouts: self.settings.layouts.clone(),
            shown: Shown {
                effects: arrangement.effects,
                mixer: arrangement.mixer,
                lanes: arrangement.has_lanes(),
                single: arrangement.single,
            },
            wave_colour: self.settings.wave_colour,
            effect_units: self.settings.effect_units,
            fx_panels: match self.settings.effect_units {
                EffectUnits::Two => [FxUnit::ALL[0], FxUnit::ALL[1]],
                EffectUnits::Four => self.fx_panels,
            },
            settings_file: SETTINGS_FILE.to_owned(),
            keymap_file: KEYMAP_FILE.to_owned(),
            library_file: LIBRARY_FILE.to_owned(),
            min_playtime: self.settings.min_playtime,
            key_notation: self.settings.key_notation,
            bpm_range: (
                self.settings.bpm_range.min().round() as u32,
                self.settings.bpm_range.max().round() as u32,
            ),
            load_stopped_only: self.settings.load_stopped_only,
            key_lock_threads: self.settings.key_lock_threads,
            recording_folder: self
                .recordings()
                .map(|folder| folder.display().to_string())
                .unwrap_or_default(),
            recording_folder_chosen: self.settings.recording_folder.is_some(),
            recording_prefix: self.settings.recording_prefix.clone(),
            recording_split: self.settings.recording_split,
            music_folders: self.settings.music_folders.clone(),
            music_folder: self.music_folder,
            import_at_start: self.settings.import_at_start,
            check_at_start: self.settings.check_at_start,
            controllers: self.manager_view(),
            tag_writing: self.settings.tag_writing,
        }
    }
}

/// The application as the MCP server sees it: its commands, run as the keyboard runs them, and
/// its state.
struct McpHost<'a>(&'a mut PlatterApp);

impl Host for McpHost<'_> {
    fn commands(&self) -> Vec<&'static CommandSpec> {
        Action::specs().collect()
    }

    fn run(&mut self, id: &str, args: &Args) -> Result<String, String> {
        let action = Action::from_args(id, args).map_err(|error| error.to_string())?;
        match action {
            Action::Engine(command) => self
                .0
                .run_engine(command)
                .map_err(|error| error.to_string())?,
            Action::App(command) => self.0.dispatch_app(command),
        }
        Ok(format!("{id}: done"))
    }

    fn state(&mut self) -> McpValue {
        crate::mcp::state_of(&self.0.engine.snapshot())
    }
}

impl eframe::App for PlatterApp {
    fn on_exit(&mut self) {
        // The output still runs, so the processor marks the end and the file is finished.
        for event in self.engine.finish_recording(EXIT_WAIT) {
            self.log_recording(&event);
        }
        self.background.stop.store(true, Ordering::Relaxed);
        // The knobs of the effects are saved here, not at every step of a drag.
        self.save_settings();
        for (path, kept) in self.keeping.flush() {
            self.browser.keep(path, kept);
        }
        // The worker of the collection is not waited for at the end of the process: give it
        // a moment to write what was just sent.
        let deadline = Instant::now() + EXIT_WAIT;
        while self.browser.collection_busy() && Instant::now() < deadline {
            self.browser.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        let midi_waiting = self.take_midi();
        self.poll();
        self.answer_mcp();
        let ctx = ui.ctx().clone();
        // A window narrower than the layout is designed for shows the whole interface
        // smaller; the layout itself has no narrow form yet.
        let zoom = ctx.zoom_factor();
        let fit = (ui.max_rect().width() * zoom / DESIGN_WIDTH).min(1.0);
        if fit > 0.1 && (fit - zoom).abs() > 0.005 {
            ctx.set_zoom_factor(fit);
        }

        let engine = self.engine.snapshot();
        let audio = self.audio_view();
        let settings = self.settings_view();
        let browser = self.browser.view();
        let view = View {
            engine: &engine,
            audio: &audio,
            settings: &settings,
            browser: &browser,
            keymap: &self.keymap,
            clock: None,
            notices: self.notices,
            midi: MidiView {
                named: self.settings.midi.len(),
                open: self.controllers.len(),
                active: self
                    .last_midi
                    .is_some_and(|last| last.elapsed() < MIDI_LIGHT),
            },
        };
        let midi_lit = view.midi.active;
        let actions = self.shell.show(ui, &view);
        for action in actions {
            self.dispatch(action);
        }
        // The window manager may leave full screen by itself; the window says what it is.
        if let Some(fullscreen) = self.fullscreen_wanted.take() {
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(fullscreen));
        }
        if let Some(fullscreen) = ctx.input(|input| input.viewport().fullscreen) {
            self.fullscreen = fullscreen;
        }

        let busy = engine.decks.iter().any(|deck| deck.playing || deck.loading)
            || engine
                .master_levels
                .iter()
                .any(|level| *level > QUIET_LEVEL)
            || browser.reading
            || self.browser.collection_busy()
            || midi_waiting;
        if busy {
            ctx.request_repaint();
        } else if self.keeping.waiting() {
            // A change of the grid or the cues is kept once it settles.
            ctx.request_repaint_after(SETTLE);
        } else if midi_lit {
            // The light of the MIDI indicator goes out without another message to wake it.
            ctx.request_repaint_after(MIDI_LIGHT);
        } else {
            ctx.request_repaint_after(IDLE_REPAINT);
        }
    }
}
