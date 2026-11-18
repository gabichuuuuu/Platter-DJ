//! The panels drawn without a window: one frame of egui at a time, with invented data.
//!
//! Four things are checked on every state of the reference screens: each control emits its
//! command, no text is written in the code, every command can be reached with the keyboard,
//! and no colour comes from outside the design tokens.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::epaint::{ClippedShape, ColorMode, Shape};
use egui::{Color32, Event, Modifiers, PointerButton, Pos2, RawInput, Rect};
use platter_core::{CueKind, DeckId, HotcueSlot};
use platter_engine::{ClockMode, Command, EffectKind, MixerMode, UnitMode};
use platter_library::{NodeId, SessionId};
use platter_ui_egui::command::NewItem;
use platter_ui_egui::command::{APP_SPECS, Action, AppCommand, DeckLayout, DeckPage, SettingsPage};
use platter_ui_egui::cx::{Control, ControlKind};
use platter_ui_egui::i18n::{Language, Texts};
use platter_ui_egui::model::DeckNotice;
use platter_ui_egui::sample::{self, Scene};
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};
use platter_ui_egui::{BrowserMenu, MenuTarget, Naming};

const OPEN: char = '\u{27e6}';
const CLOSE: char = '\u{27e7}';

/// A state of the interface to draw: a scene, the open page of the preferences and the
/// notices of the decks.
#[derive(Clone)]
struct State {
    name: &'static str,
    scene: Scene,
    page: Option<SettingsPage>,
    notices: [Option<DeckNotice>; 2],
    /// The page of the advanced panel each deck shows.
    pages: [DeckPage; 2],
    /// The hotcues of each deck are in the mapping mode.
    mapping: [bool; 2],
    /// The context menu of the browser that is open.
    menu: Option<BrowserMenu>,
    /// The field of the tree that names a playlist or a folder.
    naming: Option<Naming>,
    /// How far the tree is scrolled, so the folders of the explorer below the library show.
    tree_scroll: f32,
    /// How far the Mixer page of the preferences is scrolled.
    page_scroll: f32,
    /// The maintenance of the collection is open, and asks before removing.
    maintenance: (bool, bool),
    /// The dialog of the details of the selected track is open.
    details: bool,
    /// Controls the state draws at least, so a state that failed to draw is caught: more than
    /// five for the window or the preferences, the keys of a small dialog over them.
    min_controls: usize,
}

/// What the two decks the window shows hold, with decks C and D as they start.
fn shown<T: Default>([a, b]: [T; 2]) -> [T; DeckId::COUNT] {
    [a, b, T::default(), T::default()]
}

/// The four states of the reference screens, plus one with everything that only shows
/// when something went wrong: a clipped master and a deck that could not read its file.
fn states() -> Vec<State> {
    let state = |name, scene, page| State {
        name,
        scene,
        page,
        notices: [None; 2],
        // Deck A, whose grid is not locked, shows the GRID page and deck B the MOVE page,
        // so both are operated.
        pages: [DeckPage::Grid, DeckPage::Move],
        mapping: [false; 2],
        menu: None,
        naming: None,
        tree_scroll: 0.0,
        page_scroll: 0.0,
        maintenance: (false, false),
        details: false,
        min_controls: 6,
    };
    let mut trouble = sample::playing();
    trouble.engine.master_levels = [1.4, 0.2];
    trouble.engine.decks[0].levels = [1.2, 1.2];
    let mut trouble = state("trouble", trouble, None);
    trouble.notices[1] = Some(DeckNotice::LoadFailed);
    // The CUE page on both decks, and the hotcues of deck B waiting for the selected cue.
    let mut cues = state("cues", sample::playing(), None);
    cues.pages = [DeckPage::Cue; 2];
    cues.mapping = [false, true];
    // The clock is the tempo master, chosen by hand, with Quantize on: the keys of the
    // clock and MASTER can be operated.
    let mut clock = sample::playing();
    clock.engine.clock.auto = false;
    clock.engine.clock.master = None;
    clock.engine.clock.quantize = true;
    let clock = state("clock", clock, None);
    // The menus of a playlist of the tree and of a row of the playlist shown.
    let mut playlist_menu = state("playlist-menu", sample::playlist(), None);
    playlist_menu.menu = Some(BrowserMenu {
        target: MenuTarget::Playlist(NodeId::new(1)),
        at: egui::pos2(120.0, 760.0),
    });
    let mut entry_menu = state("entry-menu", sample::playlist(), None);
    // Opened right of the mixer: with the entries of the covers it is tall enough to rise
    // over the controls of the mixer, which another test counts.
    entry_menu.menu = Some(BrowserMenu {
        target: MenuTarget::Entry,
        at: egui::pos2(1000.0, 760.0),
    });
    let mut move_menu = state("move-menu", sample::playing(), None);
    move_menu.menu = Some(BrowserMenu {
        target: MenuTarget::MoveTo(NodeId::new(1)),
        at: egui::pos2(120.0, 760.0),
    });
    // The field that names a new playlist, and the one that renames *Friday warm-up*.
    let mut new_playlist = state("new-playlist", sample::playing(), None);
    new_playlist.naming = Some(Naming::New {
        folder: None,
        item: NewItem::Playlist,
    });
    // The question before the selected track of a playlist is deleted.
    let mut delete_menu = state("delete-menu", sample::playlist(), None);
    delete_menu.menu = Some(BrowserMenu {
        target: MenuTarget::ConfirmDelete,
        at: egui::pos2(600.0, 760.0),
    });
    // The menu of a favourite that holds a playlist.
    let mut favourite_menu = state("favourite-menu", sample::playing(), None);
    favourite_menu.menu = Some(BrowserMenu {
        target: MenuTarget::Favourite(4),
        at: egui::pos2(120.0, 660.0),
    });
    // The tree scrolled past the playlists, to History and its earlier sessions.
    let mut sessions = state("sessions", sample::playing(), None);
    sessions.tree_scroll = 6.0 * 27.0;
    // The maintenance of the collection over the Library page, and its question.
    let mut maintenance = state(
        "maintenance",
        sample::playing(),
        Some(SettingsPage::Library),
    );
    maintenance.maintenance = (true, false);
    // Close, Remove missing, Relocate and Check consistency.
    maintenance.min_controls = 4;
    let mut remove_asked = state(
        "remove-asked",
        sample::playing(),
        Some(SettingsPage::Library),
    );
    remove_asked.maintenance = (true, true);
    // Close, Keep them and Remove.
    remove_asked.min_controls = 3;
    // The details of the selected track of a playlist.
    let mut details = state("details", sample::playlist(), None);
    details.details = true;
    // The details of two selected tracks: the tick keys and Previous, the position and Next.
    let mut two = sample::playlist();
    two.browser.marked = vec![1, 2];
    two.browser.selected = Some(1);
    let mut details_several = state("details-several", two, None);
    details_several.details = true;
    // The colours and the ratings the selected track of a playlist can take.
    let mut colour_menu = state("colour-menu", sample::playlist(), None);
    colour_menu.menu = Some(BrowserMenu {
        target: MenuTarget::Colour,
        at: egui::pos2(600.0, 640.0),
    });
    // The pictures of the folder of the explorer, for the cover of the selected track.
    let mut pictures_menu = state("pictures-menu", sample::playlist(), None);
    pictures_menu.menu = Some(BrowserMenu {
        target: MenuTarget::Pictures,
        at: egui::pos2(600.0, 640.0),
    });
    let mut rating_menu = state("rating-menu", sample::playlist(), None);
    rating_menu.menu = Some(BrowserMenu {
        target: MenuTarget::Rating,
        at: egui::pos2(600.0, 640.0),
    });
    // The menu of the header of the list, which shows and hides the columns.
    let mut columns_menu = state("columns-menu", sample::playing(), None);
    columns_menu.menu = Some(BrowserMenu {
        target: MenuTarget::Columns,
        at: egui::pos2(600.0, 560.0),
    });
    // The menu of an earlier history, and the question before it is deleted.
    let mut session_menu = state("session-menu", sample::playing(), None);
    session_menu.menu = Some(BrowserMenu {
        target: MenuTarget::Session(SessionId::new(7)),
        at: egui::pos2(120.0, 660.0),
    });
    let mut confirm_session = state("confirm-session", sample::playing(), None);
    confirm_session.menu = Some(BrowserMenu {
        target: MenuTarget::ConfirmSessionDelete(SessionId::new(7)),
        at: egui::pos2(120.0, 660.0),
    });
    // An import runs: its key stops it.
    let mut importing = state("importing", sample::playing(), None);
    if let Some(collection) = importing.scene.browser.collection.as_mut() {
        collection.importing = true;
    }
    // The menu of the heading Playlists, which imports playlist files.
    let mut playlists_menu = state("playlists-menu", sample::playing(), None);
    playlists_menu.menu = Some(BrowserMenu {
        target: MenuTarget::Playlists,
        at: egui::pos2(120.0, 760.0),
    });
    // The tree scrolled to its end, where the folders of the explorer are.
    let mut explorer = state("explorer", sample::playing(), None);
    explorer.tree_scroll = 10_000.0;
    // The preview player holds a track other than the selected one: its key loads the
    // selected one.
    let mut other_preview = state("preview-other", sample::playing(), None);
    other_preview.scene.engine.preview.track = other_preview.scene.engine.decks[0].track.clone();
    let mut rename = state("rename-playlist", sample::playing(), None);
    rename.naming = Some(Naming::Rename(NodeId::new(1)));
    vec![
        state("main", sample::playing(), None),
        state("startup", sample::startup(), None),
        state(
            "preferences-audio",
            sample::playing(),
            Some(SettingsPage::Audio),
        ),
        state(
            "preferences-general",
            sample::playing(),
            Some(SettingsPage::General),
        ),
        trouble,
        cues,
        clock,
        playlist_menu,
        entry_menu,
        new_playlist,
        rename,
        move_menu,
        state("history", sample::history(), None),
        explorer,
        favourite_menu,
        sessions,
        delete_menu,
        maintenance,
        remove_asked,
        playlists_menu,
        colour_menu,
        pictures_menu,
        rating_menu,
        columns_menu,
        session_menu,
        confirm_session,
        importing,
        details,
        details_several,
        state("four-decks", sample::four_decks(), None),
        state("effects-single", single_unit(), None),
        state("four-units", sample::four_units(), None),
        state(
            "preferences-library",
            sample::playing(),
            Some(SettingsPage::Library),
        ),
        state(
            "preferences-decks",
            sample::playing(),
            Some(SettingsPage::Decks),
        ),
        other_preview,
        state(
            "preferences-controllers",
            sample::playing(),
            Some(SettingsPage::Controllers),
        ),
        state(
            "preferences-keyboard",
            sample::playing(),
            Some(SettingsPage::Keyboard),
        ),
        state(
            "preferences-mixer",
            sample::playing(),
            Some(SettingsPage::Mixer),
        ),
        State {
            page_scroll: f32::MAX,
            ..state(
                "preferences-mixer-end",
                sample::playing(),
                Some(SettingsPage::Mixer),
            )
        },
    ]
}

