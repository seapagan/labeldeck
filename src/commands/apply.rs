//! Shared application of a desired label set to a target repository.
//!
//! `sync` and `copy` differ only in where the desired label set comes
//! from (a canonical deck file versus another GitHub repository); both
//! then reconcile it against the target's current labels through this
//! module: planning, prune warnings, dry-run reporting, execution, and
//! failure accounting. Mutation ordering (creates and updates before
//! prune deletions) lives in the sync engine both commands share.

use crate::error::Result;
use crate::github::{GitHubClient, RepoSpec};
use crate::labels::Label;
use crate::plan;
use crate::sync as sync_engine;

/// Report what applying `desired` against `remote` would change,
/// without mutating anything — the `--dry-run` path shared by `sync`
/// and `copy`.
///
/// Both the desired set and the target's current labels must already
/// have been fetched. `follow_up` is the command-specific "run again
/// without --dry-run" guidance line, because each command re-runs
/// differently. Returns the process exit code.
pub(crate) fn dry_run(
    desired: &[Label],
    remote: &[Label],
    prune: bool,
    follow_up: &str,
) -> Result<i32> {
    let result = plan::plan(desired, remote, prune);
    eprintln!("Dry run: no changes were made.");
    crate::commands::diff::print_plan(&result);
    eprintln!("{}", crate::commands::diff::summarize(&result));
    eprintln!("{follow_up}");
    Ok(0)
}

/// Apply `desired` to `target` against its current `remote` labels —
/// the mutating path shared by `sync` and `copy`.
///
/// Warns before prune deletions, executes the plan (every create and
/// update before any deletion, paced per GitHub's rate-limit
/// guidance), and reports success or the existing partial-failure
/// accounting. `success_line` prefixes the success summary and already
/// names whatever the command synchronized or copied. Returns the
/// process exit code.
pub(crate) fn execute(
    client: &GitHubClient,
    target: &RepoSpec,
    desired: &[Label],
    remote: &[Label],
    prune: bool,
    success_line: &str,
) -> Result<i32> {
    let result = plan::plan(desired, remote, prune);

    execute_plan(client, target, &result, success_line)
}

/// Execute exactly the reviewed operations, preserving ordering and accounting.
pub(crate) fn execute_plan(
    client: &GitHubClient,
    target: &RepoSpec,
    result: &plan::Plan,
    success_line: &str,
) -> Result<i32> {
    if !result.deletes.is_empty() {
        eprintln!(
            "warning: pruning will DELETE {} target-only label(s); \
             deleting a label removes it from existing issues and pull \
             requests.",
            result.deletes.len()
        );
    }

    let mut printer = StderrReporter;
    let outcome = sync_engine::execute(client, target, result, &mut printer);

    if outcome.is_success() {
        eprintln!(
            "{success_line}: {} created, {} updated, {} deleted.",
            result.creates.len(),
            result.updates.len(),
            result.deletes.len(),
        );
        return Ok(0);
    }

    let failure = outcome.failure.as_ref().expect("failure implies Some");
    for applied in &outcome.applied {
        eprintln!("applied: {applied}");
    }
    eprintln!("failed:   {}", failure.operation);
    eprintln!("reason:   {}", failure.error);
    for skipped in &outcome.skipped {
        eprintln!("skipped:  {skipped}");
    }
    eprintln!(
        "GitHub does not support transactional label updates; the \
         operations listed as applied remain in effect."
    );
    Ok(2)
}

struct StderrReporter;

impl sync_engine::Reporter for StderrReporter {
    fn operation(&mut self, description: &str) {
        eprintln!("labeldeck: {description}...");
    }
}

#[cfg(test)]
mod tests;
