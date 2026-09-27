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
//! to silently switch decks, and neither is a failing existence probe).

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

/// The deck a read command selected, retaining *how* it was chosen so
/// callers can act on the distinction (e.g. dry-run suggestions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeckSelection {
    /// An explicit `--file` path, verbatim.
    Explicit(PathBuf),
    /// The local `./labels.json` default.
    Local(PathBuf),
    /// The global deck in the configuration directory.
    Global(PathBuf),
}

impl DeckSelection {
    /// The resolved deck path, regardless of how it was chosen.
    pub fn path(&self) -> &Path {
        match self {
            DeckSelection::Explicit(path)
            | DeckSelection::Local(path)
            | DeckSelection::Global(path) => path,
        }
    }
}

/// Resolution core, parameterised by candidate paths and an existence
/// probe so tests can exercise every probe outcome (including I/O
/// failures) without filesystem tricks.
fn resolve_read_with(
    explicit: Option<&Path>,
    local: &Path,
    global: &Path,
    mut probe: impl FnMut(&Path) -> std::io::Result<bool>,
) -> Result<DeckSelection> {
    if let Some(path) = explicit {
        // Explicit paths are selected verbatim; whether they are
        // readable/valid is the read step's business, not selection's.
        return Ok(DeckSelection::Explicit(path.to_path_buf()));
    }
    // A probe that errors (permissions on a parent directory, I/O
    // failure, ...) must be reported, never treated as "absent".
    if probe(local).map_err(|e| probe_error(local, e))? {
        return Ok(DeckSelection::Local(local.to_path_buf()));
    }
    if probe(global).map_err(|e| probe_error(global, e))? {
        return Ok(DeckSelection::Global(global.to_path_buf()));
    }
    Err(Error::NoDeckFile {
        local: local.to_path_buf(),
        global: global.to_path_buf(),
    })
}

fn probe_error(path: &Path, error: std::io::Error) -> Error {
    Error::Io {
        context: format!(
            "could not examine {} while looking for the canonical label \
             file",
            path.display()
        ),
        message: error.to_string(),
    }
}

/// The production existence probe: examine the directory entry itself,
/// without following symlinks.
///
/// A filesystem entry named `labels.json` — including a symlink whose
/// target is missing — makes the local deck authoritative; whether it
/// can then be opened/read/parsed is the read step's concern, never a
/// reason to fall back to the global deck. Only a confirmed `NotFound`
/// counts as absence; every other metadata error is a probe error.
pub fn probe_entry(path: &Path) -> std::io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

/// Resolve the deck a read command (`diff`/`sync`) should use, and how
/// it was chosen.
///
/// Precedence: an explicit `--file` path verbatim; then the local
/// `./labels.json` if a filesystem entry with that name exists; then
/// the global deck likewise. Fallback happens only on a *confirmed*
/// absence: an existence probe that fails with an I/O error is
/// reported for the path probed. When neither default exists the error
/// names both checked locations and how to fix the situation.
pub fn resolve_read_selection(
    explicit: Option<&Path>,
    config_dir: &Path,
) -> Result<DeckSelection> {
    resolve_read_with(
        explicit,
        &local_deck_path(),
        &global_deck_path(config_dir),
        probe_entry,
    )
}

