//! `labeldeck` library crate.
//!
//! The binary in `main.rs` is a thin shell over this library so tests can
//! exercise planning, parsing, and API behaviour directly.

pub mod auth;
pub mod canonical;
pub mod cli;
pub mod commands;
pub mod config;
pub mod deck;
pub mod error;
pub mod github;
pub mod labels;
pub mod plan;
pub(crate) mod staged_write;
pub mod sync;
