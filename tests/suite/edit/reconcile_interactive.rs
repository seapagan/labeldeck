use super::{label, ui::key};
use crate::common::{Expectation, labels_json, mock_github};
use crossterm::event::KeyCode;
use labeldeck::{
    commands::interactive::{ReconcileContext, reconcile_with},
    edit::{
        session::SessionResult,
        ui::{FinalSelection, SessionKind, UiAction},
    },
    github::{GitHubClient, RepoSpec},
};

fn context(root: &std::path::Path) -> ReconcileContext {
    ReconcileContext {
        session: SessionKind::Sync,
        prune: true,
        title: "o/r".into(),
        success_line: "Synchronized o/r".into(),
        config_dir: root.into(),
    }
}
fn page(values: &[(&str, &str, Option<&str>)]) -> Expectation {
    Expectation::get("/repos/o/r/labels?per_page=100")
        .labels_page(&labels_json(values), None)
}

#[test]
fn changed_target_content_aborts_every_selected_operation() {
    let initial = [("bug", "ededed", Some(""))];
    let changes = [
        vec![("Bug", "ededed", Some(""))],
        vec![("bug", "abcdef", Some(""))],
        vec![("bug", "ededed", Some("new"))],
        vec![("bug", "ededed", Some("")), ("extra", "ededed", Some(""))],
        vec![],
    ];
    for changed in changes {
        let mock = mock_github(vec![page(&initial), page(&changed)]);
        let root = tempfile::tempdir().unwrap();
        let client = GitHubClient::with_options(mock.base_url(), None, true);
        let error = reconcile_with(
            &client,
            &RepoSpec::parse("o/r").unwrap(),
            vec![label("new")],
            context(root.path()),
            |state, _| {
                assert_eq!(
                    mock.requests().len(),
                    1,
                    "refetch must follow TUI return"
                );
                SessionResult {
                    outcome: Ok(UiAction::Finish(FinalSelection::Plan(
                        state.selected_plan(),
                    ))),
                    saves: Vec::new(),
                }
            },
        )
        .unwrap_err();
        assert!(
            error.to_string().contains(
                "target changed while the interactive plan was open"
            )
        );
        assert!(error.to_string().contains("rerun"));
        assert_eq!(mock.requests().len(), 2);
        mock.assert_satisfied();
    }
}

