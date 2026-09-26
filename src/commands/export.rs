//! `labeldeck export`.

use std::io::Write;

use crate::commands::{
    github_client, remote_labels, repo_spec, resolve_token,
};
use crate::error::{Error, Result};

pub fn run(
    repo: &str,
    file: &Option<std::path::PathBuf>,
    force: bool,
    no_proxy: bool,
) -> Result<i32> {
    let repo = repo_spec(repo)?;
    let config_dir = crate::commands::config_dir()?;
    let client = github_client(resolve_token(&config_dir).as_ref(), no_proxy);

    let mut labels = remote_labels(&client, &repo)?;
    let json = crate::canonical::to_json(&mut labels);

    match file {
        None => {
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
        }
        Some(path) => {
            if path.exists() && !force {
                return Err(Error::OutputExists { path: path.clone() });
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
        }
    }
    Ok(0)
}
