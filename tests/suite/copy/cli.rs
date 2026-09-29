//! Argument parsing, usage errors, and prune configuration for `copy`.

use super::{SOURCE, TARGET, page};
use crate::common::{
    Isolation, against_mock, mock_github, run, stderr, stdout, write_config,
};
use std::process::Output;

/// Run `copy --dry-run` (plus any `extra` flags) against fresh reads of
/// the given source and target labels and return its standard output.
fn dry_run_stdout(
    isolation: &Isolation,
    extra: &[&str],
    source: &[(&str, &str, Option<&str>)],
    target: &[(&str, &str, Option<&str>)],
) -> String {
    let mock = mock_github(vec![page(SOURCE, source), page(TARGET, target)]);
    let mut args = vec!["copy", SOURCE, TARGET];
    args.extend_from_slice(extra);
    args.push("--dry-run");
    let output: Output =
        run(against_mock(&mock, &mut isolation.command(&args)));
    mock.assert_satisfied();
    stdout(&output)
}

/// The prune-matrix fixture: the target has one matching label and one
/// target-only label.
const MATCHED: &[(&str, &str, Option<&str>)] = &[("bug", "d73a4a", None)];
const WITH_LEGACY: &[(&str, &str, Option<&str>)] =
    &[("bug", "d73a4a", None), ("legacy", "bbbbbb", None)];

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
    let mock = mock_github(vec![page(SOURCE, MATCHED), page(TARGET, MATCHED)]);
    let mut command =
        isolation.command(&["copy", SOURCE, TARGET, "--dry-run"]);
    command.current_dir(workdir.path());
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(mock.requests().len(), 2);
}

#[test]
fn copy_prune_defaults_to_retaining_target_only_labels() {
    let isolation = Isolation::new("copy-prune-default");
    let out = dry_run_stdout(&isolation, &[], MATCHED, WITH_LEGACY);
    assert!(out.contains("RETAIN legacy"), "{out}");
}

#[test]
fn copy_prune_configuration_enables_deletion() {
    let isolation = Isolation::new("copy-prune-config");
    write_config(&isolation, "prune = true\n");
    let out = dry_run_stdout(&isolation, &[], MATCHED, WITH_LEGACY);
    assert!(out.contains("DELETE legacy"), "{out}");
}

#[test]
fn copy_no_prune_overrides_configured_pruning() {
    let isolation = Isolation::new("copy-prune-override-off");
    write_config(&isolation, "prune = true\n");
    let out =
        dry_run_stdout(&isolation, &["--no-prune"], MATCHED, WITH_LEGACY);
    assert!(out.contains("RETAIN legacy"), "{out}");
}

#[test]
fn copy_prune_overrides_disabled_configuration() {
    let isolation = Isolation::new("copy-prune-override-on");
    write_config(&isolation, "prune = false\n");
    let out = dry_run_stdout(&isolation, &["--prune"], MATCHED, WITH_LEGACY);
    assert!(out.contains("DELETE legacy"), "{out}");
}
