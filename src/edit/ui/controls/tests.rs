use super::*;
use crate::edit::{
    model::{Document, Draft},
    ui::{ActionGroup, SessionKind},
};
use colored_text::ColorLevel;

fn labels(count: usize) -> Vec<crate::labels::Label> {
    (0..count)
        .map(|i| {
            Draft {
                name: format!("label-{i}"),
                color: "ededed".into(),
                description: "description".repeat(8),
            }
            .label()
            .unwrap()
        })
        .collect()
}

fn assert_control_eligibility(ui: &UiState) {
    let rows = ui.candidates();
    assert_eq!(ui.candidate_count(None), rows.len());
    for group in [
        ActionGroup::Create,
        ActionGroup::Update,
        ActionGroup::Delete,
    ] {
        assert_eq!(
            ui.candidate_count(Some(group)),
            rows.iter().filter(|row| row.group == Some(group)).count()
        );
        assert_eq!(
            ui.enabled(Control::Group(group)),
            rows.iter().any(|row| row.group == Some(group))
        );
    }
    for control in [Control::All, Control::None, Control::Invert] {
        assert_eq!(ui.enabled(control), !rows.is_empty());
    }
    assert_eq!(
        ui.enabled(Control::Finish),
        ui.session == SessionKind::Export || !ui.selected_plan().is_empty()
    );
}

#[test]
fn borrowed_control_checks_match_rows_for_large_and_empty_selections() {
    for count in [0, 1, 1000] {
        let mut ui = UiState::export(
            Document::from_labels(labels(count)),
            "source".into(),
            "out.json".into(),
            ColorLevel::NoColor,
        );
        assert_control_eligibility(&ui);
        ui.select_rows(None, Some(false));
        assert_control_eligibility(&ui);
        ui.selected_only = true;
        ui.filter = "no matches".into();
        assert_control_eligibility(&ui);
    }
}

#[test]
fn borrowed_control_checks_match_all_operation_groups_and_resets() {
    let target = labels(3);
    let mut desired = target[..2].to_vec();
    desired[0].description = "updated".into();
    desired.push(
        Draft {
            name: "new".into(),
            color: "ededed".into(),
            description: String::new(),
        }
        .label()
        .unwrap(),
    );
    let mut ui = UiState::reconcile(
        Document::from_labels(desired),
        "target".into(),
        SessionKind::Sync,
        target,
        true,
        ColorLevel::NoColor,
    )
    .unwrap();
    assert_control_eligibility(&ui);
    for group in [
        ActionGroup::Create,
        ActionGroup::Update,
        ActionGroup::Delete,
    ] {
        ui.select_rows(Some(group), None);
        assert_control_eligibility(&ui);
    }
    assert!(!ui.enabled(Control::Finish));
    ui.edit_workspace();
    ui.done();
    assert_control_eligibility(&ui);
    assert!(ui.enabled(Control::Finish));
}