/// An interface drawn frame by frame into nothing.
struct Stage {
    ctx: egui::Context,
    shell: Shell,
    state: State,
    time: f64,
    /// Modifier keys held down during the frames.
    modifiers: Modifiers,
    /// Files dropped onto the window in the next frame.
    dropped: Vec<egui::DroppedFileHandle>,
}

impl Stage {
    fn new(state: &State) -> Self {
        Self::with_texts(state, Texts::load(Language::English).unwrap())
    }

    fn with_texts(state: &State, texts: Texts) -> Self {
        let mut shell = Shell::new(Language::English).unwrap();
        shell.set_texts(texts);
        shell.state.settings_page = state.page;
        shell.state.deck_page = shown(state.pages);
        shell.state.cue_map = shown(state.mapping);
        shell.state.browser.menu = state.menu;
        shell.state.browser.naming = state.naming;
        shell.state.browser.tree_scroll = state.tree_scroll;
        shell.state.settings.page_scroll = state.page_scroll;
        (shell.state.maintenance, shell.state.remove_asked) = state.maintenance;
        shell.state.details_wanted = state.details;
        let ctx = egui::Context::default();
        shell.install(&ctx);
        let mut stage = Self {
            ctx,
            shell,
            state: state.clone(),
            time: 0.0,
            modifiers: Modifiers::NONE,
            dropped: Vec::new(),
        };
        // Two frames, so everything that takes a frame to settle has settled.
        stage.frame(Vec::new());
        stage.frame(Vec::new());
        stage
    }

    /// Draws one frame with `events` and returns what it produced.
    fn frame(&mut self, mut events: Vec<Event>) -> (Vec<Action>, Vec<ClippedShape>) {
        self.time += 1.0 / 60.0;
        if self.modifiers != Modifiers::NONE {
            events.insert(0, Event::ModifiersChanged(self.modifiers));
        }
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(DESIGN_WIDTH, DESIGN_HEIGHT),
            )),
            time: Some(self.time),
            events,
            focused: true,
            dropped_files: std::mem::take(&mut self.dropped),
            ..RawInput::default()
        };
        let mut actions = Vec::new();
        let mut view = self.state.scene.view();
        view.notices = shown(self.state.notices);
        let shell = &mut self.shell;
        let mut output = self.ctx.run_ui(input, |ui| {
            actions = shell.show(ui, &view);
        });
        // No renderer takes the font atlas here; egui wants to be told that is intended.
        output.textures_delta.clear();
        (actions, std::mem::take(&mut output.shapes))
    }

    fn controls(&self) -> Vec<Control> {
        self.shell.controls().to_vec()
    }

    fn pointer(&mut self, pos: Pos2, pressed: Option<bool>) -> Vec<Action> {
        self.pointer_with(pos, pressed, PointerButton::Primary)
    }

    fn pointer_with(
        &mut self,
        pos: Pos2,
        pressed: Option<bool>,
        button: PointerButton,
    ) -> Vec<Action> {
        let mut events = vec![Event::PointerMoved(pos)];
        if let Some(pressed) = pressed {
            events.push(Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        self.frame(events).0
    }

    fn click(&mut self, pos: Pos2) -> Vec<Action> {
        self.click_with(pos, PointerButton::Primary)
    }

    fn click_with(&mut self, pos: Pos2, button: PointerButton) -> Vec<Action> {
        let mut actions = self.pointer_with(pos, None, button);
        actions.extend(self.pointer_with(pos, Some(true), button));
        actions.extend(self.pointer_with(pos, Some(false), button));
        actions.extend(self.frame(Vec::new()).0);
        actions
    }

    fn key(&mut self, key: egui::Key, modifiers: Modifiers) -> Vec<Action> {
        let event = |pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        let mut actions = self.frame(vec![event(true)]).0;
        actions.extend(self.frame(vec![event(false)]).0);
        actions
    }
}

fn ids(actions: &[Action]) -> Vec<&'static str> {
    actions.iter().map(Action::id).collect()
}

/// Operates one control with the pointer the way its kind is operated and returns the
/// commands that came out.
fn operate(stage: &mut Stage, control: &Control) -> Vec<Action> {
    let centre = control.rect.center();
    match control.kind {
        // The list picks the row under the pointer, and a click on the row already selected
        // changes nothing: the first row is the track of deck A, which is never the selection
        // of the sample states.
        ControlKind::Area if control.command == "browser.select" => {
            stage.click(control.rect.left_top() + egui::vec2(control.rect.width() / 2.0, 6.0))
        }
        ControlKind::Button | ControlKind::Area => stage.click(centre),
        ControlKind::Hold => {
            let mut actions = stage.pointer(centre, None);
            let pressed = stage.pointer(centre, Some(true));
            assert_eq!(
                pressed.len(),
                1,
                "{} sends one command when pressed",
                control.command
            );
            actions.extend(pressed);
            // Held for a few frames: nothing more is sent until the release.
            assert!(stage.pointer(centre, None).is_empty());
            let released = stage.pointer(centre, Some(false));
            assert_eq!(
                released.len(),
                1,
                "{} sends one command when released",
                control.command
            );
            actions.extend(released);
            actions
        }
        ControlKind::Value => {
            let mut actions = stage.pointer(centre, None);
            actions.extend(stage.pointer(centre, Some(true)));
            for step in 1..=4 {
                let moved = centre + egui::vec2(6.0, -6.0) * step as f32;
                actions.extend(stage.pointer(moved, None));
            }
            actions.extend(stage.pointer(centre + egui::vec2(24.0, -24.0), Some(false)));
            actions
        }
        ControlKind::Choice => {
            let mut actions = stage.click(centre);
            assert!(actions.is_empty(), "opening a list sends nothing");
            // The rows of the list hang below the field; one of the first two is not the
            // option already chosen.
            for row in [0.0, 1.0] {
                let pos = control.rect.center_bottom() + egui::vec2(0.0, 2.0 + 14.0 + 28.0 * row);
                actions.extend(stage.click(pos));
                if !actions.is_empty() {
                    break;
                }
                // Picking the current option closed the list: open it again.
                stage.click(centre);
            }
            actions
        }
        ControlKind::Keys => Vec::new(),
    }
}

