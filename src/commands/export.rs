//! `labeldeck export`.

use std::io::Write;
use std::path::Path;

use crate::cli::STDOUT_FILE;
use crate::commands::{
    github_client, remote_labels, repo_spec, resolve_token,
};
use crate::error::{Error, Result};

pub fn run(
    repo: &str,
    file: Option<&std::path::PathBuf>,
    force: bool,
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

    let path: &std::path::Path = file
        .map(std::path::PathBuf::as_path)
        .unwrap_or_else(|| Path::new(crate::cli::DEFAULT_LABELS_FILE));
    if path.exists() && !force {
        return Err(Error::OutputExists {
            path: path.to_path_buf(),
        });
    }
    std::fs::write(path, json.as_bytes()).map_err(|e| Error::Io {
        context: format!("could not write {}", path.display()),
        message: e.to_string(),
    })?;
    eprintln!(
        "Exported {} labels from {}/{} to {}.",
        labels.len(),
        repo.owner,
        repo.name,
        path.display()
    );
    Ok(0)
}
