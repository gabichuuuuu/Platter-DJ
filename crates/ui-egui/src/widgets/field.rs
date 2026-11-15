//! A one-line text field of Platter's own: the search field of the browser.
//!
//! Component: `SearchField` in `docs/design/DESIGN.md`. Behaviour note:
//! `docs/behavior/browser.md`, *Search*. Text is typed and erased at its end only; there is
//! no cursor to move, no selection and no clipboard.

use egui::{Align2, Event, Id, Key, Rect, Role, Sense, Stroke, StrokeKind, WidgetInfo};

use crate::command::{AppCommand, MAX_SEARCH_CHARS};
use crate::cx::{ControlKind, Cx};
use crate::icons::Glyph;
use crate::paint;

/// What the user did to the field in this frame.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct FieldInput {
    /// The whole text after the keys of the frame, when it changed.
    pub text: Option<String>,
    /// The field gave the keyboard away, to the list beside it.
    pub leave: bool,
}

/// Applies the key and text events of a frame to `text`.
fn edit(text: &str, events: &[Event]) -> FieldInput {
    let mut edited = text.to_owned();
    let mut input = FieldInput::default();
    for event in events {
        match event {
            Event::Text(typed) => {
                let room = MAX_SEARCH_CHARS.saturating_sub(edited.chars().count());
                edited.extend(typed.chars().filter(|c| !c.is_control()).take(room));
            }
            Event::Key {
                key, pressed: true, ..
            } => match key {
                Key::Backspace => {
                    edited.pop();
                }
                Key::Escape => {
                    edited.clear();
                    input.leave = true;
                }
                Key::ArrowUp | Key::ArrowDown | Key::Enter | Key::Tab => input.leave = true,
                _ => {}
            },
            _ => {}
        }
    }
    if edited != text {
        input.text = Some(edited);
    }
    input
}

/// Draws the search field holding `text` and emits `browser.search.set` when the text
/// changes. `focus` gives it the keyboard in this frame; `list` is the control the keyboard
/// goes to when the field gives it away.
pub fn search(cx: &mut Cx<'_>, rect: Rect, id: Id, text: &str, focus: bool, list: Id) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let name = cx.texts.get("browser.search");
    let command = AppCommand::BrowserSearchSet {
        text: String::new(),
    };
    // Typing is what operates it; the keyboard reaches it with a click, Tab or Ctrl+F.
    cx.record(command.id(), rect, ControlKind::Keys, true);
    let response = cx.ui.interact(rect, id, cx.sense(true, Sense::click()));
    response.widget_info(|| WidgetInfo::labeled(Role::SearchInput, true, name));
    if (focus || response.clicked()) && cx.interactive {
        response.request_focus();
    }
    let focused = response.has_focus();
    if focused {
        // Every plain key types or edits; shortcuts with Ctrl or Alt still work.
        cx.out.claim(Key::ALL);
        let input = cx.ui.input(|input| edit(text, &input.events));
        if let Some(edited) = input.text {
            cx.out.emit(AppCommand::BrowserSearchSet { text: edited });
        }
        if input.leave {
            cx.ui.memory_mut(|memory| memory.request_focus(list));
        }
    }

    let painter = cx.painter();
    let corner = paint::radius(tokens.radius.md);
    painter.rect_filled(rect, corner, color.bg_app);
    painter.rect_stroke(
        rect,
        corner,
        Stroke::new(tokens.stroke.hairline, color.line_control),
        StrokeKind::Inside,
    );
    let middle = rect.center().y;
    let icon = Rect::from_center_size(
        egui::pos2(rect.left() + tokens.space.s3 + 7.0, middle),
        egui::vec2(14.0, 14.0),
    );
    theme
        .icons
        .paint(painter, Glyph::Search, icon, color.text_secondary);
    let mut right = rect.right() - tokens.space.s3;
    if !text.is_empty() {
        let clear = Rect::from_center_size(egui::pos2(right - 7.0, middle), egui::vec2(14.0, 14.0));
        let hit = clear.expand(5.0);
        cx.record_pointer_only(command.id(), hit, true);
        let clicked = cx
            .ui
            .interact(hit, id.with("clear"), cx.sense(true, Sense::click()))
            .clicked();
        if clicked {
            cx.out.emit(command);
        }
        theme
            .icons
            .paint(cx.painter(), Glyph::Close, clear, color.text_secondary);
        right = hit.left();
    }
    let area = Rect::from_min_max(
        egui::pos2(icon.right() + tokens.space.s2 + 2.0, rect.top()),
        egui::pos2(right - tokens.space.s1, rect.bottom()),
    );
    let painter = cx.painter().with_clip_rect(area);
    if text.is_empty() {
        paint::text(
            &painter,
            theme,
            area,
            Align2::LEFT_CENTER,
            cx.texts.get("browser.search"),
            tokens.text.body,
            color.text_secondary,
        );
    } else {
        // The end of a long text, where typing happens, stays in view.
        let width = paint::text_width(&painter, theme, text, tokens.text.body);
        let anchor = if width > area.width() {
            Align2::RIGHT_CENTER
        } else {
            Align2::LEFT_CENTER
        };
        let drawn = paint::text(
            &painter,
            theme,
            area,
            anchor,
            text,
            tokens.text.body,
            color.text_primary,
        );
        if focused {
            let x = drawn.right().min(area.right()) + 1.0;
            painter.vline(
                x,
                (middle - 7.0)..=(middle + 7.0),
                Stroke::new(tokens.stroke.hairline, color.text_primary),
            );
        }
    }
    if focused && text.is_empty() {
        painter.vline(
            area.left(),
            (middle - 7.0)..=(middle + 7.0),
            Stroke::new(tokens.stroke.hairline, color.text_primary),
        );
    }
    if focused {
        paint::focus_ring(cx.painter(), theme, rect, tokens.radius.md);
    }
}

#[cfg(test)]
mod tests {
    use egui::{Event, Key, Modifiers};

    use super::{FieldInput, MAX_SEARCH_CHARS, edit};

    fn key(key: Key) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn typed_text_goes_at_the_end_and_backspace_takes_the_last_character() {
        let typed = edit("ab", &[Event::Text("cé".to_owned()), key(Key::Backspace)]);
        assert_eq!(
            typed,
            FieldInput {
                text: Some("abc".to_owned()),
                leave: false
            }
        );
        assert_eq!(edit("", &[key(Key::Backspace)]), FieldInput::default());
    }

    #[test]
    fn escape_empties_the_field_and_the_arrows_leave_it() {
        let escaped = edit("house", &[key(Key::Escape)]);
        assert_eq!(escaped.text.as_deref(), Some(""));
        assert!(escaped.leave);
        let down = edit("house", &[key(Key::ArrowDown)]);
        assert_eq!(down.text, None);
        assert!(down.leave);
    }

    #[test]
    fn the_text_stops_at_its_longest_and_control_characters_are_dropped() {
        let long = "x".repeat(MAX_SEARCH_CHARS - 1);
        let typed = edit(&long, &[Event::Text("ab\u{7}".to_owned())]);
        assert_eq!(typed.text.unwrap().chars().count(), MAX_SEARCH_CHARS);
        let bell = edit("", &[Event::Text("\u{7}".to_owned())]);
        assert_eq!(bell.text, None);
    }
}
