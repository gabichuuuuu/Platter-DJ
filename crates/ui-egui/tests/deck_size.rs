//! The decks in their two sizes. Behaviour note: `docs/behavior/interface.md`, *Deck sizes*.

#![allow(clippy::unwrap_used)]

use egui::epaint::{ClippedShape, Shape};
use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect};
use platter_ui_egui::command::{Action, AppCommand, DeckSize};
use platter_ui_egui::cx::Control;
use platter_ui_egui::i18n::Language;
use platter_ui_egui::sample::{self, Scene};
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};

/// Where the band of the decks starts: under the status header and the waveform lanes.
const BAND_TOP: f32 = 40.0 + 2.0 + 178.0 + 2.0;
const DECK_WIDTH: f32 = 520.0;

/// A text drawn in a frame and where.
struct Drawn {
    text: String,
    at: Pos2,
}

/// What a frame of `scene` in `size` paints, the controls it draws and the commands it sends,
/// after `clicks` clicks at `at`.
fn frame(size: DeckSize, at: Option<(Pos2, usize)>) -> (Vec<Drawn>, Vec<Control>, Vec<AppCommand>) {
    let mut scene: Scene = sample::playing();
    scene.settings.deck_size = size;
    let mut shell = Shell::new(Language::English).unwrap();
    let ctx = egui::Context::default();
    shell.install(&ctx);
    let view = scene.view();
    let mut sent = Vec::new();
    let mut run = |events: Vec<Event>, sent: &mut Vec<AppCommand>| {
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
        sent.extend(actions.into_iter().filter_map(|action| match action {
            Action::App(command) => Some(command),
            Action::Engine(_) => None,
        }));
        let mut drawn = Vec::new();
        for ClippedShape { shape, .. } in &output.shapes {
            collect(shape, &mut drawn);
        }
        drawn
    };
    let mut drawn = run(Vec::new(), &mut sent);
    if let Some((pos, clicks)) = at {
        let press = |pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        run(vec![Event::PointerMoved(pos)], &mut sent);
        for _ in 0..clicks {
            run(vec![press(true)], &mut sent);
            drawn = run(vec![press(false)], &mut sent);
        }
    }
    (drawn, shell.controls().to_vec(), sent)
}

fn collect(shape: &Shape, found: &mut Vec<Drawn>) {
    match shape {
        Shape::Text(text) => found.push(Drawn {
            text: text.galley.text().to_owned(),
            at: text.pos,
        }),
        Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, found)),
        _ => {}
    }
}

/// The texts drawn inside deck A, a band of `height` points.
fn deck_a(drawn: &[Drawn], height: f32) -> Vec<&str> {
    drawn
        .iter()
        .filter(|drawn| {
            drawn.at.x < DECK_WIDTH && drawn.at.y >= BAND_TOP && drawn.at.y < BAND_TOP + height
        })
        .map(|drawn| drawn.text.as_str())
        .collect()
}

fn tags() -> (String, String, String) {
    let scene = sample::playing();
    let tags = &scene.engine.decks[0].track.as_ref().unwrap().tags;
    (
        tags.title.clone().unwrap(),
        tags.artist.clone().unwrap(),
        tags.album.clone().unwrap(),
    )
}

#[test]
fn a_compact_deck_has_a_header_of_one_row_and_no_advanced_panel() {
    let (title, artist, album) = tags();
    let (full, full_controls, _) = frame(DeckSize::Full, None);
    let full_texts = deck_a(&full, 388.0);
    assert!(full_texts.contains(&title.as_str()));
    assert!(full_texts.contains(&artist.as_str()) && full_texts.contains(&album.as_str()));
    assert!(
        full_controls
            .iter()
            .any(|control| control.command == "deck.page.set")
    );

    let (compact, controls, _) = frame(DeckSize::Compact, None);
    let texts = deck_a(&compact, 356.0);
    assert!(texts.contains(&title.as_str()));
    assert!(!texts.contains(&artist.as_str()) && !texts.contains(&album.as_str()));
    assert!(
        !controls
            .iter()
            .any(|control| control.command == "deck.page.set")
    );
    // What stays: the hotcues, the loop row, the transport and the times.
    for command in [
        "deck.hotcue.trigger",
        "deck.loop.toggle",
        "deck.play_pause",
        "deck.time_display.toggle",
        "deck.sync.toggle",
    ] {
        assert!(
            controls.iter().any(|control| control.command == command),
            "{command} is missing"
        );
    }
}

#[test]
fn compact_decks_give_their_room_to_the_browser_and_keep_their_controls_inside() {
    let top_of_browser = |controls: &[Control]| {
        controls
            .iter()
            .filter(|control| control.command.starts_with("browser."))
            .map(|control| control.rect.top())
            .fold(f32::INFINITY, f32::min)
    };
    let (_, full, _) = frame(DeckSize::Full, None);
    let (_, compact, _) = frame(DeckSize::Compact, None);
    assert!(top_of_browser(&compact) < top_of_browser(&full) - 30.0);

    let band = Rect::from_min_size(egui::pos2(0.0, BAND_TOP), egui::vec2(DESIGN_WIDTH, 356.0));
    for control in compact
        .iter()
        .filter(|control| control.command.starts_with("deck.") && control.rect.left() < DECK_WIDTH)
    {
        assert!(
            band.contains_rect(control.rect),
            "{} at {:?} is outside the band of the decks",
            control.command,
            control.rect
        );
    }
    // The hotcues do not cover the sync row above them.
    let bottom_of = |command: &str| {
        compact
            .iter()
            .filter(|control| control.command == command && control.rect.left() < DECK_WIDTH)
            .map(|control| control.rect.bottom())
            .fold(f32::NEG_INFINITY, f32::max)
    };
    let top_of = |command: &str| {
        compact
            .iter()
            .filter(|control| control.command == command && control.rect.left() < DECK_WIDTH)
            .map(|control| control.rect.top())
            .fold(f32::INFINITY, f32::min)
    };
    assert!(bottom_of("deck.sync.toggle") < top_of("deck.hotcue.trigger"));
    assert!(bottom_of("deck.hotcue.trigger") < top_of("deck.loop.toggle"));
    assert!(bottom_of("deck.loop.toggle") < top_of("deck.play_pause"));
}

#[test]
fn a_double_click_on_the_header_of_a_deck_switches_the_size() {
    let (title, _, _) = tags();
    for (size, other, height) in [
        (DeckSize::Full, DeckSize::Compact, 388.0),
        (DeckSize::Compact, DeckSize::Full, 356.0),
    ] {
        let (drawn, _, _) = frame(size, None);
        let at = drawn
            .iter()
            .find(|drawn| {
                drawn.text == title && drawn.at.x < DECK_WIDTH && drawn.at.y < BAND_TOP + height
            })
            .unwrap()
            .at
            + egui::vec2(4.0, 6.0);
        let (_, _, once) = frame(size, Some((at, 1)));
        assert!(once.is_empty(), "{once:?}");
        let (_, _, twice) = frame(size, Some((at, 2)));
        assert_eq!(twice, [AppCommand::DeckSizeSet { size: other }]);
    }
}
