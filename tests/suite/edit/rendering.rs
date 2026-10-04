use super::{
    form::replace,
    label,
    ui::{ctrl, key, screen, state},
};
use colored_text::ColorLevel;
use crossterm::event::{
    Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use labeldeck::edit::{
    model::Document,
    ui::{UiAction, UiState},
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier},
};

pub(super) fn draw(ui: &mut UiState, w: u16, h: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|frame| ui.render(frame)).unwrap();
    terminal.backend().buffer().clone()
}

pub(super) fn row(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

pub(super) fn status(buffer: &Buffer) -> String {
    row(buffer, locate(buffer, "Description").1 + 2)
}

pub(super) fn locate(buffer: &Buffer, text: &str) -> (u16, u16) {
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

pub(super) fn click(ui: &mut UiState, x: u16, y: u16) -> Option<UiAction> {
    ui.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }))
}

pub(super) fn assert_focus(ui: &mut UiState, title: &str) {
    let buffer = draw(ui, 160, 24);
    let (x, y) = locate(&buffer, title);
    let value_x = x + 17;
    assert!(buffer[(value_x, y)].modifier.contains(Modifier::BOLD));
    assert_eq!(buffer[(x, y)].bg, Color::Reset);
}

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
            assert_eq!(buffer[(x, 3)].symbol(), "■");
            assert_eq!(buffer[(x + 2, 3)].symbol(), "e");
            assert_eq!(buffer[(2, 1)].symbol(), "L");
            assert_eq!(
                buffer[(2, 3)].symbol(),
                if name.starts_with('界') {
                    "界"
                } else {
                    &name[..1]
                }
            );
            assert_eq!(row(&buffer, 2), "─".repeat(width as usize));
            if width >= 80 {
                assert_eq!(locate(&buffer, "DESCRIPTION").0, x + 12);
                assert_eq!(buffer[(x + 12, 3)].symbol(), "D");
            }
        }
    }
}

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
        let header = &buffer[(2, 1)];
        assert!(header.modifier.contains(Modifier::BOLD));
        assert_eq!(header.fg == Color::Reset, level == ColorLevel::NoColor);
        let selected = &buffer[(2, 3)];
        assert!(selected.modifier.contains(Modifier::BOLD));
        assert!(!selected.modifier.contains(Modifier::REVERSED));
        let swatch = &buffer[(16, 3)];
        assert_eq!(
            swatch.fg,
            labeldeck::edit::color::preview("ededed", level)
                .unwrap_or(Color::Reset)
        );
        assert!(!swatch.modifier.contains(Modifier::REVERSED));
        assert!(row(&buffer, 3).contains("■ ededed"));
        assert!(row(&buffer, 6).contains("■ ededed"));
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

#[test]
fn compact_footer_controls_have_matching_hitboxes_and_disabled_styles() {
    let mut ui = state(false);
    key(&mut ui, KeyCode::Tab);
    let buffer = draw(&mut ui, 120, 24);
    assert!(
        row(&buffer, 23)
            .starts_with("[^Z Undo]  [^Y Redo]  [^S Apply]  [Esc Cancel]")
    );
    assert!(row(&buffer, 23)[46..].trim().is_empty());
    for text in ["[^Z Undo]", "[^Y Redo]", "[^S Apply]"] {
        let pos = locate(&buffer, text);
        assert!(buffer[pos].modifier.contains(Modifier::DIM));
    }
    assert!(click(&mut ui, 60, 23).is_none());
    assert!(!ui.document().can_undo());
    key(&mut ui, KeyCode::Delete);
    let buffer = draw(&mut ui, 120, 24);
    let undo = locate(&buffer, "[^Z Undo]");
    click(&mut ui, undo.0, undo.1);
    assert_eq!(ui.document().labels().unwrap().len(), 2);
    let buffer = draw(&mut ui, 120, 24);
    let redo = locate(&buffer, "[^Y Redo]");
    click(&mut ui, redo.0, redo.1);
    assert_eq!(ui.document().labels().unwrap().len(), 1);
    assert!(matches!(click(&mut ui, 34, 23), Some(UiAction::Cancel)));
}

