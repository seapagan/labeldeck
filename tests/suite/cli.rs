//! Binary-level integration tests: run the compiled `labeldeck` binary
//! against the local mock GitHub API.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use crate::common::{Expectation, MockGitHub, labels_json, mock_github};

const BIN: &str = env!("CARGO_BIN_EXE_labeldeck");
const REPO: &str = "octocat/hello-world";

struct Isolation {
    config_dir: PathBuf,
}

impl Isolation {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "labeldeck-cli-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self { config_dir: dir }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(BIN);
        command
            .args(args)
            .env("LABELDECK_CONFIG_DIR", &self.config_dir)
            .env_remove("LABELDECK_TOKEN")
            .env_remove("GH_TOKEN")
            .env_remove("GITHUB_TOKEN")
            .env_remove("LABELDECK_API");
        command
    }
}

impl Drop for Isolation {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.config_dir).ok();
    }
}

fn run(command: &mut Command) -> Output {
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run labeldeck binary")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn against_mock<'a>(
    mock: &MockGitHub,
    command: &'a mut Command,
) -> &'a mut Command {
    command.env("LABELDECK_API", mock.base_url())
}

fn canonical_file(dir: &Isolation, name: &str, contents: &str) -> PathBuf {
    let path = dir.config_dir.join(name);
    std::fs::write(&path, contents).unwrap();
    path
}
fn write_config(isolation: &Isolation, contents: &str) {
    std::fs::write(isolation.config_dir.join("config.toml"), contents)
        .unwrap();
}

#[test]
fn help_exits_zero_and_lists_commands() {
    let isolation = Isolation::new("help");
    let output = run(&mut isolation.command(&["--help"]));
    assert!(output.status.success());
    let text = stdout(&output);
    for command in ["export", "diff", "sync", "auth"] {
        assert!(text.contains(command), "help must mention {command}");
    }
}

#[test]
fn version_reports_name_and_semver() {
    let isolation = Isolation::new("version");
    let output = run(&mut isolation.command(&["--version"]));
    assert!(output.status.success());
    assert_eq!(stdout(&output), "labeldeck 0.1.0\n");
}

#[test]
fn invalid_repo_spec_is_a_clean_usage_error() {
    let isolation = Isolation::new("badrepo");
    let output = run(&mut isolation.command(&["export", "not-a-repo"]));
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(text.contains("OWNER/REPO"), "{text}");
    // No panic output.
    assert!(!text.contains("panicked"), "{text}");
}

#[test]
fn export_writes_deterministic_json_to_stdout() {
    let mock = mock_github(vec![
        Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
            .labels_page(
                &labels_json(&[
                    ("bug", "d73a4a", Some("broken")),
                    ("feature", "a2eeef", None),
                ]),
                None,
            ),
    ]);
    let isolation = Isolation::new("export-stdout");
    let mut command = isolation.command(&["export", REPO]);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert!(output.status.success());
    let text = stdout(&output);
    let expected = "[\n  {\n    \"name\": \"bug\",\n    \"color\": \
                    \"d73a4a\",\n    \"description\": \"broken\"\n  },\n  \
                    {\n    \"name\": \"feature\",\n    \"color\": \
                    \"a2eeef\",\n    \"description\": \"\"\n  }\n]\n";
    assert_eq!(text, expected);
}

#[test]
fn export_to_file_refuses_overwrite_without_force() {
    let page = || {
        vec![
            Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
                .labels_page(&labels_json(&[("bug", "d73a4a", None)]), None),
        ]
    };
    let isolation = Isolation::new("export-force");
    let target = isolation.config_dir.join("labels.json");

    let mock = mock_github(page());
    let output = run(against_mock(
        &mock,
        &mut isolation.command(&["export", REPO, target.to_str().unwrap()]),
    ));
    mock.assert_satisfied();
    assert!(output.status.success());
    assert!(target.exists());

    let mock = mock_github(page());
    let output = run(against_mock(
        &mock,
        &mut isolation.command(&["export", REPO, target.to_str().unwrap()]),
    ));
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("refusing to overwrite"),
        "{}",
        stderr(&output)
    );

    let mock = mock_github(page());
    let output = run(against_mock(
        &mock,
        &mut isolation.command(&[
            "export",
            REPO,
            target.to_str().unwrap(),
            "--force",
        ]),
    ));
    mock.assert_satisfied();
    assert!(output.status.success());
}

