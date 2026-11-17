//! Typing into a field that is otherwise dragged or shown: the tempo of the clock, the tempo
//! of a grid and the name of a cue. Behaviour notes: `docs/behavior/sync.md`,
//! `docs/behavior/beatgrid.md`, `docs/behavior/cues.md`.

#![allow(clippy::unwrap_used)]

use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect};
use platter_core::DeckId;
use platter_engine::Command;
use platter_ui_egui::command::{Action, AppCommand, DeckPage};
use platter_ui_egui::i18n::Language;
use platter_ui_egui::sample::{self, Scene};
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};

struct Stage {
    ctx: egui::Context,
    shell: Shell,
    scene: Scene,
}

impl Stage {
    fn new(scene: Scene) -> Self {
        let shell = Shell::new(Language::English).unwrap();
        let ctx = egui::Context::default();
        shell.install(&ctx);
        let mut stage = Self { ctx, shell, scene };
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

    fn at(&self, command: &str) -> Pos2 {
        self.shell
            .controls()
            .iter()
            .find(|control| control.command == command)
            .map(|control| control.rect.center())
            .unwrap()
    }

    fn button(&mut self, pos: Pos2, pressed: bool) -> Vec<Action> {
        self.frame(vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            },
        ])
    }

    fn double_click(&mut self, pos: Pos2) -> Vec<Action> {
        let mut actions = Vec::new();
        for pressed in [true, false, true, false] {
            actions.extend(self.button(pos, pressed));
        }
        // The field opens on the frame after the second click.
        actions.extend(self.frame(Vec::new()));
        actions
    }

    /// Types `text` and presses Enter; returns what that sent.
    fn type_and_enter(&mut self, text: &str) -> Vec<Action> {
        let erase: Vec<Event> = (0..20)
            .map(|_| Event::Key {
                key: Key::Backspace,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            })
            .collect();
        let mut actions = self.frame(erase);
        actions.extend(self.frame(vec![Event::Text(text.to_owned())]));
        actions.extend(self.frame(vec![Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]));
        actions
    }
}

fn engine(actions: &[Action]) -> Vec<Command> {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::Engine(command) => Some(command.clone()),
            Action::App(_) => None,
        })
        .collect()
}

#[test]
fn a_double_click_types_the_tempo_of_the_clock() {
    let mut scene = sample::playing();
    // The clock leads: its tempo is its own.
    scene.engine.clock.master = None;
    let mut stage = Stage::new(scene);
    let field = stage.at("clock.tempo.set");
    stage.double_click(field);
    let sent = stage.type_and_enter("128,5");
    assert_eq!(engine(&sent), [Command::ClockTempoSet { bpm: 128.5 }]);
    // A tempo out of range sends nothing.
    stage.double_click(field);
    let sent = stage.type_and_enter("9000");
    assert!(engine(&sent).is_empty(), "{sent:?}");
}

#[test]
fn a_double_click_types_the_tempo_of_a_grid_and_the_name_of_a_cue() {
    let mut stage = Stage::new(sample::playing());
    stage.shell.apply(&AppCommand::DeckPageSet {
        deck: DeckId::A,
        page: DeckPage::Grid,
    });
    stage.frame(Vec::new());
    let field = stage.at("deck.grid.bpm.set");
    stage.double_click(field);
    let sent = stage.type_and_enter("126");
    assert_eq!(
        engine(&sent),
        [Command::DeckGridBpmSet {
            deck: DeckId::A,
            bpm: 126.0
        }]
    );

    // A stage of its own: the clicks of the grid would count with these.
    let mut stage = Stage::new(sample::playing());
    stage.shell.apply(&AppCommand::DeckPageSet {
        deck: DeckId::A,
        page: DeckPage::Cue,
    });
    stage.frame(Vec::new());
    let name = stage.at("deck.cue.rename");
    stage.double_click(name);
    let sent = stage.type_and_enter("Drop");
    assert_eq!(
        engine(&sent),
        [Command::DeckCueRename {
            deck: DeckId::A,
            name: "Drop".to_owned()
        }]
    );
}
