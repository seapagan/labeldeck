//! Canonical label-deck path resolution.
//!
//! One place decides which `labels.json` a command reads or writes:
//!
//! ```text
//! read:   --file PATH → local labels.json → global labels.json
//! export: --file - | --file PATH | --global | local labels.json
//! ```
//!
//! The project-local deck wins over the global default; an explicit
//! `--file` is authoritative and never falls back; fallback to the
//! global deck happens only when the local default file is genuinely
//! absent (a present-but-invalid local deck is an error, not a reason
//! to silently switch decks).

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// The name of the canonical deck file in every location.
pub const DECK_FILE_NAME: &str = crate::cli::DEFAULT_LABELS_FILE;

/// Resolve the global deck path inside a configuration directory.
pub fn global_deck_path(config_dir: &Path) -> PathBuf {
    config_dir.join(DECK_FILE_NAME)
}

/// The local default deck in the current working directory.
pub fn local_deck_path() -> PathBuf {
    PathBuf::from(DECK_FILE_NAME)
}

/// Pure resolution core, parameterised by candidate paths so tests need
/// no working-directory or filesystem-global changes.
fn resolve_read(
    explicit: Option<&Path>,
    local: &Path,
    global: &Path,
) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    if local.exists() {
        return Ok(local.to_path_buf());
    }
    if global.exists() {
        return Ok(global.to_path_buf());
    }
    Err(Error::NoDeckFile {
        local: local.to_path_buf(),
        global: global.to_path_buf(),
    })
}

/// Resolve the deck a read command (`diff`/`sync`) should use.
///
/// Precedence: an explicit `--file` path verbatim; then the local
/// `./labels.json` if it exists; then the global deck if it exists.
/// When neither default exists the error names both checked locations
/// and how to fix the situation.
pub fn resolve_read_path(
    explicit: Option<&Path>,
    config_dir: &Path,
) -> Result<PathBuf> {
    resolve_read(explicit, &local_deck_path(), &global_deck_path(config_dir))
}

/// Resolve where `export` should write.
///
/// `--file -` (stdout) is handled by the caller before this runs.
/// `--global` targets the deck in the configuration directory; the
/// default is the local `./labels.json`. Overwrite protection applies
/// to whatever path comes back.
pub fn export_destination(
    explicit: Option<&Path>,
    global: bool,
    config_dir: &Path,
) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_path_buf();
    }
    if global {
        global_deck_path(config_dir)
    } else {
        local_deck_path()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sandbox {
        root: PathBuf,
    }

    impl Sandbox {
        fn new() -> Self {
            Self {
                root: tempfile::tempdir()
                    .expect("temp dir")
                    .keep()
                    .join("labeldeck-deck-test"),
            }
        }

        fn dir(&self, name: &str) -> PathBuf {
            let dir = self.root.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }

        fn deck(&self, name: &str) -> PathBuf {
            let path = self.dir(name).join(DECK_FILE_NAME);
            std::fs::write(
                &path,
                "[{\"name\": \"bug\", \"color\": \"d73a4a\", \
                  \"description\": \"\"}]",
            )
            .unwrap();
            path
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.root).ok();
        }
    }

    #[test]
    fn explicit_file_is_authoritative_without_fallback() {
        let sandbox = Sandbox::new();
        let global = sandbox.deck("global-home");
        let explicit = sandbox.dir("explicit").join("chosen.json");
        std::fs::write(&explicit, "[]").unwrap();
        // Even with the global default present, the explicit path wins —
        // including when it does not exist (the read error must name the
        // user's own path, not a silently swapped default).
        assert_eq!(
            resolve_read(Some(&explicit), &sandbox.deck("ignored"), &global)
                .unwrap(),
            explicit
        );
        let missing = sandbox.dir("explicit").join("absent.json");
        assert_eq!(
            resolve_read(Some(&missing), &sandbox.deck("ignored"), &global)
                .unwrap(),
            missing
        );
    }

    #[test]
    fn local_deck_wins_over_global_when_both_exist() {
        let sandbox = Sandbox::new();
        let local = sandbox.deck("workdir");
        let global = sandbox.deck("global-home");
        assert_eq!(resolve_read(None, &local, &global).unwrap(), local);
    }

    #[test]
    fn global_deck_used_when_local_absent() {
        let sandbox = Sandbox::new();
        let absent_local = sandbox.dir("empty-workdir").join(DECK_FILE_NAME);
        let global = sandbox.deck("global-home");
        assert_eq!(
            resolve_read(None, &absent_local, &global).unwrap(),
            global
        );
    }

    #[test]
    fn local_deck_wins_even_when_its_content_is_invalid() {
        // Existence, not validity, drives fallback: a present-but-bad
        // local deck must surface its own error later, never switch to
        // the global deck.
        let sandbox = Sandbox::new();
        let local = sandbox.dir("bad-workdir").join(DECK_FILE_NAME);
        std::fs::write(&local, "not json").unwrap();
        let global = sandbox.deck("global-home");
        assert_eq!(resolve_read(None, &local, &global).unwrap(), local);
    }

    #[test]
    fn missing_everywhere_names_both_locations() {
        let sandbox = Sandbox::new();
        let local = sandbox.dir("empty-workdir").join(DECK_FILE_NAME);
        let global = sandbox.dir("empty-global").join(DECK_FILE_NAME);
        let error = resolve_read(None, &local, &global).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("labels.json"), "{message}");
        assert!(
            message.contains(global.to_string_lossy().as_ref()),
            "must name the global location: {message}"
        );
        assert!(message.contains("export"), "{message}");
        assert!(message.contains("--file"), "{message}");
    }

    #[test]
    fn export_destinations_are_distinct() {
        let sandbox = Sandbox::new();
        let config_dir = sandbox.dir("config");
        let explicit = config_dir.join("other.json");
        assert_eq!(
            export_destination(Some(&explicit), false, &config_dir),
            explicit
        );
        assert_eq!(
            export_destination(Some(&explicit), true, &config_dir),
            explicit
        );
        assert_eq!(
            export_destination(None, true, &config_dir),
            global_deck_path(&config_dir)
        );
        assert_eq!(
            export_destination(None, false, &config_dir),
            local_deck_path()
        );
    }
}