#[test]
fn export_is_anonymous_when_no_token_exists() {
    let mock = mock_github(vec![
        Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
            .labels_page("[]", None),
    ]);
    let isolation = Isolation::new("export-anon");
    let mut command = isolation.command(&["export", REPO]);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert!(output.status.success());
    assert_eq!(
        mock.requests()[0].header("authorization"),
        None,
        "anonymous export must not send credentials"
    );
}

#[test]
fn export_with_env_token_sends_bearer() {
    let mock = mock_github(vec![
        Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
            .labels_page("[]", None),
    ]);
    let isolation = Isolation::new("export-token");
    let mut command = isolation.command(&["export", REPO]);
    command.env("GH_TOKEN", "gh_cli_env_token");
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert!(output.status.success());
    assert_eq!(
        mock.requests()[0].header("authorization"),
        Some("Bearer gh_cli_env_token")
    );
}

#[test]
fn diff_exit_codes_distinguish_clean_from_differences() {
    let isolation = Isolation::new("diff-exit");
    let file = canonical_file(
        &isolation,
        "canonical.json",
        "[{\"name\": \"bug\", \"color\": \"d73a4a\", \
          \"description\": \"\"}]",
    );

    // Identical: exit 0.
    let mock = mock_github(vec![
        Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
            .labels_page(&labels_json(&[("bug", "d73a4a", None)]), None),
    ]);
    let output = run(against_mock(
        &mock,
        &mut isolation.command(&["diff", file.to_str().unwrap(), REPO]),
    ));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("UNCHANGED bug"));

    // Differences: exit 1, no mutations.
    let mock = mock_github(vec![
        Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
            .labels_page(
                &labels_json(&[("docs", "0075ca", Some("old"))]),
                None,
            ),
    ]);
    let output = run(against_mock(
        &mock,
        &mut isolation.command(&["diff", file.to_str().unwrap(), REPO]),
    ));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(1));
    let text = stdout(&output);
    assert!(text.contains("CREATE bug"), "{text}");
    assert!(text.contains("RETAIN docs"), "{text}");
    assert!(!text.contains("DELETE"), "{text}");
}

#[test]
fn diff_respects_prune_flags_and_config() {
    let isolation = Isolation::new("diff-prune");
    let file = canonical_file(
        &isolation,
        "canonical.json",
        "[{\"name\": \"bug\", \"color\": \"d73a4a\", \"description\": \"\"}]",
    );

    let remote = || {
        mock_github(vec![
            Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
                .labels_page(
                    &labels_json(&[
                        ("bug", "d73a4a", None),
                        ("legacy", "bbbbbb", None),
                    ]),
                    None,
                ),
        ])
    };

    // Default: retained.
    let mock = remote();
    let output = run(against_mock(
        &mock,
        &mut isolation.command(&["diff", file.to_str().unwrap(), REPO]),
    ));
    mock.assert_satisfied();
    assert!(stdout(&output).contains("RETAIN legacy"));

    // Config prune = true: DELETE shown.
    write_config(&isolation, "prune = true\n");
    let mock = remote();
    let output = run(against_mock(
        &mock,
        &mut isolation.command(&["diff", file.to_str().unwrap(), REPO]),
    ));
    mock.assert_satisfied();
    assert!(stdout(&output).contains("DELETE legacy"));

    // CLI --no-prune overrides config.
    let mock = remote();
    let output = run(against_mock(
        &mock,
        &mut isolation.command(&[
            "diff",
            file.to_str().unwrap(),
            REPO,
            "--no-prune",
        ]),
    ));
    mock.assert_satisfied();
    assert!(stdout(&output).contains("RETAIN legacy"));

    // CLI --prune overrides disabled config.
    write_config(&isolation, "prune = false\n");
    let mock = remote();
    let output = run(against_mock(
        &mock,
        &mut isolation.command(&[
            "diff",
            file.to_str().unwrap(),
            REPO,
            "--prune",
        ]),
    ));
    mock.assert_satisfied();
    assert!(stdout(&output).contains("DELETE legacy"));
}

