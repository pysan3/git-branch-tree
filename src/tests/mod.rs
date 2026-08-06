//! Unit tests for the crate's internals.
//!
//! These live inside the crate because integration tests are separate crates and would
//! force every type they touch to be `pub`. One file per area, kept out of the module
//! sources so those stay readable.

mod base;
mod blame;
mod contract_gt;
mod deps_ancestry;
mod deps_content;
mod gitx;
mod model;
mod patchid;
mod render;
