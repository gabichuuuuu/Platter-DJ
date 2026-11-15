//! The controls of the interface. Each one draws itself with the painter from the design
//! tokens, reports itself to assistive technology and emits a command; none keeps state of
//! the engine.

pub mod choice;
pub mod field;
pub mod key;
pub mod meter;
pub mod tooltip;
pub mod value;
