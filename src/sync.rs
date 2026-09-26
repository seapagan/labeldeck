//! Execution of a planned label synchronization.
//!
//! Ordering is defensive: every non-destructive create/update runs to
//! completion before any prune deletion starts, so a failure while adding
//! or fixing labels can never leave the repository missing labels that
//! the canonical set still wants.
//!
//! GitHub's documented best practice is to pause between mutative
//! requests to avoid secondary rate limits, so executions sleep briefly
//! between mutations. This makes large syncs slower and much safer.

use std::thread;
use std::time::Duration;

use crate::github::{GitHubClient, GithubError, RepoSpec};
use crate::plan::Plan;

/// Pause between mutative API calls (GitHub best practice: avoid
/// secondary rate limits such as the 80 content-creating requests/minute
/// ceiling).
const MUTATION_PAUSE: Duration = Duration::from_secs(1);

/// Which phase an execution stopped in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Creating and updating labels.
    CreateUpdate,
    /// Deleting target-only labels (pruning).
    Delete,
}

/// A mutation that was attempted and failed.
#[derive(Debug)]
pub struct SyncFailure {
    pub phase: Phase,
    pub operation: String,
    pub error: GithubError,
}

/// What an execution actually did.
///
/// GitHub's REST API is not transactional: applied operations stay
/// applied. When `failure` is set, `skipped` lists everything that was
/// planned but never attempted, so users see exactly where the run
/// stopped instead of a false claim of rollback.
#[derive(Debug, Default)]
pub struct SyncOutcome {
    /// Operations applied successfully, in execution order.
    pub applied: Vec<String>,
    /// The first failure, if any; execution stops there.
    pub failure: Option<SyncFailure>,
    /// Planned operations never attempted (after the failure).
    pub skipped: Vec<String>,
}

impl SyncOutcome {
    pub fn is_success(&self) -> bool {
        self.failure.is_none()
    }
}

/// Progress notifications emitted during execution.
pub trait Reporter {
    fn operation(&mut self, description: &str);
}

/// A reporter that discards progress; used by callers that only want the
/// final outcome.
impl Reporter for () {
    fn operation(&mut self, _description: &str) {}
}

/// Execute the plan with GitHub's recommended pause between mutations.
pub fn execute(
    client: &GitHubClient,
    repo: &RepoSpec,
    plan: &Plan,
    reporter: impl Reporter,
) -> SyncOutcome {
    execute_with_pause(client, repo, plan, MUTATION_PAUSE, reporter)
}

/// Execute the plan with an explicit inter-mutation pause. Tests pass
/// `Duration::ZERO` to stay fast; production code uses [`execute`].
pub fn execute_with_pause(
    client: &GitHubClient,
    repo: &RepoSpec,
    plan: &Plan,
    pause: Duration,
    mut reporter: impl Reporter,
) -> SyncOutcome {
    let mut outcome = SyncOutcome::default();
    let mut first_mutation = true;

    for label in &plan.creates {
        let description = format!("create label {:?}", label.name);
        reporter.operation(&description);
        if !attempt(
            client.create_label(repo, label),
            &mut first_mutation,
            pause,
            &mut outcome,
            Phase::CreateUpdate,
            description,
        ) {
            skip_remaining_updates(&mut outcome, plan);
            return outcome;
        }
    }

    for update in &plan.updates {
        let description = format!(
            "update label {:?} in place (colour/description)",
            update.current_name
        );
        reporter.operation(&description);
        if !attempt(
            client.update_label(repo, &update.current_name, &update.desired),
            &mut first_mutation,
            pause,
            &mut outcome,
            Phase::CreateUpdate,
            description,
        ) {
            skip_remaining_updates(&mut outcome, plan);
            return outcome;
        }
    }

    for deletion in &plan.deletes {
        let description = format!("delete label {:?}", deletion.name);
        reporter.operation(&description);
        if !attempt(
            client.delete_label(repo, &deletion.name),
            &mut first_mutation,
            pause,
            &mut outcome,
            Phase::Delete,
            description,
        ) {
            let remaining =
                plan.deletes.iter().filter(|d| d.name != deletion.name);
            for skipped in remaining {
                outcome
                    .skipped
                    .push(format!("delete label {:?}", skipped.name));
            }
            return outcome;
        }
    }

    outcome
}

/// Run one mutation, pausing first when it is not the run's first.
/// Returns false when the mutation failed and the outcome was updated.
fn attempt(
    result: Result<(), GithubError>,
    first_mutation: &mut bool,
    pause: Duration,
    outcome: &mut SyncOutcome,
    phase: Phase,
    description: String,
) -> bool {
    if *first_mutation {
        *first_mutation = false;
    } else {
        thread::sleep(pause);
    }
    match result {
        Ok(()) => {
            outcome.applied.push(description);
            true
        }
        Err(error) => {
            outcome.failure = Some(SyncFailure {
                phase,
                operation: description,
                error,
            });
            false
        }
    }
}

/// A create/update failure means the destructive phase must not start;
/// every remaining update and delete is skipped.
fn skip_remaining_updates(outcome: &mut SyncOutcome, plan: &Plan) {
    for update in &plan.updates {
        outcome
            .skipped
            .push(format!("update label {:?}", update.current_name));
    }
    for deletion in &plan.deletes {
        outcome
            .skipped
            .push(format!("delete label {:?}", deletion.name));
    }
}
