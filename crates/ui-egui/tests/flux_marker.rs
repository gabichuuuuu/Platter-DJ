//! The timeline of flux in the waveforms, drawn without a window. Behaviour note:
//! `docs/behavior/flux.md`.

#![allow(clippy::unwrap_used)]

use egui::epaint::{ClippedShape, Shape};
use egui::{Color32, Pos2, RawInput, Rect};
use platter_ui_egui::i18n::Language;
use platter_ui_egui::sample::{self, Scene};
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};
use platter_ui_egui::tokens::Tokens;

/// The colours of the vertical lines the window paints.
fn lines(scene: &Scene) -> Vec<Color32> {
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
    let mut found = Vec::new();
    for ClippedShape { shape, .. } in &output.shapes {
        collect(shape, &mut found);
    }
    found
}

fn collect(shape: &Shape, found: &mut Vec<Color32>) {
    match shape {
        Shape::LineSegment { points, stroke } if points[0].x == points[1].x => {
            found.push(stroke.color);
        }
        Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, found)),
        _ => {}
    }
}

#[test]
fn with_flux_on_a_dimmed_playhead_shows_where_the_track_would_be() {
    let tokens = Tokens::load().unwrap();
    let dimmed = tokens.color.playhead.gamma_multiply(0.45);
    assert!(!lines(&sample::playing()).contains(&dimmed));
    let mut scene = sample::playing();
    let deck = &mut scene.engine.decks[0];
    deck.flux = true;
    deck.timeline_seconds = Some(deck.position_seconds + 1.0);
    let found = lines(&scene);
    // In the scrolling lane and in the overview.
    assert_eq!(found.iter().filter(|color| **color == dimmed).count(), 2);
}
