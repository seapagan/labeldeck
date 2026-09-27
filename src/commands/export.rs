//! `labeldeck export`.

use std::io::Write;

use crate::cli::STDOUT_FILE;
use crate::commands::{
    github_client, remote_labels, repo_spec, resolve_token,
};
use crate::deck;
use crate::error::{Error, Result};

pub fn run(
    repo: &str,
    file: Option<&std::path::PathBuf>,
    force: bool,
    global: bool,
    no_proxy: bool,
) -> Result<i32> {
    // `--file -` writes canonical JSON to standard output; combining it
    // with --force is rejected: stdout is never "overwritten".
    let stdout_mode = file
        .as_ref()
        .is_some_and(|path| path.as_os_str() == STDOUT_FILE);
    if stdout_mode && force {
        return Err(Error::Usage(
            "--force cannot be combined with --file -: standard output is \
             never overwrite-protected"
                .to_string(),
        ));
    }

    let repo = repo_spec(repo)?;
    let config_dir = crate::commands::config_dir()?;
    let client = github_client(resolve_token(&config_dir).as_ref(), no_proxy);

    let mut labels = remote_labels(&client, &repo)?;
    let json = crate::canonical::to_json(&mut labels);

    if stdout_mode {
        // Pure JSON on stdout: safe to pipe into other tools.
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        handle.write_all(json.as_bytes()).map_err(|e| Error::Io {
            context: "could not write to standard output".to_string(),
            message: e.to_string(),
        })?;
        handle.flush().map_err(|e| Error::Io {
            context: "could not write to standard output".to_string(),
            message: e.to_string(),
        })?;
        return Ok(0);
    }

    // For a global export, ensure/repair the configuration directory
    // BEFORE touching the deck: a previously unsearchable directory can
    // make a pre-check lie about whether the global deck exists.
    if global {
        crate::auth::ensure_config_dir(&config_dir).map_err(|e| {
            Error::Io {
                context: format!(
                    "could not create or secure configuration directory {}",
                    config_dir.display()
                ),
                message: e.to_string(),
            }
        })?;
    }

    let path = deck::export_destination(
        file.map(std::path::PathBuf::as_path),
        global,
        &config_dir,
    );
    let outcome =
        crate::staged_write::write_deck(&path, json.as_bytes(), force)?;
    eprintln!(
        "Exported {} labels from {}/{} to {}.",
        labels.len(),
        repo.owner,
        repo.name,
        path.display()
    );
    if let crate::staged_write::WriteOutcome::DurabilityUnconfirmed(error) =
        outcome
    {
        eprintln!("{}", durability_warning(&error));
    }
    Ok(0)
}

/// The warning shown when the deck was installed at the commit point
/// but the post-commit destination-directory sync failed. The export
/// itself succeeded; durability is merely unconfirmed.
fn durability_warning(error: &std::io::Error) -> String {
    format!(
        "warning: the deck was installed successfully, but filesystem \
         durability could not be confirmed because the destination \
         directory could not be synchronized: {error}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durability_warning_names_the_cause_and_keeps_success_clear() {
        let warning = durability_warning(&std::io::Error::other(
            "injected sync failure",
        ));
        assert!(
            warning
                .starts_with("warning: the deck was installed successfully"),
            "{warning}"
        );
        assert!(warning.contains("could not be synchronized"), "{warning}");
        assert!(warning.ends_with(": injected sync failure"), "{warning}");
    }
}
