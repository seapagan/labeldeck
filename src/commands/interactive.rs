//! Shared reconciliation session and post-terminal execution boundary.
use crate::{
    edit::{
        model::Document,
        session::{SaveHost, SessionResult},
        ui::{FinalSelection, SessionKind, UiAction, UiState},
    },
    error::{Error, Result},
    github::{GitHubClient, RepoSpec},
    labels::Label,
};
use std::path::PathBuf;

pub struct ReconcileContext {
    pub session: SessionKind,
    pub prune: bool,
    pub title: String,
    pub success_line: String,
    pub config_dir: PathBuf,
}

pub fn reconcile_with(
    client: &GitHubClient,
    target: &RepoSpec,
    desired: Vec<Label>,
    context: ReconcileContext,
    driver: impl FnOnce(UiState, SaveHost) -> SessionResult,
) -> Result<i32> {
    let snapshot = super::remote_labels(client, target)?;
    let level = colored_text::ColorizeConfig::color_level(
        colored_text::RenderTarget::Stdout,
    );
    let state = UiState::reconcile(
        Document::from_labels(desired),
        context.title,
        context.session,
        snapshot.clone(),
        context.prune,
        level,
    )
    .map_err(Error::Usage)?;
    let report = driver(state, SaveHost::new(context.config_dir, None));
    report.report_saves();
    match report.outcome? {
        UiAction::Cancel => {
            eprintln!(
                "Cancelled; final {} was not applied.",
                if context.session == SessionKind::Copy {
                    "copy"
                } else {
                    "sync"
                }
            );
            Ok(0)
        }
        UiAction::Finish(FinalSelection::Plan(plan)) => {
            validate_target(client, target, &snapshot)?;
            super::apply::execute_plan(
                client,
                target,
                &plan,
                &context.success_line,
            )
        }
        _ => Err(Error::Usage(
            "unexpected reconciliation session result".into(),
        )),
    }
}

fn validate_target(
    client: &GitHubClient,
    target: &RepoSpec,
    snapshot: &[Label],
) -> Result<()> {
    let current = super::remote_labels(client,target).map_err(|error| Error::Usage(format!("could not refetch target after the interactive plan: {error}. No mutations were attempted; resolve the read problem and rerun the command.")))?;
    if !crate::edit::plan::same_labels(snapshot, &current) {
        return Err(Error::Usage("the target changed while the interactive plan was open. No mutations were attempted; rerun the command.".into()));
    }
    Ok(())
}
