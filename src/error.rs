//! Unified error type and exit-code mapping.
//!
//! Exit codes (documented, tested):
//!
//! ```text
//! 0  success (diff: no differences; sync: fully applied)
//! 1  differences found (diff only)
//! 2  any error (usage, input, configuration, network, API)
//! ```

use std::path::PathBuf;

/// Everything that can make a command fail.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The canonical label file is invalid.
    #[error("{path}: {message}")]
    CanonicalFile { path: PathBuf, message: String },
    /// An `OWNER/REPO` argument could not be parsed.
    #[error("{0}")]
    RepoSpec(String),
    /// Reading or writing a local file failed.
    #[error("{context}: {message}")]
    Io { context: String, message: String },
    /// The output file exists and `--force` was not given.
    #[error("{path} already exists; refusing to overwrite it without --force")]
    OutputExists { path: PathBuf },
    /// Configuration could not be loaded.
    #[error("{0}")]
    Config(#[from] crate::config::ConfigError),
    #[error("{0}")]
    Github(#[from] crate::github::GithubError),
    /// Authentication is required but unavailable.
    #[error("{0}")]
    Auth(String),
    /// Invalid combination of command-line options.
    #[error("{0}")]
    Usage(String),
    /// No canonical label deck exists in any default location.
    #[error(
        "no canonical label file found; checked {local} and {global}. \
         Create one with `labeldeck export OWNER/REPO` (local) or \
         `labeldeck export OWNER/REPO --global`, or pass --file PATH"
    )]
    NoDeckFile { local: PathBuf, global: PathBuf },
}

/// Convenience alias for command implementations.
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    /// The process exit code for this error.
    pub fn exit_code(&self) -> i32 {
        2
    }
}
