//! Explicit global selection through parsing and the binary's real read path.
use super::*;
use clap::{Parser, error::ErrorKind};
use labeldeck::cli::Cli;

fn assert_global_notice(
    output: &std::process::Output,
    global: &std::path::Path,
) {
    let notice = format!("Using global deck: {global:?}");
    let text = stderr(output);
    assert_eq!(text.matches("Using global deck:").count(), 1, "{text}");
    assert_eq!(
        text.lines().filter(|line| *line == notice).count(),
        1,
        "{text}"
    );
    assert!(!stdout(output).contains("Using global deck:"));
}

#[test]
fn global_is_accepted_for_diff_and_every_sync_mode() {
    for args in [
        vec!["labeldeck", "diff", REPO, "--global"],
        vec!["labeldeck", "sync", REPO, "--global"],
        vec!["labeldeck", "sync", REPO, "--global", "--dry-run"],
        vec!["labeldeck", "sync", REPO, "--global", "--interactive"],
    ] {
        assert!(Cli::try_parse_from(&args).is_ok(), "{args:?}");
    }
}

#[test]
fn global_conflicts_with_file_in_both_orders_and_every_mode() {
    for mode in [
        vec!["diff"],
        vec!["sync"],
        vec!["sync", "--dry-run"],
        vec!["sync", "--interactive"],
    ] {
        for flags in [
            ["--global", "--file", "chosen.json"],
            ["--file", "chosen.json", "--global"],
        ] {
            let args =
                [vec!["labeldeck"], mode.clone(), vec![REPO], flags.to_vec()]
                    .concat();
            let error = Cli::try_parse_from(&args).err().expect("conflict");
            assert_eq!(error.kind(), ErrorKind::ArgumentConflict, "{args:?}");
        }
    }
}

#[test]
fn global_interactive_still_conflicts_with_dry_run() {
    let error = Cli::try_parse_from([
        "labeldeck",
        "sync",
        REPO,
        "--global",
        "--interactive",
        "--dry-run",
    ])
    .err()
    .expect("conflict");
    assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
}

#[test]
fn diff_global_consumes_global_even_with_a_valid_local_deck() {
    let isolation = Isolation::new("explicit-global-diff");
    let global = canonical_file(&isolation, "labels.json", GLOBAL_DECK);
    let dir = workdir("explicit-global-diff");
    std::fs::write(dir.join("labels.json"), LOCAL_DECK).unwrap();
    let mock = mock_github(remote_page(&[]));
    let mut command = isolation.command(&["diff", REPO, "--global"]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stdout(&output),
        "CREATE docs (color 0075ca, description (none))\n"
    );
    assert_global_notice(&output, &global);
}

#[test]
fn sync_global_dry_run_is_read_only_and_pins_the_selected_path() {
    let isolation = Isolation::new("explicit-global-dry-run");
    let global = canonical_file(&isolation, "labels.json", GLOBAL_DECK);
    let dir = workdir("explicit-global-dry-run");
    let local = dir.join("labels.json");
    std::fs::write(&local, LOCAL_DECK).unwrap();
    let mock = mock_github(remote_page(&[]));
    let mut command =
        isolation.command(&["sync", REPO, "--global", "--dry-run"]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("CREATE docs"));
    assert!(!stdout(&output).contains("bug"));
    let guidance = stderr(&output);
    assert!(
        guidance.contains(&format!("--file {} --no-prune", global.display()))
            || guidance.contains(&format!("file:       {global:?}")),
        "{guidance}"
    );
    assert!(!guidance.contains("--global"));
    assert_global_notice(&output, &global);
    assert_eq!(
        stdout(&output),
        "CREATE docs (color 0075ca, description (none))\n"
    );
    assert_eq!(mock.requests().len(), 1);
    assert_eq!(std::fs::read_to_string(local).unwrap(), LOCAL_DECK);
    assert_eq!(std::fs::read_to_string(global).unwrap(), GLOBAL_DECK);
}

#[test]
fn sync_global_applies_only_global_labels_without_writing_decks() {
    let isolation = Isolation::new("explicit-global-apply");
    let global = canonical_file(&isolation, "labels.json", GLOBAL_DECK);
    let dir = workdir("explicit-global-apply");
    let local = dir.join("labels.json");
    std::fs::write(&local, LOCAL_DECK).unwrap();
    let mut expectations = remote_page(&[]);
    expectations
        .push(Expectation::post(&format!("/repos/{REPO}/labels")).status(201));
    let mock = mock_github(expectations);
    let mut command = isolation.command(&["sync", REPO, "--global"]);
    command
        .current_dir(&dir)
        .env("LABELDECK_TOKEN", "test-token");
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let body: serde_json::Value =
        serde_json::from_str(&mock.requests()[1].body).unwrap();
    assert_eq!(body["name"], "docs");
    assert_eq!(body["color"], "0075ca");
    assert_global_notice(&output, &global);
    assert_eq!(std::fs::read_to_string(local).unwrap(), LOCAL_DECK);
    assert_eq!(std::fs::read_to_string(global).unwrap(), GLOBAL_DECK);
}

