use super::super::{label, ui::key};
use super::{draw, locate, row};
use colored_text::ColorLevel;
use crossterm::event::{Event, KeyCode};
use labeldeck::{
    edit::{
        model::Document,
        ui::{SessionKind, UiState},
    },
    labels::Label,
};
use ratatui::{style::Color, text::Line};

fn export(labels: Vec<Label>) -> UiState {
    UiState::export(
        Document::from_labels(labels),
        "source".into(),
        "out.json".into(),
        ColorLevel::TrueColor,
    )
}

fn described(name: &str, description: &str) -> Label {
    let mut item = label(name);
    item.description = description.into();
    item
}

fn summary_x(ui: &mut UiState, width: u16) -> u16 {
    let text = ui.selection_summary();
    let buffer = draw(ui, width, 24);
    locate(&buffer, &text).0
}

#[test]
fn description_display_width_moves_virtual_center_and_clamps_to_pane() {
    for (description, virtual_width) in [
        ("short".into(), 44),
        ("界".repeat(20), 73),
        ("x".repeat(300), 140),
    ] {
        let mut ui = export(vec![described("bug", &description)]);
        assert_eq!(summary_x(&mut ui, 140), virtual_width / 2 - 10);
        let buffer = draw(&mut ui, 140, 24);
        assert_eq!(row(&buffer, 4), "─".repeat(140));
        assert_eq!(buffer[(139, 5)].bg, Color::DarkGray);
        assert_eq!(locate(&buffer, "DESCRIPTION"), (33, 3));
    }
}

#[test]
fn reconciliation_measures_create_update_and_delete_rendered_descriptions() {
    for session in [SessionKind::Sync, SessionKind::Copy] {
        for (desired, target, rendered) in [
            (
                vec![described("bug", &"c".repeat(40))],
                vec![],
                "c".repeat(40),
            ),
            (
                vec![described("bug", &"d".repeat(20))],
                vec![described("bug", &"c".repeat(20))],
                format!("{} -> {}", "c".repeat(20), "d".repeat(20)),
            ),
            (
                vec![],
                vec![described("bug", &"x".repeat(40))],
                "x".repeat(40),
            ),
        ] {
            let mut ui = UiState::reconcile(
                Document::from_labels(desired),
                "source".into(),
                session,
                target,
                true,
                ColorLevel::TrueColor,
            )
            .unwrap();
            let text = ui.selection_summary();
            let buffer = draw(&mut ui, 140, 24);
            let virtual_width = 49 + rendered.len() as u16;
            assert_eq!(
                locate(&buffer, &text),
                (virtual_width / 2 - Line::raw(&text).width() as u16 / 2, 2)
            );
            assert_eq!(locate(&buffer, &rendered), (49, 5));
        }
    }
}

#[test]
fn scrolling_keeps_offscreen_candidates_in_the_envelope() {
    let mut labels: Vec<_> =
        (0..40).map(|i| label(&format!("label-{i:02}"))).collect();
    labels[39].description = "x".repeat(50);
    let mut ui = export(labels);
    let before = summary_x(&mut ui, 140);
    assert_eq!(before, 30);
    for _ in 0..39 {
        key(&mut ui, KeyCode::Down);
    }
    assert_eq!(summary_x(&mut ui, 140), before);
    assert_eq!(locate(&draw(&mut ui, 140, 24), "label-39").1, 13);
}

#[test]
fn filter_and_selected_only_can_shrink_the_logical_envelope() {
    let mut ui =
        export(vec![label("bug"), described("docs", &"x".repeat(50))]);
    assert_eq!(summary_x(&mut ui, 140), 31);
    key(&mut ui, KeyCode::Char('/'));
    ui.handle(Event::Paste("bug".into()));
    key(&mut ui, KeyCode::Enter);
    assert_eq!(summary_x(&mut ui, 140), 12);
    key(&mut ui, KeyCode::Char('/'));
    for _ in 0..3 {
        key(&mut ui, KeyCode::Backspace);
    }
    key(&mut ui, KeyCode::Enter);
    key(&mut ui, KeyCode::Down);
    key(&mut ui, KeyCode::Char(' '));
    assert_eq!(summary_x(&mut ui, 140), 31);
    key(&mut ui, KeyCode::Char('v'));
    assert_eq!(summary_x(&mut ui, 140), 12);
}

#[test]
fn plain_edit_buffer_matches_prechange_symbols_styles_and_positions() {
    let mut ui = UiState::new(
        Document::from_labels(vec![
            described("bug", "界 description"),
            label("docs"),
        ]),
        "deck".into(),
        false,
        ColorLevel::TrueColor,
    );
    let buffer = draw(&mut ui, 80, 24);
    assert_eq!(
        format!("{buffer:?}"),
        include_str!("plain_edit_buffer.txt").trim_end()
    );
}
