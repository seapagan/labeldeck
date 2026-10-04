use super::{
    label,
    rendering::assert_focus,
    ui::{ctrl, key, screen, state},
};
use crossterm::event::{Event, KeyCode};
use labeldeck::edit::ui::UiState;

pub(super) fn replace(ui: &mut UiState, text: &str) {
    key(ui, KeyCode::Home);
    for _ in 0..110 {
        key(ui, KeyCode::Delete);
    }
    ui.handle(Event::Paste(text.into()));
}

#[test]
fn entire_form_is_a_single_edit_with_one_undo_and_redo() {
    // Lizard misparses this Rust function's scope and inflates its NLOC.
    // #lizard forgives
    for opener in [KeyCode::Enter, KeyCode::Char('e')] {
        let mut ui = state(false);
        key(&mut ui, opener);
        assert_focus(&mut ui, "Name");
        replace(&mut ui, "defect");
        key(&mut ui, KeyCode::Down);
        assert_focus(&mut ui, "Color");
        replace(&mut ui, "ff0000");
        key(&mut ui, KeyCode::Tab);
        replace(&mut ui, "new description");
        key(&mut ui, KeyCode::Up);
        key(&mut ui, KeyCode::BackTab);
        let draft = screen(&mut ui, 80, 24);
        for text in ["defect", "ff0000", "new description"] {
            assert!(draft.contains(text), "{text}: {draft}");
        }
        assert_eq!(
            ui.document().labels().unwrap(),
            vec![label("bug"), label("docs")]
        );
        assert!(!ui.document().can_undo());
        key(&mut ui, KeyCode::Enter);
        let changed = ui.document().labels().unwrap();
        assert_eq!(changed[0].name, "defect");
        assert_eq!(changed[0].color.as_str(), "ff0000");
        assert_eq!(changed[0].description, "new description");
        assert_eq!(ui.document().entries()[0].id, 0);
        ctrl(&mut ui, 'z');
        assert_eq!(
            ui.document().labels().unwrap(),
            vec![label("bug"), label("docs")]
        );
        assert!(!ui.document().can_undo());
        ctrl(&mut ui, 'y');
        assert_eq!(ui.document().labels().unwrap(), changed);
    }
}

#[test]
fn cancel_discards_all_drafts_without_history_or_pending_row() {
    for new in [false, true] {
        let mut ui = state(false);
        key(
            &mut ui,
            if new {
                KeyCode::Char('n')
            } else {
                KeyCode::Enter
            },
        );
        for text in ["defect", "ff0000", "new"] {
            replace(&mut ui, text);
            key(&mut ui, KeyCode::Down);
        }
        key(&mut ui, KeyCode::Esc);
        assert_eq!(
            ui.document().labels().unwrap(),
            vec![label("bug"), label("docs")]
        );
        assert_eq!(ui.document().entries().len(), 2);
        assert!(!ui.document().can_undo());
        assert!(!ui.document().can_redo());
    }
}

#[test]
fn new_form_commit_records_creation_once_and_cancel_preserves_redo() {
    let mut ui = state(false);
    key(&mut ui, KeyCode::Char('n'));
    replace(&mut ui, "new");
    key(&mut ui, KeyCode::Down);
    replace(&mut ui, "123456");
    key(&mut ui, KeyCode::Enter);
    let id = ui.selected().unwrap();
    assert_eq!(ui.document().labels().unwrap().len(), 3);
    ctrl(&mut ui, 'z');
    assert_eq!(ui.document().labels().unwrap().len(), 2);
    assert!(!ui.document().can_undo());
    key(&mut ui, KeyCode::Char('n'));
    key(&mut ui, KeyCode::Esc);
    assert!(ui.document().can_redo());
    ctrl(&mut ui, 'y');
    assert_eq!(ui.document().entries().last().unwrap().id, id);
}

#[test]
fn invalid_form_preserves_drafts_and_focuses_offending_field() {
    for (field, value, error) in [
        (0, String::new(), "must not be empty"),
        (1, "fff".into(), "6 hexadecimal"),
    ] {
        let mut ui = state(false);
        key(&mut ui, KeyCode::Enter);
        for _ in 0..field {
            key(&mut ui, KeyCode::Down);
        }
        replace(&mut ui, &value);
        key(&mut ui, KeyCode::Down);
        key(&mut ui, KeyCode::Enter);
        let rendered = screen(&mut ui, 160, 24);
        assert!(rendered.contains(error), "{rendered}");
        assert_focus(&mut ui, ["Name", "Color", "Description"][field]);
        assert!(rendered.contains(&value));
        assert_eq!(
            ui.document().labels().unwrap(),
            vec![label("bug"), label("docs")]
        );
        assert!(!ui.document().can_undo());
        assert!(rendered.contains("Enter save"));
    }
}

#[test]
fn filtered_form_stays_bound_until_commit_or_cancel() {
    for cancel in [false, true] {
        let mut ui = state(false);
        key(&mut ui, KeyCode::Char('/'));
        ui.handle(Event::Paste("bug".into()));
        key(&mut ui, KeyCode::Enter);
        key(&mut ui, KeyCode::Enter);
        replace(&mut ui, "defect");
        for traversal in
            [KeyCode::Down, KeyCode::Tab, KeyCode::Up, KeyCode::BackTab]
        {
            key(&mut ui, traversal);
            screen(&mut ui, 80, 24);
            assert_eq!(ui.selected(), Some(0));
            assert_eq!(ui.document().labels().unwrap()[0].name, "bug");
        }
        key(&mut ui, if cancel { KeyCode::Esc } else { KeyCode::Enter });
        screen(&mut ui, 80, 24);
        assert_eq!(ui.selected(), cancel.then_some(0));
        assert_eq!(ui.document().labels().unwrap()[1], label("docs"));
        assert_eq!(ui.document().can_undo(), !cancel);
    }
}
