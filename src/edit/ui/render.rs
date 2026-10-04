use super::{Mode, UiState};
use crate::edit::{color::preview, model::visible_ids, plan};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table},
};

fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '\u{fffd}' } else { c })
        .collect()
}

impl UiState {
    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        self.small = area.width < 48 || area.height < 16;
        self.buttons.clear();
        if self.small {
            frame.render_widget(
                Paragraph::new(
                    "terminal too small (minimum 48x16); Esc/Ctrl-C to cancel",
                ),
                area,
            );
            return;
        }
        self.repair_selection();
        if matches!(self.mode, Mode::Confirm { .. }) {
            self.render_confirmation(frame);
            return;
        }
        let count = self
            .document
            .entries()
            .iter()
            .filter(|e| !e.deleted)
            .count();
        let dirty = plan::plan(&self.document)
            .map_or(true, |p| !p.operations.is_empty());
        frame.render_widget(
            Paragraph::new(format!(
                "labeldeck edit — {}    {count} labels{}",
                clean(&self.title),
                if dirty { " *" } else { "" }
            )),
            Rect::new(area.x, area.y, area.width, 1),
        );
        self.render_table(
            frame,
            Rect::new(area.x, area.y + 1, area.width, area.height - 10),
        );
        self.render_details(
            frame,
            Rect::new(area.x, area.bottom() - 8, area.width, 4),
        );
        frame.render_widget(
            Paragraph::new(clean(&self.error)),
            Rect::new(area.x, area.bottom() - 4, area.width, 1),
        );
        self.render_input(
            frame,
            Rect::new(area.x, area.bottom() - 3, area.width, 1),
        );
        frame.render_widget(Paragraph::new("↑↓ navigate  Enter edit  n new  Del delete  / filter  Ctrl-S apply"), Rect::new(area.x,area.bottom()-2,area.width,1));
        self.render_buttons(
            frame,
            &["Undo", "Redo", "Apply", "Cancel"],
            self.button,
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
        );
    }

    fn render_table(&mut self, frame: &mut Frame, area: Rect) {
        let visible = visible_ids(&self.document, &self.filter);
        let rows: Vec<_> = visible
            .iter()
            .filter_map(|id| {
                self.document.entries().iter().find(|e| e.id == *id)
            })
            .map(|entry| {
                let mut swatch = Span::raw("■ ");
                if let Some(color) = preview(&entry.draft.color, self.level) {
                    swatch = swatch.fg(color);
                }
                Row::new(vec![
                    Cell::from(clean(&entry.draft.name)),
                    Cell::from(Line::from(vec![
                        swatch,
                        Span::raw(clean(&entry.draft.color)),
                    ])),
                    Cell::from(clean(&entry.draft.description)),
                ])
            })
            .collect();
        self.table
            .select(visible.iter().position(|id| Some(*id) == self.selected));
        let table = Table::new(
            rows,
            [
                Constraint::Percentage(30),
                Constraint::Length(10),
                Constraint::Fill(1),
            ],
        )
        .header(Row::new(["LABEL", "COLOR", "DESCRIPTION"]))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");
        frame.render_stateful_widget(table, area, &mut self.table);
        self.rows = Rect::new(
            area.x,
            area.y + 1,
            area.width,
            area.height.saturating_sub(1),
        );
        if visible.is_empty() {
            let message = if self.filter.is_empty() {
                "No labels — n to add"
            } else {
                "No filter matches"
            };
            frame.render_widget(Paragraph::new(message), self.rows);
        }
    }

    fn render_details(&self, frame: &mut Frame, area: Rect) {
        let Some(entry) = self
            .document
            .entries()
            .iter()
            .find(|e| Some(e.id) == self.selected)
        else {
            return;
        };
        let mut swatch = Span::raw(" ██");
        if let Some(color) = preview(&entry.draft.color, self.level) {
            swatch = swatch.fg(color);
        }
        let lines = vec![
            Line::raw("─".repeat(usize::from(area.width))),
            Line::raw(format!("Name         {}", clean(&entry.draft.name))),
            Line::from(vec![
                Span::raw(format!(
                    "Color        {}",
                    clean(&entry.draft.color)
                )),
                swatch,
            ]),
            Line::raw(format!(
                "Description  {}",
                clean(&entry.draft.description)
            )),
        ];
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_input(&self, frame: &mut Frame, area: Rect) {
        let (prefix, input) = match &self.mode {
            Mode::Edit { field, input, .. } => {
                (format!("Editing {}: ", field.title()), input)
            }
            Mode::Filter { input, .. } => ("Filter: ".into(), input),
            _ => {
                frame.render_widget(
                    Paragraph::new(format!(
                        "Filter: {}  Ctrl-Z undo / Ctrl-Y redo; Tab buttons",
                        clean(&self.filter)
                    )),
                    area,
                );
                return;
            }
        };
        let prefix_width = prefix.len() as u16;
        frame.render_widget(
            Paragraph::new(prefix),
            Rect::new(area.x, area.y, prefix_width, 1),
        );
        let width = area.width.saturating_sub(prefix_width);
        let scroll = input.visual_scroll(width.saturating_sub(1) as usize);
        frame.render_widget(
            Paragraph::new(clean(input.value())).scroll((0, scroll as u16)),
            Rect::new(area.x + prefix_width, area.y, width, 1),
        );
        let cursor = input
            .visual_cursor()
            .saturating_sub(scroll)
            .min(width.saturating_sub(1) as usize);
        frame.set_cursor_position((
            area.x + prefix_width + cursor as u16,
            area.y,
        ));
    }

    fn render_confirmation(&mut self, frame: &mut Frame) {
        let Mode::Confirm { summary, apply } = &self.mode else {
            return;
        };
        let area = frame.area();
        let warning = if self.live && summary.deleted > 0 {
            "WARNING: deleting a GitHub label\nremoves it from existing issues and pull requests."
        } else {
            "Changes remain in memory until you confirm Apply."
        };
        let text = format!(
            "Confirm Apply — {}\n\n{} renamed\n{} colour/description updated\n{} created\n{} deleted\n\n{warning}",
            clean(&self.title),
            summary.renamed,
            summary.updated,
            summary.created,
            summary.deleted
        );
        frame.render_widget(
            Paragraph::new(text).wrap(ratatui::widgets::Wrap { trim: false }),
            Rect::new(area.x, area.y, area.width, area.height - 2),
        );
        self.render_buttons(
            frame,
            &["Apply", "Back"],
            Some(usize::from(!*apply)),
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
        );
    }

    fn render_buttons(
        &mut self,
        frame: &mut Frame,
        names: &[&str],
        focus: Option<usize>,
        area: Rect,
    ) {
        let width = area.width / names.len() as u16;
        for (index, name) in names.iter().enumerate() {
            let x = area.x + index as u16 * width;
            let rect = Rect::new(
                x,
                area.y,
                if index + 1 == names.len() {
                    area.right() - x
                } else {
                    width
                },
                1,
            );
            let style = if focus == Some(index) {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            frame.render_widget(
                Paragraph::new(format!("[ {name} ]")).style(style),
                rect,
            );
            self.buttons.push(rect);
        }
    }
}
