use super::{
    UiState,
    theme::{Role, UiTheme},
};
use ratatui::{
    Frame,
    layout::{HorizontalAlignment, Rect},
    text::{Line, Span},
    widgets::Paragraph,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Control {
    Save,
    Undo,
    Redo,
    Apply,
    Cancel,
    All,
    None,
    Invert,
    SelectedOnly,
    Edit,
    Done,
    Finish,
    Group(super::ActionGroup),
}

impl Control {
    pub(super) fn presentation(
        self,
        session: super::SessionKind,
    ) -> (&'static str, &'static str, Role) {
        use super::ActionGroup;
        match self {
            Self::Save => (
                if session == super::SessionKind::Edit {
                    "s"
                } else {
                    "^S"
                },
                "Save",
                Role::Apply,
            ),
            Self::Undo => ("^Z", "Undo", Role::Undo),
            Self::Redo => ("^Y", "Redo", Role::Redo),
            Self::Apply => ("^S", "Apply", Role::Apply),
            Self::Cancel => ("Esc", "Cancel", Role::Cancel),
            Self::All => ("a", "All", Role::Apply),
            Self::None => ("0", "None", Role::Apply),
            Self::Invert => ("i", "Invert", Role::Apply),
            Self::SelectedOnly => ("v", "Selected Only", Role::Apply),
            Self::Edit => ("w", "Edit", Role::Apply),
            Self::Done => ("Esc", "Done", Role::Apply),
            Self::Group(ActionGroup::Create) => ("c", "Create", Role::Apply),
            Self::Group(ActionGroup::Update) => ("u", "Update", Role::Apply),
            Self::Group(ActionGroup::Delete) => ("d", "Delete", Role::Cancel),
            Self::Finish => (
                "f",
                match session {
                    super::SessionKind::Export => "Export",
                    super::SessionKind::Copy => "Copy",
                    _ => "Apply",
                },
                Role::Apply,
            ),
        }
    }
    pub(super) fn hotkey_code(self) -> Option<crossterm::event::KeyCode> {
        let hotkey = self.presentation(super::SessionKind::Edit).0;
        if hotkey.len() == 1 {
            Some(crossterm::event::KeyCode::Char(hotkey.chars().next()?))
        } else {
            None
        }
    }
}

impl UiState {
    pub(super) fn controls(&self) -> Vec<Control> {
        use super::{ActionGroup, SessionKind, WorkspaceMode};
        if self.workspace == WorkspaceMode::Edit {
            return if self.session == SessionKind::Edit {
                vec![
                    Control::Undo,
                    Control::Redo,
                    Control::Apply,
                    Control::Cancel,
                    Control::Save,
                ]
            } else {
                vec![
                    Control::Undo,
                    Control::Redo,
                    Control::Save,
                    Control::Done,
                    Control::Cancel,
                ]
            };
        }
        let mut controls = vec![
            Control::All,
            Control::None,
            Control::Invert,
            Control::SelectedOnly,
        ];
        if self.session != SessionKind::Export {
            controls.extend([
                Control::Group(ActionGroup::Create),
                Control::Group(ActionGroup::Update),
                Control::Group(ActionGroup::Delete),
            ]);
        }
        controls.extend([Control::Edit, Control::Finish, Control::Cancel]);
        controls
    }
    pub(super) fn enabled(&self, control: Control) -> bool {
        let list =
            matches!(self.mode, super::Mode::List) && self.modal.is_none();
        match control {
            Control::Undo => {
                self.workspace == super::WorkspaceMode::Edit
                    && self.document.can_undo()
            }
            Control::Redo => {
                self.workspace == super::WorkspaceMode::Edit
                    && self.document.can_redo()
            }
            Control::Apply => self.apply_available(),
            Control::Cancel => true,
            Control::Group(group) => {
                list && self
                    .candidates()
                    .iter()
                    .any(|r| r.group == Some(group))
            }
            Control::All | Control::None | Control::Invert => {
                list && !self.candidates().is_empty()
            }
            Control::Finish => {
                list && (self.session == super::SessionKind::Export
                    || !self.selected_plan().is_empty())
            }
            _ => list,
        }
    }
    pub(super) fn focus_control(&mut self, back: bool) {
        let controls = self.controls();
        let mut index = self
            .button
            .and_then(|c| controls.iter().position(|&v| v == c))
            .unwrap_or(if back { 0 } else { controls.len() - 1 });
        for _ in 0..controls.len() {
            index = if back {
                (index + controls.len() - 1) % controls.len()
            } else {
                (index + 1) % controls.len()
            };
            if self.enabled(controls[index]) {
                self.button = Some(controls[index]);
                break;
            }
        }
    }
    pub(super) fn render_footer(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        theme: &UiTheme,
    ) {
        let mut x = area.x;
        let mut y = area.y;
        for (index, control) in self.controls().into_iter().enumerate() {
            if self.workspace == super::WorkspaceMode::Select && index == 4 {
                x = area.x;
                y += 1;
            }
            if self.session == super::SessionKind::Edit && index == 4 {
                x = area.right() - 8;
                y = area.y - 2;
            }
            let (hotkey, name, role) = control.presentation(self.session);
            let focused = self.button == Some(control);
            let enabled = self.enabled(control);
            let line = Line::from(vec![
                Span::raw("["),
                Span::styled(
                    hotkey,
                    theme.button(Role::KeyHint, enabled, focused),
                ),
                Span::raw(format!(" {name}]")),
            ]);
            let rect = Rect::new(x, y, line.width() as u16, 1);
            frame.render_widget(
                Paragraph::new(line)
                    .alignment(HorizontalAlignment::Center)
                    .style(theme.button(role, enabled, focused)),
                rect,
            );
            self.buttons.push(rect);
            x = rect.right() + 2;
        }
    }
}
