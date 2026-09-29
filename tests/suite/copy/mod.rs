//! Integration tests for `labeldeck copy` against the local mock GitHub
//! API, split by behaviour: argument and configuration handling
//! (`cli`), successful reconciliation (`behaviour`), and failure,
//! safety, and authentication guarantees (`safety`). No test in this
//! module contacts GitHub.

mod behaviour;
mod cli;
mod safety;

use std::process::Command;

use crate::common::{Expectation, MockGitHub};

const SOURCE: &str = "seapagan/template";
const TARGET: &str = "some-org/project";

/// The list-labels request path for a repository.
fn labels_path(repo: &str) -> String {
    format!("/repos/{repo}/labels?per_page=100")
}

/// A single-page labels listing for a repository.
fn page(repo: &str, labels: &[(&str, &str, Option<&str>)]) -> Expectation {
    Expectation::get(&labels_path(repo))
        .labels_page(&crate::common::labels_json(labels), None)
}

/// The mutation base path for a repository's labels.
fn labels_base(repo: &str) -> String {
    format!("/repos/{repo}/labels")
}

/// The safety invariant: only reads may touch the source, and every
/// mutation must address the target repository's labels.
fn assert_only_target_is_mutated(mock: &MockGitHub) {
    for request in mock.requests() {
        if request.method == "GET" {
            continue;
        }
        assert!(
            request.path.starts_with(&labels_base(TARGET)),
            "mutation escaped the target repository: {} {}",
            request.method,
            request.path
        );
    }
}

/// Provide the token a normal (mutating) copy authenticates with.
fn with_token(command: &mut Command) {
    command.env("LABELDECK_TOKEN", "gh_test_token");
}
