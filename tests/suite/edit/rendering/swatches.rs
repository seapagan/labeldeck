use super::super::{form::replace, label, ui::key};
use super::{draw, locate, row};
use colored_text::ColorLevel;
use crossterm::event::KeyCode;
use labeldeck::edit::model::Document;
use labeldeck::edit::ui::UiState;
use ratatui::style::Color;
use ratatui::style::Modifier;

#[test]
fn headers_and_selection_keep_hierarchy_without_inverting_swatches() {
    for level in [
        ColorLevel::TrueColor,
        ColorLevel::Ansi256,
        ColorLevel::Ansi16,
        ColorLevel::NoColor,
    ] {
        let mut ui = UiState::new(
            Document::from_labels(vec![label("bug")]),
            "deck".into(),
            false,
            level,
        );
        let buffer = draw(&mut ui, 80, 24);
        let header = &buffer[(2, 2)];
        assert!(header.modifier.contains(Modifier::BOLD));
        assert_eq!(header.fg == Color::Reset, level == ColorLevel::NoColor);
        let selected = &buffer[(2, 4)];
        assert!(selected.modifier.contains(Modifier::BOLD));
        assert!(!selected.modifier.contains(Modifier::REVERSED));
        let swatch = &buffer[(16, 4)];
        assert_eq!(
            swatch.fg,
            labeldeck::edit::color::preview("ededed", level)
                .unwrap_or(Color::Reset)
        );
        assert!(!swatch.modifier.contains(Modifier::REVERSED));
        assert!(row(&buffer, 4).contains("■ ededed"));
        assert!(row(&buffer, 7).contains("■ ededed"));
        if level == ColorLevel::NoColor {
            assert!(buffer.content().iter().all(|cell| cell.fg
                == Color::Reset
                && cell.bg == Color::Reset));
        }
    }
}

#[test]
fn live_preview_uses_draft_only_and_invalid_colour_has_neutral_placeholder() {
    for level in [ColorLevel::TrueColor, ColorLevel::NoColor] {
        let mut ui = UiState::new(
            Document::from_labels(vec![label("bug")]),
            "deck".into(),
            false,
            level,
        );
        key(&mut ui, KeyCode::Enter);
        key(&mut ui, KeyCode::Down);
        replace(&mut ui, "ff0000");
        let buffer = draw(&mut ui, 80, 24);
        let (x, y) = locate(&buffer, "ff0000");
        assert_eq!(buffer[(x - 2, y)].symbol(), "■");
        assert_eq!(
            buffer[(x - 2, y)].fg,
            if level == ColorLevel::NoColor {
                Color::Reset
            } else {
                Color::Rgb(255, 0, 0)
            }
        );
        replace(&mut ui, "ff0");
        let buffer = draw(&mut ui, 80, 24);
        let (x, y) = locate(&buffer, "ff0");
        assert_eq!(buffer[(x - 2, y)].fg, Color::Reset);
        assert_eq!(
            ui.document().labels().unwrap()[0].color.as_str(),
            "ededed"
        );
        assert!(!ui.document().can_undo());
        assert!(row(&buffer, 22).contains("Enter save"));
        assert!(!row(&buffer, 22).contains("filter"));
    }
}