#[test]
fn footer_focus_background_is_confined_to_compact_button() {
    let mut ui = UiState::new(
        Document::from_labels(vec![label("bug")]),
        "deck".into(),
        false,
        ColorLevel::TrueColor,
    );
    key(&mut ui, KeyCode::Delete);
    key(&mut ui, KeyCode::Tab);
    let buffer = draw(&mut ui, 140, 24);
    assert_ne!(buffer[(0, 23)].bg, Color::Reset);
    assert_eq!(buffer[(9, 23)].bg, Color::Reset);
    assert!(
        buffer
            .content()
            .iter()
            .filter(|cell| cell.bg != Color::Reset)
            .count()
            < 20
    );
}

#[test]
fn short_and_filtered_decks_keep_details_close_and_bottom_controls_stable() {
    let mut ui = state(false);
    for height in [16, 24, 40] {
        let buffer = draw(&mut ui, 80, height);
        assert_eq!(locate(&buffer, "Name").1, 6);
        assert_eq!(locate(&buffer, "[^Z Undo]").1, height - 1);
    }
    key(&mut ui, KeyCode::Char('/'));
    ui.handle(Event::Paste("bug".into()));
    let buffer = draw(&mut ui, 80, 24);
    assert_eq!(locate(&buffer, "Name").1, 5);
    assert!(row(&buffer, 22).contains("Esc restore"));
    let mut ui = UiState::new(
        Document::from_labels(
            (0..50).map(|i| label(&format!("label-{i:02}"))).collect(),
        ),
        "deck".into(),
        false,
        ColorLevel::NoColor,
    );
    for _ in 0..30 {
        key(&mut ui, KeyCode::Down);
    }
    let buffer = draw(&mut ui, 48, 16);
    assert_eq!(locate(&buffer, "Name").1, 8);
    assert!(screen(&mut ui, 48, 16).contains("label-30"));
    click(&mut ui, 2, 3);
    assert!(ui.selected().unwrap() > 0);
    for size in [(47, 16), (48, 15), (0, 0)] {
        draw(&mut ui, size.0, size.1);
        assert!(matches!(key(&mut ui, KeyCode::Esc), Some(UiAction::Cancel)));
    }
}

#[test]
fn no_changes_never_opens_modal_from_keyboard_or_mouse() {
    let mut ui = state(false);
    for mouse in [false, true] {
        let buffer = draw(&mut ui, 80, 24);
        if mouse {
            let p = locate(&buffer, "[^S Apply]");
            click(&mut ui, p.0, p.1);
        } else {
            ctrl(&mut ui, 's');
        }
        let text = screen(&mut ui, 80, 24);
        assert!(text.contains("No changes to apply."));
        assert!(!text.contains("Confirm Apply"));
    }
}

#[test]
fn identity_swap_with_same_final_label_set_keeps_apply_disabled() {
    let mut ui = state(false);
    key(&mut ui, KeyCode::Enter);
    replace(&mut ui, "docs");
    key(&mut ui, KeyCode::Enter);
    key(&mut ui, KeyCode::Down);
    key(&mut ui, KeyCode::Enter);
    replace(&mut ui, "bug");
    key(&mut ui, KeyCode::Enter);
    let buffer = draw(&mut ui, 80, 24);
    assert!(
        buffer[locate(&buffer, "[^S Apply]")]
            .modifier
            .contains(Modifier::DIM)
    );
    ctrl(&mut ui, 's');
    assert!(screen(&mut ui, 80, 24).contains("No changes to apply."));
}