#[test]
fn diff_prune_and_no_prune_conflict_is_a_usage_error() {
    let isolation = Isolation::new("diff-conflict");
    let file = canonical_file(
        &isolation,
        "canonical.json",
        "[{\"name\": \"bug\", \"color\": \"d73a4a\", \"description\": \"\"}]",
    );
    let output = run(&mut isolation.command(&[
        "diff",
        file.to_str().unwrap(),
        REPO,
        "--prune",
        "--no-prune",
    ]));
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn diff_reports_invalid_canonical_file_precisely() {
    let isolation = Isolation::new("diff-badfile");
    let file = canonical_file(
        &isolation,
        "canonical.json",
        "[{\"name\": \"bug\", \"color\": \"d73a4a\", \"description\": \"\"},\
          {\"name\": \"BUG\", \"color\": \"0075ca\", \"description\": \"\"}]",
    );
    let output =
        run(&mut isolation.command(&["diff", file.to_str().unwrap(), REPO]));
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(text.contains("canonical.json"), "{text}");
    assert!(text.contains("duplicate label name"), "{text}");
}

#[test]
fn sync_dry_run_performs_no_mutations() {
    let mock = mock_github(vec![
        Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
            .labels_page(&labels_json(&[("stale", "cccccc", None)]), None),
    ]);
    let isolation = Isolation::new("sync-dry");
    let file = canonical_file(
        &isolation,
        "canonical.json",
        "[{\"name\": \"bug\", \"color\": \"d73a4a\", \"description\": \"\"}]",
    );
    let mut command = isolation.command(&[
        "sync",
        file.to_str().unwrap(),
        REPO,
        "--prune",
        "--dry-run",
    ]);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    // Exactly one request: the read. Zero mutations.
    assert_eq!(mock.requests().len(), 1);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("CREATE bug"));
    assert!(stdout(&output).contains("DELETE stale"));
    assert!(stderr(&output).contains("Dry run"));
}

#[test]
fn sync_without_token_fails_cleanly_when_non_interactive() {
    let isolation = Isolation::new("sync-notoken");
    let file = canonical_file(
        &isolation,
        "canonical.json",
        "[{\"name\": \"bug\", \"color\": \"d73a4a\", \"description\": \"\"}]",
    );
    let mut command =
        isolation.command(&["sync", file.to_str().unwrap(), REPO]);
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
fn sync_applies_plan_and_exits_zero() {
    let mock = mock_github(vec![
        Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
            .labels_page(
                &labels_json(&[("docs", "0075ca", Some("old"))]),
                None,
            ),
        Expectation::post(&format!("/repos/{REPO}/labels")).status(201),
        Expectation::patch(&format!("/repos/{REPO}/labels/docs")),
    ]);
    let isolation = Isolation::new("sync-apply");
    let file = canonical_file(
        &isolation,
        "canonical.json",
        "[{\"name\": \"bug\", \"color\": \"d73a4a\", \"description\": \"\"},\
          {\"name\": \"docs\", \"color\": \"00ff00\", \"description\": \
          \"new\"}]",
    );
    let mut command =
        isolation.command(&["sync", file.to_str().unwrap(), REPO]);
    command.env("LABELDECK_TOKEN", "gh_test_token");
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    assert!(stderr(&output).contains("Synchronized"));
}

#[test]
fn auth_status_reports_env_token_without_printing_it() {
    let isolation = Isolation::new("status-env");
    let mut command = isolation.command(&["auth", "status"]);
    command.env("LABELDECK_TOKEN", "gh_super_secret_value");
    let output = run(&mut command);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("LABELDECK_TOKEN"), "{text}");
    assert!(
        !text.contains("gh_super_secret_value"),
        "token leaked: {text}"
    );
}

#[test]
fn auth_status_reports_stored_token_and_anonymous() {
    let isolation = Isolation::new("status-stored");

    let output = run(&mut isolation.command(&["auth", "status"]));
    assert!(output.status.success());
    assert!(stdout(&output).contains("none (anonymous"));

    std::fs::write(isolation.config_dir.join("token"), "stored-token\n")
        .unwrap();
    let output = run(&mut isolation.command(&["auth", "status"]));
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("stored token at"), "{text}");
    assert!(!text.contains("stored-token\n"), "{text}");
}

