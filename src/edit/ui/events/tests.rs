use super::*;
use crate::edit::ui::Field;
use colored_text::ColorLevel;
use ratatui::{Terminal, backend::TestBackend, layout::Rect};

fn state() -> UiState {
    let label = Draft {
        name: "bug".into(),
        color: "ededed".into(),
        description: String::new(),
    }
    .label()
    .unwrap();
    UiState::new(
        crate::edit::model::Document::from_labels(vec![label]),
        "deck".into(),
        false,
        ColorLevel::NoColor,
    )
}

fn key(
    ui: &mut UiState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Option<UiAction> {
    ui.handle(Event::Key(KeyEvent::new(code, modifiers)))
}

fn ctrl(ui: &mut UiState, ch: char) -> Option<UiAction> {
    key(ui, KeyCode::Char(ch), KeyModifiers::CONTROL)
}

fn click(ui: &mut UiState, x: u16, y: u16) -> Option<UiAction> {
    ui.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }))
}

#[test]
fn modified_list_shortcuts_never_fall_through() {
    for modifiers in [
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
        KeyModifiers::SUPER,
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ] {
        for ch in ['q', 'n', 'e', '/', 'c', 's', 'z', 'y'] {
            if modifiers == KeyModifiers::CONTROL && "cszy".contains(ch) {
                continue;
            }
            let mut ui = state();
            assert!(key(&mut ui, KeyCode::Char(ch), modifiers).is_none());
            assert!(matches!(ui.mode, Mode::List));
            assert!(ui.modal.is_none());
            assert!(!ui.document.can_undo());
            assert!(ui.message.is_none());
        }
    }
}

#[test]
fn control_history_and_apply_route_to_committed_document() {
    let mut ui = state();
    key(&mut ui, KeyCode::Delete, KeyModifiers::NONE);
    ctrl(&mut ui, 'z');
    assert_eq!(ui.document.labels().unwrap().len(), 1);
    ctrl(&mut ui, 'y');
    assert!(ui.document.labels().unwrap().is_empty());
    ctrl(&mut ui, 's');
    assert!(ui.modal.is_some());
    assert!(matches!(ctrl(&mut ui, 'c'), Some(UiAction::Cancel)));
}

#[test]
fn unavailable_apply_cannot_be_activated() {
    let mut ui = state();
    assert!(!ui.apply_available());
    assert!(ui.activate(Control::Apply).is_none());
    assert!(ui.modal.is_none());
    assert!(matches!(ui.mode, Mode::List));
    assert!(!ui.document.can_undo());
}

#[test]
fn plain_printable_keys_follow_form_and_filter_routing() {
    let mut ui = state();
    ui.start_edit();
    key(&mut ui, KeyCode::Char('n'), KeyModifiers::NONE);
    let Mode::Edit(form) = &ui.mode else {
        panic!("expected form")
    };
    assert_eq!(form.draft().name, "bugn");
    key(&mut ui, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut ui, KeyCode::Char('/'), KeyModifiers::NONE);
    assert!(key(&mut ui, KeyCode::Char('q'), KeyModifiers::NONE).is_none());
    assert_eq!(ui.filter, "q");
    assert!(matches!(ui.mode, Mode::Filter { .. }));
    assert_eq!(ui.document.labels().unwrap()[0].name, "bug");
    assert!(!ui.document.can_undo());
}

#[test]
fn history_from_edit_discards_draft_before_undo_and_redo() {
    for ch in ['z', 'y'] {
        let mut ui = state();
        let mut draft = ui.document.entries()[0].draft.clone();
        draft.name = "committed".into();
        ui.document.commit(0, draft).unwrap();
        if ch == 'y' {
            ui.document.undo();
        }
        ui.start_edit();
        ui.handle(Event::Paste(" stale".into()));
        ctrl(&mut ui, ch);
        assert!(matches!(ui.mode, Mode::List));
        let expected = if ch == 'z' { "bug" } else { "committed" };
        assert_eq!(ui.document.labels().unwrap()[0].name, expected);
        ui.start_edit();
        let Mode::Edit(form) = &ui.mode else {
            panic!("expected form")
        };
        assert_eq!(form.draft().name, expected);
    }
}

