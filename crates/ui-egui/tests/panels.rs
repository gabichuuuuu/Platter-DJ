//! The panels drawn without a window: one frame of egui at a time, with invented data.
//!
//! Four things are checked on every state of the reference screens: each control emits its
//! command, no text is written in the code, every command can be reached with the keyboard,
//! and no colour comes from outside the design tokens.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::{BTreeSet, HashSet};

use egui::epaint::{ClippedShape, ColorMode, Shape};
use egui::{Color32, Event, Modifiers, PointerButton, Pos2, RawInput, Rect};
use platter_core::{DeckId, HotcueSlot};
use platter_engine::Command;
use platter_ui_egui::command::{APP_SPECS, Action, AppCommand, DeckPage, SettingsPage};
use platter_ui_egui::cx::{Control, ControlKind};
use platter_ui_egui::i18n::{Language, Texts};
use platter_ui_egui::model::DeckNotice;
use platter_ui_egui::sample::{self, Scene};
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};

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
}

impl Stage {
    fn new(state: &State) -> Self {
        Self::with_texts(state, Texts::load(Language::English).unwrap())
    }

    fn with_texts(state: &State, texts: Texts) -> Self {
        let mut shell = Shell::new(Language::English).unwrap();
        shell.set_texts(texts);
        shell.state.settings_page = state.page;
        shell.state.deck_page = state.pages;
        shell.state.cue_map = state.mapping;
        let ctx = egui::Context::default();
        shell.install(&ctx);
        let mut stage = Self {
            ctx,
            shell,
            state: state.clone(),
            time: 0.0,
            modifiers: Modifiers::NONE,
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
            ..RawInput::default()
        };
        let mut actions = Vec::new();
        let mut view = self.state.scene.view();
        view.notices = self.state.notices;
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
        let mut events = vec![Event::PointerMoved(pos)];
        if let Some(pressed) = pressed {
            events.push(Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        self.frame(events).0
    }

    fn click(&mut self, pos: Pos2) -> Vec<Action> {
        let mut actions = self.pointer(pos, None);
        actions.extend(self.pointer(pos, Some(true)));
        actions.extend(self.pointer(pos, Some(false)));
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
    let mut operated = 0;
    for state in states() {
        let controls = Stage::new(&state).controls();
        assert!(
            controls.len() > 5,
            "{}: {} controls",
            state.name,
            controls.len()
        );
        for control in controls {
            if !control.enabled || control.kind == ControlKind::Keys {
                continue;
            }
            // A fresh stage for each control, so none sees what another one left behind.
            let mut stage = Stage::new(&state);
            let actions = operate(&mut stage, &control);
            assert!(
                ids(&actions).contains(&control.command),
                "{}: the control at {:?} should emit {} and emitted {:?}",
                state.name,
                control.rect,
                control.command,
                ids(&actions),
            );
            operated += 1;
        }
    }
    assert!(operated > 150, "{operated} controls operated");
}

#[test]
fn the_controls_of_a_deck_do_not_overlap_in_either_language() {
    for language in Language::ALL {
        for pages in [
            [DeckPage::Grid, DeckPage::Move],
            [DeckPage::Move, DeckPage::Grid],
            [DeckPage::Cue, DeckPage::Cue],
        ] {
            // The states with the preferences open have the decks behind the dialog.
            for state in states().into_iter().filter(|state| state.page.is_none()) {
                let mut stage = Stage::with_texts(&state, Texts::load(language).unwrap());
                stage.shell.state.deck_page = pages;
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
