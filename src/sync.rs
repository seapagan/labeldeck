//! Execution of a planned label synchronization.
//!
//! Ordering is defensive: every non-destructive create/update runs to
//! completion before any prune deletion starts, so a failure while adding
//! or fixing labels can never leave the repository missing labels that
//! the canonical set still wants.
//!
//! GitHub's documented best practice is to pause between mutative
//! requests to avoid secondary rate limits, so executions wait briefly
//! before every mutation except the first. This makes large syncs slower
//! and much safer.

use std::thread;
use std::time::Duration;

use crate::github::{GitHubClient, GithubError, RepoSpec};
use crate::labels::Label;
use crate::plan::{Deletion, Plan, Update};

/// Pause before each mutative API call except the first (GitHub best
/// practice: avoid secondary rate limits such as the 80 content-creating
/// requests/minute ceiling).
pub(crate) const MUTATION_PAUSE: Duration = Duration::from_secs(1);

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
/// applied. The three lists partition every planned mutation exactly:
/// an operation appears in `applied` (succeeded), in `failure` (the one
/// operation that was attempted and failed; execution stops there), or
/// in `skipped` (never attempted because execution had already stopped).
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

impl<R: Reporter> Reporter for &mut R {
    fn operation(&mut self, description: &str) {
        (**self).operation(description);
    }
}

/// How long to wait before the mutation with this zero-based, run-wide
/// index: the first mutation of the run is issued immediately, every
/// later one waits `pause`.
///
/// Extracted as a pure function so pacing can be tested without real
/// sleeps.
pub fn delay_before_mutation(index: usize, pause: Duration) -> Duration {
    if index == 0 { Duration::ZERO } else { pause }
}

/// Issues the pacing waits for one execution run.
struct Pacer {
    next_index: usize,
    pause: Duration,
}

impl Pacer {
    fn new(pause: Duration) -> Self {
        Self {
            next_index: 0,
            pause,
        }
    }

    /// Wait (if required) immediately *before* a mutation is issued.
    fn wait(&mut self) {
        thread::sleep(delay_before_mutation(self.next_index, self.pause));
        self.next_index += 1;
    }
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
    let mut pacer = Pacer::new(pause);

    for (index, label) in plan.creates.iter().enumerate() {
        let description = create_description(label);
        reporter.operation(&description);
        pacer.wait();
        match client.create_label(repo, label) {
            Ok(()) => outcome.applied.push(description),
            Err(error) => {
                record_failure(
                    &mut outcome,
                    Phase::CreateUpdate,
                    description,
                    error,
                );
                // A create/update failure means the destructive phase
                // must not start; every remaining operation is skipped.
                outcome.skipped.extend(
                    plan.creates[index + 1..].iter().map(create_description),
                );
                outcome
                    .skipped
                    .extend(plan.updates.iter().map(update_description));
                outcome
                    .skipped
                    .extend(plan.deletes.iter().map(delete_description));
                return outcome;
            }
        }
    }

    for (index, update) in plan.updates.iter().enumerate() {
        let description = update_description(update);
        reporter.operation(&description);
        pacer.wait();
        match client.update_label(repo, update.current_name(), &update.desired)
        {
            Ok(()) => outcome.applied.push(description),
            Err(error) => {
                record_failure(
                    &mut outcome,
                    Phase::CreateUpdate,
                    description,
                    error,
                );
                outcome.skipped.extend(
                    plan.updates[index + 1..].iter().map(update_description),
                );
                outcome
                    .skipped
                    .extend(plan.deletes.iter().map(delete_description));
                return outcome;
            }
        }
    }

    for (index, deletion) in plan.deletes.iter().enumerate() {
        let description = delete_description(deletion);
        reporter.operation(&description);
        pacer.wait();
        match client.delete_label(repo, &deletion.name) {
            Ok(()) => outcome.applied.push(description),
            Err(error) => {
                record_failure(
                    &mut outcome,
                    Phase::Delete,
                    description,
                    error,
                );
                outcome.skipped.extend(
                    plan.deletes[index + 1..].iter().map(delete_description),
                );
                return outcome;
            }
        }
    }

    outcome
}

fn record_failure(
    outcome: &mut SyncOutcome,
    phase: Phase,
    operation: String,
    error: GithubError,
) {
    outcome.failure = Some(SyncFailure {
        phase,
        operation,
        error,
    });
}

fn create_description(label: &Label) -> String {
    format!("create label {:?}", label.name)
}

fn update_description(update: &Update) -> String {
    format!(
        "update label {:?} in place (colour/description)",
        update.current_name()
    )
}

fn delete_description(deletion: &Deletion) -> String {
    format!("delete label {:?}", deletion.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_mutation_is_immediate_later_ones_wait() {
        assert_eq!(
            delay_before_mutation(0, Duration::from_secs(1)),
            Duration::ZERO
        );
        assert_eq!(
            delay_before_mutation(1, Duration::from_secs(1)),
            Duration::from_secs(1)
        );
        assert_eq!(
            delay_before_mutation(9, Duration::from_secs(1)),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn zero_pause_disables_waiting_entirely() {
        for index in 0..3 {
            assert_eq!(
                delay_before_mutation(index, Duration::ZERO),
                Duration::ZERO
            );
        }
    }
}
