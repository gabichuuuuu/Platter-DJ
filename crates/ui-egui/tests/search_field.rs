//! The search field of the browser, drawn without a window.
//!
//! Behaviour note: `docs/behavior/browser.md`, *Search*.

#![allow(clippy::unwrap_used)]

use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect};
use platter_ui_egui::command::{Action, AppCommand};
use platter_ui_egui::cx::ControlKind;
use platter_ui_egui::i18n::Language;
use platter_ui_egui::sample::{self, Scene};
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};

struct Stage {
    ctx: egui::Context,
    shell: Shell,
    scene: Scene,
}

impl Stage {
    fn new() -> Self {
        let shell = Shell::new(Language::English).unwrap();
        let ctx = egui::Context::default();
        shell.install(&ctx);
        let mut stage = Self {
            ctx,
            shell,
            scene: sample::playing(),
        };
        stage.frame(Vec::new());
        stage
    }

    fn frame(&mut self, events: Vec<Event>) -> Vec<Action> {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(DESIGN_WIDTH, DESIGN_HEIGHT),
            )),
            events,
            focused: true,
            ..RawInput::default()
        };
        let view = self.scene.view();
        let shell = &mut self.shell;
        let mut actions = Vec::new();
        let mut output = self.ctx.run_ui(input, |ui| {
            actions = shell.show(ui, &view);
        });
        output.textures_delta.clear();
        actions
    }

    fn field(&self) -> Pos2 {
        self.shell
            .controls()
            .iter()
            .find(|control| {
                control.command == "browser.search.set" && control.kind == ControlKind::Keys
            })
            .unwrap()
            .rect
            .center()
    }

    fn click(&mut self, at: Pos2) {
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        self.frame(vec![Event::PointerMoved(at), button(true)]);
        self.frame(vec![button(false)]);
    }
}

fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn searched(actions: &[Action]) -> Vec<String> {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::App(AppCommand::BrowserSearchSet { text }) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn typing_in_the_field_searches_and_plays_no_shortcut() {
    let mut stage = Stage::new();
    let field = stage.field();
    stage.click(field);
    // Z plays deck A when nothing keeps the keyboard; the field types it instead.
    let actions = stage.frame(vec![
        key(Key::Z, Modifiers::NONE),
        Event::Text("z".to_owned()),
    ]);
    assert_eq!(searched(&actions), ["z"]);
    assert!(
        actions
            .iter()
            .all(|action| matches!(action, Action::App(_))),
        "{actions:?}"
    );
}

#[test]
fn ctrl_f_gives_the_field_the_keyboard_and_escape_takes_it_back() {
    let mut stage = Stage::new();
    let ctrl = Modifiers {
        ctrl: true,
        command: true,
        ..Modifiers::NONE
    };
    let actions = stage.frame(vec![key(Key::F, ctrl)]);
    assert!(actions.contains(&Action::App(AppCommand::BrowserSearchFocus)));
    stage.shell.apply(&AppCommand::BrowserSearchFocus);
    stage.frame(Vec::new());
    let typed = stage.frame(vec![Event::Text("deep".to_owned())]);
    assert_eq!(searched(&typed), ["deep"]);
    // The sample scene keeps its empty text: Escape leaves the field, and Z plays again.
    stage.frame(vec![key(Key::Escape, Modifiers::NONE)]);
    stage.frame(Vec::new());
    let played = stage.frame(vec![key(Key::Z, Modifiers::NONE)]);
    assert!(searched(&played).is_empty());
    assert!(
        played
            .iter()
            .any(|action| matches!(action, Action::Engine(_))),
        "{played:?}"
    );
}
