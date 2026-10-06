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
        self.modal.as_ref()?;
        match key.code {
            KeyCode::Esc => self.dismiss_modal(),
            KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Left
            | KeyCode::Right => self.focus_modal(matches!(
                key.code,
                KeyCode::BackTab | KeyCode::Left
            )),
            KeyCode::Enter => return self.choose_modal(),
            _ => {}
        }
        None
    }
    fn modal_choices(&self) -> Vec<(&'static str, bool)> {
        match self.modal.as_ref().map(|m| &m.kind) {
            Some(super::ModalKind::Save) => vec![
                ("Save Local", self.save_choices[0]),
                ("Save Global", self.save_choices[1]),
                ("Back", true),
            ],
            Some(super::ModalKind::Reset) => {
                vec![("Edit", true), ("Back", true)]
            }
            Some(super::ModalKind::Finish(super::FinalSelection::Export(
                _,
            ))) => vec![("Export", true), ("Back", true)],
            Some(super::ModalKind::Finish(_))
                if self.session == super::SessionKind::Copy =>
            {
                vec![("Copy", true), ("Back", true)]
            }
            _ => vec![("Apply", true), ("Back", true)],
        }
    }
    fn focus_modal(&mut self, back: bool) {
        let choices = self.modal_choices();
        let modal = self.modal.as_mut().expect("modal");
        for _ in 0..choices.len() {
            modal.choice = if back {
                (modal.choice + choices.len() - 1) % choices.len()
            } else {
                (modal.choice + 1) % choices.len()
            };
            if choices[modal.choice].1 {
                break;
            }
        }
    }
    pub(super) fn choose_modal(&mut self) -> Option<UiAction> {
        let choice = self.modal.as_ref()?.choice;
        let choices = self.modal_choices();
        if !choices.get(choice)?.1 {
            return None;
        }
        if choices[choice].0 == "Back" {
            self.dismiss_modal();
            return None;
        }
        match &self.modal.as_ref()?.kind {
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
            super::ModalKind::Save => {
                let target = if choice == 0 {
                    crate::edit::session::SaveTarget::Local
                } else {
                    crate::edit::session::SaveTarget::Global
                };
                self.dismiss_modal();
                Some(UiAction::Save(target))
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
        if self.modal.is_none() {
            return;
        }
        let theme = UiTheme::new(self.level, false);
        let area = frame.area();
        let width = area.width.saturating_sub(4).min(
            if self.session == super::SessionKind::Edit {
                60
            } else {
                72
            },
        );
        let (title, _action, lines) = self.modal_content(width, &theme);
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
        self.render_modal_buttons(frame, rect, &theme);
    }

    fn render_modal_buttons(
        &mut self,
        frame: &mut Frame,
        rect: Rect,
        theme: &UiTheme,
    ) {
        let modal = self.modal.as_ref().expect("modal");
        let width = rect.width;
        let choices = self.modal_choices();
        let save = matches!(modal.kind, super::ModalKind::Save);
        let texts: Vec<_> = choices
            .iter()
            .map(|(name, _)| {
                if save {
                    format!("[{name}]")
                } else {
                    format!("[ {name} ]")
                }
            })
            .collect();
        let total = texts.iter().map(|t| t.len() as u16).sum::<u16>()
            + 2 * (texts.len() as u16 - 1);
        let mut x = rect.x + (width.saturating_sub(total)) / 2;
        self.buttons.clear();
        for (i, (text, (_, enabled))) in
            texts.into_iter().zip(choices).enumerate()
        {
            let button = Rect::new(x, rect.bottom() - 3, text.len() as u16, 1);
            frame.render_widget(
                Paragraph::new(text).style(theme.button(
                    if i == 0 { Role::Apply } else { Role::Cancel },
                    enabled,
                    modal.choice == i,
                )),
                button,
            );
            self.buttons.push(button);
            x = button.right() + 2;
        }
    }

    fn modal_content(
        &self,
        width: u16,
        theme: &UiTheme,
    ) -> (&'static str, &'static str, Vec<Line<'static>>) {
        use super::ModalKind;
        match &self.modal.as_ref().expect("modal").kind {
            ModalKind::Apply(summary) => (
                "Confirm Apply",
                "Apply",
                super::ConfirmApply {
                    summary: summary.clone(),
                }
                .lines(width, theme, self.live),
            ),
            ModalKind::Save => (
                "Save working deck",
                "Save",
                vec![
                    Line::from("Save full deck; overwrite selected file."),
                    Line::from(super::render::clean(&self.save_paths[0])),
                    Line::from(super::render::clean(&self.save_paths[1])),
                ],
            ),
            ModalKind::Reset => (
                "Edit working deck",
                "Edit",
                vec![Line::from(
                    "Editing will rebuild the plan and reset operation selections.",
                )],
            ),
            ModalKind::Finish(payload) => {
                self.finish_content(payload, width, theme)
            }
        }
    }
    fn finish_content(
        &self,
        payload: &super::FinalSelection,
        width: u16,
        theme: &UiTheme,
    ) -> (&'static str, &'static str, Vec<Line<'static>>) {
        use super::FinalSelection;
        match payload {
            FinalSelection::Export(labels) => (
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
            FinalSelection::Plan(plan) => {
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
