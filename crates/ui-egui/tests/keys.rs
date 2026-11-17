//! The key column of the browser and the keys compatible with the tempo master, drawn
//! without a window.
//!
//! Behaviour note: `docs/behavior/browser.md`, *Analysis in the background and keys*.

#![allow(clippy::unwrap_used)]

use egui::epaint::{ClippedShape, Shape};
use egui::{Color32, Pos2, RawInput, Rect};
use platter_engine::{Key, Mode, Transposed};
use platter_ui_egui::i18n::Language;
use platter_ui_egui::keys::KeyNotation;
use platter_ui_egui::sample::{self, Scene};
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};
use platter_ui_egui::tokens::Tokens;

/// Every text the list of the browser paints with its colour: what is below the heading of
/// its title column.
fn painted(scene: &Scene) -> Vec<(String, Color32)> {
    let all = painted_with_places(scene);
    let heading = all
        .iter()
        .find(|(text, _, _)| text == "Title")
        .map(|(_, _, y)| *y)
        .unwrap();
    all.into_iter()
        .filter(|(_, _, y)| *y > heading)
        .map(|(text, color, _)| (text, color))
        .collect()
}

/// Every text the window paints, with its colour and the top of its line.
fn painted_with_places(scene: &Scene) -> Vec<(String, Color32, f32)> {
    let mut shell = Shell::new(Language::English).unwrap();
    let ctx = egui::Context::default();
    shell.install(&ctx);
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(DESIGN_WIDTH, DESIGN_HEIGHT),
        )),
        ..RawInput::default()
    };
    let view = scene.view();
    let mut output = ctx.run_ui(input, |ui| {
        shell.show(ui, &view);
    });
    output.textures_delta.clear();
    let mut texts = Vec::new();
    for ClippedShape { shape, .. } in &output.shapes {
        collect(shape, &mut texts);
    }
    texts
}

fn collect(shape: &Shape, texts: &mut Vec<(String, Color32, f32)>) {
    match shape {
        Shape::Text(text) => {
            let color = text
                .galley
                .job
                .sections
                .first()
                .map_or(Color32::PLACEHOLDER, |section| section.format.color);
            texts.push((text.galley.text().to_owned(), color, text.pos.y));
        }
        Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, texts)),
        _ => {}
    }
}

fn colours_of(texts: &[(String, Color32)], text: &str) -> Vec<Color32> {
    texts
        .iter()
        .filter(|(painted, _)| painted == text)
        .map(|(_, color)| *color)
        .collect()
}

#[test]
fn the_keys_compatible_with_the_tempo_master_take_their_colour() {
    let tokens = Tokens::load().unwrap();
    let color = &tokens.color;
    // Deck A, the tempo master, plays A minor (1m): A min, E min (2m), D min (12m) and C maj
    // (1d) mix with it; D♭ maj (8d) does not.
    let texts = painted(&sample::playing());
    for key in ["A min", "E min", "D min", "C maj"] {
        let colours = colours_of(&texts, key);
        assert!(!colours.is_empty(), "{key} is not in the list");
        assert!(
            colours.iter().all(|colour| *colour == color.key_match),
            "{key}: {colours:?}"
        );
    }
    assert_eq!(colours_of(&texts, "D♭ maj"), [color.text_secondary]);

    // Without a key in the master deck nothing is marked.
    let mut scene = sample::playing();
    scene.engine.decks[0].resulting_key = None;
    let texts = painted(&scene);
    assert_eq!(colours_of(&texts, "C maj"), [color.text_secondary]);

    // The key it plays counts, transposed: up two semitones it plays B minor (3m), with
    // which D min and C maj no longer mix.
    let mut scene = sample::playing();
    scene.engine.decks[0].resulting_key = Some(Transposed {
        key: Key {
            tonic: 11,
            mode: Mode::Minor,
        },
        cents: 0.0,
    });
    let texts = painted(&scene);
    assert_eq!(colours_of(&texts, "C maj"), [color.text_secondary]);
    assert!(
        colours_of(&texts, "E min")
            .iter()
            .all(|colour| *colour == color.key_match)
    );
}

#[test]
fn the_notation_of_the_settings_writes_the_keys() {
    let mut scene = sample::playing();
    scene.settings.key_notation = KeyNotation::OpenKey;
    let texts = painted(&scene);
    assert!(!colours_of(&texts, "1m").is_empty());
    assert!(!colours_of(&texts, "8d").is_empty());
    assert!(colours_of(&texts, "A min").is_empty());
    scene.settings.key_notation = KeyNotation::Sharps;
    let texts = painted(&scene);
    assert!(!colours_of(&texts, "C♯ maj").is_empty());
}

#[test]
fn the_deck_shows_the_key_it_plays_between_its_semitone_keys() {
    let tokens = Tokens::load().unwrap();
    // Deck A plays A minor; deck B has no key found.
    let all = painted_with_places(&sample::playing());
    let in_decks: Vec<&(String, Color32, f32)> = all
        .iter()
        .filter(|(text, _, _)| text == "A min" || text == "--")
        .filter(|(_, color, _)| *color == tokens.color.text_primary)
        .collect();
    assert!(
        in_decks.iter().any(|(text, _, _)| text == "A min"),
        "{in_decks:?}"
    );
    assert!(
        in_decks.iter().any(|(text, _, _)| text == "--"),
        "{in_decks:?}"
    );
}
