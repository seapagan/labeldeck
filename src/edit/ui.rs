//! Small terminal editor with independently testable rendering and events.

mod selection;
pub use selection::ActionGroup;
use selection::Selection;
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
    let action = drive_session(terminal, state, &mut read, |_, _| {})?;
    match action {
        UiAction::Cancel => Ok(None),
        UiAction::Apply(document) => Ok(Some(document)),
        _ => Err(std::io::Error::other(
            "interactive result requires session driver",
        )),
    }
}

pub fn drive_session<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    state: &mut UiState,
    mut read: impl FnMut() -> std::io::Result<crossterm::event::Event>,
    mut service: impl FnMut(&mut UiState, Option<super::session::SaveTarget>),
) -> std::io::Result<UiAction> {
    loop {
        service(state, None);
        terminal
            .draw(|frame| state.render(frame))
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        match state.handle(read()?) {
            Some(UiAction::Save(target)) => service(state, Some(target)),
            Some(action) => return Ok(action),
            None => {}
        }
    }
}

pub(crate) fn run_session(
    state: &mut UiState,
    service: impl FnMut(&mut UiState, Option<super::session::SaveTarget>),
) -> std::io::Result<UiAction> {
    terminal::run(state, service)
}

pub enum UiAction {
    Cancel,
    Apply(Document),
    Finish(FinalSelection),
    Save(super::session::SaveTarget),
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

struct Modal {
    kind: ModalKind,
    choice: usize,
}
enum ModalKind {
    Apply(ChangeSummary),
    Finish(FinalSelection),
    Reset,
    Save,
}

#[derive(Debug, Clone)]
pub enum FinalSelection {
    Export(Vec<crate::labels::Label>),
    Plan(crate::plan::Plan),
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
}

pub struct UiState {
    document: Document,
    session: SessionKind,
    workspace: WorkspaceMode,
    selection: Option<Selection>,
    selected_only: bool,
    destination: Option<std::path::PathBuf>,
    save_choices: [bool; 2],
    save_paths: [String; 2],
    saves: Vec<super::session::SaveRecord>,
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
            selection: None,
            selected_only: false,
            destination: None,
            save_choices: [true; 2],
            save_paths: [
                "./labels.json".into(),
                "<config dir>/labels.json".into(),
            ],
            saves: Vec::new(),
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
            (80, 16)
        }
    }
    fn too_small(&self, width: u16, height: u16) -> bool {
        let (minimum_width, minimum_height) = self.minimum_size();
        width < minimum_width || height < minimum_height
    }
    pub fn saves(&self) -> &[super::session::SaveRecord] {
        &self.saves
    }
    pub fn set_status(&mut self, message: String) {
        self.error = message;
    }
    pub fn set_save_choices(
        &mut self,
        choices: [bool; 2],
        paths: [String; 2],
    ) {
        self.save_choices = choices;
        self.save_paths = paths;
        if let Some(Modal {
            kind: ModalKind::Save,
            choice,
        }) = &mut self.modal
            && *choice < 2
            && !choices[*choice]
        {
            *choice = 2;
        }
    }
    pub fn record_save(
        &mut self,
        result: crate::error::Result<super::session::SaveRecord>,
    ) {
        match result {
            Ok(record) => {
                self.error = format!(
                    "Saved {}{}",
                    record.path.display(),
                    if record.warning.is_some() {
                        " (durability unconfirmed)"
                    } else {
                        ""
                    }
                );
                self.saves.push(record);
            }
            Err(error) => self.error = error.to_string(),
        }
    }
    fn open_save(&mut self) {
        if self.workspace != WorkspaceMode::Edit
            || !matches!(self.mode, Mode::List)
            || self.modal.is_some()
        {
            return;
        }
        if let Err(error) = self.document.labels() {
            self.error = error;
            return;
        }
        self.modal = Some(Modal {
            kind: ModalKind::Save,
            choice: 2,
        });
        self.buttons.clear();
    }
    pub fn document(&self) -> &Document {
        &self.document
    }
    pub fn selected(&self) -> Option<EntryId> {
        self.selected
    }
    fn repair_selection(&mut self) {
        self.remember_entries();
        if let Mode::Edit(form) = &self.mode {
            if let Some(id) = form.id {
                self.selected = Some(id);
            }
            return;
        }
        let visible = self.visible();
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
    let host = super::session::SaveHost::new(
        crate::commands::config_dir()
            .map_err(|error| std::io::Error::other(error.to_string()))?,
        if live { None } else { Some(title.into()) },
    );
    let result = super::session::run(
        UiState::new(document, title.into(), live, level),
        host,
    );
    result.report_saves();
    match result
        .outcome
        .map_err(|error| std::io::Error::other(error.to_string()))?
    {
        UiAction::Cancel => Ok(None),
        UiAction::Apply(document) => Ok(Some(document)),
        _ => Err(std::io::Error::other("unexpected editor result")),
    }
}
