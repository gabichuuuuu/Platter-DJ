//! The dialog of the rules of a smart playlist, drawn without a window.
//!
//! Behaviour notes: `docs/behavior/browser.md`, *Playlists*, and `docs/behavior/library.md`,
//! *Smart playlists*.

#![allow(clippy::unwrap_used)]

use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect};
use platter_library::{NodeId, RangeField, Rule, TagField};
use platter_ui_egui::command::{Action, AppCommand, RuleField};
use platter_ui_egui::cx::ControlKind;
use platter_ui_egui::i18n::Language;
use platter_ui_egui::model::PlaylistRow;
use platter_ui_egui::sample::{self, Scene};
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};

const SMART: i64 = 5;

struct Stage {
    ctx: egui::Context,
    shell: Shell,
    scene: Scene,
}

impl Stage {
    /// The window with a smart playlist that holds `rules`, and the dialog of its rules open.
    fn open(rules: &str) -> Self {
        let mut scene = sample::playing();
        if let Some(collection) = &mut scene.browser.collection {
            collection.playlists.push(PlaylistRow {
                id: NodeId::new(SMART),
                name: "Peak time".to_owned(),
                depth: 0,
                folder: false,
                smart: true,
                rules: rules.to_owned(),
                entries: Some(37),
                current: false,
                preparation: false,
                favourite: None,
            });
        }
        let shell = Shell::new(Language::English).unwrap();
        let ctx = egui::Context::default();
        shell.install(&ctx);
        let mut stage = Self { ctx, shell, scene };
        stage.send(AppCommand::BrowserSmartRulesOpen {
            smart: NodeId::new(SMART),
        });
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

    /// Applies a command as the application does, then draws a frame.
    fn send(&mut self, command: AppCommand) {
        self.shell.apply(&command);
        self.frame(Vec::new());
    }

    fn rows(&self) -> Vec<(RuleField, String, String, usize)> {
        self.shell
            .state
            .rules
            .as_ref()
            .map(|form| {
                form.rows
                    .iter()
                    .map(|row| (row.field, row.from.clone(), row.to.clone(), row.choice))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn control(&self, command: &str) -> Option<(Pos2, bool)> {
        self.shell
            .controls()
            .iter()
            .find(|control| control.command == command && control.kind == ControlKind::Button)
            .map(|control| (control.rect.center(), control.enabled))
    }

    fn click(&mut self, at: Pos2) -> Vec<Action> {
        let press = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        self.frame(vec![Event::PointerMoved(at), press(true)]);
        self.frame(vec![press(false)])
    }
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

#[test]
fn the_dialog_opens_on_the_rules_kept_and_saves_what_was_chosen() {
    let mut stage = Stage::open("genre techno; tempo 128..; missing no");
    assert_eq!(
        stage.rows(),
        [
            (RuleField::Genre, "techno".to_owned(), String::new(), 0),
            (RuleField::Tempo, "128".to_owned(), String::new(), 0),
            (RuleField::Missing, String::new(), String::new(), 1),
        ]
    );
    stage.send(AppCommand::BrowserSmartRuleRemove { row: 2 });
    stage.send(AppCommand::BrowserSmartRuleAdd);
    stage.send(AppCommand::BrowserSmartRuleField {
        row: 2,
        field: RuleField::Colour,
    });
    stage.send(AppCommand::BrowserSmartRuleChoice { row: 2, value: 3 });

    let (save, enabled) = stage.control("browser.smart.rules.set").unwrap();
    assert!(enabled);
    assert_eq!(
        app(&stage.click(save)),
        [AppCommand::BrowserSmartRulesSet {
            smart: NodeId::new(SMART),
            rules: vec![
                Rule::Words {
                    field: TagField::Genre,
                    words: "techno".into(),
                },
                Rule::Range {
                    field: RangeField::Tempo,
                    low: Some(128.0),
                    high: None,
                },
                Rule::Colour(4),
            ],
        }]
    );
}

#[test]
fn save_waits_for_every_row_to_hold_a_rule_and_escape_closes() {
    let mut stage = Stage::open("");
    assert!(stage.rows().is_empty());
    stage.send(AppCommand::BrowserSmartRuleAdd);
    // A new row asks for words that are not there yet.
    let (_, enabled) = stage.control("browser.smart.rules.set").unwrap();
    assert!(!enabled);
    stage.send(AppCommand::BrowserSmartRuleRemove { row: 0 });
    let (_, enabled) = stage.control("browser.smart.rules.set").unwrap();
    assert!(enabled, "no rule at all is the whole collection");

    let escape = Event::Key {
        key: Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    let actions = stage.frame(vec![escape]);
    assert_eq!(app(&actions), [AppCommand::BrowserSmartRulesClose]);
    for action in &actions {
        if let Action::App(command) = action {
            stage.shell.apply(command);
        }
    }
    assert!(stage.shell.state.rules.is_none());
}

#[test]
fn sixteen_rules_fit_and_take_no_more() {
    let sixteen = vec!["analysed yes"; 16].join("; ");
    let mut stage = Stage::open(&sixteen);
    assert_eq!(stage.rows().len(), 16);
    let (_, enabled) = stage.control("browser.smart.rule.add").unwrap();
    assert!(!enabled);
    stage.send(AppCommand::BrowserSmartRuleAdd);
    assert_eq!(stage.rows().len(), 16);
    // Every row is drawn inside the window, with its key that removes it.
    let removes = stage
        .shell
        .controls()
        .iter()
        .filter(|control| control.command == "browser.smart.rule.remove")
        .count();
    assert_eq!(removes, 16);
}

#[test]
fn the_dialog_closes_when_its_smart_playlist_is_gone() {
    let mut stage = Stage::open("genre house");
    assert_eq!(stage.rows().len(), 1);
    if let Some(collection) = &mut stage.scene.browser.collection {
        collection.playlists.retain(|row| !row.smart);
    }
    stage.frame(Vec::new());
    assert!(stage.shell.state.rules.is_none());
}
