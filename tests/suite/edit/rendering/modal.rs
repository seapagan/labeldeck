use super::super::{
    form::replace, label, ui::ctrl, ui::key, ui::screen, ui::state,
};
use super::{click, draw, locate, modal_rect, row};
use colored_text::ColorLevel;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use labeldeck::edit::model::Document;
use labeldeck::edit::ui::UiAction;
use labeldeck::edit::ui::UiState;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;

#[test]
fn save_modal_styles_actions_by_role_and_activates_global_save() {
    use labeldeck::edit::session::SaveTarget;
    for level in [ColorLevel::TrueColor, ColorLevel::NoColor] {
        let mut ui = UiState::new(
            Document::from_labels(vec![label("bug")]),
            "deck".into(),
            false,
            level,
        );
        ui.set_save_choices([true, true], ["local".into(), "global".into()]);
        key(&mut ui, KeyCode::Char('s'));
        key(&mut ui, KeyCode::Tab);
        let buffer = draw(&mut ui, 80, 24);
        for text in ["[Save Local]", "[Save Global]"] {
            let cell = &buffer[locate(&buffer, text)];
            assert_eq!(
                cell.fg,
                if level == ColorLevel::NoColor {
                    Color::Reset
                } else {
                    Color::Green
                }
            );
            assert!(cell.modifier.contains(Modifier::BOLD));
        }
        let back = &buffer[locate(&buffer, "[Back]")];
        assert_eq!(
            back.fg,
            if level == ColorLevel::NoColor {
                Color::Reset
            } else {
                Color::Yellow
            }
        );
        assert!(!back.modifier.contains(Modifier::BOLD));
        key(&mut ui, KeyCode::Tab);
        assert!(matches!(
            key(&mut ui, KeyCode::Enter),
            Some(UiAction::Save(SaveTarget::Global))
        ));
    }
}

#[test]
fn modal_is_centered_bounded_overlay_with_dimmed_background_and_warning() {
    for (w, h) in [(48, 16), (80, 24), (140, 40)] {
        let mut ui = state(true);
        key(&mut ui, KeyCode::Delete);
        ctrl(&mut ui, 's');
        let buffer = draw(&mut ui, w, h);
        let rect = modal_rect(&buffer);
        assert!(rect.width <= 65 && rect.height <= 13);
        assert_eq!(rect.x, (w - rect.width) / 2);
        assert_eq!(rect.y, (h - rect.height) / 2);
        assert!(row(&buffer, 0).contains("labeldeck edit"));
        assert!(buffer[(0, 0)].modifier.contains(Modifier::DIM));
        assert!(
            !buffer[locate(&buffer, "Confirm Apply")]
                .modifier
                .contains(Modifier::DIM)
        );
        let rendered = screen(&mut ui, w, h);
        for text in [
            "1 deleted",
            "Deleting labels removes",
            "issues and pull requests.",
        ] {
            assert!(rendered.contains(text));
        }
        let apply = locate(&buffer, "[ Apply ]");
        let back = locate(&buffer, "[ Back ]");
        assert!(apply.0 >= rect.x && back.0 + 8 < rect.right());
        assert!(apply.1 > rect.y && apply.1 < rect.bottom());
        assert!(row(&buffer, h - 2).contains("←→ choose"));
    }
}

#[test]
fn modal_owns_input_and_back_restores_filter_selection_and_footer_focus() {
    let mut ui = state(false);
    key(&mut ui, KeyCode::Char('/'));
    ui.handle(Event::Paste("docs".into()));
    key(&mut ui, KeyCode::Enter);
    key(&mut ui, KeyCode::Enter);
    ui.handle(Event::Paste(" new".into()));
    key(&mut ui, KeyCode::Enter);
    for _ in 0..2 {
        key(&mut ui, KeyCode::Tab);
    }
    ctrl(&mut ui, 's');
    let buffer = draw(&mut ui, 80, 24);
    assert!(!screen(&mut ui, 80, 24).contains("Deleting labels"));
    ctrl(&mut ui, 'z');
    key(&mut ui, KeyCode::Delete);
    assert_eq!(ui.document().labels().unwrap()[1].name, "docs new");
    assert!(screen(&mut ui, 80, 24).contains("Confirm Apply"));
    let back = locate(&buffer, "[ Back ]");
    click(&mut ui, back.0, back.1);
    assert_eq!(ui.selected(), Some(1));
    assert!(screen(&mut ui, 80, 24).contains("Filter: docs"));
    key(&mut ui, KeyCode::Enter); // prior focus is Apply
    assert!(screen(&mut ui, 80, 24).contains("Confirm Apply"));
    key(&mut ui, KeyCode::Esc);
    assert_eq!(ui.selected(), Some(1));
    ctrl(&mut ui, 's');
    let buffer = draw(&mut ui, 80, 24);
    let apply = locate(&buffer, "[ Apply ]");
    assert!(matches!(
        click(&mut ui, apply.0, apply.1),
        Some(UiAction::Apply(_))
    ));
}

#[test]
fn modal_keyboard_focus_and_back_only_activate_current_choice() {
    for traversal in [
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Tab,
        KeyCode::BackTab,
    ] {
        let mut ui = state(false);
        key(&mut ui, KeyCode::Delete);
        ctrl(&mut ui, 's');
        let buffer = draw(&mut ui, 80, 24);
        assert!(
            buffer[locate(&buffer, "[ Back ]")]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
        key(&mut ui, traversal);
        let buffer = draw(&mut ui, 80, 24);
        assert!(
            buffer[locate(&buffer, "[ Apply ]")]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
        assert!(matches!(
            key(&mut ui, KeyCode::Enter),
            Some(UiAction::Apply(_))
        ));
        key(&mut ui, traversal);
        assert!(key(&mut ui, KeyCode::Enter).is_none());
        assert!(!screen(&mut ui, 80, 24).contains("Confirm Apply"));
    }
}

#[test]
fn all_summary_categories_and_warning_fit_minimum_terminal_without_colour() {
    let mut ui = UiState::new(
        Document::from_labels(vec![
            label("bug"),
            label("docs"),
            label("stale"),
        ]),
        "deck".into(),
        true,
        ColorLevel::NoColor,
    );
    key(&mut ui, KeyCode::Enter);
    replace(&mut ui, "defect");
    key(&mut ui, KeyCode::Enter);
    key(&mut ui, KeyCode::Down);
    key(&mut ui, KeyCode::Enter);
    key(&mut ui, KeyCode::Down);
    replace(&mut ui, "ff0000");
    key(&mut ui, KeyCode::Enter);
    key(&mut ui, KeyCode::Down);
    key(&mut ui, KeyCode::Delete);
    key(&mut ui, KeyCode::Char('n'));
    replace(&mut ui, "created");
    key(&mut ui, KeyCode::Enter);
    ctrl(&mut ui, 's');
    let buffer = draw(&mut ui, 48, 16);
    assert_eq!(modal_rect(&buffer), Rect::new(2, 1, 44, 13));
    for text in [
        "1 renamed",
        "1 colour/description updated",
        "1 created",
        "1 deleted",
        "issues and pull requests.",
    ] {
        locate(&buffer, text);
    }
    assert!(
        buffer
            .content()
            .iter()
            .all(|cell| cell.fg == Color::Reset && cell.bg == Color::Reset)
    );
    ui.handle(Event::Paste("ignored".into()));
    ui.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Moved,
        column: 1,
        row: 1,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(click(&mut ui, 1, 15).is_none());
    assert!(matches!(ctrl(&mut ui, 'c'), Some(UiAction::Cancel)));
}
