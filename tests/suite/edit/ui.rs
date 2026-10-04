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

pub(super) fn state(live: bool) -> UiState {
    UiState::new(
        Document::from_labels(vec![label("bug"), label("docs")]),
        "labels.json".into(),
        live,
        ColorLevel::NoColor,
    )
}
pub(super) fn key(state: &mut UiState, code: KeyCode) -> Option<UiAction> {
    state.handle(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}
pub(super) fn ctrl(state: &mut UiState, c: char) -> Option<UiAction> {
    state.handle(Event::Key(KeyEvent::new(
        KeyCode::Char(c),
        KeyModifiers::CONTROL,
    )))
}
pub(super) fn screen(state: &mut UiState, w: u16, h: u16) -> String {
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
    assert!(screen(&mut ui, 80, 24).contains("> Name"));
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
    assert_eq!(ui.document().entries().len(), 2);
    assert!(!ui.document().can_undo());
    key(&mut ui, KeyCode::Enter);
    assert!(screen(&mut ui, 80, 24).contains("must not be empty"));
    ui.handle(Event::Paste("docsnew\n\x1b\tlabel".into()));
    key(&mut ui, KeyCode::Enter);
    assert_eq!(
        ui.document().labels().unwrap().last().unwrap().name,
        "docsnewlabel"
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
fn filtered_rename_keeps_field_traversal_on_the_edited_label() {
    for neighbor in ["bug2", "docs"] {
        let mut ui = UiState::new(
            Document::from_labels(vec![label("bug"), label(neighbor)]),
            "labels.json".into(),
            false,
            ColorLevel::NoColor,
        );
        key(&mut ui, KeyCode::Char('/'));
        ui.handle(Event::Paste("bug".into()));
        key(&mut ui, KeyCode::Enter);
        key(&mut ui, KeyCode::Enter);
        for _ in 0..3 {
            key(&mut ui, KeyCode::Backspace);
        }
        ui.handle(Event::Paste("new".into()));
        key(&mut ui, KeyCode::Tab);
        assert!(screen(&mut ui, 80, 24).contains("> Color"));
        assert_eq!(ui.selected(), Some(0));
        key(&mut ui, KeyCode::Tab);
        assert!(screen(&mut ui, 80, 24).contains("> Description"));
        ui.handle(Event::Paste("correct row".into()));
        key(&mut ui, KeyCode::BackTab);
        assert!(screen(&mut ui, 80, 24).contains("> Color"));
        key(&mut ui, KeyCode::Tab);
        screen(&mut ui, 80, 24);
        key(&mut ui, KeyCode::Enter);
        screen(&mut ui, 80, 24);
        let labels = ui.document().labels().unwrap();
        assert_eq!(labels[0].name, "new");
        assert_eq!(labels[0].description, "correct row");
        assert_eq!(labels[1], label(neighbor));
        assert_eq!(ui.selected(), (neighbor == "bug2").then_some(1));
    }
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
    ui.handle(mouse(MouseEventKind::Down(MouseButton::Left), 2, 4));
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

#[test]
fn keyboard_buttons_support_history_apply_and_cancel_without_mouse() {
    let mut ui = state(false);
    key(&mut ui, KeyCode::Delete);
    key(&mut ui, KeyCode::Tab);
    key(&mut ui, KeyCode::Enter);
    assert_eq!(ui.document().labels().unwrap().len(), 2);
    key(&mut ui, KeyCode::Tab);
    key(&mut ui, KeyCode::Enter);
    assert_eq!(ui.document().labels().unwrap().len(), 1);
    key(&mut ui, KeyCode::Tab);
    key(&mut ui, KeyCode::Enter);
    assert!(screen(&mut ui, 80, 24).contains("Confirm Apply"));
    key(&mut ui, KeyCode::Esc);
    key(&mut ui, KeyCode::BackTab);
    key(&mut ui, KeyCode::BackTab);
    key(&mut ui, KeyCode::BackTab);
    assert!(matches!(
        key(&mut ui, KeyCode::Enter),
        Some(UiAction::Cancel)
    ));
}

#[test]
fn clickable_history_and_confirmation_buttons_follow_current_layout() {
    use super::rendering::{click, draw, locate};
    let mut ui = state(false);
    key(&mut ui, KeyCode::Delete);
    let buffer = draw(&mut ui, 80, 24);
    let undo = locate(&buffer, "[ Undo ]");
    click(&mut ui, undo.0, undo.1);
    assert_eq!(ui.document().labels().unwrap().len(), 2);
    let buffer = draw(&mut ui, 80, 24);
    let redo = locate(&buffer, "[ Redo ]");
    click(&mut ui, redo.0, redo.1);
    assert_eq!(ui.document().labels().unwrap().len(), 1);
    let buffer = draw(&mut ui, 80, 24);
    let apply = locate(&buffer, "[ Apply ]");
    click(&mut ui, apply.0, apply.1);
    let buffer = draw(&mut ui, 80, 24);
    let back = locate(&buffer, "[ Back ]");
    click(&mut ui, back.0, back.1);
    assert!(!screen(&mut ui, 80, 24).contains("Confirm Apply"));
    ctrl(&mut ui, 's');
    let buffer = draw(&mut ui, 80, 24);
    let apply = locate(&buffer, "[ Apply ]");
    assert!(matches!(
        click(&mut ui, apply.0, apply.1),
        Some(UiAction::Apply(_))
    ));
}

#[test]
fn invalid_apply_keeps_editor_open_and_input_mouse_never_mutates() {
    let mut ui = state(false);
    key(&mut ui, KeyCode::Enter);
    super::form::replace(&mut ui, "docs");
    key(&mut ui, KeyCode::Enter);
    ctrl(&mut ui, 's');
    assert!(screen(&mut ui, 80, 24).contains("duplicate label name"));
    key(&mut ui, KeyCode::Enter);
    screen(&mut ui, 80, 24);
    ui.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 70,
        row: 23,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(screen(&mut ui, 80, 24).contains("> Name"));
}

#[test]
fn release_keys_resize_focus_and_outside_clicks_do_not_edit_document() {
    let mut ui = state(false);
    screen(&mut ui, 80, 24);
    let mut event = KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE);
    event.kind = crossterm::event::KeyEventKind::Release;
    ui.handle(Event::Key(event));
    ui.handle(Event::Resize(100, 30));
    ui.handle(Event::FocusLost);
    ui.handle(Event::Paste("ignored".into()));
    ctrl(&mut ui, 'a');
    key(&mut ui, KeyCode::Char('x'));
    for kind in [
        MouseEventKind::Moved,
        MouseEventKind::Down(MouseButton::Left),
    ] {
        ui.handle(Event::Mouse(MouseEvent {
            kind,
            column: 79,
            row: 20,
            modifiers: KeyModifiers::NONE,
        }));
    }
    assert!(!ui.document().can_undo());
    screen(&mut ui, 20, 5);
    assert!(key(&mut ui, KeyCode::Char('x')).is_none());
    assert!(matches!(key(&mut ui, KeyCode::Esc), Some(UiAction::Cancel)));
}

#[test]
fn filter_typing_and_field_cycles_preserve_unicode_and_valid_selection() {
    let mut ui = state(false);
    key(&mut ui, KeyCode::Char('/'));
    for c in "docs".chars() {
        key(&mut ui, KeyCode::Char(c));
    }
    key(&mut ui, KeyCode::Enter);
    assert_eq!(ui.selected(), Some(1));
    key(&mut ui, KeyCode::Up);
    key(&mut ui, KeyCode::Char('e'));
    key(&mut ui, KeyCode::BackTab);
    key(&mut ui, KeyCode::Tab);
    ui.handle(Event::Paste("界é".into()));
    assert!(screen(&mut ui, 48, 16).contains("界"));
    key(&mut ui, KeyCode::Enter);
    assert_eq!(ui.document().labels().unwrap()[1].name, "docs界é");
}

#[test]
fn swatches_use_colour_only_when_capability_allows() {
    for level in [
        ColorLevel::TrueColor,
        ColorLevel::Ansi256,
        ColorLevel::Ansi16,
        ColorLevel::NoColor,
    ] {
        let mut ui = UiState::new(
            Document::from_labels(vec![label("bug")]),
            "colours".into(),
            false,
            level,
        );
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| ui.render(f)).unwrap();
        let cells: Vec<_> = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .filter(|c| c.symbol() == "█")
            .collect();
        assert_eq!(cells.len(), 4);
        assert_eq!(
            cells[0].fg == ratatui::style::Color::Reset,
            level == ColorLevel::NoColor
        );
        ctrl(&mut ui, 's');
        assert!(screen(&mut ui, 80, 24).contains("No changes to apply."));
        key(&mut ui, KeyCode::Char('x'));
        key(&mut ui, KeyCode::Right);
        key(&mut ui, KeyCode::Left);
        key(&mut ui, KeyCode::Esc);
    }
}

#[test]
fn event_loop_drives_cancel_apply_and_input_failure_with_test_backend() {
    use labeldeck::edit::ui::drive;
    for apply in [false, true] {
        let mut ui = state(false);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let keys = if apply {
            vec![
                (KeyCode::Delete, KeyModifiers::NONE),
                (KeyCode::Char('s'), KeyModifiers::CONTROL),
                (KeyCode::Tab, KeyModifiers::NONE),
                (KeyCode::Enter, KeyModifiers::NONE),
            ]
        } else {
            vec![(KeyCode::Esc, KeyModifiers::NONE)]
        };
        let mut keys = keys.into_iter();
        let result = drive(&mut terminal, &mut ui, || {
            let (code, modifiers) =
                keys.next().expect("loop must finish before events run out");
            Ok(Event::Key(KeyEvent::new(code, modifiers)))
        })
        .unwrap();
        assert_eq!(result.is_some(), apply);
    }
    let mut ui = state(false);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let error = drive(&mut terminal, &mut ui, || {
        Err(std::io::Error::other("event read failed"))
    })
    .unwrap_err();
    assert_eq!(error.to_string(), "event read failed");
}

#[test]
fn scroll_offset_and_mouse_selection_match_visible_rows_after_resize() {
    let mut ui = UiState::new(
        Document::from_labels(
            (0..50).map(|i| label(&format!("label-{i:02}"))).collect(),
        ),
        "scroll".into(),
        false,
        ColorLevel::NoColor,
    );
    for _ in 0..30 {
        key(&mut ui, KeyCode::Down);
    }
    let rendered = screen(&mut ui, 80, 24);
    assert!(rendered.contains("label-30"));
    ui.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 2,
        row: 3,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(ui.selected().unwrap() > 0);
    assert!(screen(&mut ui, 48, 16).contains("label-"));
}
