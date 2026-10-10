use super::*;

#[test]
fn relative_and_absolute_aliases_match_without_creating_missing_parents() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let path = dir.path().join("missing/config/labels.json");
    let absolute = std::env::current_dir().unwrap().join(&path);
    assert!(same_destination(&path, &absolute).unwrap());
    assert!(!path.parent().unwrap().exists());
    assert!(!same_destination(&path, &dir.path().join("other.json")).unwrap());
}

#[cfg(unix)]
#[test]
fn file_and_parent_symlinks_resolve_to_effective_destination() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let alias = dir.path().join("alias");
    symlink(&real, &alias).unwrap();
    assert!(
        same_destination(
            &real.join("missing/labels.json"),
            &alias.join("missing/labels.json")
        )
        .unwrap()
    );
    let path = real.join("labels.json");
    std::fs::write(&path, "[]").unwrap();
    let link = dir.path().join("deck.json");
    symlink(&path, &link).unwrap();
    assert!(same_destination(&path, &link).unwrap());
    std::fs::remove_file(&path).unwrap();
    assert!(same_destination(&path, &link).is_err());
}

#[cfg(windows)]
#[test]
fn missing_final_component_case_aliases_match() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        same_destination(
            &dir.path().join("labels.json"),
            &dir.path().join("LABELS.JSON")
        )
        .unwrap()
    );
}

#[test]
fn final_case_alias_comparison_respects_existing_filesystem() {
    let dir = tempfile::tempdir().unwrap();
    let lower = dir.path().join("labels.json");
    let upper = dir.path().join("LABELS.JSON");
    std::fs::write(&lower, "[]").unwrap();
    #[cfg(target_os = "linux")]
    if upper.exists()
        && std::fs::canonicalize(&lower).unwrap()
            != std::fs::canonicalize(&upper).unwrap()
    {
        assert_eq!(
            same_destination(&lower, &upper).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        return;
    }
    assert_eq!(same_destination(&lower, &upper).unwrap(), upper.exists());
}

#[test]
fn absent_case_aliases_follow_probed_filesystem_policy_without_writes() {
    let dir = tempfile::tempdir().unwrap();
    let left = resolve(&dir.path().join("missing/labels.json")).unwrap();
    let right = resolve(&dir.path().join("MISSING/LABELS.JSON")).unwrap();
    assert!(same_resolved(&left, &right, |_| Ok(true)).unwrap());
    assert!(!same_resolved(&left, &right, |_| Ok(false)).unwrap());
    assert!(
        same_resolved(&left, &right, |_| Err(io::Error::other("probe")))
            .is_err()
    );
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    assert!(
        same_resolved(&left, &left, |_| Err(io::Error::other("unused")))
            .unwrap()
    );
    let other = resolve(&dir.path().join("other.json")).unwrap();
    assert!(
        !same_resolved(&left, &other, |_| Err(io::Error::other("unused")))
            .unwrap()
    );
}

#[cfg(unix)]
#[test]
fn directory_case_probe_respects_the_current_filesystem() {
    let dir = tempfile::Builder::new()
        .prefix("CaseProbe")
        .tempdir()
        .unwrap();
    let path = dir.path().join("Probe");
    std::fs::create_dir(&path).unwrap();
    assert_eq!(
        case_insensitive(&path).unwrap(),
        dir.path().join("probe").exists()
    );
}

#[cfg(unix)]
#[test]
fn directory_case_probe_uses_children_and_refuses_to_guess_from_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("12345");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("File"), "").unwrap();
    assert_eq!(
        case_insensitive(&path).unwrap_err().kind(),
        io::ErrorKind::Unsupported
    );
    std::fs::create_dir(path.join("Child")).unwrap();
    assert_eq!(
        case_insensitive(&path).unwrap(),
        path.join("child").exists()
    );
    assert!(case_insensitive(&path.join("absent")).is_err());
}

#[test]
fn different_existing_ancestors_are_not_merged_by_missing_case_comparison() {
    let left = Resolved {
        path: "Parent/labels.json".into(),
        ancestor: "Parent".into(),
    };
    let right = Resolved {
        path: "PARENT/LABELS.JSON".into(),
        ancestor: "PARENT".into(),
    };
    assert!(
        !same_resolved(&left, &right, |_| Err(io::Error::other("unused")))
            .unwrap()
    );
}

