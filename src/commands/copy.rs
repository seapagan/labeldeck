//! `labeldeck copy` — apply one repository's labels to another.
//!
//! The source repository's current labels become the desired label set
//! for the target, which is then reconciled through the same shared
//! machinery as `sync`. The source is only ever read.

use crate::commands::apply;
use crate::commands::{
    config_dir, credentials_for_write, github_client, repo_spec, resolve_token,
};
use crate::error::{Error, Result};
use crate::github::{GitHubClient, RepoSpec};
use crate::labels::Label;

pub fn run(
    source: &str,
    target: &str,
    cli_prune: Option<bool>,
    dry_run: bool,
    no_proxy: bool,
) -> Result<i32> {
    let source = repo_spec(source)?;
    let target = repo_spec(target)?;
    if is_same_repository(&source, &target) {
        return Err(Error::Usage(format!(
            "source and target are the same repository ({}/{}); \
             `labeldeck copy` needs two different repositories",
            source.owner, source.name
        )));
    }

    let config_dir = config_dir()?;
    let config = crate::config::load(&config_dir)?;
    let prune = crate::config::effective_prune(cli_prune, &config);

    let client = client_for(&config_dir, dry_run, no_proxy)?;

    // The full desired set is read from the source, and the target's
    // current labels are read, before anything is planned or mutated:
    // a failure on either fetch leaves the target untouched.
    let desired = fetch_labels(&client, "source", &source)?;
    let remote = fetch_labels(&client, "target", &target)?;

    if dry_run {
        return apply::dry_run(
            &desired,
            &remote,
            prune,
            &follow_up_command(&source, &target, prune),
        );
    }
    apply::execute(
        &client,
        &target,
        &desired,
        &remote,
        prune,
        &format!(
            "Copied labels from {}/{} to {}/{}",
            source.owner, source.name, target.owner, target.name
        ),
    )
}

/// Build the client a copy runs with.
///
/// Dry runs only read, so they use any already-available token (which
/// still grants read access to private repositories) and never demand
/// write credentials; a real copy goes through the interactive
/// first-use login flow when no token exists.
fn client_for(
    config_dir: &std::path::Path,
    dry_run: bool,
    no_proxy: bool,
) -> Result<GitHubClient> {
    if dry_run {
        Ok(github_client(resolve_token(config_dir).as_ref(), no_proxy))
    } else {
        let stdin = std::io::stdin();
        let mut locked = stdin.lock();
        let credentials =
            credentials_for_write(config_dir, &mut locked, no_proxy)?;
        Ok(github_client(Some(&credentials), no_proxy))
    }
}

/// Whether two parsed specifications denote the same repository.
///
/// GitHub resolves owner and repository names case-insensitively, so a
/// difference in letter case must not turn a copy onto itself into two
/// reads followed by a report of no changes.
fn is_same_repository(source: &RepoSpec, target: &RepoSpec) -> bool {
    source.owner.eq_ignore_ascii_case(&target.owner)
        && source.name.eq_ignore_ascii_case(&target.name)
}

/// Fetch a repository's labels, attributing any failure to the
/// repository's role in the copy so diagnostics say which side failed.
fn fetch_labels(
    client: &GitHubClient,
    role: &'static str,
    repo: &RepoSpec,
) -> Result<Vec<Label>> {
    client
        .list_labels(repo)
        .map_err(|error| Error::LabelsFetch {
            role,
            repo: format!("{}/{}", repo.owner, repo.name),
            source: error,
        })
}

/// The "run again" guidance shown after `--dry-run`, repeating the same
/// two repositories.
///
/// A repository specification prints as a bare argument only when it is
/// provably safe as one (the same conservative policy as `sync`'s
/// guidance); anything else is rendered in a structured, unambiguous
/// form rather than displaying a command that would split or change
/// meaning when copy/pasted into any shell.
fn follow_up_command(
    source: &RepoSpec,
    target: &RepoSpec,
    prune: bool,
) -> String {
    let source = format!("{}/{}", source.owner, source.name);
    let target = format!("{}/{}", target.owner, target.name);
    let prune_flag = if prune { "--prune" } else { "--no-prune" };
    if super::is_plain_argument_token(&source)
        && super::is_plain_argument_token(&target)
    {
        format!(
            "Run again without --dry-run to apply: labeldeck copy \
             {source} {target} {prune_flag}"
        )
    } else {
        format!(
            "Run again without --dry-run, using:\n  source repository: \
             {source:?}\n  target repository: {target:?}\n  pruning:           \
             {}",
            if prune { "enabled" } else { "disabled" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_repository_is_detected_case_insensitively() {
        let parse = |spec: &str| RepoSpec::parse(spec).unwrap();
        assert!(is_same_repository(
            &parse("octocat/hello-world"),
            &parse("octocat/hello-world"),
        ));
        assert!(is_same_repository(
            &parse("Octocat/Hello-World"),
            &parse("octocat/hello-world"),
        ));
        assert!(!is_same_repository(
            &parse("octocat/hello-world"),
            &parse("octocat/goodbye-world"),
        ));
        assert!(!is_same_repository(
            &parse("octocat/hello-world"),
            &parse("someone-else/hello-world"),
        ));
    }

    #[test]
    fn follow_up_repeats_both_repositories() {
        let source = RepoSpec::parse("seapagan/template").unwrap();
        let target = RepoSpec::parse("some-org/project").unwrap();
        assert_eq!(
            follow_up_command(&source, &target, true),
            "Run again without --dry-run to apply: labeldeck copy \
             seapagan/template some-org/project --prune"
        );
        assert_eq!(
            follow_up_command(&source, &target, false),
            "Run again without --dry-run to apply: labeldeck copy \
             seapagan/template some-org/project --no-prune"
        );
    }

    #[test]
    fn awkward_specifications_never_render_a_command_form() {
        let source = RepoSpec::parse("octocat/hello-world").unwrap();
        for awkward in ["ow$ner", "ow;ner", "a&b", "café"] {
            let target = RepoSpec::parse(&format!("{awkward}/repo"))
                .expect("spec parses");
            let guidance = follow_up_command(&source, &target, true);
            assert!(
                !guidance.contains("labeldeck copy"),
                "{awkward:?} must not be shown as a copy/paste command: \
                 {guidance}"
            );
            assert!(
                guidance.contains(&format!("{:?}", format!("{awkward}/repo"))),
                "{awkward:?} must appear in escaped debug form: {guidance}"
            );
        }
    }
}
