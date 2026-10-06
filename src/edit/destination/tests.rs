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
