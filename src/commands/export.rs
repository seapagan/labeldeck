//! `labeldeck export`.

use std::io::Write;

use crate::cli::STDOUT_FILE;
use crate::commands::{
    github_client, remote_labels, repo_spec, resolve_token,
};
use crate::deck;
use crate::error::{Error, Result};
use crate::staged_write::durability_warning;

pub fn run(
    repo: &str,
    file: Option<&std::path::PathBuf>,
    force: bool,
    global: bool,
    no_proxy: bool,
) -> Result<i32> {
    // `--file -` writes canonical JSON to standard output; combining it
    // with --force is rejected: stdout is never "overwritten".
    let stdout_mode = file
        .as_ref()
        .is_some_and(|path| path.as_os_str() == STDOUT_FILE);
    if stdout_mode && force {
        return Err(Error::Usage(
            "--force cannot be combined with --file -: standard output is \
             never overwrite-protected"
                .to_string(),
        ));
    }

    let repo = repo_spec(repo)?;
    let config_dir = crate::commands::config_dir()?;
    let client = github_client(resolve_token(&config_dir).as_ref(), no_proxy);

    let mut labels = remote_labels(&client, &repo)?;
    if stdout_mode {
        let json = crate::canonical::to_json(&mut labels);
        // Pure JSON on stdout: safe to pipe into other tools.
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        handle.write_all(json.as_bytes()).map_err(|e| Error::Io {
            context: "could not write to standard output".to_string(),
            message: e.to_string(),
        })?;
        handle.flush().map_err(|e| Error::Io {
            context: "could not write to standard output".to_string(),
            message: e.to_string(),
        })?;
        return Ok(0);
    }

    secure_global(global, &config_dir)?;

    let path = deck::export_destination(
        file.map(std::path::PathBuf::as_path),
        global,
        &config_dir,
    );
    persist(labels, &repo, &path, force)
}

fn secure_global(global: bool, config_dir: &std::path::Path) -> Result<()> {
    // For a global export, ensure/repair the configuration directory
    // BEFORE touching the deck: a previously unsearchable directory can
    // make a pre-check lie about whether the global deck exists.
    if global {
        crate::auth::ensure_config_dir(config_dir).map_err(|e| Error::Io {
            context: format!(
                "could not create or secure configuration directory {}",
                config_dir.display()
            ),
            message: e.to_string(),
        })?;
    }

    Ok(())
}

fn persist(
    mut labels: Vec<crate::labels::Label>,
    repo: &crate::github::RepoSpec,
    path: &std::path::Path,
    force: bool,
) -> Result<i32> {
    let json = crate::canonical::to_json(&mut labels);
    let outcome =
        crate::staged_write::write_deck(path, json.as_bytes(), force)?;
    eprintln!(
        "Exported {} labels from {}/{} to {}.",
        labels.len(),
        repo.owner,
        repo.name,
        path.display()
    );
    if let crate::staged_write::WriteOutcome::DurabilityUnconfirmed(error) =
        outcome
    {
        eprintln!("{}", durability_warning(&error));
    }
    Ok(0)
}

/// Interactive export uses read credentials and restores the terminal before writing.
pub fn run_interactive(
    repo: &str,
    file: Option<&std::path::PathBuf>,
    force: bool,
    global: bool,
    no_proxy: bool,
) -> Result<i32> {
    if file.is_some_and(|path| path.as_os_str() == STDOUT_FILE) {
        return Err(Error::Usage("--interactive cannot be combined with --file -: the TUI owns standard output".into()));
    }
    crate::edit::session::require_terminal("export --interactive")?;
    let repo = repo_spec(repo)?;
    let config_dir = crate::commands::config_dir()?;
    let client = github_client(resolve_token(&config_dir).as_ref(), no_proxy);
    let destination = deck::export_destination(
        file.map(std::path::PathBuf::as_path),
        global,
        &config_dir,
    );
    interactive_with(
        &client,
        &repo,
        &destination,
        force,
        global,
        &config_dir,
        crate::edit::session::run,
    )
}

pub fn interactive_with<D>(
    client: &crate::github::GitHubClient,
    repo: &crate::github::RepoSpec,
    destination: &std::path::Path,
    force: bool,
    global: bool,
    config_dir: &std::path::Path,
    driver: D,
) -> Result<i32>
where
    D: FnOnce(
        crate::edit::ui::UiState,
        crate::edit::session::SaveHost,
    ) -> crate::edit::session::SessionResult,
{
    use crate::edit::{
        model::Document,
        session::SaveHost,
        ui::{FinalSelection, UiAction, UiState},
    };
    crate::staged_write::preflight(destination, force)?;
    let labels = remote_labels(client, repo)?;
    let level = colored_text::ColorizeConfig::color_level(
        colored_text::RenderTarget::Stdout,
    );
    let state = UiState::export(
        Document::from_labels(labels),
        format!("{}/{}", repo.owner, repo.name),
        destination.into(),
        level,
    );
    let host = SaveHost::new(config_dir.into(), Some(destination.into()));
    let report = driver(state, host);
    report.report_saves();
    match report.outcome? {
        UiAction::Finish(FinalSelection::Export(labels)) => {
            secure_global(global, config_dir)?;
            persist(labels, repo, destination, force)
        }
        UiAction::Cancel => {
            eprintln!("Cancelled; final export was not performed.");
            Ok(0)
        }
        _ => Err(Error::Usage("unexpected export session result".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durability_warning_names_the_cause_and_keeps_success_clear() {
        let warning = durability_warning(&std::io::Error::other(
            "injected sync failure",
        ));
        assert!(
            warning
                .starts_with("warning: the deck was installed successfully"),
            "{warning}"
        );
        assert!(warning.contains("could not be synchronized"), "{warning}");
        assert!(warning.ends_with(": injected sync failure"), "{warning}");
    }
}
