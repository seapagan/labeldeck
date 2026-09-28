//! Integration tests for `labeldeck copy` against the local mock
//! GitHub API. No test in this file contacts GitHub.

use std::process::{Command, Stdio};

use crate::common::{
    Expectation, Isolation, MockGitHub, against_mock, labels_json,
    mock_github, run, stderr, stdout, write_config,
};

const SOURCE: &str = "seapagan/template";
const TARGET: &str = "some-org/project";

/// The list-labels request path for a repository.
fn labels_path(repo: &str) -> String {
    format!("/repos/{repo}/labels?per_page=100")
}

/// A single-page labels listing for a repository.
fn page(repo: &str, labels: &[(&str, &str, Option<&str>)]) -> Expectation {
    Expectation::get(&labels_path(repo))
        .labels_page(&labels_json(labels), None)
}

/// The mutation base path for a repository's labels.
fn labels_base(repo: &str) -> String {
    format!("/repos/{repo}/labels")
}

/// The safety invariant: only reads may touch the source, and every
/// mutation must address the target repository's labels.
fn assert_only_target_is_mutated(mock: &MockGitHub) {
    for request in mock.requests() {
        if request.method == "GET" {
            continue;
        }
        assert!(
            request.path.starts_with(&labels_base(TARGET)),
            "mutation escaped the target repository: {} {}",
            request.method,
            request.path
        );
    }
}

fn with_token(command: &mut Command) -> &mut Command {
    command.env("LABELDECK_TOKEN", "gh_test_token")
}

#[test]
fn copy_requires_both_repositories() {
    let isolation = Isolation::new("copy-args");
    let output = run(&mut isolation.command(&["copy"]));
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(text.contains("<SOURCE>"), "{text}");
    assert!(text.contains("<TARGET>"), "{text}");

    let output = run(&mut isolation.command(&["copy", SOURCE]));
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(text.contains("<TARGET>"), "{text}");
}

#[test]
fn copy_rejects_malformed_repository_arguments() {
    let isolation = Isolation::new("copy-badrepo");
    for (source, target) in [
        ("octocat", TARGET),
        (SOURCE, "octocat"),
        ("a/b/c", TARGET),
        (SOURCE, "a/b/c"),
        ("/repo", TARGET),
        ("owner/", TARGET),
        ("https://github.com/seapagan/template", TARGET),
        (SOURCE, "https://github.com/some-org/project"),
        ("https://github.com/seapagan/template.git", TARGET),
        ("owner/repo extra", TARGET),
    ] {
        let output = run(&mut isolation.command(&["copy", source, target]));
        assert_eq!(
            output.status.code(),
            Some(2),
            "{source} -> {target} must be rejected"
        );
        let text = stderr(&output);
        assert!(
            text.contains("invalid repository"),
            "{source} -> {target}: {text}"
        );
    }
}

#[test]
fn copy_rejects_the_same_repository_on_both_sides() {
    let isolation = Isolation::new("copy-same");
    let output = run(&mut isolation.command(&["copy", SOURCE, SOURCE]));
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(text.contains("same repository"), "{text}");
    assert!(text.contains(SOURCE), "{text}");

    // GitHub resolves owner and repository names case-insensitively, so
    // a case difference is still the same repository.
    let output =
        run(&mut isolation.command(&["copy", SOURCE, "Seapagan/Template"]));
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("same repository"));

    // The rejection happens before anything is fetched or mutated, so
    // --dry-run changes nothing about it.
    let output =
        run(&mut isolation.command(&["copy", SOURCE, SOURCE, "--dry-run"]));
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn copy_has_no_file_option() {
    let isolation = Isolation::new("copy-file");
    let output = run(&mut isolation.command(&[
        "copy",
        SOURCE,
        TARGET,
        "--file",
        "labels.json",
    ]));
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(text.contains("unexpected argument"), "{text}");
}

#[test]
fn copy_prune_and_no_prune_conflict_is_a_usage_error() {
    let isolation = Isolation::new("copy-conflict");
    let output = run(&mut isolation.command(&[
        "copy",
        SOURCE,
        TARGET,
        "--prune",
        "--no-prune",
    ]));
    assert_eq!(output.status.code(), Some(2));
}

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

#[test]
fn copy_respects_prune_flags_and_config() {
    let isolation = Isolation::new("copy-prune");
    let reads = || {
        mock_github(vec![
            page(SOURCE, &[("bug", "d73a4a", None)]),
            page(
                TARGET,
                &[("bug", "d73a4a", None), ("legacy", "bbbbbb", None)],
            ),
        ])
    };

    // Default: retained.
    let mock = reads();
    let output = run(against_mock(
        &mock,
        &mut isolation.command(&["copy", SOURCE, TARGET, "--dry-run"]),
    ));
    mock.assert_satisfied();
    assert!(stdout(&output).contains("RETAIN legacy"));

    // Config prune = true: DELETE shown.
    write_config(&isolation, "prune = true\n");
    let mock = reads();
    let output = run(against_mock(
        &mock,
        &mut isolation.command(&["copy", SOURCE, TARGET, "--dry-run"]),
    ));
    mock.assert_satisfied();
    assert!(stdout(&output).contains("DELETE legacy"));

    // CLI --no-prune overrides the configuration.
    let mock = reads();
    let output = run(against_mock(
        &mock,
        &mut isolation.command(&[
            "copy",
            SOURCE,
            TARGET,
            "--dry-run",
            "--no-prune",
        ]),
    ));
    mock.assert_satisfied();
    assert!(stdout(&output).contains("RETAIN legacy"));

    // CLI --prune overrides a disabled configuration.
    write_config(&isolation, "prune = false\n");
    let mock = reads();
    let output = run(against_mock(
        &mock,
        &mut isolation.command(&[
            "copy",
            SOURCE,
            TARGET,
            "--dry-run",
            "--prune",
        ]),
    ));
    mock.assert_satisfied();
    assert!(stdout(&output).contains("DELETE legacy"));
}

