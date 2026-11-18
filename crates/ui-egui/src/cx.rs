//! What every widget and panel is handed while a frame is drawn.

use egui::{Id, Key, Modifiers, Painter, Rect, Sense};

use crate::command::Action;
use crate::i18n::Texts;
use crate::keymap::Keymap;
use crate::theme::Theme;

/// How a control is operated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind {
    /// Emits its command when clicked.
    Button,
    /// Emits its command when pressed and again when released.
    Hold,
    /// Emits its command with a new value when dragged.
    Value,
    /// Emits its command when clicked at a point: a waveform or a list.
    Area,
    /// Opens a list and emits its command when an option is picked.
    Choice,
    /// A command the focused control emits for a key, such as the arrows of a list.
    Keys,
}

/// A control drawn in the current frame and the command it emits. The list of them is what
/// lets the tests prove that every control emits its command and that every command can be
/// reached with the keyboard.
#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    pub command: &'static str,
    pub rect: Rect,
    pub kind: ControlKind,
    pub enabled: bool,
    /// The control takes the keyboard focus.
    pub focusable: bool,
}

/// What a frame produces besides the picture.
#[derive(Debug, Default)]
pub struct FrameOutput {
    pub actions: Vec<Action>,
    pub controls: Vec<Control>,
    /// Keys the focused control uses itself; a shortcut on one of them does not fire.
    claimed: Vec<Key>,
}

impl FrameOutput {
    pub fn emit(&mut self, action: impl Into<Action>) {
        self.actions.push(action.into());
    }

    pub fn claim(&mut self, keys: &[Key]) {
        self.claimed.extend_from_slice(keys);
    }

    /// Whether the focused control takes typed text: it keeps the letters for itself.
    pub fn takes_text(&self) -> bool {
        self.claimed.contains(&Key::A)
    }

    /// Whether the focused control keeps `key` for itself. A chord with Control or Alt is
    /// never kept: controls only use plain keys and Shift.
    pub fn is_claimed(&self, key: Key, modifiers: &Modifiers) -> bool {
        !modifiers.command && !modifiers.alt && self.claimed.contains(&key)
    }
}

/// An identifier that is stable from frame to frame for a control named by `salt`.
pub fn id(salt: impl std::hash::Hash + std::fmt::Debug) -> Id {
    Id::unique("platter").with(salt)
}

pub struct Cx<'a> {
    pub ui: &'a egui::Ui,
    pub theme: &'a Theme,
    pub texts: &'a Texts,
    pub out: &'a mut FrameOutput,
    /// False for what lies behind an open dialog: it is drawn and cannot be operated.
    pub interactive: bool,
    /// The shortcuts the tooltips name; `None` while tooltips are off.
    pub help: Option<&'a Keymap>,
}

impl<'a> Cx<'a> {
    pub fn painter(&self) -> &'a Painter {
        self.ui.painter()
    }

    /// The text of a translation key.
    pub fn t(&self, key: &'a str) -> &'a str {
        self.texts.get(key)
    }

    /// The text of a translation key with its placeholders filled.
    pub fn tf(&self, key: &str, values: &[(&str, &str)]) -> String {
        self.texts.format(key, values)
    }

    /// The sense of a control: `sense` when it can be operated, hover alone otherwise.
    pub fn sense(&self, enabled: bool, sense: Sense) -> Sense {
        if enabled && self.interactive {
            sense
        } else {
            Sense::hover()
        }
    }

    /// The same context for another `Ui`, such as the one of a pop-up.
    pub fn with_ui<'b>(&'b mut self, ui: &'b egui::Ui) -> Cx<'b> {
        Cx {
            ui,
            theme: self.theme,
            texts: self.texts,
            out: self.out,
            interactive: self.interactive,
            help: self.help,
        }
    }

    pub fn record(&mut self, command: &'static str, rect: Rect, kind: ControlKind, enabled: bool) {
        self.record_control(Control {
            command,
            rect,
            kind,
            enabled,
            focusable: enabled,
        });
    }

    /// Records a control that only the pointer operates. Its command must be reachable
    /// from the keyboard some other way.
    pub fn record_pointer_only(&mut self, command: &'static str, rect: Rect, enabled: bool) {
        self.record_control(Control {
            command,
            rect,
            kind: ControlKind::Button,
            enabled,
            focusable: false,
        });
    }

    fn record_control(&mut self, control: Control) {
        // A control a scrolled page has moved out of view cannot be operated where it is.
        if self.interactive && self.ui.clip_rect().contains(control.rect.center()) {
            self.out.controls.push(control);
        }
    }
}
