//! Command-line interface definition (clap derive).

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "labeldeck",
    version,
    about = "Export, diff, and safely synchronize GitHub repository \
             labels from a canonical JSON file",
    long_about = "labeldeck manages GitHub repository labels against a \
                  canonical JSON file.\n\nLabels missing on GitHub are \
                  created; labels whose colour or description differ are \
                  updated in place (never deleted and recreated, so \
                  issue and pull-request associations survive); \
                  target-only labels are retained unless pruning is \
                  enabled."
)]
pub struct Cli {
    /// Bypass any configured HTTP proxy for this invocation.
    #[arg(long, global = true)]
    pub no_proxy: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Export a repository's labels as a canonical JSON file
    #[command(
        after_help = "Writes to FILE, or to standard output when FILE is \
                      omitted. An existing FILE is never overwritten \
                      without --force."
    )]
    Export {
        /// Repository to export, as OWNER/REPO.
        repo: String,
        /// Output file (standard output when omitted).
        file: Option<PathBuf>,
        /// Overwrite FILE even if it already exists.
        #[arg(long)]
        force: bool,
    },

    /// Compare a canonical label file with a repository, changing nothing
    Diff {
        /// Canonical label file to compare against the repository.
        file: PathBuf,
        /// Repository to compare with, as OWNER/REPO.
        repo: String,
        /// Delete target-only labels (overrides the configuration).
        #[arg(long, conflicts_with = "no_prune")]
        prune: bool,
        /// Keep target-only labels (overrides the configuration).
        #[arg(long)]
        no_prune: bool,
    },

    /// Synchronize a repository with a canonical label file
    #[command(
        after_help = "Creates missing labels, updates changed labels in \
                      place, and (only with pruning enabled) deletes \
                      target-only labels. Mutations pause briefly between \
                      requests per GitHub's rate-limit guidance. \
                      Deletions remove labels from existing issues and \
                      pull requests; --dry-run shows the plan without \
                      changing anything."
    )]
    Sync {
        /// Canonical label file to apply.
        file: PathBuf,
        /// Repository to synchronize, as OWNER/REPO.
        repo: String,
        /// Delete target-only labels (overrides the configuration).
        #[arg(long, conflicts_with = "no_prune")]
        prune: bool,
        /// Keep target-only labels (overrides the configuration).
        #[arg(long)]
        no_prune: bool,
        /// Show what would happen without applying any changes.
        #[arg(long)]
        dry_run: bool,
    },

    /// Manage the GitHub token labeldeck uses
    #[command(subcommand)]
    Auth(AuthCommand),
}

#[derive(Subcommand)]
pub enum AuthCommand {
    /// Store a GitHub token for future use
    #[command(after_help = "Prompts for the token without echoing it. Use \
                      --token-stdin in scripts: the token is read from \
                      standard input and stored. Tokens are never passed \
                      as command-line arguments, where process listings \
                      could observe them.")]
    Login {
        /// Read the token from standard input instead of a hidden prompt,
        /// validate it, and store it (for scripts and CI).
        #[arg(long)]
        token_stdin: bool,
    },
    /// Remove the stored token
    Logout,
    /// Show where authentication would come from, without any network
    /// request and without ever printing the token
    Status,
}

/// The effective CLI prune override: `Some(true)` for `--prune`,
/// `Some(false)` for `--no-prune`, `None` when neither flag was passed.
pub fn prune_override(prune: bool, no_prune: bool) -> Option<bool> {
    if prune {
        Some(true)
    } else if no_prune {
        Some(false)
    } else {
        None
    }
}