#[test]
fn global_dry_run_guidance_handles_shell_special_config_paths() {
    let isolation = Isolation::new("explicit-global-special-path");
    let config_dir = isolation.config_dir.join("deck $; dir");
    std::fs::create_dir(&config_dir).unwrap();
    let global = config_dir.join("labels.json");
    std::fs::write(&global, GLOBAL_DECK).unwrap();
    let dir = workdir("explicit-global-special-path");
    std::fs::write(dir.join("labels.json"), LOCAL_DECK).unwrap();
    let mock = mock_github(remote_page(&[]));
    let mut command =
        isolation.command(&["sync", REPO, "--global", "--dry-run"]);
    command
        .current_dir(&dir)
        .env("LABELDECK_CONFIG_DIR", &config_dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("CREATE docs"));
    let guidance = stderr(&output);
    assert!(
        guidance.contains(&format!("file:       {global:?}")),
        "{guidance}"
    );
    assert!(!guidance.contains("labeldeck sync"), "{guidance}");
    assert_global_notice(&output, &global);
}

// macOS filesystems can reject raw non-UTF-8 names; guidance tests remain Unix-wide.
#[cfg(target_os = "linux")]
#[test]
fn explicit_non_utf8_file_remains_authoritative() {
    use std::os::unix::ffi::OsStrExt;
    let isolation = Isolation::new("explicit-non-utf8-file");
    canonical_file(&isolation, "labels.json", GLOBAL_DECK);
    let dir = workdir("explicit-non-utf8-file");
    std::fs::write(dir.join("labels.json"), GLOBAL_DECK).unwrap();
    let file = dir
        .path()
        .join(std::ffi::OsStr::from_bytes(b"chosen\xff.json"));
    std::fs::write(&file, LOCAL_DECK).unwrap();
    for args in [vec!["diff", REPO], vec!["sync", REPO, "--dry-run"]] {
        let mock = mock_github(remote_page(&[("bug", "d73a4a", None)]));
        let mut command = isolation.command(&args);
        command.current_dir(&dir).arg("--file").arg(&file);
        let output = run(against_mock(&mock, &mut command));
        mock.assert_satisfied();
        assert_eq!(output.status.code(), Some(0));
        assert!(stdout(&output).contains("UNCHANGED bug"));
        assert!(!stdout(&output).contains("docs"));
    }
}

fn assert_global_error(isolation: &Isolation, dir: &Workdir, expected: &str) {
    let global = isolation.config_dir.join("labels.json");
    for args in [
        vec!["diff", REPO, "--global"],
        vec!["sync", REPO, "--global"],
        vec!["sync", REPO, "--global", "--dry-run"],
    ] {
        let mock = mock_github(vec![]);
        let mut command = isolation.command(&args);
        command.current_dir(dir);
        let output = run(against_mock(&mock, &mut command));
        assert_eq!(output.status.code(), Some(2));
        let error = stderr(&output);
        assert!(error.contains(global.to_string_lossy().as_ref()), "{error}");
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("Using global deck:"), "{error}");
        assert!(stdout(&output).is_empty());
        assert!(mock.requests().is_empty());
        mock.assert_satisfied();
    }
}

#[test]
fn implicit_global_read_reports_once_for_diff_and_both_sync_paths() {
    let isolation = Isolation::new("implicit-global-notice");
    let global = canonical_file(&isolation, "labels.json", GLOBAL_DECK);
    let dir = workdir("implicit-global-notice");
    for args in [
        vec!["diff", REPO],
        vec!["sync", REPO, "--dry-run"],
        vec!["sync", REPO],
    ] {
        let mock = mock_github(remote_page(&[("docs", "0075ca", None)]));
        let mut command = isolation.command(&args);
        command
            .current_dir(&dir)
            .env("LABELDECK_TOKEN", "test-token");
        let output = run(against_mock(&mock, &mut command));
        mock.assert_satisfied();
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        assert_global_notice(&output, &global);
        assert_eq!(mock.requests().len(), 1);
    }
}

#[test]
fn local_and_explicit_global_paths_do_not_report_global_selection() {
    let isolation = Isolation::new("non-global-notice");
    let global = canonical_file(&isolation, "labels.json", GLOBAL_DECK);
    let dir = workdir("non-global-notice");
    std::fs::write(dir.join("labels.json"), LOCAL_DECK).unwrap();
    for (explicit, labels) in [
        (false, vec![("bug", "d73a4a", None)]),
        (true, vec![("docs", "0075ca", None)]),
    ] {
        for args in [
            vec!["diff", REPO],
            vec!["sync", REPO, "--dry-run"],
            vec!["sync", REPO],
        ] {
            let mock = mock_github(remote_page(&labels));
            let mut command = isolation.command(&args);
            command
                .current_dir(&dir)
                .env("LABELDECK_TOKEN", "test-token");
            if explicit {
                command.arg("--file").arg(&global);
            }
            let output = run(against_mock(&mock, &mut command));
            mock.assert_satisfied();
            assert_eq!(output.status.code(), Some(0));
            assert!(!stderr(&output).contains("Using global deck:"));
        }
    }
}

