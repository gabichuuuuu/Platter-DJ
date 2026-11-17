//! A context menu: the commands that act on one row, in a list that opens where the user
//! asked for it, with the right button or with Shift+F10.
//!
//! It is drawn above everything else and takes the keyboard while it is open: the arrows
//! move through it, Enter operates the item and Escape closes it. A press outside closes it
//! and still reaches what it lands on. Behaviour note: `docs/behavior/browser.md`,
//! *Playlists*.

use egui::{Area, EventFilter, Id, Key, Order, Pos2, Rect, Role, Sense, StrokeKind, WidgetInfo};

use crate::command::Action;
use crate::cx::{ControlKind, Cx};
use crate::paint;

/// Height of one item.
pub const ROW_HEIGHT: f32 = 28.0;
const MIN_WIDTH: f32 = 180.0;

pub struct Item<'a> {
    pub label: &'a str,
    /// The command the item sends; `None` for an item that only changes what the window
    /// shows, such as one that opens a field, which its caller handles.
    pub action: Option<Action>,
}

/// What became of an open menu in one frame.
pub struct Outcome {
    pub open: bool,
    /// The item operated, if one was.
    pub picked: Option<usize>,
}

/// The item the keyboard is on, kept from frame to frame while the menu is open.
#[derive(Clone, Copy, Default)]
struct Highlight(usize);

/// Where a menu of `count` items whose widest label is `width` goes when asked for at `at`:
/// below and right of it, moved inside `screen` when it would leave it.
fn place(at: Pos2, count: usize, width: f32, screen: Rect) -> Rect {
    let size = egui::vec2(width, ROW_HEIGHT * count as f32);
    let x = at.x.min(screen.right() - size.x).max(screen.left());
    let y = if at.y + size.y > screen.bottom() {
        (at.y - size.y).max(screen.top())
    } else {
        at.y
    };
    Rect::from_min_size(egui::pos2(x, y), size)
}

