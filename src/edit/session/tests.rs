use super::*;
use crate::{edit::model::Draft, labels::Label};

fn document() -> Document {
    Document::from_labels(vec![
        Draft {
            name: "bug".into(),
            color: "ededed".into(),
            description: String::new(),
        }
        .label()
        .unwrap(),
    ])
}
fn host(root: &std::path::Path) -> SaveHost {
    SaveHost {
        local: root.join("labels.json"),
        config_dir: root.join("missing/config"),
        protected: None,
    }
}

#[test]
fn save_writes_canonical_full_deck_and_preserves_history() {
    let dir = tempfile::tempdir().unwrap();
    let host = host(dir.path());
    let mut document = document();
    let new = Draft {
        name: "new".into(),
        color: "ABCDEF".into(),
        description: String::new(),
    };
    document.create(new).unwrap();
    for target in [SaveTarget::Local, SaveTarget::Global] {
        let record = host.save(&document, target).unwrap();
        let text = std::fs::read_to_string(record.path).unwrap();
        let labels: Vec<Label> = crate::canonical::parse(&text).unwrap();
        assert_eq!(labels.len(), 2);
        assert_eq!(
            text,
            crate::canonical::to_json(&mut document.labels().unwrap())
        );
        assert!(document.can_undo());
    }
    assert!(document.undo());
    assert!(document.can_redo());
}

#[test]
fn validation_prevents_writes_and_directory_creation() {
    let dir = tempfile::tempdir().unwrap();
    let host = host(dir.path());
    let mut document = document();
    let mut duplicate = document.entries()[0].draft.clone();
    duplicate.name = "BUG".into();
    document.create(duplicate).unwrap();
    assert!(host.save(&document, SaveTarget::Global).is_err());
    assert!(!host.config_dir.exists());
}

#[test]
fn protected_destination_is_rechecked_before_every_save() {
    let dir = tempfile::tempdir().unwrap();
    let mut host = host(dir.path());
    host.protected = Some(host.local.clone());
    assert!(host.save(&document(), SaveTarget::Local).is_err());
    assert!(!host.local.exists());
    host.protected = None;
    std::fs::write(&host.local, "old").unwrap();
    host.save(&document(), SaveTarget::Local).unwrap();
    assert!(
        std::fs::read_to_string(&host.local)
            .unwrap()
            .contains("bug")
    );
}

#[test]
fn durability_warning_is_successful_record_and_write_failure_is_error() {
    let dir = tempfile::tempdir().unwrap();
    let host = host(dir.path());
    let record = host
        .save_with(&document(), SaveTarget::Local, |path, bytes, force| {
            crate::staged_write::staged_write(
                path,
                bytes,
                force,
                &mut |stage| {
                    if stage == crate::staged_write::Stage::DirectorySync {
                        Err(io::Error::other("injected durability failure"))
                    } else {
                        Ok(())
                    }
                },
            )
        })
        .unwrap();
    assert!(record.warning.unwrap().contains("installed successfully"));
    assert!(host.local.exists());
    let failed = host.save_with(&document(), SaveTarget::Local, |_, _, _| {
        Err(Error::Usage("injected write failure".into()))
    });
    assert!(failed.is_err());
}

#[cfg(unix)]
#[test]
fn destination_alias_appearing_during_session_is_caught() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let mut host = host(dir.path());
    let protected = dir.path().join("protected.json");
    std::fs::write(&protected, "untouched").unwrap();
    host.protected = Some(protected.clone());
    assert!(host.allowed(SaveTarget::Local).unwrap());
    symlink(&protected, &host.local).unwrap();
    assert!(!host.allowed(SaveTarget::Local).unwrap());
    assert!(host.save(&document(), SaveTarget::Local).is_err());
    assert_eq!(std::fs::read_to_string(protected).unwrap(), "untouched");
}

