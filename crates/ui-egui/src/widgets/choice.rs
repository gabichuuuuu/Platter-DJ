//! Controls that pick one of several options: the drop-down and the segmented control.
//!
//! Component: `Select` in `docs/design/DESIGN.md`; the segmented control is the `seg`
//! control of the preference screens.

use egui::{
    Align2, Area, Color32, EventFilter, Id, Key, Order, Rect, Role, Sense, StrokeKind, WidgetInfo,
};

use crate::command::Action;
use crate::cx::{ControlKind, Cx};
use crate::icons::Glyph;
use crate::paint;
use crate::widgets::tooltip;

pub struct Choice<'a> {
    pub label: &'a str,
    /// An option the current device cannot use is shown and cannot be picked.
    pub enabled: bool,
}

pub struct SelectSpec<'a> {
    /// Name for assistive technology.
    pub name: &'a str,
    pub choices: &'a [Choice<'a>],
    /// `None` when the value is none of the options.
    pub current: Option<usize>,
    pub enabled: bool,
}

/// What an open drop-down remembers: the row the keyboard is on.
#[derive(Clone, Copy, Default)]
struct Open {
    highlighted: usize,
    /// The first option the list shows, when it has more than it shows at once.
    first: usize,
}

const ROW_HEIGHT: f32 = 28.0;
/// Most options an open list shows at once; the wheel and the arrows move through the rest.
const VISIBLE_ROWS: usize = 12;

/// Draws a drop-down in `rect` showing `choices[current]`, and emits `make(index)` when
/// another option is picked. `current` is `None` when the value is none of the options.
pub fn select(
    cx: &mut Cx<'_>,
    rect: Rect,
    id: Id,
    spec: SelectSpec<'_>,
    make: impl Fn(usize) -> Action,
) {
    drop_down(cx, rect, id, spec, make, Look::Field, None);
}

/// A drop-down with the small type of the keys, for the narrow selectors of the effect units
/// (`EffectUnit` in `docs/design/DESIGN.md`).
pub fn small_select(
    cx: &mut Cx<'_>,
    rect: Rect,
    id: Id,
    spec: SelectSpec<'_>,
    make: impl Fn(usize) -> Action,
) {
    drop_down(cx, rect, id, spec, make, Look::Small, None);
}

/// How a drop-down is drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Field,
    Small,
    Label,
    /// The title of a panel, which opens the menu of the panel.
    Title,
}

/// The title a menu is drawn as: its text in the `button` style and its ink.
pub struct Title<'a> {
    pub text: &'a str,
    pub ink: Color32,
}

/// A title that opens a menu of `spec.choices` under it with a click or the right button, as
/// the title of an effect unit opens *Save snapshot* (`EffectUnit` in `docs/design/DESIGN.md`).
/// The title is drawn whether the menu is enabled or not; `make(index)` is emitted when an
/// option is picked.
pub fn title_menu(
    cx: &mut Cx<'_>,
    rect: Rect,
    id: Id,
    title: Title<'_>,
    spec: SelectSpec<'_>,
    make: impl Fn(usize) -> Action,
) {
    drop_down(cx, rect, id, spec, make, Look::Title, Some(title));
}

/// The width of the list of a title menu.
const TITLE_LIST_WIDTH: f32 = 220.0;

/// A drop-down drawn as a small label: its text alone, with a fill under the pointer, as the
/// name of the mixer effect under its knob (`MixerEffect` in `docs/design/DESIGN.md`). Its
/// list is wide enough for the options.
pub fn label_select(
    cx: &mut Cx<'_>,
    rect: Rect,
    id: Id,
    spec: SelectSpec<'_>,
    make: impl Fn(usize) -> Action,
) {
    drop_down(cx, rect, id, spec, make, Look::Label, None);
}

/// The width of the list of a label drop-down.
const LABEL_LIST_WIDTH: f32 = 120.0;