#[test]
fn resize_and_modal_transitions_invalidate_old_button_hitboxes() {
    let mut ui = state(false);
    draw(&mut ui, 80, 24);
    ui.handle(Event::Resize(100, 30));
    assert!(click(&mut ui, 79, 23).is_none());
    let mut ui = state(false);
    key(&mut ui, KeyCode::Delete);
    draw(&mut ui, 80, 24);
    ctrl(&mut ui, 's');
    assert!(click(&mut ui, 2, 23).is_none());
    let buffer = draw(&mut ui, 80, 24);
    let apply = locate(&buffer, "[ Apply ]");
    key(&mut ui, KeyCode::Esc);
    assert!(click(&mut ui, apply.0, apply.1).is_none());
}

#[test]
fn clicking_edit_fields_uses_rendered_rectangles_and_retains_drafts() {
    let mut ui = state(false);
    key(&mut ui, KeyCode::Enter);
    for (text, draft) in [
        ("Color", "ff0000"),
        ("Description", "draft"),
        ("Name", "defect"),
    ] {
        let buffer = draw(&mut ui, 80, 24);
        let pos = locate(&buffer, text);
        click(&mut ui, pos.0, pos.1);
        replace(&mut ui, draft);
        assert_focus(&mut ui, text);
    }
    assert!(!ui.document().can_undo());
    key(&mut ui, KeyCode::Enter);
    let label = &ui.document().labels().unwrap()[0];
    assert_eq!(
        (&*label.name, label.color.as_str(), &*label.description),
        ("defect", "ff0000", "draft")
    );
}

pub(super) fn modal_rect(buffer: &Buffer) -> Rect {
    let (x, y) = locate(buffer, "┌");
    let right = (x..buffer.area.width)
        .find(|i| buffer[(*i, y)].symbol() == "┐")
        .unwrap();
    let bottom = (y..buffer.area.height)
        .find(|i| buffer[(x, *i)].symbol() == "└")
        .unwrap();
    Rect::new(x, y, right - x + 1, bottom - y + 1)
}

#[test]
fn modal_is_centered_bounded_overlay_with_dimmed_background_and_warning() {
    for (w, h) in [(48, 16), (80, 24), (140, 40)] {
        let mut ui = state(true);
        key(&mut ui, KeyCode::Delete);
        ctrl(&mut ui, 's');
        let buffer = draw(&mut ui, w, h);
        let rect = modal_rect(&buffer);
        assert!(rect.width <= 65 && rect.height <= 13);
        assert_eq!(rect.x, (w - rect.width) / 2);
        assert_eq!(rect.y, (h - rect.height) / 2);
        assert!(row(&buffer, 0).contains("labeldeck edit"));
        assert!(buffer[(0, 0)].modifier.contains(Modifier::DIM));
        assert!(
            !buffer[locate(&buffer, "Confirm Apply")]
                .modifier
                .contains(Modifier::DIM)
        );
        let rendered = screen(&mut ui, w, h);
        for text in [
            "1 deleted",
            "Deleting labels removes",
            "issues and pull requests.",
        ] {
            assert!(rendered.contains(text));
        }
        let apply = locate(&buffer, "[ Apply ]");
        let back = locate(&buffer, "[ Back ]");
        assert!(apply.0 >= rect.x && back.0 + 8 < rect.right());
        assert!(apply.1 > rect.y && apply.1 < rect.bottom());
        assert!(row(&buffer, h - 2).contains("←→ choose"));
    }
}

