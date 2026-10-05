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
    let (file, staged) = create_staging_file(dir, dest)?;
    stage_contents(file, bytes, dest, fault)?;

    // COMMIT POINT: from here on, the effective destination holds the
    // new deck.
    commit_staged(&staged, &effective, dest, force, fault)?;

    // POST-COMMIT: the deck is installed; a failing durability sync
    // must not present the export as though it never happened.
    Ok(post_commit_outcome(dir, fault))
}

/// PRE-COMMIT: write the complete replacement into the staging file
/// and make it durable before any destination is touched.
fn stage_contents(
    mut file: std::fs::File,
    bytes: &[u8],
    dest: &Path,
    fault: &mut dyn FnMut(Stage) -> std::io::Result<()>,
) -> Result<()> {
    fault(Stage::Contents).map_err(|e| write_error(dest, e))?;
    file.write_all(bytes).map_err(|e| write_error(dest, e))?;
    file.flush().map_err(|e| write_error(dest, e))?;
    file.sync_all().map_err(|e| write_error(dest, e))
}

/// COMMIT: install the staged file at the effective destination.
///
/// `force` atomically replaces the destination; otherwise the
/// exclusive rename installs only if the destination is absent, and
/// an occupied destination — however it came to exist — becomes
/// [`Error::OutputExists`] with the destination untouched.
fn commit_staged(
    staged: &Staged,
    effective: &Path,
    dest: &Path,
    force: bool,
    fault: &mut dyn FnMut(Stage) -> std::io::Result<()>,
) -> Result<()> {
    fault(Stage::Commit).map_err(|e| write_error(dest, e))?;
    if force {
        return std::fs::rename(&staged.path, effective)
            .map_err(|e| write_error(dest, e));
    }
    match renamore::rename_exclusive(&staged.path, effective) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(Error::OutputExists {
                path: dest.to_path_buf(),
            })
        }
        Err(e) => Err(write_error(dest, e)),
    }
}

/// POST-COMMIT: report whether the installed name's durability was
/// confirmed. A sync that is attempted and fails is not a failed
/// export, so it never rolls anything back.
fn post_commit_outcome(
    dir: &Path,
    fault: &mut dyn FnMut(Stage) -> std::io::Result<()>,
) -> WriteOutcome {
    match fault(Stage::DirectorySync).and_then(|()| sync_directory(dir)) {
        Ok(()) => WriteOutcome::Durable,
        Err(e) => WriteOutcome::DurabilityUnconfirmed(e),
    }
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
    match examine_destination(dest)? {
        None => Ok(std::borrow::Cow::Borrowed(dest)),
        Some(meta) if meta.file_type().is_symlink() => {
            Ok(std::borrow::Cow::Owned(symlink_target(dest)?))
        }
        Some(meta) => {
            ensure_replaceable(dest, dest, &meta)?;
            Ok(std::borrow::Cow::Borrowed(dest))
        }
    }
}

/// Examine the destination entry itself, without following symlinks.
/// `None` means the destination is absent.
fn examine_destination(dest: &Path) -> Result<Option<std::fs::Metadata>> {
    match std::fs::symlink_metadata(dest) {
        Ok(meta) => Ok(Some(meta)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::Io {
            context: format!(
                "could not examine {} while preparing the export",
                dest.display()
            ),
            message: e.to_string(),
        }),
    }
}

/// Resolve a valid symlink destination, once, to its final target —
/// complete chain, relative targets included — and validate that the
/// target may be replaced.
fn symlink_target(dest: &Path) -> Result<PathBuf> {
    let target = std::fs::canonicalize(dest).map_err(|e| Error::Io {
        context: format!(
            "destination {} is a symbolic link whose target could not \
             be resolved",
            dest.display()
        ),
        message: e.to_string(),
    })?;
    let meta = std::fs::symlink_metadata(&target).map_err(|e| Error::Io {
        context: format!(
            "could not examine {} resolved from symbolic link {}",
            target.display(),
            dest.display()
        ),
        message: e.to_string(),
    })?;
    ensure_replaceable(dest, &target, &meta)?;
    Ok(target)
}

/// Refuse to replace directories and other non-regular entries,
/// whether reached directly or through a symlink.
fn ensure_replaceable(
    dest: &Path,
    path: &Path,
    meta: &std::fs::Metadata,
) -> Result<()> {
    if meta.is_file() {
        return Ok(());
    }
    let subject = if path == dest {
        format!("destination {}", dest.display())
    } else {
        format!(
            "destination {} resolves to {}",
            dest.display(),
            path.display()
        )
    };
    let (state, advice): (&str, &str) = if meta.is_dir() {
        ("is a directory", "a deck file cannot replace a directory")
    } else {
        (
            "is not a regular file",
            "only regular files can be replaced",
        )
    };
    Err(Error::Io {
        context: format!("{subject} {state}"),
        message: advice.to_string(),
    })
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

#[cfg(test)]
mod tests;

/// The warning shown when the deck was installed at the commit point
/// but the post-commit destination-directory sync failed. The export
/// itself succeeded; durability is merely unconfirmed.
pub(crate) fn durability_warning(error: &std::io::Error) -> String {
    format!(
        "warning: the deck was installed successfully, but filesystem \
         durability could not be confirmed because the destination \
         directory could not be synchronized: {error}"
    )
}