#[cfg(unix)]
#[test]
fn global_notice_escapes_control_characters_in_config_paths() {
    let isolation = Isolation::new("global-notice-control-path");
    let config_dir = isolation.config_dir.join("deck\n\t\x1bdir");
    std::fs::create_dir(&config_dir).unwrap();
    let global = config_dir.join("labels.json");
    std::fs::write(&global, GLOBAL_DECK).unwrap();
    let dir = workdir("global-notice-control-path");
    let mock = mock_github(remote_page(&[]));
    let mut command =
        isolation.command(&["sync", REPO, "--global", "--dry-run"]);
    command
        .current_dir(&dir)
        .env("LABELDECK_CONFIG_DIR", &config_dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    assert_global_notice(&output, &global);
    let text = stderr(&output);
    assert!(!text.contains('\x1b') && !text.contains('\t'), "{text}");
    assert!(!text.contains("deck\n"), "{text}");
    assert!(text.contains(&format!("file:       {global:?}")), "{text}");
    assert!(!text.contains("--global"), "{text}");
}

#[test]
fn selected_global_errors_never_fall_back_to_valid_local() {
    for (contents, expected) in [
        (None, "could not read"),
        (Some("{not json"), "invalid JSON"),
        (
            Some(
                "[{\"name\":\"\",\"color\":\"0075ca\",\"description\":\"\"}]",
            ),
            "name must not be empty",
        ),
    ] {
        let isolation = Isolation::new("explicit-global-error");
        let dir = workdir("explicit-global-error");
        std::fs::write(dir.join("labels.json"), LOCAL_DECK).unwrap();
        if let Some(contents) = contents {
            canonical_file(&isolation, "labels.json", contents);
        }
        assert_global_error(&isolation, &dir, expected);
    }
}

#[test]
fn unreadable_global_entry_never_falls_back_to_valid_local() {
    let isolation = Isolation::new("explicit-global-unreadable");
    let dir = workdir("explicit-global-unreadable");
    std::fs::write(dir.join("labels.json"), LOCAL_DECK).unwrap();
    // Reading a directory fails even as root, on every supported platform.
    std::fs::create_dir(isolation.config_dir.join("labels.json")).unwrap();
    assert_global_error(&isolation, &dir, "could not read");
}

#[cfg(unix)]
#[test]
fn global_symlink_loop_reports_read_failure_without_local_fallback() {
    let isolation = Isolation::new("explicit-global-loop");
    let dir = workdir("explicit-global-loop");
    std::fs::write(dir.join("labels.json"), LOCAL_DECK).unwrap();
    std::os::unix::fs::symlink(
        "labels.json",
        isolation.config_dir.join("labels.json"),
    )
    .unwrap();
    assert_global_error(&isolation, &dir, "could not read");
}

#[test]
fn global_selection_ignores_an_occupied_local_entry() {
    let isolation = Isolation::new("explicit-global-occupied");
    canonical_file(&isolation, "labels.json", GLOBAL_DECK);
    let dir = workdir("explicit-global-occupied");
    std::fs::create_dir(dir.join("labels.json")).unwrap();
    let mock = mock_github(remote_page(&[("docs", "0075ca", None)]));
    let mut command = isolation.command(&["diff", REPO, "--global"]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("UNCHANGED docs"));
}

#[test]
fn global_diff_preserves_config_prune_and_cli_overrides() {
    let isolation = Isolation::new("explicit-global-prune");
    canonical_file(&isolation, "labels.json", GLOBAL_DECK);
    write_config(&isolation, "prune = true\n");
    let dir = workdir("explicit-global-prune");
    std::fs::write(dir.join("labels.json"), LOCAL_DECK).unwrap();
    for (flags, code, operation) in [
        (vec![], 1, "DELETE extra"),
        (vec!["--no-prune"], 0, "RETAIN extra"),
        (vec!["--prune"], 1, "DELETE extra"),
    ] {
        let mock = mock_github(remote_page(&[
            ("docs", "0075ca", None),
            ("extra", "ededed", None),
        ]));
        let args = [vec!["diff", REPO, "--global"], flags].concat();
        let mut command = isolation.command(&args);
        command.current_dir(&dir);
        let output = run(against_mock(&mock, &mut command));
        mock.assert_satisfied();
        assert_eq!(output.status.code(), Some(code));
        assert!(stdout(&output).contains(operation));
        assert!(stdout(&output).contains("UNCHANGED docs"));
    }
}
