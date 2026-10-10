use super::super::{label, ui::key};
use super::{draw, locate};
use colored_text::ColorLevel;
use crossterm::event::KeyCode;
use labeldeck::edit::{
    model::Document,
    session::SaveRecord,
    ui::{SessionKind, UiState},
};
use ratatui::style::{Color, Modifier};

#[test]
fn save_success_and_failure_have_distinct_styles_and_replace_prior_messages() {
    for level in [ColorLevel::TrueColor, ColorLevel::NoColor] {
        let mut ui = UiState::new(
            Document::from_labels(vec![label("bug")]),
            "deck".into(),
            false,
            level,
        );
        ui.record_save(Err(labeldeck::error::Error::Usage(
            "Save failed".into(),
        )));
        let buffer = draw(&mut ui, 80, 24);
        let cell = &buffer[locate(&buffer, "Save failed")];
        assert_eq!(
            cell.fg,
            if level == ColorLevel::NoColor {
                Color::Reset
            } else {
                Color::Red
            }
        );
        assert!(cell.modifier.contains(Modifier::BOLD));
        ui.record_save(Ok(SaveRecord {
            path: "working.json".into(),
            warning: None,
        }));
        let buffer = draw(&mut ui, 80, 24);
        let cell = &buffer[locate(&buffer, "Saved working.json")];
        assert_eq!(
            cell.fg,
            if level == ColorLevel::NoColor {
                Color::Reset
            } else {
                Color::Cyan
            }
        );
        assert!(!cell.modifier.contains(Modifier::BOLD));
        assert_eq!(ui.saves().len(), 1);
        ui.record_save(Err(labeldeck::error::Error::Usage(
            "Save failed again".into(),
        )));
        let buffer = draw(&mut ui, 80, 24);
        assert!(
            buffer[locate(&buffer, "Save failed again")]
                .modifier
                .contains(Modifier::BOLD)
        );
    }
}

#[test]
fn returning_from_reconciliation_edit_renders_a_normal_plan_reset_notice() {
    let mut ui = UiState::reconcile(
        Document::from_labels(vec![label("bug")]),
        "target".into(),
        SessionKind::Sync,
        Vec::new(),
        false,
        ColorLevel::TrueColor,
    )
    .unwrap();
    key(&mut ui, KeyCode::Char('w'));
    key(&mut ui, KeyCode::Esc);
    let buffer = draw(&mut ui, 100, 24);
    let cell = &buffer[locate(&buffer, "Plan updated — selections reset.")];
    assert_eq!(cell.fg, Color::Cyan);
    assert!(!cell.modifier.contains(Modifier::BOLD));
}

#[test]
fn invalid_form_keeps_error_styling_and_cancel_clears_the_message() {
    let mut ui = UiState::new(
        Document::from_labels(vec![label("bug")]),
        "deck".into(),
        false,
        ColorLevel::TrueColor,
    );
    key(&mut ui, KeyCode::Enter);
    super::super::form::replace(&mut ui, "");
    key(&mut ui, KeyCode::Enter);
    let buffer = draw(&mut ui, 80, 24);
    let cell = &buffer[locate(&buffer, "label name must not be empty")];
    assert_eq!(cell.fg, Color::Red);
    assert!(cell.modifier.contains(Modifier::BOLD));
    key(&mut ui, KeyCode::Esc);
    let buffer = draw(&mut ui, 80, 24);
    assert!(!super::status(&buffer).contains("must not be empty"));
    assert_eq!(ui.document().labels().unwrap(), vec![label("bug")]);
}