#[test]
fn copy_applies_the_source_labels_to_the_target() {
    let mock = mock_github(vec![
        page(
            SOURCE,
            &[
                ("bug", "d73a4a", Some("broken things")),
                ("docs", "0075ca", None),
                ("feature", "a2eeef", Some("new idea")),
            ],
        ),
        page(
            TARGET,
            &[
                ("bug", "ff0000", Some("broken things")),
                ("docs", "0075ca", Some("old docs")),
                ("stale", "cccccc", None),
            ],
        ),
        Expectation::post(&labels_base(TARGET)).status(201),
        Expectation::patch(&format!("{}/bug", labels_base(TARGET))),
        Expectation::patch(&format!("{}/docs", labels_base(TARGET))),
    ]);
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
    for request in mock.requests() {
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

#[test]
fn copy_source_fetch_failure_mutates_nothing() {
    let mock = mock_github(vec![
        Expectation::get(&labels_path(SOURCE))
            .status(500)
            .body("{\"message\": \"boom\"}"),
    ]);
    let isolation = Isolation::new("copy-srcfail");
    let mut command = isolation.command(&["copy", SOURCE, TARGET]);
    with_token(&mut command);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(
        text.contains(&format!("source repository {SOURCE}")),
        "{text}"
    );
    for request in mock.requests() {
        assert_eq!(
            request.method, "GET",
            "a source failure must leave the target untouched"
        );
    }
}

#[test]
fn copy_target_fetch_failure_mutates_nothing() {
    let mock = mock_github(vec![
        page(SOURCE, &[("bug", "d73a4a", None)]),
        Expectation::get(&labels_path(TARGET))
            .status(404)
            .body("{\"message\": \"Not Found\"}"),
    ]);
    let isolation = Isolation::new("copy-tgtfail");
    let mut command = isolation.command(&["copy", SOURCE, TARGET]);
    with_token(&mut command);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(
        text.contains(&format!("target repository {TARGET}")),
        "{text}"
    );
    for request in mock.requests() {
        assert_eq!(
            request.method, "GET",
            "a target failure must leave the target untouched"
        );
    }
}

#[test]
fn copy_mutation_failure_reports_partial_application() {
    let mock = mock_github(vec![
        page(
            SOURCE,
            &[("alpha", "111111", None), ("beta", "222222", None)],
        ),
        page(TARGET, &[]),
        Expectation::post(&labels_base(TARGET)).status(201),
        Expectation::post(&labels_base(TARGET))
            .status(401)
            .body("{\"message\": \"Bad credentials\"}"),
    ]);
    let isolation = Isolation::new("copy-mutfail");
    let mut command = isolation.command(&["copy", SOURCE, TARGET]);
    with_token(&mut command);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(text.contains("applied: create label \"alpha\""), "{text}");
    assert!(text.contains("failed:   create label \"beta\""), "{text}");
    assert!(text.contains("reason:   "), "{text}");
    assert!(
        text.contains("GitHub does not support transactional"),
        "{text}"
    );
    assert_only_target_is_mutated(&mock);
}

#[test]
fn copy_without_token_fails_cleanly_when_non_interactive() {
    let isolation = Isolation::new("copy-notoken");
    let mut command = isolation.command(&["copy", SOURCE, TARGET]);
    // stdin is piped (not a terminal) by default in Command::output.
    command.stdin(Stdio::null());
    let output = run(&mut command);
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(text.contains("authentication is required"), "{text}");
    assert!(
        text.contains("labeldeck auth login"),
        "must point at the fix: {text}"
    );
}

#[test]
fn copy_never_resolves_a_local_or_global_deck() {
    let isolation = Isolation::new("copy-nodeck");
    // Decoy decks in every location a read command would resolve; copy
    // must ignore them because the desired set comes from the source.
    let workdir = tempfile::tempdir().unwrap();
    std::fs::write(
        workdir.path().join("labels.json"),
        "this is not even JSON",
    )
    .unwrap();
    std::fs::write(
        isolation.config_dir.join("labels.json"),
        "neither is this",
    )
    .unwrap();
    let mock = mock_github(vec![
        page(SOURCE, &[("bug", "d73a4a", None)]),
        page(TARGET, &[("bug", "d73a4a", None)]),
    ]);
    let mut command =
        isolation.command(&["copy", SOURCE, TARGET, "--dry-run"]);
    command.current_dir(workdir.path());
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(mock.requests().len(), 2);
}
