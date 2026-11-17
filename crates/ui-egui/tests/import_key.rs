//! The key of the import while an import runs: it stops it, and the line above the list counts
//! the files looked at. Behaviour note: `docs/behavior/browser.md`, *The collection*.

#![allow(clippy::unwrap_used)]

use egui::epaint::{ClippedShape, Shape};
use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect};
use platter_ui_egui::command::{Action, AppCommand};
use platter_ui_egui::i18n::Language;
use platter_ui_egui::model::ImportOutcome;
use platter_ui_egui::sample::{self, Scene};
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};

/// The texts a frame of `scene` paints, and what a click on the key of the import sends.
fn import_key(scene: &Scene) -> (Vec<String>, Vec<AppCommand>) {
    let mut shell = Shell::new(Language::English).unwrap();
    let ctx = egui::Context::default();
    shell.install(&ctx);
    let view = scene.view();
    let run = |shell: &mut Shell, events: Vec<Event>| {
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
    let (texts, _) = run(&mut shell, Vec::new());
    let key = shell
        .controls()
        .iter()
        .find(|control| {
            control.command == "library.import_folder" || control.command == "library.import.stop"
        })
        .map(|control| control.rect.center())
        .unwrap();
    let press = |pressed| Event::PointerButton {
        pos: key,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    run(&mut shell, vec![Event::PointerMoved(key), press(true)]);
    let (_, actions) = run(&mut shell, vec![press(false)]);
    let sent = actions
        .into_iter()
        .filter_map(|action| match action {
            Action::App(command) => Some(command),
            Action::Engine(_) => None,
        })
        .collect();
    (texts, sent)
}

fn collect(shape: &Shape, found: &mut Vec<String>) {
    match shape {
        Shape::Text(text) => found.push(text.galley.text().to_owned()),
        Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, found)),
        _ => {}
    }
}

#[test]
fn while_an_import_runs_its_key_stops_it_and_the_files_are_counted() {
    let mut scene = sample::playing();
    let collection = scene.browser.collection.as_mut().unwrap();
    collection.importing = true;
    collection.import_looked = 1234;
    let (texts, sent) = import_key(&scene);
    assert!(texts.iter().any(|text| text == "Stop import"), "{texts:?}");
    assert!(
        texts
            .iter()
            .any(|text| text.contains("1234 files looked at")),
        "{texts:?}"
    );
    assert_eq!(sent, [AppCommand::LibraryImportStop]);

    let mut scene = sample::playing();
    let collection = scene.browser.collection.as_mut().unwrap();
    collection.imported = Some(ImportOutcome::Done {
        added: 2,
        known: 1,
        unreadable: 0,
        stopped: true,
    });
    let (texts, sent) = import_key(&scene);
    assert!(
        texts
            .iter()
            .any(|text| text.contains("Import stopped: 2 added")),
        "{texts:?}"
    );
    assert!(matches!(
        sent.as_slice(),
        [AppCommand::LibraryImportFolder { .. }]
    ));
}
