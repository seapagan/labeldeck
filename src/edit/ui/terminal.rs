use super::{UiAction, UiState};
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture,
        EnableBracketedPaste, EnableMouseCapture,
    },
    execute,
};
use std::io;
use std::sync::Arc;

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
    mut state: UiState,
) -> io::Result<Option<crate::edit::model::Document>> {
    let _guard = Guard;
    let old_hook = Arc::new(std::panic::take_hook());
    let hook = Arc::clone(&old_hook);
    std::panic::set_hook(Box::new(move |info| {
        restore();
        hook(info);
    }));
    let result = run_loop(&mut state);
    // Replace Ratatui's added hook so repeated invocations do not stack it.
    std::panic::set_hook(Box::new(move |info| old_hook(info)));
    result
}

fn run_loop(
    state: &mut UiState,
) -> io::Result<Option<crate::edit::model::Document>> {
    let mut terminal = ratatui::try_init()?;
    execute!(io::stdout(), EnableMouseCapture, EnableBracketedPaste)?;
    loop {
        terminal.draw(|frame| state.render(frame))?;
        match state.handle(event::read()?) {
            Some(UiAction::Cancel) => return Ok(None),
            Some(UiAction::Apply(document)) => return Ok(Some(document)),
            None => {}
        }
    }
}
