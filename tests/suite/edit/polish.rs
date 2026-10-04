use super::{
    form::replace,
    label,
    rendering::{click, draw, locate, row},
    ui::{ctrl, key},
};
use colored_text::ColorLevel;
use crossterm::event::{Event, KeyCode};
use labeldeck::edit::{
    model::{Document, EntryId, visible_ids},
    ui::UiState,
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    style::{Color, Modifier},
    text::Line,
};

fn form_state(level: ColorLevel) -> UiState {
    let mut item = label("testing");
    item.description = "description value".into();
    UiState::new(
        Document::from_labels(vec![item]),
        "deck".into(),
        false,
        level,
    )
}

fn values(buffer: &Buffer) -> [u16; 3] {
    [
        ("Name", "testing"),
        ("Color", "ededed"),
        ("Description", "description value"),
    ]
    .map(|(title, text)| {
        let (_, y) = locate(buffer, title);
        (2..buffer.area.width)
            .find(|&x| {
                let rest: String = (x..buffer.area.width)
                    .map(|i| buffer[(i, y)].symbol())
                    .collect();
                rest.starts_with(text)
            })
            .unwrap()
    })
}

#[test]
fn all_values_share_one_column_in_view_and_every_edit_focus() {
    for width in [48, 80, 140] {
        let mut ui = form_state(ColorLevel::TrueColor);
        let before = draw(&mut ui, width, 24);
        let columns = values(&before);
        assert_eq!(columns, [columns[0]; 3]);
        let (_, y) = locate(&before, "Color");
        assert_eq!(before[(columns[0] - 2, y)].symbol(), "■");
        key(&mut ui, KeyCode::Enter);
        for _ in 0..3 {
            assert_eq!(values(&draw(&mut ui, width, 24)), columns);
            key(&mut ui, KeyCode::Tab);
        }
        key(&mut ui, KeyCode::Esc);
        assert_eq!(values(&draw(&mut ui, width, 24)), columns);
    }
}

fn assert_color_cells(buffer: &Buffer, x: u16, y: u16, level: ColorLevel) {
    let focused: Vec<_> = (0..buffer.area.width)
        .filter(|&i| {
            let cell = &buffer[(i, y)];
            if level == ColorLevel::NoColor {
                cell.modifier.contains(Modifier::REVERSED)
            } else {
                cell.bg == Color::DarkGray
            }
        })
        .collect();
    assert_eq!(focused, (x..x + 6).collect::<Vec<_>>());
    assert_eq!(buffer[(x - 2, y)].bg, Color::Reset);
    assert!(!buffer[(x - 2, y)].modifier.contains(Modifier::REVERSED));
}

#[test]
fn color_focus_is_six_cells_with_unscrolled_text_and_bounded_cursor() {
    for level in [ColorLevel::TrueColor, ColorLevel::NoColor] {
        for text in ["", "a", "abcde", "abcdef"] {
            let mut ui = form_state(level);
            let x = values(&draw(&mut ui, 48, 16))[0];
            key(&mut ui, KeyCode::Enter);
            key(&mut ui, KeyCode::Tab);
            replace(&mut ui, text);
            let mut terminal =
                Terminal::new(TestBackend::new(48, 16)).unwrap();
            terminal.draw(|f| ui.render(f)).unwrap();
            let buffer = terminal.backend().buffer();
            let (_, y) = locate(buffer, "Color");
            assert_color_cells(buffer, x, y, level);
            let rendered: String =
                (x..x + 6).map(|i| buffer[(i, y)].symbol()).collect();
            assert_eq!(rendered, format!("{text:6}"));
            assert_eq!(
                terminal.get_cursor_position().unwrap().x,
                x + text.len().min(5) as u16
            );
            for _ in 0..5 {
                key(&mut ui, KeyCode::Right);
            }
            if !text.is_empty() {
                key(&mut ui, KeyCode::Left);
                terminal.draw(|f| ui.render(f)).unwrap();
                assert_eq!(
                    terminal.get_cursor_position().unwrap().x,
                    x + text.len().min(5) as u16 - 1
                );
                assert_color_cells(terminal.backend().buffer(), x, y, level);
            }
            key(&mut ui, KeyCode::Home);
            for offset in 0..=text.len() {
                terminal.draw(|f| ui.render(f)).unwrap();
                assert_eq!(
                    terminal.get_cursor_position().unwrap().x,
                    x + offset.min(5) as u16
                );
                key(&mut ui, KeyCode::Right);
            }
        }
    }
}

#[test]
fn text_field_focus_uses_display_width_plus_one_and_clamps_to_available_cells()
{
    for level in [ColorLevel::TrueColor, ColorLevel::NoColor] {
        for width in [48, 80, 140] {
            for field in [0, 2] {
                for (text, display_width) in [
                    ("", 0),
                    ("a", 1),
                    ("normal", 6),
                    ("é", 1),
                    ("界", 2),
                    ("e\u{301}", 1),
                ] {
                    assert_text_width(
                        level,
                        width,
                        field,
                        text,
                        display_width,
                    );
                }
                assert_text_width(level, width, field, &"界".repeat(30), 60);
            }
        }
    }
}

