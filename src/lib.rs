//! `labeldeck` library crate.
//!
//! The binary in `main.rs` is a thin shell over this library so tests can
//! exercise planning, parsing, and API behaviour directly.

pub mod auth;
pub mod canonical;
pub mod config;
pub mod github;
pub mod labels;
pub mod plan;
pub mod sync;
