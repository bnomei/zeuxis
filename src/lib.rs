//! Library surface for Zeuxis, a local MCP screenshot server.
//!
//! The crate separates capture adapters, MCP tool schemas, artifact storage,
//! platform permission gates, and subprocess worker supervision so embedders can
//! replace process or filesystem boundaries in tests.

pub mod capture;
pub mod cursor;
pub mod mcp;
pub mod platform;
pub mod runtime_config;
pub mod storage;
pub mod worker;
