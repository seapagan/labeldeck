use super::label;
use crate::common::{Expectation, mock_github};
use labeldeck::{
    edit::{
        execute::execute_with_pause,
        plan::{EditPlan, Operation},
    },
    github::{GitHubClient, RepoSpec},
};
use std::time::Duration;

fn client(mock: &crate::common::MockGitHub) -> GitHubClient {
    GitHubClient::with_options(mock.base_url(), None, true)
}

#[test]
fn edit_patch_sends_new_name_and_existing_update_does_not() {
    for name in ["a b", "a/b", "café", "50%"] {
        let encoded = match name {
            "a b" => "a%20b",
            "a/b" => "a%2Fb",
            "café" => "caf%C3%A9",
            _ => "50%25",
        };
        let path = format!("/repos/o/r/labels/{encoded}");
        let mock = mock_github(vec![
            Expectation::patch(&path),
            Expectation::patch(&path),
        ]);
        let repo = RepoSpec::parse("o/r").unwrap();
        client(&mock)
            .edit_label(&repo, name, &label("new"))
            .unwrap();
        client(&mock)
            .update_label(&repo, name, &label("new"))
            .unwrap();
        let requests = mock.requests();
        let renamed: serde_json::Value =
            serde_json::from_str(&requests[0].body).unwrap();
        let old: serde_json::Value =
            serde_json::from_str(&requests[1].body).unwrap();
        assert_eq!(
            renamed,
            serde_json::json!({"new_name":"new","color":"ededed","description":""})
        );
        assert_eq!(
            old,
            serde_json::json!({"color":"ededed","description":""})
        );
        mock.assert_satisfied();
    }
}

#[test]
fn edit_patch_retains_status_errors_and_conflict_details() {
    for status in [401, 403, 404, 422] {
        let mock = mock_github(vec![Expectation::patch("/repos/o/r/labels/A").status(status).body(r#"{"message":"conflict","errors":[{"code":"already_exists"}]}"#)]);
        let error = client(&mock)
            .edit_label(&RepoSpec::parse("o/r").unwrap(), "A", &label("B"))
            .unwrap_err();
        match (status, error) {
            (401, labeldeck::github::GithubError::Unauthorized { .. })
            | (403, labeldeck::github::GithubError::Forbidden { .. })
            | (404, labeldeck::github::GithubError::NotFound { .. }) => {}
            (422, labeldeck::github::GithubError::Validation { detail }) => {
                assert!(detail.contains("already_exists"))
            }
            (_, other) => panic!("wrong error {other}"),
        }
        mock.assert_satisfied();
    }
}

fn operations() -> Vec<Operation> {
    vec![
        Operation::TemporaryRename {
            current_name: "B".into(),
            desired: label("labeldeck-edit-tmp-1"),
        },
        Operation::Rename {
            current_name: "A".into(),
            desired: label("B"),
        },
        Operation::Create(label("C")),
        Operation::Update {
            current_name: "D".into(),
            desired: label("D"),
        },
        Operation::Delete {
            name: "labeldeck-edit-tmp-1".into(),
        },
    ]
}

fn expectations() -> Vec<Expectation> {
    vec![
        Expectation::patch("/repos/o/r/labels/B"),
        Expectation::patch("/repos/o/r/labels/A"),
        Expectation::post("/repos/o/r/labels").status(201),
        Expectation::patch("/repos/o/r/labels/D"),
        Expectation::delete("/repos/o/r/labels/labeldeck-edit-tmp-1")
            .status(204),
    ]
}

#[test]
fn failures_partition_applied_failed_skipped_and_stop_all_later_requests() {
    for failed in 0..5 {
        let mut responses = expectations();
        responses[failed] = responses[failed]
            .clone()
            .status(422)
            .body(r#"{"message":"rejected"}"#);
        let mock = mock_github(responses);
        let plan = EditPlan {
            operations: operations(),
            summary: Default::default(),
        };
        let result = execute_with_pause(
            &client(&mock),
            &RepoSpec::parse("o/r").unwrap(),
            &plan,
            Duration::ZERO,
            (),
        );
        assert_eq!(
            result.applied,
            plan.operations[..failed]
                .iter()
                .map(Operation::description)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            result.failure.as_ref().unwrap().operation,
            plan.operations[failed].description()
        );
        assert_eq!(
            result.skipped,
            plan.operations[failed + 1..]
                .iter()
                .map(Operation::description)
                .collect::<Vec<_>>()
        );
        assert_eq!(mock.requests().len(), failed + 1);
        if failed > 0 {
            assert!(result.applied[0].contains("labeldeck-edit-tmp-1"));
        }
        mock.assert_satisfied();
    }
}

#[test]
fn successful_execution_reports_every_operation_in_sequence() {
    let mock = mock_github(expectations());
    let plan = EditPlan {
        operations: operations(),
        summary: Default::default(),
    };
    let result = execute_with_pause(
        &client(&mock),
        &RepoSpec::parse("o/r").unwrap(),
        &plan,
        Duration::ZERO,
        (),
    );
    assert_eq!(result.applied.len(), 5);
    assert!(result.failure.is_none());
    assert!(result.skipped.is_empty());
    assert_eq!(mock.requests().len(), 5);
    mock.assert_satisfied();
}
