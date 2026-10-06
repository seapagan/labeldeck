use super::{
    Mode, SessionKind, UiState, WorkspaceMode,
    form::DETAIL_HEIGHT,
    theme::{Role, UiTheme},
};
use crate::edit::{color::preview, plan};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    text::{Line, Span},
    widgets::{Cell, HighlightSpacing, Paragraph, Row, Table},
};

const TITLE_HEIGHT: u16 = 2; // Title plus one blank spacer row.

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
        Span::styled(if valid { "■ " } else { "· " }, style)
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        self.small = self.too_small(area.width, area.height);
        self.buttons.clear();
        self.fields = [Rect::default(); 3];
        self.rows = Rect::default();
        if self.small {
            frame.render_widget(
                Paragraph::new(format!(
                    "terminal too small (minimum {}x{}); Esc/Ctrl-C to cancel",
                    self.minimum_size().0,
                    self.minimum_size().1
                )),
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
        if self.workspace == WorkspaceMode::Select {
            frame.render_widget(
                Paragraph::new(self.selection_summary())
                    .style(theme.style(Role::Help)),
                Rect::new(area.x, area.y + 1, area.width, 1),
            );
        }
        self.render_body(frame, area, &theme);
        self.render_controls(frame, area, &theme);
        if self.modal.is_some() {
            self.render_modal(frame);
        }
    }

    fn render_body(&mut self, frame: &mut Frame, area: Rect, theme: &UiTheme) {
        let visible = self.visible();
        let height = (visible.len().max(1).saturating_add(2)).min(usize::from(
            area.height
                - TITLE_HEIGHT
                - DETAIL_HEIGHT
                - self.controls_height(),
        )) as u16;
        let table_area =
            Rect::new(area.x, area.y + TITLE_HEIGHT, area.width, height);
        self.render_table(frame, table_area, theme);
        let details =
            Rect::new(area.x, table_area.bottom(), area.width, DETAIL_HEIGHT);
        self.render_details(frame, details, theme);
        frame.render_widget(
            Paragraph::new(clean(&self.error)).style(theme.style(Role::Error)),
            Rect::new(
                details.x + 2,
                details.bottom() - 1,
                details.width.saturating_sub(2),
                1,
            ),
        );
    }

    fn controls_height(&self) -> u16 {
        if self.workspace == WorkspaceMode::Select {
            4
        } else {
            3
        }
    }

    fn render_controls(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        theme: &UiTheme,
    ) {
        self.render_filter(
            frame,
            Rect::new(
                area.x,
                area.bottom() - self.controls_height(),
                area.width,
                1,
            ),
            theme,
        );
        self.render_help(
            frame,
            Rect::new(
                area.x,
                area.bottom() - self.controls_height() + 1,
                area.width,
                1,
            ),
        );
        self.render_footer(
            frame,
            Rect::new(
                area.x,
                area.bottom() - self.controls_height() + 2,
                area.width,
                self.controls_height() - 2,
            ),
            theme,
        );
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
                "labeldeck {} — {}    {count} labels{}",
                if self.workspace == WorkspaceMode::Select {
                    "select"
                } else {
                    "edit"
                },
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
        let visible = self.visible();
        let table = self.table_widget(&visible, area.width, theme);
        self.table
            .select(visible.iter().position(|id| Some(*id) == self.selected));
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
            let message = if self.workspace == WorkspaceMode::Select
                && self.candidates().is_empty()
            {
                "No changes"
            } else if self.workspace == WorkspaceMode::Select
                && self.selected_only
                && self.candidates().iter().all(|r| !r.checked)
            {
                "No selected items"
            } else if !self.filter.is_empty() {
                "No filter matches"
            } else {
                "No labels — n to add"
            };
            frame.render_widget(
                Paragraph::new(message).style(theme.style(Role::Help)),
                self.rows,
            );
        }
    }

    fn table_widget(
        &self,
        visible: &[crate::edit::model::EntryId],
        width: u16,
        theme: &UiTheme,
    ) -> Table<'static> {
        let selecting = self.workspace == WorkspaceMode::Select;
        let candidates = self.candidates();
        let drafts = self.table_drafts(visible, &candidates);
        let (widths, header) = self.table_columns(&drafts, width);
        let rows: Vec<_> = visible
            .iter()
            .zip(&drafts)
            .map(|(id, draft)| {
                let mut cells = self.label_cells(draft, theme);
                if selecting
                    && let Some(row) = candidates.iter().find(|r| r.id == *id)
                {
                    cells.insert(
                        0,
                        Cell::from(if row.checked { "[x]" } else { "[ ]" }),
                    );
                    if let Some(group) = row.group {
                        cells.insert(
                            1,
                            Cell::from(match group {
                                super::ActionGroup::Create => "CREATE",
                                super::ActionGroup::Update => "UPDATE",
                                super::ActionGroup::Delete => "DELETE",
                            }),
                        );
                    }
                }
                Row::new(cells)
            })
            .collect();
        Table::new(rows, widths)
            .column_spacing(2)
            .highlight_spacing(HighlightSpacing::Always)
            .header(
                Row::new(header)
                    .style(theme.style(Role::Header))
                    .bottom_margin(1),
            )
            .style(theme.style(Role::DetailValue))
            .row_highlight_style(theme.style(Role::Selected))
            .highlight_symbol("> ")
    }

    fn table_drafts(
        &self,
        visible: &[crate::edit::model::EntryId],
        candidates: &[super::selection::Candidate],
    ) -> Vec<crate::edit::model::Draft> {
        visible
            .iter()
            .filter_map(|id| {
                if self.workspace == WorkspaceMode::Select {
                    candidates
                        .iter()
                        .find(|r| r.id == *id)
                        .map(|r| r.draft.clone())
                } else {
                    self.document
                        .entries()
                        .iter()
                        .find(|e| e.id == *id)
                        .map(|e| e.draft.clone())
                }
            })
            .collect()
    }
    fn table_columns(
        &self,
        drafts: &[crate::edit::model::Draft],
        width: u16,
    ) -> (Vec<Constraint>, Vec<&'static str>) {
        let selecting = self.workspace == WorkspaceMode::Select;
        let extra = if selecting {
            if self.session == SessionKind::Export {
                5
            } else {
                14
            }
        } else {
            0
        };
        let name_width = drafts
            .iter()
            .map(|d| Line::raw(clean(&d.name)).width())
            .max()
            .unwrap_or(0)
            .saturating_add(1)
            .clamp(12, 34)
            .min(usize::from(width.saturating_sub(28 + extra)))
            as u16;
        let mut widths = vec![
            Constraint::Length(name_width),
            Constraint::Length(
                if selecting && self.session != SessionKind::Export {
                    18
                } else {
                    10
                },
            ),
            Constraint::Fill(1),
        ];
        let mut header = vec!["LABEL", "COLOR", "DESCRIPTION"];
        if selecting {
            widths.insert(0, Constraint::Length(3));
            header.insert(0, "[ ]");
            if self.session != SessionKind::Export {
                widths.insert(1, Constraint::Length(6));
                header.insert(1, "ACTION");
            }
        }
        (widths, header)
    }

    fn label_cells(
        &self,
        draft: &crate::edit::model::Draft,
        theme: &UiTheme,
    ) -> Vec<Cell<'static>> {
        vec![
            Cell::from(clean(&draft.name)),
            Cell::from(Line::from(vec![
                self.swatch(&draft.color, theme),
                Span::raw(clean(&draft.color)),
            ])),
            Cell::from(clean(&draft.description)),
        ]
    }

    fn render_filter(&self, frame: &mut Frame, area: Rect, theme: &UiTheme) {
        frame.render_widget(
            Paragraph::new("Filter: ").style(theme.style(Role::Filter)),
            Rect::new(area.x, area.y, 8, 1),
        );
        let width = area.width
            - 8
            - if self.session == SessionKind::Edit {
                10
            } else {
                0
            };
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
                Mode::List if self.workspace == WorkspaceMode::Select => &[
                    ("↑↓", "navigate"),
                    ("Space", "toggle"),
                    ("/", "filter"),
                    ("Tab", "controls"),
                ],
                Mode::List => &[
                    ("↑↓", "navigate"),
                    ("Enter", "edit"),
                    ("n", "new"),
                    ("Del", "delete"),
                    ("/", "filter labels"),
                    ("Tab", "cycle buttons"),
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
                        format!(" {action}  "),
                        theme.style(Role::Help),
                    ),
                ]
            })
            .collect();
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}
