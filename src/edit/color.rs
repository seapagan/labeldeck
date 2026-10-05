//! Renderer-neutral colour resolution; stored values never depend on capability.

use colored_text::{
    AnsiColor, ColorLevel, ResolvedColor, hex_to_rgb, resolve_rgb,
};
use ratatui::style::Color;

pub fn preview(hex: &str, level: ColorLevel) -> Option<Color> {
    let color = crate::labels::LabelColor::parse(hex).ok()?;
    let (r, g, b) = hex_to_rgb(color.as_str()).ok()?;
    resolve_rgb(r, g, b, level).map(map_color)
}

pub fn map_color(resolved: ResolvedColor) -> Color {
    match resolved {
        ResolvedColor::Ansi256(n) => Color::Indexed(n),
        ResolvedColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
        ResolvedColor::Named(named) => match named {
            AnsiColor::Black => Color::Black,
            AnsiColor::Red => Color::Red,
            AnsiColor::Green => Color::Green,
            AnsiColor::Yellow => Color::Yellow,
            AnsiColor::Blue => Color::Blue,
            AnsiColor::Magenta => Color::Magenta,
            AnsiColor::Cyan => Color::Cyan,
            AnsiColor::White => Color::Gray,
            AnsiColor::BrightBlack => Color::DarkGray,
            AnsiColor::BrightRed => Color::LightRed,
            AnsiColor::BrightGreen => Color::LightGreen,
            AnsiColor::BrightYellow => Color::LightYellow,
            AnsiColor::BrightBlue => Color::LightBlue,
            AnsiColor::BrightMagenta => Color::LightMagenta,
            AnsiColor::BrightCyan => Color::LightCyan,
            AnsiColor::BrightWhite => Color::White,
        },
    }
}
