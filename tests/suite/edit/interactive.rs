use super::{
    label,
    ui::{ctrl, key, screen},
};
use colored_text::ColorLevel;
use crossterm::event::KeyCode;
use labeldeck::edit::{
    model::Document,
    ui::{UiAction, UiState, WorkspaceMode},
};

fn export() -> UiState {
    UiState::export(
        Document::from_labels(vec![label("bug"), label("docs")]),
        "source".into(),
        "out.json".into(),
        ColorLevel::NoColor,
    )
}

#[test]
fn select_blocks_document_mutation_shortcuts() {
    let mut ui = export();
    assert_eq!(ui.workspace(), WorkspaceMode::Select);
    for code in [
        KeyCode::Delete,
        KeyCode::Char('n'),
        KeyCode::Enter,
        KeyCode::Char('e'),
    ] {
        key(&mut ui, code);
    }
    for ch in ['z', 'y', 's'] {
        ctrl(&mut ui, ch);
    }
    assert!(!ui.document().can_undo());
    assert_eq!(ui.document().labels().unwrap().len(), 2);
    assert_eq!(ui.workspace(), WorkspaceMode::Select);
}

#[test]
fn export_selection_and_edit_share_document_identity() {
    let mut ui = export();
    key(&mut ui, KeyCode::Char(' '));
    assert_eq!(ui.selected_labels().unwrap(), vec![label("docs")]);
    key(&mut ui, KeyCode::Char('v'));
    key(&mut ui, KeyCode::Char('w'));
    assert_eq!(ui.workspace(), WorkspaceMode::Edit);
    assert!(screen(&mut ui, 80, 24).contains("bug"));
    key(&mut ui, KeyCode::Delete);
    ctrl(&mut ui, 'z');
    key(&mut ui, KeyCode::Esc);
    assert_eq!(ui.selected_labels().unwrap(), vec![label("docs")]);
    assert!(screen(&mut ui, 80, 24).contains("1 / 2 labels selected"));
    assert!(matches!(ctrl(&mut ui, 'c'), Some(UiAction::Cancel)));
}

fn reconcile() -> UiState {
    let mut desired = label("Bug");
    desired.description = "new".into();
    UiState::reconcile(
        Document::from_labels(vec![desired, label("create")]),
        "target".into(),
        labeldeck::edit::ui::SessionKind::Sync,
        vec![label("bug"), label("delete")],
        true,
        ColorLevel::NoColor,
    )
    .unwrap()
}

fn filter(ui: &mut UiState, text: &str) {
    key(ui, KeyCode::Char('/'));
    ui.handle(crossterm::event::Event::Paste(text.into()));
    key(ui, KeyCode::Enter);
}

#[test]
fn selection_controls_are_global_under_filter() {
    let mut ui = export();
    filter(&mut ui, "bug");
    for (hotkey, count) in [('0', 0), ('i', 2), ('a', 2), ('i', 0)] {
        key(&mut ui, KeyCode::Char(hotkey));
        assert_eq!(ui.selected_labels().unwrap().len(), count);
    }
    assert!(screen(&mut ui, 80, 16).contains("0 / 2 labels selected"));
}

#[test]
fn selected_only_and_filter_empty_states_are_distinct() {
    let mut ui = export();
    key(&mut ui, KeyCode::Char('0'));
    key(&mut ui, KeyCode::Char('v'));
    assert!(screen(&mut ui, 80, 16).contains("No selected items"));
    key(&mut ui, KeyCode::Char('v'));
    filter(&mut ui, "absent");
    assert!(screen(&mut ui, 80, 16).contains("No filter matches"));
    let mut ui = UiState::reconcile(
        Document::from_labels(vec![label("bug")]),
        "target".into(),
        labeldeck::edit::ui::SessionKind::Copy,
        vec![label("bug")],
        false,
        ColorLevel::NoColor,
    )
    .unwrap();
    assert!(screen(&mut ui, 80, 16).contains("No changes"));
}

#[test]
fn reconciliation_groups_and_subset_use_normal_plan() {
    let mut ui = reconcile();
    assert!(screen(&mut ui, 100, 24).contains("3 / 3 selected"));
    filter(&mut ui, "create");
    for (keycode, counts) in [
        ('c', (0, 1, 1)),
        ('u', (0, 0, 1)),
        ('d', (0, 0, 0)),
        ('u', (0, 1, 0)),
    ] {
        key(&mut ui, KeyCode::Char(keycode));
        let plan = ui.selected_plan();
        assert_eq!(
            (plan.creates.len(), plan.updates.len(), plan.deletes.len()),
            counts
        );
        assert!(plan.retained.is_empty() && plan.unchanged.is_empty());
    }
    assert_eq!(ui.selected_plan().updates[0].current_name(), "bug");
}

#[test]
fn customized_plan_requires_edit_confirmation_and_resets_on_done() {
    let mut ui = reconcile();
    key(&mut ui, KeyCode::Char(' '));
    key(&mut ui, KeyCode::Char('w'));
    assert!(screen(&mut ui, 80, 24).contains("reset operation selections"));
    key(&mut ui, KeyCode::Esc);
    assert_eq!(ui.selected_plan().creates.len(), 0);
    key(&mut ui, KeyCode::Char('w'));
    key(&mut ui, KeyCode::Tab);
    key(&mut ui, KeyCode::Enter);
    assert_eq!(ui.workspace(), WorkspaceMode::Edit);
    key(&mut ui, KeyCode::Esc);
    assert_eq!(ui.selected_plan().creates.len(), 1);
    assert!(screen(&mut ui, 80, 24).contains("selections reset"));
}

