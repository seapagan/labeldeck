//! `labeldeck sync` — apply a canonical label set to a repository.

use crate::commands::diff;
use crate::commands::{
    config_dir, credentials_for_write, github_client, read_canonical,
    remote_labels, repo_spec, resolve_token,
};
use crate::error::Result;
use crate::github::RepoSpec;
use crate::plan;
use crate::sync as sync_engine;

pub fn run(
    repo: &str,
    file: Option<&std::path::PathBuf>,
    cli_prune: Option<bool>,
    dry_run: bool,
    no_proxy: bool,
) -> Result<i32> {
    let repo = repo_spec(repo)?;
    let config_dir = config_dir()?;
    let selection = crate::deck::resolve_read_selection(
        file.map(std::path::PathBuf::as_path),
        &config_dir,
    )?;
    let canonical = read_canonical(selection.path())?;
    let config = crate::config::load(&config_dir)?;
    let prune = crate::config::effective_prune(cli_prune, &config);

    if dry_run {
        // Dry runs only read; they never need credentials.
        let client =
            github_client(resolve_token(&config_dir).as_ref(), no_proxy);
        let remote = remote_labels(&client, &repo)?;
        let result = plan::plan(&canonical, &remote, prune);
        eprintln!("Dry run: no changes were made.");
        diff::print_plan(&result);
        eprintln!("{}", diff::summarize(&result));
        eprintln!(
            "Run again without --dry-run to apply: {}",
            follow_up_command(&repo, file, prune),
        );
        return Ok(0);
    }

    let stdin = std::io::stdin();
    let mut locked = stdin.lock();
    let credentials =
        credentials_for_write(&config_dir, &mut locked, no_proxy)?;
    let client = github_client(Some(&credentials), no_proxy);

    let remote = remote_labels(&client, &repo)?;
    let result = plan::plan(&canonical, &remote, prune);

    if prune && !result.deletes.is_empty() {
        eprintln!(
            "warning: pruning will DELETE {} target-only label(s); \
             deleting a label removes it from existing issues and pull \
             requests.",
            result.deletes.len()
        );
    }

    let mut printer = StderrReporter;
    let outcome = sync_engine::execute(&client, &repo, &result, &mut printer);

    if outcome.is_success() {
        eprintln!(
            "Synchronized {}/{}: {} created, {} updated, {} deleted.",
            repo.owner,
            repo.name,
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

/// The "run again" suggestion shown after `--dry-run`, preserving how
/// the deck was selected: explicit `--file` paths are repeated
/// verbatim; automatic selection (local or global default) prints the
/// clean default form without exposing the resolved global path.
fn follow_up_command(
    repo: &RepoSpec,
    file: Option<&std::path::PathBuf>,
    prune: bool,
) -> String {
    let prune_flag = if prune { "--prune" } else { "--no-prune" };
    match file {
        Some(path) => format!(
            "labeldeck sync {}/{} --file {} {prune_flag}",
            repo.owner,
            repo.name,
            path.display()
        ),
        None => {
            format!("labeldeck sync {}/{} {prune_flag}", repo.owner, repo.name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_selection_suggests_the_clean_default_form() {
        let repo = RepoSpec::parse("octocat/hello-world").unwrap();
        assert_eq!(
            follow_up_command(&repo, None, true),
            "labeldeck sync octocat/hello-world --prune"
        );
        assert_eq!(
            follow_up_command(&repo, None, false),
            "labeldeck sync octocat/hello-world --no-prune"
        );
    }

    #[test]
    fn explicit_selection_repeats_the_file_argument() {
        let repo = RepoSpec::parse("octocat/hello-world").unwrap();
        let path = std::path::PathBuf::from("custom.json");
        assert_eq!(
            follow_up_command(&repo, Some(&path), true),
            "labeldeck sync octocat/hello-world --file custom.json --prune"
        );
        assert_eq!(
            follow_up_command(&repo, Some(&path), false),
            "labeldeck sync octocat/hello-world --file custom.json \
             --no-prune"
        );
    }
}
