//! Command-line interface definition (clap derive).

use std::path::PathBuf;

use clap::builder::TypedValueParser;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "labeldeck",
    version,
    about = "Export, copy, diff, and safely synchronize GitHub \
             repository labels",
    long_about = "labeldeck manages GitHub repository labels against a \
                  canonical JSON file.\n\nLabels missing on GitHub are \
                  created; labels whose colour or description differ are \
                  updated in place (never deleted and recreated, so \
                  issue and pull-request associations survive); \
                  target-only labels are retained unless pruning is \
                  enabled.\n\n`labeldeck copy SOURCE TARGET` applies \
                  one repository's labels directly to another \
                  repository with the same reconciliation, without a \
                  local canonical file."
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
    /// Interactively edit an exact deck or a repository's live labels
    #[command(
        after_help = "Bare edit opens only ./labels.json, never the global deck. --global opens only the global deck; --file PATH opens exactly that existing file. OWNER/REPO edits live labels and requires write authentication. Input and output must be interactive terminals. Apply and its confirmation write the source. Save (s) writes the full working deck to Local or Global without clearing history; targets aliasing the file source are disabled. Completed Saves remain after Cancel. Renames preserve issue/PR label associations; live deletions remove them. External changes detected before Apply are refused. Ctrl-Z/Ctrl-Y undo/redo; Ctrl-S opens Apply; Tab selects buttons; Esc cancels a field or exits. Colour previews respect terminal capability and NO_COLOR."
    )]
    Edit {
        /// Repository to edit live, as OWNER/REPO; omit for a local deck.
        #[arg(conflicts_with_all = ["global", "file"])]
        repo: Option<String>,
        /// Edit this exact existing deck; '-' is invalid.
        #[arg(long, value_name = "PATH", value_parser = clap::builder::OsStringValueParser::new().try_map(edit_file_path))]
        file: Option<PathBuf>,
        /// Edit only the deck in the labeldeck configuration directory.
        #[arg(long, conflicts_with = "file")]
        global: bool,
    },
    /// Export a repository's labels as a canonical JSON file
    #[command(
        after_help = "Writes to ./labels.json by default; --global writes \
                      your personal default deck to the labeldeck \
                      configuration directory, --file PATH picks an exact \
                      destination, and --file - writes canonical JSON to \
                      standard output. An existing file is never \
                      overwritten without --force (which cannot be \
                      combined with --file -). --interactive (-i) selects and edits a working copy before confirmed export; it cannot use --file -."
    )]
    Export {
        /// Select and edit a working deck in the terminal before exporting.
        #[arg(short = 'i', long)]
        interactive: bool,
        /// Repository to export, as OWNER/REPO.
        repo: String,
        /// Output file (default: ./labels.json; '-' writes standard output).
        #[arg(long, value_name = "PATH")]
        file: Option<PathBuf>,
        /// Overwrite the output file even if it already exists.
        #[arg(long)]
        force: bool,
        /// Write the canonical deck to the labeldeck configuration
        /// directory instead of the local file.
        #[arg(long, conflicts_with = "file")]
        global: bool,
    },

    /// Compare a canonical label file with a repository, changing nothing
    #[command(
        after_help = "Uses ./labels.json, or the global deck when no local \
                      entry exists. --global selects only the deck in the \
                      labeldeck configuration directory, even when a local \
                      deck exists; it conflicts with --file PATH."
    )]
    Diff {
        /// Repository to compare with, as OWNER/REPO.
        repo: String,
        /// Use PATH exactly instead of ./labels.json or the global
        /// default deck.
        #[arg(long, value_name = "PATH")]
        file: Option<PathBuf>,
        /// Use only the deck in the labeldeck configuration directory.
        #[arg(long, conflicts_with = "file")]
        global: bool,
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
                      target-only labels. The canonical deck is \
                      ./labels.json, or the global deck in the labeldeck \
                      configuration directory when no local file exists; \
                      --global selects only the global deck, even when a \
                      local deck exists, and conflicts with --file PATH, \
                      which uses that path exactly. Mutations pause \
                      briefly between requests per GitHub's rate-limit \
                      guidance. Deletions remove labels from existing \
                      issues and pull requests; --dry-run shows the plan \
                      without changing anything."
    )]
    Sync {
        /// Select reconciliation operations and edit a working deck in the terminal.
        #[arg(short = 'i', long, conflicts_with = "dry_run")]
        interactive: bool,
        /// Repository to synchronize, as OWNER/REPO.
        repo: String,
        /// Use PATH exactly instead of ./labels.json or the global
        /// default deck.
        #[arg(long, value_name = "PATH")]
        file: Option<PathBuf>,
        /// Use only the deck in the labeldeck configuration directory.
        #[arg(long, conflicts_with = "file")]
        global: bool,
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

    /// Copy labels directly from one repository to another
    #[command(
        after_help = "Makes the source repository's current labels the \
                      desired set for the target, reconciled exactly as \
                      sync would: missing labels are created, changed \
                      labels are updated in place, and target-only \
                      labels are kept unless pruning is enabled. The \
                      source repository is only ever read; no canonical \
                      deck is used or resolved. --dry-run shows the \
                      plan without changing anything."
    )]
    Copy {
        /// Select reconciliation operations and edit a SOURCE-derived working deck.
        #[arg(short = 'i', long, conflicts_with = "dry_run")]
        interactive: bool,
        /// Repository whose labels are copied, as OWNER/REPO.
        source: String,
        /// Repository the labels are applied to, as OWNER/REPO.
        target: String,
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

/// Canonical label file used when `--file` is not supplied.
pub const DEFAULT_LABELS_FILE: &str = "labels.json";

/// `--file -` selects standard output for `export`.
pub const STDOUT_FILE: &str = "-";

fn edit_file_path(value: std::ffi::OsString) -> Result<PathBuf, String> {
    if value == STDOUT_FILE {
        Err("--file - is invalid for edit; use an existing deck path".into())
    } else {
        Ok(PathBuf::from(value))
    }
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