#[test]
fn apply_in_submodes_preserves_input_and_history() {
    for opener in ['e', '/'] {
        let mut ui = state();
        ui.document
            .create(Draft {
                name: "new".into(),
                color: "ededed".into(),
                description: String::new(),
            })
            .unwrap();
        key(&mut ui, KeyCode::Char(opener), KeyModifiers::NONE);
        ui.handle(Event::Paste(" draft".into()));
        ctrl(&mut ui, 's');
        assert!(ui.modal.is_none());
        assert!(!ui.apply_available());
        assert!(ui.activate(Control::Apply).is_none());
        assert!(ui.modal.is_none());
        match &ui.mode {
            Mode::Edit(form) => assert_eq!(form.draft().name, "bug draft"),
            Mode::Filter { input, .. } => assert_eq!(input.value(), " draft"),
            Mode::List => panic!("Apply discarded the input"),
        }
        assert_eq!(ui.document.labels().unwrap().len(), 2);
        key(&mut ui, KeyCode::Esc, KeyModifiers::NONE);
        assert!(ui.apply_available());
        ctrl(&mut ui, 's');
        assert!(ui.modal.is_some());
    }
}

#[test]
fn release_keys_are_ignored_even_for_global_cancel() {
    let mut ui = state();
    for (code, modifiers) in [
        (KeyCode::Delete, KeyModifiers::NONE),
        (KeyCode::Char('c'), KeyModifiers::CONTROL),
    ] {
        let mut event = KeyEvent::new(code, modifiers);
        event.kind = KeyEventKind::Release;
        assert!(ui.handle(Event::Key(event)).is_none());
    }
    assert!(!ui.document.can_undo());
}

#[test]
fn modal_owns_keys_before_history_and_mode_routing() {
    let mut ui = state();
    ui.delete_selected();
    ui.confirm();
    for ch in ['z', 'y', 's'] {
        ctrl(&mut ui, ch);
        assert!(ui.modal.is_some());
        assert!(ui.document.labels().unwrap().is_empty());
    }
    key(&mut ui, KeyCode::Char('n'), KeyModifiers::NONE);
    assert!(matches!(ui.mode, Mode::List));
    key(&mut ui, KeyCode::Tab, KeyModifiers::NONE);
    assert!(matches!(
        key(&mut ui, KeyCode::Enter, KeyModifiers::NONE),
        Some(UiAction::Apply(_))
    ));
}

const MODAL_MODIFIERS: [KeyModifiers; 7] = [
    KeyModifiers::CONTROL,
    KeyModifiers::ALT,
    KeyModifiers::SUPER,
    KeyModifiers::META,
    KeyModifiers::HYPER,
    KeyModifiers::SHIFT,
    KeyModifiers::CONTROL.union(KeyModifiers::ALT),
];

#[test]
fn modified_modal_enter_never_activates_either_choice() {
    for modifiers in MODAL_MODIFIERS {
        for apply in [false, true] {
            let mut ui = state();
            ui.delete_selected();
            ui.confirm();
            ui.modal.as_mut().unwrap().choice = usize::from(!apply);
            assert!(key(&mut ui, KeyCode::Enter, modifiers).is_none());
            assert_eq!((ui.modal.as_ref().unwrap().choice == 0), apply);
        }
    }
}

#[test]
fn modified_modal_navigation_preserves_focus_and_confirmation() {
    for modifiers in MODAL_MODIFIERS {
        for code in [
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Tab,
            KeyCode::BackTab,
            KeyCode::Esc,
        ] {
            if modifiers == KeyModifiers::SHIFT && code == KeyCode::BackTab {
                continue;
            }
            let mut ui = state();
            ui.delete_selected();
            ui.confirm();
            for apply in [false, true] {
                ui.modal.as_mut().unwrap().choice = usize::from(!apply);
                assert!(key(&mut ui, code, modifiers).is_none());
                assert_eq!((ui.modal.as_ref().unwrap().choice == 0), apply);
            }
        }
    }
}

