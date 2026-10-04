use super::{
    Mode, UiState,
    theme::{Role, UiTheme},
};
use crate::edit::{color::preview, model::visible_ids, plan};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    text::{Line, Span},
    widgets::{Cell, HighlightSpacing, Paragraph, Row, Table},
};

pub(super) fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '\u{fffd}' } else { c })
        .collect()
}

impl UiState {
    pub(super) fn dirty(&self) -> bool {
        let baseline: Vec<_> = self
            .document
            .entries()
            .iter()
            .filter_map(|entry| entry.original.clone())
            .collect();
        self.document
            .labels()
            .map_or(true, |labels| !plan::same_labels(&baseline, &labels))
    }

    pub(super) fn swatch(&self, hex: &str, theme: &UiTheme) -> Span<'static> {
        let valid = crate::labels::LabelColor::parse(hex).is_ok();
        let mut style = theme.style(Role::DetailValue);
        if let Some(color) = preview(hex, self.level) {
            style = style.fg(color);
        }
        Span::styled(if valid { "██ " } else { "░░ " }, style)
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        self.small = area.width < 48 || area.height < 16;
        self.buttons.clear();
        self.fields = [Rect::default(); 3];
        self.rows = Rect::default();
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
        let theme = UiTheme::new(self.level, self.modal.is_some());
        self.render_title(
            frame,
            Rect::new(area.x, area.y, area.width, 1),
            &theme,
        );
        let visible = visible_ids(&self.document, &self.filter);
        let height = (visible.len().max(1).saturating_add(2))
            .min(usize::from(area.height - 9)) as u16;
        let table_area = Rect::new(area.x, area.y + 1, area.width, height);
        self.render_table(frame, table_area, &theme);
        self.render_details(
            frame,
            Rect::new(area.x, table_area.bottom(), area.width, 4),
            &theme,
        );
        frame.render_widget(
            Paragraph::new(clean(&self.error)).style(theme.style(Role::Error)),
            Rect::new(area.x, area.bottom() - 4, area.width, 1),
        );
        self.render_filter(
            frame,
            Rect::new(area.x, area.bottom() - 3, area.width, 1),
            &theme,
        );
        self.render_help(
            frame,
            Rect::new(area.x, area.bottom() - 2, area.width, 1),
        );
        self.render_footer(
            frame,
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
            &theme,
        );
        if self.modal.is_some() {
            self.render_modal(frame);
        }
    }

    fn render_title(&self, frame: &mut Frame, area: Rect, theme: &UiTheme) {
        let count = self
            .document
            .entries()
            .iter()
            .filter(|e| !e.deleted)
            .count();
        frame.render_widget(
            Paragraph::new(format!(
                "labeldeck edit — {}    {count} labels{}",
                clean(&self.title),
                if self.dirty() { " *" } else { "" }
            ))
            .style(theme.style(Role::Title)),
            area,
        );
    }

    fn render_table(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        theme: &UiTheme,
    ) {
        let visible = visible_ids(&self.document, &self.filter);
        let entries: Vec<_> = visible
            .iter()
            .filter_map(|id| {
                self.document.entries().iter().find(|e| e.id == *id)
            })
            .collect();
        let name_width = name_width(&entries, area.width);
        let rows: Vec<_> = entries
            .iter()
            .map(|entry| self.table_row(&entry.draft, theme))
            .collect();
        self.table
            .select(visible.iter().position(|id| Some(*id) == self.selected));
        let table = Table::new(
            rows,
            [
                Constraint::Length(name_width),
                Constraint::Length(10),
                Constraint::Fill(1),
            ],
        )
        .column_spacing(2)
        .highlight_spacing(HighlightSpacing::Always)
        .header(
            Row::new(["LABEL", "COLOR", "DESCRIPTION"])
                .style(theme.style(Role::Header))
                .bottom_margin(1),
        )
        .style(theme.style(Role::DetailValue))
        .row_highlight_style(theme.style(Role::Selected))
        .highlight_symbol("> ");
        frame.render_stateful_widget(table, area, &mut self.table);
        frame.render_widget(
            Paragraph::new("─".repeat(usize::from(area.width)))
                .style(theme.style(Role::Separator)),
            Rect::new(area.x, area.y + 1, area.width, 1),
        );
        self.rows = Rect::new(
            area.x,
            area.y + 2,
            area.width,
            area.height.saturating_sub(2),
        );
        if visible.is_empty() {
            let message = if self.filter.is_empty() {
                "No labels — n to add"
            } else {
                "No filter matches"
            };
            frame.render_widget(
                Paragraph::new(message).style(theme.style(Role::Help)),
                self.rows,
            );
        }
    }

    fn table_row(
        &self,
        draft: &crate::edit::model::Draft,
        theme: &UiTheme,
    ) -> Row<'static> {
        Row::new(vec![
            Cell::from(clean(&draft.name)),
            Cell::from(Line::from(vec![
                self.swatch(&draft.color, theme),
                Span::raw(clean(&draft.color)),
            ])),
            Cell::from(clean(&draft.description)),
        ])
    }

    fn render_filter(&self, frame: &mut Frame, area: Rect, theme: &UiTheme) {
        frame.render_widget(
            Paragraph::new("Filter: ").style(theme.style(Role::Filter)),
            Rect::new(area.x, area.y, 8, 1),
        );
        let width = area.width - 8;
        let input = if let Mode::Filter { input, .. } = &self.mode {
            Some(input)
        } else {
            None
        };
        let scroll = input
            .map_or(0, |i| i.visual_scroll(width.saturating_sub(1) as usize));
        frame.render_widget(
            Paragraph::new(clean(&self.filter))
                .style(theme.style(Role::DetailValue))
                .scroll((0, scroll as u16)),
            Rect::new(area.x + 8, area.y, width, 1),
        );
        if self.modal.is_none()
            && let Some(input) = input
        {
            let cursor = input
                .visual_cursor()
                .saturating_sub(scroll)
                .min(width.saturating_sub(1) as usize);
            frame.set_cursor_position((area.x + 8 + cursor as u16, area.y));
        }
    }

    fn render_help(&self, frame: &mut Frame, area: Rect) {
        let hints: &[(&str, &str)] = if self.modal.is_some() {
            &[("←→", "choose"), ("Enter", "confirm"), ("Esc", "back")]
        } else {
            match self.mode {
                Mode::Edit(_) => &[
                    ("↑↓", "field"),
                    ("Enter", "save"),
                    ("Esc", "cancel"),
                    ("Tab/Shift-Tab", "field"),
                ],
                Mode::Filter { .. } => &[
                    ("type", "to filter"),
                    ("Enter", "accept"),
                    ("Esc", "restore"),
                ],
                Mode::List => &[
                    ("↑↓", "navigate"),
                    ("Enter", "edit"),
                    ("n", "new"),
                    ("Del", "delete"),
                    ("/", "filter"),
                    ("Ctrl-S", "apply"),
                    ("Ctrl-Z/Y", "undo/redo"),
                    ("Tab", "buttons"),
                ],
            }
        };
        let theme = UiTheme::new(self.level, false);
        let spans: Vec<_> = hints
            .iter()
            .flat_map(|(key, action)| {
                [
                    Span::styled(*key, theme.style(Role::KeyHint)),
                    Span::styled(
                        format!(" {action}   "),
                        theme.style(Role::Help),
                    ),
                ]
            })
            .collect();
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

fn name_width(
    entries: &[&crate::edit::model::EditorEntry],
    width: u16,
) -> u16 {
    entries
        .iter()
        .map(|e| Line::raw(clean(&e.draft.name)).width())
        .max()
        .unwrap_or(0)
        .saturating_add(1)
        .clamp(12, 34)
        .min(usize::from(width.saturating_sub(28))) as u16
}