#[test]
fn export_preferences_survive_create_undo_redo_and_rename() {
    let mut ui = export();
    key(&mut ui, KeyCode::Char('w'));
    key(&mut ui, KeyCode::Char('n'));
    ui.handle(crossterm::event::Event::Paste("new".into()));
    key(&mut ui, KeyCode::Enter);
    key(&mut ui, KeyCode::Esc);
    assert_eq!(ui.selected_labels().unwrap().len(), 3);
    key(&mut ui, KeyCode::Char(' '));
    key(&mut ui, KeyCode::Char('w'));
    ctrl(&mut ui, 'z');
    ctrl(&mut ui, 'y');
    key(&mut ui, KeyCode::Down);
    key(&mut ui, KeyCode::Down);
    key(&mut ui, KeyCode::Enter);
    super::form::replace(&mut ui, "renamed");
    key(&mut ui, KeyCode::Enter);
    key(&mut ui, KeyCode::Esc);
    assert_eq!(
        ui.selected_labels().unwrap(),
        vec![label("bug"), label("docs")]
    );
    assert_eq!(ui.document().labels().unwrap()[2].name, "renamed");
}

#[test]
fn returning_to_selection_validates_full_deck() {
    let mut ui = export();
    key(&mut ui, KeyCode::Char('w'));
    key(&mut ui, KeyCode::Enter);
    super::form::replace(&mut ui, "DOCS");
    key(&mut ui, KeyCode::Enter);
    key(&mut ui, KeyCode::Esc);
    assert_eq!(ui.workspace(), WorkspaceMode::Edit);
    assert!(screen(&mut ui, 80, 24).contains("duplicate label name"));
}

#[test]
fn form_escape_never_leaves_edit_and_done_never_discards_draft() {
    let mut ui = export();
    key(&mut ui, KeyCode::Char('w'));
    key(&mut ui, KeyCode::Enter);
    ctrl(&mut ui, 's');
    assert!(!screen(&mut ui, 80, 24).contains("Confirm"));
    key(&mut ui, KeyCode::Esc);
    assert_eq!(ui.workspace(), WorkspaceMode::Edit);
    key(&mut ui, KeyCode::Esc);
    assert_eq!(ui.workspace(), WorkspaceMode::Select);
}

#[test]
fn operation_rows_show_target_spelling_and_changed_properties() {
    let mut ui = reconcile();
    let text = screen(&mut ui, 140, 24);
    assert!(
        text.contains("CREATE")
            && text.contains("UPDATE")
            && text.contains("DELETE")
    );
    assert!(text.contains("bug") && text.contains(" -> new"));
    assert!(text.contains("delete") && text.contains("ededed"));
    assert!(!text.contains("Bug"));
}

#[test]
fn row_click_selects_and_checkbox_click_toggles() {
    use super::rendering::{click, draw, locate};
    let mut ui = export();
    let buffer = draw(&mut ui, 80, 24);
    let (x, y) = locate(&buffer, "docs");
    click(&mut ui, x, y);
    assert_eq!(ui.selected(), Some(1));
    assert_eq!(ui.selected_labels().unwrap().len(), 2);
    click(&mut ui, 2, y);
    assert_eq!(ui.selected_labels().unwrap(), vec![label("bug")]);
}

#[test]
fn zero_selection_export_confirmation_returns_empty_payload() {
    let mut ui = export();
    key(&mut ui, KeyCode::Char('0'));
    key(&mut ui, KeyCode::Char('f'));
    assert!(screen(&mut ui, 80, 24).contains("0 labels selected"));
    key(&mut ui, KeyCode::Tab);
    let Some(UiAction::Finish(labeldeck::edit::ui::FinalSelection::Export(
        labels,
    ))) = key(&mut ui, KeyCode::Enter)
    else {
        panic!("export result");
    };
    assert!(labels.is_empty());
}

#[test]
fn zero_operations_disable_final_action_and_delete_warning_uses_subset() {
    let mut ui = reconcile();
    key(&mut ui, KeyCode::Char('0'));
    key(&mut ui, KeyCode::Char('f'));
    assert!(!screen(&mut ui, 80, 24).contains("Confirm Apply"));
    key(&mut ui, KeyCode::Char('u'));
    key(&mut ui, KeyCode::Char('f'));
    assert!(!screen(&mut ui, 80, 24).contains("Deleting labels"));
    key(&mut ui, KeyCode::Esc);
    key(&mut ui, KeyCode::Char('d'));
    key(&mut ui, KeyCode::Char('f'));
    assert!(screen(&mut ui, 80, 24).contains("1 deleted"));
}

#[test]
fn each_session_uses_shared_size_requirement_for_render_and_resize() {
    for mut ui in [
        export(),
        reconcile(),
        UiState::new(
            Document::from_labels(vec![label("bug")]),
            "deck".into(),
            false,
            ColorLevel::NoColor,
        ),
    ] {
        let (width, height) = ui.minimum_size();
        assert!(
            !screen(&mut ui, width, height).contains("terminal too small")
        );
        ui.handle(crossterm::event::Event::Resize(width - 1, height));
        assert!(
            screen(&mut ui, width - 1, height).contains("terminal too small")
        );
        assert!(key(&mut ui, KeyCode::Delete).is_none());
        ui.handle(crossterm::event::Event::Resize(width, height));
        assert!(
            !screen(&mut ui, width, height).contains("terminal too small")
        );
        assert!(matches!(ctrl(&mut ui, 'c'), Some(UiAction::Cancel)));
    }
}