#[test]
fn shift_backtab_navigates_modal_and_control_c_still_cancels() {
    let mut ui = state();
    ui.delete_selected();
    ui.confirm();
    assert!(matches!(ctrl(&mut ui, 'c'), Some(UiAction::Cancel)));
    key(&mut ui, KeyCode::BackTab, KeyModifiers::SHIFT);
    assert!((ui.modal.as_ref().unwrap().choice == 0));
    assert!(matches!(ctrl(&mut ui, 'c'), Some(UiAction::Cancel)));
    key(&mut ui, KeyCode::BackTab, KeyModifiers::SHIFT);
    assert!(!(ui.modal.as_ref().unwrap().choice == 0));
    assert!(key(&mut ui, KeyCode::Enter, KeyModifiers::NONE).is_none());
    assert!(ui.modal.is_none());
}

#[test]
fn apply_button_delegates_no_changes_and_confirmation_to_confirm() {
    let mut ui = state();
    assert!(ui.activate(Control::Apply).is_none());
    assert!(ui.modal.is_none());
    assert!(
        matches!(&ui.message, Some(super::super::Message::Status(text)) if text == "No changes to apply.")
    );
    ui.delete_selected();
    assert!(ui.activate(Control::Apply).is_none());
    assert!(ui.message.is_none());
    assert!(!(ui.modal.as_ref().unwrap().choice == 0));
    assert!(ui.activate(Control::Apply).is_none());
    assert!(!(ui.modal.as_ref().unwrap().choice == 0));
}

#[test]
fn cancel_routes_from_every_mode_and_small_screen() {
    for opener in ['e', '/'] {
        let mut ui = state();
        key(&mut ui, KeyCode::Char(opener), KeyModifiers::NONE);
        assert!(matches!(ctrl(&mut ui, 'c'), Some(UiAction::Cancel)));
    }
    let mut ui = state();
    ui.resize(20, 5);
    assert!(ctrl(&mut ui, 'q').is_none());
    assert!(matches!(ctrl(&mut ui, 'c'), Some(UiAction::Cancel)));
}

#[test]
fn resize_clears_all_mouse_hit_areas_until_render() {
    let mut ui = state();
    ui.buttons = vec![Rect::new(0, 23, 9, 1)];
    ui.rows = Rect::new(0, 4, 80, 2);
    ui.fields = [Rect::new(2, 8, 20, 1); 3];
    ui.handle(Event::Resize(100, 30));
    assert!(ui.buttons.is_empty());
    assert_eq!(ui.rows, Rect::default());
    assert_eq!(ui.fields, [Rect::default(); 3]);
    assert!(click(&mut ui, 2, 23).is_none());
    assert!(!ui.document.can_undo());
}

#[test]
fn mouse_dispatch_respects_edit_filter_and_modal_precedence() {
    let mut ui = state();
    ui.start_edit();
    ui.fields[1] = Rect::new(2, 8, 20, 1);
    click(&mut ui, 2, 8);
    let Mode::Edit(form) = &ui.mode else {
        panic!("expected form")
    };
    assert!(form.field == Field::Color);
    ui.mode = Mode::Filter {
        before: String::new(),
        input: Input::default(),
    };
    ui.buttons = vec![Rect::new(0, 23, 9, 1); 4];
    assert!(click(&mut ui, 2, 23).is_none());
    assert!(matches!(ui.mode, Mode::Filter { .. }));
    ui.mode = Mode::List;
    ui.delete_selected();
    ui.confirm();
    ui.buttons = vec![Rect::new(10, 10, 9, 1)];
    assert!(matches!(click(&mut ui, 10, 10), Some(UiAction::Apply(_))));
}

