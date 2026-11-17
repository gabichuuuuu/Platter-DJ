//! Folders of playlists and *History* close and open in the tree. Behaviour note:
//! `docs/behavior/browser.md`, *Playlists* and *History*.

#![allow(clippy::unwrap_used)]

use egui::epaint::{ClippedShape, Shape};
use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect};
use platter_library::NodeId;
use platter_ui_egui::command::{Action, AppCommand};
use platter_ui_egui::files::FileBrowser;
use platter_ui_egui::i18n::Language;
use platter_ui_egui::sample::{self, Scene};
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};

/// The tree scrolled down to *History*.
const TO_HISTORY: f32 = 4.0 * 27.0;

/// What a frame of `scene` paints and the controls it draws, with the tree scrolled by
/// `scroll`; a click on `click` first.
fn frame(scene: &Scene, scroll: f32, click: Option<Pos2>) -> (Vec<String>, Vec<AppCommand>, Shell) {
    let mut shell = Shell::new(Language::English).unwrap();
    shell.state.browser.tree_scroll = scroll;
    let ctx = egui::Context::default();
    shell.install(&ctx);
    let view = scene.view();
    let mut run = |events: Vec<Event>| {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(DESIGN_WIDTH, DESIGN_HEIGHT),
            )),
            events,
            focused: true,
            ..RawInput::default()
        };
        let mut actions = Vec::new();
        let mut output = ctx.run_ui(input, |ui| {
            actions = shell.show(ui, &view);
        });
        output.textures_delta.clear();
        let mut texts = Vec::new();
        for ClippedShape { shape, .. } in &output.shapes {
            collect(shape, &mut texts);
        }
        (texts, actions)
    };
    let (mut texts, mut actions) = run(Vec::new());
    if let Some(pos) = click {
        let press = |pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        run(vec![Event::PointerMoved(pos), press(true)]);
        (texts, actions) = run(vec![press(false)]);
    }
    let commands = actions
        .into_iter()
        .filter_map(|action| match action {
            Action::App(command) => Some(command),
            Action::Engine(_) => None,
        })
        .collect();
    (texts, commands, shell)
}

fn collect(shape: &Shape, found: &mut Vec<String>) {
    match shape {
        Shape::Text(text) => found.push(text.galley.text().to_owned()),
        Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, found)),
        _ => {}
    }
}

/// Where the pointer operates the chevron that sends `command`.
fn chevron(shell: &Shell, command: &str) -> Pos2 {
    shell
        .controls()
        .iter()
        .find(|control| control.command == command && control.rect.width() < 30.0)
        .map(|control| control.rect.center())
        .unwrap()
}

#[test]
fn a_closed_folder_hides_its_playlists_and_its_chevron_opens_it() {
    let terrace = |texts: &[String]| {
        texts
            .iter()
            .filter(|text| *text == "Summer terrace")
            .count()
    };
    let (texts, _, shell) = frame(&sample::playing(), 0.0, None);
    let open = terrace(&texts);
    let (_, sent, _) = frame(
        &sample::playing(),
        0.0,
        Some(chevron(&shell, "browser.folder.toggle")),
    );
    assert_eq!(
        sent,
        [AppCommand::BrowserFolderToggle {
            folder: NodeId::new(3)
        }]
    );

    let mut scene = sample::playing();
    scene.browser.closed_folders = vec![NodeId::new(3)];
    let (texts, _, _) = frame(&scene, 0.0, None);
    assert!(texts.iter().any(|text| text == "Archive"));
    // Its favourite chip still names it; its row in the tree is gone.
    assert_eq!(terrace(&texts), open - 1);
    // What comes after the folder, at its level, still shows.
    assert!(texts.iter().any(|text| text == "Friday warm-up"));
}

#[test]
fn history_hides_and_shows_the_earlier_sessions() {
    let (texts, _, shell) = frame(&sample::playing(), TO_HISTORY, None);
    assert!(texts.iter().any(|text| text == "2026-11-14 21:30"));
    let (_, sent, _) = frame(
        &sample::playing(),
        TO_HISTORY,
        Some(chevron(&shell, "browser.history.toggle")),
    );
    assert_eq!(sent, [AppCommand::BrowserHistoryToggle]);

    let mut scene = sample::playing();
    scene.browser.history_open = false;
    let (texts, _, _) = frame(&scene, TO_HISTORY, None);
    assert!(texts.iter().any(|text| text == "History"));
    assert!(!texts.iter().any(|text| text == "2026-11-14 21:30"));
}

#[test]
fn the_browser_keeps_what_is_closed() {
    let mut browser = FileBrowser::new(Vec::new());
    let folder = NodeId::new(3);
    assert!(browser.view().closed_folders.is_empty() && browser.view().history_open);
    browser.apply(&AppCommand::BrowserFolderToggle { folder });
    browser.apply(&AppCommand::BrowserHistoryToggle);
    let view = browser.view();
    assert_eq!(view.closed_folders, [folder]);
    assert!(!view.history_open);
    browser.apply(&AppCommand::BrowserFolderToggle { folder });
    assert!(browser.view().closed_folders.is_empty());
}
