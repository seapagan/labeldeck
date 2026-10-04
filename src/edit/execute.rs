//! Paced execution with explicit accounting; applied changes are never rolled back.

use super::plan::{EditPlan, Operation};
use crate::github::{GitHubClient, GithubError, RepoSpec};
use crate::sync::{MUTATION_PAUSE, Reporter, delay_before_mutation};
use std::time::Duration;

#[derive(Debug)]
pub struct EditFailure {
    pub operation: String,
    pub error: GithubError,
}

#[derive(Debug, Default)]
pub struct EditOutcome {
    pub applied: Vec<String>,
    pub failure: Option<EditFailure>,
    pub skipped: Vec<String>,
}

pub fn execute(
    client: &GitHubClient,
    repo: &RepoSpec,
    plan: &EditPlan,
    reporter: impl Reporter,
) -> EditOutcome {
    execute_with_pause(client, repo, plan, MUTATION_PAUSE, reporter)
}

pub fn execute_with_pause(
    client: &GitHubClient,
    repo: &RepoSpec,
    plan: &EditPlan,
    pause: Duration,
    mut reporter: impl Reporter,
) -> EditOutcome {
    let mut outcome = EditOutcome::default();
    for (index, operation) in plan.operations.iter().enumerate() {
        let description = operation.description();
        reporter.operation(&description);
        std::thread::sleep(delay_before_mutation(index, pause));
        let result = match operation {
            Operation::Create(label) => client.create_label(repo, label),
            Operation::Update {
                current_name,
                desired,
            } => client.update_label(repo, current_name, desired),
            Operation::Rename {
                current_name,
                desired,
            }
            | Operation::TemporaryRename {
                current_name,
                desired,
            } => client.edit_label(repo, current_name, desired),
            Operation::Delete { name } => client.delete_label(repo, name),
        };
        match result {
            Ok(()) => outcome.applied.push(description),
            Err(error) => {
                outcome.failure = Some(EditFailure {
                    operation: description,
                    error,
                });
                outcome.skipped.extend(
                    plan.operations[index + 1..]
                        .iter()
                        .map(Operation::description),
                );
                break;
            }
        }
    }
    outcome
}
