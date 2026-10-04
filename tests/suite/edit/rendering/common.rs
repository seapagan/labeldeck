use crossterm::event::{
    Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use labeldeck::edit::ui::{UiAction, UiState};
use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier},
};

pub(in super::super) fn draw(ui: &mut UiState, w: u16, h: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|frame| ui.render(frame)).unwrap();
    terminal.backend().buffer().clone()
}

pub(in super::super) fn row(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

pub(in super::super) fn status(buffer: &Buffer) -> String {
    row(buffer, locate(buffer, "Description").1 + 2)
}

pub(in super::super) fn locate(buffer: &Buffer, text: &str) -> (u16, u16) {
    for y in 0..buffer.area.height {
        // All searched controls are ASCII. Count terminal cells, not UTF-8 bytes.
        for x in 0..buffer.area.width {
            let rest: String = (x..buffer.area.width)
                .map(|i| buffer[(i, y)].symbol())
                .collect();
            if rest.starts_with(text) {
                return (x, y);
            }
        }
    }
    panic!("missing {text}: {buffer:?}")
}

pub(in super::super) fn click(
    ui: &mut UiState,
    x: u16,
    y: u16,
) -> Option<UiAction> {
    ui.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }))
}

pub(in super::super) fn assert_focus(ui: &mut UiState, title: &str) {
    let buffer = draw(ui, 160, 24);
    let (x, y) = locate(&buffer, title);
    let value_x = x + 17;
    assert!(buffer[(value_x, y)].modifier.contains(Modifier::BOLD));
    assert_eq!(buffer[(x, y)].bg, Color::Reset);
}

pub(in super::super) fn modal_rect(buffer: &Buffer) -> Rect {
    let (x, y) = locate(buffer, "┌");
    let right = (x..buffer.area.width)
        .find(|i| buffer[(*i, y)].symbol() == "┐")
        .unwrap();
    let bottom = (y..buffer.area.height)
        .find(|i| buffer[(x, *i)].symbol() == "└")
        .unwrap();
    Rect::new(x, y, right - x + 1, bottom - y + 1)
}
