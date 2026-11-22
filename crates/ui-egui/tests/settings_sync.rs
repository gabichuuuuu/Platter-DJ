//! The Sync page of the preferences, drawn without a window.
//!
//! Behaviour note: `docs/behavior/midi-clock.md`.

#![allow(clippy::unwrap_used)]

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect};
use platter_engine::Command;
use platter_ui_egui::command::{Action, SettingsPage};
use platter_ui_egui::cx::Control;
use platter_ui_egui::i18n::Language;
use platter_ui_egui::sample::{self, Scene};
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};

/// The commands of the controls of the page.
const PAGE: [&str; 6] = [
    "clock.midi.output.set",
    "clock.midi.send.toggle",
    "clock.midi.restart",
    "clock.midi.offset.set",
    "clock.midi.input.set",
    "clock.source.set",
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
        shell.state.settings_page = Some(SettingsPage::Sync);
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
            .filter(|control| control.command == command)
            .cloned()
            .collect()
    }
}

#[test]
fn the_page_has_a_control_for_each_clock_setting() {
    let stage = Stage::new(sample::playing(), Language::English);
    for command in PAGE {
        // Internal and External for the source; one control for each of the others.
        let expected = if command == "clock.source.set" { 2 } else { 1 };
        assert_eq!(stage.controls(command).len(), expected, "{command}");
    }
}

#[test]
fn the_source_of_the_master_clock_is_chosen_on_the_page() {
    let mut stage = Stage::new(sample::playing(), Language::English);
    let external = stage.controls("clock.source.set")[1].rect.center();
    let engine: Vec<Command> = stage
        .click(external)
        .into_iter()
        .filter_map(|action| match action {
            Action::Engine(command) => Some(command),
            Action::App(_) => None,
        })
        .collect();
    assert_eq!(engine, [Command::ClockSourceSet { external: true }]);
}

#[test]
fn the_switch_sends_the_clock() {
    let mut stage = Stage::new(sample::playing(), Language::English);
    let switch = stage.controls("clock.midi.send.toggle")[0].rect.center();
    let engine: Vec<Command> = stage
        .click(switch)
        .into_iter()
        .filter_map(|action| match action {
            Action::Engine(command) => Some(command),
            Action::App(_) => None,
        })
        .collect();
    assert_eq!(engine, [Command::ClockMidiSendToggle]);
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
