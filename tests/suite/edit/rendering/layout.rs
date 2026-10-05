use super::super::{form::replace, label, ui::key, ui::screen, ui::state};
use super::{assert_focus, click, draw, locate, row};
use colored_text::ColorLevel;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use labeldeck::edit::model::Document;
use labeldeck::edit::ui::UiAction;
use labeldeck::edit::ui::UiState;

#[test]
fn short_and_filtered_decks_keep_details_close_and_bottom_controls_stable() {
    let mut ui = state(false);
    for height in [16, 24, 40] {
        let buffer = draw(&mut ui, 80, height);
        assert_eq!(locate(&buffer, "Name").1, 7);
        assert_eq!(locate(&buffer, "[^Z Undo]").1, height - 1);
    }
    key(&mut ui, KeyCode::Char('/'));
    ui.handle(Event::Paste("bug".into()));
    let buffer = draw(&mut ui, 80, 24);
    assert_eq!(locate(&buffer, "Name").1, 6);
    assert!(row(&buffer, 22).contains("Esc restore"));
    let mut ui = UiState::new(
        Document::from_labels(
            (0..50).map(|i| label(&format!("label-{i:02}"))).collect(),
        ),
        "deck".into(),
        false,
        ColorLevel::NoColor,
    );
    for _ in 0..30 {
        key(&mut ui, KeyCode::Down);
    }
    let buffer = draw(&mut ui, 48, 16);
    assert_eq!(locate(&buffer, "Name").1, 8);
    assert!(screen(&mut ui, 48, 16).contains("label-30"));
    click(&mut ui, 2, 4);
    assert!(ui.selected().unwrap() > 0);
    for size in [(47, 16), (48, 15), (0, 0)] {
        draw(&mut ui, size.0, size.1);
        assert!(matches!(key(&mut ui, KeyCode::Esc), Some(UiAction::Cancel)));
    }
}

#[test]
fn clicking_edit_fields_uses_rendered_rectangles_and_retains_drafts() {
    let mut ui = state(false);
    key(&mut ui, KeyCode::Enter);
    for (text, draft) in [
        ("Color", "ff0000"),
        ("Description", "draft"),
        ("Name", "defect"),
    ] {
        let buffer = draw(&mut ui, 80, 24);
        let pos = locate(&buffer, text);
        click(&mut ui, pos.0, pos.1);
        replace(&mut ui, draft);
        assert_focus(&mut ui, text);
    }
    assert!(!ui.document().can_undo());
    key(&mut ui, KeyCode::Enter);
    let label = &ui.document().labels().unwrap()[0];
    assert_eq!(
        (&*label.name, label.color.as_str(), &*label.description),
        ("defect", "ff0000", "draft")
    );
}
