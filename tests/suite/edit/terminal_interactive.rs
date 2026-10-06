//! Real terminal boundary tests; portable state/event contracts use TestBackend.
#![cfg(unix)]
use crate::common::{Expectation, Isolation, labels_json, mock_github};
use std::process::Command;

fn terminal(
    isolation: &Isolation,
    args: &[&str],
    api: &str,
    steps: &[(&str, &str)],
) -> String {
    let template = isolation.command(args);
    let mut command = Command::new("python3");
    command
        .args([concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/terminal/drive.py"
        )])
        .arg(template.get_program())
        .arg(serde_json::to_string(steps).unwrap())
        .arg("labeldeck")
        .args(args);
    for (name, value) in template.get_envs() {
        match value {
            Some(value) => {
                command.env(name, value);
            }
            None => {
                command.env_remove(name);
            }
        }
    }
    command
        .env("LABELDECK_API", api)
        .env("LABELDECK_TOKEN", "test-token")
        .env("NO_COLOR", "1")
        .env("TERM", "xterm")
        .current_dir(&isolation.config_dir);
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8(output.stdout).unwrap()
}

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
            ("labels selected", "0f"),
            ("Confirm Export", "\t\r"),
        ],
    );
    assert!(output.contains("Exported 0 labels"));
    assert!(isolation.config_dir.join("labels.json").exists());
    assert!(
        labeldeck::commands::read_canonical(
            &isolation.config_dir.join("out.json")
        )
        .unwrap()
        .is_empty()
    );
    mock.assert_satisfied();
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