#[cfg(unix)]
#[test]
fn first_global_save_secures_config_directory() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let host = host(dir.path());
    host.save(&document(), SaveTarget::Global).unwrap();
    assert_eq!(
        std::fs::metadata(&host.config_dir)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn non_directory_save_parent_reports_error_without_recording_success() {
    let root = tempfile::tempdir().unwrap();
    let mut host = host(root.path());
    std::fs::write(host.config_dir.parent().unwrap(), "file").unwrap();
    host.protected = Some(root.path().join("protected.json"));
    let mut ui = crate::edit::ui::UiState::new(
        document(),
        "deck".into(),
        false,
        colored_text::ColorLevel::NoColor,
    );
    host.refresh(&mut ui);
    host.service(
        &mut ui,
        crate::edit::session::SaveRequest::Save(SaveTarget::Global),
    );
    assert!(ui.saves().is_empty());
    assert!(!host.config_dir.exists());
}

#[test]
fn first_global_save_with_unicode_missing_parents_and_distinct_protected_path_succeeds()
 {
    let dir = tempfile::tempdir().unwrap();
    let mut host = host(dir.path());
    host.config_dir = dir.path().join("配置/labeldeck");
    host.protected = Some(dir.path().join("配置/export/out.json"));
    assert!(host.allowed(SaveTarget::Global).unwrap());
    let record = host.save(&document(), SaveTarget::Global).unwrap();
    assert_eq!(
        crate::commands::read_canonical(&record.path).unwrap(),
        document().labels().unwrap()
    );
    assert!(!host.protected.unwrap().exists());
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn ambiguous_unicode_protected_global_destination_prevents_directory_creation()
{
    let dir = tempfile::tempdir().unwrap();
    let mut host = host(dir.path());
    host.config_dir = dir.path().join("Étage");
    host.protected = Some(dir.path().join("étage/labels.json"));
    assert!(host.save(&document(), SaveTarget::Global).is_err());
    assert!(!host.config_dir.exists());
    assert!(!host.protected.unwrap().exists());
}

#[cfg(unix)]
#[test]
fn protected_hard_link_retains_inode_and_contents_after_atomic_save() {
    use std::os::unix::fs::MetadataExt;
    let dir = tempfile::tempdir().unwrap();
    let mut host = host(dir.path());
    let protected = dir.path().join("protected.json");
    std::fs::write(&protected, "original").unwrap();
    let original = std::fs::metadata(&protected).unwrap();
    std::fs::hard_link(&protected, &host.local).unwrap();
    host.protected = Some(protected.clone());
    assert!(host.allowed(SaveTarget::Local).unwrap());
    host.save(&document(), SaveTarget::Local).unwrap();
    assert_eq!(std::fs::read_to_string(&protected).unwrap(), "original");
    assert_eq!(std::fs::metadata(&protected).unwrap().ino(), original.ino());
    assert_ne!(
        std::fs::metadata(&host.local).unwrap().ino(),
        original.ino()
    );
    assert_eq!(
        crate::commands::read_canonical(&host.local).unwrap(),
        document().labels().unwrap()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn uncertain_linux_case_alias_refuses_first_global_save() {
    let dir = tempfile::tempdir().unwrap();
    let mut host = host(dir.path());
    host.config_dir = dir.path().join("Config");
    host.protected = Some(dir.path().join("CONFIG/LABELS.JSON"));
    let error = host.save(&document(), SaveTarget::Global).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("could not compare Save destinations")
    );
    assert!(!host.config_dir.exists());
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
}

#[test]
fn invalid_protected_ancestor_blocks_save_before_writes_or_config_creation() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("file");
    std::fs::write(&file, "original").unwrap();
    let mut host = host(root.path());
    for suffix in ["out.json", "nested/out.json"] {
        host.protected = Some(file.join(suffix));
        for target in [SaveTarget::Local, SaveTarget::Global] {
            assert!(host.allowed(target).is_err());
            let error = host
                .save_with(&document(), target, |_, _, _| {
                    panic!("invalid protected ancestor must prevent writing")
                })
                .unwrap_err();
            assert!(matches!(error, Error::Io { .. }));
        }
    }
    assert_eq!(std::fs::read_to_string(file).unwrap(), "original");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}
