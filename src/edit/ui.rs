//! Small terminal editor with independently testable rendering and events.

mod events;
mod render;
mod terminal;

use super::{
    model::{Document, EntryId, visible_ids},
    plan::ChangeSummary,
};
use colored_text::{ColorLevel, ColorizeConfig, RenderTarget};
use ratatui::{layout::Rect, widgets::TableState};
use tui_input::Input;

pub enum UiAction {
    Cancel,
    Apply(Document),
}

#[derive(Clone, Copy)]
enum Field {
    Name,
    Color,
    Description,
}

impl Field {
    fn title(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Color => "Color",
            Self::Description => "Description",
        }
    }
    fn next(self, back: bool) -> Self {
        match (self, back) {
            (Self::Name, false) | (Self::Description, true) => Self::Color,
            (Self::Color, false) | (Self::Name, true) => Self::Description,
            _ => Self::Name,
        }
    }
}

enum Mode {
    List,
    Edit {
        id: EntryId,
        field: Field,
        input: Input,
    },
    Filter {
        before: String,
        input: Input,
    },
    Confirm {
        summary: ChangeSummary,
        apply: bool,
    },
}

pub struct UiState {
    document: Document,
    title: String,
    live: bool,
    level: ColorLevel,
    selected: Option<EntryId>,
    filter: String,
    mode: Mode,
    error: String,
    button: Option<usize>,
    buttons: Vec<Rect>,
    rows: Rect,
    table: TableState,
    small: bool,
}

impl UiState {
    pub fn new(
        document: Document,
        title: String,
        live: bool,
        level: ColorLevel,
    ) -> Self {
        let selected = visible_ids(&document, "").first().copied();
        Self {
            document,
            title,
            live,
            level,
            selected,
            filter: String::new(),
            mode: Mode::List,
            error: String::new(),
            button: None,
            buttons: Vec::new(),
            rows: Rect::default(),
            table: TableState::default(),
            small: false,
        }
    }
    pub fn document(&self) -> &Document {
        &self.document
    }
    pub fn selected(&self) -> Option<EntryId> {
        self.selected
    }
    fn repair_selection(&mut self) {
        let visible = visible_ids(&self.document, &self.filter);
        if self.selected.is_none_or(|id| !visible.contains(&id)) {
            self.selected = visible.first().copied();
        }
    }
}

pub fn run(
    document: Document,
    title: &str,
    live: bool,
) -> std::io::Result<Option<Document>> {
    let level = ColorizeConfig::color_level(RenderTarget::Stdout);
    terminal::run(UiState::new(document, title.into(), live, level))
}
