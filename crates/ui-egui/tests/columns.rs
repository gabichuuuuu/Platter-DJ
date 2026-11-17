//! The columns of the list of the browser: the menu of the header shows and hides them, and the
//! headers sort. Behaviour note: `docs/behavior/browser.md`, *Sorting*.

#![allow(clippy::unwrap_used)]

use egui::epaint::{ClippedShape, Shape};
use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect};
use platter_ui_egui::command::{Action, AppCommand, ListColumn, SortColumn};
use platter_ui_egui::files::FileBrowser;
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

    /// Draws a frame; returns what it sent and every text it painted.
    fn frame(&mut self, events: Vec<Event>) -> (Vec<AppCommand>, Vec<String>) {
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
        let mut texts = Vec::new();
        for ClippedShape { shape, .. } in &output.shapes {
            collect(shape, &mut texts);
        }
        let commands = actions
            .into_iter()
            .filter_map(|action| match action {
                Action::App(command) => Some(command),
                Action::Engine(_) => None,
            })
            .collect();
        (commands, texts)
    }

    fn click(&mut self, pos: Pos2, button: PointerButton) -> (Vec<AppCommand>, Vec<String>) {
        let press = |pressed| Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        };
        self.frame(vec![Event::PointerMoved(pos), press(true)]);
        let (sent, _) = self.frame(vec![press(false)]);
        let (more, texts) = self.frame(Vec::new());
        (sent.into_iter().chain(more).collect(), texts)
    }

    /// The headings that sort, from left to right.
    fn headings(&self) -> Vec<Rect> {
        let mut headings: Vec<Rect> = self
            .shell
            .controls()
            .iter()
            .filter(|control| control.command == "browser.sort")
            .map(|control| control.rect)
            .collect();
        headings.sort_by(|a, b| a.left().total_cmp(&b.left()));
        headings
    }
}

fn collect(shape: &Shape, found: &mut Vec<String>) {
    match shape {
        Shape::Text(text) => found.push(text.galley.text().to_owned()),
        Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, found)),
        _ => {}
    }
}

#[test]
fn a_secondary_click_on_the_header_opens_the_menu_of_the_columns() {
    let mut stage = Stage::new(sample::playing());
    // The colour first, then the title.
    let title = stage.headings()[1].center();
    let (sent, texts) = stage.click(title, PointerButton::Secondary);
    assert!(sent.is_empty(), "{sent:?}");
    for entry in [
        "Hide Artist",
        "Hide Album",
        "Show Genre",
        "Show Comment",
        "Hide File",
    ] {
        assert!(texts.iter().any(|text| text == entry), "{entry}: {texts:?}");
    }
    // The menu lists the columns in their order, one row each.
    let rows: Vec<Rect> = stage
        .shell
        .controls()
        .iter()
        .filter(|control| control.command == "browser.column.toggle")
        .map(|control| control.rect)
        .collect();
    assert_eq!(rows.len(), ListColumn::ALL.len());
    let (sent, _) = stage.click(rows[3].center(), PointerButton::Primary);
    assert_eq!(
        sent,
        [AppCommand::BrowserColumnToggle {
            column: ListColumn::Genre
        }]
    );
}

#[test]
fn a_hidden_column_has_no_heading_and_a_shown_one_sorts() {
    let shown = Stage::new(sample::playing()).headings().len();
    let mut scene = sample::playing();
    scene.browser.columns = vec![ListColumn::Colour, ListColumn::Genre, ListColumn::Rating];
    let mut stage = Stage::new(scene);
    let headings = stage.headings();
    // The title, the colour, the genre and the rating.
    assert_eq!(headings.len(), 4, "{headings:?}");
    assert!(headings.len() < shown);
    let mut sorts = Vec::new();
    for heading in headings {
        let (sent, _) = stage.click(heading.center(), PointerButton::Primary);
        sorts.extend(sent);
    }
    assert_eq!(
        sorts,
        [
            AppCommand::BrowserSort {
                column: SortColumn::Colour
            },
            AppCommand::BrowserSort {
                column: SortColumn::Title
            },
            AppCommand::BrowserSort {
                column: SortColumn::Genre
            },
            AppCommand::BrowserSort {
                column: SortColumn::Rating
            },
        ]
    );
}

#[test]
fn toggling_keeps_the_columns_in_their_order() {
    let mut browser = FileBrowser::new(Vec::new());
    assert_eq!(browser.columns(), ListColumn::SHOWN_AT_FIRST);
    browser.apply(&AppCommand::BrowserColumnToggle {
        column: ListColumn::Comment,
    });
    browser.apply(&AppCommand::BrowserColumnToggle {
        column: ListColumn::Genre,
    });
    browser.apply(&AppCommand::BrowserColumnToggle {
        column: ListColumn::Artist,
    });
    assert_eq!(
        browser.columns(),
        [
            ListColumn::Colour,
            ListColumn::Album,
            ListColumn::Genre,
            ListColumn::Bpm,
            ListColumn::Key,
            ListColumn::Time,
            ListColumn::Rating,
            ListColumn::Comment,
            ListColumn::File,
        ]
    );
}