fn drop_down(
    cx: &mut Cx<'_>,
    rect: Rect,
    id: Id,
    spec: SelectSpec<'_>,
    make: impl Fn(usize) -> Action,
    look: Look,
    title: Option<Title<'_>>,
) {
    let menu = look == Look::Title;
    let SelectSpec {
        name,
        choices,
        current,
        enabled,
    } = spec;
    let command = make(current.unwrap_or(0)).id();
    cx.record(command, rect, ControlKind::Choice, enabled);
    let sense = cx.sense(enabled, Sense::click());
    let response = cx.ui.interact(rect, id, sense);
    let shown = current
        .and_then(|index| choices.get(index))
        .map_or("", |choice| choice.label);
    response.widget_info(|| {
        let mut info = WidgetInfo::labeled(Role::ComboBox, enabled, name);
        info.current_text_value = Some(shown.to_owned());
        info
    });
    let ctx = cx.ui.ctx().clone();
    let mut open = ctx.data(|data| data.get_temp::<Open>(id));
    let focused = response.has_focus();
    let alt_down = focused
        && cx
            .ui
            .input(|input| input.modifiers.alt && input.key_pressed(Key::ArrowDown));
    if enabled && (response.clicked() || (menu && response.secondary_clicked()) || alt_down) {
        open = match open {
            Some(_) => None,
            None => {
                let highlighted = current.unwrap_or(0);
                Some(Open {
                    highlighted,
                    first: highlighted.saturating_sub(VISIBLE_ROWS / 2),
                })
            }
        };
    }
    if focused {
        cx.out.claim(&[Key::Space, Key::Enter]);
    }
    if enabled && open.is_none() {
        tooltip::tooltip(cx, &response, name, None);
    }

    face(
        cx,
        rect,
        look,
        (title, shown),
        (enabled, response.hovered()),
    );
    let tokens = &cx.theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    if focused {
        paint::focus_ring(painter, cx.theme, rect, tokens.radius.md);
    }

    let Some(mut state) = open else {
        ctx.data_mut(|data| data.remove_temp::<Open>(id));
        return;
    };
    // The list is open: the keyboard moves inside it until a pick or Escape.
    let filter = EventFilter {
        vertical_arrows: true,
        escape: true,
        ..EventFilter::default()
    };
    cx.ui
        .memory_mut(|memory| memory.set_focus_lock_filter(id, filter));
    cx.out.claim(&[Key::ArrowUp, Key::ArrowDown, Key::Escape]);
    let (up, down, pick, close) = cx.ui.input(|input| {
        (
            input.key_pressed(Key::ArrowUp),
            input.key_pressed(Key::ArrowDown) && !input.modifiers.alt,
            input.key_pressed(Key::Enter),
            input.key_pressed(Key::Escape),
        )
    });
    let step = |from: usize, forward: bool| {
        // The next option that can be picked, without wrapping around.
        let mut index = from;
        loop {
            let next = if forward {
                index + 1
            } else {
                index.checked_sub(1)?
            };
            if next >= choices.len() {
                return None;
            }
            index = next;
            if choices[index].enabled {
                return Some(index);
            }
        }
    };
    if down {
        state.highlighted = step(state.highlighted, true).unwrap_or(state.highlighted);
    }
    if up {
        state.highlighted = step(state.highlighted, false).unwrap_or(state.highlighted);
    }
    let mut picked = None;
    if pick
        && focused
        && choices
            .get(state.highlighted)
            .is_some_and(|choice| choice.enabled)
    {
        picked = Some(state.highlighted);
    }

    let visible = choices.len().min(VISIBLE_ROWS);
    let last_first = choices.len() - visible;
    // The highlighted option stays in view, and the wheel scrolls a long list.
    let wheel = cx.ui.input(|input| input.smooth_scroll_delta.y);
    if wheel.abs() > 0.0 {
        let rows = (wheel.abs() / ROW_HEIGHT).ceil().max(1.0) as usize;
        state.first = if wheel > 0.0 {
            state.first.saturating_sub(rows)
        } else {
            state.first + rows
        };
    } else if state.highlighted < state.first {
        state.first = state.highlighted;
    } else if state.highlighted >= state.first + visible {
        state.first = state.highlighted + 1 - visible;
    }
    state.first = state.first.min(last_first);
    let list = Rect::from_min_size(
        rect.left_bottom() + egui::vec2(0.0, tokens.space.s1),
        egui::vec2(
            match look {
                Look::Label => rect.width().max(LABEL_LIST_WIDTH),
                Look::Title => rect.width().max(TITLE_LIST_WIDTH),
                Look::Field | Look::Small => rect.width(),
            },
            ROW_HEIGHT * visible as f32,
        ),
    );
    let mut clicked_inside = false;
    Area::new(id.with("list"))
        .order(Order::Foreground)
        .fixed_pos(list.min)
        .show(&ctx, |ui| {
            ui.allocate_rect(list, Sense::hover());
            let painter = ui.painter();
            painter.rect(
                list,
                paint::radius(tokens.radius.md),
                color.surface_control,
                paint::hairline(cx.theme, color.line_control),
                StrokeKind::Inside,
            );
            for (index, choice) in choices.iter().enumerate().skip(state.first).take(visible) {
                let row = Rect::from_min_size(
                    list.min + egui::vec2(0.0, ROW_HEIGHT * (index - state.first) as f32),
                    egui::vec2(list.width(), ROW_HEIGHT),
                );
                let sense = if choice.enabled {
                    Sense::click()
                } else {
                    Sense::hover()
                };
                let row_response = ui.interact(row, id.with(("row", index)), sense);
                row_response.widget_info(|| {
                    WidgetInfo::selected(
                        Role::ListBoxOption,
                        choice.enabled,
                        Some(index) == current,
                        choice.label,
                    )
                });
                if row_response.clicked() {
                    picked = Some(index);
                    clicked_inside = true;
                }
                let fill = if Some(index) == current {
                    Some(color.surface_selected)
                } else if choice.enabled && (row_response.hovered() || index == state.highlighted) {
                    Some(color.surface_hover)
                } else {
                    None
                };
                if let Some(fill) = fill {
                    painter.rect_filled(row.shrink(1.0), paint::radius(tokens.radius.sm), fill);
                }
                let ink = if choice.enabled {
                    color.text_primary
                } else {
                    color.text_disabled
                };
                let text_rect = row.shrink2(egui::vec2(tokens.space.s3, 0.0));
                paint::text_elided(
                    painter,
                    cx.theme,
                    text_rect,
                    choice.label,
                    tokens.text.body,
                    ink,
                );
            }
        });
    let clicked_outside = !clicked_inside
        && !response.clicked()
        && !response.secondary_clicked()
        && cx.ui.input(|input| input.pointer.any_click())
        && cx
            .ui
            .input(|input| input.pointer.interact_pos())
            .is_some_and(|pos| !list.contains(pos));
    if let Some(index) = picked {
        if Some(index) != current {
            cx.out.emit(make(index));
        }
        ctx.data_mut(|data| data.remove_temp::<Open>(id));
    } else if close || clicked_outside {
        ctx.data_mut(|data| data.remove_temp::<Open>(id));
    } else {
        ctx.data_mut(|data| data.insert_temp(id, state));
    }
}

