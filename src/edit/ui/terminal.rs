use super::{UiAction, UiState, drive_session};
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture,
        EnableBracketedPaste, EnableMouseCapture,
    },
    execute,
};
use std::io;
use std::panic::{self, AssertUnwindSafe, PanicHookInfo};
use std::sync::Arc;

#[cfg(test)]
mod tests;

type Hook = Box<dyn Fn(&PanicHookInfo<'_>) + Send + Sync + 'static>;

struct PanicHookGuard {
    previous: Option<Arc<Hook>>,
}

impl PanicHookGuard {
    fn install() -> Self {
        let previous = Arc::new(panic::take_hook());
        let hook = Arc::clone(&previous);
        panic::set_hook(Box::new(move |info| {
            restore();
            hook(info);
        }));
        Self {
            previous: Some(previous),
        }
    }

    fn run<T>(self, operation: impl FnOnce() -> T) -> T {
        // set_hook is forbidden while panicking. Catch first, remove added
        // wrappers and restore the original hook, then resume unwinding.
        let result = panic::catch_unwind(AssertUnwindSafe(operation));
        drop(self);
        match result {
            Ok(value) => value,
            Err(payload) => panic::resume_unwind(payload),
        }
    }
}

impl Drop for PanicHookGuard {
    fn drop(&mut self) {
        // Dropping the installed chain releases its reference to the original
        // hook. Restore the original box rather than stacking another wrapper.
        drop(panic::take_hook());
        if let Some(previous) = self.previous.take() {
            let hook = Arc::try_unwrap(previous)
                .unwrap_or_else(|hook| Box::new(move |info| hook(info)));
            panic::set_hook(hook);
        }
    }
}

/// Best effort cleanup attempts every mode even if an earlier operation fails.
fn restore() {
    let _ = execute!(io::stdout(), DisableMouseCapture);
    let _ = execute!(io::stdout(), DisableBracketedPaste);
    let _ = ratatui::try_restore();
}

struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        restore();
    }
}

pub(super) fn run(
    state: &mut UiState,
    mut service: impl FnMut(&mut UiState, crate::edit::session::SaveRequest),
) -> io::Result<UiAction> {
    let _guard = Guard;
    PanicHookGuard::install().run(|| {
        let mut terminal = ratatui::try_init()?;
        execute!(io::stdout(), EnableMouseCapture, EnableBracketedPaste)?;
        drive_session(&mut terminal, state, event::read, &mut service)
    })
}
