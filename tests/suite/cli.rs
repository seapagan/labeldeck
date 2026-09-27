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
            .env_remove("LABELDECK_API")
            .env_remove("HTTP_PROXY")
            .env_remove("http_proxy")
            .env_remove("HTTPS_PROXY")
            .env_remove("https_proxy")
            .env_remove("ALL_PROXY")
            .env_remove("all_proxy")
            .env_remove("NO_PROXY")
            .env_remove("no_proxy");
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
    let mut command = isolation.command(&["export", REPO, "--file", "-"]);
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
        &mut isolation.command(&[
            "export",
            REPO,
            "--file",
            target.to_str().unwrap(),
        ]),
    ));
    mock.assert_satisfied();
    assert!(output.status.success());
    assert!(target.exists());

    let mock = mock_github(page());
    let output = run(against_mock(
        &mock,
        &mut isolation.command(&[
            "export",
            REPO,
            "--file",
            target.to_str().unwrap(),
        ]),
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
            "--file",
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
    let mut command = isolation.command(&["export", REPO, "--file", "-"]);
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
    let mut command = isolation.command(&["export", REPO, "--file", "-"]);
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
        &mut isolation.command(&[
            "diff",
            REPO,
            "--file",
            file.to_str().unwrap(),
        ]),
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
        &mut isolation.command(&[
            "diff",
            REPO,
            "--file",
            file.to_str().unwrap(),
        ]),
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
        &mut isolation.command(&[
            "diff",
            REPO,
            "--file",
            file.to_str().unwrap(),
        ]),
    ));
    mock.assert_satisfied();
    assert!(stdout(&output).contains("RETAIN legacy"));

    // Config prune = true: DELETE shown.
    write_config(&isolation, "prune = true\n");
    let mock = remote();
    let output = run(against_mock(
        &mock,
        &mut isolation.command(&[
            "diff",
            REPO,
            "--file",
            file.to_str().unwrap(),
        ]),
    ));
    mock.assert_satisfied();
    assert!(stdout(&output).contains("DELETE legacy"));

    // CLI --no-prune overrides config.
    let mock = remote();
    let output = run(against_mock(
        &mock,
        &mut isolation.command(&[
            "diff",
            REPO,
            "--file",
            file.to_str().unwrap(),
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
            REPO,
            "--file",
            file.to_str().unwrap(),
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
        REPO,
        "--file",
        file.to_str().unwrap(),
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
    let output = run(&mut isolation.command(&[
        "diff",
        REPO,
        "--file",
        file.to_str().unwrap(),
    ]));
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
        REPO,
        "--file",
        file.to_str().unwrap(),
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
        isolation.command(&["sync", REPO, "--file", file.to_str().unwrap()]);
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
        isolation.command(&["sync", REPO, "--file", file.to_str().unwrap()]);
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
        isolation.command(&["diff", REPO, "--file", file.to_str().unwrap()]);
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
        isolation.command(&["diff", REPO, "--file", file.to_str().unwrap()]);
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
    let output = run(&mut isolation.command(&[
        "diff",
        REPO,
        "--file",
        file.to_str().unwrap(),
    ]));
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
        REPO,
        "--file",
        isolation.config_dir.join("absent.json").to_str().unwrap(),
    ]));
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("could not read"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn proxy_environment_is_honored_by_default() {
    // The client must use the environment proxy (pointed at a dead
    // address), so the request fails instead of reaching the mock.
    let mock = mock_github(vec![
        Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
            .labels_page("[]", None),
    ]);
    let isolation = Isolation::new("proxy-env");
    let mut command = isolation.command(&["export", REPO]);
    command.env("HTTP_PROXY", "http://127.0.0.1:9");
    let output = run(against_mock(&mock, &mut command));
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(text.contains("could not reach GitHub"), "{text}");
}

#[test]
fn no_proxy_bypasses_the_environment_proxy() {
    // Same dead proxy in the environment, but --no-proxy must reach the
    // mock directly. Export, diff, and auth login all go through the
    // same client construction path.
    let isolation = Isolation::new("proxy-bypass");

    let mock = mock_github(vec![
        Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
            .labels_page("[]", None),
    ]);
    let mut command =
        isolation.command(&["export", REPO, "--file", "-", "--no-proxy"]);
    command.env("HTTP_PROXY", "http://127.0.0.1:9");
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));

    let file = canonical_file(
        &isolation,
        "labels.json",
        "[{\"name\": \"bug\", \"color\": \"d73a4a\", \"description\": \"\"}]",
    );
    let mock = mock_github(vec![
        Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
            .labels_page(&labels_json(&[("bug", "d73a4a", None)]), None),
    ]);
    let mut command = isolation.command(&[
        "diff",
        REPO,
        "--file",
        file.to_str().unwrap(),
        "--no-proxy",
    ]);
    command.env("HTTP_PROXY", "http://127.0.0.1:9");
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));

    let mock = mock_github(vec![
        Expectation::get("/user").body(r#"{"login":"seapagan"}"#),
    ]);
    let mut command =
        isolation.command(&["auth", "login", "--token-stdin", "--no-proxy"]);
    command.env("HTTP_PROXY", "http://127.0.0.1:9");
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
}

