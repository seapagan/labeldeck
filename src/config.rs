//! User configuration: `config.toml` in the labeldeck configuration
//! directory.
//!
//! The configuration is deliberately tiny — `prune` is the only key — and
//! unknown keys are rejected so typos fail loudly instead of silently
//! changing behaviour.

use std::fmt::Write as _;
use std::path::PathBuf;

use serde::Deserialize;

/// Environment variable that overrides the configuration directory.
/// Primarily a test seam; also useful for isolated automation.
pub const CONFIG_DIR_ENV: &str = "LABELDECK_CONFIG_DIR";

/// Failures while loading configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// No platform configuration directory could be determined.
    #[error(
        "could not determine a configuration directory for this \
         platform; set {CONFIG_DIR_ENV}"
    )]
    NoConfigDir,
    /// The configuration file exists but could not be parsed.
    #[error("invalid configuration file {path}: {message}")]
    Invalid { path: PathBuf, message: String },
    /// The configuration file could not be read.
    #[error("could not read configuration file {path}: {message}")]
    Unreadable { path: PathBuf, message: String },
}

/// The user configuration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Delete target-only labels during `sync`. Built-in default: false.
    #[serde(default)]
    pub prune: bool,
}

/// Resolve the labeldeck configuration directory.
///
/// Precedence: `LABELDECK_CONFIG_DIR`, then the platform configuration
/// directory (`~/.config/labeldeck` on Linux,
/// `~/Library/Application Support/labeldeck` on macOS,
/// `%APPDATA%\labeldeck` on Windows).
pub fn config_dir(
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<PathBuf, ConfigError> {
    if let Some(dir) = env(CONFIG_DIR_ENV)
        && !dir.trim().is_empty()
    {
        return Ok(PathBuf::from(dir));
    }
    dirs::config_dir()
        .map(|base| base.join("labeldeck"))
        .ok_or(ConfigError::NoConfigDir)
}

/// The path of the configuration file within a configuration directory.
pub fn config_path(dir: &std::path::Path) -> PathBuf {
    dir.join("config.toml")
}

/// Load the configuration from a directory.
///
/// A missing file is not an error: the built-in defaults apply. A present
/// but malformed file is an error with the path and a precise reason.
pub fn load(dir: &std::path::Path) -> Result<Config, ConfigError> {
    let path = config_path(dir);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Config::default());
        }
        Err(e) => {
            return Err(ConfigError::Unreadable {
                path,
                message: e.to_string(),
            });
        }
    };
    toml::from_str(&text).map_err(|e| ConfigError::Invalid {
        path,
        message: e.to_string(),
    })
}

/// Render the prune setting given the CLI override and configuration,
/// applying the documented precedence:
/// explicit CLI flag → configuration file → built-in default (false).
pub fn effective_prune(cli_override: Option<bool>, config: &Config) -> bool {
    cli_override.unwrap_or(config.prune)
}

/// One-line diagnostic for reporting which prune setting applies.
pub fn describe_prune(prune: bool) -> String {
    let mut message = String::new();
    if prune {
        let _ = write!(
            message,
            "pruning enabled: target-only labels will be deleted \
             (removing them from issues and pull requests)"
        );
    } else {
        let _ = write!(
            message,
            "pruning disabled: target-only labels are retained"
        );
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn temp_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "labeldeck-config-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn missing_config_file_yields_defaults() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(load(&dir).unwrap(), Config { prune: false });
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn prune_true_is_parsed() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(config_path(&dir), "prune = true\n").unwrap();
        assert_eq!(load(&dir).unwrap(), Config { prune: true });
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn malformed_toml_reports_path_and_reason() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(config_path(&dir), "prune = \n").unwrap();
        let error = load(&dir).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("invalid configuration file"), "{message}");
        assert!(message.contains("config.toml"), "{message}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn wrong_type_for_prune_is_rejected() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(config_path(&dir), "prune = \"yes\"\n").unwrap();
        let error = load(&dir).unwrap_err();
        assert!(error.to_string().contains("prune"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(config_path(&dir), "prue = true\n").unwrap();
        let error = load(&dir).unwrap_err();
        assert!(error.to_string().contains("unknown field"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn config_dir_env_override_wins() {
        let dir = temp_dir();
        let resolved = config_dir(&|key: &str| {
            if key == CONFIG_DIR_ENV {
                Some(dir.to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .unwrap();
        assert_eq!(resolved, dir);
    }

    #[test]
    fn config_dir_env_blank_falls_back_to_platform() {
        let resolved = config_dir(&|_: &str| Some("  ".to_string())).unwrap();
        assert!(resolved.ends_with("labeldeck"), "{resolved:?}");
    }

    #[test]
    fn config_dir_without_env_uses_platform_directory() {
        let resolved = config_dir(&no_env).unwrap();
        assert!(resolved.ends_with("labeldeck"), "{resolved:?}");
    }

    #[test]
    fn cli_override_beats_config_beats_default() {
        let configured = Config { prune: true };
        assert!(!effective_prune(Some(false), &configured));
        assert!(!effective_prune(Some(false), &Config::default()));
        assert!(effective_prune(None, &configured));
        assert!(!effective_prune(None, &Config::default()));
        // `--prune` with no config file.
        assert!(effective_prune(Some(true), &Config::default()));
    }

    #[test]
    fn prune_descriptions_are_distinct_and_actionable() {
        assert!(describe_prune(true).contains("deleted"));
        assert!(describe_prune(false).contains("retained"));
    }
}
