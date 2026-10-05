use colored_text::{AnsiColor, ColorLevel, ResolvedColor};
use labeldeck::edit::color::{map_color, preview};
use ratatui::style::Color;

#[test]
fn ansi_palette_mapping_preserves_all_sgr_identities() {
    let mappings = [
        (AnsiColor::Black, Color::Black),
        (AnsiColor::Red, Color::Red),
        (AnsiColor::Green, Color::Green),
        (AnsiColor::Yellow, Color::Yellow),
        (AnsiColor::Blue, Color::Blue),
        (AnsiColor::Magenta, Color::Magenta),
        (AnsiColor::Cyan, Color::Cyan),
        (AnsiColor::White, Color::Gray),
        (AnsiColor::BrightBlack, Color::DarkGray),
        (AnsiColor::BrightRed, Color::LightRed),
        (AnsiColor::BrightGreen, Color::LightGreen),
        (AnsiColor::BrightYellow, Color::LightYellow),
        (AnsiColor::BrightBlue, Color::LightBlue),
        (AnsiColor::BrightMagenta, Color::LightMagenta),
        (AnsiColor::BrightCyan, Color::LightCyan),
        (AnsiColor::BrightWhite, Color::White),
    ];
    for (ansi, expected) in mappings {
        assert_eq!(map_color(ResolvedColor::Named(ansi)), expected);
    }
}

#[test]
fn indexed_rgb_and_no_colour_are_direct_and_do_not_change_hex() {
    assert_eq!(map_color(ResolvedColor::Ansi256(123)), Color::Indexed(123));
    assert_eq!(map_color(ResolvedColor::Rgb(1, 2, 3)), Color::Rgb(1, 2, 3));
    assert_eq!(preview("abcdef", ColorLevel::NoColor), None);
    assert_eq!(preview("bad", ColorLevel::TrueColor), None);
    assert_eq!(
        preview("abcdef", ColorLevel::TrueColor),
        Some(Color::Rgb(171, 205, 239))
    );
    assert!(matches!(
        preview("abcdef", ColorLevel::Ansi256),
        Some(Color::Indexed(_))
    ));
    assert!(preview("abcdef", ColorLevel::Ansi16).is_some());
}
