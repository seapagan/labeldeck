use super::{Field, Mode, UiAction, UiState};
use crate::edit::{model::visible_ids, plan};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton,
    MouseEvent, MouseEventKind,
};
use ratatui::layout::Position;
use tui_input::{Input, InputRequest, backend::crossterm::EventHandler};

impl UiState {
    pub fn handle(&mut self, event: Event) -> Option<UiAction> {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Release {
                return None;
            }
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && key.code == KeyCode::Char('c')
            {
                return Some(UiAction::Cancel);
            }
            if self.small {
                return matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
                    .then_some(UiAction::Cancel);
            }
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                match key.code {
                    KeyCode::Char('z' | 'y') => {
                        self.mode = Mode::List;
                        if key.code == KeyCode::Char('z') {
                            self.document.undo();
                        } else {
                            self.document.redo();
                        }
                        self.error.clear();
                        self.repair_selection();
                        return None;
                    }
                    KeyCode::Char('s') if matches!(self.mode, Mode::List) => {
                        self.confirm();
                        return None;
                    }
                    _ => {}
                }
            }
            return self.key(key);
        }
        match event {
            Event::Paste(text) => self.paste(&text),
            Event::Mouse(mouse) if !self.small => return self.mouse(mouse),
            _ => {}
        }
        None
    }

    fn key(&mut self, key: KeyEvent) -> Option<UiAction> {
        match &mut self.mode {
            Mode::List => self.list_key(key.code),
            Mode::Confirm { apply, .. } => match key.code {
                KeyCode::Esc => {
                    self.mode = Mode::List;
                    None
                }
                KeyCode::Tab
                | KeyCode::BackTab
                | KeyCode::Left
                | KeyCode::Right => {
                    *apply = !*apply;
                    None
                }
                KeyCode::Enter if *apply => {
                    Some(UiAction::Apply(self.document.clone()))
                }
                KeyCode::Enter => {
                    self.mode = Mode::List;
                    None
                }
                _ => None,
            },
            Mode::Edit { .. } => {
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
        match code {
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
            KeyCode::Tab => {
                self.button = Some(self.button.map_or(0, |i| (i + 1) % 4))
            }
            KeyCode::BackTab => {
                self.button = Some(self.button.map_or(3, |i| (i + 3) % 4))
            }
            KeyCode::Enter if self.button.is_some() => {
                return self.activate(self.button.unwrap());
            }
            KeyCode::Enter | KeyCode::Char('e') => {
                self.start_edit(Field::Name)
            }
            KeyCode::Char('n') => {
                self.filter.clear();
                self.selected = Some(self.document.add());
                self.start_edit(Field::Name);
            }
            KeyCode::Delete => {
                if let Some(id) = self.selected {
                    self.document.delete(id);
                    self.repair_selection();
                }
            }
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

    fn navigate(&mut self, distance: isize) {
        self.button = None;
        let visible = visible_ids(&self.document, &self.filter);
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

    fn start_edit(&mut self, field: Field) {
        let Some(entry) = self
            .document
            .entries()
            .iter()
            .find(|e| Some(e.id) == self.selected && !e.deleted)
        else {
            return;
        };
        let value = match field {
            Field::Name => &entry.draft.name,
            Field::Color => &entry.draft.color,
            Field::Description => &entry.draft.description,
        };
        self.mode = Mode::Edit {
            id: entry.id,
            field,
            input: Input::new(value.clone()),
        };
        self.button = None;
        self.error.clear();
    }

    fn edit_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.mode = Mode::List;
            self.error.clear();
            self.repair_selection();
            return;
        }
        if matches!(key.code, KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab)
        {
            if let Err(error) = self.commit_input(key.code) {
                self.error = error;
            }
        } else if let Mode::Edit { input, .. } = &mut self.mode {
            input.handle_event(&Event::Key(key));
        }
    }

    fn commit_input(&mut self, code: KeyCode) -> Result<(), String> {
        let Mode::Edit { id, field, input } = &self.mode else {
            return Ok(());
        };
        let mut draft = self
            .document
            .entries()
            .iter()
            .find(|e| e.id == *id)
            .ok_or("the edited label no longer exists")?
            .draft
            .clone();
        match field {
            Field::Name => draft.name = input.value().into(),
            Field::Color => draft.color = input.value().into(),
            Field::Description => draft.description = input.value().into(),
        }
        let field = *field;
        self.document.commit(*id, draft)?;
        self.mode = Mode::List;
        self.error.clear();
        if code != KeyCode::Enter {
            self.start_edit(field.next(code == KeyCode::BackTab));
        } else {
            self.repair_selection();
        }
        Ok(())
    }

    fn paste(&mut self, text: &str) {
        let input = match &mut self.mode {
            Mode::Edit { input, .. } | Mode::Filter { input, .. } => input,
            _ => return,
        };
        for ch in text.chars().filter(|ch| !ch.is_control()) {
            input.handle(InputRequest::InsertChar(ch));
        }
        if matches!(self.mode, Mode::Filter { .. }) {
            if let Mode::Filter { input, .. } = &self.mode {
                self.filter = input.value().into();
            }
            self.repair_selection();
        }
    }

    fn confirm(&mut self) {
        match plan::plan(&self.document) {
            Ok(plan) => {
                self.mode = Mode::Confirm {
                    summary: plan.summary,
                    apply: false,
                };
                self.error.clear();
            }
            Err(error) => self.error = error,
        }
    }

    fn activate(&mut self, button: usize) -> Option<UiAction> {
        self.error.clear();
        match button {
            0 => {
                self.document.undo();
                self.repair_selection();
            }
            1 => {
                self.document.redo();
                self.repair_selection();
            }
            2 => self.confirm(),
            _ => return Some(UiAction::Cancel),
        }
        None
    }

    fn mouse(&mut self, mouse: MouseEvent) -> Option<UiAction> {
        let position = Position::new(mouse.column, mouse.row);
        if let Mode::Confirm { .. } = self.mode {
            if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                if self.buttons.first().is_some_and(|r| r.contains(position)) {
                    return Some(UiAction::Apply(self.document.clone()));
                }
                if self.buttons.get(1).is_some_and(|r| r.contains(position)) {
                    self.mode = Mode::List;
                }
            }
            return None;
        }
        if !matches!(self.mode, Mode::List) {
            return None;
        }
        match mouse.kind {
            MouseEventKind::ScrollUp => self.navigate(-1),
            MouseEventKind::ScrollDown => self.navigate(1),
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(index) =
                    self.buttons.iter().position(|r| r.contains(position))
                {
                    return self.activate(index);
                }
                if self.rows.contains(position) {
                    let index = usize::from(mouse.row - self.rows.y)
                        + self.table.offset();
                    self.selected = visible_ids(&self.document, &self.filter)
                        .get(index)
                        .copied();
                    self.button = None;
                }
            }
            _ => {}
        }
        None
    }
}