#[test]
fn every_control_emits_its_command() {
    let states = states();
    let mut jobs = Vec::new();
    for state in &states {
        let controls = Stage::new(state).controls();
        assert!(
            controls.len() >= state.min_controls,
            "{}: {} controls",
            state.name,
            controls.len()
        );
        jobs.extend(
            controls
                .into_iter()
                .filter(|control| control.enabled && control.kind != ControlKind::Keys)
                .map(|control| (state, control)),
        );
    }
    // A fresh stage for each control, so none sees what another one left behind. Each stage
    // builds its own window and fonts, so the controls are shared out among the cores; the
    // checks are the same as one after another.
    let workers = std::thread::available_parallelism().map_or(1, usize::from);
    std::thread::scope(|scope| {
        for worker in 0..workers {
            let jobs = &jobs;
            scope.spawn(move || {
                for (state, control) in jobs.iter().skip(worker).step_by(workers) {
                    let mut stage = Stage::new(state);
                    let actions = operate(&mut stage, control);
                    assert!(
                        ids(&actions).contains(&control.command),
                        "{}: the control at {:?} should emit {} and emitted {:?}",
                        state.name,
                        control.rect,
                        control.command,
                        ids(&actions),
                    );
                }
            });
        }
    });
    assert!(jobs.len() > 150, "{} controls operated", jobs.len());
}