fn assert_text_width(
    level: ColorLevel,
    width: u16,
    field: usize,
    text: &str,
    display_width: u16,
) {
    let mut ui = form_state(level);
    key(&mut ui, KeyCode::Enter);
    for _ in 0..field {
        key(&mut ui, KeyCode::Tab);
    }
    replace(&mut ui, text);
    let mut terminal = Terminal::new(TestBackend::new(width, 16)).unwrap();
    terminal.draw(|f| ui.render(f)).unwrap();
    let buffer = terminal.backend().buffer();
    let (label_x, y) = locate(buffer, ["Name", "Color", "Description"][field]);
    let x = label_x + 17;
    let highlighted: Vec<_> = (0..width)
        .filter(|&i| {
            let cell = &buffer[(i, y)];
            if level == ColorLevel::NoColor {
                cell.modifier.contains(Modifier::REVERSED)
            } else {
                cell.bg == Color::DarkGray
            }
        })
        // Wide glyphs occupy their continuation cells with the same style;
        // Ratatui resets those hidden cells in its test buffer.
        .flat_map(|i| i..i + Line::raw(buffer[(i, y)].symbol()).width() as u16)
        .collect();
    let expected = (display_width + 1).min(width - x);
    assert_eq!(highlighted, (x..x + expected).collect::<Vec<_>>());
    let cursor = terminal.get_cursor_position().unwrap();
    assert!(cursor.x >= x && cursor.x < x + expected);
    if display_width < width - x {
        assert_eq!(cursor.x, x + display_width);
    }
}

#[test]
fn text_field_end_right_is_a_noop_and_first_left_is_visible() {
    for field in [0, 2] {
        let mut ui = form_state(ColorLevel::TrueColor);
        key(&mut ui, KeyCode::Enter);
        for _ in 0..field {
            key(&mut ui, KeyCode::Tab);
        }
        replace(&mut ui, "abc");
        let mut terminal = Terminal::new(TestBackend::new(48, 16)).unwrap();
        terminal.draw(|f| ui.render(f)).unwrap();
        let end = terminal.get_cursor_position().unwrap();
        for _ in 0..5 {
            key(&mut ui, KeyCode::Right);
            terminal.draw(|f| ui.render(f)).unwrap();
            assert_eq!(terminal.get_cursor_position().unwrap(), end);
        }
        key(&mut ui, KeyCode::Left);
        terminal.draw(|f| ui.render(f)).unwrap();
        assert_eq!(terminal.get_cursor_position().unwrap().x, end.x - 1);
        key(&mut ui, KeyCode::Delete);
        key(&mut ui, KeyCode::Char('d'));
        key(&mut ui, KeyCode::Enter);
        let item = &ui.document().labels().unwrap()[0];
        assert_eq!(
            if field == 0 {
                &item.name
            } else {
                &item.description
            },
            "abd"
        );
    }
}

fn assert_local_status(buffer: &Buffer, message: &str, level: ColorLevel) {
    let (_, y) = locate(buffer, "Description");
    assert!(row(buffer, y + 1).trim().is_empty());
    assert!(row(buffer, y + 2).contains(message));
    if !message.is_empty() {
        let cell = &buffer[locate(buffer, message)];
        assert!(cell.modifier.contains(Modifier::BOLD));
        assert_eq!(
            cell.fg,
            if level == ColorLevel::NoColor {
                Color::Reset
            } else {
                Color::Red
            }
        );
    }
}

#[test]
fn local_status_reserves_spacer_and_message_without_moving_controls() {
    for level in [ColorLevel::TrueColor, ColorLevel::NoColor] {
        for (width, height) in [(48, 16), (80, 24), (140, 40)] {
            let mut ui = form_state(level);
            key(&mut ui, KeyCode::Enter);
            key(&mut ui, KeyCode::Tab);
            let before = draw(&mut ui, width, height);
            assert_local_status(&before, "", level);
            let (_, y) = locate(&before, "Description");
            assert!(row(&before, y + 2).trim().is_empty());
            key(&mut ui, KeyCode::Char('g'));
            let rejected = draw(&mut ui, width, height);
            assert_local_status(
                &rejected,
                "Color must contain only hexadecimal",
                level,
            );
            replace(&mut ui, "abc");
            key(&mut ui, KeyCode::Enter);
            let invalid = draw(&mut ui, width, height);
            assert_local_status(&invalid, "expected 6 hexadecimal", level);
            key(&mut ui, KeyCode::Backspace);
            let cleared = draw(&mut ui, width, height);
            assert!(row(&cleared, y + 2).trim().is_empty());
            for buffer in [&rejected, &invalid, &cleared] {
                assert_eq!(
                    locate(buffer, "Description"),
                    locate(&before, "Description")
                );
                for row_y in height - 3..height {
                    assert_eq!(row(buffer, row_y), row(&before, row_y));
                }
            }
        }
    }
}