#[test]
fn target_refetch_failure_reports_zero_mutations_and_rerun() {
    let mock = mock_github(vec![
        page(&[]),
        Expectation::get("/repos/o/r/labels?per_page=100")
            .status(403)
            .body("{}"),
    ]);
    let root = tempfile::tempdir().unwrap();
    let client = GitHubClient::with_options(mock.base_url(), None, true);
    let error = reconcile_with(
        &client,
        &RepoSpec::parse("o/r").unwrap(),
        vec![label("new")],
        context(root.path()),
        |state, _| SessionResult {
            outcome: Ok(UiAction::Finish(FinalSelection::Plan(
                state.selected_plan(),
            ))),
            saves: Vec::new(),
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("No mutations were attempted"));
    assert!(error.to_string().contains("rerun"));
    assert_eq!(mock.requests().len(), 2);
    mock.assert_satisfied();
}

#[test]
fn reordered_target_executes_selected_delete_without_related_create() {
    let mock = mock_github(vec![
        page(&[("old", "ededed", Some("")), ("retain", "ededed", Some(""))]),
        page(&[("retain", "ededed", Some("")), ("old", "ededed", Some(""))]),
        Expectation::delete("/repos/o/r/labels/old"),
    ]);
    let root = tempfile::tempdir().unwrap();
    let client = GitHubClient::with_options(mock.base_url(), None, true);
    let code = reconcile_with(
        &client,
        &RepoSpec::parse("o/r").unwrap(),
        vec![label("new"), label("retain")],
        context(root.path()),
        |mut state, _| {
            key(&mut state, KeyCode::Char('c'));
            assert!(state.selected_plan().creates.is_empty());
            SessionResult {
                outcome: Ok(UiAction::Finish(FinalSelection::Plan(
                    state.selected_plan(),
                ))),
                saves: Vec::new(),
            }
        },
    )
    .unwrap();
    assert_eq!(code, 0);
    mock.assert_satisfied();
}

#[test]
fn create_update_precede_delete_and_partial_failure_accounting_survives() {
    let mut bug = label("bug");
    bug.description = "new".into();
    let snapshot =
        page(&[("bug", "ededed", Some("")), ("old", "ededed", Some(""))]);
    let mock = mock_github(vec![
        snapshot.clone(),
        snapshot,
        Expectation::post("/repos/o/r/labels"),
        Expectation::patch("/repos/o/r/labels/bug")
            .status(422)
            .body("{}"),
    ]);
    let root = tempfile::tempdir().unwrap();
    let client = GitHubClient::with_options(mock.base_url(), None, true);
    let code = reconcile_with(
        &client,
        &RepoSpec::parse("o/r").unwrap(),
        vec![label("new"), bug],
        context(root.path()),
        |state, _| SessionResult {
            outcome: Ok(UiAction::Finish(FinalSelection::Plan(
                state.selected_plan(),
            ))),
            saves: Vec::new(),
        },
    )
    .unwrap();
    assert_eq!(code, 2);
    assert_eq!(mock.requests().len(), 4);
    mock.assert_satisfied();
}

#[test]
fn completed_save_remains_when_target_validation_fails() {
    for refetch_error in [false, true] {
        let last = if refetch_error {
            Expectation::get("/repos/o/r/labels?per_page=100")
                .status(403)
                .body("{}")
        } else {
            page(&[("changed", "ededed", Some(""))])
        };
        let mock = mock_github(vec![page(&[]), last]);
        let root = tempfile::tempdir().unwrap();
        let client = GitHubClient::with_options(mock.base_url(), None, true);
        let error = reconcile_with(
            &client,
            &RepoSpec::parse("o/r").unwrap(),
            vec![label("new")],
            context(root.path()),
            |state, host| {
                let record = host
                    .save(
                        state.document(),
                        labeldeck::edit::session::SaveTarget::Global,
                    )
                    .unwrap();
                SessionResult {
                    outcome: Ok(UiAction::Finish(FinalSelection::Plan(
                        state.selected_plan(),
                    ))),
                    saves: vec![record],
                }
            },
        );
        assert!(error.is_err());
        assert!(root.path().join("labels.json").exists());
        mock.assert_satisfied();
    }
}

#[test]
fn sync_explicit_save_can_overwrite_its_loaded_source() {
    let mock = mock_github(vec![page(&[])]);
    let root = tempfile::tempdir().unwrap();
    let deck = root.path().join("labels.json");
    std::fs::write(&deck, labeldeck::canonical::to_json(&mut [label("new")]))
        .unwrap();
    let desired = labeldeck::commands::read_canonical(&deck).unwrap();
    let client = GitHubClient::with_options(mock.base_url(), None, true);
    reconcile_with(
        &client,
        &RepoSpec::parse("o/r").unwrap(),
        desired,
        context(root.path()),
        |mut state, host| {
            key(&mut state, KeyCode::Char('w'));
            key(&mut state, KeyCode::Delete);
            let record = host
                .save(
                    state.document(),
                    labeldeck::edit::session::SaveTarget::Global,
                )
                .unwrap();
            assert!(state.document().can_undo());
            SessionResult {
                outcome: Ok(UiAction::Cancel),
                saves: vec![record],
            }
        },
    )
    .unwrap();
    assert!(
        labeldeck::commands::read_canonical(&deck)
            .unwrap()
            .is_empty()
    );
    mock.assert_satisfied();
}

#[test]
fn copy_only_reads_source_once_and_mutates_selected_target_operations() {
    let mock = mock_github(vec![
        Expectation::get("/repos/o/source/labels?per_page=100")
            .labels_page(&labels_json(&[("new", "ededed", Some(""))]), None),
        page(&[("old", "ededed", Some(""))]),
        page(&[("old", "ededed", Some(""))]),
        Expectation::post("/repos/o/r/labels"),
    ]);
    let root = tempfile::tempdir().unwrap();
    let client = GitHubClient::with_options(mock.base_url(), None, true);
    let code = labeldeck::commands::copy::interactive_with(
        &client,
        &RepoSpec::parse("o/source").unwrap(),
        &RepoSpec::parse("o/r").unwrap(),
        true,
        root.path(),
        |mut state, _| {
            assert_eq!(state.session(), SessionKind::Copy);
            key(&mut state, KeyCode::Char('d'));
            SessionResult {
                outcome: Ok(UiAction::Finish(FinalSelection::Plan(
                    state.selected_plan(),
                ))),
                saves: Vec::new(),
            }
        },
    )
    .unwrap();
    assert_eq!(code, 0);
    assert_eq!(mock.requests().len(), 4);
    mock.assert_satisfied();
}

#[test]
fn same_repository_copy_rejection_precedes_terminal_check() {
    let error = labeldeck::commands::copy::run_interactive(
        "o/r", "O/R", None, false, true,
    )
    .unwrap_err();
    assert!(error.to_string().contains("same repository"));
}
