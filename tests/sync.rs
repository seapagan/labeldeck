//! Integration tests for sync execution against the local mock GitHub.
//! These verify the safety-critical ordering guarantees.

mod common;

use std::time::Duration;

use common::{Expectation, labels_json, mock_github};
use labeldeck::github::{GitHubClient, RepoSpec};
use labeldeck::labels::{Label, LabelColor};
use labeldeck::plan;
use labeldeck::sync::{self, Phase};

fn repo() -> RepoSpec {
    RepoSpec::parse("octocat/hello-world").unwrap()
}

fn label(name: &str, color: &str, description: &str) -> Label {
    Label {
        name: name.to_string(),
        color: LabelColor::parse(color).unwrap(),
        description: description.to_string(),
    }
}

fn client(mock: &common::MockGitHub) -> GitHubClient {
    GitHubClient::with_base_url(mock.base_url(), None)
}

fn list_expectation() -> Expectation {
    Expectation::get("/repos/octocat/hello-world/labels?per_page=100")
}

#[test]
fn sync_applies_creates_updates_and_prune_deletes_in_order() {
    // Remote: bug (stale colour), stale (target-only).
    // Canonical: bug (new colour), feature (new).
    let mock = mock_github(vec![
        list_expectation().labels_page(
            &labels_json(&[
                ("bug", "d73a4a", None),
                ("stale", "cccccc", None),
            ]),
            None,
        ),
        Expectation::post("/repos/octocat/hello-world/labels").status(201),
        Expectation::patch("/repos/octocat/hello-world/labels/bug"),
        Expectation::delete("/repos/octocat/hello-world/labels/stale")
            .status(204),
    ]);
    let canonical = vec![
        label("bug", "ff0000", "broken things"),
        label("feature", "a2eeef", "new idea"),
    ];
    let remote = client(&mock).list_labels(&repo()).unwrap();
    let plan = plan::plan(&canonical, &remote, true);
    let outcome = sync::execute_with_pause(
        &client(&mock),
        &repo(),
        &plan,
        Duration::ZERO,
        (),
    );
    mock.assert_satisfied();
    assert!(outcome.is_success(), "{outcome:?}");

    let requests = mock.requests();
    let methods: Vec<&str> =
        requests.iter().skip(1).map(|r| r.method.as_str()).collect();
    assert_eq!(
        methods,
        ["POST", "PATCH", "DELETE"],
        "mutations must run creates, then updates, then prune deletes"
    );
}

#[test]
fn sync_without_prune_never_deletes() {
    let mock = mock_github(vec![
        list_expectation().labels_page(
            &labels_json(&[
                ("bug", "d73a4a", None),
                ("stale", "cccccc", None),
            ]),
            None,
        ),
        Expectation::patch("/repos/octocat/hello-world/labels/bug"),
    ]);
    let canonical = vec![label("bug", "ff0000", "")];
    let remote = client(&mock).list_labels(&repo()).unwrap();
    let plan = plan::plan(&canonical, &remote, false);
    let outcome = sync::execute_with_pause(
        &client(&mock),
        &repo(),
        &plan,
        Duration::ZERO,
        (),
    );
    mock.assert_satisfied();
    assert!(outcome.is_success());

    for request in mock.requests() {
        assert_ne!(
            request.method, "DELETE",
            "prune is off; no deletion may be attempted"
        );
    }
}

#[test]
fn identical_sets_issue_zero_mutations() {
    let mock = mock_github(vec![list_expectation().labels_page(
        &labels_json(&[("bug", "d73a4a", Some("broken"))]),
        None,
    )]);
    let canonical = vec![label("bug", "d73a4a", "broken")];
    let remote = client(&mock).list_labels(&repo()).unwrap();
    let plan = plan::plan(&canonical, &remote, true);
    sync::execute_with_pause(
        &client(&mock),
        &repo(),
        &plan,
        Duration::ZERO,
        (),
    );
    mock.assert_satisfied();
    assert_eq!(mock.requests().len(), 1, "only the initial read");
}

