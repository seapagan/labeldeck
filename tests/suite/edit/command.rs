use super::label;

#[test]
fn file_session_cancel_and_apply_use_loaded_snapshot_without_terminal() {
    use labeldeck::commands::edit::edit_file_with;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("deck.json");
    let bytes = labeldeck::canonical::to_json(&mut [label("A")]);
    std::fs::write(&path, &bytes).unwrap();
    let selection = SourceSelection::Explicit(path.clone());
    assert_eq!(
        edit_file_with(&selection, |doc, title, live| {
            assert!(!live);
            assert!(title.contains("deck.json"));
            assert_eq!(doc.labels().unwrap(), vec![label("A")]);
            Ok(None)
        })
        .unwrap(),
        0
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
    assert_eq!(
        edit_file_with(&selection, |_, _, _| Ok(Some(changed_document())))
            .unwrap(),
        0
    );
    assert_eq!(
        labeldeck::canonical::parse(&std::fs::read_to_string(path).unwrap())
            .unwrap(),
        vec![label("B")]
    );
}

#[test]
fn live_session_cancel_only_fetches_and_apply_refetches_before_patch() {
    use labeldeck::commands::edit::edit_remote_with;
    for apply in [false, true] {
        let mut expectations = vec![
            Expectation::get("/repos/o/r/labels?per_page=100")
                .labels_page(&labels_json(&[("A", "ededed", Some(""))]), None),
        ];
        if apply {
            expectations.push(expectations[0].clone());
            expectations.push(Expectation::patch("/repos/o/r/labels/A"));
        }
        let mock = mock_github(expectations);
        let client = GitHubClient::with_options(mock.base_url(), None, true);
        assert_eq!(
            edit_remote_with(
                &client,
                &RepoSpec::parse("o/r").unwrap(),
                |doc, title, live| {
                    assert!(live);
                    assert!(title.contains("o/r"));
                    assert_eq!(doc.labels().unwrap(), vec![label("A")]);
                    Ok(apply.then(changed_document))
                }
            )
            .unwrap(),
            0
        );
        assert_eq!(mock.requests().len(), if apply { 3 } else { 1 });
        mock.assert_satisfied();
    }
}

#[test]
fn exact_file_read_errors_and_invalid_utf8_never_fallback() {
    let root = tempfile::tempdir().unwrap();
    assert!(
        load_file(&SourceSelection::Remote(RepoSpec::parse("o/r").unwrap()))
            .is_err()
    );
    assert!(
        load_file(&SourceSelection::Explicit(root.path().into())).is_err()
    );
    let path = root.path().join("bad.json");
    std::fs::write(&path, [255u8]).unwrap();
    assert!(load_file(&SourceSelection::Explicit(path)).is_err());
}

#[cfg(unix)]
#[test]
fn explicit_non_utf8_paths_remain_supported_by_cli() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let path = OsString::from_vec(vec![b'x', 255]);
    let cli = Cli::try_parse_from([
        OsString::from("labeldeck"),
        OsString::from("edit"),
        OsString::from("--file"),
        path.clone(),
    ])
    .unwrap();
    assert!(
        matches!(cli.command,Command::Edit {file:Some(file),..} if file.as_os_str() == path)
    );
}

#[cfg(unix)]
#[test]
fn file_session_preserves_non_utf8_source_identity_for_save_protection() {
    use labeldeck::{
        commands::edit::edit_file_session_with,
        edit::{
            session::{SaveHost, SaveTarget, SessionResult},
            ui::UiAction,
        },
    };
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let root = tempfile::tempdir().unwrap();
    let directory =
        root.path().join(OsString::from_vec(b"deck-\xff".to_vec()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("labels.json");
    std::fs::write(&path, "[]").unwrap();
    let selection = SourceSelection::Explicit(path.clone());
    edit_file_session_with(&selection, |_, title, _, source| {
        assert_eq!(source, path);
        assert_ne!(Path::new(title), source);
        let host = SaveHost {
            local: path.clone(),
            config_dir: root.path().join("config"),
            protected: Some(source.into()),
        };
        assert!(host.save(&changed_document(), SaveTarget::Local).is_err());
        SessionResult {
            outcome: Ok(UiAction::Cancel),
            saves: Vec::new(),
        }
    })
    .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"[]");
}

#[cfg(unix)]
#[test]
fn local_apply_keeps_staged_write_symlink_policy() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target.json");
    let path = root.path().join("link.json");
    std::fs::write(&target, "[]").unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    let result = apply_file(&path, &[label("A")], b"[]", &changed_document());
    assert_eq!(result.unwrap(), 0);
    assert!(
        std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read_to_string(target).unwrap(),
        labeldeck::canonical::to_json(&mut [label("B")])
    );
}
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
