//! The key: Platter's button. It emits a command and never changes state itself.
//!
//! Component: `TransportButton` in `docs/design/DESIGN.md`, the model for every key.

use egui::{Align2, Color32, Id, Key as KeyboardKey, Rect, Role, Sense, StrokeKind, WidgetInfo};

use crate::command::Action;
use crate::cx::{ControlKind, Cx};
use crate::icons::Glyph;
use crate::paint;
use crate::tokens::TextStyle;

pub enum Face<'a> {
    Text(&'a str),
    /// An icon at a size in pixels.
    Icon(Glyph, f32),
    /// An icon followed by a text.
    IconText(Glyph, &'a str),
}

pub struct KeySpec<'a> {
    pub face: Face<'a>,
    /// Name for assistive technology; for a key with a text it is usually that text.
    pub name: &'a str,
    /// Colour of the LED strip of a key that is on.
    pub lit: Option<Color32>,
    /// Colour of the LED strip while the key is held down, for a key that is otherwise off.
    pub held: Option<Color32>,
    pub enabled: bool,
    /// Style of the text; the button style unless a small key needs another.
    pub text: Option<TextStyle>,
}

impl<'a> KeySpec<'a> {
    pub fn text(label: &'a str) -> Self {
        Self {
            face: Face::Text(label),
            name: label,
            lit: None,
            held: None,
            enabled: true,
            text: None,
        }
    }

    pub fn icon(glyph: Glyph, size: f32, name: &'a str) -> Self {
        Self {
            face: Face::Icon(glyph, size),
            name,
            lit: None,
            held: None,
            enabled: true,
            text: None,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// What a key does when it is operated.
pub enum Press {
    /// One command, on click or on Space or Enter.
    Click(Action),
    /// A command when the key goes down and another when it comes up, for functions that
    /// act while held.
    Hold { down: Action, up: Action },
}

/// Draws a key in `rect` and emits its command when it is operated.
pub fn key(cx: &mut Cx<'_>, rect: Rect, id: Id, spec: KeySpec<'_>, press: Press) {
    let (command, kind) = match &press {
        Press::Click(action) => (action.id(), ControlKind::Button),
        Press::Hold { down, .. } => (down.id(), ControlKind::Hold),
    };
    cx.record(command, rect, kind, spec.enabled);
    let sense = cx.sense(spec.enabled, Sense::click());
    let response = cx.ui.interact(rect, id, sense);
    response.widget_info(|| WidgetInfo::labeled(Role::Button, spec.enabled, spec.name));
    let focused = response.has_focus();
    if focused {
        cx.out.claim(&[KeyboardKey::Space, KeyboardKey::Enter]);
    }

    let mut down = response.is_pointer_button_down_on();
    match press {
        Press::Click(action) => {
            if response.clicked() {
                cx.out.emit(action);
            }
        }
        Press::Hold { down: press, up } => {
            let keyboard = focused
                && cx.ui.input(|input| {
                    input.key_down(KeyboardKey::Space) || input.key_down(KeyboardKey::Enter)
                });
            down = spec.enabled && (down || keyboard);
            let was_down = cx
                .ui
                .ctx()
                .data(|data| data.get_temp::<bool>(id))
                .unwrap_or(false);
            if down != was_down {
                cx.ui.ctx().data_mut(|data| data.insert_temp(id, down));
                cx.out.emit(if down { press } else { up });
            }
        }
    }

    let tokens = &cx.theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    let corner = paint::radius(tokens.radius.md);
    let lit = spec
        .lit
        .or(spec.held.filter(|_| down))
        .filter(|_| spec.enabled);
    let fill = if !spec.enabled {
        Color32::TRANSPARENT
    } else if down || lit.is_some() {
        color.surface_pressed
    } else if response.hovered() {
        color.surface_hover
    } else {
        color.surface_control
    };
    painter.rect(
        rect,
        corner,
        fill,
        paint::hairline(cx.theme, color.line_key),
        StrokeKind::Inside,
    );
    if let Some(led) = lit {
        let strip = Rect::from_min_size(rect.min, egui::vec2(rect.width(), tokens.stroke.led));
        painter.rect_filled(
            strip,
            egui::CornerRadius {
                nw: corner.nw,
                ne: corner.ne,
                sw: 0,
                se: 0,
            },
            led,
        );
    }
    let ink = if spec.enabled {
        color.text_primary
    } else {
        color.text_disabled
    };
    let style = spec.text.unwrap_or(tokens.text.button);
    match spec.face {
        Face::Text(label) => {
            paint::text(
                painter,
                cx.theme,
                rect,
                Align2::CENTER_CENTER,
                label,
                style,
                ink,
            );
        }
        Face::Icon(glyph, size) => {
            let icon = Rect::from_center_size(rect.center(), egui::vec2(size, size));
            cx.theme.icons.paint(painter, glyph, icon, ink);
        }
        Face::IconText(glyph, label) => {
            let size = 14.0;
            let gap = tokens.space.s2 + tokens.space.s1;
            let width = size + gap + paint::text_width(painter, cx.theme, label, style);
            let left = rect.center().x - width / 2.0;
            let icon = Rect::from_min_size(
                egui::pos2(left, rect.center().y - size / 2.0),
                egui::vec2(size, size),
            );
            cx.theme.icons.paint(painter, glyph, icon, ink);
            let label_rect = Rect::from_min_max(egui::pos2(icon.max.x + gap, rect.min.y), rect.max);
            paint::text(
                painter,
                cx.theme,
                label_rect,
                Align2::LEFT_CENTER,
                label,
                style,
                ink,
            );
        }
    }
    if focused {
        paint::focus_ring(painter, cx.theme, rect, tokens.radius.md);
    }
}

/// Width a key needs for its text: the text and the padding of a button on both sides. A
/// key grows with a longer translation and never cuts its label.
pub fn width_for(cx: &Cx<'_>, label: &str, minimum: f32) -> f32 {
    let tokens = &cx.theme.tokens;
    let text = paint::text_width(cx.painter(), cx.theme, label, tokens.text.button);
    (text + 2.0 * tokens.space.s4).ceil().max(minimum)
}
