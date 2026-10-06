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
    Undo,
    Redo,
    Apply,
    Cancel,
}

impl UiState {
    pub(super) fn controls(&self) -> Vec<Control> {
        vec![
            Control::Undo,
            Control::Redo,
            Control::Apply,
            Control::Cancel,
        ]
    }
    pub(super) fn enabled(&self, control: Control) -> bool {
        match control {
            Control::Undo => self.document.can_undo(),
            Control::Redo => self.document.can_redo(),
            Control::Apply => self.apply_available(),
            Control::Cancel => true,
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
        for control in self.controls() {
            let (hotkey, name, role) = match control {
                Control::Undo => ("^Z", "Undo", Role::Undo),
                Control::Redo => ("^Y", "Redo", Role::Redo),
                Control::Apply => ("^S", "Apply", Role::Apply),
                Control::Cancel => ("Esc", "Cancel", Role::Cancel),
            };
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
            let rect = Rect::new(x, area.y, line.width() as u16, 1);
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
