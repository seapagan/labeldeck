//! `labeldeck export`.

use std::io::Write;

use crate::cli::STDOUT_FILE;
use crate::commands::{
    github_client, remote_labels, repo_spec, resolve_token,
};
use crate::deck;
use crate::error::{Error, Result};

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
    let json = crate::canonical::to_json(&mut labels);

    if stdout_mode {
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

    // For a global export, ensure/repair the configuration directory
    // BEFORE touching the deck: a previously unsearchable directory can
    // make a pre-check lie about whether the global deck exists.
    if global {
        crate::auth::ensure_config_dir(&config_dir).map_err(|e| {
            Error::Io {
                context: format!(
                    "could not create or secure configuration directory {}",
                    config_dir.display()
                ),
                message: e.to_string(),
            }
        })?;
    }

    let path = deck::export_destination(
        file.map(std::path::PathBuf::as_path),
        global,
        &config_dir,
    );
    write_deck(&path, json.as_bytes(), force)?;
    eprintln!(
        "Exported {} labels from {}/{} to {}.",
        labels.len(),
        repo.owner,
        repo.name,
        path.display()
    );
    Ok(0)
}

/// Write the canonical deck to `path` with overwrite safety enforced by
/// the filesystem itself.
///
/// Without `force` the destination is created with create-new
/// semantics: an existing file — however it came to exist, including
/// after any earlier check or permission repair — fails atomically
/// with [`Error::OutputExists`] and its contents are never touched.
/// Other I/O failures stay contextual I/O errors. With `force` the
/// destination is deliberately created/truncated and written.
fn write_deck(
    path: &std::path::Path,
    bytes: &[u8],
    force: bool,
) -> Result<()> {
    use std::io::Write;
    if force {
        let mut file = std::fs::File::create(path).map_err(|e| Error::Io {
            context: format!("could not write {}", path.display()),
            message: e.to_string(),
        })?;
        file.write_all(bytes).map_err(|e| Error::Io {
            context: format!("could not write {}", path.display()),
            message: e.to_string(),
        })?;
        return Ok(());
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error::OutputExists {
                    path: path.to_path_buf(),
                }
            } else {
                Error::Io {
                    context: format!("could not write {}", path.display()),
                    message: e.to_string(),
                }
            }
        })?;
    file.write_all(bytes).map_err(|e| Error::Io {
        context: format!("could not write {}", path.display()),
        message: e.to_string(),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keeps the temp directory alive for the test's duration.
    struct Dir(tempfile::TempDir);

    impl Dir {
        fn new(name: &str) -> (Self, std::path::PathBuf) {
            let guard = tempfile::tempdir().expect("temp dir");
            let path = guard.path().join(name);
            (Self(guard), path)
        }
    }

    #[test]
    fn without_force_a_new_destination_is_created() {
        let (_guard, path) = Dir::new("new.json");
        write_deck(&path, b"[]", false).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"[]");
    }

    #[test]
    fn without_force_an_existing_destination_is_never_modified() {
        let (_guard, path) = Dir::new("existing.json");
        std::fs::write(&path, b"original").unwrap();
        let error = write_deck(&path, b"replacement", false).unwrap_err();
        assert!(matches!(error, Error::OutputExists { .. }), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
    }

    #[test]
    fn atomic_create_new_guards_destinations_that_appear_late() {
        let (_guard, path);
        // There is no prior existence check to fool: whatever exists at
        // open time — created before the call or between any earlier
        // preparation and the open — produces AlreadyExists at the
        // atomic create, never a truncation.
        (_guard, path) = Dir::new("late.json");
        std::fs::write(&path, b"first").unwrap();
        assert!(matches!(
            write_deck(&path, b"second", false).unwrap_err(),
            Error::OutputExists { .. }
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
    }

    #[test]
    fn with_force_the_destination_is_deliberately_replaced() {
        let (_guard, path) = Dir::new("forced.json");
        std::fs::write(&path, b"old contents").unwrap();
        write_deck(&path, b"new contents", true).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new contents");
    }

    #[test]
    fn non_existence_failures_stay_io_errors() {
        // A path whose parent is missing cannot be created: that is an
        // I/O error, not a claim that the destination exists.
        let (guard, _parent) = Dir::new("parent-marker");
        let path = guard.0.path().join("missing-dir").join("deck.json");
        let error = write_deck(&path, b"[]", false).unwrap_err();
        assert!(
            matches!(error, Error::Io { .. })
                && !matches!(error, Error::OutputExists { .. }),
            "{error}"
        );
    }
}
