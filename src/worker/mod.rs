//! Subprocess capture worker namespace.
//!
//! Worker mode isolates blocking desktop capture in a hidden `__worker`
//! process. The parent owns timeouts and adoption; the child owns capture and
//! encoded artifact writes.

pub mod child;
pub mod contract;
pub mod parent;
