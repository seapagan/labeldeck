//! Single integration-test binary.
//!
//! All mock-server-based integration tests compile as modules of one
//! binary so the shared `common` harness is fully used in every
//! compilation.

mod common;
mod suite;