/// The closed drop-down: a field with its chevron, a label, or the title of a menu.
fn face(
    cx: &Cx<'_>,
    rect: Rect,
    look: Look,
    (title, shown): (Option<Title<'_>>, &str),
    (enabled, hovered): (bool, bool),
) {
    let label = look == Look::Label;
    let menu = look == Look::Title;
    let tokens = &cx.theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    let ink = if enabled {
        color.text_primary
    } else {
        color.text_disabled
    };
    if let Some(title) = title.filter(|_| menu) {
        if hovered && enabled {
            painter.rect_filled(rect, paint::radius(tokens.radius.sm), color.surface_hover);
        }
        paint::text(
            painter,
            cx.theme,
            rect,
            Align2::LEFT_CENTER,
            title.text,
            tokens.text.button,
            title.ink,
        );
    } else if label {
        if hovered && enabled {
            painter.rect_filled(rect, paint::radius(tokens.radius.sm), color.surface_hover);
        }
        let ink = if enabled {
            color.text_secondary
        } else {
            color.text_disabled
        };
        paint::text_elided(painter, cx.theme, rect, shown, tokens.text.label, ink);
    } else {
        let fill = if hovered && enabled {
            color.surface_hover
        } else {
            color.surface_control
        };
        let outline = if enabled {
            color.line_control
        } else {
            color.line_key
        };
        painter.rect(
            rect,
            paint::radius(tokens.radius.md),
            fill,
            paint::hairline(cx.theme, outline),
            StrokeKind::Inside,
        );
        let (padding, glyph, style) = match look {
            Look::Small => (tokens.space.s2, 10.0, tokens.text.label),
            _ => (tokens.space.s3, 14.0, tokens.text.body),
        };
        let chevron = Rect::from_center_size(
            egui::pos2(rect.right() - padding - glyph / 2.0, rect.center().y),
            egui::vec2(glyph, glyph),
        );
        cx.theme
            .icons
            .paint(painter, Glyph::ChevronDown, chevron, ink);
        let text_rect = Rect::from_min_max(
            rect.min + egui::vec2(padding, 0.0),
            egui::pos2(chevron.left() - tokens.space.s1, rect.bottom()),
        );
        paint::text_elided(painter, cx.theme, text_rect, shown, style, ink);
    }
}

/// Draws a segmented control with its left edge at `rect.min` and the height of `rect`;
/// each segment is as wide as its label needs. Emits `make(index)` when another segment is
/// picked. Returns the rectangle the control covers.
pub fn segmented(
    cx: &mut Cx<'_>,
    rect: Rect,
    id: Id,
    choices: &[Choice<'_>],
    current: Option<usize>,
    make: impl Fn(usize) -> Action,
) -> Rect {
    let tokens = &cx.theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    let widths: Vec<f32> = choices
        .iter()
        .map(|choice| {
            let text = paint::text_width(painter, cx.theme, choice.label, tokens.text.button);
            (text + 2.0 * tokens.space.s4).ceil()
        })
        .collect();
    let outer = Rect::from_min_size(
        rect.min,
        egui::vec2(widths.iter().sum::<f32>() + 2.0, rect.height()),
    );
    let mut left = outer.left() + 1.0;
    for (index, (choice, width)) in choices.iter().zip(&widths).enumerate() {
        let segment = Rect::from_min_size(
            egui::pos2(left, outer.top() + 1.0),
            egui::vec2(*width, outer.height() - 2.0),
        );
        left += width;
        let selected = Some(index) == current;
        let action = make(index);
        cx.record(action.id(), segment, ControlKind::Button, choice.enabled);
        let sense = cx.sense(choice.enabled, Sense::click());
        let response = cx.ui.interact(segment, id.with(index), sense);
        response.widget_info(|| {
            WidgetInfo::selected(Role::RadioButton, choice.enabled, selected, choice.label)
        });
        if response.has_focus() {
            cx.out.claim(&[Key::Space, Key::Enter]);
        }
        // Picking the segment that is already selected is passed on too: whoever carries
        // the command out compares it with what stands.
        if choice.enabled {
            tooltip::tooltip(cx, &response, choice.label, Some(&action));
        }
        if response.clicked() {
            cx.out.emit(action);
        }
        let fill = if selected {
            color.surface_pressed
        } else if response.hovered() && choice.enabled {
            color.surface_hover
        } else {
            color.surface_control
        };
        painter.rect_filled(segment, egui::CornerRadius::ZERO, fill);
        if index > 0 {
            painter.vline(
                segment.left(),
                segment.y_range(),
                paint::hairline(cx.theme, color.line_key),
            );
        }
        let ink = if !choice.enabled {
            color.text_disabled
        } else if selected {
            color.text_primary
        } else {
            color.text_secondary
        };
        paint::text(
            painter,
            cx.theme,
            segment,
            Align2::CENTER_CENTER,
            choice.label,
            tokens.text.button,
            ink,
        );
        if response.has_focus() {
            paint::focus_ring(painter, cx.theme, segment, tokens.radius.md);
        }
    }
    painter.rect_stroke(
        outer,
        paint::radius(tokens.radius.md),
        paint::hairline(cx.theme, color.line_key),
        StrokeKind::Inside,
    );
    outer
}
