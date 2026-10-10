use super::*;

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

mod confirmation;
