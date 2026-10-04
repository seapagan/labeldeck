use super::{
    form::replace,
    rendering::{draw, locate, row, status},
    ui::{key, state},
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use labeldeck::edit::ui::UiState;
use ratatui::{Terminal, backend::TestBackend, layout::Position};

fn colour_form() -> UiState {
    let mut ui = state(false);
    key(&mut ui, KeyCode::Enter);
    key(&mut ui, KeyCode::Down);
    replace(&mut ui, "");
    ui
}

fn cursor(ui: &mut UiState) -> Position {
    let mut terminal = Terminal::new(TestBackend::new(160, 24)).unwrap();
    terminal.draw(|frame| ui.render(frame)).unwrap();
    terminal.get_cursor_position().unwrap()
}

#[test]
fn colour_accepts_ascii_hex_keyboard_and_paste_in_canonical_case() {
    for paste in [false, true] {
        for text in ["012345", "6789ab", "cdefAB", "ABCDEF"] {
            let mut ui = colour_form();
            if paste {
                ui.handle(Event::Paste(text.into()));
            } else {
                for ch in text.chars() {
                    ui.handle(Event::Key(KeyEvent::new(
                        KeyCode::Char(ch),
                        KeyModifiers::SHIFT,
                    )));
                }
            }
            let buffer = draw(&mut ui, 160, 24);
            let (_, y) = locate(&buffer, "Color");
            assert!(
                row(&buffer, y)
                    .contains(&format!("■ {}", text.to_ascii_lowercase()))
            );
            assert!(!ui.document().can_undo());
            key(&mut ui, KeyCode::Enter);
            assert_eq!(
                ui.document().labels().unwrap()[0].color.as_str(),
                text.to_ascii_lowercase()
            );
            assert!(ui.document().can_undo());
        }
    }
}

#[test]
fn colour_rejects_invalid_characters_atomically_without_cursor_or_history_changes()
 {
    for paste in [false, true] {
        for ch in [' ', '#', '!', 'g', 'é', '１', '\n'] {
            let mut ui = colour_form();
            ui.handle(Event::Paste("abc".into()));
            key(&mut ui, KeyCode::Left);
            let before = cursor(&mut ui);
            if paste {
                ui.handle(Event::Paste(format!("1{ch}2")));
            } else {
                key(&mut ui, KeyCode::Char(ch));
            }
            assert_eq!(cursor(&mut ui), before);
            let buffer = draw(&mut ui, 160, 24);
            let (_, y) = locate(&buffer, "Color");
            assert!(row(&buffer, y).contains("· abc"));
            assert!(status(&buffer).contains("only hexadecimal digits"));
            assert!(!ui.document().can_undo());
            assert_eq!(
                ui.document().labels().unwrap()[0].color.as_str(),
                "ededed"
            );
            key(&mut ui, KeyCode::Char('D'));
            assert!(row(&draw(&mut ui, 160, 24), y).contains("· abdc"));
        }
    }
}

#[test]
fn colour_seventh_character_and_overlong_paste_preserve_draft() {
    for text in ["a", "AB", "1234567"] {
        let mut ui = colour_form();
        ui.handle(Event::Paste("012345".into()));
        key(&mut ui, KeyCode::Home);
        key(&mut ui, KeyCode::Right);
        let before = cursor(&mut ui);
        ui.handle(Event::Paste(text.into()));
        assert_eq!(cursor(&mut ui), before);
        let buffer = draw(&mut ui, 160, 24);
        assert!(status(&buffer).contains("limited to 6 hexadecimal digits"));
        assert!(!ui.document().can_undo());
        key(&mut ui, KeyCode::Enter);
        assert_eq!(
            ui.document().labels().unwrap()[0].color.as_str(),
            "012345"
        );
    }
    let mut ui = colour_form();
    replace(&mut ui, "123456");
    key(&mut ui, KeyCode::Char('7'));
    key(&mut ui, KeyCode::Enter);
    assert_eq!(ui.document().labels().unwrap()[0].color.as_str(), "123456");
}

#[test]
fn partial_colour_is_safe_but_final_domain_validation_requires_six_digits() {
    for text in ["", "a", "01", "abc", "a12f", "a12f9"] {
        let mut ui = colour_form();
        ui.handle(Event::Paste(text.into()));
        let buffer = draw(&mut ui, 160, 24);
        let (_, y) = locate(&buffer, "Color");
        assert!(row(&buffer, y).contains(&format!("· {text}")));
        key(&mut ui, KeyCode::Enter);
        let buffer = draw(&mut ui, 160, 24);
        assert!(status(&buffer).contains("expected 6 hexadecimal digits"));
        assert!(row(&buffer, 22).contains("Enter save"));
        assert!(!ui.document().can_undo());
        assert_eq!(
            ui.document().labels().unwrap()[0].color.as_str(),
            "ededed"
        );
    }
}

#[test]
fn name_and_description_limits_count_unicode_characters_and_reject_whole_insertions()
 {
    for (field, limit, title) in
        [(0, 50, "Label names"), (2, 100, "Descriptions")]
    {
        for unit in ["a", "é", "界", "e\u{301}"] {
            let mut ui = state(false);
            key(&mut ui, KeyCode::Enter);
            for _ in 0..field {
                key(&mut ui, KeyCode::Down);
            }
            let value = unit.repeat(limit / unit.chars().count());
            replace(&mut ui, &value);
            key(&mut ui, KeyCode::Home);
            let before = cursor(&mut ui);
            key(&mut ui, KeyCode::Char('x'));
            assert_eq!(cursor(&mut ui), before);
            ui.handle(Event::Paste("too much".into()));
            assert_eq!(cursor(&mut ui), before);
            assert!(!ui.document().can_undo());
            assert!(status(&draw(&mut ui, 160, 24)).contains(&format!(
                "{title} are limited to {limit} characters."
            )));
            key(&mut ui, KeyCode::Enter);
            let labels = ui.document().labels().unwrap();
            assert_eq!(
                if field == 0 {
                    &labels[0].name
                } else {
                    &labels[0].description
                },
                &value
            );
        }
    }
}

#[test]
fn oversized_paste_is_rejected_without_truncating_even_below_limit() {
    for (field, limit) in [(0, 50), (2, 100)] {
        let mut ui = state(false);
        key(&mut ui, KeyCode::Enter);
        for _ in 0..field {
            key(&mut ui, KeyCode::Down);
        }
        replace(&mut ui, "safe");
        let before = cursor(&mut ui);
        ui.handle(Event::Paste("é".repeat(limit)));
        assert_eq!(cursor(&mut ui), before);
        assert!(!ui.document().can_undo());
        key(&mut ui, KeyCode::Enter);
        let labels = ui.document().labels().unwrap();
        assert_eq!(
            if field == 0 {
                &labels[0].name
            } else {
                &labels[0].description
            },
            "safe"
        );
    }
}

#[test]
fn typing_reaches_unicode_limits_without_counting_utf8_bytes() {
    for (field, limit) in [(0, 50), (2, 100)] {
        let mut ui = state(false);
        key(&mut ui, KeyCode::Enter);
        for _ in 0..field {
            key(&mut ui, KeyCode::Down);
        }
        replace(&mut ui, "");
        for _ in 0..limit {
            key(&mut ui, KeyCode::Char('é'));
        }
        assert!(status(&draw(&mut ui, 160, 24)).trim().is_empty());
        let before = cursor(&mut ui);
        key(&mut ui, KeyCode::Char('é'));
        assert_eq!(cursor(&mut ui), before);
        assert!(!ui.document().can_undo());
        key(&mut ui, KeyCode::Enter);
        let labels = ui.document().labels().unwrap();
        assert_eq!(
            if field == 0 {
                &labels[0].name
            } else {
                &labels[0].description
            },
            &"é".repeat(limit)
        );
    }
}

#[test]
fn deletion_at_limit_allows_insertion_at_the_same_cursor() {
    for (field, limit) in [(0, 50), (1, 6), (2, 100)] {
        let mut ui = state(false);
        key(&mut ui, KeyCode::Enter);
        for _ in 0..field {
            key(&mut ui, KeyCode::Down);
        }
        replace(&mut ui, &"a".repeat(limit));
        key(&mut ui, KeyCode::Home);
        key(&mut ui, KeyCode::Char('b'));
        key(&mut ui, KeyCode::Delete);
        key(&mut ui, KeyCode::Char('b'));
        assert!(status(&draw(&mut ui, 160, 24)).trim().is_empty());
        key(&mut ui, KeyCode::Enter);
        let labels = ui.document().labels().unwrap();
        let value = match field {
            0 => &labels[0].name,
            1 => labels[0].color.as_str(),
            _ => &labels[0].description,
        };
        assert_eq!(value, format!("b{}", "a".repeat(limit - 1)));
    }
}
