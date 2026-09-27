//! Crash-safe staged writes for canonical deck files.
//!
//! Lifecycle, with one explicit commit point:
//!
//! ```text
//! PRE-COMMIT   create staging file beside the destination
//!              write_all → flush → sync_all (staging)
//! COMMIT       force:      atomic replace-existing rename
//!              no force:   atomic install-if-absent rename
//! POST-COMMIT  destination-directory durability sync (where supported)
//! ```
//!
//! The staging file is hidden and uniquely named (`.labeldeck-tmp-*`)
//! in the destination's own directory, so the commit never crosses a
//! filesystem boundary. A Drop guard removes the staging name on
//! every handled failure: a failed export leaves no partial
//! destination and no staging leftovers, and the destination name
//! only ever references complete, fsynced contents. Only a hard
//! process crash can strand a staging file.
//!
//! # Commit operations
//!
//! * `force` commits with [`std::fs::rename`], an atomic same-volume
//!   replace on Linux/macOS (POSIX `rename(2)`) and on Windows
//!   (`MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`, plus a
//!   POSIX-semantics `SetFileInformationByHandle` fallback). An
//!   existing destination is never truncated or removed first.
//! * without `force` the commit uses [`renamore::rename_exclusive`]:
//!   `renameat2(RENAME_NOREPLACE)` on Linux, `renamex_np(RENAME_EXCL)`
//!   on macOS, and `MoveFileExW` with no replace flag on Windows. No
//!   hard links are involved, so filesystems without hard-link
//!   support (FAT/FAT32/exFAT) are not penalised; if the OS/files
//!   system genuinely lacks an atomic no-replace rename, the commit
//!   returns a clear I/O error instead of a non-atomic fallback. An
//!   occupied destination — however it came to exist, including
//!   between staging and commit — fails atomically with
//!   [`Error::OutputExists`] and is never modified.
//!
//! # Durability
//!
//! The staged contents are fsynced before the commit. On Unix the
//! destination directory is fsynced after it so the installed name
//! itself survives a crash; on Windows `std` cannot open a directory
//! for syncing, so that step is intentionally absent and no warning
//! is emitted for it. A post-commit sync that is *attempted and
//! fails* does not fail the export — the deck is already installed —
//! but is reported as [`WriteOutcome::DurabilityUnconfirmed`]. The
//! guarantee is crash-safe contents replacement, not
//! storage-controller-level durability on every filesystem.
//!
//! # Permissions
//!
//! The staging file is created with the platform's default mode for
//! new files (the usual umask on Unix), so a forced replacement
//! resets the destination's mode to that default rather than
//! preserving the previous mode. Decks are not secrets, and the
//! global deck lives inside the private (0700) configuration
//! directory, which is what protects it.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{Error, Result};

/// Result of a staged write, distinguishing a failed install from a
/// successful install whose strongest durability confirmation failed.
#[derive(Debug)]
pub(crate) enum WriteOutcome {
    /// Installed at the commit point and the destination-directory
    /// sync confirmed it (or the platform has no such step).
    Durable,
    /// Installed at the commit point, but the post-commit
    /// destination-directory sync failed. The deck contents are
    /// complete; the error is carried for reporting.
    DurabilityUnconfirmed(std::io::Error),
}

/// Points where a staged write can be made to fail, so tests can
/// prove the failure guarantees deterministically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    /// Before the staged contents are written and synced.
    Contents,
    /// Immediately before the atomic commit.
    Commit,
    /// Immediately before the post-commit directory durability sync.
    DirectorySync,
}

fn no_fault(_: Stage) -> std::io::Result<()> {
    Ok(())
}

/// Monotonic suffix for staging names within this process.
static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A staging file's name; dropped exactly when the write is done
/// (successfully or not) so leftovers never survive a handled error.
struct Staged {
    path: PathBuf,
}

