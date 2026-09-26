//! `labeldeck auth` — login, logout, status.

use std::io::{BufRead, IsTerminal, Write as _};
use std::path::Path;
use std::sync::Arc;

use crate::auth::{self, ResolvedToken, TokenSource};
use crate::commands::{API_BASE_ENV, config_dir, github_client};
use crate::error::{Error, Result};

pub fn login(token_stdin: bool, no_proxy: bool) -> Result<i32> {
    let config_dir = config_dir()?;
    let stdin = std::io::stdin();
    let mut locked = stdin.lock();

    if token_stdin {
        // Script/CI path: one line from standard input, no prompts.
        let mut line = String::new();
        locked.read_line(&mut line).map_err(|e| {
            Error::Auth(format!(
                "could not read the token from standard input: {e}"
            ))
        })?;
        let token = line.trim();
        if token.is_empty() {
            return Err(Error::Auth(
                "no token was provided on standard input".to_string(),
            ));
        }
        validate_and_store(&config_dir, token, no_proxy)?;
        return Ok(0);
    }

    if !locked.is_terminal() {
        return Err(Error::Auth(
            "auth login needs an interactive terminal for a hidden \
             prompt; use `labeldeck auth login --token-stdin` in scripts \
             or set LABELDECK_TOKEN, GH_TOKEN, or GITHUB_TOKEN"
                .to_string(),
        ));
    }

    let token = rpassword::prompt_password(
        "GitHub token (input hidden; paste and press Enter): ",
    )
    .map_err(|e| Error::Auth(format!("could not read the token: {e}")))?;
    let token = token.trim();
    if token.is_empty() {
        return Err(Error::Auth("no token was entered".to_string()));
    }

    validate_and_store(&config_dir, token, no_proxy)?;
    println!(
        "The token is stored; `labeldeck auth logout` removes it. \
         Tokens from LABELDECK_TOKEN, GH_TOKEN, or GITHUB_TOKEN still \
         take precedence over the stored token."
    );
    Ok(0)
}

/// Validate a token against the API and report the matching user.
fn validate_token(token: &str, no_proxy: bool) -> Result<String> {
    let client = github_client(
        Some(&ResolvedToken {
            token: Arc::from(token),
            source: TokenSource::Environment("LABELDECK_TOKEN"),
        }),
        no_proxy,
    );
    let login = client.authenticated_login().map_err(Error::Github)?;
    println!("Token valid for GitHub user {login}.");
    Ok(login)
}

/// Persist an already-validated token.
fn store_validated_token(config_dir: &Path, token: &str) -> Result<()> {
    auth::store_token(config_dir, token).map_err(|e| Error::Io {
        context: "could not store the token".to_string(),
        message: e.to_string(),
    })?;
    println!("Stored at {}.", auth::token_path(config_dir).display());
    Ok(())
}

/// Validate a token against the API, then persist it (explicit login).
fn validate_and_store(
    config_dir: &Path,
    token: &str,
    no_proxy: bool,
) -> Result<()> {
    validate_token(token, no_proxy)?;
    store_validated_token(config_dir, token)
}

pub fn logout() -> Result<i32> {
    let config_dir = config_dir()?;
    match auth::remove_stored_token(&config_dir) {
        Ok(true) => {
            println!(
                "Removed the stored token ({}).",
                auth::token_path(&config_dir).display()
            );
            println!(
                "Tokens supplied through LABELDECK_TOKEN, GH_TOKEN, or \
                 GITHUB_TOKEN are unaffected."
            );
        }
        Ok(false) => {
            println!("No stored token to remove.");
        }
        Err(e) => {
            return Err(Error::Io {
                context: "could not remove the stored token".to_string(),
                message: e.to_string(),
            });
        }
    }
    Ok(0)
}

pub fn status() -> Result<i32> {
    let config_dir = config_dir()?;
    println!("Configuration directory: {}", config_dir.display());
    println!(
        "API endpoint: {}",
        std::env::var(API_BASE_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "https://api.github.com".to_string())
    );
    match crate::commands::resolve_token(&config_dir) {
        Some(resolved) => match &resolved.source {
            TokenSource::Environment(name) => {
                println!("Authentication: token from {name}");
            }
            TokenSource::Stored(path) => {
                println!("Authentication: stored token at {}", path.display());
            }
            TokenSource::Ephemeral => {
                println!(
                    "Authentication: token supplied interactively for \
                     this run only"
                );
            }
        },
        None => {
            println!(
                "Authentication: none (anonymous; public repositories, \
                 read-only)"
            );
        }
    }
    let config = crate::config::load(&config_dir)?;
    println!(
        "Prune default: {}",
        if config.prune { "enabled" } else { "disabled" }
    );
    Ok(0)
}

