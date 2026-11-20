//! The Decks, Mixer and Library pages of the preferences and the maintenance of the collection,
//! drawn without a window.
//!
//! Behaviour notes: `docs/behavior/settings.md`, Decks, Mixer and Library; `docs/behavior/browser.md`,
//! *Collection maintenance*.

#![allow(clippy::unwrap_used)]

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect};
use platter_engine::{Command, MixerMode, SyncMode};
use platter_ui_egui::collection::TagWriting;
use platter_ui_egui::command::{Action, AppCommand, DeckSize, SettingsPage};
use platter_ui_egui::cx::Control;
use platter_ui_egui::i18n::Language;
use platter_ui_egui::keys::KeyNotation;
use platter_ui_egui::sample::{self, Scene};
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};

/// The commands of the controls of the page.
const PAGE: [&str; 7] = [
    "global.fade_markers.toggle",
    "mixer.mode.set",
    "mixer.autogain.toggle",
    "fx.keep_values.toggle",
    "mixer.external.eq_gain.toggle",
    "mixer.external.master_volume.toggle",
    "mixer.crossfader.fade_beats.set",
];

struct Stage {
    ctx: egui::Context,
    shell: Shell,
    scene: Scene,
    time: f64,
}

impl Stage {
    fn new(scene: Scene, language: Language) -> Self {
        Self::on_page(scene, language, SettingsPage::Mixer)
    }

    fn on_page(scene: Scene, language: Language, page: SettingsPage) -> Self {
        let mut shell = Shell::new(language).unwrap();
        shell.state.settings_page = Some(page);
        let ctx = egui::Context::default();
        shell.install(&ctx);
        let mut stage = Self {
            ctx,
            shell,
            scene,
            time: 0.0,
        };
        stage.frame(Vec::new());
        stage.frame(Vec::new());
        stage
    }

    fn frame(&mut self, events: Vec<Event>) -> Vec<Action> {
        self.time += 1.0 / 60.0;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(DESIGN_WIDTH, DESIGN_HEIGHT),
            )),
            time: Some(self.time),
            events,
            focused: true,
            ..RawInput::default()
        };
        let mut actions = Vec::new();
        let view = self.scene.view();
        let shell = &mut self.shell;
        let mut output = self.ctx.run_ui(input, |ui| {
            actions = shell.show(ui, &view);
        });
        output.textures_delta.clear();
        actions
    }

    fn click(&mut self, pos: Pos2) -> Vec<Action> {
        let button = |pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let mut actions = self.frame(vec![Event::PointerMoved(pos)]);
        actions.extend(self.frame(vec![Event::PointerMoved(pos), button(true)]));
        actions.extend(self.frame(vec![button(false)]));
        actions.extend(self.frame(Vec::new()));
        actions
    }

    /// The controls of the page, in the order they are drawn.
    fn mixer_controls(&self) -> Vec<Control> {
        self.shell
            .controls()
            .iter()
            .filter(|control| PAGE.contains(&control.command))
            .cloned()
            .collect()
    }

    /// Scrolls the page, which is taller than its room, and lets it settle.
    fn scroll_to(&mut self, scroll: f32) {
        self.shell.state.settings.page_scroll = scroll;
        self.frame(Vec::new());
        self.frame(Vec::new());
    }

    /// How many controls emit `command` at the top of the page or at its end, wherever they
    /// show.
    fn count_anywhere(&mut self, command: &str) -> usize {
        let mut most = 0;
        for scroll in [0.0, f32::MAX] {
            self.scroll_to(scroll);
            let count = self
                .mixer_controls()
                .iter()
                .filter(|control| control.command == command && control.enabled)
                .count();
            most = most.max(count);
        }
        self.scroll_to(0.0);
        most
    }

    fn click_control(&mut self, command: &str, nth: usize) -> Vec<Action> {
        let find = |stage: &Self| {
            stage
                .mixer_controls()
                .into_iter()
                .filter(|control| control.command == command)
                .nth(nth)
        };
        let control = match find(self) {
            Some(control) => control,
            None => {
                self.scroll_to(f32::MAX);
                find(self).unwrap()
            }
        };
        self.click(control.rect.center())
    }
}

fn engine(actions: Vec<Action>) -> Vec<Command> {
    actions
        .into_iter()
        .filter_map(|action| match action {
            Action::Engine(command) => Some(command),
            Action::App(_) => None,
        })
        .collect()
}

