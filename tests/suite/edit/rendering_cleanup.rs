use super::{
    form::replace,
    rendering::{click, draw, locate, row},
    ui::{key, state},
};
use colored_text::ColorLevel;
use crossterm::event::KeyCode;
use labeldeck::edit::ui::UiState;
use ratatui::style::{Color, Modifier};

#[test]
fn only_editable_values_receive_local_focus_without_underlines_or_markers() {
    for level in [ColorLevel::TrueColor, ColorLevel::NoColor] {
        let mut ui = UiState::new(
            state(false).document().clone(),
            "deck".into(),
            false,
            level,
        );
        key(&mut ui, KeyCode::Enter);
        for (focused, traversal) in [
            ("Name", KeyCode::Down),
            ("Color", KeyCode::Tab),
            ("Description", KeyCode::BackTab),
            ("Color", KeyCode::Up),
        ] {
            let buffer = draw(&mut ui, 100, 24);
            for title in ["Name", "Color", "Description"] {
                let (x, y) = locate(&buffer, title);
                let value_x = x + 15 + if title == "Color" { 2 } else { 0 };
                for cell in (2..100).map(|x| &buffer[(x, y)]) {
                    assert!(!cell.modifier.contains(Modifier::UNDERLINED));
                }
                assert_eq!(
                    buffer[(value_x, y)].modifier.contains(Modifier::BOLD),
                    title == focused
                );
                assert_eq!(
                    buffer[(value_x, y)].bg,
                    if title == focused && level != ColorLevel::NoColor {
                        Color::DarkGray
                    } else {
                        Color::Reset
                    }
                );
                assert_eq!(buffer[(x, y)].bg, Color::Reset);
                assert!(!buffer[(x, y)].modifier.contains(Modifier::BOLD));
                assert!(!buffer[(x, y)].modifier.contains(Modifier::REVERSED));
                assert_eq!(
                    buffer[(value_x, y)].modifier.contains(Modifier::REVERSED),
                    title == focused && level == ColorLevel::NoColor
                );
                assert!(!row(&buffer, y).contains('>'));
                if title == "Color" {
                    assert_eq!(buffer[(value_x - 2, y)].bg, Color::Reset);
                    assert!(
                        !buffer[(value_x - 2, y)]
                            .modifier
                            .contains(Modifier::REVERSED)
                    );
                }
                assert!(!buffer[(70, y)].modifier.contains(Modifier::BOLD));
                assert_eq!(buffer[(70, y)].bg, Color::Reset);
            }
            key(&mut ui, traversal);
        }
    }
}

#[test]
fn edit_accent_is_continuous_with_breathing_room_even_at_minimum_size() {
    for level in [ColorLevel::TrueColor, ColorLevel::NoColor] {
        let mut ui = UiState::new(
            state(false).document().clone(),
            "deck".into(),
            false,
            level,
        );
        key(&mut ui, KeyCode::Enter);
        let buffer = draw(&mut ui, 48, 16);
        let (_, y) = locate(&buffer, "Name");
        assert!(row(&buffer, y - 1).trim().is_empty());
        for row_y in y..y + 3 {
            let accent = &buffer[(0, row_y)];
            assert_eq!(accent.symbol(), "│");
            assert_eq!(
                accent.fg,
                if level == ColorLevel::NoColor {
                    Color::Reset
                } else {
                    Color::Cyan
                }
            );
            assert!(!accent.modifier.contains(Modifier::UNDERLINED));
        }
        if level == ColorLevel::NoColor {
            assert!(
                buffer
                    .content()
                    .iter()
                    .all(|c| c.fg == Color::Reset && c.bg == Color::Reset)
            );
        } else {
            let (_, focused_y) = locate(&buffer, "Name");
            assert_eq!(buffer[(17, focused_y)].bg, Color::DarkGray);
            assert_eq!(buffer[(17, focused_y + 1)].bg, Color::Reset);
            assert_eq!(buffer[(47, focused_y)].bg, Color::Reset);
        }
    }
}

#[test]
fn long_unicode_drafts_scroll_inside_bounded_local_fields() {
    let mut ui = state(false);
    key(&mut ui, KeyCode::Enter);
    replace(&mut ui, &"界".repeat(50));
    let buffer = draw(&mut ui, 48, 16);
    let (_, y) = locate(&buffer, "Name");
    assert!(row(&buffer, y).contains('界'));
    assert!(click(&mut ui, 47, y).is_none());
    key(&mut ui, KeyCode::Down);
    let buffer = draw(&mut ui, 48, 16);
    let (_, y) = locate(&buffer, "Color");
    // Unused cells must not act as an oversized field hitbox.
    click(&mut ui, 47, y + 1);
    super::rendering::assert_focus(&mut ui, "Color");
}

#[test]
fn view_and_edit_labels_values_and_swatches_keep_the_same_horizontal_alignment()
 {
    for width in [48, 80, 140] {
        let mut ui = state(false);
        let before = draw(&mut ui, width, 24);
        key(&mut ui, KeyCode::Enter);
        let editing = draw(&mut ui, width, 24);
        for title in ["Name", "Color", "Description"] {
            let (x, y) = locate(&before, title);
            assert_eq!(locate(&editing, title), (x, y));
            assert_eq!(x, 2);
            for column in x..width {
                assert_eq!(
                    before[(column, y)].symbol(),
                    editing[(column, y)].symbol()
                );
            }
        }
        key(&mut ui, KeyCode::Esc);
        let cancelled = draw(&mut ui, width, 24);
        assert_eq!(locate(&cancelled, "Color"), locate(&before, "Color"));
    }
}

#[test]
fn list_help_explains_button_cycling() {
    let mut ui = state(false);
    let buffer = draw(&mut ui, 80, 24);
    assert!(row(&buffer, 22).contains("Tab cycle buttons"));
    assert!(!row(&buffer, 22).contains("Tab buttons"));
}

#[test]
fn grouped_hotkey_buttons_fit_minimum_width_with_inactive_gaps() {
    for width in [48, 80, 140] {
        let mut ui = state(false);
        let buffer = draw(&mut ui, width, 16);
        for (label, x) in [
            ("[^Z Undo]", 0),
            ("[^Y Redo]", 11),
            ("[^S Apply]", 22),
            ("[Esc Cancel]", 34),
        ] {
            assert_eq!(locate(&buffer, label), (x, 15));
        }
        for x in [9, 10, 20, 21, 32, 33, 46, 47] {
            assert_eq!(buffer[(x, 15)].symbol(), " ");
            assert!(click(&mut ui, x, 15).is_none());
        }
        assert!(!ui.document().can_undo());
        assert!(!row(&draw(&mut ui, width, 16), 12).contains("No changes"));
    }
}
