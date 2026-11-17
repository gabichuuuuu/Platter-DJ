//! One-line text fields of Platter's own: the search field of the browser and the field that
//! names a playlist or a folder.
//!
//! Component: `SearchField` in `docs/design/DESIGN.md`. Behaviour notes:
//! `docs/behavior/browser.md`, *Search* and *Playlists*. Text is typed and erased at its end
//! only; there is no cursor to move, no selection and no clipboard.

use egui::{Align2, Event, Id, Key, Rect, Role, Sense, Stroke, StrokeKind, WidgetInfo};

use platter_library::MAX_NAME_CHARS;

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

/// Adds what was typed to the end of `text`, up to `limit` characters in all.
fn type_into(text: &mut String, typed: &str, limit: usize) {
    let room = limit.saturating_sub(text.chars().count());
    text.extend(typed.chars().filter(|c| !c.is_control()).take(room));
}

/// Applies the key and text events of a frame to `text`.
fn edit(text: &str, events: &[Event]) -> FieldInput {
    let mut edited = text.to_owned();
    let mut input = FieldInput::default();
    for event in events {
        match event {
            Event::Text(typed) => type_into(&mut edited, typed, MAX_SEARCH_CHARS),
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
    let placeholder = cx.texts.get("browser.search");
    text_and_caret(cx, area, text, placeholder, focused);
    if focused {
        paint::focus_ring(cx.painter(), theme, rect, tokens.radius.md);
    }
}

/// Draws `text` in `area`, or `placeholder` while it is empty, and the caret at its end
/// while the field has the keyboard. The end of a long text, where typing happens, stays in
/// view.
fn text_and_caret(cx: &Cx<'_>, area: Rect, text: &str, placeholder: &str, focused: bool) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let middle = area.center().y;
    let painter = cx.painter().with_clip_rect(area);
    let caret = if text.is_empty() {
        paint::text(
            &painter,
            theme,
            area,
            Align2::LEFT_CENTER,
            placeholder,
            tokens.text.body,
            color.text_secondary,
        );
        area.left()
    } else {
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
        drawn.right().min(area.right()) + 1.0
    };
    if focused {
        painter.vline(
            caret,
            (middle - 7.0)..=(middle + 7.0),
            Stroke::new(tokens.stroke.hairline, color.text_primary),
        );
    }
}

/// Longest text a field of a form takes, as the library keeps a tag.
const MAX_FORM_CHARS: usize = 1000;

/// Draws a field of a form that edits `text` while it has the keyboard, and records it as a
/// control of `command`, the command that sends the form. Escape and Tab are left to the
/// dialog: one closes it, the other goes to the next field.
pub fn text(
    cx: &mut Cx<'_>,
    rect: Rect,
    id: Id,
    text: &mut String,
    name: &str,
    command: &'static str,
) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    cx.record(command, rect, ControlKind::Keys, true);
    let response = cx.ui.interact(rect, id, cx.sense(true, Sense::click()));
    response.widget_info(|| WidgetInfo::labeled(Role::TextInput, true, name));
    if response.clicked() {
        response.request_focus();
    }
    let focused = response.has_focus();
    if focused {
        let keys: Vec<Key> = Key::ALL
            .iter()
            .copied()
            .filter(|key| !matches!(key, Key::Escape | Key::Tab))
            .collect();
        cx.out.claim(&keys);
        cx.ui.input(|input| {
            for event in &input.events {
                match event {
                    Event::Text(typed) => type_into(text, typed, MAX_FORM_CHARS),
                    Event::Key {
                        key: Key::Backspace,
                        pressed: true,
                        ..
                    } => {
                        text.pop();
                    }
                    _ => {}
                }
            }
        });
    }
    let painter = cx.painter();
    painter.rect(
        rect,
        paint::radius(tokens.radius.md),
        color.bg_app,
        paint::hairline(
            theme,
            if focused {
                color.focus_ring
            } else {
                color.line_control
            },
        ),
        StrokeKind::Inside,
    );
    let area = rect.shrink2(egui::vec2(tokens.space.s3, 0.0));
    text_and_caret(cx, area, text, "", focused);
}

/// How the user left a name field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Finish {
    /// Enter with a name: the name, without the spaces around it.
    Confirm(String),
    /// Escape, Enter with no name, or a press elsewhere.
    Cancel,
}

/// Applies the key and text events of a frame to the text of a name field.
fn edit_name(text: &mut String, events: &[Event]) -> Option<Finish> {
    for event in events {
        match event {
            Event::Text(typed) => type_into(text, typed, MAX_NAME_CHARS),
            Event::Key {
                key, pressed: true, ..
            } => match key {
                Key::Backspace => {
                    text.pop();
                }
                Key::Escape => return Some(Finish::Cancel),
                Key::Enter => {
                    let name = text.trim();
                    return Some(if name.is_empty() {
                        Finish::Cancel
                    } else {
                        Finish::Confirm(name.to_owned())
                    });
                }
                _ => {}
            },
            _ => {}
        }
    }
    None
}

/// The text of a name field from frame to frame, and whether it has had the keyboard.
#[derive(Clone, Default)]
struct Naming {
    text: String,
    focused: bool,
}

