use super::*;

#[test]
fn keys_wait_for_markers_even_across_reads() {
    let mut script = Script::default();
    let mut keys = Vec::new();
    let steps = [("first marker", "a"), ("second marker", "b")];
    script.observe(b"first mar", &steps, &mut keys).unwrap();
    assert!(keys.is_empty());
    script
        .observe(b"ker second marker", &steps, &mut keys)
        .unwrap();
    assert_eq!(keys, b"a");
    // The next step must be observed after the previous keys were sent.
    script
        .observe(b"unrelated output", &steps, &mut keys)
        .unwrap();
    assert_eq!(keys, b"a");
    script.observe(b"second marker", &steps, &mut keys).unwrap();
    assert_eq!(keys, b"ab");
}

#[test]
fn cursor_queries_are_answered_once_even_across_reads() {
    let mut script = Script::default();
    let mut responses = Vec::new();
    script.observe(b"\x1b[6", &[], &mut responses).unwrap();
    assert!(responses.is_empty());
    script.observe(b"n\x1b[6n", &[], &mut responses).unwrap();
    assert_eq!(responses, b"\x1b[1;1R\x1b[1;1R");
    script
        .observe(b"ordinary output", &[], &mut responses)
        .unwrap();
    assert_eq!(responses, b"\x1b[1;1R\x1b[1;1R");
}

#[test]
fn builder_preserves_the_isolated_command() {
    let isolation = Isolation::new("pty-builder");
    let template = isolation.command(&["edit", "--file", "deck.json"]);
    let command = command(&isolation, &["edit", "--file", "deck.json"], "api");
    let argv: Vec<_> = std::iter::once(template.get_program().to_owned())
        .chain(template.get_args().map(std::ffi::OsStr::to_owned))
        .collect();
    assert_eq!(command.get_argv(), &argv);
    assert_eq!(
        command.get_cwd().map(std::ffi::OsString::as_os_str),
        Some(isolation.config_dir.as_os_str())
    );
    assert_eq!(
        command.get_env("LABELDECK_CONFIG_DIR"),
        Some(isolation.config_dir.as_os_str())
    );
    for name in [
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "HTTP_PROXY",
        "http_proxy",
        "NO_PROXY",
        "no_proxy",
    ] {
        assert_eq!(command.get_env(name), None);
    }
    for (name, value) in [
        ("LABELDECK_API", "api"),
        ("LABELDECK_TOKEN", "test-token"),
        ("NO_COLOR", "1"),
        ("TERM", "xterm"),
    ] {
        assert_eq!(command.get_env(name), Some(std::ffi::OsStr::new(value)));
    }
}

#[test]
fn eof_does_not_allow_an_unreached_step() {
    let isolation = Isolation::new("pty-unreached");
    let mut session =
        Session::open(command(&isolation, &["--help"], "api")).unwrap();
    let mut transcript = Vec::new();
    let error = session
        .drive(
            &[("impossible marker", "x")],
            Instant::now() + TIMEOUT,
            &mut transcript,
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unreached terminal step: impossible marker")
    );
    assert!(!transcript.is_empty());
    assert!(session.exited);
}

#[test]
fn nonzero_child_exit_is_rejected() {
    let isolation = Isolation::new("pty-nonzero");
    let mut session =
        Session::open(command(&isolation, &["--invalid-option"], "api"))
            .unwrap();
    let mut transcript = Vec::new();
    let error = session
        .drive(&[], Instant::now() + TIMEOUT, &mut transcript)
        .unwrap_err();
    assert!(error.to_string().contains("child exit status"));
    assert!(!transcript.is_empty());
    assert!(session.exited);
}

#[test]
fn successful_exit_without_alternate_screen_restoration_is_rejected() {
    let isolation = Isolation::new("pty-restoration");
    let mut session =
        Session::open(command(&isolation, &["--help"], "api")).unwrap();
    let mut transcript = Vec::new();
    let error = session
        .drive(&[], Instant::now() + TIMEOUT, &mut transcript)
        .unwrap_err();
    assert_eq!(error.to_string(), "alternate screen was not restored");
    assert!(session.exited);
}

#[test]
fn expired_deadline_terminates_and_reaps_the_child() {
    let isolation = Isolation::new("pty-timeout");
    std::fs::write(isolation.config_dir.join("deck.json"), "[]").unwrap();
    let mut session = Session::open(command(
        &isolation,
        &["edit", "--file", "deck.json"],
        "api",
    ))
    .unwrap();
    assert!(session.child.try_wait().unwrap().is_none());
    let error = session
        .drive(
            &[("impossible marker", "x")],
            Instant::now(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    // Drop kills, waits, closes the PTY and joins the reader before returning.
    drop(session);
}
