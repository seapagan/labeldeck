//! Shared plumbing for command implementations.

pub mod auth;
pub mod diff;
pub mod export;
pub mod sync;

use std::path::PathBuf;
use std::sync::Arc;

use crate::auth::ResolvedToken;
use crate::error::{Error, Result};
use crate::github::{GitHubClient, RepoSpec};

/// Environment variable overriding the GitHub API base URL. A test seam
/// and emergency compatibility escape hatch; not needed for normal use.
pub const API_BASE_ENV: &str = "LABELDECK_API";

/// Read an environment variable (thin wrapper for clarity).
fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// Parse and validate an `OWNER/REPO` argument.
pub fn repo_spec(spec: &str) -> Result<RepoSpec> {
    RepoSpec::parse(spec).map_err(Error::RepoSpec)
}

/// Resolve the configuration directory for this invocation.
pub fn config_dir() -> Result<PathBuf> {
    crate::config::config_dir(&env).map_err(Error::Config)
}

/// Resolve the token according to the documented precedence.
pub fn resolve_token(config_dir: &std::path::Path) -> Option<ResolvedToken> {
    crate::auth::resolve_token(config_dir, &env)
}

/// Build a GitHub client for the resolved token, honouring
/// [`API_BASE_ENV`] when set.
pub fn github_client(token: Option<&ResolvedToken>) -> GitHubClient {
    let token = token.map(|resolved| Arc::clone(&resolved.token));
    match env(API_BASE_ENV) {
        Some(base) if !base.trim().is_empty() => {
            GitHubClient::with_base_url(base.trim(), token)
        }
        _ => GitHubClient::new(token),
    }
}

/// Read and validate a canonical label file.
pub fn read_canonical(
    path: &std::path::Path,
) -> Result<Vec<crate::labels::Label>> {
    let text = std::fs::read_to_string(path).map_err(|e| Error::Io {
        context: format!("could not read {}", path.display()),
        message: e.to_string(),
    })?;
    crate::canonical::parse(&text).map_err(|e| Error::CanonicalFile {
        path: PathBuf::from(path),
        message: e.to_string(),
    })
}

/// Fetch the complete remote label set for a repository.
pub fn remote_labels(
    client: &GitHubClient,
    repo: &RepoSpec,
) -> Result<Vec<crate::labels::Label>> {
    client.list_labels(repo).map_err(Error::Github)
}

/// Obtain credentials for a mutating command, offering the first-use
/// login flow when running on an interactive terminal. Non-interactive
/// invocations fail cleanly instead of waiting for input that will
/// never arrive.
pub fn credentials_for_write(
    config_dir: &std::path::Path,
    stdin: &mut (impl std::io::BufRead + std::io::IsTerminal),
) -> Result<ResolvedToken> {
    if let Some(resolved) = resolve_token(config_dir) {
        return Ok(resolved);
    }
    if !stdin.is_terminal() {
        return Err(Error::Auth(
            "authentication is required to change labels; run \
             `labeldeck auth login` or set LABELDECK_TOKEN, GH_TOKEN, \
             or GITHUB_TOKEN"
                .to_string(),
        ));
    }
    auth::prompt_and_store_login(config_dir, stdin)
}
