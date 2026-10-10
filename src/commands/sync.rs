//! `labeldeck sync` — apply a canonical label set to a repository.

use crate::commands::apply;
use crate::commands::{
    config_dir, credentials_for_write, github_client, read_canonical,
    remote_labels, repo_spec, resolve_token,
};
use crate::error::Result;
use crate::github::RepoSpec;

pub fn run(
    repo: &str,
    file: Option<&std::path::PathBuf>,
    global: bool,
    cli_prune: Option<bool>,
    dry_run: bool,
    no_proxy: bool,
) -> Result<i32> {
    let repo = repo_spec(repo)?;
    let config_dir = config_dir()?;
    let selection = crate::deck::resolve_read_selection(
        file.map(std::path::PathBuf::as_path),
        global,
        &config_dir,
    )?;
    let canonical = read_canonical(selection.path())?;
    let config = crate::config::load(&config_dir)?;
    let prune = crate::config::effective_prune(cli_prune, &config);

    if dry_run {
        // Dry runs only read; they never need credentials.
        let client =
            github_client(resolve_token(&config_dir).as_ref(), no_proxy);
        let remote = remote_labels(&client, &repo)?;
        return apply::dry_run(
            &canonical,
            &remote,
            prune,
            &follow_up_command(&repo, &selection, prune),
        );
    }

    let stdin = std::io::stdin();
    let mut locked = stdin.lock();
    let credentials =
        credentials_for_write(&config_dir, &mut locked, no_proxy)?;
    let client = github_client(Some(&credentials), no_proxy);

    let remote = remote_labels(&client, &repo)?;
    apply::execute(
        &client,
        &repo,
        &canonical,
        &remote,
        prune,
        &format!("Synchronized {}/{}", repo.owner, repo.name),
    )
}

/// Select a normal reconciliation plan; working edits never implicitly rewrite the source deck.
pub fn run_interactive(
    repo: &str,
    file: Option<&std::path::PathBuf>,
    global: bool,
    cli_prune: Option<bool>,
    dry_run: bool,
    no_proxy: bool,
) -> Result<i32> {
    if dry_run {
        return Err(crate::error::Error::Usage(
            "--interactive conflicts with --dry-run".into(),
        ));
    }
    crate::edit::session::require_terminal("sync --interactive")?;
    let repo = repo_spec(repo)?;
    let config_dir = config_dir()?;
    let selection = crate::deck::resolve_read_selection(
        file.map(std::path::PathBuf::as_path),
        global,
        &config_dir,
    )?;
    let canonical = read_canonical(selection.path())?;
    let config = crate::config::load(&config_dir)?;
    let prune = crate::config::effective_prune(cli_prune, &config);
    let credentials = credentials_for_write(
        &config_dir,
        &mut std::io::stdin().lock(),
        no_proxy,
    )?;
    let client = github_client(Some(&credentials), no_proxy);
    let context = super::interactive::ReconcileContext {
        session: crate::edit::ui::SessionKind::Sync,
        prune,
        title: format!("{}/{}", repo.owner, repo.name),
        success_line: format!("Synchronized {}/{}", repo.owner, repo.name),
        config_dir,
    };
    super::interactive::reconcile_with(
        &client,
        &repo,
        canonical,
        context,
        crate::edit::session::run,
    )
}