/// The interactive first-use login flow used by `sync`.
///
/// Prompts securely for a token, validates it against GitHub, then asks
/// whether to store it (defaulting to yes). Declining keeps the token
/// in memory for the current process only — nothing is written to disk.
/// Environment-provided tokens never enter this flow.
pub fn prompt_and_store_login(
    config_dir: &Path,
    stdin: &mut (impl BufRead + std::io::IsTerminal),
    no_proxy: bool,
) -> Result<ResolvedToken> {
    if !stdin.is_terminal() {
        return Err(Error::Auth(
            "authentication is required but no token is available".to_string(),
        ));
    }
    println!(
        "No GitHub token is configured. labeldeck needs one to change \
         labels."
    );
    let token = rpassword::prompt_password(
        "GitHub token (input hidden; paste and press Enter): ",
    )
    .map_err(|e| Error::Auth(format!("could not read the token: {e}")))?;
    let token = token.trim();
    if token.is_empty() {
        return Err(Error::Auth("no token was entered".to_string()));
    }

    let store = {
        let mut answer = String::new();
        loop {
            print!("Store this token for future use? [Y/n] ");
            let _ = std::io::stdout().flush();
            answer.clear();
            stdin.read_line(&mut answer).map_err(|e| {
                Error::Auth(format!("could not read the answer: {e}"))
            })?;
            match crate::auth::parse_store_answer(&answer) {
                Some(decision) => break decision,
                None => {
                    println!(
                        "Please answer y, yes, n, or no (Enter means yes)."
                    );
                }
            }
        }
    };

    complete_first_use(config_dir, token, store, |token| {
        validate_token(token, no_proxy)
    })
}

/// Finish a first-use login after the token and store decision exist:
/// validate, then persist (or deliberately not), returning the token
/// with accurate provenance.
///
/// `store = false` keeps the token in memory only and labels it
/// [`TokenSource::Ephemeral`]; `store = true` writes the token file and
/// labels it [`TokenSource::Stored`].
fn complete_first_use(
    config_dir: &Path,
    token: &str,
    store: bool,
    validate: impl FnOnce(&str) -> Result<String>,
) -> Result<ResolvedToken> {
    validate(token)?;
    let source = if store {
        store_validated_token(config_dir, token)?;
        TokenSource::Stored(auth::token_path(config_dir))
    } else {
        println!(
            "Token will be used for this run only; nothing was written \
             to disk."
        );
        TokenSource::Ephemeral
    };
    Ok(ResolvedToken {
        token: Arc::from(token),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config_dir() -> std::path::PathBuf {
        tempfile::tempdir()
            .expect("temp dir")
            .keep()
            .join("labeldeck")
    }

    fn ok_validate(_: &str) -> Result<String> {
        Ok("seapagan".to_string())
    }

    #[test]
    fn declined_persistence_is_ephemeral_and_writes_nothing() {
        let dir = temp_config_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let resolved = complete_first_use(
            &dir,
            "gh_interactive_token",
            false,
            ok_validate,
        )
        .unwrap();
        assert_eq!(&*resolved.token, "gh_interactive_token");
        assert_eq!(resolved.source, TokenSource::Ephemeral);
        assert!(
            !auth::token_path(&dir).exists(),
            "declined persistence must not create a token file"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn accepted_persistence_is_stored_on_disk() {
        let dir = temp_config_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let resolved = complete_first_use(
            &dir,
            "gh_interactive_token",
            true,
            ok_validate,
        )
        .unwrap();
        assert_eq!(
            resolved.source,
            TokenSource::Stored(auth::token_path(&dir))
        );
        assert_eq!(
            auth::read_stored_token(&dir).as_deref(),
            Some("gh_interactive_token")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn validation_failure_prevents_any_persistence() {
        fn reject(_: &str) -> Result<String> {
            Err(Error::Auth("invalid credentials".to_string()))
        }
        let dir = temp_config_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let error = complete_first_use(
            &dir,
            "gh_bad_token",
            true, // even with store accepted, nothing may be written
            reject,
        )
        .unwrap_err();
        assert!(error.to_string().contains("invalid credentials"));
        assert!(!auth::token_path(&dir).exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
