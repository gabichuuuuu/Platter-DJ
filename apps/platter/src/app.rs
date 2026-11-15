//! The application: it owns the engine, the audio output, the settings and the file
//! selector, builds the view of each frame and carries out the commands the interface
//! answers with.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use platter_audio_io::{
    AudioError, BUFFER_SIZES, DeviceInfo, DeviceKind, Output, SAMPLE_RATES, Source, StreamRequest,
};
use platter_core::log::{Level, Logger};
use platter_core::{DeckId, SampleRate};
use platter_engine::{Command, Engine, Event, Processor};
use platter_ui_egui::command::{Action, AppCommand};
use platter_ui_egui::files::{FileBrowser, Root};
use platter_ui_egui::i18n::Language;
use platter_ui_egui::keymap::{DEFAULT_KEYMAP, Keymap};
use platter_ui_egui::model::{
    AudioProblem, AudioState, AudioView, DeckNotice, DeviceView, OutputKind, SettingsView,
    StreamView, View,
};
use platter_ui_egui::shell::{DESIGN_WIDTH, Shell};

use crate::config::{self, KEYMAP_FILE, LOG_FILE, SETTINGS_FILE, Settings};

/// How often the devices are looked for again while none is playing.
const DEVICE_RETRY: Duration = Duration::from_secs(3);
/// How often an idle window is drawn, for the readings that change on their own.
const IDLE_REPAINT: Duration = Duration::from_millis(100);
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
    /// The folder of the files of the user; without one nothing is saved.
    folder: Option<PathBuf>,
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
}

impl PlatterApp {
    pub fn new(shell: Shell, settings: Settings, folder: Option<PathBuf>) -> Self {
        let logger = folder.as_ref().and_then(|folder| {
            std::fs::create_dir_all(folder).ok()?;
            Logger::to_file(&folder.join(LOG_FILE)).ok()
        });
        let home_name = shell.texts().get("browser.home").to_owned();
        let mut browser = FileBrowser::new(roots(&home_name, &settings.roots));
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
        let (engine, processor) = Engine::new(rate_of(engine_rate));
        let mut app = Self {
            folder,
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
            browser,
            notices: [None; DeckId::COUNT],
        };
        app.log(
            Level::Info,
            &format!("Platter DJ {} started", env!("CARGO_PKG_VERSION")),
        );
        app.load_keymap();
        app.open_audio(Some(source_of(processor)));
        let curve = app.settings.crossfader_curve;
        app.dispatch(Command::MixerCrossfaderCurveSet { curve }.into());
        app.apply_mixer_settings();
        app
    }

    fn log(&self, level: Level, message: &str) {
        if let Some(logger) = &self.logger {
            logger.line(level, message);
        }
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

    /// Builds an engine at the sample rate of the settings with the controls where they
    /// stand, and loads again the tracks the decks held. Returns its processor.
    fn rebuild_engine(&mut self) -> Processor {
        let snapshot = self.engine.snapshot();
        let rate = self.settings.sample_rate;
        let (engine, processor) = Engine::with_controls_of(rate_of(rate), &self.engine);
        self.engine = engine;
        self.engine_rate = rate;
        for deck in DeckId::ALL {
            if let Some(track) = &snapshot.decks[deck.index()].track {
                let path = track.path.clone();
                self.dispatch(Command::DeckLoad { deck, path }.into());
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
                match &command {
                    Command::DeckLoad { deck, .. } => self.notices[deck.index()] = None,
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
                );
                self.dispatch_engine(command);
                if mixer_setting {
                    self.keep_mixer_settings();
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
        ];
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
        ] {
            if now != wanted {
                commands.push(toggle);
            }
        }
        for command in commands {
            self.dispatch_engine(command);
        }
    }

    /// Keeps the settings of the Mixer page as the engine has them, whatever changed them:
    /// the page, a shortcut or a controller.
    fn keep_mixer_settings(&mut self) {
        let mixer = self.engine.snapshot().mixer;
        let kept = Settings {
            mixer_mode: mixer.mode,
            autogain: mixer.autogain,
            external_eq_gain: mixer.external_eq_gain,
            external_master_volume: mixer.external_master_volume,
            // A length set by a command between whole beats is saved to the nearest one.
            fade_beats: mixer.fade_beats.round() as u32,
            ..self.settings.clone()
        };
        if kept != self.settings {
            self.settings = kept;
            self.save_settings();
        }
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
            AppCommand::SettingsOpen { .. } => {
                // The page lists the devices: look for them now, not when it was last open.
                self.devices = platter_audio_io::devices();
                self.last_device_scan = Instant::now();
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

    fn restore_defaults(&mut self) {
        // Where the file selector stands and its roots are not settings of these pages.
        self.settings = Settings {
            folder: self.settings.folder.clone(),
            roots: self.settings.roots.clone(),
            ..Settings::default()
        };
        self.save_settings();
        self.set_language(self.settings.language);
        let curve = self.settings.crossfader_curve;
        self.dispatch(Command::MixerCrossfaderCurveSet { curve }.into());
        self.apply_mixer_settings();
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

    /// Takes in what happened in the background since the last frame.
    fn poll(&mut self) {
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
            }
        }
        self.browser.poll();
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
            settings_file: SETTINGS_FILE.to_owned(),
            keymap_file: KEYMAP_FILE.to_owned(),
        }
    }
}

impl eframe::App for PlatterApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
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
        };
        let actions = self.shell.show(ui, &view);
        for action in actions {
            self.dispatch(action);
        }

        let busy = engine.decks.iter().any(|deck| deck.playing || deck.loading)
            || engine
                .master_levels
                .iter()
                .any(|level| *level > QUIET_LEVEL)
            || browser.reading;
        if busy {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(IDLE_REPAINT);
        }
    }
}
