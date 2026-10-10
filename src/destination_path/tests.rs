use super::*;

#[test]
fn missing_suffix_requires_directory_even_when_ancestor_is_a_file() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("file");
    std::fs::write(&file, "original").unwrap();
    let canonical = std::fs::canonicalize(&file).unwrap();
    assert_eq!(
        resolve_existing(&file, vec![]).unwrap(),
        (canonical.clone(), canonical)
    );
    let error = resolve_existing(&file, vec![OsString::from("deck.json")])
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotADirectory);
    assert_eq!(std::fs::read_to_string(file).unwrap(), "original");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn missing_components_are_restored_in_order_without_creation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("missing/config/deck.json");
    let ancestor = std::fs::canonicalize(root.path()).unwrap();
    assert_eq!(
        resolve(&path).unwrap(),
        (ancestor.join("missing/config/deck.json"), ancestor)
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn missing_non_utf8_components_are_preserved_without_creation() {
    use std::os::unix::ffi::OsStringExt;
    let root = tempfile::tempdir().unwrap();
    let name = OsString::from_vec(vec![b'd', 0xff]);
    let path = root.path().join(&name).join("deck.json");
    let ancestor = std::fs::canonicalize(root.path()).unwrap();
    assert_eq!(
        resolve(&path).unwrap(),
        (ancestor.join(name).join("deck.json"), ancestor)
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
