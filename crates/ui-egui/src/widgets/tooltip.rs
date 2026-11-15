//! The tooltip: short help beside a key, on hover or on keyboard focus.
//!
//! Component: `Tooltip` in `docs/design/DESIGN.md`. Behaviour note:
//! `docs/behavior/interface.md`, Tooltips.

use std::time::Duration;

use egui::emath::GuiRounding as _;
use egui::{Align2, Key, LayerId, Order, Rect, Response};

use crate::command::Action;
use crate::cx::{Cx, id};
use crate::keymap::{Chord, Keymap};
use crate::paint;
use crate::panels::settings::chord_text;

/// Seconds the pointer rests on a control before its tooltip shows.
const DELAY: f64 = 0.6;
/// Distance between the control and its tooltip.
const GAP: f32 = 8.0;
const MAX_WIDTH: f32 = 240.0;

/// What a control remembers between frames about its tooltip.
#[derive(Clone, Copy, Default)]
struct Hover {
    /// When the pointer came onto the control; `None` while it is elsewhere.
    since: Option<f64>,
    /// The focus came from the keyboard, not from a click.
    keyboard: bool,
    /// Escape hid the tooltip until the pointer and the focus leave.
    dismissed: bool,
}

/// Shows the tooltip of a control when it is due: `name` and the shortcut of `action`, if
/// the keymap has one. Nothing shows while tooltips are off or the control lies behind a
/// dialog.
pub fn tooltip(cx: &Cx<'_>, response: &Response, name: &str, action: &Action) {
    let Some(keymap) = cx.help else {
        return;
    };
    if !cx.interactive || name.is_empty() {
        return;
    }
    let ctx = cx.ui.ctx();
    let memory = response.id.with("tooltip");
    let hovered = response.hovered();
    let focused = response.has_focus();
    if !hovered && !focused {
        ctx.data_mut(|data| data.remove_temp::<Hover>(memory));
        return;
    }
    let now = cx.ui.input(|input| input.time);
    let mut state = ctx
        .data(|data| data.get_temp::<Hover>(memory))
        .unwrap_or_default();
    if response.gained_focus() {
        state.keyboard = !cx
            .ui
            .input(|input| input.pointer.any_pressed() || input.pointer.any_down());
    }
    state.keyboard &= focused;
    state.since = if hovered {
        state.since.or(Some(now))
    } else {
        None
    };
    if cx.ui.input(|input| input.key_pressed(Key::Escape)) {
        state.dismissed = true;
    }
    let rested = state.since.map_or(0.0, |since| now - since);
    let shown = !state.dismissed && ((focused && state.keyboard) || rested >= DELAY);
    if hovered && !shown && !state.dismissed {
        ctx.request_repaint_after(Duration::from_secs_f64(DELAY - rested));
    }
    ctx.data_mut(|data| data.insert_temp(memory, state));
    if shown {
        let shortcut = shortcut(keymap, action).map(|chord| chord_text(cx.texts, chord));
        draw(cx, response.rect, name, shortcut.as_deref());
    }
}

/// The chord of the first binding that sends exactly `action`.
fn shortcut<'k>(keymap: &'k Keymap, action: &Action) -> Option<&'k Chord> {
    keymap
        .bindings()
        .iter()
        .find(|binding| {
            binding.command == action.id()
                && Action::from_args(binding.command, &binding.args)
                    .is_ok_and(|bound| bound == *action)
        })
        .map(|binding| &binding.chord)
}

fn draw(cx: &Cx<'_>, anchor: Rect, name: &str, shortcut: Option<&str>) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx
        .ui
        .ctx()
        .layer_painter(LayerId::new(Order::Tooltip, id("tooltip")));
    let pad = egui::vec2(tokens.space.s3, tokens.space.s2);
    let body = tokens.text.body;
    let cap_style = tokens.text.readout;
    let cap_pad = tokens.space.s2;
    let cap_width = shortcut.map_or(0.0, |text| {
        paint::text_width(&painter, theme, text, cap_style) + 2.0 * cap_pad
    });
    let cap_room = if shortcut.is_some() {
        cap_width + tokens.space.s3
    } else {
        0.0
    };
    let text_width = paint::text_width(&painter, theme, name, body)
        .min(MAX_WIDTH - 2.0 * pad.x - cap_room)
        .max(0.0);
    let size = egui::vec2(
        text_width + cap_room + 2.0 * pad.x,
        body.line_height + 2.0 * pad.y,
    );
    let screen = cx.ui.ctx().content_rect();
    let below = anchor.bottom() + GAP;
    let top = if below + size.y <= screen.bottom() {
        below
    } else {
        anchor.top() - GAP - size.y
    };
    let left = anchor
        .left()
        .min(screen.right() - size.x)
        .max(screen.left());
    let frame = Rect::from_min_size(egui::pos2(left, top), size).round_ui();
    painter.rect_filled(
        frame,
        paint::radius(tokens.radius.md),
        color.surface_pressed,
    );
    let line = Rect::from_min_size(frame.min + pad, egui::vec2(text_width, body.line_height));
    paint::text_elided(&painter, theme, line, name, body, color.text_primary);
    if let Some(text) = shortcut {
        let cap = Rect::from_min_size(
            egui::pos2(line.right() + tokens.space.s3, line.top()),
            egui::vec2(cap_width, body.line_height),
        );
        painter.rect_filled(cap, paint::radius(tokens.radius.sm), color.bg_app);
        paint::text(
            &painter,
            theme,
            cap,
            Align2::CENTER_CENTER,
            text,
            cap_style,
            color.text_secondary,
        );
    }
}
