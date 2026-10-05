use super::super::label;
use super::{draw, locate, row};
use colored_text::ColorLevel;
use labeldeck::edit::model::Document;
use labeldeck::edit::ui::UiState;

#[test]
fn content_columns_are_bounded_unicode_aware_and_header_aligned() {
    for (name, color_x) in [
        ("CI".into(), 16),
        ("a".repeat(25), 30),
        ("x".repeat(50), 38),
        ("界".repeat(10), 25),
    ] {
        for width in [48, 80, 140] {
            let mut item = label(&name);
            item.description = "Description gets the remaining width".into();
            let mut ui = UiState::new(
                Document::from_labels(vec![item]),
                "deck".into(),
                false,
                ColorLevel::NoColor,
            );
            let buffer = draw(&mut ui, width, 24);
            let (x, _) = locate(&buffer, "COLOR");
            assert_eq!(
                x,
                if width == 48 {
                    color_x.min(24)
                } else {
                    color_x
                }
            );
            assert_eq!(buffer[(x, 4)].symbol(), "■");
            assert_eq!(buffer[(x + 2, 4)].symbol(), "e");
            assert_eq!(buffer[(2, 2)].symbol(), "L");
            assert_eq!(
                buffer[(2, 4)].symbol(),
                if name.starts_with('界') {
                    "界"
                } else {
                    &name[..1]
                }
            );
            assert_eq!(row(&buffer, 3), "─".repeat(width as usize));
            if width >= 80 {
                assert_eq!(locate(&buffer, "DESCRIPTION").0, x + 12);
                assert_eq!(buffer[(x + 12, 4)].symbol(), "D");
            }
        }
    }
}
