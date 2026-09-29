//! Successful reconciliation behaviour of `copy`: planning output and
//! the mutations a real copy issues.

use super::{
    SOURCE, TARGET, assert_only_target_is_mutated, labels_base, labels_path,
    page, with_token,
};
use crate::common::{
    Expectation, Isolation, MockGitHub, against_mock, labels_json,
    mock_github, run, stderr, stdout,
};

#[test]
fn copy_dry_run_reports_the_plan_and_mutates_nothing() {
    let mock = mock_github(vec![
        page(
            SOURCE,
            &[
                ("bug", "d73a4a", Some("broken")),
                ("feature", "a2eeef", None),
            ],
        ),
        page(
            TARGET,
            &[("bug", "ff0000", None), ("stale", "cccccc", None)],
        ),
    ]);
    let isolation = Isolation::new("copy-dry");
    let output = run(against_mock(
        &mock,
        &mut isolation.command(&[
            "copy",
            SOURCE,
            TARGET,
            "--prune",
            "--dry-run",
        ]),
    ));
    mock.assert_satisfied();
    // Exactly two requests: the source and target reads. Zero mutations.
    assert_eq!(mock.requests().len(), 2);
    assert_eq!(output.status.code(), Some(0));
    let out = stdout(&output);
    assert!(out.contains("UPDATE bug"), "{out}");
    assert!(out.contains("CREATE feature"), "{out}");
    assert!(out.contains("DELETE stale"), "{out}");
    let text = stderr(&output);
    assert!(text.contains("Dry run"), "{text}");
    assert!(
        text.contains(&format!("labeldeck copy {SOURCE} {TARGET} --prune")),
        "guidance must repeat both repositories: {text}"
    );
    // Reads need no credentials: an available token is used, but none
    // is required for public repositories.
    for request in mock.requests() {
        assert_eq!(request.header("authorization"), None);
    }
}

/// The apply fixture: three source labels, three target labels, so a
/// copy creates `feature`, updates `bug` (colour) and `docs`
/// (description) in place, and retains `stale` (no pruning).
const SOURCE_LABELS: &[(&str, &str, Option<&str>)] = &[
    ("bug", "d73a4a", Some("broken things")),
    ("docs", "0075ca", None),
    ("feature", "a2eeef", Some("new idea")),
];
const TARGET_LABELS: &[(&str, &str, Option<&str>)] = &[
    ("bug", "ff0000", Some("broken things")),
    ("docs", "0075ca", Some("old docs")),
    ("stale", "cccccc", None),
];

/// The mock for applying [`SOURCE_LABELS`] to [`TARGET_LABELS`]: one
/// create then two in-place updates, both reads first.
fn apply_mock() -> MockGitHub {
    mock_github(vec![
        page(SOURCE, SOURCE_LABELS),
        page(TARGET, TARGET_LABELS),
        Expectation::post(&labels_base(TARGET)).status(201),
        Expectation::patch(&format!("{}/bug", labels_base(TARGET))),
        Expectation::patch(&format!("{}/docs", labels_base(TARGET))),
    ])
}

#[test]
fn copy_applies_the_source_labels_to_the_target() {
    let mock = apply_mock();
    let isolation = Isolation::new("copy-apply");
    let mut command = isolation.command(&["copy", SOURCE, TARGET]);
    with_token(&mut command);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    let text = stderr(&output);
    assert!(
        text.contains(&format!(
            "Copied labels from {SOURCE} to {TARGET}: 1 created, \
             2 updated, 0 deleted."
        )),
        "{text}"
    );
    assert_only_target_is_mutated(&mock);
}

#[test]
fn copy_create_carries_the_exact_source_label_properties() {
    let mock = apply_mock();
    let isolation = Isolation::new("copy-payload");
    let mut command = isolation.command(&["copy", SOURCE, TARGET]);
    with_token(&mut command);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));

    // The source's exact properties become the create payload.
    let requests = mock.requests();
    let create = requests
        .iter()
        .find(|request| request.method == "POST")
        .expect("the feature create");
    assert_eq!(create.path, labels_base(TARGET));
    let payload: serde_json::Value =
        serde_json::from_str(&create.body).unwrap();
    assert_eq!(payload["name"], "feature");
    assert_eq!(payload["color"], "a2eeef");
    assert_eq!(payload["description"], "new idea");

    // The same credentials read the source and mutated the target.
    for request in requests {
        assert_eq!(
            request.header("authorization"),
            Some("Bearer gh_test_token"),
            "every request must carry the token: {} {}",
            request.method,
            request.path
        );
    }
}

