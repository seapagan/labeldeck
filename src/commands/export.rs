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

/// Write the canonical deck to `path` crash-safely, via a staged file
/// in the destination's own directory.
///
/// The complete replacement is written to a hidden, uniquely named
/// staging file (`write_all` → `flush` → `sync_all`) and only then
/// installed at the destination:
///
/// * `force`: the staging file atomically **replaces** the
///   destination. `std::fs::rename` is an atomic same-volume replace
///   on Linux/macOS (POSIX `rename(2)`) and on Windows
///   (`MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`, with a
///   POSIX-semantics `SetFileInformationByHandle` fallback), so an
///   existing destination is never truncated or removed first and a
///   failure before the rename leaves it byte-for-byte unchanged.
/// * without `force`: the staging file is **hard-linked** to the
///   destination. A hard link appears at the destination only if that
///   name does not exist — an existing destination, however it came
///   to exist, fails atomically with [`Error::OutputExists`] and its
///   contents are never touched — and the destination name only ever
///   references the fully written staging inode, never a partial
///   file. The staging name is removed afterwards (both names share
///   one inode). Filesystems without hard links (e.g. FAT/exFAT
///   volumes on Windows) surface a contextual I/O error instead.
///
/// Any failure before the install removes the staging file, so a
/// failed export leaves no partial destination and no staging
/// leftovers; only a hard process crash can strand a staging file.
///
/// Durability: the staged contents are fsynced before the install,
/// and on Unix the destination directory is fsynced after it so the
/// new name itself survives a crash. On Windows, `std` cannot open a
/// directory for syncing, so the install is durable only to the
/// extent NTFS metadata journaling provides. The guarantee is
/// crash-safe *contents replacement*, not storage-controller-level
/// durability on every filesystem.
///
/// Permissions: the staged file is created with the platform's
/// default mode for new files (the usual umask on Unix), so a forced
/// replacement resets an existing destination's mode to that default
/// rather than preserving the old mode. Ordinary decks are not
/// secrets, and the global deck lives inside the private (0700)
/// configuration directory, which is what protects it.
///
/// `--file -` (standard output) is handled by the caller and never
/// reaches this code.
fn write_deck(
    path: &std::path::Path,
    bytes: &[u8],
    force: bool,
) -> Result<()> {
    staged_write(path, bytes, force, &mut no_fault)
}

/// Points where a staged write can be made to fail, so tests can
/// prove the failure guarantees deterministically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Before the staged contents are written and synced.
    Contents,
    /// Before the staged file is installed at the destination.
    Install,
}

fn no_fault(_: Stage) -> std::io::Result<()> {
    Ok(())
}

/// Monotonic suffix for staging names within this process.
static STAGING_SEQUENCE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// A staging file's name; dropped exactly when the write is done
/// (successfully or not) so leftovers never survive a handled error.
struct Staged {
    path: std::path::PathBuf,
}

impl Drop for Staged {
    fn drop(&mut self) {
        // After a successful install the staging name is already gone
        // (renamed away, or the hard link's staging-side name); after
        // any failure this deletes the leftover. Both are fine to
        // attempt unconditionally.
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The directory to stage in: the destination's own directory, so the
/// final install never crosses a filesystem boundary. A bare
/// destination filename stages in the current directory.
fn staging_dir(dest: &std::path::Path) -> &std::path::Path {
    match dest.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => std::path::Path::new("."),
    }
}

fn write_error(path: &std::path::Path, error: std::io::Error) -> Error {
    Error::Io {
        context: format!("could not write {}", path.display()),
        message: error.to_string(),
    }
}

/// The staged-write state machine shared by both finalization modes.
fn staged_write(
    dest: &std::path::Path,
    bytes: &[u8],
    force: bool,
    fault: &mut dyn FnMut(Stage) -> std::io::Result<()>,
) -> Result<()> {
    use std::io::Write;
    use std::sync::atomic::Ordering;

    let dir = staging_dir(dest);
    let mut attempts = 0u32;
    let (mut file, staged) = loop {
        let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = dir
            .join(
                format!(".labeldeck-tmp-{}-{sequence}", std::process::id(),),
            );
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (file, Staged { path: candidate }),
            // Same-directory create_new already avoids real
            // collisions; the bounded retry loop only covers a stray
            // same-named file.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                attempts += 1;
                if attempts >= 1024 {
                    return Err(write_error(dest, e));
                }
            }
            Err(e) => return Err(write_error(dest, e)),
        }
    };

    fault(Stage::Contents).map_err(|e| write_error(dest, e))?;
    file.write_all(bytes).map_err(|e| write_error(dest, e))?;
    file.flush().map_err(|e| write_error(dest, e))?;
    file.sync_all().map_err(|e| write_error(dest, e))?;

    fault(Stage::Install).map_err(|e| write_error(dest, e))?;
    if force {
        std::fs::rename(&staged.path, dest)
            .map_err(|e| write_error(dest, e))?;
    } else {
        match std::fs::hard_link(&staged.path, dest) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(Error::OutputExists {
                    path: dest.to_path_buf(),
                });
            }
            Err(e) => return Err(write_error(dest, e)),
        }
    }
    // The staged guard's Drop removes the staging name (already gone
    // after a rename) once the directory sync has been requested.
    sync_directory(dir).map_err(|e| write_error(dest, e))?;
    Ok(())
}

