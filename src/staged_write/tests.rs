//! Staged-write tests, out of line so the production file stays
//! implementation and documentation.
//!
//! The child module reaches the parent's private items through
//! normal Rust module privacy (`use super::*`).

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

/// Staging-file names currently present in `dir`.
fn staging_leftovers(dir: &Path) -> Vec<String> {
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
    let error =
        staged_write(&path, b"new contents", true, &mut fault).unwrap_err();
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
    let error =
        staged_write(&path, b"new contents", true, &mut fault).unwrap_err();
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
    let payload: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
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

/// Symlink semantics are exercised on Unix, where CI can create
/// symlinks unconditionally. The production code handles symlinks on
/// every platform; Windows CI cannot rely on symlink-creation
/// privileges, so coverage there is the portable suite above.
#[cfg(unix)]
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

#[test]
fn export_preflight_distinguishes_missing_parents_from_io_errors() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing/config/labels.json");
    for force in [false, true] {
        preflight(&missing, force).unwrap();
    }
    assert!(!missing.parent().unwrap().exists());
    let file = root.path().join("file");
    std::fs::write(&file, "original").unwrap();
    for force in [false, true] {
        assert!(matches!(
            preflight(&file.join("labels.json"), force),
            Err(Error::Io { .. })
        ));
    }
    assert_eq!(std::fs::read_to_string(file).unwrap(), "original");
}

#[test]
fn preflight_checks_files_directories_and_absence_without_mutation() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("file.json");
    let dir = root.path().join("directory");
    std::fs::write(&file, "original").unwrap();
    std::fs::create_dir(&dir).unwrap();
    for force in [false, true] {
        for missing in ["new.json", "missing/parent/new.json"] {
            preflight(&root.path().join(missing), force).unwrap();
        }
        if force {
            preflight(&file, force).unwrap();
            assert!(matches!(preflight(&dir, force), Err(Error::Io { .. })));
        } else {
            for occupied in [&file, &dir] {
                assert!(matches!(
                    preflight(occupied, force),
                    Err(Error::OutputExists { .. })
                ));
            }
        }
    }
    assert_eq!(std::fs::read_to_string(file).unwrap(), "original");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
}

#[cfg(unix)]
#[test]
fn preflight_checks_symlink_targets_without_modifying_links_or_targets() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file"), "original").unwrap();
    std::fs::create_dir(root.path().join("dir")).unwrap();
    for target in ["file", "dir", "missing"] {
        let link = root.path().join(format!("link-{target}"));
        std::os::unix::fs::symlink(target, &link).unwrap();
        assert!(matches!(
            preflight(&link, false),
            Err(Error::OutputExists { .. })
        ));
        if target == "file" {
            preflight(&link, true).unwrap();
        } else {
            assert!(matches!(preflight(&link, true), Err(Error::Io { .. })));
        }
        assert_eq!(std::fs::read_link(link).unwrap(), Path::new(target));
    }
    assert_eq!(
        std::fs::read_to_string(root.path().join("file")).unwrap(),
        "original"
    );
    assert_eq!(
        std::fs::read_dir(root.path().join("dir")).unwrap().count(),
        0
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 5);
}

#[cfg(unix)]
#[test]
fn preflight_rejects_unsupported_entries_and_symlink_targets() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("socket");
    let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let link = root.path().join("link");
    std::os::unix::fs::symlink("socket", &link).unwrap();
    for path in [&socket, &link] {
        assert!(matches!(
            preflight(path, false),
            Err(Error::OutputExists { .. })
        ));
        let error = preflight(path, true).unwrap_err();
        assert!(error.to_string().contains("not a regular file"));
    }
    assert_eq!(std::fs::read_link(link).unwrap(), Path::new("socket"));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
}

#[test]
fn preflight_rejects_immediate_and_deeper_non_directory_ancestors() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("file");
    std::fs::write(&file, "original").unwrap();
    for force in [false, true] {
        for suffix in ["labels.json", "missing/config/labels.json"] {
            assert!(matches!(
                preflight(&file.join(suffix), force),
                Err(Error::Io { .. })
            ));
        }
    }
    assert_eq!(std::fs::read_to_string(file).unwrap(), "original");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn preflight_validates_symlink_parents_without_mutation() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("dir")).unwrap();
    std::fs::write(root.path().join("file"), "original").unwrap();
    for target in ["dir", "file", "missing"] {
        let link = root.path().join(format!("link-{target}"));
        std::os::unix::fs::symlink(target, &link).unwrap();
        for force in [false, true] {
            for suffix in ["labels.json", "nested/config/labels.json"] {
                let result = preflight(&link.join(suffix), force);
                if target == "dir" {
                    result.unwrap();
                } else {
                    assert!(matches!(result, Err(Error::Io { .. })));
                }
            }
        }
        assert_eq!(std::fs::read_link(link).unwrap(), Path::new(target));
    }
    assert_eq!(
        std::fs::read_dir(root.path().join("dir")).unwrap().count(),
        0
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 5);
}
