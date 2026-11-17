//! The mixer panel in external mixer mode, drawn without a window. Behaviour note:
//! `docs/behavior/external-mixer.md`.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;

use egui::{Pos2, RawInput, Rect};
use platter_engine::MixerMode;
use platter_ui_egui::i18n::Language;
use platter_ui_egui::sample::{self, Scene};
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};

/// The commands of the controls of the mixer the window draws.
fn mixer_commands(scene: &Scene) -> BTreeSet<&'static str> {
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
    shell
        .controls()
        .iter()
        .filter(|control| {
            control.command.starts_with("mixer.") || control.command == "deck.key.set"
        })
        // The mixer sits between the decks, 520 wide each in a 1440 window.
        .filter(|control| control.rect.center().x > 520.0 && control.rect.center().x < 920.0)
        .map(|control| control.command)
        .collect()
}

#[test]
fn in_external_mode_each_channel_keeps_its_gain_its_key_and_its_pre_listen() {
    let inside = mixer_commands(&sample::playing());
    for command in [
        "mixer.eq.set",
        "mixer.fader.set",
        "mixer.crossfader.set",
        "mixer.filter.set",
    ] {
        assert!(inside.contains(command), "{command}: {inside:?}");
    }
    let mut scene = sample::playing();
    scene.engine.mixer.mode = MixerMode::External;
    let outside = mixer_commands(&scene);
    for command in ["mixer.gain.set", "deck.key.set", "mixer.cue.toggle"] {
        assert!(outside.contains(command), "{command}: {outside:?}");
    }
    for command in [
        "mixer.eq.set",
        "mixer.eq.kill.toggle",
        "mixer.fader.set",
        "mixer.filter.set",
        "mixer.pan.set",
        "mixer.crossfader.set",
        "mixer.crossfader.step",
        "mixer.xfader_assign.set",
    ] {
        assert!(!outside.contains(command), "{command}: {outside:?}");
    }
}