#[test]
fn export_writes_labels_json_in_the_working_directory_by_default() {
    let isolation = Isolation::new("export-default");
    let workdir = isolation.config_dir.clone();
    let mock = mock_github(vec![
        Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
            .labels_page(
                &labels_json(&[("bug", "d73a4a", Some("broken"))]),
                None,
            ),
    ]);
    let mut command = isolation.command(&["export", REPO]);
    command.current_dir(&workdir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert!(output.status.success());
    let written =
        std::fs::read_to_string(workdir.join("labels.json")).unwrap();
    assert!(written.contains("\"bug\""));
    assert!(written.ends_with('\n'));

    // Overwrite protection applies to the default file too.
    let mock = mock_github(vec![
        Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
            .labels_page("[]", None),
    ]);
    let mut command = isolation.command(&["export", REPO]);
    command.current_dir(&workdir);
    let output = run(against_mock(&mock, &mut command));
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("refusing to overwrite"));

    let mock = mock_github(vec![
        Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
            .labels_page("[]", None),
    ]);
    let mut command = isolation.command(&["export", REPO, "--force"]);
    command.current_dir(&workdir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert!(output.status.success());
}

#[test]
fn export_rejects_force_with_stdout_file() {
    let isolation = Isolation::new("export-stdout-force");
    let output =
        run(&mut isolation
            .command(&["export", REPO, "--file", "-", "--force"]));
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("cannot be combined"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn export_stdout_mode_is_never_blocked_by_an_existing_labels_json() {
    let isolation = Isolation::new("export-stdout-clean");
    std::fs::write(isolation.config_dir.join("labels.json"), "stale").unwrap();
    let mock = mock_github(vec![
        Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
            .labels_page(&labels_json(&[("bug", "d73a4a", None)]), None),
    ]);
    let mut command = isolation.command(&["export", REPO, "--file", "-"]);
    command.current_dir(&isolation.config_dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert!(output.status.success());
    assert!(stdout(&output).starts_with("[\n"));
}

#[test]
fn diff_reads_labels_json_from_the_working_directory_by_default() {
    let isolation = Isolation::new("diff-default");
    std::fs::write(
        isolation.config_dir.join("labels.json"),
        "[{\"name\": \"bug\", \"color\": \"d73a4a\", \"description\": \"\"}]",
    )
    .unwrap();
    let mock = mock_github(vec![
        Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
            .labels_page(&labels_json(&[("bug", "d73a4a", None)]), None),
    ]);
    let mut command = isolation.command(&["diff", REPO]);
    command.current_dir(&isolation.config_dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("UNCHANGED bug"));
}

#[test]
fn sync_reads_labels_json_from_the_working_directory_by_default() {
    let isolation = Isolation::new("sync-default");
    std::fs::write(
        isolation.config_dir.join("labels.json"),
        "[{\"name\": \"bug\", \"color\": \"d73a4a\", \"description\": \"\"}]",
    )
    .unwrap();
    let mock = mock_github(vec![
        Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
            .labels_page("[]", None),
        Expectation::post(&format!("/repos/{REPO}/labels")).status(201),
    ]);
    let mut command = isolation.command(&["sync", REPO]);
    command
        .current_dir(&isolation.config_dir)
        .env("LABELDECK_TOKEN", "gh_test_token");
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    assert!(stderr(&output).contains("Synchronized"));
}

#[test]
fn old_positional_file_syntax_is_rejected_cleanly() {
    let isolation = Isolation::new("old-syntax");
    for args in [
        vec!["diff", "labels.json", REPO],
        vec!["sync", "labels.json", REPO],
    ] {
        let output = run(&mut isolation.command(&args));
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let text = stderr(&output);
        assert!(
            text.contains("Usage:") || text.contains("unexpected argument"),
            "clean usage error expected: {text}"
        );
        assert!(!text.contains("panicked"), "{text}");
    }
}

#[test]
fn repository_argument_position_is_consistent_across_commands() {
    // The OWNER/REPO argument is first for export, diff, and sync, so
    // the same spec is accepted (and rejected) identically everywhere.
    let isolation = Isolation::new("repo-position");
    for command_name in ["export", "diff", "sync"] {
        let output =
            run(&mut isolation.command(&[command_name, "not-a-repo"]));
        assert_eq!(output.status.code(), Some(2), "{command_name}");
        assert!(
            stderr(&output).contains("OWNER/REPO"),
            "{command_name}: {}",
            stderr(&output)
        );
    }
}

// ---------------------------------------------------------------------
// Global default deck: read precedence and export --global.

const LOCAL_DECK: &str =
    "[{\"name\": \"bug\", \"color\": \"d73a4a\", \"description\": \"\"}]";
const GLOBAL_DECK: &str =
    "[{\"name\": \"docs\", \"color\": \"0075ca\", \"description\": \"\"}]";

/// A per-test working directory whose cleanup guard is retained for
/// the fixture's lifetime (RAII via `tempfile::TempDir`).
struct Workdir {
    _guard: tempfile::TempDir,
}

impl Workdir {
    fn new(_tag: &str) -> Self {
        Self {
            _guard: tempfile::tempdir().expect("temp dir"),
        }
    }

    fn path(&self) -> &std::path::Path {
        self._guard.path()
    }

    fn join(&self, name: &str) -> std::path::PathBuf {
        self._guard.path().join(name)
    }
}

impl AsRef<std::path::Path> for Workdir {
    fn as_ref(&self) -> &std::path::Path {
        self.path()
    }
}

impl std::ops::Deref for Workdir {
    type Target = std::path::Path;

    fn deref(&self) -> &Self::Target {
        self.path()
    }
}

fn workdir(tag: &str) -> Workdir {
    Workdir::new(tag)
}

fn remote_page(labels: &[(&str, &str, Option<&str>)]) -> Vec<Expectation> {
    vec![
        Expectation::get(&format!("/repos/{REPO}/labels?per_page=100"))
            .labels_page(&labels_json(labels), None),
    ]
}

#[test]
fn diff_prefers_local_deck_when_local_and_global_exist() {
    let isolation = Isolation::new("precedence-local");
    std::fs::write(isolation.config_dir.join("labels.json"), GLOBAL_DECK)
        .unwrap();
    let dir = workdir("precedence-local");
    std::fs::write(dir.join("labels.json"), LOCAL_DECK).unwrap();

    // Remote exactly matches the LOCAL deck: local winning means exit 0.
    let mock = mock_github(remote_page(&[("bug", "d73a4a", None)]));
    let mut command = isolation.command(&["diff", REPO]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(text.contains("UNCHANGED bug"), "{text}");
    assert!(
        !text.contains("docs"),
        "global deck must not be used: {text}"
    );
}

#[test]
fn diff_and_sync_fall_back_to_global_deck_when_local_absent() {
    let isolation = Isolation::new("precedence-global");
    std::fs::write(isolation.config_dir.join("labels.json"), GLOBAL_DECK)
        .unwrap();
    let dir = workdir("precedence-global");

    // Remote matches the GLOBAL deck: falling back means exit 0.
    let mock = mock_github(remote_page(&[("docs", "0075ca", None)]));
    let mut command = isolation.command(&["diff", REPO]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("UNCHANGED docs"));

    let mock = mock_github(remote_page(&[("docs", "0075ca", None)]));
    let mut command = isolation.command(&["sync", REPO, "--dry-run"]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("UNCHANGED docs"));
}

#[test]
fn explicit_file_beats_local_and_global_decks() {
    let isolation = Isolation::new("precedence-explicit");
    std::fs::write(isolation.config_dir.join("labels.json"), GLOBAL_DECK)
        .unwrap();
    let dir = workdir("precedence-explicit");
    std::fs::write(dir.join("labels.json"), LOCAL_DECK).unwrap();
    let explicit = dir.join("chosen.json");
    std::fs::write(
        &explicit,
        "[{\"name\": \"feature\", \"color\": \"a2eeef\", \"description\": \"\"}]",
    )
    .unwrap();

    let mock = mock_github(remote_page(&[]));
    let mut command = isolation.command(&[
        "diff",
        REPO,
        "--file",
        explicit.to_str().unwrap(),
    ]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(1));
    let text = stdout(&output);
    assert!(text.contains("CREATE feature"), "{text}");
    assert!(!text.contains("bug") && !text.contains("docs"), "{text}");
}

#[test]
fn explicit_missing_file_fails_without_fallback() {
    let isolation = Isolation::new("explicit-missing");
    std::fs::write(isolation.config_dir.join("labels.json"), GLOBAL_DECK)
        .unwrap();
    let dir = workdir("explicit-missing");
    let absent = dir.join("absent.json");

    let mock = mock_github(remote_page(&[("docs", "0075ca", None)]));
    let mut command =
        isolation.command(&["diff", REPO, "--file", absent.to_str().unwrap()]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    // The mock expectation is intentionally left unconsumed: no read of
    // the repository may happen once the explicit file is missing.
    let text = stderr(&output);
    assert_eq!(output.status.code(), Some(2));
    assert!(text.contains("absent.json"), "{text}");
}

#[test]
fn explicit_malformed_file_fails_without_fallback() {
    let isolation = Isolation::new("explicit-bad");
    std::fs::write(isolation.config_dir.join("labels.json"), GLOBAL_DECK)
        .unwrap();
    let dir = workdir("explicit-bad");
    let broken = dir.join("broken.json");
    std::fs::write(&broken, "{not json").unwrap();

    let mock = mock_github(remote_page(&[("docs", "0075ca", None)]));
    let mut command =
        isolation.command(&["diff", REPO, "--file", broken.to_str().unwrap()]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(text.contains("broken.json"), "{text}");
    assert!(text.contains("invalid JSON"), "{text}");
}

#[test]
fn local_malformed_deck_fails_without_global_fallback() {
    let isolation = Isolation::new("local-bad");
    std::fs::write(isolation.config_dir.join("labels.json"), GLOBAL_DECK)
        .unwrap();
    let dir = workdir("local-bad");
    std::fs::write(dir.join("labels.json"), "{not json").unwrap();

    let mock = mock_github(remote_page(&[("docs", "0075ca", None)]));
    let mut command = isolation.command(&["diff", REPO]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(text.contains("invalid JSON"), "{text}");
    assert!(text.contains("labels.json"), "{text}");
}

#[test]
fn local_non_file_deck_fails_without_global_fallback() {
    let isolation = Isolation::new("local-non-file");
    std::fs::write(isolation.config_dir.join("labels.json"), GLOBAL_DECK)
        .unwrap();
    let dir = workdir("local-non-file");
    // A directory named `labels.json` exists, so the resolver selects the
    // local entry; reading it as a canonical deck then fails on every
    // platform and privilege level (root included).
    std::fs::create_dir(dir.join("labels.json")).unwrap();

    let mock = mock_github(remote_page(&[("docs", "0075ca", None)]));
    let mut command = isolation.command(&["diff", REPO]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(text.contains("could not read"), "{text}");
    assert!(text.contains("labels.json"), "{text}");
    assert!(
        !text.contains(isolation.config_dir.to_string_lossy().as_ref()),
        "must fail on the local deck, not the global one: {text}"
    );
    let out = stdout(&output);
    assert!(!out.contains("docs"), "global deck must not be used: {out}");
}

#[test]
fn missing_local_and_global_decks_lists_both_locations() {
    let isolation = Isolation::new("no-decks");
    let dir = workdir("no-decks");

    let mock = mock_github(remote_page(&[]));
    let mut command = isolation.command(&["diff", REPO]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(text.contains("no canonical label file found"), "{text}");
    assert!(text.contains("labels.json"), "{text}");
    assert!(
        text.contains(isolation.config_dir.to_string_lossy().as_ref()),
        "must name the global location: {text}"
    );
    assert!(text.contains("--file"), "{text}");
    assert!(text.contains("--global"), "{text}");
}

#[test]
fn export_global_writes_the_config_directory_deck() {
    let mock = mock_github(remote_page(&[("bug", "d73a4a", Some("broken"))]));
    let isolation = Isolation::new("export-global");
    let dir = workdir("export-global");

    let mut command = isolation.command(&["export", REPO, "--global"]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert!(output.status.success());
    let global = isolation.config_dir.join("labels.json");
    let written = std::fs::read_to_string(&global).unwrap();
    assert!(written.contains("\"bug\""));
    assert!(
        !dir.join("labels.json").exists(),
        "local deck must be untouched"
    );

    // Overwrite protection applies to the global deck too.
    let mock = mock_github(remote_page(&[]));
    let mut command = isolation.command(&["export", REPO, "--global"]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("refusing to overwrite"));

    // --global --force deliberately replaces it.
    let mock = mock_github(remote_page(&[]));
    let mut command =
        isolation.command(&["export", REPO, "--global", "--force"]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert!(output.status.success());
    assert_eq!(std::fs::read_to_string(&global).unwrap(), "[]\n");
}

#[test]
fn export_global_conflicts_with_file() {
    let isolation = Isolation::new("export-global-conflict");
    let output = run(&mut isolation.command(&[
        "export",
        REPO,
        "--global",
        "--file",
        "other.json",
    ]));
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("cannot be used with"));
}

#[test]
fn plain_export_never_modifies_an_existing_global_deck() {
    let mock = mock_github(remote_page(&[("bug", "d73a4a", None)]));
    let isolation = Isolation::new("export-local-only");
    let global = isolation.config_dir.join("labels.json");
    std::fs::write(&global, "sentinel").unwrap();
    let dir = workdir("export-local-only");

    let mut command = isolation.command(&["export", REPO]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert!(output.status.success());
    assert!(dir.join("labels.json").exists());
    assert_eq!(
        std::fs::read_to_string(&global).unwrap(),
        "sentinel",
        "plain export must never touch the global deck"
    );
}

// ---------------------------------------------------------------------
// sync --dry-run follow-up command suggestion.

fn dry_run_list() -> Vec<Expectation> {
    remote_page(&[("docs", "0075ca", None)])
}

#[test]
fn dry_run_suggestion_pins_the_selected_local_deck() {
    let isolation = Isolation::new("suggest-local");
    let dir = workdir("suggest-local");
    std::fs::write(dir.join("labels.json"), GLOBAL_DECK).unwrap();
    let mock = mock_github(dry_run_list());
    let mut command =
        isolation.command(&["sync", REPO, "--dry-run", "--prune"]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    let text = stderr(&output);
    // The automatically selected local deck is pinned explicitly, so
    // the rerun cannot re-resolve precedence.
    assert!(
        text.contains(
            "Run again without --dry-run to apply: labeldeck sync \
             octocat/hello-world --file labels.json --prune"
        ),
        "{text}"
    );
}

#[test]
fn dry_run_suggestion_pins_the_resolved_global_path() {
    let isolation = Isolation::new("suggest-global");
    let global = isolation.config_dir.join("labels.json");
    std::fs::write(&global, GLOBAL_DECK).unwrap();
    let dir = workdir("suggest-global"); // no local deck

    let mock = mock_github(dry_run_list());
    let mut command = isolation.command(&["sync", REPO, "--dry-run"]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    let text = stderr(&output);
    // The resolved global deck is pinned exactly. Whether it renders
    // as a bare argument or the structured form depends on the
    // platform's temporary-directory path; both must carry the exact
    // path so the rerun cannot re-resolve precedence.
    let command_form = format!("--file {} --no-prune", global.display());
    let structured_form = format!("file:       {:?}", global);
    assert!(
        text.contains(&command_form) || text.contains(&structured_form),
        "guidance must pin the exact global deck path: {text}"
    );
}

#[test]
fn global_backed_dry_run_guidance_stays_pinned_when_local_deck_appears() {
    let isolation = Isolation::new("suggest-global-race");
    let global = isolation.config_dir.join("labels.json");
    std::fs::write(&global, GLOBAL_DECK).unwrap();
    let dir = workdir("suggest-global-race"); // no local deck yet

    let mock = mock_github(dry_run_list());
    let mut command = isolation.command(&["sync", REPO, "--dry-run", "--prune"]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));

    // A local deck appears between the dry run and the rerun.
    std::fs::write(dir.join("labels.json"), LOCAL_DECK).unwrap();

    // Following the pinned guidance (--file <the global deck used by
    // the dry run, here in dry-run form to stay read-only) still
    // reads the global deck: the remote matches it exactly, so the
    // local deck's "bug" label must never surface in the plan.
    let mock = mock_github(dry_run_list());
    let mut command = isolation.command(&[
        "sync",
        REPO,
        "--file",
        global.to_str().unwrap(),
        "--dry-run",
        "--prune",
    ]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(!text.contains("bug"), "local deck leaked in: {text}");
    assert!(!text.contains("CREATE"), "plan must be empty: {text}");
}

#[test]
fn dry_run_suggestion_repeats_explicit_file() {
    let isolation = Isolation::new("suggest-explicit");
    let dir = workdir("suggest-explicit");
    std::fs::write(dir.join("labels.json"), GLOBAL_DECK).unwrap();
    std::fs::write(isolation.config_dir.join("labels.json"), GLOBAL_DECK)
        .unwrap();
    std::fs::write(dir.join("custom.json"), LOCAL_DECK).unwrap();

    let mock = mock_github(dry_run_list());
    let mut command = isolation.command(&[
        "sync",
        REPO,
        "--file",
        "custom.json",
        "--dry-run",
        "--prune",
    ]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    let text = stderr(&output);
    assert!(
        text.contains(
            "Run again without --dry-run to apply: labeldeck sync \
             octocat/hello-world --file custom.json --prune"
        ),
        "{text}"
    );
}

#[cfg(unix)]
#[test]
fn dangling_local_deck_symlink_does_not_fall_back_to_global() {
    use std::os::unix::fs::symlink;
    let isolation = Isolation::new("dangling-local");
    // A perfectly valid global deck exists...
    std::fs::write(isolation.config_dir.join("labels.json"), GLOBAL_DECK)
        .unwrap();
    let dir = workdir("dangling-local");
    // ...but the local entry is an authoritative (if dangling) symlink.
    symlink("/definitely/not/here", dir.join("labels.json")).unwrap();

    // The remote matches the global deck, so a silent fallback would
    // exit 0; the local deck's authority must instead surface the read
    // error for the local path.
    let mock = mock_github(remote_page(&[("docs", "0075ca", None)]));
    let mut command = isolation.command(&["diff", REPO]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(
        text.contains("could not read labels.json"),
        "error must refer to the local deck: {text}"
    );
}

#[test]
fn dry_run_suggestion_renders_awkward_paths_safely() {
    let isolation = Isolation::new("suggest-awkward");
    let dir = workdir("suggest-awkward");
    let awkward = dir.join("my labels.json");
    std::fs::write(&awkward, GLOBAL_DECK).unwrap();

    let mock = mock_github(dry_run_list());
    let mut command = isolation.command(&[
        "sync",
        REPO,
        "--file",
        awkward.to_str().unwrap(),
        "--dry-run",
        "--prune",
    ]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert_eq!(output.status.code(), Some(0));
    let text = stderr(&output);
    // The path itself is not plain, so no command form may be shown...
    assert!(
        !text.contains("labeldeck sync"),
        "awkward path must not be rendered as a command: {text}"
    );
    // ...but the exact path and settings must be.
    assert!(text.contains("file:       "), "{text}");
    assert!(text.contains("my labels.json"), "{text}");
    assert!(text.contains("repository: octocat/hello-world"), "{text}");
    assert!(text.contains("pruning:    enabled"), "{text}");
}

#[cfg(unix)]
#[test]
fn global_export_repairing_directory_permissions_never_truncates() {
    use std::os::unix::fs::PermissionsExt;
    let isolation = Isolation::new("export-global-repair");
    let global = isolation.config_dir.join("labels.json");
    std::fs::write(&global, LOCAL_DECK).unwrap();
    let dir = workdir("export-global-repair");

    // An unsearchable configuration directory makes naive existence
    // checks report false; the atomic destination-open must still
    // protect the existing deck after the directory is repaired.
    std::fs::set_permissions(
        &isolation.config_dir,
        std::fs::Permissions::from_mode(0o000),
    )
    .unwrap();

    let mock = mock_github(remote_page(&[("bug", "d73a4a", None)]));
    let mut command = isolation.command(&["export", REPO, "--global"]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("refusing to overwrite"));

    // The directory was repaired, but the deck is byte-for-byte intact.
    assert_eq!(
        std::fs::read_to_string(&global).unwrap(),
        LOCAL_DECK,
        "the existing global deck must not be truncated or replaced"
    );

    // With --force the same repaired-directory run intentionally
    // replaces the deck.
    let mock = mock_github(remote_page(&[]));
    let mut command =
        isolation.command(&["export", REPO, "--global", "--force"]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    mock.assert_satisfied();
    assert!(output.status.success());
    assert_eq!(std::fs::read_to_string(&global).unwrap(), "[]\n");
}

#[test]
fn refused_overwrites_leave_contents_untouched() {
    // Local default and explicit --file variants of the same guarantee,
    // with content assertions (the global case is covered above).
    let isolation = Isolation::new("refused-contents");
    let dir = workdir("refused-contents");
    std::fs::write(dir.join("labels.json"), LOCAL_DECK).unwrap();

    let mock = mock_github(remote_page(&[("bug", "d73a4a", None)]));
    let mut command = isolation.command(&["export", REPO]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        std::fs::read_to_string(dir.join("labels.json")).unwrap(),
        LOCAL_DECK
    );

    let explicit = dir.join("chosen.json");
    std::fs::write(&explicit, GLOBAL_DECK).unwrap();
    let mock = mock_github(remote_page(&[("bug", "d73a4a", None)]));
    let mut command = isolation.command(&[
        "export",
        REPO,
        "--file",
        explicit.to_str().unwrap(),
    ]);
    command.current_dir(&dir);
    let output = run(against_mock(&mock, &mut command));
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(std::fs::read_to_string(&explicit).unwrap(), GLOBAL_DECK);

    // Both succeed and replace with --force.
    let mock = mock_github(remote_page(&[]));
    let mut command = isolation.command(&["export", REPO, "--force"]);
    command.current_dir(&dir);
    assert!(run(against_mock(&mock, &mut command)).status.success());
    assert_eq!(
        std::fs::read_to_string(dir.join("labels.json")).unwrap(),
        "[]\n"
    );

    let mock = mock_github(remote_page(&[]));
    let mut command = isolation.command(&[
        "export",
        REPO,
        "--file",
        explicit.to_str().unwrap(),
        "--force",
    ]);
    command.current_dir(&dir);
    assert!(run(against_mock(&mock, &mut command)).status.success());
    assert_eq!(std::fs::read_to_string(&explicit).unwrap(), "[]\n");
}
