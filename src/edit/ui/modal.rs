use super::{
    UiAction, UiState,
    theme::{Role, UiTheme},
};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{HorizontalAlignment, Rect},
    text::Line,
    widgets::{Block, Clear, Paragraph},
};

impl UiState {
    pub(super) fn modal_key(&mut self, key: KeyEvent) -> Option<UiAction> {
        let modal = self.modal.as_mut()?;
        match key.code {
            KeyCode::Esc => self.dismiss_modal(),
            KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Left
            | KeyCode::Right => modal.apply = !modal.apply,
            KeyCode::Enter if modal.apply => {
                return Some(UiAction::Apply(self.document.clone()));
            }
            KeyCode::Enter => self.dismiss_modal(),
            _ => {}
        }
        None
    }

    pub(super) fn dismiss_modal(&mut self) {
        self.modal = None;
        self.buttons.clear();
    }

    pub(super) fn render_modal(&mut self, frame: &mut Frame) {
        let Some(modal) = &self.modal else {
            return;
        };
        let theme = UiTheme::new(self.level, false);
        let area = frame.area();
        let width = area.width.saturating_sub(4).min(60);
        let lines = modal.lines(width, &theme, self.live);
        let body_height = lines.len() as u16;
        let height = body_height + 6;
        let rect = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, rect);
        let block = Block::bordered()
            .title(
                Line::from(" Confirm Apply ")
                    .style(theme.style(Role::ModalTitle)),
            )
            .title_alignment(HorizontalAlignment::Center)
            .border_style(theme.style(Role::ModalBorder));
        frame.render_widget(block, rect);
        frame.render_widget(
            Paragraph::new(lines),
            Rect::new(rect.x + 2, rect.y + 2, width - 4, body_height),
        );
        let x = rect.x + (width - 20) / 2;
        self.buttons = vec![
            Rect::new(x, rect.bottom() - 3, 9, 1),
            Rect::new(x + 12, rect.bottom() - 3, 8, 1),
        ];
        for (i, (text, role)) in
            [("[ Apply ]", Role::Apply), ("[ Back ]", Role::Cancel)]
                .into_iter()
                .enumerate()
        {
            frame.render_widget(
                Paragraph::new(text)
                    .alignment(HorizontalAlignment::Center)
                    .style(theme.button(role, true, modal.apply == (i == 0))),
                self.buttons[i],
            );
        }
    }
}

impl super::ConfirmApply {
    fn lines(
        &self,
        width: u16,
        theme: &UiTheme,
        live: bool,
    ) -> Vec<Line<'static>> {
        let counts = [
            (self.summary.renamed, "renamed"),
            (self.summary.updated, "colour/description updated"),
            (self.summary.created, "created"),
            (self.summary.deleted, "deleted"),
        ];
        let mut lines: Vec<_> = counts
            .into_iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, text)| {
                Line::from(format!("{n} {text}"))
                    .style(theme.style(Role::ModalBody))
            })
            .collect();
        let warning = live && self.summary.deleted > 0;
        if warning {
            lines.push(Line::default());
            let text = if width >= 50 {
                [
                    "Deleting labels removes them from existing",
                    "issues and pull requests.",
                ]
            } else {
                [
                    "Deleting labels removes them from",
                    "existing issues and pull requests.",
                ]
            };
            lines.extend(text.map(|text| {
                Line::from(text).style(theme.style(Role::Warning))
            }));
        }
        lines
    }
}
