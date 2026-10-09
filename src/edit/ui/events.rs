use super::{
    Control, Modal, ModalKind, Mode, SessionKind, UiAction, UiState,
    WorkspaceMode, form::EditForm,
};
use crate::edit::{model::Draft, plan};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton,
    MouseEvent, MouseEventKind,
};
use ratatui::layout::Position;
use tui_input::{
    Input, InputRequest,
    backend::crossterm::{EventHandler, to_input_request},
};

#[cfg(test)]
mod tests;

impl UiState {
    pub fn handle(&mut self, event: Event) -> Option<UiAction> {
        match event {
            Event::Key(key) => return self.handle_key(key),
            Event::Resize(width, height) => self.resize(width, height),
            Event::Paste(text) => self.paste(&text),
            Event::Mouse(mouse) if !self.small => return self.mouse(mouse),
            _ => {}
        }
        None
    }

    fn resize(&mut self, width: u16, height: u16) {
        self.small = self.too_small(width, height);
        self.buttons.clear();
        self.fields = [ratatui::layout::Rect::default(); 3];
        self.rows = ratatui::layout::Rect::default();
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<UiAction> {
        // Lizard misparses this Rust function's scope and inflates its NLOC.
        // #lizard forgives
        if key.kind == KeyEventKind::Release {
            return None;
        }
        if key.modifiers == KeyModifiers::CONTROL
            && key.code == KeyCode::Char('c')
        {
            return Some(UiAction::Cancel);
        }
        // List shortcuts require plain keys. Keep modifiers for text inputs.
        let shortcut_modifier = key.modifiers.intersects(
            KeyModifiers::CONTROL
                | KeyModifiers::ALT
                | KeyModifiers::SUPER
                | KeyModifiers::HYPER
                | KeyModifiers::META,
        );
        if self.small {
            if !shortcut_modifier
                && matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
            {
                return self.key(key);
            }
            return None;
        }
        if self.modal.is_some() {
            return self.modal_key(key);
        }
        if key.modifiers == KeyModifiers::CONTROL {
            match key.code {
                KeyCode::Char('z' | 'y')
                    if self.workspace == WorkspaceMode::Edit =>
                {
                    self.mode = Mode::List;
                    if key.code == KeyCode::Char('z') {
                        self.document.undo();
                    } else {
                        self.document.redo();
                    }
                    self.message = None;
                    self.repair_selection();
                    return None;
                }
                KeyCode::Char('s') => {
                    if matches!(self.mode, Mode::List) {
                        if self.session == SessionKind::Edit {
                            self.confirm();
                        } else if self.workspace == WorkspaceMode::Edit {
                            self.open_save();
                        }
                    }
                    return None;
                }
                _ => {}
            }
        }
        if shortcut_modifier && matches!(self.mode, Mode::List) {
            return None;
        }
        self.key(key)
    }

    fn key(&mut self, key: KeyEvent) -> Option<UiAction> {
        match &mut self.mode {
            Mode::List => self.list_key(key.code),
            Mode::Edit(_) => {
                self.edit_key(key);
                None
            }
            Mode::Filter { before, input } => {
                match key.code {
                    KeyCode::Esc => {
                        self.filter = before.clone();
                        self.mode = Mode::List;
                    }
                    KeyCode::Enter => {
                        self.mode = Mode::List;
                    }
                    _ => {
                        input.handle_event(&Event::Key(key));
                        self.filter = input.value().into();
                    }
                }
                self.repair_selection();
                None
            }
        }
    }

    fn list_key(&mut self, code: KeyCode) -> Option<UiAction> {
        if self.workspace == WorkspaceMode::Select {
            return self.select_key(code);
        }
        match code {
            KeyCode::Esc | KeyCode::Char('q')
                if self.session != SessionKind::Edit =>
            {
                self.done();
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                return Some(UiAction::Cancel);
            }
            KeyCode::Up => self.navigate(-1),
            KeyCode::Down => self.navigate(1),
            KeyCode::PageUp => {
                self.navigate(-(self.rows.height.max(5) as isize))
            }
            KeyCode::PageDown => {
                self.navigate(self.rows.height.max(5) as isize)
            }
            KeyCode::Tab => self.focus_control(false),
            KeyCode::BackTab => self.focus_control(true),
            KeyCode::Enter if self.button.is_some() => {
                return self.activate(self.button.unwrap());
            }
            KeyCode::Enter | KeyCode::Char('e') => self.start_edit(),
            KeyCode::Char('n') => {
                self.mode = Mode::Edit(EditForm::new(
                    None,
                    Draft {
                        name: String::new(),
                        color: "ededed".into(),
                        description: String::new(),
                    },
                ));
                self.button = None;
                self.message = None;
            }
            KeyCode::Delete => self.delete_selected(),
            KeyCode::Char('s') => self.open_save(),
            KeyCode::Char('/') => {
                self.button = None;
                self.mode = Mode::Filter {
                    before: self.filter.clone(),
                    input: Input::new(self.filter.clone()),
                };
            }
            _ => {}
        }
        None
    }

    fn select_key(&mut self, code: KeyCode) -> Option<UiAction> {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                return Some(UiAction::Cancel);
            }
            KeyCode::Up => self.navigate(-1),
            KeyCode::Down => self.navigate(1),
            KeyCode::PageUp => {
                self.navigate(-(self.rows.height.max(5) as isize))
            }
            KeyCode::PageDown => {
                self.navigate(self.rows.height.max(5) as isize)
            }
            KeyCode::Tab => self.focus_control(false),
            KeyCode::BackTab => self.focus_control(true),
            KeyCode::Enter if self.button.is_some() => {
                return self.activate(self.button.unwrap());
            }
            KeyCode::Char(' ') => self.toggle_row(),
            KeyCode::Char('/') => {
                self.button = None;
                self.mode = Mode::Filter {
                    before: self.filter.clone(),
                    input: Input::new(self.filter.clone()),
                };
            }
            _ => {
                if let Some(control) = self
                    .controls()
                    .into_iter()
                    .find(|c| c.hotkey_code() == Some(code))
                {
                    return self.activate(control);
                }
            }
        }
        None
    }

    fn delete_selected(&mut self) {
        let visible = self.visible();
        let Some(index) =
            visible.iter().position(|&id| Some(id) == self.selected)
        else {
            return;
        };
        if self.document.delete(visible[index]) {
            let remaining = self.visible();
            self.selected = remaining
                .get(index.min(remaining.len().saturating_sub(1)))
                .copied();
        }
    }

    fn navigate(&mut self, distance: isize) {
        self.button = None;
        let visible = self.visible();
        let index = visible
            .iter()
            .position(|&id| Some(id) == self.selected)
            .unwrap_or(0);
        self.selected = visible
            .get(
                index
                    .saturating_add_signed(distance)
                    .min(visible.len().saturating_sub(1)),
            )
            .copied();
    }

    fn start_edit(&mut self) {
        let Some(entry) = self
            .document
            .entries()
            .iter()
            .find(|e| Some(e.id) == self.selected && !e.deleted)
        else {
            return;
        };
        self.mode =
            Mode::Edit(EditForm::new(Some(entry.id), entry.draft.clone()));
        self.button = None;
        self.message = None;
    }

    fn edit_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::List;
                self.message = None;
                self.repair_selection();
            }
            KeyCode::Enter => {
                if let Err(error) = self.commit_form() {
                    self.set_error(error);
                }
            }
            _ => {
                if let Mode::Edit(form) = &mut self.mode {
                    match key.code {
                        KeyCode::Up | KeyCode::BackTab => {
                            form.field = form.field.next(true)
                        }
                        KeyCode::Down | KeyCode::Tab => {
                            form.field = form.field.next(false)
                        }
                        _ => {
                            if let Some(request) =
                                to_input_request(&Event::Key(key))
                            {
                                let error = form
                                    .handle(request)
                                    .err()
                                    .unwrap_or_default();
                                self.set_error(error);
                            }
                        }
                    }
                }
            }
        }
    }

    fn commit_form(&mut self) -> Result<(), String> {
        let Mode::Edit(form) = &mut self.mode else {
            return Ok(());
        };
        let draft = form.validate()?;
        if let Some(id) = form.id {
            self.document.commit(id, draft)?;
        } else {
            self.selected = Some(self.document.create(draft)?);
        }
        self.mode = Mode::List;
        self.message = None;
        self.repair_selection();
        Ok(())
    }

    fn paste(&mut self, text: &str) {
        if self.modal.is_some() {
            return;
        }
        match &mut self.mode {
            Mode::Edit(form) => {
                let error = form.insert(text).err().unwrap_or_default();
                self.set_error(error);
            }
            Mode::Filter { input, .. } => {
                for ch in text.chars().filter(|ch| !ch.is_control()) {
                    input.handle(InputRequest::InsertChar(ch));
                }
                self.filter = input.value().into();
                self.repair_selection();
            }
            _ => {}
        }
    }

    pub(super) fn apply_available(&self) -> bool {
        self.workspace == WorkspaceMode::Edit
            && matches!(self.mode, Mode::List)
            && self.modal.is_none()
            && self.dirty()
    }

    fn confirm(&mut self) {
        if !matches!(self.mode, Mode::List) || self.modal.is_some() {
            return;
        }
        if !self.dirty() {
            self.set_status("No changes to apply.".into());
            return;
        }
        match plan::plan(&self.document) {
            Ok(plan) => {
                self.modal = Some(Modal {
                    kind: ModalKind::Apply(plan.summary),
                    choice: 1,
                });
                self.buttons.clear();
                self.message = None;
            }
            Err(error) => self.set_error(error),
        }
    }

    pub(super) fn activate(&mut self, control: Control) -> Option<UiAction> {
        if control == Control::Apply {
            self.confirm();
            return None;
        }
        if !self.enabled(control) {
            return None;
        }
        self.message = None;
        match control {
            Control::Undo => {
                self.document.undo();
                self.repair_selection();
            }
            Control::Redo => {
                self.document.redo();
                self.repair_selection();
            }
            Control::Cancel => return Some(UiAction::Cancel),
            Control::Apply => unreachable!(),
            Control::All => self.select_rows(None, Some(true)),
            Control::None => self.select_rows(None, Some(false)),
            Control::Invert => self.select_rows(None, None),
            Control::Group(group) => self.select_rows(Some(group), None),
            Control::SelectedOnly => {
                self.selected_only = !self.selected_only;
                self.repair_selection();
            }
            Control::Edit => self.enter_workspace(),
            Control::Done => self.done(),
            Control::Save => self.open_save(),
            Control::Finish => self.confirm_finish(),
        }
        None
    }

    fn mouse(&mut self, mouse: MouseEvent) -> Option<UiAction> {
        if self.modal.is_some() {
            return self.choose_modal_mouse(mouse);
        }
        match &mut self.mode {
            Mode::Edit(form) => {
                form_mouse(form, &self.fields, mouse);
                None
            }
            Mode::List => self.list_mouse(mouse),
            _ => None,
        }
    }

    fn list_mouse(&mut self, mouse: MouseEvent) -> Option<UiAction> {
        let position = Position::new(mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::ScrollUp => self.navigate(-1),
            MouseEventKind::ScrollDown => self.navigate(1),
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(index) =
                    self.buttons.iter().position(|r| r.contains(position))
                {
                    return self.activate(self.controls()[index]);
                }
                if self.rows.contains(position) {
                    let index = usize::from(mouse.row - self.rows.y)
                        + self.table.offset();
                    self.selected = self.visible().get(index).copied();
                    self.button = None;
                    if self.workspace == WorkspaceMode::Select
                        && (self.rows.x + 2..self.rows.x + 5)
                            .contains(&mouse.column)
                    {
                        self.toggle_row();
                    }
                }
            }
            _ => {}
        }
        None
    }
}

fn form_mouse(
    form: &mut EditForm,
    fields: &[ratatui::layout::Rect; 3],
    mouse: MouseEvent,
) {
    let position = Position::new(mouse.column, mouse.row);
    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
        && let Some(i) = fields.iter().position(|r| r.contains(position))
    {
        form.field = [
            super::Field::Name,
            super::Field::Color,
            super::Field::Description,
        ][i];
    }
}
