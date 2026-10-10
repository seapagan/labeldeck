use super::*;
use crate::edit::ui::Message;

#[test]
fn apply_button_delegates_no_changes_and_confirmation_to_confirm() {
    let mut ui = state();
    assert!(ui.activate(Control::Apply).is_none());
    assert!(ui.modal.is_none());
    assert!(
        matches!(&ui.message, Some(Message::Status(text)) if text == "No changes to apply.")
    );
    ui.delete_selected();
    assert!(ui.activate(Control::Apply).is_none());
    assert!(ui.message.is_none());
    assert!(!(ui.modal.as_ref().unwrap().choice == 0));
    assert!(ui.activate(Control::Apply).is_none());
    assert!(!(ui.modal.as_ref().unwrap().choice == 0));
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