#[test]
fn modal_owns_input_and_back_restores_filter_selection_and_footer_focus() {
    let mut ui = state(false);
    key(&mut ui, KeyCode::Char('/'));
    ui.handle(Event::Paste("docs".into()));
    key(&mut ui, KeyCode::Enter);
    key(&mut ui, KeyCode::Enter);
    ui.handle(Event::Paste(" new".into()));
    key(&mut ui, KeyCode::Enter);
    for _ in 0..3 {
        key(&mut ui, KeyCode::Tab);
    }
    ctrl(&mut ui, 's');
    let buffer = draw(&mut ui, 80, 24);
    assert!(!screen(&mut ui, 80, 24).contains("Deleting labels"));
    ctrl(&mut ui, 'z');
    key(&mut ui, KeyCode::Delete);
    assert_eq!(ui.document().labels().unwrap()[1].name, "docs new");
    assert!(screen(&mut ui, 80, 24).contains("Confirm Apply"));
    let back = locate(&buffer, "[ Back ]");
    click(&mut ui, back.0, back.1);
    assert_eq!(ui.selected(), Some(1));
    assert!(screen(&mut ui, 80, 24).contains("Filter: docs"));
    key(&mut ui, KeyCode::Enter); // prior focus is Apply
    assert!(screen(&mut ui, 80, 24).contains("Confirm Apply"));
    key(&mut ui, KeyCode::Esc);
    assert_eq!(ui.selected(), Some(1));
    ctrl(&mut ui, 's');
    let buffer = draw(&mut ui, 80, 24);
    let apply = locate(&buffer, "[ Apply ]");
    assert!(matches!(
        click(&mut ui, apply.0, apply.1),
        Some(UiAction::Apply(_))
    ));
}

#[test]
fn modal_keyboard_focus_and_back_only_activate_current_choice() {
    for traversal in [
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Tab,
        KeyCode::BackTab,
    ] {
        let mut ui = state(false);
        key(&mut ui, KeyCode::Delete);
        ctrl(&mut ui, 's');
        let buffer = draw(&mut ui, 80, 24);
        assert!(
            buffer[locate(&buffer, "[ Back ]")]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
        key(&mut ui, traversal);
        let buffer = draw(&mut ui, 80, 24);
        assert!(
            buffer[locate(&buffer, "[ Apply ]")]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
        assert!(matches!(
            key(&mut ui, KeyCode::Enter),
            Some(UiAction::Apply(_))
        ));
        key(&mut ui, traversal);
        assert!(key(&mut ui, KeyCode::Enter).is_none());
        assert!(!screen(&mut ui, 80, 24).contains("Confirm Apply"));
    }
}

#[test]
fn all_summary_categories_and_warning_fit_minimum_terminal_without_colour() {
    let mut ui = UiState::new(
        Document::from_labels(vec![
            label("bug"),
            label("docs"),
            label("stale"),
        ]),
        "deck".into(),
        false,
        ColorLevel::NoColor,
    );
    key(&mut ui, KeyCode::Enter);
    replace(&mut ui, "defect");
    key(&mut ui, KeyCode::Enter);
    key(&mut ui, KeyCode::Down);
    key(&mut ui, KeyCode::Enter);
    key(&mut ui, KeyCode::Down);
    replace(&mut ui, "ff0000");
    key(&mut ui, KeyCode::Enter);
    key(&mut ui, KeyCode::Down);
    key(&mut ui, KeyCode::Delete);
    key(&mut ui, KeyCode::Char('n'));
    replace(&mut ui, "created");
    key(&mut ui, KeyCode::Enter);
    ctrl(&mut ui, 's');
    let buffer = draw(&mut ui, 48, 16);
    assert_eq!(modal_rect(&buffer), Rect::new(2, 1, 44, 13));
    for text in [
        "1 renamed",
        "1 colour/description updated",
        "1 created",
        "1 deleted",
        "issues and pull requests.",
    ] {
        locate(&buffer, text);
    }
    assert!(
        buffer
            .content()
            .iter()
            .all(|cell| cell.fg == Color::Reset && cell.bg == Color::Reset)
    );
    ui.handle(Event::Paste("ignored".into()));
    ui.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Moved,
        column: 1,
        row: 1,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(click(&mut ui, 1, 15).is_none());
    assert!(matches!(ctrl(&mut ui, 'c'), Some(UiAction::Cancel)));
}