#[test]
fn copy_with_prune_deletes_target_only_labels_after_other_mutations() {
    let mock = mock_github(vec![
        page(SOURCE, &[("bug", "d73a4a", Some("broken"))]),
        page(
            TARGET,
            &[
                ("bug", "ff0000", None),
                ("feature", "a2eeef", None),
                ("stale", "cccccc", None),
            ],
        ),
        Expectation::patch(&format!("{}/bug", labels_base(TARGET))),
        Expectation::delete(&format!("{}/feature", labels_base(TARGET)))
            .status(204),
        Expectation::delete(&format!("{}/stale", labels_base(TARGET)))
            .status(204),
    ]);
    let isolation = Isolation::new("copy-prune-apply");
    let mut command = isolation.command(&["copy", SOURCE, TARGET, "--prune"]);
    with_token(&mut command);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    let text = stderr(&output);
    assert!(
        text.contains("warning: pruning will DELETE 2 target-only label(s)"),
        "{text}"
    );
    assert!(text.contains("0 created, 1 updated, 2 deleted."), "{text}");
    assert_only_target_is_mutated(&mock);
    let requests = mock.requests();
    let methods: Vec<&str> =
        requests.iter().map(|r| r.method.as_str()).collect();
    assert_eq!(
        methods,
        ["GET", "GET", "PATCH", "DELETE", "DELETE"],
        "mutations must run updates before prune deletes"
    );
}

#[test]
fn copy_with_identical_sets_issues_zero_mutations() {
    let mock = mock_github(vec![
        page(SOURCE, &[("bug", "d73a4a", Some("broken"))]),
        page(TARGET, &[("bug", "d73a4a", Some("broken"))]),
    ]);
    let isolation = Isolation::new("copy-identical");
    let mut command = isolation.command(&["copy", SOURCE, TARGET, "--prune"]);
    with_token(&mut command);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(mock.requests().len(), 2, "only the two reads");
    let text = stderr(&output);
    assert!(text.contains("0 created, 0 updated, 0 deleted."), "{text}");
}

#[test]
fn copy_from_an_empty_source_with_prune_deletes_everything() {
    let mock = mock_github(vec![
        page(SOURCE, &[]),
        page(TARGET, &[("stale", "cccccc", None)]),
        Expectation::delete(&format!("{}/stale", labels_base(TARGET)))
            .status(204),
    ]);
    let isolation = Isolation::new("copy-empty-prune");
    let mut command = isolation.command(&["copy", SOURCE, TARGET, "--prune"]);
    with_token(&mut command);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    let text = stderr(&output);
    assert!(text.contains("0 created, 0 updated, 1 deleted."), "{text}");
    assert_only_target_is_mutated(&mock);
}

#[test]
fn copy_from_an_empty_source_without_prune_keeps_target_labels() {
    let mock = mock_github(vec![
        page(SOURCE, &[]),
        page(TARGET, &[("stale", "cccccc", None)]),
    ]);
    let isolation = Isolation::new("copy-empty-keep");
    let mut command = isolation.command(&["copy", SOURCE, TARGET]);
    with_token(&mut command);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(mock.requests().len(), 2, "only the two reads");
    let text = stderr(&output);
    assert!(text.contains("0 created, 0 updated, 0 deleted."), "{text}");
}

#[test]
fn copy_follows_pagination_on_both_sides() {
    let mock = mock_github(Vec::new());
    let source_page_two =
        format!("{}/repos/{SOURCE}/labels?page=2", mock.base_url());
    mock.expect(Expectation::get(&labels_path(SOURCE)).labels_page(
        &labels_json(&[("bug", "d73a4a", None)]),
        Some(&source_page_two),
    ));
    mock.expect(
        Expectation::get(&format!("/repos/{SOURCE}/labels?page=2"))
            .labels_page(&labels_json(&[("feature", "a2eeef", None)]), None),
    );
    let target_page_two =
        format!("{}/repos/{TARGET}/labels?page=2", mock.base_url());
    mock.expect(Expectation::get(&labels_path(TARGET)).labels_page(
        &labels_json(&[("bug", "d73a4a", None)]),
        Some(&target_page_two),
    ));
    mock.expect(
        Expectation::get(&format!("/repos/{TARGET}/labels?page=2"))
            .labels_page(&labels_json(&[("legacy", "bbbbbb", None)]), None),
    );

    let isolation = Isolation::new("copy-pages");
    let output = run(against_mock(
        &mock,
        &mut isolation.command(&[
            "copy",
            SOURCE,
            TARGET,
            "--prune",
            "--dry-run",
        ]),
    ));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    let out = stdout(&output);
    assert!(out.contains("UNCHANGED bug"), "{out}");
    assert!(out.contains("CREATE feature"), "{out}");
    assert!(out.contains("DELETE legacy"), "{out}");
}
