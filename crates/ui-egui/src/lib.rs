//! The interface of Platter: one dark theme from the design tokens, widgets drawn with the
//! painter, the panels of the main window and the preferences, keyboard shortcuts and
//! translations.
//!
//! The application builds a [`model::View`] every frame and calls [`shell::Shell::show`].
//! The interface draws it and answers with [`command::Action`]s. It holds no state of the
//! engine: what it remembers between frames is only how things are shown.

pub mod collection;
pub mod command;
pub mod covers;
pub mod cx;
pub mod files;
mod format;
pub mod i18n;
pub mod icons;
pub mod keymap;
pub mod keys;
pub mod model;
mod paint;
mod panels;
pub mod played;
pub mod sample;
pub mod shell;
pub mod theme;
pub mod tokens;
mod widgets;

pub use panels::browser::{BrowserMenu, MenuTarget, Naming};
pub use panels::deck::NUDGE_AMOUNT;
