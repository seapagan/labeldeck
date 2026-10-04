use super::super::{
    form::replace, label, ui::ctrl, ui::key, ui::screen, ui::state,
};
use super::{click, draw, locate, row};
use colored_text::ColorLevel;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use labeldeck::edit::model::Document;
use labeldeck::edit::ui::UiAction;
use labeldeck::edit::ui::UiState;
use ratatui::style::Color;
use ratatui::style::Modifier;

#[test]
fn compact_footer_controls_have_matching_hitboxes_and_disabled_styles() {
    let mut ui = state(false);
    key(&mut ui, KeyCode::Tab);
    let buffer = draw(&mut ui, 120, 24);
    assert!(
        row(&buffer, 23)
            .starts_with("[^Z Undo]  [^Y Redo]  [^S Apply]  [Esc Cancel]")
    );
    assert!(row(&buffer, 23)[46..].trim().is_empty());
    for text in ["[^Z Undo]", "[^Y Redo]", "[^S Apply]"] {
        let pos = locate(&buffer, text);
        assert!(buffer[pos].modifier.contains(Modifier::DIM));
    }
    assert!(click(&mut ui, 60, 23).is_none());
    assert!(!ui.document().can_undo());
    key(&mut ui, KeyCode::Delete);
    let buffer = draw(&mut ui, 120, 24);
    let undo = locate(&buffer, "[^Z Undo]");
    click(&mut ui, undo.0, undo.1);
    assert_eq!(ui.document().labels().unwrap().len(), 2);
    let buffer = draw(&mut ui, 120, 24);
    let redo = locate(&buffer, "[^Y Redo]");
    click(&mut ui, redo.0, redo.1);
    assert_eq!(ui.document().labels().unwrap().len(), 1);
    assert!(matches!(click(&mut ui, 34, 23), Some(UiAction::Cancel)));
}

#[test]
fn footer_focus_background_is_confined_to_compact_button() {
    let mut ui = UiState::new(
        Document::from_labels(vec![label("bug")]),
        "deck".into(),
        false,
        ColorLevel::TrueColor,
    );
    key(&mut ui, KeyCode::Delete);
    key(&mut ui, KeyCode::Tab);
    let buffer = draw(&mut ui, 140, 24);
    assert_ne!(buffer[(0, 23)].bg, Color::Reset);
    assert_eq!(buffer[(9, 23)].bg, Color::Reset);
    assert!(
        buffer
            .content()
            .iter()
            .filter(|cell| cell.bg != Color::Reset)
            .count()
            < 20
    );
}

#[test]
fn no_changes_never_opens_modal_from_keyboard_or_mouse() {
    let mut ui = state(false);
    for mouse in [false, true] {
        let buffer = draw(&mut ui, 80, 24);
        if mouse {
            let p = locate(&buffer, "[^S Apply]");
            click(&mut ui, p.0, p.1);
        } else {
            ctrl(&mut ui, 's');
        }
        let text = screen(&mut ui, 80, 24);
        assert!(text.contains("No changes to apply."));
        assert!(!text.contains("Confirm Apply"));
    }
}

#[test]
fn identity_swap_with_same_final_label_set_keeps_apply_disabled() {
    let mut ui = state(false);
    key(&mut ui, KeyCode::Enter);
    replace(&mut ui, "docs");
    key(&mut ui, KeyCode::Enter);
    key(&mut ui, KeyCode::Down);
    key(&mut ui, KeyCode::Enter);
    replace(&mut ui, "bug");
    key(&mut ui, KeyCode::Enter);
    let buffer = draw(&mut ui, 80, 24);
    assert!(
        buffer[locate(&buffer, "[^S Apply]")]
            .modifier
            .contains(Modifier::DIM)
    );
    ctrl(&mut ui, 's');
    assert!(screen(&mut ui, 80, 24).contains("No changes to apply."));
}

#[test]
fn resize_and_modal_transitions_invalidate_old_button_hitboxes() {
    let mut ui = state(false);
    draw(&mut ui, 80, 24);
    ui.handle(Event::Resize(100, 30));
    assert!(click(&mut ui, 79, 23).is_none());
    let mut ui = state(false);
    key(&mut ui, KeyCode::Delete);
    draw(&mut ui, 80, 24);
    ctrl(&mut ui, 's');
    assert!(click(&mut ui, 2, 23).is_none());
    let buffer = draw(&mut ui, 80, 24);
    let apply = locate(&buffer, "[ Apply ]");
    key(&mut ui, KeyCode::Esc);
    assert!(click(&mut ui, apply.0, apply.1).is_none());
}
