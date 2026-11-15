//! The MIDI indicator of the status header, drawn without a window.
//!
//! Behaviour note: `docs/behavior/interface.md`, MIDI.

#![allow(clippy::unwrap_used)]

use egui::epaint::{ClippedShape, Shape};
use egui::{Color32, Pos2, RawInput, Rect};
use platter_ui_egui::i18n::Language;
use platter_ui_egui::model::MidiView;
use platter_ui_egui::sample;
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};
use platter_ui_egui::tokens::Tokens;

/// Every text the header paints with its colour, and the colours of its filled circles.
fn header(midi: MidiView) -> (Vec<(String, Color32)>, Vec<Color32>) {
    let mut shell = Shell::new(Language::English).unwrap();
    let ctx = egui::Context::default();
    shell.install(&ctx);
    let mut scene = sample::playing();
    scene.midi = midi;
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
    let (mut texts, mut dots) = (Vec::new(), Vec::new());
    for ClippedShape { shape, .. } in &output.shapes {
        collect(shape, &mut texts, &mut dots);
    }
    (texts, dots)
}

/// Shapes in the band of the status header, 32 points high.
fn collect(shape: &Shape, texts: &mut Vec<(String, Color32)>, dots: &mut Vec<Color32>) {
    match shape {
        Shape::Text(text) if text.pos.y < 32.0 => {
            let color = text
                .galley
                .job
                .sections
                .first()
                .map_or(Color32::PLACEHOLDER, |section| section.format.color);
            texts.push((text.galley.text().to_owned(), color));
        }
        Shape::Circle(circle) if circle.center.y < 32.0 => dots.push(circle.fill),
        Shape::Vec(shapes) => {
            for shape in shapes {
                collect(shape, texts, dots);
            }
        }
        _ => {}
    }
}

fn color_of(texts: &[(String, Color32)], text: &str) -> Option<Color32> {
    texts
        .iter()
        .find(|(painted, _)| painted == text)
        .map(|(_, color)| *color)
}

#[test]
fn no_controller_in_the_settings_draws_no_indicator() {
    let (texts, _) = header(MidiView::default());
    assert_eq!(color_of(&texts, "MIDI"), None);
}

#[test]
fn the_dot_and_the_count_say_how_many_controllers_are_open() {
    let tokens = Tokens::load().unwrap();
    let color = &tokens.color;
    for (open, dot) in [
        (2, color.status_ok),
        (1, color.status_warn),
        (0, color.status_error),
    ] {
        let (texts, dots) = header(MidiView {
            named: 2,
            open,
            active: false,
        });
        assert!(
            color_of(&texts, &format!("{open}/2")).is_some(),
            "{texts:?}"
        );
        assert!(dots.contains(&dot), "{open} open: {dots:?}");
    }
}

#[test]
fn the_label_lights_while_messages_come() {
    let tokens = Tokens::load().unwrap();
    let quiet = MidiView {
        named: 1,
        open: 1,
        active: false,
    };
    let (texts, _) = header(quiet);
    assert_eq!(color_of(&texts, "MIDI"), Some(tokens.color.text_secondary));
    let (texts, _) = header(MidiView {
        active: true,
        ..quiet
    });
    assert_eq!(color_of(&texts, "MIDI"), Some(tokens.color.text_primary));
}