/// Resolve the deck a read command should use (path only).
pub fn resolve_read_path(
    explicit: Option<&Path>,
    config_dir: &Path,
) -> Result<PathBuf> {
    Ok(resolve_read_selection(explicit, config_dir)?
        .path()
        .to_path_buf())
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
        // The TempDir guard cleans up on drop; the root lives beneath it.
        _guard: tempfile::TempDir,
        root: PathBuf,
    }

    impl Sandbox {
        fn new() -> Self {
            let guard = tempfile::tempdir().expect("temp dir");
            let root = guard.path().join("labeldeck-deck-test");
            std::fs::create_dir_all(&root).unwrap();
            Self {
                _guard: guard,
                root,
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

    /// Injected probes let every outcome — including probe failures —
    /// be tested deterministically. Probe results are described as
    /// `Ok(bool)` or an `ErrorKind` (io::Error is not Clone).
    fn probe_with(
        local_result: Result<bool, std::io::ErrorKind>,
        global_result: Result<bool, std::io::ErrorKind>,
    ) -> impl Fn(Option<&Path>, &Path, &Path) -> Result<DeckSelection> {
        move |explicit: Option<&Path>, local: &Path, global: &Path| {
            let mut probes = 0;
            resolve_read_with(explicit, local, global, move |path| {
                probes += 1;
                let outcome = if path == local {
                    local_result
                } else {
                    assert_eq!(probes, 2, "global probed before local");
                    global_result
                };
                outcome.map_err(std::io::Error::from)
            })
        }
    }

    #[test]
    fn explicit_file_is_authoritative_without_probing_defaults() {
        let sandbox = Sandbox::new();
        let global = sandbox.deck("global-home");
        let explicit = sandbox.dir("explicit").join("chosen.json");
        std::fs::write(&explicit, "[]").unwrap();
        // The probe always errors: explicit selection must never touch
        // either default, and must not validate the path either.
        let never = Err(std::io::ErrorKind::PermissionDenied);
        let resolver = probe_with(never, never);
        assert_eq!(
            resolver(Some(&explicit), &sandbox.deck("ignored"), &global)
                .unwrap(),
            DeckSelection::Explicit(explicit)
        );
        let missing = sandbox.dir("explicit").join("absent.json");
        assert_eq!(
            resolver(Some(&missing), &sandbox.deck("ignored"), &global)
                .unwrap(),
            DeckSelection::Explicit(missing)
        );
    }

    #[test]
    fn local_probe_true_selects_local_without_probing_global() {
        let sandbox = Sandbox::new();
        let local = sandbox.deck("workdir");
        let global = sandbox.deck("global-home");
        let resolver =
            probe_with(Ok(true), Err(std::io::ErrorKind::PermissionDenied));
        assert_eq!(
            resolver(None, &local, &global).unwrap(),
            DeckSelection::Local(local)
        );
    }

    #[test]
    fn local_probe_false_global_probe_true_selects_global() {
        let sandbox = Sandbox::new();
        let local = sandbox.dir("empty-workdir").join(DECK_FILE_NAME);
        let global = sandbox.deck("global-home");
        let resolver = probe_with(Ok(false), Ok(true));
        assert_eq!(
            resolver(None, &local, &global).unwrap(),
            DeckSelection::Global(global)
        );
    }

    #[test]
    fn both_probes_false_names_both_locations() {
        let sandbox = Sandbox::new();
        let local = sandbox.dir("empty-workdir").join(DECK_FILE_NAME);
        let global = sandbox.dir("empty-global").join(DECK_FILE_NAME);
        let error = probe_with(Ok(false), Ok(false))(None, &local, &global)
            .unwrap_err();
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
    fn local_probe_error_is_reported_without_fallback() {
        let sandbox = Sandbox::new();
        let local = sandbox.dir("workdir").join(DECK_FILE_NAME);
        let global = sandbox.deck("global-home");
        let error = probe_with(
            Err(std::io::ErrorKind::PermissionDenied),
            Ok(true),
        )(None, &local, &global)
        .unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("could not examine"),
            "probe failure must be reported: {message}"
        );
        assert!(
            message.contains(local.to_string_lossy().as_ref()),
            "must name the local path: {message}"
        );
        assert!(
            !message.contains(global.to_string_lossy().as_ref()),
            "must not implicate the global path: {message}"
        );
    }

    #[test]
    fn global_probe_error_is_reported() {
        let sandbox = Sandbox::new();
        let local = sandbox.dir("empty-workdir").join(DECK_FILE_NAME);
        let global = sandbox.dir("global-home").join(DECK_FILE_NAME);
        let error = probe_with(
            Ok(false),
            Err(std::io::ErrorKind::PermissionDenied),
        )(None, &local, &global)
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("could not examine"), "{message}");
        assert!(
            message.contains(global.to_string_lossy().as_ref()),
            "must name the global path: {message}"
        );
    }

    // ----- filesystem-backed behaviour (real try_exists) -------------

    #[test]
    fn local_deck_wins_over_global_when_both_exist() {
        let sandbox = Sandbox::new();
        let local = sandbox.deck("workdir");
        let global = sandbox.deck("global-home");
        let explicit = None;
        assert_eq!(
            resolve_read_with(explicit, &local, &global, probe_entry).unwrap(),
            DeckSelection::Local(local)
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
        assert_eq!(
            resolve_read_with(None, &local, &global, probe_entry).unwrap(),
            DeckSelection::Local(local)
        );
    }

    #[test]
    fn global_deck_used_when_local_absent_on_disk() {
        let sandbox = Sandbox::new();
        let absent_local = sandbox.dir("empty-workdir").join(DECK_FILE_NAME);
        let global = sandbox.deck("global-home");
        assert_eq!(
            resolve_read_with(None, &absent_local, &global, probe_entry,)
                .unwrap(),
            DeckSelection::Global(global)
        );
    }

    // ----- production probe (symlink_metadata semantics) ------------

    #[test]
    fn probe_entry_reports_regular_files_as_present() {
        let sandbox = Sandbox::new();
        let path = sandbox.deck("probe");
        assert!(probe_entry(&path).unwrap());
    }

    #[test]
    fn probe_entry_reports_missing_paths_as_absent() {
        let sandbox = Sandbox::new();
        let path = sandbox.dir("empty").join(DECK_FILE_NAME);
        assert!(!probe_entry(&path).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn probe_entry_counts_dangling_symlinks_as_present() {
        let sandbox = Sandbox::new();
        let dir = sandbox.dir("dangling");
        let link = dir.join(DECK_FILE_NAME);
        std::os::unix::fs::symlink("/definitely/not/here", &link).unwrap();
        // The directory entry exists even though the target never will.
        assert!(probe_entry(&link).unwrap());
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