#[test]
fn unresolved_unicode_aliases_are_rejected_only_when_identity_is_ambiguous() {
    let dir = tempfile::tempdir().unwrap();
    let left = resolve(&dir.path().join("Étage/labels.json")).unwrap();
    let right = resolve(&dir.path().join("étage/labels.json")).unwrap();
    assert_eq!(
        same_resolved(&left, &right, |_| Ok(true))
            .unwrap_err()
            .kind(),
        io::ErrorKind::Unsupported
    );
    assert_eq!(
        same_resolved(&left, &right, |_| Ok(false))
            .unwrap_err()
            .kind(),
        io::ErrorKind::Unsupported
    );
    let distinct = resolve(&dir.path().join("étage/other.json")).unwrap();
    assert!(
        !same_resolved(&left, &distinct, |_| Err(io::Error::other("unused")))
            .unwrap()
    );
    let matching = resolve(&dir.path().join("Étage/LABELS.JSON")).unwrap();
    assert!(same_resolved(&left, &matching, |_| Ok(true)).unwrap());
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn missing_unicode_parent_case_aliases_cannot_enable_an_unsafe_save() {
    let dir = tempfile::tempdir().unwrap();
    let left = dir.path().join("Étage/labels.json");
    let right = dir.path().join("étage/labels.json");
    assert_eq!(
        same_destination(&left, &right).unwrap_err().kind(),
        io::ErrorKind::Unsupported
    );
    assert!(!left.parent().unwrap().exists());
    assert!(!right.parent().unwrap().exists());
}

#[cfg(target_os = "macos")]
#[test]
fn missing_final_case_aliases_respect_the_macos_volume() {
    let dir = tempfile::tempdir().unwrap();
    let probe = dir.path().join("Probe");
    std::fs::write(&probe, "").unwrap();
    let insensitive = dir.path().join("probe").exists();
    assert_eq!(
        same_destination(
            &dir.path().join("labels.json"),
            &dir.path().join("LABELS.JSON")
        )
        .unwrap(),
        insensitive
    );
}

#[cfg(target_os = "linux")]
#[test]
fn linux_entry_evidence_distinguishes_hard_links_from_uncertain_aliases() {
    use std::ffi::{OsStr, OsString};
    let left = Some(OsStr::new("labels.json"));
    let right = Some(OsStr::new("LABELS.JSON"));
    let names = vec![OsString::from("labels.json")];
    assert_eq!(
        distinct_linux_entries(left, right, &names)
            .unwrap_err()
            .kind(),
        io::ErrorKind::Unsupported
    );
    let names =
        vec![OsString::from("labels.json"), OsString::from("LABELS.JSON")];
    assert!(!distinct_linux_entries(left, right, &names).unwrap());
}

#[cfg(target_os = "linux")]
#[test]
fn linux_missing_case_aliases_report_uncertainty_without_creating_parents() {
    let dir = tempfile::tempdir().unwrap();
    for (left, right) in [
        ("labels.json", "LABELS.JSON"),
        ("missing/labels.json", "MISSING/LABELS.JSON"),
        ("Étage/labels.json", "étage/labels.json"),
    ] {
        assert_eq!(
            same_destination(&dir.path().join(left), &dir.path().join(right))
                .unwrap_err()
                .kind(),
            io::ErrorKind::Unsupported
        );
    }
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
}

#[test]
fn non_directory_ancestors_are_invalid_even_for_identical_paths() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("file");
    std::fs::write(&file, "original").unwrap();
    for suffix in ["labels.json", "missing/config/labels.json"] {
        let invalid = file.join(suffix);
        assert!(same_destination(&invalid, &invalid).is_err());
        assert!(
            same_destination(&root.path().join("valid.json"), &invalid)
                .is_err()
        );
    }
    assert_eq!(std::fs::read_to_string(file).unwrap(), "original");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn invalid_symlink_ancestors_propagate_errors_without_creating_suffixes() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file"), "original").unwrap();
    for target in ["file", "missing"] {
        let link = root.path().join(format!("link-{target}"));
        std::os::unix::fs::symlink(target, &link).unwrap();
        for suffix in ["labels.json", "nested/labels.json"] {
            assert!(
                same_destination(&link.join(suffix), &link.join(suffix))
                    .is_err()
            );
        }
        assert_eq!(std::fs::read_link(link).unwrap(), Path::new(target));
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 3);
}
