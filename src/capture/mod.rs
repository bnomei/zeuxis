//! Screenshot capture boundary, coordinate math, and the production xcap adapter.
//!
//! Capture backends return images and metadata in logical desktop coordinates;
//! MCP tools and worker code layer validation, storage, and result payloads on
//! top of this boundary.

pub mod backend;
pub mod region;
pub mod xcap_backend;
