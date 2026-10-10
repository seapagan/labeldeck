use super::*;

#[test]
fn invalid_protected_parent_refresh_reports_error_and_disables_saves() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("file");
    std::fs::write(&file, "original").unwrap();
    let mut ui = interactive_state(SessionKind::Export);
    let host = crate::edit::session::SaveHost {
        local: root.path().join("labels.json"),
        config_dir: root.path().join("missing/config"),
        protected: Some(file.join("nested/out.json")),
    };
    host.refresh(&mut ui);
    assert!(
        matches!(&ui.message, Some(super::super::super::Message::Error(text))
        if text.contains("Could not check Save destination"))
    );
    assert_eq!(ui.save_choices, [false, false]);
    host.service(
        &mut ui,
        crate::edit::session::SaveRequest::Save(
            crate::edit::session::SaveTarget::Global,
        ),
    );
    assert!(ui.saves().is_empty());
    assert!(!host.config_dir.exists());
    assert_eq!(std::fs::read_to_string(file).unwrap(), "original");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}