#[test]
fn the_controls_of_a_deck_do_not_overlap_in_either_language() {
    for language in Language::ALL {
        for pages in [
            [DeckPage::Grid, DeckPage::Move],
            [DeckPage::Move, DeckPage::Grid],
            [DeckPage::Cue, DeckPage::Cue],
        ] {
            // The states with the preferences or the details open have the decks behind the
            // dialog.
            for state in states()
                .into_iter()
                .filter(|state| state.page.is_none() && !state.details)
            {
                let mut stage = Stage::with_texts(&state, Texts::load(language).unwrap());
                stage.shell.state.deck_page = shown(pages);
                stage.frame(Vec::new());
                stage.frame(Vec::new());
                let controls: Vec<Control> = stage
                    .controls()
                    .into_iter()
                    .filter(|control| control.command.starts_with("deck."))
                    .collect();
                assert!(controls.len() > 20, "{}", controls.len());
                for (index, first) in controls.iter().enumerate() {
                    for second in &controls[index + 1..] {
                        let shared = first.rect.intersect(second.rect);
                        assert!(
                            shared.width() < 0.5 || shared.height() < 0.5,
                            "{}, {language:?}, {pages:?}: {} at {:?} overlaps {} at {:?}",
                            state.name,
                            first.command,
                            first.rect,
                            second.command,
                            second.rect,
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn the_controls_of_the_header_do_not_overlap_in_either_language() {
    for language in Language::ALL {
        for state in states()
            .into_iter()
            .filter(|state| state.page.is_none() && !state.details)
        {
            let stage = Stage::with_texts(&state, Texts::load(language).unwrap());
            let controls: Vec<Control> = stage
                .controls()
                .into_iter()
                .filter(|control| control.rect.bottom() <= 40.0)
                .collect();
            assert!(controls.len() > 10, "{}", controls.len());
            for (index, first) in controls.iter().enumerate() {
                assert!(first.rect.left() >= 0.0 && first.rect.right() <= DESIGN_WIDTH);
                for second in &controls[index + 1..] {
                    let shared = first.rect.intersect(second.rect);
                    assert!(
                        shared.width() < 0.5 || shared.height() < 0.5,
                        "{}, {language:?}: {} at {:?} overlaps {} at {:?}",
                        state.name,
                        first.command,
                        first.rect,
                        second.command,
                        second.rect,
                    );
                }
            }
        }
    }
}

/// The mixer packs two channels and the master column into the space between the decks;
/// no control may cover another or reach into a deck.
#[test]
fn the_controls_of_the_mixer_do_not_overlap_and_stay_between_the_decks() {
    for language in Language::ALL {
        for state in states()
            .into_iter()
            .filter(|state| state.page.is_none() && !state.details)
        {
            let mut stage = Stage::with_texts(&state, Texts::load(language).unwrap());
            stage.frame(Vec::new());
            let controls: Vec<Control> = stage
                .controls()
                .into_iter()
                .filter(|control| {
                    control.command.starts_with("mixer.") || control.command.starts_with("master.")
                })
                .collect();
            assert!(
                controls.len() > 25,
                "{}, {language:?}: {}",
                state.name,
                controls.len()
            );
            // The mixer sits between the decks, which are 520 wide in a 1440 window; its top
            // moves with the notice band, so only the sides are checked.
            let panel = Rect::from_x_y_ranges(522.0..=918.0, 0.0..=DESIGN_HEIGHT);
            for (index, first) in controls.iter().enumerate() {
                assert!(
                    panel.contains_rect(first.rect),
                    "{}, {language:?}: {} at {:?} reaches into a deck",
                    state.name,
                    first.command,
                    first.rect,
                );
                for second in &controls[index + 1..] {
                    let shared = first.rect.intersect(second.rect);
                    assert!(
                        shared.width() < 0.5 || shared.height() < 0.5,
                        "{}, {language:?}: {} at {:?} overlaps {} at {:?}",
                        state.name,
                        first.command,
                        first.rect,
                        second.command,
                        second.rect,
                    );
                }
            }
        }
    }
}

/// Whether each control that emits `command` is enabled, left to right.
fn enabled(stage: &Stage, command: &str) -> Vec<bool> {
    let mut controls: Vec<Control> = stage
        .controls()
        .into_iter()
        .filter(|control| control.command == command)
        .collect();
    controls.sort_by(|first, second| first.rect.left().total_cmp(&second.rect.left()));
    controls.iter().map(|control| control.enabled).collect()
}

fn state_named(name: &str) -> State {
    states()
        .into_iter()
        .find(|state| state.name == name)
        .unwrap()
}

#[test]
fn the_tempo_keys_of_the_clock_rest_while_a_deck_is_the_tempo_master() {
    // In the main state deck A is the tempo master, chosen automatically.
    let main = Stage::new(&state_named("main"));
    let clock = Stage::new(&state_named("clock"));
    for command in ["clock.tempo.set", "clock.tap"] {
        assert_eq!(enabled(&main, command), [false], "{command}");
        assert_eq!(enabled(&clock, command), [true], "{command}");
    }
    for command in ["clock.tempo.step", "clock.bend"] {
        assert_eq!(enabled(&main, command), [false, false], "{command}");
        assert_eq!(enabled(&clock, command), [true, true], "{command}");
    }
    for stage in [&main, &clock] {
        assert_eq!(enabled(stage, "clock.mode.set"), [true, true]);
        assert_eq!(enabled(stage, "global.snap.toggle"), [true]);
        assert_eq!(enabled(stage, "deck.sync.toggle"), [true, true]);
    }
}

#[test]
fn master_only_shows_the_tempo_master_while_it_is_chosen_automatically() {
    assert_eq!(
        enabled(&Stage::new(&state_named("main")), "deck.master.set"),
        [false, false]
    );
    assert_eq!(
        enabled(&Stage::new(&state_named("clock")), "deck.master.set"),
        [true, true]
    );
    // Deck A of the start screen is empty, so it has no tempo to lead with.
    let mut startup = state_named("startup");
    startup.scene.engine.clock.auto = false;
    assert_eq!(
        enabled(&Stage::new(&startup), "deck.master.set"),
        [false, false]
    );
}

#[test]
fn the_modes_of_the_clock_send_the_mode_they_name() {
    let stage = Stage::new(&state_named("main"));
    let mut modes: Vec<Control> = stage
        .controls()
        .into_iter()
        .filter(|control| control.command == "clock.mode.set")
        .collect();
    modes.sort_by(|first, second| first.rect.left().total_cmp(&second.rect.left()));
    for (control, mode) in modes.iter().zip([ClockMode::Auto, ClockMode::Master]) {
        let actions = Stage::new(&state_named("main")).click(control.rect.center());
        assert!(
            actions.contains(&Action::Engine(Command::ClockModeSet { mode })),
            "{actions:?}"
        );
    }
}

/// The filled rectangles of a frame in `color`.
fn filled(shapes: &[ClippedShape], color: Color32) -> Vec<Rect> {
    fn collect(shape: &Shape, color: Color32, out: &mut Vec<Rect>) {
        match shape {
            Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, color, out)),
            Shape::Rect(rect) if rect.fill == color => out.push(rect.rect),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in shapes {
        collect(&clipped.shape, color, &mut out);
    }
    out
}

#[test]
fn the_phase_meter_grows_from_its_centre_by_the_offset_of_the_beat() {
    let tokens = platter_ui_egui::tokens::Tokens::load().unwrap();
    let deck_b = tokens.all_colors["deck-b"];
    // Behind and ahead by 0.12 of a beat, then on the beat.
    let mut found = Vec::new();
    for phase in [-0.12, 0.12, 0.0] {
        let mut state = state_named("main");
        state.scene.engine.decks[1].phase = Some(phase);
        let mut stage = Stage::new(&state);
        let (_, shapes) = stage.frame(Vec::new());
        let sync = stage
            .controls()
            .into_iter()
            .filter(|control| control.command == "deck.sync.toggle")
            .max_by(|first, second| first.rect.left().total_cmp(&second.rect.left()))
            .unwrap();
        // The bar is the thin one in the colour of the deck on the row of SYNC.
        let bars: Vec<Rect> = filled(&shapes, deck_b)
            .into_iter()
            .filter(|rect| {
                rect.height() < 6.0
                    && rect.center().y > sync.rect.top()
                    && rect.center().y < sync.rect.bottom()
                    && rect.left() > sync.rect.right()
            })
            .collect();
        found.push(bars);
    }
    let [behind, ahead, on] = [&found[0], &found[1], &found[2]];
    assert_eq!(behind.len(), 1, "{behind:?}");
    assert_eq!(ahead.len(), 1, "{ahead:?}");
    assert!(on.is_empty(), "{on:?}");
    let width = 0.12 * 120.0;
    assert!((behind[0].width() - width).abs() < 0.01, "{behind:?}");
    assert!((ahead[0].width() - width).abs() < 0.01, "{ahead:?}");
    // Both start at the centre of the slot: one ends there, the other begins there.
    assert!((behind[0].right() - ahead[0].left()).abs() < 0.01);
}

/// The headphone knobs sit closer than their values are wide: the value of the one under
/// the pointer must show alone on its row and stay inside the master column.
#[test]
fn the_value_of_a_headphone_knob_shows_alone_in_the_master_column() {
    fn texts(shape: &Shape, out: &mut Vec<(String, Rect)>) {
        match shape {
            Shape::Vec(shapes) => shapes.iter().for_each(|shape| texts(shape, out)),
            Shape::Text(text) if !text.galley.job.text.trim().is_empty() => {
                out.push((text.galley.job.text.clone(), text.visual_bounding_rect()));
            }
            _ => {}
        }
    }
    // Right of the divider of the master column, to the edge of the mixer.
    let column = 522.0 + 324.0..=918.0;
    for language in Language::ALL {
        for command in ["mixer.cue.mix.set", "mixer.cue.volume.set"] {
            let state = &states()[0];
            let mut stage = Stage::with_texts(state, Texts::load(language).unwrap());
            stage.frame(Vec::new());
            let knob = stage
                .controls()
                .into_iter()
                .find(|control| control.command == command)
                .unwrap();
            stage.pointer(knob.rect.center(), None);
            let (_, shapes) = stage.frame(Vec::new());
            let mut painted = Vec::new();
            for clipped in &shapes {
                texts(&clipped.shape, &mut painted);
            }
            let row = knob.rect.bottom()..=knob.rect.bottom() + 15.0;
            let on_row: Vec<_> = painted
                .iter()
                .filter(|(_, rect)| {
                    row.contains(&rect.center().y) && column.contains(&rect.center().x)
                })
                .collect();
            assert_eq!(on_row.len(), 1, "{language:?}, {command}: {on_row:?}");
            let (text, rect) = on_row[0];
            assert!(
                rect.left() >= *column.start() && rect.right() <= *column.end(),
                "{language:?}, {command}: {text} at {rect:?} leaves the column"
            );
        }
    }
}

/// The first hotcue of deck A, which holds a cue in the sample scene.
fn first_pad(stage: &Stage) -> Control {
    stage
        .controls()
        .into_iter()
        .find(|control| control.command == "deck.hotcue.trigger")
        .unwrap()
}

#[test]
fn shift_and_a_press_on_a_hotcue_delete_its_cue_and_the_release_sends_nothing() {
    let mut stage = Stage::new(&states()[0]);
    let centre = first_pad(&stage).rect.center();
    stage.modifiers = Modifiers::SHIFT;
    stage.pointer(centre, None);
    let pressed = stage.pointer(centre, Some(true));
    assert_eq!(
        pressed,
        vec![Action::Engine(Command::DeckHotcueDelete {
            deck: DeckId::A,
            slot: HotcueSlot::ALL[0],
        })]
    );
    assert!(stage.pointer(centre, Some(false)).is_empty());
}

#[test]
fn a_hotcue_sends_its_press_and_its_release_with_its_slot() {
    let mut stage = Stage::new(&states()[0]);
    let centre = first_pad(&stage).rect.center();
    let trigger = |pressed| {
        vec![Action::Engine(Command::DeckHotcueTrigger {
            deck: DeckId::A,
            slot: HotcueSlot::ALL[0],
            pressed,
        })]
    };
    stage.pointer(centre, None);
    assert_eq!(stage.pointer(centre, Some(true)), trigger(true));
    assert_eq!(stage.pointer(centre, Some(false)), trigger(false));
}

#[test]
fn a_hotcue_in_the_mapping_mode_takes_the_selected_cue_and_ends_the_mode() {
    let state = states()
        .into_iter()
        .find(|state| state.name == "cues")
        .unwrap();
    let mut stage = Stage::new(&state);
    let pads: Vec<Control> = stage
        .controls()
        .into_iter()
        .filter(|control| control.command == "deck.hotcue.map")
        .collect();
    assert_eq!(pads.len(), 8, "the eight hotcues of deck B");
    let actions = stage.click(pads[6].rect.center());
    assert_eq!(
        actions,
        vec![
            Action::Engine(Command::DeckHotcueMap {
                deck: DeckId::B,
                slot: HotcueSlot::ALL[6],
            }),
            Action::App(AppCommand::DeckCueMapToggle { deck: DeckId::B }),
        ]
    );
}

#[test]
fn the_mapping_mode_ends_when_no_cue_is_selected() {
    let mut state = states()
        .into_iter()
        .find(|state| state.name == "cues")
        .unwrap();
    // The selected cue of deck B is deleted while its hotcues wait for it.
    let cues = &mut state.scene.engine.decks[1].cues;
    let selected = cues.selected().unwrap();
    cues.delete(selected);
    let stage = Stage::new(&state);
    let controls = stage.controls();
    assert!(
        controls
            .iter()
            .all(|control| control.command != "deck.hotcue.map")
    );
    let pads = controls
        .iter()
        .filter(|control| control.command == "deck.hotcue.trigger" && control.enabled)
        .count();
    assert_eq!(pads, 16, "the hotcues of both decks act again");
    assert!(!stage.shell.state.cue_map[1]);
}

#[test]
fn the_type_list_of_a_cue_picks_its_last_option() {
    let state = states()
        .into_iter()
        .find(|state| state.name == "cues")
        .unwrap();
    let mut stage = Stage::new(&state);
    // Deck B: its selected cue is of the type cue, and its grid has a tempo.
    let list = stage
        .controls()
        .into_iter()
        .rfind(|control| control.command == "deck.cue.type.set")
        .unwrap();
    assert!(stage.click(list.rect.center()).is_empty());
    // The fifth row of the list that hangs below the field.
    let row = list.rect.center_bottom() + egui::vec2(0.0, 2.0 + 14.0 + 28.0 * 4.0);
    assert_eq!(
        stage.click(row),
        vec![Action::Engine(Command::DeckCueTypeSet {
            deck: DeckId::B,
            kind: CueKind::Loop,
        })]
    );
}

#[test]
fn a_disabled_control_emits_nothing() {
    let state = &states()[1];
    let mut found = 0;
    for control in Stage::new(state).controls() {
        if control.enabled || control.kind == ControlKind::Keys {
            continue;
        }
        let mut stage = Stage::new(state);
        let actions = stage.click(control.rect.center());
        assert!(
            actions.is_empty(),
            "{} is disabled and emitted {:?}",
            control.command,
            ids(&actions)
        );
        found += 1;
    }
    // The empty deck, the deck that is still reading and the mixer without headphones.
    assert!(found >= 15, "{found} disabled controls");
}

#[test]
fn the_controls_carry_the_values_their_position_stands_for() {
    let state = &states()[0];
    let mut stage = Stage::new(state);
    let controls = stage.controls();
    let find = |command: &str, index: usize| {
        controls
            .iter()
            .filter(|control| control.command == command)
            .nth(index)
            .unwrap()
            .clone()
    };
    // The overview seeks to where it is clicked: a quarter of the way into deck A.
    let overview = find("deck.seek", 0);
    let quarter = overview.rect.left_center() + egui::vec2(overview.rect.width() / 4.0, 0.0);
    let actions = stage.click(quarter);
    let Some(Action::Engine(Command::DeckSeek { deck, seconds })) = actions.first() else {
        panic!("{actions:?}");
    };
    assert_eq!(*deck, DeckId::A);
    assert!((seconds - 372.4 / 4.0).abs() < 1.0, "{seconds}");

    // A click at the left end of the crossfader goes all the way to deck A.
    let mut stage = Stage::new(state);
    let crossfader = find("mixer.crossfader.set", 0);
    let actions = stage.click(crossfader.rect.left_center() + egui::vec2(1.0, 0.0));
    assert_eq!(
        actions.last(),
        Some(&Action::Engine(Command::MixerCrossfaderSet {
            position: 0.0
        }))
    );

    // The file list selects the row under the pointer; the third row is already selected.
    let mut stage = Stage::new(state);
    let list = find("browser.select", 0);
    let fifth = list.rect.left_top() + egui::vec2(200.0, 27.0 * 4.5);
    assert_eq!(
        stage.click(fifth),
        [Action::App(AppCommand::BrowserSelect { index: 4 })]
    );
    let third = list.rect.left_top() + egui::vec2(200.0, 27.0 * 2.5);
    assert!(Stage::new(state).click(third).is_empty());
}

/// A click on a knob gives it the keyboard: the keys that adjust it work right after.
#[test]
fn a_click_on_a_knob_gives_it_the_keyboard() {
    let state = &states()[0];
    let mut stage = Stage::new(state);
    let knob = stage
        .controls()
        .into_iter()
        .find(|control| control.command == "mixer.gain.set")
        .unwrap();
    stage.click(knob.rect.center());
    let actions = stage.key(egui::Key::ArrowUp, Modifiers::NONE);
    assert!(
        ids(&actions).contains(&"mixer.gain.set"),
        "an arrow after a click should move the knob, got {:?}",
        ids(&actions)
    );
}

/// A drag is only recognised once the pointer has moved, so a fast drag is first seen far
/// from where it started; it must still carry the row it was pressed on.
#[test]
fn a_fast_drag_loads_the_row_it_started_on() {
    let state = &states()[0];
    let mut stage = Stage::new(state);
    let controls = stage.controls();
    let list = controls
        .iter()
        .find(|control| control.command == "browser.select")
        .unwrap();
    let overview_b = controls
        .iter()
        .filter(|control| control.command == "deck.seek")
        .nth(1)
        .unwrap();
    let fifth = list.rect.left_top() + egui::vec2(200.0, 27.0 * 4.5);
    let target = overview_b.rect.center();
    stage.pointer(fifth, None);
    let mut actions = stage.pointer(fifth, Some(true));
    actions.extend(stage.pointer(target, None));
    actions.extend(stage.pointer(target, None));
    actions.extend(stage.pointer(target, Some(false)));
    let expected = state.scene.view().browser.rows[4].path.clone();
    assert!(
        actions.contains(&Action::Engine(Command::DeckLoad {
            deck: DeckId::B,
            path: expected.clone(),
        })),
        "expected {expected:?} in deck B, got {actions:?}"
    );
}

#[test]
fn the_focused_control_answers_to_its_keys_and_shortcuts_stay_out_of_its_way() {
    let state = &states()[0];
    let mut stage = Stage::new(state);
    let list = stage
        .controls()
        .into_iter()
        .find(|control| control.command == "browser.select")
        .unwrap();
    // A click gives the list the keyboard.
    stage.click(list.rect.left_top() + egui::vec2(200.0, 27.0 * 2.5));
    assert_eq!(
        stage.key(egui::Key::ArrowDown, Modifiers::NONE),
        [Action::App(AppCommand::BrowserMove { delta: 1 })]
    );
    assert_eq!(
        stage.key(egui::Key::ArrowUp, Modifiers::NONE),
        [Action::App(AppCommand::BrowserMove { delta: -1 })]
    );
    assert_eq!(
        stage.key(egui::Key::Enter, Modifiers::NONE),
        [Action::App(AppCommand::BrowserActivate)]
    );
    assert_eq!(
        stage.key(egui::Key::Backspace, Modifiers::NONE),
        [Action::App(AppCommand::BrowserUp)]
    );
    // Keys the list does not use still trigger their shortcuts.
    assert_eq!(
        stage.key(egui::Key::ArrowLeft, Modifiers::SHIFT),
        [Action::App(AppCommand::BrowserLoadSelected {
            deck: DeckId::A
        })]
    );
    assert_eq!(
        stage.key(egui::Key::Z, Modifiers::NONE),
        [Action::Engine(Command::DeckPlayPause { deck: DeckId::A })]
    );
    // A held shortcut sends its release.
    assert_eq!(
        stage.key(egui::Key::X, Modifiers::NONE),
        [
            Action::Engine(Command::DeckCue {
                deck: DeckId::A,
                pressed: true
            }),
            Action::Engine(Command::DeckCue {
                deck: DeckId::A,
                pressed: false
            }),
        ]
    );
}

#[test]
fn the_preferences_take_the_keyboard_from_the_decks_and_close_with_escape() {
    let state = &states()[2];
    let mut stage = Stage::new(state);
    // With the dialog open the shortcuts of the decks rest.
    assert!(stage.key(egui::Key::Z, Modifiers::NONE).is_empty());
    assert_eq!(
        stage.key(egui::Key::Escape, Modifiers::NONE),
        [Action::App(AppCommand::SettingsClose)]
    );
    // And nothing behind it can be operated: a click where deck A has its play key.
    let play = Stage::new(&states()[0])
        .controls()
        .into_iter()
        .find(|control| control.command == "deck.play_pause")
        .unwrap();
    assert!(stage.click(play.rect.center()).is_empty());
}

/// Every text painted in a frame.
fn texts_of(shapes: &[ClippedShape]) -> Vec<String> {
    fn collect(shape: &Shape, out: &mut Vec<String>) {
        match shape {
            Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, out)),
            // A text of no characters shows nothing.
            Shape::Text(text) if !text.galley.job.text.is_empty() => {
                out.push(text.galley.job.text.clone());
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in shapes {
        collect(&clipped.shape, &mut out);
    }
    out
}

/// What a frame may show that is not a translated text: data of the scene.
fn data_of(scene: &Scene) -> BTreeSet<String> {
    let mut data = BTreeSet::new();
    for deck in &scene.engine.decks {
        data.extend(
            deck.cues
                .iter()
                .map(|(_, cue)| cue.name.as_str().to_owned()),
        );
        if let Some(track) = &deck.track {
            let tags = &track.tags;
            data.extend(
                [tags.title.clone(), tags.artist.clone(), tags.album.clone()]
                    .into_iter()
                    .flatten(),
            );
        }
    }
    for row in &scene.browser.rows {
        data.insert(row.name.clone());
        // The folder of a file, as the details of a track name it.
        if let Some(folder) = row.path.parent() {
            data.insert(folder.to_string_lossy().into_owned());
        }
        if let platter_ui_egui::model::RowKind::Track(track) = &row.kind {
            data.extend(
                [
                    track.title.clone(),
                    track.artist.clone(),
                    track.album.clone(),
                ]
                .into_iter()
                .flatten(),
            );
        }
    }
    data.extend(scene.browser.tree.iter().map(|row| row.name.clone()));
    // The pictures a cover can be imported from, by their names.
    data.extend(
        scene
            .browser
            .pictures
            .iter()
            .filter_map(|path| Some(path.file_name()?.to_string_lossy().into_owned())),
    );
    // The controllers of the settings: their inputs, mapping files and conditions.
    let manager = &scene.settings.controllers;
    data.extend(manager.inputs.iter().cloned());
    data.extend(
        manager
            .importable
            .iter()
            .filter_map(|path| Some(path.file_name()?.to_string_lossy().into_owned())),
    );
    for controller in &manager.controllers {
        data.insert(controller.input.clone());
        data.insert(controller.mapping.clone());
        for binding in &controller.bindings {
            let when: Vec<String> = binding
                .when
                .iter()
                .map(|(modifier, value)| format!("M{modifier} = {value}"))
                .collect();
            data.insert(when.join(", "));
        }
    }
    if let Some(collection) = &scene.browser.collection {
        data.extend(collection.playlists.iter().map(|row| row.name.clone()));
        data.extend(
            collection
                .session
                .iter()
                .chain(&collection.sessions)
                .map(|row| row.label.clone()),
        );
        data.extend(collection.missing.iter().cloned());
    }
    data.extend(scene.browser.crumbs.iter().cloned());
    data.extend(scene.audio.devices.iter().map(|device| device.name.clone()));
    data
}

/// A text made of digits, signs and punctuation alone says the same in every language.
fn is_numeric(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || "+-.:%, ".contains(c))
}

#[test]
fn no_panel_paints_a_text_that_is_not_translated() {
    let marked = Texts::load(Language::English).unwrap().marked(OPEN, CLOSE);
    for state in states() {
        let mut stage = Stage::with_texts(&state, marked.clone());
        let (_, shapes) = stage.frame(Vec::new());
        let data = data_of(&state.scene);
        let painted = texts_of(&shapes);
        assert!(
            painted.len() > 40,
            "{}: {} texts",
            state.name,
            painted.len()
        );
        for text in painted {
            let translated = text.contains(OPEN) && text.contains(CLOSE);
            // The letter of a deck on its badge, and the keys of a shortcut, are data too.
            let letter = text.len() == 1
                && text
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
            let key = egui::Key::from_name(&text).is_some();
            assert!(
                translated || data.contains(&text) || is_numeric(&text) || letter || key,
                "{}: the text {text:?} does not come from the translations",
                state.name
            );
        }
    }
}

#[test]
fn every_text_a_panel_asks_for_exists() {
    // A key without a text shows as the key itself: lowercase words joined by dots.
    let looks_like_a_key = |text: &str| {
        text.contains('.')
            && text
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '.' || c == '_')
            && !text.starts_with('.')
            && !text.ends_with('.')
    };
    for language in Language::ALL {
        for state in states() {
            let mut stage = Stage::with_texts(&state, Texts::load(language).unwrap());
            let (_, shapes) = stage.frame(Vec::new());
            for text in texts_of(&shapes) {
                assert!(
                    !looks_like_a_key(&text),
                    "{}, {:?}: the key {text:?} has no text",
                    state.name,
                    language
                );
            }
        }
    }
}

#[test]
fn every_command_has_a_label_in_both_languages() {
    for language in Language::ALL {
        let texts = Texts::load(language).unwrap();
        for spec in Action::specs() {
            let key = format!("command.{}", spec.id);
            assert_ne!(
                texts.get(&key),
                key,
                "{language:?} has no label for {}",
                spec.id
            );
        }
    }
}

#[test]
fn every_command_of_the_interface_can_be_reached_with_the_keyboard() {
    // Everything any state of the interface can emit, with whether a control that takes the
    // focus emits it.
    let mut by_focus = BTreeSet::new();
    let mut emitted = BTreeSet::new();
    let mut keymap = None;
    for state in states() {
        let stage = Stage::new(&state);
        for control in stage.controls() {
            emitted.insert(control.command);
            if control.focusable {
                by_focus.insert(control.command);
            }
        }
        keymap = Some(state.scene.keymap.clone());
    }
    let keymap = keymap.unwrap();
    for binding in keymap.bindings() {
        emitted.insert(binding.command);
    }
    // The dialog closes with Escape as well as with its key.
    by_focus.insert("app.settings.close");
    for command in &emitted {
        assert!(
            by_focus.contains(command) || keymap.has_command(command),
            "{command} has no shortcut and no control that takes the focus"
        );
    }
    // Every command of the application is one the interface emits somewhere.
    for spec in APP_SPECS {
        assert!(
            emitted.contains(spec.id),
            "nothing in the interface emits {}",
            spec.id
        );
    }
    // And the commands of the engine the interface is built around are all there.
    for command in [
        "deck.play_pause",
        "deck.cue",
        "deck.cup",
        "deck.seek",
        "deck.tempo.set",
        "deck.tempo.range.cycle",
        "deck.nudge",
        "deck.grid.bpm.set",
        "deck.grid.bpm.scale",
        "deck.grid.bpm.step",
        "deck.grid.tap",
        "deck.grid.marker.set",
        "deck.grid.marker.move",
        "deck.grid.marker.delete",
        "deck.grid.marker.previous",
        "deck.grid.marker.next",
        "deck.grid.lock.toggle",
        "deck.loop.toggle",
        "deck.loop.in",
        "deck.loop.out",
        "deck.loop.size.step",
        "deck.beatjump",
        "deck.move",
        "deck.move.mode.set",
        "deck.move.size.step",
        "deck.move.fine.toggle",
        "deck.hotcue.trigger",
        "deck.hotcue.map",
        "deck.cue.store",
        "deck.cue.select",
        "deck.cue.type.set",
        "deck.cue.delete",
        "deck.sync.toggle",
        "deck.master.set",
        "clock.mode.set",
        "clock.tempo.set",
        "clock.tempo.step",
        "clock.tap",
        "clock.bend",
        "global.snap.toggle",
        "global.quantize.toggle",
        "mixer.gain.set",
        "mixer.eq.set",
        "mixer.eq.kill.toggle",
        "mixer.filter.set",
        "mixer.pan.set",
        "mixer.cue.toggle",
        "mixer.cue.mix.set",
        "mixer.cue.volume.set",
        "mixer.fader.set",
        "mixer.crossfader.set",
        "mixer.crossfader.curve.set",
        "master.gain.set",
        "master.test_tone.play",
        "master.limiter.toggle",
    ] {
        assert!(
            emitted.contains(command),
            "nothing in the interface emits {command}"
        );
    }
}

/// Every colour painted in a frame, with what painted it.
fn colours_of(shapes: &[ClippedShape]) -> Vec<(Color32, &'static str)> {
    fn stroke(
        color: Color32,
        width: f32,
        what: &'static str,
        out: &mut Vec<(Color32, &'static str)>,
    ) {
        if width > 0.0 {
            out.push((color, what));
        }
    }
    fn collect(shape: &Shape, out: &mut Vec<(Color32, &'static str)>) {
        match shape {
            Shape::Noop => {}
            Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, out)),
            Shape::Circle(circle) => {
                out.push((circle.fill, "circle fill"));
                stroke(
                    circle.stroke.color,
                    circle.stroke.width,
                    "circle stroke",
                    out,
                );
            }
            Shape::LineSegment { stroke: line, .. } => stroke(line.color, line.width, "line", out),
            Shape::Rect(rect) => {
                out.push((rect.fill, "rectangle fill"));
                stroke(
                    rect.stroke.color,
                    rect.stroke.width,
                    "rectangle stroke",
                    out,
                );
            }
            Shape::Path(path) => {
                out.push((path.fill, "path fill"));
                match &path.stroke.color {
                    ColorMode::Solid(color) => {
                        stroke(*color, path.stroke.width, "path stroke", out)
                    }
                    ColorMode::UV(_) => panic!("a path is stroked with a computed colour"),
                }
            }
            Shape::Text(text) => {
                assert!(text.override_text_color.is_none());
                for section in &text.galley.job.sections {
                    out.push((section.format.color, "text"));
                    out.push((section.format.background, "text background"));
                }
            }
            Shape::Mesh(mesh) => {
                out.extend(mesh.vertices.iter().map(|vertex| (vertex.color, "mesh")));
            }
            other => panic!("a panel painted a shape the colour check does not know: {other:?}"),
        }
    }
    let mut out = Vec::new();
    for clipped in shapes {
        collect(&clipped.shape, &mut out);
    }
    out
}

#[test]
fn no_panel_paints_a_colour_outside_the_design_tokens() {
    let tokens = platter_ui_egui::tokens::Tokens::load().unwrap();
    let palette: BTreeSet<[u8; 3]> = tokens
        .all_colors
        .values()
        .map(|color| [color.r(), color.g(), color.b()])
        .collect();
    let opaque: HashSet<Color32> = tokens
        .all_colors
        .values()
        .copied()
        .filter(|color| color.a() == 255)
        .collect();
    for state in states() {
        let mut stage = Stage::new(&state);
        // The pointer rests on a key, so the hover colour is painted too.
        let hover = stage.controls()[0].rect.center();
        stage.pointer(hover, None);
        let (_, shapes) = stage.frame(Vec::new());
        let colours = colours_of(&shapes);
        assert!(
            colours.len() > 500,
            "{}: {} colours",
            state.name,
            colours.len()
        );
        let mut seen = HashSet::new();
        for (colour, what) in colours {
            if colour == Color32::TRANSPARENT {
                continue;
            }
            seen.insert(colour);
            if colour.a() == 255 {
                assert!(
                    opaque.contains(&colour),
                    "{}: a {what} uses {colour:?}, which is not a design token",
                    state.name
                );
                continue;
            }
            // A veil is a token with some of the background showing through: the colour
            // behind its alpha must be a token.
            let [r, g, b, _] = colour.to_srgba_unmultiplied();
            let close = palette.iter().any(|token| {
                token
                    .iter()
                    .zip([r, g, b])
                    .all(|(token, channel)| token.abs_diff(channel) <= 3)
            });
            assert!(
                close,
                "{}: a {what} uses the translucent {colour:?}, which is not a design token",
                state.name
            );
        }
        // The frame really shows the theme: the background, the panels and the deck hues;
        // where a deck has a loop, the brackets of the loop; and where it has a cue in its
        // first slot, the colour of that slot.
        let looped = state.scene.engine.decks[0].looping.region.is_some();
        let cued = state.scene.engine.decks[0]
            .cues
            .get(HotcueSlot::ALL[0])
            .is_some();
        for token in [
            "hotcue-1",
            "bg-app",
            "surface-panel",
            "text-primary",
            "deck-a",
            "deck-b",
            "loop-region",
        ] {
            if (token == "loop-region" && !looped) || (token == "hotcue-1" && !cued) {
                continue;
            }
            assert!(
                seen.contains(&tokens.all_colors[token]),
                "{}: {token} is not painted",
                state.name
            );
        }
    }
}

#[test]
fn readouts_keep_their_width_while_their_digits_change() {
    // The remaining time of deck A is drawn right-aligned in a fixed block. With digits of
    // equal width the text starts at the same place whatever the time is.
    let left_edge = |position: f64| {
        let mut state = states()[0].clone();
        state.scene.engine.decks[0].position_seconds = position;
        let mut stage = Stage::new(&state);
        let (_, shapes) = stage.frame(Vec::new());
        let mut edges = Vec::new();
        fn collect(shape: &Shape, edges: &mut Vec<(String, f32)>) {
            match shape {
                Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, edges)),
                Shape::Text(text) => edges.push((text.galley.job.text.clone(), text.pos.x)),
                _ => {}
            }
        }
        for clipped in &shapes {
            collect(&clipped.shape, &mut edges);
        }
        // The large time of deck A is the first text of that shape.
        edges
            .into_iter()
            .find(|(text, _)| text.starts_with('-') && text.len() == 8 && text.contains(':'))
            .unwrap()
    };
    let (narrow, first) = left_edge(261.25);
    let (wide, second) = left_edge(293.55);
    assert_eq!((narrow.as_str(), wide.as_str()), ("-01:51.1", "-01:18.8"));
    assert!((first - second).abs() < 0.01, "{first} and {second}");
}

/// A file dropped onto the window, as the platform hands it over.
#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        Ok(Vec::new())
    }
}

/// Deck C holds a track, is the tempo master and clips, and deck D is empty, while the
/// window shows decks A and B: `docs/behavior/decks.md`.
#[test]
fn the_window_draws_with_decks_c_and_d_in_use_and_drops_only_onto_a_shown_deck() {
    let mut scene = sample::playing();
    scene.engine.decks[DeckId::C.index()] = scene.engine.decks[DeckId::A.index()].clone();
    scene.engine.decks[DeckId::C.index()].levels = [1.2, 1.2];
    scene.engine.clock.master = Some(DeckId::C);
    let mut state = state_named("main");
    let shown_only = Stage::new(&state).controls().len();
    state.scene = scene;
    let mut stage = Stage::new(&state);
    assert_eq!(
        stage.controls().len(),
        shown_only,
        "decks C and D add no control to the window"
    );
    stage.frame(vec![Event::PointerGone]);
    // Decks A and B hold tracks and the pointer is over no deck: the drop goes to deck A,
    // not to deck D, empty but not shown.
    stage.dropped = vec![Arc::new(Dropped(PathBuf::from("dropped.wav")))];
    let (actions, _) = stage.frame(Vec::new());
    let loads: Vec<_> = actions
        .iter()
        .filter_map(|action| match action {
            Action::Engine(Command::DeckLoad { deck, .. }) => Some(*deck),
            _ => None,
        })
        .collect();
    assert_eq!(loads, [DeckId::A]);
}

/// The deck each control of `command` acts on, with where it is, found by clicking each one.
fn decks_of(state: &State, command: &str) -> Vec<(DeckId, Rect)> {
    let controls: Vec<Control> = Stage::new(state)
        .controls()
        .into_iter()
        .filter(|control| control.command == command)
        .collect();
    controls
        .iter()
        .map(|control| {
            let mut stage = Stage::new(state);
            let deck = stage
                .click(control.rect.center())
                .iter()
                .find_map(|action| match action {
                    Action::Engine(
                        Command::DeckPlayPause { deck } | Command::MixerFaderSet { deck, .. },
                    ) => Some(*deck),
                    Action::App(AppCommand::BrowserLoadSelected { deck }) => Some(*deck),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{command} at {:?} names no deck", control.rect));
            (deck, control.rect)
        })
        .collect()
}

fn rect_of(found: &[(DeckId, Rect)], deck: DeckId) -> Rect {
    found
        .iter()
        .find(|(of, _)| *of == deck)
        .map(|(_, rect)| *rect)
        .unwrap_or_else(|| panic!("no control of deck {deck:?}"))
}

/// `docs/behavior/interface.md`, *Deck layout*: A over C on the left, B over D on the right,
/// the channels in the order A, B, C, D, and a load key per deck.
#[test]
fn the_four_deck_layout_puts_c_under_a_and_d_under_b() {
    let four = state_named("four-decks");
    let plays = decks_of(&four, "deck.play_pause");
    assert_eq!(plays.len(), 4);
    let [a, b, c, d] = DeckId::ALL.map(|deck| rect_of(&plays, deck));
    assert_eq!(c.left(), a.left());
    assert!(c.top() > a.bottom());
    assert_eq!(d.left(), b.left());
    assert!(d.top() > b.bottom());
    assert!(b.left() > a.right());

    let mut faders = decks_of(&four, "mixer.fader.set");
    faders.sort_by(|first, second| first.1.left().total_cmp(&second.1.left()));
    let order: Vec<DeckId> = faders.iter().map(|(deck, _)| *deck).collect();
    assert_eq!(order, DeckId::ALL);

    let mut loads = decks_of(&four, "browser.load_selected");
    loads.sort_by(|first, second| first.1.left().total_cmp(&second.1.left()));
    let order: Vec<DeckId> = loads.iter().map(|(deck, _)| *deck).collect();
    assert_eq!(order, DeckId::ALL);

    // With two decks the window keeps decks A and B only, with their lanes above them.
    let two = state_named("main");
    let plays: Vec<DeckId> = decks_of(&two, "deck.play_pause")
        .into_iter()
        .map(|(deck, _)| deck)
        .collect();
    assert_eq!(plays, [DeckId::A, DeckId::B]);
    assert_eq!(decks_of(&two, "browser.load_selected").len(), 2);
}

/// A file dropped on deck D loads into it; dropped away from the decks it goes to the first
/// empty deck of the four.
#[test]
fn a_file_dropped_in_the_four_deck_layout_reaches_decks_c_and_d() {
    let four = state_named("four-decks");
    let plays = decks_of(&four, "deck.play_pause");
    let loads = |stage: &mut Stage, events: Vec<Event>| -> Vec<DeckId> {
        stage.dropped = vec![Arc::new(Dropped(PathBuf::from("dropped.wav")))];
        stage
            .frame(events)
            .0
            .iter()
            .filter_map(|action| match action {
                Action::Engine(Command::DeckLoad { deck, .. }) => Some(*deck),
                _ => None,
            })
            .collect()
    };
    let mut stage = Stage::new(&four);
    let over_d = rect_of(&plays, DeckId::D).center();
    assert_eq!(
        loads(&mut stage, vec![Event::PointerMoved(over_d)]),
        [DeckId::D]
    );

    let mut empty_c = four.clone();
    empty_c.scene.engine.decks[DeckId::C.index()] = platter_engine::DeckSnapshot::default();
    let mut stage = Stage::new(&empty_c);
    stage.frame(vec![Event::PointerGone]);
    assert_eq!(loads(&mut stage, Vec::new()), [DeckId::C]);
}

/// The General page of the preferences chooses two or four decks.
#[test]
fn the_general_page_chooses_four_decks() {
    let mut stage = Stage::new(&state_named("preferences-general"));
    let mut keys: Vec<Control> = stage
        .controls()
        .into_iter()
        .filter(|control| control.command == "app.deck_layout.set")
        .collect();
    keys.sort_by(|first, second| first.rect.left().total_cmp(&second.rect.left()));
    assert_eq!(keys.len(), 2);
    assert_eq!(
        stage.click(keys[1].rect.center()),
        [Action::App(AppCommand::DeckLayoutSet {
            layout: DeckLayout::Four
        })]
    );
}

/// The effect row of the four-deck layout holds units 1 and 2, in group mode as the M4 screen
/// draws them and in single mode; no control covers another, in either language.
/// `docs/behavior/effects.md`.
#[test]
fn the_effect_row_shows_both_units_in_either_mode_without_overlaps() {
    for layout in ["four-decks", "main"] {
        effect_row_in(state_named(layout));
    }
}

fn effect_row_in(group: State) {
    let mut single = group.clone();
    let unit = &mut single.scene.engine.effects.units[1];
    unit.values.mode = UnitMode::Single;
    unit.single = Some(EffectKind::Delay);
    unit.values.single.controls = EffectKind::Delay.defaults();
    let group_commands = [
        "fx.unit.mode.set",
        "fx.unit.dry_wet.set",
        "fx.slot.effect.set",
        "fx.slot.amount.set",
        "fx.slot.toggle",
    ];
    let single_commands = [
        "fx.single.effect.set",
        "fx.single.knob.set",
        "fx.single.key.toggle",
        "fx.single.toggle",
        "fx.unit.reset",
    ];
    for (state, commands) in [(group, &group_commands[..]), (single, &single_commands[..])] {
        for language in Language::ALL {
            let stage = Stage::with_texts(&state, Texts::load(language).unwrap());
            let controls: Vec<Control> = stage
                .controls()
                .into_iter()
                .filter(|control| control.rect.top() >= 42.0 && control.rect.bottom() <= 114.0)
                .collect();
            for command in commands {
                assert!(
                    controls.iter().any(|control| control.command == *command),
                    "{language:?}: no {command} in the effect row"
                );
            }
            for (index, first) in controls.iter().enumerate() {
                for second in &controls[index + 1..] {
                    let shared = first.rect.intersect(second.rect);
                    assert!(
                        shared.width() < 0.5 || shared.height() < 0.5,
                        "{language:?}: {} at {:?} overlaps {} at {:?}",
                        first.command,
                        first.rect,
                        second.command,
                        second.rect,
                    );
                }
            }
        }
    }
}

/// The master column of the four-deck mixer has the Limiter key under the level knob and
/// above the headphones, as `docs/design/screens/M4-effects.html` draws it; the two-deck
/// mixer has none (`docs/behavior/master.md`, *Differences*).
#[test]
fn the_four_deck_master_column_has_the_limiter_key() {
    for language in Language::ALL {
        let four = state_named("four-decks");
        let mut stage = Stage::with_texts(&four, Texts::load(language).unwrap());
        stage.frame(Vec::new());
        let controls = stage.controls();
        let find = |command: &str| {
            let found: Vec<&Control> = controls
                .iter()
                .filter(|control| control.command == command)
                .collect();
            assert_eq!(found.len(), 1, "{language:?}: {command}");
            found[0].rect
        };
        let key = find("master.limiter.toggle");
        let level = find("master.gain.set");
        let mix = find("mixer.cue.mix.set");
        assert!(
            key.top() > level.bottom() && key.bottom() < mix.top(),
            "{key:?}"
        );
        assert!((key.center().x - level.center().x).abs() < 0.5, "{key:?}");
        assert_eq!(key.height(), 18.0);
        // Inside the master column, right of its divider.
        let column = Rect::from_x_y_ranges(522.0 + 324.0..=918.0, 0.0..=DESIGN_HEIGHT);
        assert!(column.contains_rect(key), "{language:?}: {key:?}");
    }
    let two = Stage::new(&state_named("main"));
    assert!(
        two.controls()
            .iter()
            .all(|control| control.command != "master.limiter.toggle")
    );
}

/// Each narrow channel sends its deck to units 1 and 2 and turns its mixer effect.
#[test]
fn each_channel_of_the_four_deck_mixer_has_its_effect_controls() {
    let four = state_named("four-decks");
    for command in [
        "mixer.fx_assign.toggle",
        "mixer.effect.set",
        "mixer.effect.select",
    ] {
        let found = Stage::new(&four)
            .controls()
            .into_iter()
            .filter(|control| control.command == command)
            .count();
        let expected = if command == "mixer.fx_assign.toggle" {
            8
        } else {
            4
        };
        assert_eq!(found, expected, "{command}");
    }
    // The two-deck mixer has the assign keys of decks A and B and no mixer effect knob.
    let two = Stage::new(&state_named("main"));
    let count = |command: &str| {
        two.controls()
            .into_iter()
            .filter(|control| control.command == command)
            .count()
    };
    assert_eq!(count("mixer.fx_assign.toggle"), 4);
    assert_eq!(count("mixer.effect.set"), 0);
}

/// In external mixer mode the mixer effect knob and its name collapse with the channel, as
/// the filter does; the assign keys stay, since a unit in insert routing still acts on the
/// deck before its output (`docs/behavior/effects.md`, *Assignment and routing*).
#[test]
fn the_mixer_effect_collapses_with_the_channel_in_external_mode() {
    let mut external = state_named("four-decks");
    external.scene.engine.mixer.mode = MixerMode::External;
    let stage = Stage::new(&external);
    let count = |command: &str| {
        stage
            .controls()
            .into_iter()
            .filter(|control| control.command == command)
            .count()
    };
    assert_eq!(count("mixer.filter.set"), 0);
    assert_eq!(count("mixer.effect.set"), 0);
    assert_eq!(count("mixer.effect.select"), 0);
    assert_eq!(count("mixer.fx_assign.toggle"), 8);
}

/// The four-deck screen with effect unit 2 in single mode on the delay, so the menu of its
/// title can save a snapshot.
fn single_unit() -> Scene {
    let mut scene = sample::four_decks();
    let unit = &mut scene.engine.effects.units[1];
    unit.values.mode = UnitMode::Single;
    unit.single = Some(EffectKind::Delay);
    unit.values.single.controls = EffectKind::Delay.defaults();
    scene
}

/// `docs/behavior/effects.md`, *Units and modes*: the title of a unit in single mode opens a
/// menu whose *Save snapshot* saves the effect of that unit; in group mode it opens nothing.
#[test]
fn the_title_of_a_unit_in_single_mode_saves_a_snapshot_from_its_menu() {
    let single = state_named("effects-single");
    let mut stage = Stage::new(&single);
    let titles: Vec<Control> = stage
        .controls()
        .into_iter()
        .filter(|control| control.command == "fx.unit.snapshot.save")
        .collect();
    assert_eq!(titles.len(), 2);
    let (group, single_title) = if titles[0].rect.left() < titles[1].rect.left() {
        (&titles[0], &titles[1])
    } else {
        (&titles[1], &titles[0])
    };
    assert!(!group.enabled && single_title.enabled);
    let opened = stage.pointer(single_title.rect.center(), None);
    assert!(opened.is_empty());
    for button in [PointerButton::Secondary, PointerButton::Primary] {
        // The right button opens the menu, as the primary one does.
        let mut actions = stage.click_with(single_title.rect.center(), button);
        assert!(actions.is_empty(), "opening the menu sends nothing");
        let item = single_title.rect.left_bottom() + egui::vec2(20.0, 16.0);
        actions.extend(stage.click(item));
        assert_eq!(
            actions,
            [Action::Engine(Command::FxUnitSnapshotSave {
                unit: platter_engine::FxUnit::ALL[1],
            })]
        );
    }
}

/// `docs/behavior/effects.md`, *Units and modes*: with four units each panel switches between
/// its two units and each channel has a key per unit.
#[test]
fn four_units_give_each_panel_a_switch_and_each_channel_four_keys() {
    let four = state_named("four-units");
    let mut stage = Stage::new(&four);
    let count = |stage: &Stage, command: &str| {
        stage
            .controls()
            .into_iter()
            .filter(|control| control.command == command)
            .count()
    };
    assert_eq!(count(&stage, "app.fx_panel.show"), 4);
    assert_eq!(count(&stage, "mixer.fx_assign.toggle"), 16);
    // The key of unit 1 in the left panel brings unit 1 back.
    let mut switches: Vec<Control> = stage
        .controls()
        .into_iter()
        .filter(|control| control.command == "app.fx_panel.show")
        .collect();
    switches.sort_by(|first, second| first.rect.left().total_cmp(&second.rect.left()));
    let actions = stage.click(switches[0].rect.center());
    assert_eq!(
        actions,
        [Action::App(AppCommand::FxPanelShow {
            unit: platter_engine::FxUnit::ALL[0],
        })]
    );
    // With two units, neither the switch nor keys 3 and 4.
    let two = Stage::new(&state_named("four-decks"));
    assert_eq!(count(&two, "app.fx_panel.show"), 0);
    assert_eq!(count(&two, "mixer.fx_assign.toggle"), 8);
}

/// The Mixer page scrolls: the crossfader section sits below its room, under the effect
/// units, and the keyboard brings it into view (`docs/behavior/effects.md`, *Units and modes*).
#[test]
fn the_keyboard_scrolls_the_mixer_page_to_the_crossfader_curve() {
    let mixer = state_named("preferences-mixer");
    let mut stage = Stage::new(&mixer);
    let routing = |stage: &Stage| {
        stage
            .controls()
            .into_iter()
            .filter(|control| control.command == "mixer.crossfader.curve.set")
            .collect::<Vec<Control>>()
    };
    assert!(routing(&stage).is_empty(), "the curve starts out of view");
    let mut found = Vec::new();
    for _ in 0..80 {
        stage.key(egui::Key::Tab, Modifiers::NONE);
        found = routing(&stage);
        if !found.is_empty() {
            break;
        }
    }
    assert!(
        !found.is_empty(),
        "the focus never brought the curve into view"
    );
    // The curve in view takes a click: one of its segments is not the current one.
    let mut actions = Vec::new();
    for control in &found {
        actions.extend(stage.click(control.rect.center()));
    }
    assert!(
        actions.iter().any(|action| matches!(
            action,
            Action::Engine(Command::MixerCrossfaderCurveSet { .. })
        )),
        "{actions:?}"
    );
}
