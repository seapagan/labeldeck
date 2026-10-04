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

impl UiState {
    pub(super) fn render_footer(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        theme: &UiTheme,
    ) {
        let enabled = [
            self.document.can_undo(),
            self.document.can_redo(),
            self.dirty(),
            true,
        ];
        let mut x = area.x;
        for (i, (hotkey, name, role)) in [
            ("^Z", "Undo", Role::Undo),
            ("^Y", "Redo", Role::Redo),
            ("^S", "Apply", Role::Apply),
            ("Esc", "Cancel", Role::Cancel),
        ]
        .into_iter()
        .enumerate()
        {
            let focused = self.button == Some(i);
            let line = Line::from(vec![
                Span::raw("["),
                Span::styled(
                    hotkey,
                    theme.button(Role::KeyHint, enabled[i], focused),
                ),
                Span::raw(format!(" {name}]")),
            ]);
            let rect = Rect::new(x, area.y, line.width() as u16, 1);
            frame.render_widget(
                Paragraph::new(line)
                    .alignment(HorizontalAlignment::Center)
                    .style(theme.button(role, enabled[i], focused)),
                rect,
            );
            self.buttons.push(rect);
            x = rect.right() + 2;
        }
    }
}
