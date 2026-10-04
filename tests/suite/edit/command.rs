use super::label;
use crate::common::{
    Expectation, Isolation, labels_json, mock_github, run, stderr,
};
use clap::Parser;
use labeldeck::{
    cli::{Cli, Command},
    commands::edit::{
        SourceSelection, apply_file, apply_remote, load_file, select_source,
    },
    edit::model::Document,
    github::{GitHubClient, RepoSpec},
};
use std::path::Path;

#[test]
fn edit_cli_accepts_only_exclusive_sources_and_rejects_stdin() {
    for args in [
        vec!["labeldeck", "edit"],
        vec!["labeldeck", "edit", "--global"],
        vec!["labeldeck", "edit", "--file", "some.json"],
        vec!["labeldeck", "edit", "o/r"],
    ] {
        assert!(matches!(
            Cli::try_parse_from(args).unwrap().command,
            Command::Edit { .. }
        ));
    }
    for args in [
        vec!["labeldeck", "edit", "o/r", "--global"],
        vec!["labeldeck", "edit", "o/r", "--file", "x"],
        vec!["labeldeck", "edit", "--global", "--file", "x"],
        vec!["labeldeck", "edit", "--file", "-"],
    ] {
        assert!(Cli::try_parse_from(args).is_err());
    }
    assert!(
        Cli::try_parse_from(["labeldeck", "--no-proxy", "edit", "o/r"])
            .unwrap()
            .no_proxy
    );
    for args in [
        vec!["labeldeck", "sync", "o/r"],
        vec!["labeldeck", "copy", "o/r", "a/b"],
        vec!["labeldeck", "export", "o/r"],
    ] {
        assert!(Cli::try_parse_from(args).is_ok());
    }
}

#[test]
fn source_selection_never_falls_back_or_probes_other_decks() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("labels.json"), "[]").unwrap();
    assert!(
        matches!(select_source(None,None,false,root.path()).unwrap(),SourceSelection::Local(path) if path == Path::new("labels.json"))
    );
    assert!(
        matches!(select_source(None,None,true,root.path()).unwrap(),SourceSelection::Global(path) if path == root.path().join("labels.json"))
    );
    assert!(
        matches!(select_source(None,Some(Path::new("exact.json")),false,root.path()).unwrap(),SourceSelection::Explicit(path) if path == Path::new("exact.json"))
    );
    assert!(matches!(
        select_source(Some("o/r"), None, false, root.path()).unwrap(),
        SourceSelection::Remote(_)
    ));
    assert!(select_source(Some("bad"), None, false, root.path()).is_err());
    assert!(select_source(Some("o/r"), None, true, root.path()).is_err());
    assert!(
        select_source(None, Some(Path::new("-")), false, root.path()).is_err()
    );
}

#[test]
fn missing_and_invalid_exact_files_have_actionable_errors() {
    let root = tempfile::tempdir().unwrap();
    for (selection, expected) in [
        (
            SourceSelection::Local(root.path().join("local.json")),
            "labeldeck export OWNER/REPO",
        ),
        (
            SourceSelection::Global(root.path().join("global.json")),
            "--global",
        ),
        (
            SourceSelection::Explicit(root.path().join("exact.json")),
            "--file",
        ),
    ] {
        assert!(
            load_file(&selection)
                .unwrap_err()
                .to_string()
                .contains(expected)
        );
    }
    let path = root.path().join("invalid.json");
    std::fs::write(&path, "{}").unwrap();
    assert!(
        load_file(&SourceSelection::Explicit(path))
            .unwrap_err()
            .to_string()
            .contains("invalid")
    );
}

#[test]
fn no_op_preserves_noncanonical_original_bytes_and_changed_file_is_not_touched()
 {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("labels.json");
    let contents =
        br##"[ {"name":"A", "color":"#EDEDED", "description":null} ]"##;
    std::fs::write(&path, contents).unwrap();
    let original =
        load_file(&SourceSelection::Explicit(path.clone())).unwrap();
    let doc = Document::from_labels(original.baseline.clone());
    assert_eq!(
        apply_file(&path, &original.baseline, &original.contents, &doc)
            .unwrap(),
        0
    );
    assert_eq!(std::fs::read(&path).unwrap(), contents);
    std::fs::write(&path, b"[]").unwrap();
    assert_eq!(
        apply_file(&path, &original.baseline, &original.contents, &doc)
            .unwrap(),
        0
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"[]");
}

