use super::*;

#[test]
fn resized_save_modal_preserves_workspace_history_selection_and_saves() {
    let mut ui = interactive_state(SessionKind::Export);
    ui.document.delete(0);
    ui.record_save(Ok(crate::edit::session::SaveRecord {
        path: "saved.json".into(),
        warning: None,
    }));
    let selected = ui.selected;
    let labels = ui.selected_labels().unwrap();
    ui.set_save_choices([true, true], ["local".into(), "global".into()]);
    key(&mut ui, KeyCode::Char('s'), KeyModifiers::NONE);
    ui.modal.as_mut().unwrap().choice = 0;
    ui.handle(Event::Resize(20, 5));
    for code in [KeyCode::Enter, KeyCode::Tab, KeyCode::Char('q')] {
        assert!(key(&mut ui, code, KeyModifiers::NONE).is_none());
        assert_eq!(ui.modal.as_ref().unwrap().choice, 0);
    }
    assert!(matches!(ctrl(&mut ui, 'c'), Some(UiAction::Cancel)));
    assert!(key(&mut ui, KeyCode::Esc, KeyModifiers::NONE).is_none());
    assert!(ui.modal.is_none());
    assert_eq!(ui.workspace, WorkspaceMode::Edit);
    assert_eq!(ui.selected, selected);
    assert_eq!(ui.selected_labels().unwrap(), labels);
    assert!(ui.document.can_undo());
    assert_eq!(ui.saves.len(), 1);
    ui.handle(Event::Resize(100, 30));
    ctrl(&mut ui, 'z');
    assert_eq!(ui.document.labels().unwrap().len(), 1);
    key(&mut ui, KeyCode::Char('s'), KeyModifiers::NONE);
    key(&mut ui, KeyCode::Tab, KeyModifiers::NONE);
    assert!(matches!(
        key(&mut ui, KeyCode::Enter, KeyModifiers::NONE),
        Some(UiAction::Save(_))
    ));
}

#[test]
fn hidden_apply_confirmation_ignores_keys_and_resumes_after_resize() {
    for small in [false, true] {
        let mut ui = state();
        ui.delete_selected();
        ui.confirm();
        ui.modal.as_mut().unwrap().choice = 0;
        if small {
            ui.handle(Event::Resize(20, 5));
            for code in [KeyCode::Enter, KeyCode::Tab, KeyCode::Char('a')] {
                assert!(key(&mut ui, code, KeyModifiers::NONE).is_none());
                assert_eq!(ui.modal.as_ref().unwrap().choice, 0);
            }
        }
        assert!(
            key(&mut ui, KeyCode::Char('q'), KeyModifiers::NONE).is_none()
        );
        assert!(ui.modal.is_some());
        assert!(key(&mut ui, KeyCode::Esc, KeyModifiers::NONE).is_none());
        assert!(ui.modal.is_none());
        assert!(ui.document.can_undo());
        ui.handle(Event::Resize(80, 24));
        ui.confirm();
        key(&mut ui, KeyCode::Tab, KeyModifiers::NONE);
        assert!(matches!(
            key(&mut ui, KeyCode::Enter, KeyModifiers::NONE),
            Some(UiAction::Apply(_))
        ));
    }
}