#[test]
fn create_failure_stops_before_deletes() {
    // Canonical adds one label and prunes one; the create fails with 401
    // so the destructive delete must never be attempted.
    let mock = mock_github(vec![
        list_expectation()
            .labels_page(&labels_json(&[("stale", "cccccc", None)]), None),
        Expectation::post("/repos/octocat/hello-world/labels")
            .status(401)
            .body(r#"{"message":"Bad credentials"}"#),
    ]);
    let canonical = vec![label("feature", "a2eeef", "")];
    let remote = client(&mock).list_labels(&repo()).unwrap();
    let plan = plan::plan(&canonical, &remote, true);
    let outcome = sync::execute_with_pause(
        &client(&mock),
        &repo(),
        &plan,
        Duration::ZERO,
        (),
    );
    mock.assert_satisfied();

    let failure = outcome.failure.expect("create must fail");
    assert_eq!(failure.phase, Phase::CreateUpdate);
    assert!(failure.operation.contains("create label"), "{failure:?}");
    assert!(
        outcome
            .skipped
            .iter()
            .any(|s| s.contains("delete label \"stale\"")),
        "delete must be reported as skipped: {:?}",
        outcome.skipped
    );
    for request in mock.requests() {
        assert_ne!(request.method, "DELETE", "no delete may be attempted");
    }
}

#[test]
fn update_failure_reports_partial_application() {
    // Two updates planned; the first succeeds, the second fails.
    let mock = mock_github(vec![
        list_expectation().labels_page(
            &labels_json(&[
                ("bug", "d73a4a", None),
                ("docs", "0075ca", Some("old")),
            ]),
            None,
        ),
        Expectation::patch("/repos/octocat/hello-world/labels/bug"),
        Expectation::patch("/repos/octocat/hello-world/labels/docs")
            .status(422)
            .body(r#"{"message":"Validation Failed"}"#),
    ]);
    let canonical = vec![
        label("bug", "ff0000", "new"),
        label("docs", "00ff00", "new"),
    ];
    let remote = client(&mock).list_labels(&repo()).unwrap();
    let plan = plan::plan(&canonical, &remote, false);
    let outcome = sync::execute_with_pause(
        &client(&mock),
        &repo(),
        &plan,
        Duration::ZERO,
        (),
    );
    mock.assert_satisfied();

    assert_eq!(outcome.applied.len(), 1, "first update applied");
    assert!(outcome.applied[0].contains("bug"));
    let failure = outcome.failure.expect("second update must fail");
    assert!(failure.operation.contains("docs"), "{failure:?}");
}

#[test]
fn delete_failure_stops_remaining_deletes_and_reports_them() {
    let mock = mock_github(vec![
        list_expectation().labels_page(
            &labels_json(&[
                ("ancient", "aaaaaa", None),
                ("stale", "cccccc", None),
                ("dead", "dddddd", None),
            ]),
            None,
        ),
        Expectation::delete("/repos/octocat/hello-world/labels/ancient")
            .status(204),
        Expectation::delete("/repos/octocat/hello-world/labels/dead")
            .status(429)
            .header("retry-after", "60")
            .body(r#"{"message":"secondary rate limit"}"#),
    ]);
    let remote = client(&mock).list_labels(&repo()).unwrap();
    let plan = plan::plan(&[], &remote, true);
    let outcome = sync::execute_with_pause(
        &client(&mock),
        &repo(),
        &plan,
        Duration::ZERO,
        (),
    );
    mock.assert_satisfied();

    assert_eq!(outcome.applied.len(), 1);
    let failure = outcome.failure.expect("delete must fail");
    assert_eq!(failure.phase, Phase::Delete);
    assert!(
        outcome.skipped.iter().any(|s| s.contains("\"stale\"")),
        "remaining delete must be reported skipped: {:?}",
        outcome.skipped
    );
}

#[test]
fn successful_sync_reports_every_applied_operation() {
    let mock = mock_github(vec![
        list_expectation()
            .labels_page(&labels_json(&[("stale", "cccccc", None)]), None),
        Expectation::post("/repos/octocat/hello-world/labels").status(201),
        Expectation::delete("/repos/octocat/hello-world/labels/stale")
            .status(204),
    ]);
    let canonical = vec![label("feature", "a2eeef", "")];
    let remote = client(&mock).list_labels(&repo()).unwrap();
    let plan = plan::plan(&canonical, &remote, true);
    let mut seen: Vec<String> = Vec::new();
    let outcome = sync::execute_with_pause(
        &client(&mock),
        &repo(),
        &plan,
        Duration::ZERO,
        Progress(&mut seen),
    );
    mock.assert_satisfied();
    assert!(outcome.is_success());
    assert_eq!(seen, outcome.applied, "reporter must see every operation");
}

struct Progress<'a>(&'a mut Vec<String>);

impl sync::Reporter for Progress<'_> {
    fn operation(&mut self, description: &str) {
        self.0.push(description.to_string());
    }
}