fn changed_document() -> Document {
    let mut doc = Document::from_labels(vec![label("A")]);
    let mut draft = doc.entries()[0].draft.clone();
    draft.name = "B".into();
    doc.commit(doc.entries()[0].id, draft).unwrap();
    doc
}

#[test]
fn apply_writes_canonical_json_to_only_the_selected_file() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("chosen.json");
    std::fs::write(&path, "[]").unwrap();
    std::fs::write(root.path().join("labels.json"), "untouched").unwrap();
    assert_eq!(
        apply_file(&path, &[label("A")], b"[]", &changed_document()).unwrap(),
        0
    );
    let actual = std::fs::read_to_string(&path).unwrap();
    assert_eq!(actual, labeldeck::canonical::to_json(&mut [label("B")]));
    assert_eq!(
        std::fs::read_to_string(root.path().join("labels.json")).unwrap(),
        "untouched"
    );
}

#[test]
fn external_byte_change_or_removal_refuses_write_without_creation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("deck.json");
    std::fs::write(&path, "[ ]").unwrap();
    assert!(
        apply_file(&path, &[label("A")], b"[]", &changed_document())
            .unwrap_err()
            .to_string()
            .contains("changed while")
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[ ]");
    std::fs::remove_file(&path).unwrap();
    assert!(
        apply_file(&path, &[label("A")], b"[]", &changed_document()).is_err()
    );
    assert!(!path.exists());
}

#[test]
fn remote_noop_makes_no_requests() {
    let mock = mock_github(vec![]);
    let client = GitHubClient::with_options(mock.base_url(), None, true);
    assert_eq!(
        apply_remote(
            &client,
            &RepoSpec::parse("o/r").unwrap(),
            &[label("A")],
            &Document::from_labels(vec![label("A")])
        )
        .unwrap(),
        0
    );
    assert!(mock.requests().is_empty());
}

#[test]
fn remote_changed_baseline_performs_zero_mutations() {
    for values in [
        vec![("a", "ededed", Some(""))],
        vec![("A", "ffffff", Some(""))],
        vec![("A", "ededed", Some("changed"))],
        vec![],
    ] {
        let mock = mock_github(vec![
            Expectation::get("/repos/o/r/labels?per_page=100")
                .labels_page(&labels_json(&values), None),
        ]);
        let client = GitHubClient::with_options(mock.base_url(), None, true);
        let error = apply_remote(
            &client,
            &RepoSpec::parse("o/r").unwrap(),
            &[label("A")],
            &changed_document(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("changed while"));
        assert_eq!(mock.requests().len(), 1);
        assert_eq!(mock.requests()[0].method, "GET");
        mock.assert_satisfied();
    }
}

#[test]
fn remote_order_difference_allows_rename_and_reports_failures_with_exit_two() {
    for status in [200, 422] {
        let mock = mock_github(vec![
            Expectation::get("/repos/o/r/labels?per_page=100").labels_page(
                &labels_json(&[
                    ("D", "ededed", Some("")),
                    ("A", "ededed", Some("")),
                ]),
                None,
            ),
            Expectation::patch("/repos/o/r/labels/A")
                .status(status)
                .body(r#"{"message":"rejected"}"#),
        ]);
        let client = GitHubClient::with_options(mock.base_url(), None, true);
        let mut doc = Document::from_labels(vec![label("A"), label("D")]);
        let mut draft = doc.entries()[0].draft.clone();
        draft.name = "B".into();
        doc.commit(0, draft).unwrap();
        assert_eq!(
            apply_remote(
                &client,
                &RepoSpec::parse("o/r").unwrap(),
                &[label("A"), label("D")],
                &doc
            )
            .unwrap(),
            if status == 200 { 0 } else { 2 }
        );
        assert_eq!(mock.requests().len(), 2);
        mock.assert_satisfied();
    }
}

#[test]
fn nonterminal_edit_fails_without_prompting_or_contacting_github() {
    let isolation = Isolation::new("edit-nonterminal");
    let output = run(&mut isolation.command(&["edit", "o/r"]));
    assert!(!output.status.success());
    assert!(stderr(&output).contains("interactive terminal"));
}
