use super::{
    UiAction, UiState,
    theme::{Role, UiTheme},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{HorizontalAlignment, Rect},
    text::Line,
    widgets::{Block, Clear, Paragraph},
};

impl UiState {
    pub(super) fn modal_key(&mut self, key: KeyEvent) -> Option<UiAction> {
        if key.modifiers != KeyModifiers::NONE
            && !(key.code == KeyCode::BackTab
                && key.modifiers == KeyModifiers::SHIFT)
        {
            return None;
        }
        let modal = self.modal.as_mut()?;
        match key.code {
            KeyCode::Esc => self.dismiss_modal(),
            KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Left
            | KeyCode::Right => modal.choice = 1 - modal.choice,
            KeyCode::Enter => return self.choose_modal(),
            _ => {}
        }
        None
    }
    pub(super) fn choose_modal(&mut self) -> Option<UiAction> {
        let modal = self.modal.as_ref()?;
        self.buttons.clear();
        if modal.choice == 1 {
            self.dismiss_modal();
            return None;
        }
        match &modal.kind {
            super::ModalKind::Apply(_) => {
                Some(UiAction::Apply(self.document.clone()))
            }
            super::ModalKind::Finish(payload) => {
                Some(UiAction::Finish(payload.clone()))
            }
            super::ModalKind::Reset => {
                self.edit_workspace();
                None
            }
        }
    }
    pub(super) fn choose_modal_mouse(
        &mut self,
        mouse: crossterm::event::MouseEvent,
    ) -> Option<UiAction> {
        use crossterm::event::{MouseButton, MouseEventKind};
        let position = ratatui::layout::Position::new(mouse.column, mouse.row);
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && let Some(index) =
                self.buttons.iter().position(|r| r.contains(position))
        {
            self.modal.as_mut()?.choice = index;
            return self.choose_modal();
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
        let width = area.width.saturating_sub(4).min(
            if self.session == super::SessionKind::Edit {
                60
            } else {
                72
            },
        );
        let (title, action, lines) = self.modal_content(width, &theme);
        let body_height = lines.len() as u16;
        let height = (body_height + 6).min(area.height);
        let rect = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, rect);
        frame.render_widget(
            Block::bordered()
                .title(
                    Line::from(format!(" {title} "))
                        .style(theme.style(Role::ModalTitle)),
                )
                .title_alignment(HorizontalAlignment::Center)
                .border_style(theme.style(Role::ModalBorder)),
            rect,
        );
        frame.render_widget(
            Paragraph::new(lines),
            Rect::new(
                rect.x + 2,
                rect.y + 2,
                width - 4,
                body_height.min(height.saturating_sub(5)),
            ),
        );
        let x = rect.x + (width - 20) / 2;
        self.buttons = vec![
            Rect::new(x, rect.bottom() - 3, 10, 1),
            Rect::new(x + 12, rect.bottom() - 3, 8, 1),
        ];
        for (i, (text, role)) in [
            (format!("[ {action} ]"), Role::Apply),
            ("[ Back ]".into(), Role::Cancel),
        ]
        .into_iter()
        .enumerate()
        {
            frame.render_widget(
                Paragraph::new(text)
                    .alignment(HorizontalAlignment::Center)
                    .style(theme.button(role, true, modal.choice == i)),
                self.buttons[i],
            );
        }
    }
    fn modal_content(
        &self,
        width: u16,
        theme: &UiTheme,
    ) -> (&'static str, &'static str, Vec<Line<'static>>) {
        use super::{FinalSelection, ModalKind};
        match &self.modal.as_ref().expect("modal").kind {
            ModalKind::Apply(summary) => (
                "Confirm Apply",
                "Apply",
                super::ConfirmApply {
                    summary: summary.clone(),
                }
                .lines(width, theme, self.live),
            ),
            ModalKind::Reset => (
                "Edit working deck",
                "Edit",
                vec![Line::from(
                    "Editing will rebuild the plan and reset operation selections.",
                )],
            ),
            ModalKind::Finish(FinalSelection::Export(labels)) => (
                "Confirm Export",
                "Export",
                vec![
                    Line::from(format!("{} labels selected", labels.len())),
                    Line::from(super::render::clean(
                        &self
                            .destination
                            .as_ref()
                            .expect("export destination")
                            .display()
                            .to_string(),
                    )),
                ],
            ),
            ModalKind::Finish(FinalSelection::Plan(plan)) => {
                let summary = crate::edit::plan::ChangeSummary {
                    created: plan.creates.len(),
                    updated: plan.updates.len(),
                    deleted: plan.deletes.len(),
                    renamed: 0,
                };
                let action = if self.session == super::SessionKind::Copy {
                    "Copy"
                } else {
                    "Apply"
                };
                (
                    if action == "Copy" {
                        "Confirm Copy"
                    } else {
                        "Confirm Apply"
                    },
                    action,
                    super::ConfirmApply { summary }.lines(width, theme, true),
                )
            }
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