/// The "run again" guidance shown after `--dry-run`, pinned to the
/// exact deck the dry run used.
///
/// The rerun repeats the selected deck as an explicit `--file`
/// argument so it can never re-resolve local/global precedence, which
/// may have changed since the dry run read its deck: a local
/// `labels.json` appearing after a global-backed dry run must not
/// silently change which deck gets applied. All three selection kinds
/// ([`DeckSelection::Explicit`], [`DeckSelection::Local`],
/// [`DeckSelection::Global`]) therefore pin their exact resolved path.
///
/// A pinned path prints as a bare `--file` argument only when it is
/// provably safe as such; otherwise the guidance switches to a
/// structured, unambiguous form rather than displaying a command that
/// would split or change meaning when copy/pasted into any shell.
fn follow_up_command(
    repo: &RepoSpec,
    selection: &crate::deck::DeckSelection,
    prune: bool,
) -> String {
    let path = selection.path();
    let prune_flag = if prune { "--prune" } else { "--no-prune" };
    // Lossy text decides *safety* (non-UTF-8 paths contain
    // replacement characters and are never plain tokens), but
    // the structured form renders the path with Debug-style
    // escaping so newlines, tabs, control characters, and
    // non-UTF-8 bytes stay visible as escapes instead of
    // visually restructuring the diagnostic.
    if super::is_plain_argument_token(&path.to_string_lossy()) {
        format!(
            "Run again without --dry-run to apply: labeldeck sync \
             {}/{} --file {} {prune_flag}",
            repo.owner,
            repo.name,
            path.display()
        )
    } else {
        format!(
            "Run again without --dry-run, using:\n  repository: \
             {}/{}\n  file:       {:?}\n  pruning:    {}",
            repo.owner,
            repo.name,
            path,
            if prune { "enabled" } else { "disabled" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deck::DeckSelection;

    #[test]
    fn local_selection_pins_the_local_deck() {
        let repo = RepoSpec::parse("octocat/hello-world").unwrap();
        let selection =
            DeckSelection::Local(std::path::PathBuf::from("labels.json"));
        assert_eq!(
            follow_up_command(&repo, &selection, true),
            "Run again without --dry-run to apply: labeldeck sync \
             octocat/hello-world --file labels.json --prune"
        );
        assert_eq!(
            follow_up_command(&repo, &selection, false),
            "Run again without --dry-run to apply: labeldeck sync \
             octocat/hello-world --file labels.json --no-prune"
        );
    }

    #[test]
    fn global_selection_pins_the_resolved_global_path() {
        let repo = RepoSpec::parse("octocat/hello-world").unwrap();
        let selection = DeckSelection::Global(std::path::PathBuf::from(
            "/home/me/.config/labeldeck/labels.json",
        ));
        assert_eq!(
            follow_up_command(&repo, &selection, true),
            "Run again without --dry-run to apply: labeldeck sync \
             octocat/hello-world --file \
             /home/me/.config/labeldeck/labels.json --prune"
        );
        assert_eq!(
            follow_up_command(&repo, &selection, false),
            "Run again without --dry-run to apply: labeldeck sync \
             octocat/hello-world --file \
             /home/me/.config/labeldeck/labels.json --no-prune"
        );
    }

    #[test]
    fn awkward_selected_paths_never_render_a_command_form() {
        let repo = RepoSpec::parse("octocat/hello-world").unwrap();
        for awkward in [
            "my labels.json",
            "it's.json",
            "say \"hi\".json",
            "C:\\Users\\me\\labels.json",
            "a;b&c.json",
            "$HOME/labels.json",
            "-flag-like.json",
            "labels*.json",
            "we\nird.json",
            "ta\tb.json",
            "con\x07trol.json",
            "café.json",
        ] {
            // Explicit and globally selected decks alike must render
            // the selected path safely.
            for selection in [
                DeckSelection::Explicit(std::path::PathBuf::from(awkward)),
                DeckSelection::Global(std::path::PathBuf::from(awkward)),
            ] {
                let guidance = follow_up_command(&repo, &selection, true);
                assert!(
                    !guidance.contains("labeldeck sync"),
                    "{awkward:?} ({selection:?}) must not be shown as a \
                     copy/paste command: {guidance}"
                );
                // The diagnostic form is the path's own Debug
                // rendering, so it stays identifiable while every
                // control character and backslash remains visibly
                // escaped.
                assert!(
                    guidance.contains(&format!("{:?}", selection.path())),
                    "{awkward:?} must appear in escaped debug form: {guidance}"
                );
                assert!(guidance.contains("repository: octocat/hello-world"));
                assert!(guidance.contains("file:"));
                assert!(guidance.contains("pruning:    enabled"));
            }
        }
    }

    #[test]
    fn embedded_newlines_and_tabs_do_not_restructure_the_guidance() {
        let repo = RepoSpec::parse("octocat/hello-world").unwrap();
        for awkward in ["we\nird.json", "ta\tb.json", "con\x07trol.json"] {
            let selection =
                DeckSelection::Explicit(std::path::PathBuf::from(awkward));
            let guidance = follow_up_command(&repo, &selection, false);
            // The file field stays one field on one line, ending in
            // the escaped debug form, and no raw control character
            // leaks.
            let file_line = guidance
                .split('\n')
                .find(|line| line.starts_with("  file:"))
                .expect("file field");
            assert_eq!(
                file_line,
                format!("  file:       {:?}", selection.path()),
                "{awkward:?}"
            );
            assert!(guidance.contains("pruning:    disabled"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_render_as_escaped_bytes() {
        use std::os::unix::ffi::OsStrExt;
        let repo = RepoSpec::parse("octocat/hello-world").unwrap();
        let path = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(
            b"bad\xff.json",
        ));
        for selection in [
            DeckSelection::Explicit(path.clone()),
            DeckSelection::Global(path),
        ] {
            let guidance = follow_up_command(&repo, &selection, true);
            assert!(!guidance.contains("labeldeck sync"));
            // The invalid byte is shown as an escape, never silently
            // rendered as a lossy replacement character.
            assert!(
                guidance.contains("bad\\xFF.json")
                    || guidance.contains("bad\\xff.json"),
                "non-UTF-8 byte must be escaped: {guidance}"
            );
            assert!(!guidance.contains('\u{FFFD}'), "{guidance}");
        }
    }

    #[test]
    fn plain_explicit_paths_keep_the_command_form() {
        let repo = RepoSpec::parse("octocat/hello-world").unwrap();
        for plain in ["labels-alt.json", "decks/main.json", "v1.2-beta_labels"]
        {
            let selection =
                DeckSelection::Explicit(std::path::PathBuf::from(plain));
            let guidance = follow_up_command(&repo, &selection, false);
            assert!(
                guidance.contains(&format!("--file {plain} --no-prune")),
                "{plain}: {guidance}"
            );
        }
    }

    #[test]
    fn explicit_selection_repeats_the_file_argument() {
        let repo = RepoSpec::parse("octocat/hello-world").unwrap();
        let selection =
            DeckSelection::Explicit(std::path::PathBuf::from("custom.json"));
        assert_eq!(
            follow_up_command(&repo, &selection, true),
            "Run again without --dry-run to apply: labeldeck sync \
             octocat/hello-world --file custom.json --prune"
        );
        assert_eq!(
            follow_up_command(&repo, &selection, false),
            "Run again without --dry-run to apply: labeldeck sync \
             octocat/hello-world --file custom.json --no-prune"
        );
    }
}
