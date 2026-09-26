//! GitHub token resolution and storage.
//!
//! Precedence (documented, tested):
//!
//! ```text
//! LABELDECK_TOKEN → GH_TOKEN → GITHUB_TOKEN → stored token → anonymous
//! ```
//!
//! The stored token lives in its own file (`token`) inside the labeldeck
//! configuration directory, never in `config.toml`. On Unix the file is
//! created with mode 0600 and the directory with 0700; on Windows the
//! file receives the default user-profile protections and no stronger
//! ACL claim is made.
//!
//! Tokens are never included in diagnostics, errors, or status output.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Environment variables that can supply a token, in precedence order.
pub const TOKEN_ENV_VARS: [&str; 3] =
    ["LABELDECK_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"];

/// Where a resolved token came from (diagnostics only; never the value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenSource {
    /// An environment variable, by name.
    Environment(&'static str),
    /// The stored token file.
    Stored(PathBuf),
}

/// A resolved token and its origin.
#[derive(Debug, Clone)]
pub struct ResolvedToken {
    pub token: Arc<str>,
    pub source: TokenSource,
}

/// The path of the stored token file within a configuration directory.
pub fn token_path(config_dir: &Path) -> PathBuf {
    config_dir.join("token")
}

/// Resolve the token to use, honouring the documented precedence.
///
/// The `env` lookup is injected so tests stay deterministic without
/// mutating process-global environment variables. Empty or blank
/// variable values count as unset.
pub fn resolve_token(
    config_dir: &Path,
    env: &dyn Fn(&str) -> Option<String>,
) -> Option<ResolvedToken> {
    for name in TOKEN_ENV_VARS {
        if let Some(value) = env(name) {
            if !value.trim().is_empty() {
                return Some(ResolvedToken {
                    token: Arc::from(value.trim()),
                    source: TokenSource::Environment(name),
                });
            }
        }
    }
    read_stored_token(config_dir).map(|token| ResolvedToken {
        token: Arc::from(token),
        source: TokenSource::Stored(token_path(config_dir)),
    })
}

/// Read the stored token, if present and non-blank.
pub fn read_stored_token(config_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(token_path(config_dir)).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Persist a token for future use.
///
/// Creates the configuration directory when needed. On Unix the token
/// file is written 0600 and the directory is tightened to 0700; other
/// platforms get the platform default protections.
pub fn store_token(config_dir: &Path, token: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(config_dir)?;
    restrict_directory_permissions(config_dir);
    let path = token_path(config_dir);
    write_private_file(&path, token)
}

/// Remove the stored token. Returns whether a file was removed.
pub fn remove_stored_token(config_dir: &Path) -> std::io::Result<bool> {
    match std::fs::remove_file(token_path(config_dir)) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

fn write_private_file(path: &Path, contents: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(contents.as_bytes())?;
        // The mode above can be weakened by umask-interaction on some
        // platforms when the file already existed; re-assert it.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            path,
            std::fs::Permissions::from_mode(0o600),
        )?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let mut file = std::fs::File::create(path)?;
        file.write_all(contents.as_bytes())?;
        Ok(())
    }
}

#[cfg(unix)]
fn restrict_directory_permissions(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).ok();
}

#[cfg(not(unix))]
fn restrict_directory_permissions(_dir: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config_dir() -> std::path::PathBuf {
        tempfile::tempdir()
            .expect("temp dir")
            .keep()
            .join("labeldeck")
    }

    fn env_with<'a>(
        values: &'a [(&'a str, &'a str)],
    ) -> impl Fn(&str) -> Option<String> + use<'a> {
        move |key: &str| {
            values
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.to_string())
        }
    }

    #[test]
    fn precedence_labeldeck_token_first() {
        let dir = temp_config_dir();
        store_token(&dir, "stored-token").unwrap();
        let resolved = resolve_token(
            &dir,
            &env_with(&[
                ("LABELDECK_TOKEN", "labeldeck-token"),
                ("GH_TOKEN", "gh-token"),
                ("GITHUB_TOKEN", "github-token"),
            ]),
        )
        .unwrap();
        assert_eq!(&*resolved.token, "labeldeck-token");
        assert_eq!(
            resolved.source,
            TokenSource::Environment("LABELDECK_TOKEN")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn precedence_gh_token_before_github_token() {
        let dir = temp_config_dir();
        let resolved = resolve_token(
            &dir,
            &env_with(&[
                ("GH_TOKEN", "gh-token"),
                ("GITHUB_TOKEN", "github-token"),
            ]),
        )
        .unwrap();
        assert_eq!(&*resolved.token, "gh-token");
        assert_eq!(resolved.source, TokenSource::Environment("GH_TOKEN"));
    }

    #[test]
    fn precedence_stored_token_after_environment() {
        let dir = temp_config_dir();
        store_token(&dir, "stored-token").unwrap();
        let resolved = resolve_token(&dir, &|_: &str| None).unwrap();
        assert_eq!(&*resolved.token, "stored-token");
        assert_eq!(resolved.source, TokenSource::Stored(token_path(&dir)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_token_anywhere_is_anonymous() {
        let dir = temp_config_dir();
        assert!(resolve_token(&dir, &|_: &str| None).is_none());
    }

    #[test]
    fn blank_environment_variables_are_ignored() {
        let dir = temp_config_dir();
        store_token(&dir, "stored-token").unwrap();
        let resolved = resolve_token(
            &dir,
            &env_with(&[
                ("LABELDECK_TOKEN", "  "),
                ("GH_TOKEN", ""),
                ("GITHUB_TOKEN", "\n"),
            ]),
        )
        .unwrap();
        assert_eq!(&*resolved.token, "stored-token");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn stored_token_is_trimmed() {
        let dir = temp_config_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(token_path(&dir), "  padded-token \n").unwrap();
        let resolved = resolve_token(&dir, &|_: &str| None).unwrap();
        assert_eq!(&*resolved.token, "padded-token");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_stored_token_counts_as_absent() {
        let dir = temp_config_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(token_path(&dir), "\n").unwrap();
        assert!(resolve_token(&dir, &|_: &str| None).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remove_stored_token_reports_existence() {
        let dir = temp_config_dir();
        assert!(!remove_stored_token(&dir).unwrap());
        store_token(&dir, "temp").unwrap();
        assert!(remove_stored_token(&dir).unwrap());
        assert!(read_stored_token(&dir).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn unix_token_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_config_dir();
        store_token(&dir, "secret").unwrap();
        let mode = std::fs::metadata(token_path(&dir))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "token file must be 0600");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn unix_config_dir_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_config_dir();
        store_token(&dir, "secret").unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "config dir must be 0700");
        std::fs::remove_dir_all(&dir).ok();
    }
}