impl Drop for Staged {
    fn drop(&mut self) {
        // After a successful commit the staging name is already gone
        // (renamed away); after any failure this deletes the
        // leftover. Both are fine to attempt unconditionally.
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The directory to stage in: the destination's own directory, so
/// the commit never crosses a filesystem boundary. A bare
/// destination filename stages in the current directory.
fn staging_dir(dest: &Path) -> &Path {
    match dest.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

fn write_error(path: &Path, error: std::io::Error) -> Error {
    Error::Io {
        context: format!("could not write {}", path.display()),
        message: error.to_string(),
    }
}

/// Write the canonical deck to `dest` crash-safely. See the module
/// documentation for the full contract.
pub(crate) fn write_deck(
    dest: &Path,
    bytes: &[u8],
    force: bool,
) -> Result<WriteOutcome> {
    staged_write(dest, bytes, force, &mut no_fault)
}

/// The staged-write state machine shared by both commit modes.
pub(crate) fn staged_write(
    dest: &Path,
    bytes: &[u8],
    force: bool,
    fault: &mut dyn FnMut(Stage) -> std::io::Result<()>,
) -> Result<WriteOutcome> {
    let effective = effective_destination(dest, force)?;
    let dir = staging_dir(&effective);
    let (mut file, staged) = create_staging_file(dir, dest)?;

    fault(Stage::Contents).map_err(|e| write_error(dest, e))?;
    file.write_all(bytes).map_err(|e| write_error(dest, e))?;
    file.flush().map_err(|e| write_error(dest, e))?;
    file.sync_all().map_err(|e| write_error(dest, e))?;

    // COMMIT POINT: from here on, the effective destination holds the
    // new deck.
    fault(Stage::Commit).map_err(|e| write_error(dest, e))?;
    if force {
        std::fs::rename(&staged.path, &effective)
            .map_err(|e| write_error(dest, e))?;
    } else {
        match renamore::rename_exclusive(&staged.path, &effective) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(Error::OutputExists {
                    path: dest.to_path_buf(),
                });
            }
            Err(e) => return Err(write_error(dest, e)),
        }
    }

    // POST-COMMIT: the deck is installed; a failing durability sync
    // must not present the export as though it never happened.
    let durability =
        fault(Stage::DirectorySync).and_then(|()| sync_directory(dir));
    Ok(match durability {
        Ok(()) => WriteOutcome::Durable,
        Err(e) => WriteOutcome::DurabilityUnconfirmed(e),
    })
}

/// Resolve the path the commit will actually act on.
///
/// Without `force` this is always the given path unchanged: the
/// no-replace commit decides atomically, and any existing entry — a
/// regular file, a directory, a valid or dangling symlink — refuses
/// installation with [`Error::OutputExists`] without following or
/// modifying it.
///
/// With `force`:
///
/// * an absent destination installs at the given path;
/// * a regular file is replaced at the given path;
/// * a **valid symlink** is honoured as the user's statement that
///   the deck lives elsewhere: the whole chain is resolved once
///   (relative targets resolve against the directory containing
///   each link, per normal filesystem semantics) and the **target**
///   is atomically replaced while every link is preserved;
/// * a **dangling symlink** fails clearly (`canonicalize` cannot
///   resolve the target) and the link is left untouched — the
///   missing target is not silently created;
/// * a directory or other non-regular destination fails clearly
///   rather than being replaced.
///
/// Practical race semantics, documented honestly: this is an
/// ordinary user CLI, not a hardened path-resolution sandbox. The
/// chain is resolved exactly once, staging happens beside the
/// resolved target, and the commit never re-follows the original
/// link — so a link swapped after resolution cannot redirect the
/// write, though it also means the previously resolved target, not
/// any new one, receives the replacement.
fn effective_destination<'a>(
    dest: &'a Path,
    force: bool,
) -> Result<std::borrow::Cow<'a, Path>> {
    if !force {
        return Ok(std::borrow::Cow::Borrowed(dest));
    }
    match std::fs::symlink_metadata(dest) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(std::borrow::Cow::Borrowed(dest))
        }
        Err(e) => Err(Error::Io {
            context: format!(
                "could not examine {} while preparing the export",
                dest.display()
            ),
            message: e.to_string(),
        }),
        Ok(meta) if meta.file_type().is_symlink() => {
            let target =
                std::fs::canonicalize(dest).map_err(|e| Error::Io {
                    context: format!(
                        "destination {} is a symbolic link whose target \
                         could not be resolved",
                        dest.display()
                    ),
                    message: e.to_string(),
                })?;
            match std::fs::symlink_metadata(&target) {
                Ok(t) if t.is_dir() => Err(Error::Io {
                    context: format!(
                        "destination {} resolves to {} which is a \
                         directory",
                        dest.display(),
                        target.display()
                    ),
                    message: "a deck file cannot replace a directory"
                        .to_string(),
                }),
                Ok(t) if !t.is_file() => Err(Error::Io {
                    context: format!(
                        "destination {} resolves to {} which is not a \
                         regular file",
                        dest.display(),
                        target.display()
                    ),
                    message: "only regular files can be replaced".to_string(),
                }),
                Ok(_) => Ok(std::borrow::Cow::Owned(target)),
                Err(e) => Err(Error::Io {
                    context: format!(
                        "could not examine {} resolved from symbolic \
                         link {}",
                        target.display(),
                        dest.display()
                    ),
                    message: e.to_string(),
                }),
            }
        }
        Ok(meta) if meta.is_dir() => Err(Error::Io {
            context: format!("destination {} is a directory", dest.display()),
            message: "a deck file cannot replace a directory".to_string(),
        }),
        Ok(meta) if !meta.is_file() => Err(Error::Io {
            context: format!(
                "destination {} is not a regular file",
                dest.display()
            ),
            message: "only regular files can be replaced".to_string(),
        }),
        Ok(_) => Ok(std::borrow::Cow::Borrowed(dest)),
    }
}

