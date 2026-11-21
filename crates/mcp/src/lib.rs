//! The MCP server of Platter: an assistant that speaks the Model Context Protocol drives the
//! application through its commands. Behaviour note: `docs/behavior/mcp.md`; the choices:
//! `docs/decisions/0037-mcp-server.md`.
//!
//! The crate reads and writes the messages and answers them; the application gives it its
//! commands and its state through [`Host`] and carries the messages on its standard input and
//! output.

pub mod json;
mod server;

pub use server::{Host, LEGACY_VERSION, PROTOCOL_VERSION, Server};
