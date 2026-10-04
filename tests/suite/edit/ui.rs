use super::label;
use colored_text::ColorLevel;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use labeldeck::edit::{
    model::Document,
    ui::{UiAction, UiState},
};
use ratatui::{Terminal, backend::TestBackend};

fn state(live: bool) -> UiState {
    UiState::new(
        Document::from_labels(vec![label("bug"), label("docs")]),
        "labels.json".into(),
        live,
        ColorLevel::NoColor,
    )
}
fn key(state: &mut UiState, code: KeyCode) -> Option<UiAction> {
    state.handle(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}
fn ctrl(state: &mut UiState, c: char) -> Option<UiAction> {
    state.handle(Event::Key(KeyEvent::new(
        KeyCode::Char(c),
        KeyModifiers::CONTROL,
    )))
}
fn screen(state: &mut UiState, w: u16, h: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| state.render(f)).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect()
}

#[test]
fn normal_empty_and_small_screens_render_safely() {
    let mut ui = state(false);
    let normal = screen(&mut ui, 80, 24);
    for text in [
        "bug", "docs", "ededed", "Undo", "Redo", "Apply", "Cancel", ">",
    ] {
        assert!(normal.contains(text), "{text}: {normal}");
    }
    for (w, h) in [(0, 0), (1, 1), (20, 5), (47, 15)] {
        let small = screen(&mut ui, w, h);
        if w >= 20 {
            assert!(small.contains("terminal too small"));
        }
    }
    let mut empty = UiState::new(
        Document::from_labels(vec![]),
        "empty".into(),
        false,
        ColorLevel::NoColor,
    );
    assert!(screen(&mut empty, 80, 24).contains("No labels"));
    assert!(key(&mut empty, KeyCode::Enter).is_none());
}

#[test]
fn navigation_filtering_and_selection_repair_are_presentation_only() {
    let mut ui = state(false);
    key(&mut ui, KeyCode::Down);
    assert_eq!(ui.selected(), Some(1));
    key(&mut ui, KeyCode::PageUp);
    assert_eq!(ui.selected(), Some(0));
    key(&mut ui, KeyCode::PageDown);
    assert_eq!(ui.selected(), Some(1));
    key(&mut ui, KeyCode::Char('/'));
    ui.handle(Event::Paste("BUG".into()));
    assert!(screen(&mut ui, 80, 24).contains("Filter"));
    assert_eq!(ui.selected(), Some(0));
    key(&mut ui, KeyCode::Enter);
    assert!(!ui.document().can_undo());
    key(&mut ui, KeyCode::Char('/'));
    ui.handle(Event::Paste("missing".into()));
    assert!(screen(&mut ui, 80, 24).contains("No filter matches"));
    assert_eq!(ui.selected(), None);
    key(&mut ui, KeyCode::Esc);
    assert_eq!(ui.selected(), Some(0));
}

#[test]
fn staged_edits_commit_once_and_history_discards_stale_input() {
    let mut ui = state(false);
    key(&mut ui, KeyCode::Enter);
    assert!(screen(&mut ui, 80, 24).contains("Editing Name"));
    ui.handle(Event::Paste(" new".into()));
    assert!(!ui.document().can_undo());
    key(&mut ui, KeyCode::Enter);
    assert_eq!(ui.document().labels().unwrap()[0].name, "bug new");
    key(&mut ui, KeyCode::Enter);
    ui.handle(Event::Paste(" stale".into()));
    ctrl(&mut ui, 'z');
    assert_eq!(ui.document().labels().unwrap()[0].name, "bug");
    ctrl(&mut ui, 'y');
    assert_eq!(ui.document().labels().unwrap()[0].name, "bug new");
    key(&mut ui, KeyCode::Enter);
    ui.handle(Event::Paste(" discard".into()));
    key(&mut ui, KeyCode::Esc);
    assert_eq!(ui.document().labels().unwrap()[0].name, "bug new");
}

