//! `labeldeck sync` — apply a canonical label set to a repository.

use crate::commands::diff;
use crate::commands::{
    config_dir, credentials_for_write, github_client, read_canonical,
    remote_labels, repo_spec, resolve_token,
};
use crate::error::Result;
use crate::plan;
use crate::sync as sync_engine;

pub fn run(
    file: &std::path::Path,
    repo: &str,
    cli_prune: Option<bool>,
    dry_run: bool,
) -> Result<i32> {
    let repo = repo_spec(repo)?;
    let canonical = read_canonical(file)?;
    let config_dir = config_dir()?;
    let config = crate::config::load(&config_dir)?;
    let prune = crate::config::effective_prune(cli_prune, &config);

    if dry_run {
        // Dry runs only read; they never need credentials.
        let client = github_client(resolve_token(&config_dir).as_ref());
        let remote = remote_labels(&client, &repo)?;
        let result = plan::plan(&canonical, &remote, prune);
        eprintln!("Dry run: no changes were made.");
        diff::print_plan(&result);
        eprintln!("{}", diff::summarize(&result));
        eprintln!(
            "Run again without --dry-run to apply: labeldeck sync {} \
             {}/{} {}",
            file.display(),
            repo.owner,
            repo.name,
            if prune { "--prune" } else { "--no-prune" },
        );
        return Ok(0);
    }

    let stdin = std::io::stdin();
    let mut locked = stdin.lock();
    let credentials = credentials_for_write(&config_dir, &mut locked)?;
    let client = github_client(Some(&credentials));

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
