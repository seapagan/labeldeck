//! Regression tests for partial-failure accounting: applied / failed /
//! skipped must partition the planned mutations exactly.

use std::time::Duration;

use crate::common::{Expectation, MockGitHub, labels_json, mock_github};
use labeldeck::github::{GitHubClient, RepoSpec};
use labeldeck::labels::{Label, LabelColor};
use labeldeck::plan::{self, Plan};
use labeldeck::sync::{self, Phase, SyncOutcome};

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

fn client(mock: &MockGitHub) -> GitHubClient {
    GitHubClient::with_base_url(mock.base_url(), None)
}

fn list_expectation() -> Expectation {
    Expectation::get("/repos/octocat/hello-world/labels?per_page=100")
}

fn post_ok() -> Expectation {
    Expectation::post("/repos/octocat/hello-world/labels").status(201)
}

fn post_fail() -> Expectation {
    Expectation::post("/repos/octocat/hello-world/labels")
        .status(422)
        .body(r#"{"message":"Validation Failed"}"#)
}

fn patch(name: &str) -> Expectation {
    Expectation::patch(&format!("/repos/octocat/hello-world/labels/{name}"))
}

fn patch_fail(name: &str) -> Expectation {
    patch(name)
        .status(422)
        .body(r#"{"message":"Validation Failed"}"#)
}

fn delete_ok(name: &str) -> Expectation {
    Expectation::delete(&format!("/repos/octocat/hello-world/labels/{name}"))
        .status(204)
}

fn delete_fail(name: &str) -> Expectation {
    Expectation::delete(&format!("/repos/octocat/hello-world/labels/{name}"))
        .status(429)
        .header("retry-after", "60")
        .body(r#"{"message":"secondary rate limit"}"#)
}

/// Assert the exact applied/failed/skipped partition and that no
/// operation appears in more than one category.
fn assert_partition(
    outcome: &SyncOutcome,
    applied: &[&str],
    failed: &str,
    skipped: &[&str],
    phase: Phase,
) {
    let applied_strings: Vec<String> =
        applied.iter().map(|s| s.to_string()).collect();
    let skipped_strings: Vec<String> =
        skipped.iter().map(|s| s.to_string()).collect();
    assert_eq!(outcome.applied, applied_strings, "applied mismatch");
    assert_eq!(outcome.skipped, skipped_strings, "skipped mismatch");
    let failure = outcome
        .failure
        .as_ref()
        .unwrap_or_else(|| panic!("expected a failure, got {outcome:?}"));
    assert_eq!(failure.operation, failed, "failed operation mismatch");
    assert_eq!(failure.phase, phase, "failed phase mismatch");

    let mut everything: Vec<&String> = Vec::new();
    everything.extend(outcome.applied.iter());
    everything.push(&failure.operation);
    everything.extend(outcome.skipped.iter());
    for (index, item) in everything.iter().enumerate() {
        let duplicated = everything
            .iter()
            .enumerate()
            .any(|(other, candidate)| other != index && *candidate == *item);
        assert!(
            !duplicated,
            "operation {item:?} appears in more than one category"
        );
    }
}

fn run(mock: &MockGitHub, canonical: &[Label], prune: bool) -> SyncOutcome {
    let remote = client(mock).list_labels(&repo()).unwrap();
    let plan: Plan = plan::plan(canonical, &remote, prune);
    sync::execute_with_pause(&client(mock), &repo(), &plan, Duration::ZERO, ())
}

fn no_deletes(mock: &MockGitHub) {
    for request in mock.requests() {
        assert_ne!(
            request.method, "DELETE",
            "no destructive delete may follow a failed create/update"
        );
    }
}

/// Remote with one stale label (to update) and two target-only labels
/// (to prune), so plans exercise all three phases.
fn mixed_remote() -> Vec<Expectation> {
    vec![list_expectation().labels_page(
        &labels_json(&[
            ("bug", "d73a4a", Some("old")),
            ("keep-out-of-canonical", "111111", None),
            ("also-extra", "222222", None),
        ]),
        None,
    )]
}

fn mixed_canonical() -> Vec<Label> {
    vec![
        label("bug", "ff0000", "new"),       // update
        label("feature", "a2eeef", "brand"), // create
    ]
}

#[test]
fn failure_at_first_create_skips_everything_else() {
    let mut expectations = mixed_remote();
    expectations.push(post_fail());
    let mock = mock_github(expectations);
    let outcome = run(&mock, &mixed_canonical(), true);
    mock.assert_satisfied();
    assert_partition(
        &outcome,
        &[],
        "create label \"feature\"",
        &[
            "update label \"bug\" in place (colour/description)",
            "delete label \"also-extra\"",
            "delete label \"keep-out-of-canonical\"",
        ],
        Phase::CreateUpdate,
    );
    no_deletes(&mock);
}

#[test]
fn failure_at_last_create_after_successes_skips_only_later_phases() {
    let mut expectations = mixed_remote();
    // Two creates planned (feature, zebra); the second fails.
    let canonical = vec![
        label("bug", "ff0000", "new"),       // update
        label("feature", "a2eeef", "brand"), // create (ok)
        label("zebra", "000000", "z"),       // create (fails)
    ];
    expectations.push(post_ok());
    expectations.push(post_fail());
    let mock = mock_github(expectations);
    let outcome = run(&mock, &canonical, true);
    mock.assert_satisfied();
    assert_partition(
        &outcome,
        &["create label \"feature\""],
        "create label \"zebra\"",
        &[
            "update label \"bug\" in place (colour/description)",
            "delete label \"also-extra\"",
            "delete label \"keep-out-of-canonical\"",
        ],
        Phase::CreateUpdate,
    );
    no_deletes(&mock);
}

#[test]
fn failure_at_first_update_skips_remaining_updates_and_deletes() {
    // Two updates planned; the first fails, the second must be skipped,
    // and the prune deletes must never start.
    let list = list_expectation().labels_page(
        &labels_json(&[
            ("bug", "d73a4a", Some("old")),
            ("docs", "0075ca", Some("older")),
            ("keep-out-of-canonical", "111111", None),
            ("also-extra", "222222", None),
        ]),
        None,
    );
    let mock = mock_github(vec![list, patch_fail("bug")]);
    let canonical = vec![
        label("bug", "ff0000", "new"), // update (fails)
        label("docs", "0075ca", "changed"), // update (skipped)
    ];
    let outcome = run(&mock, &canonical, true);
    mock.assert_satisfied();
    assert_partition(
        &outcome,
        &[],
        "update label \"bug\" in place (colour/description)",
        &[
            "update label \"docs\" in place (colour/description)",
            "delete label \"also-extra\"",
            "delete label \"keep-out-of-canonical\"",
        ],
        Phase::CreateUpdate,
    );
    no_deletes(&mock);
}

#[test]
fn failure_mid_updates_keeps_applied_updates_out_of_skipped() {
    // Three updates planned; the first succeeds, the second fails, the
    // third must be skipped — and the applied one must NOT reappear.
    let list = list_expectation().labels_page(
        &labels_json(&[
            ("alpha", "111111", Some("old")),
            ("beta", "222222", Some("old")),
            ("gamma", "333333", Some("old")),
        ]),
        None,
    );
    let mock = mock_github(vec![list, patch("alpha"), patch_fail("beta")]);
    let canonical = vec![
        label("alpha", "aaaaaa", "new"),
        label("beta", "bbbbbb", "new"),
        label("gamma", "cccccc", "new"),
    ];
    let outcome = run(&mock, &canonical, false);
    mock.assert_satisfied();
    assert_partition(
        &outcome,
        &["update label \"alpha\" in place (colour/description)"],
        "update label \"beta\" in place (colour/description)",
        &["update label \"gamma\" in place (colour/description)"],
        Phase::CreateUpdate,
    );
}

#[test]
fn failure_at_final_update_reports_no_unnecessary_skips() {
    let list = list_expectation().labels_page(
        &labels_json(&[
            ("alpha", "111111", Some("old")),
            ("beta", "222222", Some("old")),
        ]),
        None,
    );
    let mock = mock_github(vec![list, patch("alpha"), patch_fail("beta")]);
    let canonical = vec![
        label("alpha", "aaaaaa", "new"),
        label("beta", "bbbbbb", "new"),
    ];
    let outcome = run(&mock, &canonical, true);
    mock.assert_satisfied();
    // Prune deletes were planned but never started.
    // (No target-only labels here, so nothing else is skipped.)
    assert_partition(
        &outcome,
        &["update label \"alpha\" in place (colour/description)"],
        "update label \"beta\" in place (colour/description)",
        &[],
        Phase::CreateUpdate,
    );
}

#[test]
fn failure_at_first_delete_skips_remaining_deletes() {
    // Deletes run alphabetically: alpha (ok), beta (fails), gamma
    // (skipped) — applied deletes must not be re-listed as skipped.
    let list = list_expectation().labels_page(
        &labels_json(&[
            ("alpha", "111111", None),
            ("beta", "222222", None),
            ("gamma", "333333", None),
        ]),
        None,
    );
    let mock =
        mock_github(vec![list, delete_ok("alpha"), delete_fail("beta")]);
    let outcome = run(&mock, &[], true);
    mock.assert_satisfied();
    assert_partition(
        &outcome,
        &["delete label \"alpha\""],
        "delete label \"beta\"",
        &["delete label \"gamma\""],
        Phase::Delete,
    );
}

#[test]
fn failure_at_final_delete_reports_empty_skipped() {
    let list = list_expectation()
        .labels_page(&labels_json(&[("alpha", "111111", None)]), None);
    let mock = mock_github(vec![list, delete_fail("alpha")]);
    let outcome = run(&mock, &[], true);
    mock.assert_satisfied();
    assert_partition(
        &outcome,
        &[],
        "delete label \"alpha\"",
        &[],
        Phase::Delete,
    );
}

#[test]
fn successful_run_partitions_everything_into_applied() {
    let mut expectations = mixed_remote();
    expectations.push(post_ok());
    expectations.push(patch("bug"));
    expectations.push(delete_ok("also-extra"));
    expectations.push(delete_ok("keep-out-of-canonical"));
    let mock = mock_github(expectations);
    let outcome = run(&mock, &mixed_canonical(), true);
    mock.assert_satisfied();
    assert!(outcome.is_success());
    assert!(outcome.skipped.is_empty());
    assert_eq!(outcome.applied.len(), 4);
    assert_eq!(
        outcome.applied,
        [
            "create label \"feature\"",
            "update label \"bug\" in place (colour/description)",
            "delete label \"also-extra\"",
            "delete label \"keep-out-of-canonical\"",
        ]
    );
}