#[test]
fn the_page_has_a_control_for_each_mixer_setting() {
    let mut stage = Stage::new(sample::playing(), Language::English);
    let mut count = |command: &str| stage.count_anywhere(command);
    assert_eq!(count("mixer.mode.set"), 2);
    assert_eq!(count("mixer.autogain.toggle"), 1);
    assert_eq!(count("fx.keep_values.toggle"), 1);
    assert_eq!(count("global.fade_markers.toggle"), 1);
    assert_eq!(count("mixer.external.eq_gain.toggle"), 1);
    assert_eq!(count("mixer.external.master_volume.toggle"), 1);
    assert_eq!(count("mixer.crossfader.fade_beats.set"), 7);
}

#[test]
fn each_control_emits_its_command() {
    let mut stage = Stage::new(sample::playing(), Language::English);
    assert_eq!(
        engine(stage.click_control("mixer.mode.set", 1)),
        [Command::MixerModeSet {
            mode: MixerMode::External
        }]
    );
    for (command, expected) in [
        ("mixer.autogain.toggle", Command::MixerAutogainToggle),
        ("fx.keep_values.toggle", Command::FxKeepValuesToggle),
        (
            "global.fade_markers.toggle",
            Command::GlobalFadeMarkersToggle,
        ),
        (
            "mixer.external.eq_gain.toggle",
            Command::MixerExternalEqGainToggle,
        ),
        (
            "mixer.external.master_volume.toggle",
            Command::MixerExternalMasterVolumeToggle,
        ),
    ] {
        assert_eq!(engine(stage.click_control(command, 0)), [expected]);
    }
    for (nth, beats) in [(0, 1.0), (3, 8.0), (6, 64.0)] {
        assert_eq!(
            engine(stage.click_control("mixer.crossfader.fade_beats.set", nth)),
            [Command::MixerCrossfaderFadeBeatsSet { beats }]
        );
    }
}

#[test]
fn the_controls_fit_the_page_in_both_languages() {
    for language in [Language::English, Language::Spanish] {
        let stage = Stage::new(sample::playing(), language);
        let controls = stage.mixer_controls();
        let page = Rect::from_min_size(Pos2::ZERO, egui::vec2(DESIGN_WIDTH, DESIGN_HEIGHT));
        for (index, control) in controls.iter().enumerate() {
            assert!(page.contains_rect(control.rect), "{}", control.command);
            for other in &controls[index + 1..] {
                assert!(
                    !control.rect.shrink(0.5).intersects(other.rect.shrink(0.5)),
                    "{} overlaps {} in {language:?}",
                    control.command,
                    other.command
                );
            }
        }
    }
}

/// The controls of the minimum play time on the Library page, in the order they are drawn.
fn playtimes(stage: &Stage) -> Vec<Control> {
    stage
        .shell
        .controls()
        .iter()
        .filter(|control| control.command == "app.min_playtime.set")
        .cloned()
        .collect()
}

#[test]
fn the_library_page_sets_the_minimum_play_time() {
    let mut stage = Stage::on_page(sample::playing(), Language::English, SettingsPage::Library);
    let controls = playtimes(&stage);
    assert_eq!(controls.len(), 7);
    let page = Rect::from_min_size(Pos2::ZERO, egui::vec2(DESIGN_WIDTH, DESIGN_HEIGHT));
    assert!(
        controls
            .iter()
            .all(|control| page.contains_rect(control.rect))
    );
    let actions = stage.click(controls[4].rect.center());
    assert!(
        actions.contains(&Action::App(AppCommand::MinPlaytimeSet { seconds: 60 })),
        "{actions:?}"
    );
    let spanish = Stage::on_page(sample::playing(), Language::Spanish, SettingsPage::Library);
    assert_eq!(playtimes(&spanish).len(), 7);
}

fn app(actions: &[Action]) -> Vec<AppCommand> {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::App(command) => Some(command.clone()),
            Action::Engine(_) => None,
        })
        .collect()
}

fn control(stage: &Stage, command: &str) -> Option<Control> {
    stage
        .shell
        .controls()
        .iter()
        .find(|control| control.command == command)
        .cloned()
}

