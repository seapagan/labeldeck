//! `labeldeck sync` — apply a canonical label set to a repository.

use crate::commands::diff;
use crate::commands::{
    config_dir, credentials_for_write, github_client, read_canonical,
    remote_labels, repo_spec, resolve_token,
};
use crate::error::Result;
use crate::github::RepoSpec;
use crate::plan;
use crate::sync as sync_engine;

pub fn run(
    repo: &str,
    file: Option<&std::path::PathBuf>,
    cli_prune: Option<bool>,
    dry_run: bool,
    no_proxy: bool,
) -> Result<i32> {
    let repo = repo_spec(repo)?;
    let config_dir = config_dir()?;
    let selection = crate::deck::resolve_read_selection(
        file.map(std::path::PathBuf::as_path),
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
        let result = plan::plan(&canonical, &remote, prune);
        eprintln!("Dry run: no changes were made.");
        diff::print_plan(&result);
        eprintln!("{}", diff::summarize(&result));
        eprintln!("{}", follow_up_command(&repo, file, prune));
        return Ok(0);
    }

    let stdin = std::io::stdin();
    let mut locked = stdin.lock();
    let credentials =
        credentials_for_write(&config_dir, &mut locked, no_proxy)?;
    let client = github_client(Some(&credentials), no_proxy);

    let remote = remote_labels(&client, &repo)?;
    let result = plan::plan(&canonical, &remote, prune);

    if prune && !result.deletes.is_empty() {
        eprintln!(
            "warning: pruning will DELETE {} target-only label(s); \
             deleting a label removes it from existing issues and pull \
             requests.",
            result.deletes.len()
        );
    }

    let mut printer = StderrReporter;
    let outcome = sync_engine::execute(&client, &repo, &result, &mut printer);

    if outcome.is_success() {
        eprintln!(
            "Synchronized {}/{}: {} created, {} updated, {} deleted.",
            repo.owner,
            repo.name,
            result.creates.len(),
            result.updates.len(),
            result.deletes.len(),
        );
        return Ok(0);
    }

    let failure = outcome.failure.as_ref().expect("failure implies Some");
    for applied in &outcome.applied {
        eprintln!("applied: {applied}");
    }
    eprintln!("failed:   {}", failure.operation);
    eprintln!("reason:   {}", failure.error);
    for skipped in &outcome.skipped {
        eprintln!("skipped:  {skipped}");
    }
    eprintln!(
        "GitHub does not support transactional label updates; the \
         operations listed as applied remain in effect."
    );
    Ok(2)
}

struct StderrReporter;

impl sync_engine::Reporter for StderrReporter {
    fn operation(&mut self, description: &str) {
        eprintln!("labeldeck: {description}...");
    }
}

/// Whether a path can appear as a bare argument in a suggested command
/// without any risk of changing argument boundaries or meaning.
///
/// Conservatively allowlisted: plain ASCII letters, digits, `.`, `_`,
/// `-`, and `/` (a path separator on every supported platform), and it
/// must not be empty or begin with `-` (which would read as a flag).
/// Anything else — spaces, quotes, backslashes, shell
/// metacharacters, non-ASCII — is rendered in the structured form
/// instead, because quoting rules differ between POSIX shells,
/// PowerShell, and CMD and no single quoted form is safe everywhere.
fn is_plain_path_token(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('-')
        && path.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'.' | b'_' | b'-' | b'/')
        })
}

