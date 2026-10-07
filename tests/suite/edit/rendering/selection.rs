use super::super::{
    label,
    ui::{key, state},
};
use super::{click, draw, locate, row};
use colored_text::ColorLevel;
use crossterm::event::KeyCode;
use labeldeck::edit::{
    model::Document,
    ui::{SessionKind, UiState},
};
use ratatui::{
    style::{Color, Modifier},
    text::Line,
};

fn select(session: SessionKind, level: ColorLevel) -> UiState {
    let document = Document::from_labels(vec![label("bug"), label("docs")]);
    if session == SessionKind::Export {
        UiState::export(document, "source".into(), "out.json".into(), level)
    } else {
        UiState::reconcile(
            document,
            "source".into(),
            session,
            vec![],
            false,
            level,
        )
        .unwrap()
    }
}

fn summary(session: SessionKind) -> &'static str {
    if session == SessionKind::Export {
        "2 / 2 labels selected"
    } else {
        "2 / 2 selected — 2 create, 0 update, 0 delete"
    }
}

#[test]
fn select_spacing_and_blank_checkbox_header_preserve_row_alignment() {
    for session in [SessionKind::Export, SessionKind::Sync, SessionKind::Copy]
    {
        let mut ui = select(session, ColorLevel::NoColor);
        let buffer = draw(&mut ui, 120, 24);
        assert!(row(&buffer, 0).starts_with("labeldeck select — source"));
        assert_eq!(row(&buffer, 1), " ".repeat(120));
        assert_eq!(locate(&buffer, summary(session)).1, 2);
        let label_x = if session == SessionKind::Export {
            7
        } else {
            15
        };
        assert_eq!(locate(&buffer, "LABEL"), (label_x, 3));
        for x in 2..5 {
            assert_eq!(buffer[(x, 3)].symbol(), " ");
        }
        assert_eq!(row(&buffer, 4), "─".repeat(120));
        assert_eq!(locate(&buffer, "[x]"), (2, 5));
        assert_eq!(locate(&buffer, "bug"), (label_x, 5));
        assert_eq!(locate(&buffer, "docs"), (label_x, 6));
        assert_eq!(buffer[(2, 6)].symbol(), "[");
        assert_eq!(buffer[(3, 6)].symbol(), "x");
        assert_eq!(buffer[(4, 6)].symbol(), "]");
    }
}

#[test]
fn summary_centers_over_table_columns_and_has_its_own_emphasis() {
    for session in [SessionKind::Export, SessionKind::Sync, SessionKind::Copy]
    {
        for width in [80, 121, 140] {
            for level in [ColorLevel::TrueColor, ColorLevel::NoColor] {
                let buffer = draw(&mut select(session, level), width, 24);
                let text = summary(session);
                let length = Line::raw(text).width() as u16;
                let x = 2 + (width - 2) / 2 - length / 2;
                assert_eq!(locate(&buffer, text), (x, 2));
                assert_ne!(x, width / 2 - length / 2);
                for column in x..x + length {
                    let cell = &buffer[(column, 2)];
                    let color = if level == ColorLevel::NoColor {
                        Color::Reset
                    } else {
                        Color::LightMagenta
                    };
                    assert_eq!(cell.fg, color);
                    assert!(cell.modifier.contains(Modifier::BOLD));
                    assert!(!cell.modifier.contains(Modifier::DIM));
                }
            }
        }
    }
}

#[test]
fn plain_edit_retains_title_spacer_header_and_rows() {
    for width in [48, 80, 140] {
        let buffer = draw(&mut state(false), width, 24);
        assert!(row(&buffer, 0).starts_with("labeldeck edit"));
        assert_eq!(row(&buffer, 1), " ".repeat(usize::from(width)));
        assert_eq!(locate(&buffer, "LABEL"), (2, 2));
        assert_eq!(row(&buffer, 3), "─".repeat(usize::from(width)));
        assert_eq!(locate(&buffer, "bug"), (2, 4));
        assert_eq!(locate(&buffer, "docs"), (2, 5));
    }
}

#[test]
fn select_help_and_shifted_mouse_rows_preserve_selection_controls() {
    for session in [SessionKind::Export, SessionKind::Sync, SessionKind::Copy]
    {
        let mut ui = select(session, ColorLevel::NoColor);
        let buffer = draw(&mut ui, 120, 24);
        assert_eq!(locate(&buffer, "Tab cycle buttons").1, 21);
        click(&mut ui, 2, 3);
        click(&mut ui, 2, 2);
        assert_eq!(locate(&draw(&mut ui, 120, 24), summary(session)).1, 2);
        click(&mut ui, 2, 5);
        assert_eq!(draw(&mut ui, 120, 24)[(3, 5)].symbol(), " ");
        key(&mut ui, KeyCode::Char(' '));
        assert_eq!(draw(&mut ui, 120, 24)[(3, 5)].symbol(), "x");
        click(&mut ui, 20, 6);
        key(&mut ui, KeyCode::Char(' '));
        let buffer = draw(&mut ui, 120, 24);
        assert_eq!(buffer[(3, 5)].symbol(), "x");
        assert_eq!(buffer[(3, 6)].symbol(), " ");
    }
}
