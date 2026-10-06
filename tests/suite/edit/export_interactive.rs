use super::ui::key;
use crate::common::{Expectation, labels_json, mock_github};
use crossterm::event::KeyCode;
use labeldeck::{
    commands::export::interactive_with,
    edit::{
        session::SessionResult,
        ui::{FinalSelection, UiAction},
    },
    github::{GitHubClient, RepoSpec},
};

fn source() -> crate::common::MockGitHub {
    mock_github(vec![
        Expectation::get("/repos/o/r/labels?per_page=100").labels_page(
            &labels_json(&[
                ("bug", "ededed", Some("")),
                ("docs", "ededed", Some("")),
            ]),
            None,
        ),
    ])
}

#[test]
fn interactive_export_writes_selected_subset_only_after_driver_returns() {
    for empty in [false, true] {
        let mock = source();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("deck.json");
        let client = GitHubClient::with_options(mock.base_url(), None, true);
        interactive_with(
            &client,
            &RepoSpec::parse("o/r").unwrap(),
            &path,
            false,
            false,
            root.path(),
            |mut state, _| {
                key(&mut state, KeyCode::Char(if empty { '0' } else { ' ' }));
                assert!(!path.exists());
                SessionResult {
                    outcome: Ok(UiAction::Finish(FinalSelection::Export(
                        state.selected_labels().unwrap(),
                    ))),
                    saves: Vec::new(),
                }
            },
        )
        .unwrap();
        let labels = labeldeck::canonical::parse(
            &std::fs::read_to_string(path).unwrap(),
        )
        .unwrap();
        assert_eq!(labels.len(), usize::from(!empty));
        if !empty {
            assert_eq!(labels[0].name, "docs");
        }
        assert_eq!(mock.requests().len(), 1);
        mock.assert_satisfied();
    }
}

#[test]
fn late_export_destination_refuses_overwrite_without_force() {
    for force in [false, true] {
        let mock = source();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("labels.json");
        let client = GitHubClient::with_options(mock.base_url(), None, true);
        let result = interactive_with(
            &client,
            &RepoSpec::parse("o/r").unwrap(),
            &path,
            force,
            false,
            root.path(),
            |state, _| {
                std::fs::write(&path, "appeared while open").unwrap();
                SessionResult {
                    outcome: Ok(UiAction::Finish(FinalSelection::Export(
                        state.selected_labels().unwrap(),
                    ))),
                    saves: Vec::new(),
                }
            },
        );
        assert_eq!(result.is_ok(), force);
        if !force {
            assert_eq!(
                std::fs::read_to_string(path).unwrap(),
                "appeared while open"
            );
        }
        mock.assert_satisfied();
    }
}

#[test]
fn global_export_rechecks_configuration_directory_after_terminal_exit() {
    let mock = source();
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let destination = config.join("labels.json");
    let client = GitHubClient::with_options(mock.base_url(), None, true);
    let result = interactive_with(
        &client,
        &RepoSpec::parse("o/r").unwrap(),
        &destination,
        false,
        true,
        &config,
        |state, _| {
            std::fs::write(&config, "appeared while open").unwrap();
            SessionResult {
                outcome: Ok(UiAction::Finish(FinalSelection::Export(
                    state.selected_labels().unwrap(),
                ))),
                saves: Vec::new(),
            }
        },
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("could not create or secure configuration directory")
    );
    assert_eq!(
        std::fs::read_to_string(config).unwrap(),
        "appeared while open"
    );
    assert!(!destination.exists());
    mock.assert_satisfied();
}

#[test]
fn export_cancel_after_save_keeps_full_deck_and_skips_final_destination() {
    let mock = source();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("out.json");
    let client = GitHubClient::with_options(mock.base_url(), None, true);
    interactive_with(
        &client,
        &RepoSpec::parse("o/r").unwrap(),
        &path,
        false,
        false,
        root.path(),
        |mut state, host| {
            key(&mut state, KeyCode::Char('0'));
            let record = host
                .save(
                    state.document(),
                    labeldeck::edit::session::SaveTarget::Global,
                )
                .unwrap();
            SessionResult {
                outcome: Ok(UiAction::Cancel),
                saves: vec![record],
            }
        },
    )
    .unwrap();
    assert!(!path.exists());
    assert_eq!(
        labeldeck::canonical::parse(
            &std::fs::read_to_string(root.path().join("labels.json")).unwrap()
        )
        .unwrap()
        .len(),
        2
    );
    mock.assert_satisfied();
}

#[test]
fn saved_deck_survives_final_export_failure() {
    let mock = source();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("absent/out.json");
    let client = GitHubClient::with_options(mock.base_url(), None, true);
    let result = interactive_with(
        &client,
        &RepoSpec::parse("o/r").unwrap(),
        &path,
        false,
        false,
        root.path(),
        |state, host| {
            let record = host
                .save(
                    state.document(),
                    labeldeck::edit::session::SaveTarget::Global,
                )
                .unwrap();
            SessionResult {
                outcome: Ok(UiAction::Finish(FinalSelection::Export(
                    state.selected_labels().unwrap(),
                ))),
                saves: vec![record],
            }
        },
    );
    assert!(result.is_err());
    assert!(root.path().join("labels.json").exists());
    mock.assert_satisfied();
}