#[test]
fn ratatui_advances_offset_without_manual_scroll_management() {
    let mut ui = state();
    for i in 1..50 {
        ui.document
            .create(Draft {
                name: format!("label-{i:02}"),
                color: "ededed".into(),
                description: String::new(),
            })
            .unwrap();
    }
    ui.navigate(30);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| ui.render(frame)).unwrap();
    assert!(ui.table.offset() > 0);
    assert_eq!(ui.table.selected(), Some(30));
    let offset = ui.table.offset();
    let mut terminal = Terminal::new(TestBackend::new(48, 16)).unwrap();
    terminal.draw(|frame| ui.render(frame)).unwrap();
    assert!(ui.table.offset() > offset);
    assert_eq!(ui.selected, Some(30));
    let offset = ui.table.offset();
    let rows = ui.rows;
    click(&mut ui, rows.x, rows.y);
    assert_eq!(ui.selected, Some(offset as u64));
}

#[test]
fn focus_skips_unavailable_history_and_apply() {
    let mut ui = state();
    key(&mut ui, KeyCode::Tab, KeyModifiers::NONE);
    assert!(matches!(
        key(&mut ui, KeyCode::Enter, KeyModifiers::NONE),
        Some(UiAction::Cancel)
    ));
}

fn interactive_state(session: SessionKind) -> UiState {
    let document = state().document;
    let mut ui = if session == SessionKind::Export {
        UiState::export(
            document,
            "source".into(),
            "out.json".into(),
            ColorLevel::NoColor,
        )
    } else {
        UiState::reconcile(
            document,
            "target".into(),
            session,
            vec![],
            false,
            ColorLevel::NoColor,
        )
        .unwrap()
    };
    ui.edit_workspace();
    ui
}

#[test]
fn interactive_edit_exit_keys_validate_and_preserve_history_and_saves() {
    for session in [SessionKind::Export, SessionKind::Sync, SessionKind::Copy]
    {
        for code in [KeyCode::Esc, KeyCode::Char('q')] {
            for small in [false, true] {
                let mut ui = interactive_state(session);
                ui.document.delete(0);
                ui.record_save(Ok(crate::edit::session::SaveRecord {
                    path: "saved.json".into(),
                    warning: None,
                }));
                if small {
                    ui.resize(20, 5);
                }
                assert!(key(&mut ui, code, KeyModifiers::NONE).is_none());
                assert_eq!(ui.workspace, WorkspaceMode::Select);
                assert!(ui.document.can_undo());
                assert_eq!(ui.saves.len(), 1);
                ui.document.undo();
                assert_eq!(ui.document.labels().unwrap().len(), 1);
            }
        }
    }
}

#[test]
fn interactive_edit_exit_rejects_invalid_full_document() {
    for code in [KeyCode::Esc, KeyCode::Char('q')] {
        let mut ui = interactive_state(SessionKind::Export);
        ui.document = crate::edit::model::Document::from_labels(vec![
            state().document.labels().unwrap()[0].clone(),
            state().document.labels().unwrap()[0].clone(),
        ]);
        assert!(key(&mut ui, code, KeyModifiers::NONE).is_none());
        assert_eq!(ui.workspace, WorkspaceMode::Edit);
        assert!(matches!(ui.message, Some(super::super::Message::Error(_))));
    }
}

#[test]
fn q_remains_input_or_cancel_outside_interactive_edit_list() {
    let mut ui = interactive_state(SessionKind::Export);
    ui.start_edit();
    assert!(key(&mut ui, KeyCode::Char('q'), KeyModifiers::NONE).is_none());
    let Mode::Edit(form) = &ui.mode else {
        panic!("expected form")
    };
    assert_eq!(form.draft().name, "bugq");
    ui.mode = Mode::List;
    ui.done();
    assert!(matches!(
        key(&mut ui, KeyCode::Char('q'), KeyModifiers::NONE),
        Some(UiAction::Cancel)
    ));
    assert!(matches!(
        key(&mut state(), KeyCode::Char('q'), KeyModifiers::NONE),
        Some(UiAction::Cancel)
    ));
}
