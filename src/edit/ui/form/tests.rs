use super::{EditForm, Field};
use crate::edit::model::Draft;
use tui_input::InputRequest;

fn form(field: Field, value: &str) -> EditForm {
    let mut form = EditForm::new(
        None,
        Draft {
            name: "name".into(),
            color: "abcdef".into(),
            description: "description".into(),
        },
    );
    form.field = field;
    form.inputs[field as usize] = tui_input::Input::new(value.into());
    form.handle(InputRequest::GoToEnd).unwrap();
    form
}

#[test]
fn right_at_end_is_a_noop_and_left_moves_immediately_in_every_field() {
    for (field, text, end) in [
        (Field::Name, "aé界", 3),
        (Field::Description, "description", 11),
        (Field::Color, "abc", 3),
        (Field::Color, "abcdef", 5),
    ] {
        let mut form = form(field, text);
        assert_eq!(form.input().cursor(), end);
        let before = format!("{:?}", form.input());
        for _ in 0..5 {
            form.handle(InputRequest::GoToNextChar).unwrap();
            assert_eq!(format!("{:?}", form.input()), before);
        }
        form.handle(InputRequest::GoToPrevChar).unwrap();
        assert_eq!(form.input().cursor(), end - 1);
        assert_eq!(form.input().value(), text);
    }
}

#[test]
fn opening_full_color_starts_on_last_digit_and_delete_removes_it() {
    let mut form = EditForm::new(
        None,
        Draft {
            name: "name".into(),
            color: "abcdef".into(),
            description: String::new(),
        },
    );
    form.field = Field::Color;
    assert_eq!(form.input().cursor(), 5);
    form.handle(InputRequest::DeleteNextChar).unwrap();
    assert_eq!(form.input().value(), "abcde");
    assert_eq!(form.input().cursor(), 5);
    form.insert("F").unwrap();
    assert_eq!(form.input().value(), "abcdef");
    assert_eq!(form.input().cursor(), 5);
}

#[test]
fn all_color_navigation_stays_inside_text_and_six_cells() {
    for value in ["", "a", "abcde", "abcdef"] {
        let mut form = form(Field::Color, value);
        let end = value.len().min(5);
        for request in [
            InputRequest::GoToEnd,
            InputRequest::GoToNextWord,
            InputRequest::SetCursor(99),
        ] {
            form.handle(request).unwrap();
            assert_eq!(form.input().cursor(), end);
        }
        form.handle(InputRequest::GoToStart).unwrap();
        for _ in 0..8 {
            form.handle(InputRequest::GoToNextChar).unwrap();
            assert!(form.input().cursor() <= end);
        }
        assert_eq!(form.input().cursor(), end);
    }
}

#[test]
fn partial_color_insertion_delete_and_backspace_keep_cursor_bounded() {
    let mut form = form(Field::Color, "abc");
    form.handle(InputRequest::GoToNextChar).unwrap();
    form.handle(InputRequest::GoToPrevChar).unwrap();
    form.insert("F").unwrap();
    assert_eq!(form.input().value(), "abfc");
    assert_eq!(form.input().cursor(), 3);
    form.handle(InputRequest::DeleteNextChar).unwrap();
    assert_eq!(form.input().value(), "abf");
    form.handle(InputRequest::DeletePrevChar).unwrap();
    assert_eq!(form.input().value(), "ab");
    assert_eq!(form.input().cursor(), 2);
}