#[test]
fn the_maintenance_opens_from_the_library_page_and_asks_before_removing() {
    let mut stage = Stage::on_page(sample::playing(), Language::English, SettingsPage::Library);
    let open = control(&stage, "library.maintenance.open").unwrap();
    let actions = stage.click(open.rect.center());
    assert_eq!(app(&actions), [AppCommand::LibraryMaintenanceOpen]);
    stage.shell.apply(&AppCommand::LibraryMaintenanceOpen);
    stage.frame(Vec::new());
    // The two missing tracks of the sample can be relocated in the folder of the explorer.
    let relocate = control(&stage, "library.relocate").unwrap();
    assert!(relocate.enabled);
    assert!(control(&stage, "library.missing.remove").is_none());
    let ask = control(&stage, "library.missing.ask").unwrap();
    assert_eq!(
        app(&stage.click(ask.rect.center())),
        [AppCommand::LibraryMissingAsk]
    );
    stage.shell.apply(&AppCommand::LibraryMissingAsk);
    stage.frame(Vec::new());
    let remove = control(&stage, "library.missing.remove").unwrap();
    assert!(control(&stage, "library.missing.keep").is_some());
    assert_eq!(
        app(&stage.click(remove.rect.center())),
        [AppCommand::LibraryMissingRemove]
    );
    // Escape closes the maintenance, not the preferences.
    let actions = stage.frame(vec![Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }]);
    assert_eq!(app(&actions), [AppCommand::LibraryMaintenanceClose]);
}

#[test]
fn the_decks_page_sets_the_sync_mode() {
    let mut stage = Stage::on_page(sample::playing(), Language::English, SettingsPage::Decks);
    let modes: Vec<Control> = stage
        .shell
        .controls()
        .iter()
        .filter(|control| control.command == "global.sync.mode.set")
        .cloned()
        .collect();
    assert_eq!(modes.len(), 2);
    assert_eq!(
        engine(stage.click(modes[0].rect.center())),
        [Command::GlobalSyncModeSet {
            mode: SyncMode::Tempo
        }]
    );
}

#[test]
fn the_decks_page_sets_the_notation_of_keys() {
    let mut stage = Stage::on_page(sample::playing(), Language::English, SettingsPage::Decks);
    let mut notations: Vec<Control> = stage
        .shell
        .controls()
        .iter()
        .filter(|control| control.command == "app.key_notation.set")
        .cloned()
        .collect();
    notations.sort_by(|first, second| first.rect.left().total_cmp(&second.rect.left()));
    assert_eq!(notations.len(), 3);
    assert_eq!(
        app(&stage.click(notations[2].rect.center())),
        [AppCommand::KeyNotationSet {
            notation: KeyNotation::OpenKey
        }]
    );
}

#[test]
fn the_library_page_sets_what_is_written_into_the_files() {
    let mut stage = Stage::on_page(sample::playing(), Language::English, SettingsPage::Library);
    let mut modes: Vec<Control> = stage
        .shell
        .controls()
        .iter()
        .filter(|control| control.command == "library.tag_writing.set")
        .cloned()
        .collect();
    modes.sort_by(|first, second| first.rect.left().total_cmp(&second.rect.left()));
    assert_eq!(modes.len(), 3);
    assert_eq!(
        app(&stage.click(modes[2].rect.center())),
        [AppCommand::TagWritingSet {
            mode: TagWriting::Nothing
        }]
    );
}

#[test]
fn the_library_page_keeps_the_music_folders_and_what_happens_at_start() {
    let mut stage = Stage::on_page(sample::playing(), Language::English, SettingsPage::Library);
    let add = control(&stage, "library.music_folder.add").unwrap();
    assert!(add.enabled);
    assert_eq!(
        app(&stage.click(add.rect.center())),
        [AppCommand::MusicFolderAdd {
            path: "Home/Music/Sets".into()
        }]
    );
    let remove = control(&stage, "library.music_folder.remove").unwrap();
    assert_eq!(
        app(&stage.click(remove.rect.center())),
        [AppCommand::MusicFolderRemove { index: 0 }]
    );
    for (command, sent) in [
        (
            "library.import_at_start.toggle",
            AppCommand::ImportAtStartToggle,
        ),
        (
            "library.check_at_start.toggle",
            AppCommand::CheckAtStartToggle,
        ),
    ] {
        let switch = control(&stage, command).unwrap();
        assert_eq!(app(&stage.click(switch.rect.center())), [sent]);
    }

    // The folder the explorer shows is already listed: the add key rests. With no folder,
    // there is nothing to remove.
    let mut scene = sample::playing();
    scene.browser.explorer = Some("Music/Sets".into());
    let stage = Stage::on_page(scene, Language::English, SettingsPage::Library);
    assert!(!control(&stage, "library.music_folder.add").unwrap().enabled);
    let mut scene = sample::playing();
    scene.settings.music_folders.clear();
    let stage = Stage::on_page(scene, Language::English, SettingsPage::Library);
    assert!(
        !control(&stage, "library.music_folder.remove")
            .unwrap()
            .enabled
    );
}

