//! Minimal MCP server discovery/enumeration utilities and shared types.
//!
//! This module focuses on stdio transports for Claude/VSCode/Cursor config
//! formats and intentionally omits HTTP for now to keep surface area small.

pub mod discovery;
pub mod enumerator;
pub mod types;

pub use discovery::*;
pub use types::*;