/// The "run again" guidance shown after `--dry-run`, preserving how the
/// deck was selected.
///
/// Automatically selected decks (local default or global fallback)
/// print the clean default command without exposing the resolved
/// global path. An explicit `--file` prints the same concise form only
/// when the path is provably safe as a bare argument; otherwise the
/// guidance switches to a structured, unambiguous form rather than
/// displaying a command that would split or change meaning when
/// copy/pasted into any shell.
fn follow_up_command(
    repo: &RepoSpec,
    file: Option<&std::path::PathBuf>,
    prune: bool,
) -> String {
    let prune_flag = if prune { "--prune" } else { "--no-prune" };
    match file {
        None => format!(
            "Run again without --dry-run to apply: labeldeck sync {}/{} \
             {prune_flag}",
            repo.owner, repo.name
        ),
        Some(path) => {
            // Lossy text decides *safety* (non-UTF-8 paths contain
            // replacement characters and are never plain tokens), but
            // the structured form renders the path with Debug-style
            // escaping so newlines, tabs, control characters, and
            // non-UTF-8 bytes stay visible as escapes instead of
            // visually restructuring the diagnostic.
            if is_plain_path_token(&path.to_string_lossy()) {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_selection_suggests_the_clean_default_form() {
        let repo = RepoSpec::parse("octocat/hello-world").unwrap();
        assert_eq!(
            follow_up_command(&repo, None, true),
            "Run again without --dry-run to apply: \
             labeldeck sync octocat/hello-world --prune"
        );
        assert_eq!(
            follow_up_command(&repo, None, false),
            "Run again without --dry-run to apply: \
             labeldeck sync octocat/hello-world --no-prune"
        );
    }

    #[test]
    fn awkward_paths_never_render_a_command_form() {
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
            let path = std::path::PathBuf::from(awkward);
            let guidance = follow_up_command(&repo, Some(&path), true);
            assert!(
                !guidance.contains("labeldeck sync"),
                "{awkward:?} must not be shown as a copy/paste command: \
                 {guidance}"
            );
            // The diagnostic form is the path's own Debug rendering, so
            // it stays identifiable while every control character and
            // backslash remains visibly escaped.
            assert!(
                guidance.contains(&format!("{:?}", path)),
                "{awkward:?} must appear in escaped debug form: {guidance}"
            );
            assert!(guidance.contains("repository: octocat/hello-world"));
            assert!(guidance.contains("file:"));
            assert!(guidance.contains("pruning:    enabled"));
        }
    }

    #[test]
    fn embedded_newlines_and_tabs_do_not_restructure_the_guidance() {
        let repo = RepoSpec::parse("octocat/hello-world").unwrap();
        for awkward in ["we\nird.json", "ta\tb.json", "con\x07trol.json"] {
            let path = std::path::PathBuf::from(awkward);
            let guidance = follow_up_command(&repo, Some(&path), false);
            // The file field stays one field on one line, ending in the
            // escaped debug form, and no raw control character leaks.
            let file_line = guidance
                .split('\n')
                .find(|line| line.starts_with("  file:"))
                .expect("file field");
            assert_eq!(
                file_line,
                format!("  file:       {:?}", path),
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
        let guidance = follow_up_command(&repo, Some(&path), true);
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

    #[test]
    fn plain_explicit_paths_keep_the_command_form() {
        let repo = RepoSpec::parse("octocat/hello-world").unwrap();
        for plain in ["labels-alt.json", "decks/main.json", "v1.2-beta_labels"]
        {
            let path = std::path::PathBuf::from(plain);
            let guidance = follow_up_command(&repo, Some(&path), false);
            assert!(
                guidance.contains(&format!("--file {plain} --no-prune")),
                "{plain}: {guidance}"
            );
        }
    }

    #[test]
    fn explicit_selection_repeats_the_file_argument() {
        let repo = RepoSpec::parse("octocat/hello-world").unwrap();
        let path = std::path::PathBuf::from("custom.json");
        assert_eq!(
            follow_up_command(&repo, Some(&path), true),
            "Run again without --dry-run to apply: labeldeck sync \
             octocat/hello-world --file custom.json --prune"
        );
        assert_eq!(
            follow_up_command(&repo, Some(&path), false),
            "Run again without --dry-run to apply: labeldeck sync \
             octocat/hello-world --file custom.json --no-prune"
        );
    }
}
