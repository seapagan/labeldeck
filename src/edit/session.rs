//! Filesystem host and exit records for the shared terminal workspace.
use super::{
    model::Document,
    ui::{self, UiAction, UiState},
};
use crate::{
    error::{Error, Result},
    staged_write::{self, WriteOutcome},
};
use std::{io, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveTarget {
    Local,
    Global,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveRecord {
    pub path: PathBuf,
    pub warning: Option<String>,
}

pub struct SessionResult {
    pub outcome: Result<UiAction>,
    pub saves: Vec<SaveRecord>,
}
impl SessionResult {
    /// Call after terminal restoration, before any fallible final action.
    pub fn report_saves(&self) {
        for save in &self.saves {
            eprintln!("Saved working deck to {}.", save.path.display());
            if let Some(warning) = &save.warning {
                eprintln!("{warning}");
            }
        }
    }
}

pub struct SaveHost {
    pub local: PathBuf,
    pub config_dir: PathBuf,
    pub protected: Option<PathBuf>,
}
impl SaveHost {
    pub fn new(config_dir: PathBuf, protected: Option<PathBuf>) -> Self {
        Self {
            local: crate::deck::local_deck_path(),
            config_dir,
            protected,
        }
    }
    fn path(&self, target: SaveTarget) -> PathBuf {
        match target {
            SaveTarget::Local => self.local.clone(),
            SaveTarget::Global => {
                crate::deck::global_deck_path(&self.config_dir)
            }
        }
    }
    fn allowed(&self, target: SaveTarget) -> io::Result<bool> {
        self.protected.as_ref().map_or(Ok(true), |protected| {
            super::destination::same_destination(&self.path(target), protected)
                .map(|same| !same)
        })
    }
    pub fn refresh(&self, state: &mut UiState) {
        let mut choices = [false; 2];
        let mut paths = [String::new(), String::new()];
        for (i, target) in [SaveTarget::Local, SaveTarget::Global]
            .into_iter()
            .enumerate()
        {
            match self.allowed(target) {
                Ok(allowed) => choices[i] = allowed,
                Err(error) => state.set_error(format!(
                    "Could not check Save destination: {error}"
                )),
            }
            let path = self.path(target);
            paths[i] = format!(
                "{}{}",
                path.display(),
                if path.exists() {
                    " (overwrite)"
                } else {
                    " (new file)"
                }
            );
        }
        state.set_save_choices(choices, paths);
    }
    pub fn save(
        &self,
        document: &Document,
        target: SaveTarget,
    ) -> Result<SaveRecord> {
        self.save_with(document, target, staged_write::write_deck)
    }
    fn save_with(
        &self,
        document: &Document,
        target: SaveTarget,
        write: impl FnOnce(&std::path::Path, &[u8], bool) -> Result<WriteOutcome>,
    ) -> Result<SaveRecord> {
        let mut labels = document.labels().map_err(Error::Usage)?;
        let path = self.path(target);
        self.require_allowed(target)?;
        if target == SaveTarget::Global {
            crate::auth::ensure_config_dir(&self.config_dir).map_err(|error| Error::Io { context:format!("could not create or secure configuration directory {}",self.config_dir.display()),message:error.to_string() })?;
        }
        self.require_allowed(target)?;
        let json = crate::canonical::to_json(&mut labels);
        let outcome = write(&path, json.as_bytes(), true)?;
        let warning = match outcome {
            WriteOutcome::Durable => None,
            WriteOutcome::DurabilityUnconfirmed(error) => {
                Some(staged_write::durability_warning(&error))
            }
        };
        Ok(SaveRecord { path, warning })
    }
    fn require_allowed(&self, target: SaveTarget) -> Result<()> {
        match self.allowed(target) {
            Ok(true) => Ok(()),
            Ok(false) => Err(Error::Usage(
                "Save destination is protected by the session's final action"
                    .into(),
            )),
            Err(error) => Err(Error::Io {
                context: "could not compare Save destinations".into(),
                message: error.to_string(),
            }),
        }
    }
    pub fn service(&self, state: &mut UiState, request: Option<SaveTarget>) {
        if let Some(target) = request {
            let result = self.save(state.document(), target);
            state.record_save(result);
        } else {
            self.refresh(state);
        }
    }
}

pub fn run(mut state: UiState, host: SaveHost) -> SessionResult {
    let outcome = ui::run_session(&mut state, |state, request| {
        host.service(state, request)
    })
    .map_err(|error| Error::Io {
        context: "interactive workspace failed".into(),
        message: error.to_string(),
    });
    SessionResult {
        outcome,
        saves: state.saves().to_vec(),
    }
}

pub fn require_terminal(command: &str) -> Result<()> {
    use std::io::IsTerminal;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Error::Usage(format!(
            "{command} requires an interactive terminal for input and output; run it directly in a terminal without piping or redirection"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