#[test]
fn long_deck_local_status_fits_minimum_size_and_truncates_on_one_row() {
    let mut ui = UiState::new(
        Document::from_labels(
            (0..30).map(|i| label(&format!("label-{i}"))).collect(),
        ),
        "deck".into(),
        false,
        ColorLevel::TrueColor,
    );
    key(&mut ui, KeyCode::Enter);
    key(&mut ui, KeyCode::Tab);
    key(&mut ui, KeyCode::Char('g'));
    let buffer = draw(&mut ui, 48, 16);
    assert_local_status(
        &buffer,
        "Color must contain only hexadecimal",
        ColorLevel::TrueColor,
    );
    let (_, y) = locate(&buffer, "Description");
    assert_eq!(y + 2, 12);
    assert_eq!(row(&buffer, y + 2).chars().count(), 48);
    assert!(row(&buffer, 13).starts_with("Filter:"));
    assert!(row(&buffer, 15).starts_with("[^Z Undo]"));
}

fn deletion_state(filtered: bool, only: bool) -> UiState {
    let names = if only {
        vec!["hidden", "match-a", "other"]
    } else {
        vec!["hidden", "match-a", "other", "match-b", "match-c", "tail"]
    };
    let labels = if filtered {
        names
    } else if only {
        vec!["match-a"]
    } else {
        vec!["match-a", "match-b", "match-c"]
    };
    let mut ui = UiState::new(
        Document::from_labels(labels.into_iter().map(label).collect()),
        "deck".into(),
        false,
        ColorLevel::NoColor,
    );
    if filtered {
        key(&mut ui, KeyCode::Char('/'));
        ui.handle(Event::Paste("match".into()));
        key(&mut ui, KeyCode::Enter);
    }
    ui
}

fn assert_deleted_history(
    ui: &mut UiState,
    before: &[EntryId],
    expected: Option<EntryId>,
    filtered: bool,
) {
    assert_eq!(ui.selected(), expected);
    draw(ui, 80, 24);
    assert_eq!(ui.selected(), expected);
    ctrl(ui, 'z');
    assert_eq!(
        visible_ids(ui.document(), if filtered { "match" } else { "" }),
        before
    );
    assert_eq!(ui.selected(), expected.or_else(|| before.first().copied()));
    ctrl(ui, 'y');
    assert_eq!(ui.selected(), expected);
}

#[test]
fn deletion_reselects_same_visible_index_for_first_middle_last_and_only() {
    for filtered in [false, true] {
        for (index, only) in [(0, false), (1, false), (2, false), (0, true)] {
            for mouse_select in [false, true] {
                let mut ui = deletion_state(filtered, only);
                let before = visible_ids(
                    ui.document(),
                    if filtered { "match" } else { "" },
                );
                if mouse_select {
                    draw(&mut ui, 80, 24);
                    click(&mut ui, 2, 3 + index as u16);
                } else {
                    for _ in 0..index {
                        key(&mut ui, KeyCode::Down);
                    }
                }
                assert_eq!(ui.selected(), Some(before[index]));
                key(&mut ui, KeyCode::Delete);
                let expected = if only {
                    None
                } else {
                    Some(before[if index == 1 { 2 } else { 1 }])
                };
                assert_deleted_history(&mut ui, &before, expected, filtered);
            }
        }
    }
}

#[test]
fn repeated_deletion_follows_surviving_visible_entries_until_empty() {
    for filtered in [false, true] {
        let mut ui = deletion_state(filtered, false);
        let ids =
            visible_ids(ui.document(), if filtered { "match" } else { "" });
        key(&mut ui, KeyCode::Down);
        for expected in [Some(ids[2]), Some(ids[0]), None] {
            key(&mut ui, KeyCode::Delete);
            assert_eq!(ui.selected(), expected);
            draw(&mut ui, 48, 16);
            assert_eq!(ui.selected(), expected);
        }
        key(&mut ui, KeyCode::Delete);
        assert_eq!(ui.selected(), None);
        ctrl(&mut ui, 'z');
        assert_eq!(ui.selected(), Some(ids[0]));
    }
}

#[test]
fn filter_uses_names_and_clearing_retains_selected_identity() {
    let mut description_only = label("bug");
    description_only.description = "testing".into();
    let mut ui = UiState::new(
        Document::from_labels(vec![description_only, label("Testing")]),
        "deck".into(),
        false,
        ColorLevel::NoColor,
    );
    assert!(row(&draw(&mut ui, 140, 24), 22).contains("/ filter labels"));
    key(&mut ui, KeyCode::Char('/'));
    ui.handle(Event::Paste("tE".into()));
    assert_eq!(ui.selected(), Some(1));
    key(&mut ui, KeyCode::Enter);
    let buffer = draw(&mut ui, 140, 24);
    assert!(!row(&buffer, 3).contains("bug"));
    assert_eq!(visible_ids(ui.document(), "tE"), vec![1]);
    key(&mut ui, KeyCode::Char('/'));
    key(&mut ui, KeyCode::Home);
    key(&mut ui, KeyCode::Delete);
    key(&mut ui, KeyCode::Delete);
    key(&mut ui, KeyCode::Enter);
    assert_eq!(ui.selected(), Some(1));
    assert_eq!(visible_ids(ui.document(), ""), vec![0, 1]);
}
