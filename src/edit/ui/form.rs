use super::Field;
use crate::{
    edit::model::{Draft, EntryId},
    labels::{LABEL_COLOR_LEN, MAX_DESCRIPTION_LEN, MAX_NAME_LEN},
};
use tui_input::{Input, InputRequest};

pub(super) struct EditForm {
    pub id: Option<EntryId>,
    pub field: Field,
    pub inputs: [Input; 3],
}

impl EditForm {
    pub fn new(id: Option<EntryId>, draft: Draft) -> Self {
        Self {
            id,
            field: Field::Name,
            inputs: [draft.name, draft.color, draft.description]
                .map(Input::new),
        }
    }

    pub fn input(&mut self) -> &mut Input {
        &mut self.inputs[self.field as usize]
    }

    pub fn handle(&mut self, request: InputRequest) -> Result<(), String> {
        if let InputRequest::InsertChar(ch) = request {
            self.insert(&ch.to_string())
        } else {
            self.input().handle(request);
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
        Ok(())
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
            let prefix = 15 + if field == Field::Color { 2 } else { 0 };
            let width = if form.is_some() {
                (prefix
                    + Line::raw(super::render::clean(value))
                        .width()
                        .saturating_add(1)
                        .max(12))
                .min(usize::from(area.right() - x)) as u16
            } else {
                area.right() - x
            };
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
            Rect::new(area.x, y, 15, 1),
        );
        let mut x = area.x + 15;
        if field == Field::Color {
            frame.render_widget(
                Paragraph::new(self.swatch(value, theme)),
                Rect::new(x, y, 2, 1),
            );
            x += 2;
        }
        let width = area.right().saturating_sub(x);
        let scroll = form.map_or(0, |f| {
            f.inputs[index].visual_scroll(width.saturating_sub(1) as usize)
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
