//! Failure, safety, and authentication guarantees of `copy`: fetch
//! failures leave the target untouched, mutations only ever address the
//! target, and credential handling matches the other commands.

use std::process::Stdio;

use super::{
    SOURCE, TARGET, assert_only_target_is_mutated, labels_base, labels_path,
    page, with_token,
};
use crate::common::{
    Expectation, Isolation, against_mock, mock_github, run, stderr,
};

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
