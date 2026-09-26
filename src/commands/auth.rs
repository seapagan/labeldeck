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
fn validate_token(
    config_dir: &Path,
    token: &str,
    no_proxy: bool,
) -> Result<String> {
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
    validate_token(config_dir, token, no_proxy)?;
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

    validate_token(config_dir, token, no_proxy)?;

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

    if store {
        store_validated_token(config_dir, token)?;
    } else {
        println!(
            "Token will be used for this run only; nothing was written \
             to disk."
        );
    }

    Ok(ResolvedToken {
        token: Arc::from(token),
        source: TokenSource::Stored(auth::token_path(config_dir)),
    })
}