fn create_staging_file(
    dir: &Path,
    dest: &Path,
) -> Result<(std::fs::File, Staged)> {
    let mut attempts = 0u32;
    loop {
        let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = dir
            .join(format!(".labeldeck-tmp-{}-{sequence}", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((file, Staged { path: candidate })),
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
    }
}

/// fsync the directory holding the destination so the installed name
/// itself is durable. Omitted on Windows: `std` cannot open a
/// directory there without backup-semantics privileges.
#[cfg(unix)]
fn sync_directory(dir: &Path) -> std::io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Staging-file names currently present in `dir` (test assertions).
#[cfg(test)]
fn staging_leftovers(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .expect("read dir")
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".labeldeck-tmp-"))
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;

    /// Keeps the temp directory alive for the test's duration.
    struct Dir(tempfile::TempDir);

    impl Dir {
        fn new(name: &str) -> (Self, PathBuf) {
            let guard = tempfile::tempdir().expect("temp dir");
            let path = guard.path().join(name);
            (Self(guard), path)
        }
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
    fn without_force_a_new_destination_is_created_durably() {
        let (guard, path) = Dir::new("new.json");
        let outcome = write_deck(&path, b"[]", false).unwrap();
        assert!(matches!(outcome, WriteOutcome::Durable));
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
        // when the no-replace commit runs — created before the call
        // or between staging and commit — fails atomically at the
        // commit, never an overwrite.
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
        let outcome = write_deck(&path, b"new contents", true).unwrap();
        assert!(matches!(outcome, WriteOutcome::Durable));
        assert_eq!(std::fs::read(&path).unwrap(), b"new contents");
        assert!(staging_leftovers(guard.0.path()).is_empty());
    }

    #[test]
    fn with_force_an_absent_destination_is_created() {
        let (guard, path) = Dir::new("forced-new.json");
        let outcome = write_deck(&path, b"fresh", true).unwrap();
        assert!(matches!(outcome, WriteOutcome::Durable));
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
            "force must not touch the destination before the commit"
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
    fn failed_commit_leaves_an_existing_destination_unchanged() {
        let (guard, path) = Dir::new("commit-fail.json");
        std::fs::write(&path, b"old contents").unwrap();
        let mut fault = fault_at(Stage::Commit);
        let error = staged_write(&path, b"new contents", true, &mut fault)
            .unwrap_err();
        assert!(matches!(error, Error::Io { .. }), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), b"old contents");
        assert!(staging_leftovers(guard.0.path()).is_empty());
    }

    #[test]
    fn failed_commit_leaves_no_partial_destination() {
        let (guard, path) = Dir::new("commit-absent.json");
        let mut fault = fault_at(Stage::Commit);
        staged_write(&path, b"new contents", false, &mut fault).unwrap_err();
        assert!(
            !path.exists(),
            "the final name must appear only at the atomic commit"
        );
        assert!(staging_leftovers(guard.0.path()).is_empty());
    }

    #[test]
    fn failed_directory_sync_reports_installed_but_unconfirmed() {
        let (guard, path) = Dir::new("durability-fail.json");
        let mut fault = fault_at(Stage::DirectorySync);
        let outcome =
            staged_write(&path, b"complete deck", true, &mut fault).unwrap();
        // The deck is fully installed and is not rolled back; the
        // outcome merely reports that durability was not confirmed.
        match outcome {
            WriteOutcome::DurabilityUnconfirmed(error) => {
                assert_eq!(error.to_string(), "injected failure");
            }
            other => panic!("expected DurabilityUnconfirmed, got {other:?}"),
        }
        assert_eq!(std::fs::read(&path).unwrap(), b"complete deck");
        assert!(staging_leftovers(guard.0.path()).is_empty());
    }

    #[test]
    fn installed_decks_hold_the_complete_contents() {
        // A payload far larger than any single write buffer proves the
        // destination only ever references fully staged content.
        let payload: Vec<u8> =
            (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let (guard, path) = Dir::new("large.json");
        let outcome = write_deck(&path, &payload, false).unwrap();
        assert!(matches!(outcome, WriteOutcome::Durable));
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

/// Symlink semantics are exercised on Unix, where CI can create
/// symlinks unconditionally. The production code handles symlinks on
/// every platform; Windows CI cannot rely on symlink-creation
/// privileges, so coverage there is the portable suite above.
#[cfg(unix)]
#[cfg(test)]
mod symlink_tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn is_symlink(path: &Path) -> bool {
        std::fs::symlink_metadata(path)
            .expect("symlink_metadata")
            .file_type()
            .is_symlink()
    }

    #[test]
    fn forced_export_through_an_absolute_symlink_replaces_the_target() {
        let guard = tempfile::tempdir().expect("temp dir");
        let shared = guard.path().join("shared");
        let work = guard.path().join("work");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        let target = shared.join("labels.json");
        std::fs::write(&target, b"OLD").unwrap();
        let deck = work.join("labels.json");
        symlink(&target, &deck).unwrap();

        write_deck(&deck, b"NEW", true).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"NEW");
        assert!(is_symlink(&deck), "the symlink itself must survive");
        assert_eq!(
            std::fs::read_link(&deck).unwrap(),
            target,
            "the symlink target must be unchanged"
        );
    }

    #[test]
    fn forced_export_through_a_relative_symlink_replaces_the_target() {
        let guard = tempfile::tempdir().expect("temp dir");
        let shared = guard.path().join("shared");
        let work = guard.path().join("work");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        let target = shared.join("labels.json");
        std::fs::write(&target, b"OLD").unwrap();
        let deck = work.join("labels.json");
        // Relative to the directory containing the link, not the
        // process working directory.
        symlink("../shared/labels.json", &deck).unwrap();

        write_deck(&deck, b"NEW", true).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"NEW");
        assert!(is_symlink(&deck));
        assert_eq!(
            std::fs::read_link(&deck).unwrap(),
            Path::new("../shared/labels.json")
        );
    }

    #[test]
    fn forced_export_through_a_symlink_chain_updates_the_final_target() {
        let guard = tempfile::tempdir().expect("temp dir");
        let dir = guard.path().join("links");
        std::fs::create_dir_all(&dir).unwrap();
        let final_target = guard.path().join("real-deck.json");
        std::fs::write(&final_target, b"OLD").unwrap();
        symlink("../real-deck.json", dir.join("second-link.json")).unwrap();
        symlink("second-link.json", dir.join("first-link.json")).unwrap();

        write_deck(&dir.join("first-link.json"), b"NEW", true).unwrap();

        assert_eq!(std::fs::read(&final_target).unwrap(), b"NEW");
        assert!(is_symlink(&dir.join("first-link.json")));
        assert!(is_symlink(&dir.join("second-link.json")));
        assert!(
            !dir.join("real-deck.json").exists(),
            "no stray file beside the links"
        );
    }

    #[test]
    fn without_force_a_symlink_destination_refuses_and_keeps_target() {
        let guard = tempfile::tempdir().expect("temp dir");
        let target = guard.path().join("target.json");
        std::fs::write(&target, b"OLD").unwrap();
        let deck = guard.path().join("labels.json");
        symlink("target.json", &deck).unwrap();

        let error = write_deck(&deck, b"NEW", false).unwrap_err();
        assert!(matches!(error, Error::OutputExists { .. }), "{error}");
        assert_eq!(std::fs::read(&target).unwrap(), b"OLD");
        assert!(is_symlink(&deck));
    }

    #[test]
    fn without_force_a_dangling_symlink_refuses_and_keeps_link() {
        let guard = tempfile::tempdir().expect("temp dir");
        let deck = guard.path().join("labels.json");
        symlink("does-not-exist.json", &deck).unwrap();

        let error = write_deck(&deck, b"NEW", false).unwrap_err();
        assert!(matches!(error, Error::OutputExists { .. }), "{error}");
        assert!(is_symlink(&deck));
        assert!(
            !guard.path().join("does-not-exist.json").exists(),
            "the missing target must not be created"
        );
    }

    #[test]
    fn forced_export_to_a_dangling_symlink_fails_and_preserves_link() {
        let guard = tempfile::tempdir().expect("temp dir");
        let deck = guard.path().join("labels.json");
        symlink("nowhere.json", &deck).unwrap();

        let error = write_deck(&deck, b"NEW", true).unwrap_err();
        let text = error.to_string();
        assert!(text.contains("symbolic link"), "{text}");
        assert!(text.contains("labels.json"), "{text}");
        assert!(is_symlink(&deck), "the dangling link must survive");
        assert!(
            !guard.path().join("nowhere.json").exists(),
            "the missing target must not be silently created"
        );
        assert!(staging_leftovers(guard.path()).is_empty());
    }

    #[test]
    fn forced_export_through_a_symlink_stages_beside_the_resolved_target() {
        let guard = tempfile::tempdir().expect("temp dir");
        let link_dir = guard.path().join("links");
        let target_dir = guard.path().join("targets");
        std::fs::create_dir_all(&link_dir).unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();
        let target = target_dir.join("labels.json");
        std::fs::write(&target, b"OLD").unwrap();
        let deck = link_dir.join("labels.json");
        symlink(&target, &deck).unwrap();

        // Fail right after staging is created; at that moment the
        // staging file must live beside the resolved target, and the
        // symlink's directory must hold nothing but the link.
        let mut staged_beside_target = None;
        let mut fault = |stage: Stage| -> std::io::Result<()> {
            if stage == Stage::Contents {
                staged_beside_target =
                    Some(staging_leftovers(&target_dir).len());
                Err(std::io::Error::other("stop before write"))
            } else {
                Ok(())
            }
        };
        assert!(staged_write(&deck, b"NEW", true, &mut fault).is_err());
        assert_eq!(staged_beside_target, Some(1));
        assert!(
            staging_leftovers(&link_dir).is_empty(),
            "nothing may be staged beside the symlink"
        );
        // The failed pre-commit attempt changed nothing.
        assert_eq!(std::fs::read(&target).unwrap(), b"OLD");
    }

    #[test]
    fn forced_export_to_a_directory_fails_without_touching_it() {
        let guard = tempfile::tempdir().expect("temp dir");
        let dir = guard.path().join("deck-dir");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("inner.txt"), b"precious").unwrap();

        let error = write_deck(&dir, b"NEW", true).unwrap_err();
        let text = error.to_string();
        assert!(text.contains("deck-dir"), "{text}");
        assert!(text.contains("directory"), "{text}");
        assert!(dir.is_dir(), "the directory must survive");
        assert_eq!(std::fs::read(dir.join("inner.txt")).unwrap(), b"precious");
        assert!(staging_leftovers(guard.path()).is_empty());
    }
}