#[test]
fn new_row_is_visible_under_filter_and_invalid_name_stays_in_input() {
    let mut ui = state(false);
    key(&mut ui, KeyCode::Char('/'));
    ui.handle(Event::Paste("docs".into()));
    key(&mut ui, KeyCode::Enter);
    key(&mut ui, KeyCode::Char('n'));
    let id = ui.selected().unwrap();
    assert!(id > 1);
    key(&mut ui, KeyCode::Enter);
    assert!(screen(&mut ui, 80, 24).contains("must not be empty"));
    ui.handle(Event::Paste("new\n\x1b\tlabel".into()));
    key(&mut ui, KeyCode::Enter);
    assert_eq!(
        ui.document().labels().unwrap().last().unwrap().name,
        "newlabel"
    );
    key(&mut ui, KeyCode::Delete);
    assert_eq!(ui.document().labels().unwrap().len(), 2);
    ctrl(&mut ui, 'z');
    assert_eq!(ui.document().labels().unwrap().len(), 3);
}

#[test]
fn tab_cursor_and_field_switching_commit_colour_and_description() {
    let mut ui = state(false);
    key(&mut ui, KeyCode::Enter);
    key(&mut ui, KeyCode::Tab);
    for _ in 0..6 {
        key(&mut ui, KeyCode::Backspace);
    }
    ui.handle(Event::Paste("abcdef".into()));
    key(&mut ui, KeyCode::Tab);
    ui.handle(Event::Paste("description".into()));
    key(&mut ui, KeyCode::Left);
    key(&mut ui, KeyCode::Char('!'));
    key(&mut ui, KeyCode::BackTab);
    key(&mut ui, KeyCode::Enter);
    let labels = ui.document().labels().unwrap();
    assert_eq!(labels[0].color.as_str(), "abcdef");
    assert_eq!(labels[0].description, "descriptio!n");
}

#[test]
fn apply_requires_confirmation_and_warns_for_live_deletes() {
    let mut ui = state(true);
    key(&mut ui, KeyCode::Delete);
    ctrl(&mut ui, 's');
    let confirm = screen(&mut ui, 80, 24);
    assert!(confirm.contains("1 deleted"));
    assert!(confirm.contains("issues and pull requests"));
    key(&mut ui, KeyCode::Enter); // defaults to Back
    assert!(!screen(&mut ui, 80, 24).contains("Confirm Apply"));
    ctrl(&mut ui, 's');
    key(&mut ui, KeyCode::Tab);
    assert!(matches!(
        key(&mut ui, KeyCode::Enter),
        Some(UiAction::Apply(_))
    ));
}

#[test]
fn mouse_buttons_rows_and_wheel_use_rendered_hit_areas() {
    let mut ui = state(false);
    screen(&mut ui, 80, 24);
    let mouse = |kind, x, y| {
        Event::Mouse(MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        })
    };
    ui.handle(mouse(MouseEventKind::Down(MouseButton::Left), 2, 3));
    assert_eq!(ui.selected(), Some(1));
    ui.handle(mouse(MouseEventKind::ScrollUp, 2, 3));
    assert_eq!(ui.selected(), Some(0));
    ui.handle(mouse(MouseEventKind::ScrollDown, 2, 3));
    assert_eq!(ui.selected(), Some(1));
    assert!(matches!(
        ui.handle(mouse(MouseEventKind::Down(MouseButton::Left), 70, 23)),
        Some(UiAction::Cancel)
    ));
}

#[test]
fn quit_escape_and_ctrl_c_cancel_cleanly_from_submodes() {
    for code in [KeyCode::Char('q'), KeyCode::Esc] {
        assert!(matches!(
            key(&mut state(false), code),
            Some(UiAction::Cancel)
        ));
    }
    let mut ui = state(false);
    key(&mut ui, KeyCode::Enter);
    assert!(matches!(ctrl(&mut ui, 'c'), Some(UiAction::Cancel)));
    let mut ui = state(false);
    screen(&mut ui, 20, 5);
    assert!(matches!(ctrl(&mut ui, 'c'), Some(UiAction::Cancel)));
}