/// fsync the directory holding the destination so the installed name
/// itself is durable. Omitted on Windows: `std` cannot open a
/// directory there without backup-semantics privileges.
#[cfg(unix)]
fn sync_directory(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_dir: &std::path::Path) -> std::io::Result<()> {
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

    fn staging_leftovers(dir: &std::path::Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .expect("read dir")
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(".labeldeck-tmp-"))
            .collect()
    }

    fn fault_at(stage: Stage) -> impl FnMut(Stage) -> std::io::Result<()> {
        move |reached: Stage| {
            if reached == stage {
                Err(std::io::Error::other("injected failure"))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn without_force_a_new_destination_is_created() {
        let (guard, path) = Dir::new("new.json");
        write_deck(&path, b"[]", false).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"[]");
        assert!(staging_leftovers(guard.0.path()).is_empty());
    }

    #[test]
    fn without_force_an_existing_destination_is_never_modified() {
        let (guard, path) = Dir::new("existing.json");
        std::fs::write(&path, b"original").unwrap();
        let error = write_deck(&path, b"replacement", false).unwrap_err();
        assert!(matches!(error, Error::OutputExists { .. }), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert!(staging_leftovers(guard.0.path()).is_empty());
    }

    #[test]
    fn install_guards_destinations_that_appear_late() {
        let (guard, path);
        // There is no prior existence check to fool: whatever exists
        // when the hard link is attempted — created before the call
        // or between staging and install — produces AlreadyExists at
        // the atomic link, never an overwrite.
        (guard, path) = Dir::new("late.json");
        std::fs::write(&path, b"first").unwrap();
        assert!(matches!(
            write_deck(&path, b"second", false).unwrap_err(),
            Error::OutputExists { .. }
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
        assert!(staging_leftovers(guard.0.path()).is_empty());
    }

    #[test]
    fn with_force_the_destination_is_deliberately_replaced() {
        let (guard, path) = Dir::new("forced.json");
        std::fs::write(&path, b"old contents").unwrap();
        write_deck(&path, b"new contents", true).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new contents");
        assert!(staging_leftovers(guard.0.path()).is_empty());
    }

    #[test]
    fn with_force_an_absent_destination_is_created() {
        let (guard, path) = Dir::new("forced-new.json");
        write_deck(&path, b"fresh", true).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"fresh");
        assert!(staging_leftovers(guard.0.path()).is_empty());
    }

    #[test]
    fn non_existence_failures_stay_io_errors() {
        // A path whose parent is missing cannot be staged: that is an
        // I/O error, not a claim that the destination exists.
        let (guard, _parent) = Dir::new("parent-marker");
        let path = guard.0.path().join("missing-dir").join("deck.json");
        let error = write_deck(&path, b"[]", false).unwrap_err();
        assert!(
            matches!(error, Error::Io { .. })
                && !matches!(error, Error::OutputExists { .. }),
            "{error}"
        );
        // Nothing could be staged in the existing parent either.
        assert!(staging_leftovers(guard.0.path()).is_empty());
    }

    #[test]
    fn failed_contents_leave_an_existing_destination_unchanged() {
        let (guard, path) = Dir::new("contents-fail.json");
        std::fs::write(&path, b"old contents").unwrap();
        let mut fault = fault_at(Stage::Contents);
        let error = staged_write(&path, b"new contents", true, &mut fault)
            .unwrap_err();
        assert!(matches!(error, Error::Io { .. }), "{error}");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"old contents",
            "force must not touch the destination before the install"
        );
        assert!(staging_leftovers(guard.0.path()).is_empty());
    }

    #[test]
    fn failed_contents_never_create_the_destination() {
        let (guard, path) = Dir::new("contents-absent.json");
        let mut fault = fault_at(Stage::Contents);
        staged_write(&path, b"new contents", false, &mut fault).unwrap_err();
        assert!(
            !path.exists(),
            "no partial final deck may appear on a failed write"
        );
        assert!(staging_leftovers(guard.0.path()).is_empty());
    }

    #[test]
    fn failed_install_leaves_an_existing_destination_unchanged() {
        let (guard, path) = Dir::new("install-fail.json");
        std::fs::write(&path, b"old contents").unwrap();
        let mut fault = fault_at(Stage::Install);
        let error = staged_write(&path, b"new contents", true, &mut fault)
            .unwrap_err();
        assert!(matches!(error, Error::Io { .. }), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), b"old contents");
        assert!(staging_leftovers(guard.0.path()).is_empty());
    }

    #[test]
    fn failed_install_leaves_no_partial_destination() {
        let (guard, path) = Dir::new("install-absent.json");
        let mut fault = fault_at(Stage::Install);
        staged_write(&path, b"new contents", false, &mut fault).unwrap_err();
        assert!(
            !path.exists(),
            "the final name must appear only at the atomic install"
        );
        assert!(staging_leftovers(guard.0.path()).is_empty());
    }

    #[test]
    fn installed_decks_hold_the_complete_contents() {
        // A payload far larger than any single write buffer proves the
        // destination only ever references fully staged content.
        let payload: Vec<u8> =
            (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let (guard, path) = Dir::new("large.json");
        write_deck(&path, &payload, false).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), payload);
        assert!(staging_leftovers(guard.0.path()).is_empty());
        let (guard, forced) = Dir::new("large-forced.json");
        std::fs::write(&forced, b"tiny").unwrap();
        write_deck(&forced, &payload, true).unwrap();
        assert_eq!(std::fs::read(&forced).unwrap(), payload);
        assert!(staging_leftovers(guard.0.path()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn forced_replacement_keeps_owner_read_write() {
        use std::os::unix::fs::PermissionsExt;
        let (_guard, path) = Dir::new("mode.json");
        std::fs::write(&path, b"old").unwrap();
        // Replacing an existing deck stages a fresh file with the
        // default mode (umask); owner read/write must remain.
        write_deck(&path, b"new", true).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o600, 0o600);
    }
}