#[test]
fn auth_login_token_stdin_validates_and_stores() {
    let mock = mock_github(vec![
        Expectation::get("/user").body(r#"{"login":"seapagan"}"#),
    ]);
    let isolation = Isolation::new("login");
    let mut command = isolation.command(&["auth", "login", "--token-stdin"]);
    against_mock(&mock, &mut command);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn labeldeck");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"gh_test_token_123\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    mock.assert_satisfied();
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("seapagan"), "{text}");
    let stored =
        std::fs::read_to_string(isolation.config_dir.join("token")).unwrap();
    assert_eq!(stored.trim(), "gh_test_token_123");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(isolation.config_dir.join("token"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn auth_login_interactive_requires_a_terminal() {
    let isolation = Isolation::new("login-notty");
    let mut command = isolation.command(&["auth", "login"]);
    command.stdin(Stdio::null());
    let output = run(&mut command);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("--token-stdin"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn environment_tokens_are_never_persisted() {
    let mock = mock_github(vec![
        Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
            .labels_page(&labels_json(&[("bug", "d73a4a", None)]), None),
    ]);
    let isolation = Isolation::new("env-no-persist");
    let file = canonical_file(
        &isolation,
        "labels.json",
        "[{\"name\": \"bug\", \"color\": \"d73a4a\", \"description\": \"\"}]",
    );
    let mut command =
        isolation.command(&["diff", file.to_str().unwrap(), REPO]);
    command.env("GITHUB_TOKEN", "gh_env_secret_token");
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    assert!(
        !isolation.config_dir.join("token").exists(),
        "environment token must never be written to disk"
    );

    // `auth status` with an environment token also reports it without
    // persisting anything.
    let mut command = isolation.command(&["auth", "status"]);
    command.env("GITHUB_TOKEN", "gh_env_secret_token");
    let output = run(&mut command);
    assert!(output.status.success());
    assert!(stdout(&output).contains("GITHUB_TOKEN"));
    assert!(
        !isolation.config_dir.join("token").exists(),
        "auth status must not persist the environment token"
    );
}

#[test]
fn token_never_appears_in_error_output() {
    let mock = mock_github(vec![
        Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
            .status(401)
            .body(r#"{"message":"Bad credentials"}"#),
    ]);
    let isolation = Isolation::new("no-leak");
    let file = canonical_file(
        &isolation,
        "labels.json",
        "[{\"name\": \"bug\", \"color\": \"d73a4a\", \"description\": \"\"}]",
    );
    let mut command =
        isolation.command(&["diff", file.to_str().unwrap(), REPO]);
    command.env("LABELDECK_TOKEN", "gh_super_secret_token_42");
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(2));
    let text = format!("{}{}", stdout(&output), stderr(&output));
    assert!(
        !text.contains("gh_super_secret_token_42"),
        "token leaked into error output: {text}"
    );
}
#[test]
fn auth_logout_removes_stored_token() {
    let isolation = Isolation::new("logout");
    std::fs::write(isolation.config_dir.join("token"), "x").unwrap();
    let output = run(&mut isolation.command(&["auth", "logout"]));
    assert!(output.status.success());
    assert!(!isolation.config_dir.join("token").exists());

    let output = run(&mut isolation.command(&["auth", "logout"]));
    assert!(output.status.success());
    assert!(stdout(&output).contains("No stored token"));
}

#[test]
fn malformed_config_is_a_clean_error() {
    let isolation = Isolation::new("badconfig");
    write_config(&isolation, "prune = maybe\n");
    let file = canonical_file(
        &isolation,
        "canonical.json",
        "[{\"name\": \"bug\", \"color\": \"d73a4a\", \"description\": \"\"}]",
    );
    let output =
        run(&mut isolation.command(&["diff", file.to_str().unwrap(), REPO]));
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("invalid configuration file"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn missing_canonical_file_is_a_clean_error() {
    let isolation = Isolation::new("nofile");
    let output = run(&mut isolation.command(&[
        "diff",
        isolation.config_dir.join("absent.json").to_str().unwrap(),
        REPO,
    ]));
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("could not read"),
        "{}",
        stderr(&output)
    );
}
