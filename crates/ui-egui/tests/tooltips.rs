//! The tooltips of the keys, drawn without a window.
//!
//! Behaviour note: `docs/behavior/interface.md`, Tooltips.

#![allow(clippy::unwrap_used)]

use egui::epaint::{ClippedShape, Shape};
use egui::{Event, Key, Modifiers, Pos2, RawInput, Rect};
use platter_ui_egui::command::SettingsPage;
use platter_ui_egui::i18n::Language;
use platter_ui_egui::sample::{self, Scene};
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};

struct Stage {
    ctx: egui::Context,
    shell: Shell,
    scene: Scene,
    time: f64,
}

impl Stage {
    fn new(tooltips: bool) -> Self {
        let shell = Shell::new(Language::English).unwrap();
        let ctx = egui::Context::default();
        shell.install(&ctx);
        let mut scene = sample::playing();
        scene.settings.tooltips = tooltips;
        let mut stage = Self {
            ctx,
            shell,
            scene,
            time: 0.0,
        };
        stage.frame(Vec::new(), 0.0);
        stage
    }

    /// Draws one frame `seconds` after the last one and returns every text it painted.
    fn frame(&mut self, events: Vec<Event>, seconds: f64) -> Vec<String> {
        self.time += seconds.max(1.0 / 60.0);
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
        let view = self.scene.view();
        let shell = &mut self.shell;
        let mut output = self.ctx.run_ui(input, |ui| {
            shell.show(ui, &view);
        });
        output.textures_delta.clear();
        texts(&output.shapes)
    }

    /// The centre of the key that opens the preferences, in the status header.
    fn settings_key(&self) -> Pos2 {
        self.shell
            .controls()
            .iter()
            .find(|control| control.command == "app.settings.open")
            .unwrap()
            .rect
            .center()
    }
}

fn texts(shapes: &[ClippedShape]) -> Vec<String> {
    let mut found = Vec::new();
    for clipped in shapes {
        collect(&clipped.shape, &mut found);
    }
    found
}

fn collect(shape: &Shape, found: &mut Vec<String>) {
    match shape {
        Shape::Text(text) => found.push(text.galley.text().to_owned()),
        Shape::Vec(shapes) => {
            for shape in shapes {
                collect(shape, found);
            }
        }
        _ => {}
    }
}

fn has(texts: &[String], text: &str) -> bool {
    texts.iter().any(|painted| painted == text)
}

#[test]
fn a_key_shows_its_name_and_shortcut_after_a_moment_under_the_pointer() {
    let mut stage = Stage::new(true);
    let key = stage.settings_key();
    let at_once = stage.frame(vec![Event::PointerMoved(key)], 0.0);
    assert!(!has(&at_once, "Preferences"), "not at once");
    let soon = stage.frame(vec![Event::PointerMoved(key)], 0.3);
    assert!(!has(&soon, "Preferences"), "not before 600 ms");
    let later = stage.frame(Vec::new(), 0.4);
    assert!(has(&later, "Preferences"), "{later:?}");
    assert!(
        has(&later, "Ctrl + Comma") || has(&later, "Ctrl + ,"),
        "{later:?}"
    );
    // Escape hides it until the pointer leaves.
    let escape = |pressed| Event::Key {
        key: Key::Escape,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    stage.frame(vec![escape(true)], 0.0);
    let hidden = stage.frame(vec![escape(false)], 1.0);
    assert!(!has(&hidden, "Preferences"));
    let gone = stage.frame(vec![Event::PointerMoved(Pos2::new(700.0, 700.0))], 0.0);
    assert!(!has(&gone, "Preferences"));
}

#[test]
fn no_tooltip_shows_while_they_are_off() {
    let mut stage = Stage::new(false);
    let key = stage.settings_key();
    stage.frame(vec![Event::PointerMoved(key)], 0.0);
    let later = stage.frame(Vec::new(), 2.0);
    assert!(!has(&later, "Preferences"));
}

fn count(texts: &[String], text: &str) -> usize {
    texts.iter().filter(|painted| *painted == text).count()
}

#[test]
fn a_fader_and_a_list_show_their_names_too() {
    let mut stage = Stage::new(true);
    let crossfader = stage
        .shell
        .controls()
        .iter()
        .find(|control| control.command == "mixer.crossfader.set")
        .unwrap()
        .rect
        .center();
    let before = stage.frame(vec![Event::PointerMoved(crossfader)], 0.0);
    let later = stage.frame(Vec::new(), 0.7);
    assert_eq!(
        count(&later, "Crossfader"),
        count(&before, "Crossfader") + 1,
        "{later:?}"
    );

    let mut stage = Stage::new(true);
    stage.shell.state.settings_page = Some(SettingsPage::Decks);
    stage.frame(Vec::new(), 0.0);
    let range = stage
        .shell
        .controls()
        .iter()
        .find(|control| control.command == "app.analysis_range.set")
        .unwrap()
        .rect
        .center();
    let before = stage.frame(vec![Event::PointerMoved(range)], 0.0);
    let later = stage.frame(Vec::new(), 0.7);
    assert_eq!(
        count(&later, "Range of tempos"),
        count(&before, "Range of tempos") + 1,
        "{later:?}"
    );
}
