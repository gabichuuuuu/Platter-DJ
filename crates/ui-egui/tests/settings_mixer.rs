//! The Mixer page of the preferences, drawn without a window.
//!
//! Behaviour note: `docs/behavior/settings.md`, Mixer.

#![allow(clippy::unwrap_used)]

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect};
use platter_engine::{Command, MixerMode};
use platter_ui_egui::command::{Action, SettingsPage};
use platter_ui_egui::cx::Control;
use platter_ui_egui::i18n::Language;
use platter_ui_egui::sample::{self, Scene};
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};

/// The commands of the controls of the page.
const PAGE: [&str; 5] = [
    "mixer.mode.set",
    "mixer.autogain.toggle",
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
        let mut shell = Shell::new(language).unwrap();
        shell.state.settings_page = Some(SettingsPage::Mixer);
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

    fn click_control(&mut self, command: &str, nth: usize) -> Vec<Action> {
        let control = self
            .mixer_controls()
            .into_iter()
            .filter(|control| control.command == command)
            .nth(nth)
            .unwrap();
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
    let stage = Stage::new(sample::playing(), Language::English);
    let count = |command: &str| {
        stage
            .mixer_controls()
            .iter()
            .filter(|control| control.command == command && control.enabled)
            .count()
    };
    assert_eq!(count("mixer.mode.set"), 2);
    assert_eq!(count("mixer.autogain.toggle"), 1);
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
