use colored_text::ColorLevel;
use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy)]
pub(super) enum Role {
    Title,
    Header,
    Separator,
    Selected,
    DetailLabel,
    DetailValue,
    Field,
    FocusedField,
    Filter,
    Help,
    KeyHint,
    Error,
    Warning,
    Apply,
    Cancel,
    Undo,
    Redo,
    Disabled,
    ModalBorder,
    ModalTitle,
    ModalBody,
}

pub(super) struct UiTheme {
    colour: bool,
    dimmed: bool,
}

impl UiTheme {
    pub fn new(level: ColorLevel, dimmed: bool) -> Self {
        Self {
            colour: level != ColorLevel::NoColor,
            dimmed,
        }
    }

    pub fn style(&self, role: Role) -> Style {
        use Role::*;
        let mut style = Style::default();
        if self.colour {
            style = match role {
                Title | Header | DetailLabel | Filter | KeyHint | Undo
                | Redo | ModalBorder | ModalTitle => style.fg(Color::Cyan),
                Apply => style.fg(Color::Green),
                Cancel | Warning => style.fg(Color::Yellow),
                Error => style.fg(Color::Red),
                Separator => style.fg(Color::DarkGray),
                Selected | FocusedField => style.bg(Color::DarkGray),
                _ => style,
            };
        }
        style = match role {
            Title | Header | Selected | FocusedField | KeyHint | Error
            | Warning | Apply | ModalTitle => {
                style.add_modifier(Modifier::BOLD)
            }
            Help | Disabled => style.add_modifier(Modifier::DIM),
            _ => style,
        };
        if matches!(role, Field | FocusedField) {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        if self.dimmed {
            style = style
                .bg(Color::Reset)
                .remove_modifier(Modifier::BOLD)
                .add_modifier(Modifier::DIM);
        }
        style
    }

    pub fn button(&self, role: Role, enabled: bool, focused: bool) -> Style {
        let mut style =
            self.style(if enabled { role } else { Role::Disabled });
        if focused {
            style = style.add_modifier(Modifier::UNDERLINED);
            if enabled {
                style = style.add_modifier(Modifier::BOLD);
            }
            if self.colour && !self.dimmed {
                style = style.bg(Color::DarkGray);
            }
        }
        style
    }
}