#[test]
fn the_decks_page_sets_the_range_of_tempos_of_the_analysis() {
    let mut stage = Stage::on_page(sample::playing(), Language::English, SettingsPage::Decks);
    let range = control(&stage, "app.analysis_range.set").unwrap();
    assert!(stage.click(range.rect.center()).is_empty());
    // The second row of the list: the first range of one octave.
    let row = range.rect.center_bottom() + egui::vec2(0.0, 2.0 + 14.0 + 28.0);
    assert_eq!(
        app(&stage.click(row)),
        [AppCommand::AnalysisRangeSet { min: 60, max: 120 }]
    );
}

#[test]
fn the_decks_page_sets_the_tempo_range_and_the_general_page_the_start_in_full_screen() {
    let mut stage = Stage::on_page(sample::playing(), Language::English, SettingsPage::Decks);
    let mut ranges: Vec<Control> = stage
        .shell
        .controls()
        .iter()
        .filter(|control| control.command == "app.tempo_range.set")
        .cloned()
        .collect();
    ranges.sort_by(|first, second| first.rect.left().total_cmp(&second.rect.left()));
    assert_eq!(ranges.len(), 3);
    assert_eq!(
        app(&stage.click(ranges[2].rect.center())),
        [AppCommand::TempoRangeSet { percent: 50 }]
    );

    let mut stage = Stage::on_page(sample::playing(), Language::English, SettingsPage::General);
    let switch = control(&stage, "app.fullscreen_at_start.toggle").unwrap();
    assert_eq!(
        app(&stage.click(switch.rect.center())),
        [AppCommand::FullscreenAtStartToggle]
    );

    // The size of the decks: `docs/behavior/interface.md`, *Deck sizes*.
    let mut sizes: Vec<Control> = stage
        .shell
        .controls()
        .iter()
        .filter(|control| control.command == "app.deck_size.set")
        .cloned()
        .collect();
    sizes.sort_by(|first, second| first.rect.left().total_cmp(&second.rect.left()));
    assert_eq!(sizes.len(), 2);
    assert_eq!(
        app(&stage.click(sizes[1].rect.center())),
        [AppCommand::DeckSizeSet {
            size: DeckSize::Compact
        }]
    );
}

/// The size of the preferences dialog, centred in the window.
const DIALOG_SIZE: egui::Vec2 = egui::vec2(880.0, 620.0);

#[test]
fn the_controls_of_the_general_decks_and_library_pages_fit_the_dialog_in_both_languages() {
    let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(DESIGN_WIDTH, DESIGN_HEIGHT));
    let dialog = Rect::from_center_size(screen.center(), DIALOG_SIZE);
    for language in [Language::English, Language::Spanish] {
        for page in [
            SettingsPage::General,
            SettingsPage::Decks,
            SettingsPage::Library,
        ] {
            let stage = Stage::on_page(sample::playing(), language, page);
            let controls: Vec<Control> = stage
                .shell
                .controls()
                .iter()
                .filter(|control| dialog.contains(control.rect.center()))
                .cloned()
                .collect();
            assert!(controls.len() > 3, "{page:?}");
            for (index, control) in controls.iter().enumerate() {
                assert!(
                    dialog.contains_rect(control.rect),
                    "{} leaves the dialog on {page:?} in {language:?}: {:?}",
                    control.command,
                    control.rect
                );
                for other in &controls[index + 1..] {
                    assert!(
                        !control.rect.shrink(0.5).intersects(other.rect.shrink(0.5)),
                        "{} overlaps {} on {page:?} in {language:?}",
                        control.command,
                        other.command
                    );
                }
            }
        }
    }
}
