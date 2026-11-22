//! The Input page of the preferences, drawn without a window.
//!
//! Behaviour note: `docs/behavior/live-input.md`.

#![allow(clippy::unwrap_used)]

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect};
use platter_core::DeckId;
use platter_engine::{Command, DeckKind};
use platter_ui_egui::command::{Action, SettingsPage};
use platter_ui_egui::cx::Control;
use platter_ui_egui::i18n::Language;
use platter_ui_egui::sample::{self, Scene};
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};

/// The commands of the controls of the page.
const PAGE: [&str; 5] = [
    "input.device.set",
    "input.routing.set",
    "deck.type.set",
    "mixer.aux.set",
    "recorder.source.set",
];

struct Stage {
    ctx: egui::Context,
    shell: Shell,
    scene: Scene,
    time: f64,
}

impl Stage {
    fn new(scene: Scene, language: Language) -> Self {
        let mut shell = Shell::new(language).unwrap();
        shell.state.settings_page = Some(SettingsPage::Input);
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

    fn controls(&self, command: &str) -> Vec<Control> {
        self.shell
            .controls()
            .iter()
            .filter(|control| PAGE.contains(&control.command) && control.command == command)
            .cloned()
            .collect()
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
fn the_page_has_a_control_for_each_input_setting() {
    let stage = Stage::new(sample::playing(), Language::English);
    assert_eq!(stage.controls("input.device.set").len(), 1);
    // Four decks, the auxiliary input, the recorders and the FX Send.
    assert_eq!(stage.controls("input.routing.set").len(), 7);
    // The mix or the input, for the mix recorder.
    assert_eq!(stage.controls("recorder.source.set").len(), 2);
    // Track and Live input, for each deck.
    assert_eq!(stage.controls("deck.type.set").len(), 8);
    assert_eq!(stage.controls("mixer.aux.set").len(), 1);
}

#[test]
fn the_type_of_a_deck_is_chosen_on_the_page() {
    let mut stage = Stage::new(sample::playing(), Language::English);
    // The second key of deck B: live input.
    let live_b = stage.controls("deck.type.set")[3].rect.center();
    assert_eq!(
        engine(stage.click(live_b)),
        [Command::DeckTypeSet {
            deck: DeckId::B,
            kind: DeckKind::Live
        }]
    );
}

#[test]
fn the_controls_of_the_page_do_not_overlap_in_either_language() {
    for language in [Language::English, Language::Spanish] {
        let stage = Stage::new(sample::playing(), language);
        let controls: Vec<Control> = PAGE
            .iter()
            .flat_map(|command| stage.controls(command))
            .collect();
        for (index, one) in controls.iter().enumerate() {
            for other in &controls[index + 1..] {
                let shared = one.rect.intersect(other.rect);
                assert!(
                    shared.width() <= 0.0 || shared.height() <= 0.0,
                    "{} and {} overlap in {language:?}",
                    one.command,
                    other.command
                );
            }
        }
    }
}