/// Draws the menu with `items`, asked for at `at`, and emits the action of the item
/// operated. On closing, the keyboard goes back to `back`, the control the menu was opened
/// from.
pub fn menu(cx: &mut Cx<'_>, id: Id, at: Pos2, items: &[Item<'_>], back: Id) -> Outcome {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let ctx = cx.ui.ctx().clone();
    let painter = cx.painter();
    let widest = items
        .iter()
        .map(|item| paint::text_width(painter, theme, item.label, tokens.text.body))
        .fold(0.0, f32::max);
    let width = (widest + 2.0 * tokens.space.s3).max(MIN_WIDTH);
    let rect = place(at, items.len(), width, ctx.content_rect());
    let opening = ctx.data(|data| data.get_temp::<Highlight>(id)).is_none();
    let mut highlight = ctx
        .data(|data| data.get_temp::<Highlight>(id))
        .map_or(0, |Highlight(index)| {
            index.min(items.len().saturating_sub(1))
        });

    // What the menu covers cannot be operated while it is open.
    cx.out
        .controls
        .retain(|control| !rect.intersects(control.rect));
    let rows: Vec<Rect> = (0..items.len())
        .map(|index| {
            Rect::from_min_size(
                rect.min + egui::vec2(0.0, ROW_HEIGHT * index as f32),
                egui::vec2(rect.width(), ROW_HEIGHT),
            )
        })
        .collect();
    for (item, row) in items.iter().zip(&rows) {
        if let Some(action) = &item.action {
            cx.record(action.id(), *row, ControlKind::Button, true);
        }
    }

    let focus = id.with("keys");
    let has_focus = cx.ui.memory(|memory| memory.has_focus(focus));
    // The click that opened the menu takes the focus away from it in that frame, so the
    // menu asks for it until it has it.
    if !has_focus {
        cx.ui.memory_mut(|memory| memory.request_focus(focus));
    }
    // Escape closes the menu even before it has the keyboard.
    cx.out.claim(&[Key::Escape]);
    let escape = cx.ui.input(|input| input.key_pressed(Key::Escape));
    let (up, down, enter) = if has_focus {
        let filter = EventFilter {
            vertical_arrows: true,
            escape: true,
            tab: true,
            ..EventFilter::default()
        };
        cx.ui
            .memory_mut(|memory| memory.set_focus_lock_filter(focus, filter));
        cx.out.claim(&[
            Key::ArrowUp,
            Key::ArrowDown,
            Key::Enter,
            Key::Space,
            Key::Tab,
        ]);
        cx.ui.input(|input| {
            (
                input.key_pressed(Key::ArrowUp),
                input.key_pressed(Key::ArrowDown),
                input.key_pressed(Key::Enter) || input.key_pressed(Key::Space),
            )
        })
    } else {
        (false, false, false)
    };
    if down {
        highlight = (highlight + 1).min(items.len().saturating_sub(1));
    }
    if up {
        highlight = highlight.saturating_sub(1);
    }
    let mut picked = enter.then_some(highlight);

    Area::new(id)
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .show(&ctx, |ui| {
            let frame = ui.interact(rect, focus, Sense::focusable_noninteractive());
            frame.widget_info(|| WidgetInfo::labeled(Role::Menu, true, ""));
            let painter = ui.painter();
            painter.rect(
                rect,
                paint::radius(tokens.radius.md),
                color.surface_control,
                paint::hairline(theme, color.line_control),
                StrokeKind::Inside,
            );
            for (index, (item, row)) in items.iter().zip(&rows).enumerate() {
                let response = ui.interact(*row, id.with(("item", index)), Sense::click());
                response.widget_info(|| WidgetInfo::labeled(Role::MenuItem, true, item.label));
                // The pointer takes the highlight when it moves over an item; resting where the
                // menu opened, it leaves the arrows in charge.
                if response.hovered() && ui.input(|input| input.pointer.delta() != egui::Vec2::ZERO)
                {
                    highlight = index;
                }
                if response.clicked() {
                    picked = Some(index);
                }
                if index == highlight {
                    painter.rect_filled(
                        row.shrink(1.0),
                        paint::radius(tokens.radius.sm),
                        color.surface_hover,
                    );
                }
                paint::text_elided(
                    painter,
                    theme,
                    row.shrink2(egui::vec2(tokens.space.s3, 0.0)),
                    item.label,
                    tokens.text.body,
                    color.text_primary,
                );
            }
        });
    let pressed_outside = !opening
        && cx.ui.input(|input| {
            input.pointer.any_pressed()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|pos| !rect.contains(pos))
        });
    if let Some(action) = picked
        .and_then(|index| items.get(index))
        .and_then(|item| item.action.clone())
    {
        cx.out.emit(action);
    }
    let open = picked.is_none() && !escape && !pressed_outside;
    if open {
        ctx.data_mut(|data| data.insert_temp(id, Highlight(highlight)));
    } else {
        ctx.data_mut(|data| data.remove_temp::<Highlight>(id));
        if has_focus {
            cx.ui.memory_mut(|memory| memory.request_focus(back));
        }
    }
    Outcome { open, picked }
}

#[cfg(test)]
mod tests {
    use egui::{Pos2, Rect};

    use super::{ROW_HEIGHT, place};

    #[test]
    fn a_menu_stays_inside_the_screen() {
        let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 800.0));
        let below = place(egui::pos2(100.0, 100.0), 3, 200.0, screen);
        assert_eq!(below.min, egui::pos2(100.0, 100.0));
        // Near the bottom right corner it opens above and to the left.
        let corner = place(egui::pos2(950.0, 780.0), 3, 200.0, screen);
        assert_eq!(corner.max.x, 1000.0);
        assert_eq!(corner.max.y, 780.0);
        assert_eq!(corner.height(), 3.0 * ROW_HEIGHT);
        assert!(screen.contains_rect(corner));
    }
}
