//! Exact-source interactive editing and optimistic Apply guards.

use crate::{
    canonical, deck,
    edit::{
        execute,
        model::Document,
        plan,
        session::{self, SaveHost, SessionResult},
        ui,
    },
    error::{Error, Result},
    github::{GitHubClient, RepoSpec},
    labels::Label,
    staged_write,
};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum SourceSelection {
    Local(PathBuf),
    Global(PathBuf),
    Explicit(PathBuf),
    Remote(RepoSpec),
}

#[derive(Debug)]
pub struct FileSnapshot {
    pub path: PathBuf,
    pub baseline: Vec<Label>,
    pub contents: Vec<u8>,
}

pub fn select_source(
    repo: Option<&str>,
    file: Option<&Path>,
    global: bool,
    config_dir: &Path,
) -> Result<SourceSelection> {
    if usize::from(repo.is_some())
        + usize::from(file.is_some())
        + usize::from(global)
        > 1
    {
        return Err(Error::Usage(
            "OWNER/REPO, --global, and --file are mutually exclusive".into(),
        ));
    }
    if let Some(repo) = repo {
        return Ok(SourceSelection::Remote(super::repo_spec(repo)?));
    }
    if let Some(path) = file {
        if path.as_os_str() == "-" {
            return Err(Error::Usage(
                "--file - is invalid for edit; use an existing deck path"
                    .into(),
            ));
        }
        return Ok(SourceSelection::Explicit(path.into()));
    }
    if global {
        Ok(SourceSelection::Global(deck::global_deck_path(config_dir)))
    } else {
        Ok(SourceSelection::Local(deck::local_deck_path()))
    }
}

pub fn load_file(selection: &SourceSelection) -> Result<FileSnapshot> {
    let (path, hint) = match selection {
        SourceSelection::Local(path) => (
            path,
            "Create it with `labeldeck export OWNER/REPO`, or select an existing deck with `labeldeck edit --file PATH`",
        ),
        SourceSelection::Global(path) => (
            path,
            "Create it with `labeldeck export OWNER/REPO --global`",
        ),
        SourceSelection::Explicit(path) => (
            path,
            "Select an existing deck with `labeldeck edit --file PATH`, or export one with `labeldeck export OWNER/REPO --file PATH`",
        ),
        SourceSelection::Remote(_) => {
            return Err(Error::Usage(
                "a live repository is not a deck file".into(),
            ));
        }
    };
    let contents = std::fs::read(path).map_err(|error| Error::Io {
        context: format!("could not read {}", path.display()),
        message: if error.kind() == std::io::ErrorKind::NotFound {
            format!("selected deck is missing. {hint}")
        } else {
            error.to_string()
        },
    })?;
    let text = std::str::from_utf8(&contents).map_err(|error| {
        Error::CanonicalFile {
            path: path.clone(),
            message: error.to_string(),
        }
    })?;
    let baseline =
        canonical::parse(text).map_err(|error| Error::CanonicalFile {
            path: path.clone(),
            message: error.to_string(),
        })?;
    Ok(FileSnapshot {
        path: path.clone(),
        baseline,
        contents,
    })
}

pub fn run(
    repo: Option<&str>,
    file: Option<&PathBuf>,
    global: bool,
    no_proxy: bool,
) -> Result<i32> {
    session::require_terminal("edit")?;
    let config_dir = super::config_dir()?;
    let selection =
        select_source(repo, file.map(PathBuf::as_path), global, &config_dir)?;
    match selection {
        SourceSelection::Remote(repo) => {
            let credentials = super::credentials_for_write(
                &config_dir,
                &mut std::io::stdin().lock(),
                no_proxy,
            )?;
            let client = super::github_client(Some(&credentials), no_proxy);
            edit_remote_session_with(
                &client,
                &repo,
                |document, title, live| {
                    open(document, title, live, &config_dir)
                },
            )
        }
        selection => {
            edit_file_session_with(&selection, |document, title, live| {
                open(document, title, live, &config_dir)
            })
        }
    }
}

/// Run a file session with an editor driver; source loading and persistence
/// remain identical for a real terminal and deterministic event-driven tests.
pub fn edit_file_with(
    selection: &SourceSelection,
    editor: impl FnOnce(Document, &str, bool) -> Result<Option<Document>>,
) -> Result<i32> {
    edit_file_session_with(selection, |document, title, live| {
        editor_result(editor(document, title, live))
    })
}

pub fn edit_file_session_with(
    selection: &SourceSelection,
    editor: impl FnOnce(Document, &str, bool) -> SessionResult,
) -> Result<i32> {
    let snapshot = load_file(selection)?;
    let document = editor(
        Document::from_labels(snapshot.baseline.clone()),
        &snapshot.path.display().to_string(),
        false,
    );
    finish_edit(document, |document| {
        apply_file(
            &snapshot.path,
            &snapshot.baseline,
            &snapshot.contents,
            &document,
        )
    })
}

