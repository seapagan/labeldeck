//! Real terminal boundary tests; portable state/event contracts use TestBackend.
use crate::common::terminal::{terminal, terminal_in};
use crate::common::{Expectation, Isolation, labels_json, mock_github};

#[test]
fn export_terminal_session_edits_saves_then_exports_empty_selection() {
    let isolation = Isolation::new("interactive-pty-export");
    let mock = mock_github(vec![
        Expectation::get("/repos/o/r/labels?per_page=100")
            .labels_page(&labels_json(&[("bug", "ededed", Some(""))]), None),
    ]);
    let output = terminal(
        &isolation,
        &["export", "o/r", "-i", "--file", "out.json"],
        mock.base_url(),
        &[
            ("labeldeck select", "w"),
            ("Save", "\u{13}"),
            ("Save working deck", "\t\r"),
            ("Saved", "\u{1b}"),
            ("[w Edit]", "0f"),
            ("Confirm Export", "\t\r"),
        ],
    );
    let exported =
        std::fs::read_to_string(isolation.config_dir.join("out.json"));
    let requests: Vec<_> = mock
        .requests()
        .into_iter()
        .map(|request| (request.method, request.path))
        .collect();
    mock.assert_satisfied();
    assert_eq!(
        requests,
        [("GET".into(), "/repos/o/r/labels?per_page=100".into())]
    );
    assert_eq!(exported.unwrap().trim(), "[]");
    assert!(isolation.config_dir.join("labels.json").exists());
    assert!(
        labeldeck::commands::read_canonical(
            &isolation.config_dir.join("out.json")
        )
        .unwrap()
        .is_empty()
    );
    assert!(output.contains("Exported 0 labels"));
}

#[test]
fn sync_terminal_session_confirms_after_target_refetch() {
    let isolation = Isolation::new("interactive-pty-sync");
    std::fs::write(
        isolation.config_dir.join("labels.json"),
        "[{\"name\":\"new\",\"color\":\"ededed\",\"description\":\"\"}]",
    )
    .unwrap();
    let page = Expectation::get("/repos/o/r/labels?per_page=100")
        .labels_page("[]", None);
    let mock = mock_github(vec![
        page.clone(),
        page,
        Expectation::post("/repos/o/r/labels"),
    ]);
    let output = terminal(
        &isolation,
        &["sync", "o/r", "-i"],
        mock.base_url(),
        &[("labeldeck select", "f"), ("Confirm Apply", "\t\r")],
    );
    assert!(output.contains("Synchronized o/r"));
    mock.assert_satisfied();
}

#[test]
fn sync_global_terminal_session_applies_global_with_local_present() {
    let isolation = Isolation::new("interactive-pty-global-sync");
    let dir = tempfile::tempdir().unwrap();
    let global = isolation.config_dir.join("labels.json");
    let local = dir.path().join("labels.json");
    let global_deck =
        "[{\"name\":\"global\",\"color\":\"ededed\",\"description\":\"\"}]";
    let local_deck =
        "[{\"name\":\"local\",\"color\":\"ededed\",\"description\":\"\"}]";
    std::fs::write(&global, global_deck).unwrap();
    std::fs::write(&local, local_deck).unwrap();
    let page = Expectation::get("/repos/o/r/labels?per_page=100")
        .labels_page("[]", None);
    let mock = mock_github(vec![
        page.clone(),
        page,
        Expectation::post("/repos/o/r/labels"),
    ]);
    let output = terminal_in(
        &isolation,
        &["sync", "o/r", "--global", "--interactive"],
        mock.base_url(),
        &[("labeldeck select", "f"), ("Confirm Apply", "\t\r")],
        dir.path(),
    );
    assert!(output.contains("Synchronized o/r"));
    mock.assert_satisfied();
    let body: serde_json::Value =
        serde_json::from_str(&mock.requests()[2].body).unwrap();
    assert_eq!(body["name"], "global");
    assert_eq!(std::fs::read_to_string(&global).unwrap(), global_deck);
    assert_eq!(std::fs::read_to_string(local).unwrap(), local_deck);
    let notice = format!("Using global deck: {global:?}");
    // ConPTY can replay the main screen when the alternate screen closes.
    let before_terminal = output
        .split_once("\x1b[?1049h")
        .expect("interactive sync must enter the alternate screen")
        .0;
    assert_eq!(before_terminal.matches("Using global deck:").count(), 1);
    assert!(before_terminal.contains(&notice));
}

#[test]
fn copy_terminal_session_cancels_without_target_refetch() {
    let isolation = Isolation::new("interactive-pty-copy");
    let mock = mock_github(vec![
        Expectation::get("/repos/o/source/labels?per_page=100")
            .labels_page("[]", None),
        Expectation::get("/repos/o/r/labels?per_page=100")
            .labels_page("[]", None),
    ]);
    let output = terminal(
        &isolation,
        &["copy", "o/source", "o/r", "-i"],
        mock.base_url(),
        &[("No changes", "\u{3}")],
    );
    assert!(output.contains("Cancelled; final copy was not applied"));
    mock.assert_satisfied();
}

#[test]
fn standalone_edit_terminal_save_then_cancel_preserves_source() {
    let isolation = Isolation::new("interactive-pty-edit");
    let deck =
        "[{\"name\":\"bug\",\"color\":\"ededed\",\"description\":\"\"}]";
    let path = isolation.config_dir.join("deck.json");
    std::fs::write(&path, deck).unwrap();
    let output = terminal(
        &isolation,
        &["edit", "--file", "deck.json"],
        "http://127.0.0.1:1",
        &[
            ("labeldeck edit", "s"),
            ("Save working deck", "\t\r"),
            ("Saved", "\u{3}"),
        ],
    );
    assert!(output.contains("Completed Saves remain on disk"));
    assert_eq!(std::fs::read_to_string(path).unwrap(), deck);
}

#[test]
fn remote_edit_terminal_save_then_cancel_leaves_github_unchanged() {
    let isolation = Isolation::new("interactive-pty-remote-edit");
    let mock = mock_github(vec![
        Expectation::get("/repos/o/r/labels?per_page=100")
            .labels_page(&labels_json(&[("bug", "ededed", Some(""))]), None),
    ]);
    let output = terminal(
        &isolation,
        &["edit", "o/r"],
        mock.base_url(),
        &[
            ("labeldeck edit", "s"),
            ("Save working deck", "\t\r"),
            ("Saved", "\u{3}"),
        ],
    );
    assert!(output.contains("Completed Saves remain on disk"));
    let deck = labeldeck::commands::read_canonical(
        &isolation.config_dir.join("labels.json"),
    )
    .unwrap();
    assert_eq!(deck[0].name, "bug");
    mock.assert_satisfied();
}