/// Draws a field that names a playlist or a folder, starting from `initial`, and records it
/// as the control that sends `command`. It takes the keyboard when it appears; the caller
/// sends the command when it answers [`Finish::Confirm`].
pub fn name(
    cx: &mut Cx<'_>,
    rect: Rect,
    id: Id,
    initial: &str,
    placeholder: &str,
    command: &'static str,
) -> Option<Finish> {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    cx.record(command, rect, ControlKind::Keys, true);
    let response = cx.ui.interact(rect, id, cx.sense(true, Sense::click()));
    response.widget_info(|| WidgetInfo::labeled(Role::TextInput, true, placeholder));
    let ctx = cx.ui.ctx().clone();
    let mut state = ctx
        .data(|data| data.get_temp::<Naming>(id))
        .unwrap_or_else(|| Naming {
            text: initial.to_owned(),
            focused: false,
        });
    let focused = response.has_focus();
    let finish = if focused {
        state.focused = true;
        // Every plain key types or edits; shortcuts with Ctrl or Alt still work.
        cx.out.claim(Key::ALL);
        cx.ui
            .input(|input| edit_name(&mut state.text, &input.events))
    } else if state.focused {
        // The keyboard went elsewhere, by a press outside or Tab.
        Some(Finish::Cancel)
    } else {
        response.request_focus();
        None
    };
    if finish.is_some() {
        ctx.data_mut(|data| data.remove_temp::<Naming>(id));
    } else {
        ctx.data_mut(|data| data.insert_temp(id, state.clone()));
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
    let area = rect.shrink2(egui::vec2(tokens.space.s2, 0.0));
    text_and_caret(cx, area, &state.text, placeholder, focused);
    if focused {
        paint::focus_ring(cx.painter(), theme, rect, tokens.radius.md);
    }
    finish
}

#[cfg(test)]
mod tests {
    use egui::{Event, Key, Modifiers};

    use super::{FieldInput, Finish, MAX_NAME_CHARS, MAX_SEARCH_CHARS, edit, edit_name};

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

    #[test]
    fn enter_confirms_a_name_without_the_spaces_around_it() {
        let mut text = "Old".to_owned();
        let finish = edit_name(
            &mut text,
            &[
                key(Key::Backspace),
                key(Key::Backspace),
                key(Key::Backspace),
                Event::Text(" Friday ".to_owned()),
                key(Key::Enter),
            ],
        );
        assert_eq!(finish, Some(Finish::Confirm("Friday".to_owned())));
    }

    #[test]
    fn escape_or_enter_without_a_name_cancels_and_typing_alone_does_not_finish() {
        let mut text = String::from("Set");
        assert_eq!(
            edit_name(&mut text, &[key(Key::Escape)]),
            Some(Finish::Cancel)
        );
        let mut blank = String::from("  ");
        assert_eq!(
            edit_name(&mut blank, &[key(Key::Enter)]),
            Some(Finish::Cancel)
        );
        let mut typing = String::new();
        assert_eq!(
            edit_name(&mut typing, &[Event::Text("a".to_owned()), key(Key::Tab)]),
            None
        );
        assert_eq!(typing, "a");
    }

    #[test]
    fn a_name_stops_at_the_longest_the_library_keeps() {
        let mut text = "x".repeat(MAX_NAME_CHARS);
        edit_name(&mut text, &[Event::Text("y".to_owned())]);
        assert_eq!(text.chars().count(), MAX_NAME_CHARS);
    }
}

/// What [`typing`] did this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Typing {
    /// Not typing: the caller draws its own control.
    Idle,
    /// The field is open and drawn.
    Open,
    /// Enter confirmed this text.
    Confirmed(String),
}

/// Turns a control into a text field while the user types into it: a double click on
/// `rect`, or Enter while the control `control` has the keyboard, opens it with `initial`;
/// Enter confirms, Escape or a press elsewhere closes it. Recorded as the control that sends
/// `command` while it is open.
pub fn typing(
    cx: &mut Cx<'_>,
    rect: Rect,
    control: Id,
    initial: &str,
    name: &str,
    command: &'static str,
) -> Typing {
    let open_id = control.with("typing");
    let ctx = cx.ui.ctx().clone();
    let open = ctx
        .data(|data| data.get_temp::<bool>(open_id))
        .unwrap_or(false);
    if !open {
        let double = cx.interactive
            && cx.ui.input(|input| {
                input
                    .pointer
                    .button_double_clicked(egui::PointerButton::Primary)
                    && input
                        .pointer
                        .interact_pos()
                        .is_some_and(|pos| rect.contains(pos))
            });
        let enter = cx.interactive
            && cx.ui.memory(|memory| memory.has_focus(control))
            && cx.ui.input(|input| input.key_pressed(Key::Enter));
        if !(double || enter) {
            return Typing::Idle;
        }
        ctx.data_mut(|data| data.insert_temp(open_id, true));
    }
    match self::name(cx, rect, open_id.with("field"), initial, name, command) {
        Some(Finish::Confirm(text)) => {
            ctx.data_mut(|data| data.remove_temp::<bool>(open_id));
            Typing::Confirmed(text)
        }
        Some(Finish::Cancel) => {
            ctx.data_mut(|data| data.remove_temp::<bool>(open_id));
            Typing::Idle
        }
        None => Typing::Open,
    }
}
