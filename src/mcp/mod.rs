//! MCP protocol surface for Zeuxis tools, errors, and structured results.
//!
//! `ZeuxisScreenshotServer` owns the rmcp tool router while sibling modules keep
//! client-facing schemas, result payloads, and stable error codes in one place.

pub mod errors;
pub mod result;
pub mod server;
pub mod tools;

pub use server::ZeuxisScreenshotServer;
