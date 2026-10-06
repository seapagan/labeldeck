//! Small terminal editor with independently testable rendering and events.

mod controls;
use controls::Control;
mod events;
mod form;
mod modal;
mod render;
mod terminal;
mod theme;

use super::{
    model::{Document, EntryId, visible_ids},
    plan::ChangeSummary,
};
use colored_text::{ColorLevel, ColorizeConfig, RenderTarget};
use ratatui::{layout::Rect, widgets::TableState};
use tui_input::Input;

/// Drive the same rendering/event loop with any synchronous terminal backend.
/// Source persistence remains outside this loop, after terminal restoration.
pub fn drive<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    state: &mut UiState,
    mut read: impl FnMut() -> std::io::Result<crossterm::event::Event>,
) -> std::io::Result<Option<Document>> {
    loop {
        terminal
            .draw(|frame| state.render(frame))
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        match state.handle(read()?) {
            Some(UiAction::Cancel) => return Ok(None),
            Some(UiAction::Apply(document)) => return Ok(Some(document)),
            None => {}
        }
    }
}

pub enum UiAction {
    Cancel,
    Apply(Document),
}

#[derive(Clone, Copy, PartialEq, Eq)]
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
    Edit(form::EditForm),
    Filter { before: String, input: Input },
}

enum Modal {
    Apply(ConfirmApply),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Edit,
    Export,
    Sync,
    Copy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceMode {
    Edit,
    Select,
}

struct ConfirmApply {
    summary: ChangeSummary,
    apply: bool,
}

pub struct UiState {
    document: Document,
    session: SessionKind,
    workspace: WorkspaceMode,
    title: String,
    live: bool,
    level: ColorLevel,
    selected: Option<EntryId>,
    filter: String,
    mode: Mode,
    modal: Option<Modal>,
    error: String,
    button: Option<Control>,
    buttons: Vec<Rect>,
    rows: Rect,
    fields: [Rect; 3],
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
            session: SessionKind::Edit,
            workspace: WorkspaceMode::Edit,
            title,
            live,
            level,
            selected,
            filter: String::new(),
            mode: Mode::List,
            modal: None,
            error: String::new(),
            button: None,
            buttons: Vec::new(),
            rows: Rect::default(),
            fields: [Rect::default(); 3],
            table: TableState::default(),
            small: false,
        }
    }
    pub fn session(&self) -> SessionKind {
        self.session
    }
    pub fn workspace(&self) -> WorkspaceMode {
        self.workspace
    }
    pub fn minimum_size(&self) -> (u16, u16) {
        if self.session == SessionKind::Edit {
            (48, 16)
        } else {
            (60, 16)
        }
    }
    fn too_small(&self, width: u16, height: u16) -> bool {
        let (minimum_width, minimum_height) = self.minimum_size();
        width < minimum_width || height < minimum_height
    }
    pub fn document(&self) -> &Document {
        &self.document
    }
    pub fn selected(&self) -> Option<EntryId> {
        self.selected
    }
    fn repair_selection(&mut self) {
        if let Mode::Edit(form) = &self.mode {
            if let Some(id) = form.id {
                self.selected = Some(id);
            }
            return;
        }
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