/// Run a live session using an already authenticated client.
pub fn edit_remote_with(
    client: &GitHubClient,
    repo: &RepoSpec,
    editor: impl FnOnce(Document, &str, bool) -> Result<Option<Document>>,
) -> Result<i32> {
    edit_remote_session_with(client, repo, |document, title, live| {
        editor_result(editor(document, title, live))
    })
}

pub fn edit_remote_session_with(
    client: &GitHubClient,
    repo: &RepoSpec,
    editor: impl FnOnce(Document, &str, bool) -> SessionResult,
) -> Result<i32> {
    let baseline = super::remote_labels(client, repo)?;
    let title = format!("{}/{} (live)", repo.owner, repo.name);
    let report = editor(Document::from_labels(baseline.clone()), &title, true);
    finish_edit(report, |document| {
        apply_remote(client, repo, &baseline, &document)
    })
}

fn editor_result(result: Result<Option<Document>>) -> SessionResult {
    SessionResult {
        outcome: result.map(|document| {
            document.map_or(ui::UiAction::Cancel, ui::UiAction::Apply)
        }),
        saves: Vec::new(),
    }
}

fn open(
    document: Document,
    title: &str,
    live: bool,
    config_dir: &Path,
) -> SessionResult {
    let host = SaveHost::new(
        config_dir.into(),
        if live { None } else { Some(title.into()) },
    );
    let level = colored_text::ColorizeConfig::color_level(
        colored_text::RenderTarget::Stdout,
    );
    session::run(ui::UiState::new(document, title.into(), live, level), host)
}

fn finish_edit(
    report: SessionResult,
    apply: impl FnOnce(Document) -> Result<i32>,
) -> Result<i32> {
    report.report_saves();
    match report.outcome? {
        ui::UiAction::Apply(document) => apply(document),
        ui::UiAction::Cancel => {
            eprintln!(
                "{}",
                if report.saves.is_empty() {
                    "Cancelled; no changes applied."
                } else {
                    "Cancelled; source Apply was not performed. Completed Saves remain on disk."
                }
            );
            Ok(0)
        }
        _ => Err(Error::Usage("unexpected editor result".into())),
    }
}
fn nothing() -> Result<i32> {
    eprintln!("Nothing to apply; no changes made.");
    Ok(0)
}

pub fn apply_file(
    path: &Path,
    baseline: &[Label],
    contents: &[u8],
    document: &Document,
) -> Result<i32> {
    let mut labels = document.labels().map_err(Error::Usage)?;
    if plan::same_labels(baseline, &labels) {
        return nothing();
    }
    let current = std::fs::read(path).map_err(|error| Error::Io {
        context: format!(
            "could not reread {} before Apply; rerun edit",
            path.display()
        ),
        message: error.to_string(),
    })?;
    if current != contents {
        return Err(Error::Usage(format!(
            "{} changed while the editor was open; refusing to overwrite it. Rerun edit",
            path.display()
        )));
    }
    let json = canonical::to_json(&mut labels);
    let outcome = staged_write::write_deck(path, json.as_bytes(), true)?;
    eprintln!("Applied {} labels to {}.", labels.len(), path.display());
    if let staged_write::WriteOutcome::DurabilityUnconfirmed(error) = outcome {
        eprintln!("{}", staged_write::durability_warning(&error));
    }
    Ok(0)
}

pub fn apply_remote(
    client: &GitHubClient,
    repo: &RepoSpec,
    baseline: &[Label],
    document: &Document,
) -> Result<i32> {
    let plan = plan::plan(document).map_err(Error::Usage)?;
    if plan.operations.is_empty() {
        return nothing();
    }
    let current = super::remote_labels(client, repo)?;
    if !plan::same_labels(baseline, &current) {
        return Err(Error::Usage("the repository changed while the editor was open; no mutations were attempted. Rerun edit".into()));
    }
    let outcome = execute::execute(client, repo, &plan, StderrReporter);
    report_outcome(&plan, &outcome)
}

fn report_outcome(
    plan: &plan::EditPlan,
    outcome: &execute::EditOutcome,
) -> Result<i32> {
    if let Some(failure) = &outcome.failure {
        for applied in &outcome.applied {
            eprintln!("applied: {applied}");
        }
        eprintln!("failed: {}\nreason: {}", failure.operation, failure.error);
        for skipped in &outcome.skipped {
            eprintln!("skipped: {skipped}");
        }
        eprintln!(
            "GitHub does not support transactional label updates; applied operations remain in effect, including any temporary renames listed above."
        );
        return Ok(2);
    }
    let summary = &plan.summary;
    eprintln!(
        "Applied: {} created, {} renamed, {} updated, {} deleted.",
        summary.created, summary.renamed, summary.updated, summary.deleted
    );
    Ok(0)
}

struct StderrReporter;
impl crate::sync::Reporter for StderrReporter {
    fn operation(&mut self, description: &str) {
        eprintln!("labeldeck: {description}...");
    }
}
