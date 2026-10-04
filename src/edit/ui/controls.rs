use super::{
    UiState,
    theme::{Role, UiTheme},
};
use ratatui::{
    Frame,
    layout::{HorizontalAlignment, Rect},
    widgets::Paragraph,
};

impl UiState {
    pub(super) fn render_footer(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        theme: &UiTheme,
    ) {
        let rectangles = [
            Rect::new(area.x, area.y, 8, 1),
            Rect::new(area.x + 9, area.y, 8, 1),
            Rect::new(area.right() - 20, area.y, 9, 1),
            Rect::new(area.right() - 10, area.y, 10, 1),
        ];
        let enabled = [
            self.document.can_undo(),
            self.document.can_redo(),
            self.dirty(),
            true,
        ];
        for (i, (name, role)) in [
            ("Undo", Role::Undo),
            ("Redo", Role::Redo),
            ("Apply", Role::Apply),
            ("Cancel", Role::Cancel),
        ]
        .into_iter()
        .enumerate()
        {
            frame.render_widget(
                Paragraph::new(format!("[ {name} ]"))
                    .alignment(HorizontalAlignment::Center)
                    .style(theme.button(
                        role,
                        enabled[i],
                        self.button == Some(i),
                    )),
                rectangles[i],
            );
        }
        self.buttons = rectangles.into();
    }
}
