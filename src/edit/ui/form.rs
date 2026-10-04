use super::Field;
use crate::{
    edit::model::{Draft, EntryId},
    labels::{LABEL_COLOR_LEN, MAX_DESCRIPTION_LEN, MAX_NAME_LEN},
};
use tui_input::{Input, InputRequest};

pub(super) const DETAIL_HEIGHT: u16 = 6;
const LABEL_WIDTH: u16 = 15;
const SWATCH_WIDTH: u16 = 2;
const VALUE_OFFSET: u16 = LABEL_WIDTH + SWATCH_WIDTH;

#[cfg(test)]
mod tests;

pub(super) struct EditForm {
    pub id: Option<EntryId>,
    pub field: Field,
    pub inputs: [Input; 3],
}

impl EditForm {
    pub fn new(id: Option<EntryId>, draft: Draft) -> Self {
        let mut form = Self {
            id,
            field: Field::Name,
            inputs: [draft.name, draft.color, draft.description]
                .map(Input::new),
        };
        form.clamp_color_cursor();
        form
    }

    pub fn input(&mut self) -> &mut Input {
        &mut self.inputs[self.field as usize]
    }

    pub fn handle(&mut self, request: InputRequest) -> Result<(), String> {
        if matches!(request, InputRequest::GoToNextChar)
            && self.field == Field::Color
            && self.input().cursor() == LABEL_COLOR_LEN - 1
        {
            return Ok(());
        }
        if let InputRequest::InsertChar(ch) = request {
            self.insert(&ch.to_string())
        } else {
            self.input().handle(request);
            self.clamp_color_cursor();
            Ok(())
        }
    }

    pub fn insert(&mut self, text: &str) -> Result<(), String> {
        let text = if self.field == Field::Color {
            if !text.bytes().all(|ch| ch.is_ascii_hexdigit()) {
                return Err(
                    "Color must contain only hexadecimal digits (0-9, A-F)."
                        .into(),
                );
            }
            text.to_ascii_lowercase()
        } else {
            text.chars().filter(|ch| !ch.is_control()).collect()
        };
        let (limit, message) = match self.field {
            Field::Name => (
                MAX_NAME_LEN,
                format!(
                    "Label names are limited to {MAX_NAME_LEN} characters."
                ),
            ),
            Field::Color => (
                LABEL_COLOR_LEN,
                format!(
                    "Color is limited to {LABEL_COLOR_LEN} hexadecimal digits."
                ),
            ),
            Field::Description => (
                MAX_DESCRIPTION_LEN,
                format!(
                    "Descriptions are limited to {MAX_DESCRIPTION_LEN} characters."
                ),
            ),
        };
        if self
            .input()
            .value()
            .chars()
            .count()
            .saturating_add(text.chars().count())
            > limit
        {
            return Err(message);
        }
        for ch in text.chars() {
            self.input().handle(InputRequest::InsertChar(ch));
        }
        self.clamp_color_cursor();
        Ok(())
    }

    fn clamp_color_cursor(&mut self) {
        let input = &mut self.inputs[Field::Color as usize];
        if input.cursor() >= LABEL_COLOR_LEN {
            input.handle(InputRequest::SetCursor(LABEL_COLOR_LEN - 1));
        }
    }

    pub fn draft(&self) -> Draft {
        Draft {
            name: self.inputs[0].value().into(),
            color: self.inputs[1].value().into(),
            description: self.inputs[2].value().into(),
        }
    }

    pub fn validate(&mut self) -> Result<Draft, String> {
        let draft = self.draft();
        // Use domain validation for both messages and ordering.
        if let Err(error) = draft.label() {
            self.field =
                if crate::labels::LabelColor::parse(&draft.color).is_err() {
                    Field::Color
                } else if draft.name.is_empty()
                    || draft.name.chars().count() > crate::labels::MAX_NAME_LEN
                {
                    Field::Name
                } else {
                    Field::Description
                };
            return Err(error);
        }
        Ok(draft)
    }
}

impl super::UiState {
    pub(super) fn render_details(
        &mut self,
        frame: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
        theme: &super::theme::UiTheme,
    ) {
        use super::{Mode, theme::Role};
        use ratatui::{layout::Rect, text::Line, widgets::Paragraph};
        let (draft, form) = if let Mode::Edit(form) = &self.mode {
            (form.draft(), Some(form))
        } else if let Some(entry) = self
            .document
            .entries()
            .iter()
            .find(|e| Some(e.id) == self.selected)
        {
            (entry.draft.clone(), None)
        } else {
            return;
        };
        if form.is_none() {
            frame.render_widget(
                Paragraph::new("─".repeat(usize::from(area.width)))
                    .style(theme.style(Role::Separator)),
                Rect::new(area.x, area.y, area.width, 1),
            );
        }
        for (index, (field, value)) in [
            (Field::Name, &draft.name),
            (Field::Color, &draft.color),
            (Field::Description, &draft.description),
        ]
        .into_iter()
        .enumerate()
        {
            let y = area.y + 1 + index as u16;
            let x = area.x + 2;
            let value_width = if field == Field::Color {
                LABEL_COLOR_LEN
            } else if form.is_some() {
                Line::raw(super::render::clean(value))
                    .width()
                    .saturating_add(1)
            } else {
                usize::from(area.right().saturating_sub(x + VALUE_OFFSET))
            };
            let width = (usize::from(VALUE_OFFSET) + value_width)
                .min(usize::from(area.right().saturating_sub(x)))
                as u16;
            self.fields[index] = Rect::new(x, y, width, 1);
            if form.is_some() {
                frame.render_widget(
                    Paragraph::new("│").style(theme.style(Role::EditAccent)),
                    Rect::new(area.x, y, 1, 1),
                );
            }
            self.render_field(
                frame,
                self.fields[index],
                theme,
                field,
                value,
                form,
            );
        }
    }
    fn render_field(
        &self,
        frame: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
        theme: &super::theme::UiTheme,
        field: Field,
        value: &str,
        form: Option<&EditForm>,
    ) {
        use super::{render::clean, theme::Role};
        use ratatui::{layout::Rect, widgets::Paragraph};
        let focused = form.is_some_and(|f| f.field == field);
        let y = area.y;
        let index = field as usize;
        let role = if focused {
            Role::FocusedField
        } else if form.is_some() {
            Role::Field
        } else {
            Role::DetailValue
        };
        frame.render_widget(
            Paragraph::new(field.title())
                .style(theme.style(Role::DetailLabel)),
            Rect::new(area.x, y, LABEL_WIDTH, 1),
        );
        let x = area.x + VALUE_OFFSET;
        if field == Field::Color {
            frame.render_widget(
                Paragraph::new(self.swatch(value, theme)),
                Rect::new(x - SWATCH_WIDTH, y, SWATCH_WIDTH, 1),
            );
        }
        let width = area.right().saturating_sub(x);
        let scroll = form.map_or(0, |f| {
            if field == Field::Color {
                // All six digits fit, and the logical cursor stays inside them.
                0
            } else {
                f.inputs[index].visual_scroll(width.saturating_sub(1) as usize)
            }
        });
        frame.render_widget(
            Paragraph::new(clean(value))
                .style(theme.style(role))
                .scroll((0, scroll as u16)),
            Rect::new(x, y, width, 1),
        );
        if focused && self.modal.is_none() {
            let cursor = form.expect("focused form").inputs[index]
                .visual_cursor()
                .saturating_sub(scroll)
                .min(width.saturating_sub(1) as usize);
            frame.set_cursor_position((x + cursor as u16, y));
        }
    }
}
