//! Session selection, separate from document history and reconciliation plans.
use super::{
    FinalSelection, Modal, ModalKind, Mode, SessionKind, UiState,
    WorkspaceMode,
};
use crate::{
    edit::model::{Document, Draft, EntryId, visible_ids},
    labels::Label,
    plan::Plan,
};
use colored_text::ColorLevel;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionGroup {
    Create,
    Update,
    Delete,
}

pub(super) enum Selection {
    Export(BTreeMap<EntryId, bool>),
    Operations {
        plan: Plan,
        checked: Vec<bool>,
        target: Vec<Label>,
        prune: bool,
    },
}

pub(super) struct Candidate {
    pub id: EntryId,
    pub group: Option<ActionGroup>,
    pub draft: Draft,
    pub checked: bool,
}

impl UiState {
    pub fn export(
        document: Document,
        title: String,
        destination: PathBuf,
        level: ColorLevel,
    ) -> Self {
        let mut state = Self::new(document, title, false, level);
        state.session = SessionKind::Export;
        state.workspace = WorkspaceMode::Select;
        state.destination = Some(destination);
        state.selection = Some(Selection::Export(BTreeMap::new()));
        state.repair_selection();
        state
    }

    pub fn reconcile(
        document: Document,
        title: String,
        session: SessionKind,
        target: Vec<Label>,
        prune: bool,
        level: ColorLevel,
    ) -> Result<Self, String> {
        if !matches!(session, SessionKind::Sync | SessionKind::Copy) {
            return Err("reconciliation requires sync or copy".into());
        }
        let plan = crate::plan::plan(&document.labels()?, &target, prune);
        let checked = vec![true; operation_count(&plan)];
        let mut state = Self::new(document, title, false, level);
        state.session = session;
        state.workspace = WorkspaceMode::Select;
        state.selection = Some(Selection::Operations {
            plan,
            checked,
            target,
            prune,
        });
        state.repair_selection();
        Ok(state)
    }

    pub(super) fn remember_entries(&mut self) {
        if let Some(Selection::Export(preferences)) = &mut self.selection {
            for entry in self.document.entries() {
                preferences.entry(entry.id).or_insert(true);
            }
        }
    }

    pub(super) fn candidates(&self) -> Vec<Candidate> {
        match &self.selection {
            Some(Selection::Export(preferences)) => self
                .document
                .entries()
                .iter()
                .filter(|e| !e.deleted)
                .map(|entry| Candidate {
                    id: entry.id,
                    group: None,
                    draft: entry.draft.clone(),
                    checked: preferences
                        .get(&entry.id)
                        .copied()
                        .unwrap_or(true),
                })
                .collect(),
            Some(Selection::Operations {
                plan,
                checked,
                target,
                ..
            }) => operation_rows(plan, target, checked),
            None => Vec::new(),
        }
    }

    pub(super) fn visible(&self) -> Vec<EntryId> {
        if self.workspace == WorkspaceMode::Edit {
            return visible_ids(&self.document, &self.filter);
        }
        self.candidates()
            .into_iter()
            .filter(|row| {
                crate::edit::model::matches_filter(&row.draft, &self.filter)
                    && (!self.selected_only || row.checked)
            })
            .map(|row| row.id)
            .collect()
    }

    pub fn selected_labels(&self) -> Result<Vec<Label>, String> {
        self.document.labels()?;
        self.candidates()
            .into_iter()
            .filter(|r| r.checked)
            .map(|r| r.draft.label())
            .collect()
    }

    pub fn selected_plan(&self) -> Plan {
        let Some(Selection::Operations { plan, checked, .. }) =
            &self.selection
        else {
            return Plan::default();
        };
        let mut result = Plan::default();
        let mut flags = checked.iter().copied();
        result.creates = plan
            .creates
            .iter()
            .filter(|_| flags.next().unwrap_or(false))
            .cloned()
            .collect();
        result.updates = plan
            .updates
            .iter()
            .filter(|_| flags.next().unwrap_or(false))
            .cloned()
            .collect();
        result.deletes = plan
            .deletes
            .iter()
            .filter(|_| flags.next().unwrap_or(false))
            .cloned()
            .collect();
        result
    }

    pub fn selection_summary(&self) -> String {
        let rows = self.candidates();
        let selected = rows.iter().filter(|r| r.checked).count();
        if self.session == SessionKind::Export {
            return format!("{selected} / {} labels selected", rows.len());
        }
        let count = |group| {
            rows.iter()
                .filter(|r| r.checked && r.group == Some(group))
                .count()
        };
        format!(
            "{selected} / {} selected — {} create, {} update, {} delete",
            rows.len(),
            count(ActionGroup::Create),
            count(ActionGroup::Update),
            count(ActionGroup::Delete)
        )
    }

    pub(super) fn select_rows(
        &mut self,
        group: Option<ActionGroup>,
        value: Option<bool>,
    ) {
        let rows: Vec<_> = self
            .candidates()
            .into_iter()
            .filter(|r| group.is_none() || r.group == group)
            .collect();
        let group_value = !rows.iter().all(|r| r.checked);
        for row in rows {
            self.check(
                row.id,
                value.unwrap_or(if group.is_some() {
                    group_value
                } else {
                    !row.checked
                }),
            );
        }
        self.repair_selection();
    }

    fn check(&mut self, id: EntryId, value: bool) {
        match &mut self.selection {
            Some(Selection::Export(prefs)) => {
                prefs.insert(id, value);
            }
            Some(Selection::Operations { checked, .. }) => {
                if let Some(flag) = checked.get_mut(id as usize) {
                    *flag = value;
                }
            }
            None => {}
        }
    }

    pub(super) fn toggle_row(&mut self) {
        if let Some(row) = self
            .candidates()
            .into_iter()
            .find(|r| Some(r.id) == self.selected)
        {
            self.check(row.id, !row.checked);
        }
        self.repair_selection();
    }

    pub(super) fn enter_workspace(&mut self) {
        let customized = matches!(&self.selection, Some(Selection::Operations { checked, .. }) if checked.iter().any(|v| !v));
        if customized {
            self.modal = Some(Modal {
                kind: ModalKind::Reset,
                choice: 1,
            });
        } else {
            self.edit_workspace();
        }
    }

    pub(super) fn edit_workspace(&mut self) {
        self.workspace = WorkspaceMode::Edit;
        self.mode = Mode::List;
        self.button = None;
        self.modal = None;
        self.repair_selection();
    }

    pub(super) fn done(&mut self) {
        if !matches!(self.mode, Mode::List) {
            return;
        }
        let labels = match self.document.labels() {
            Ok(labels) => labels,
            Err(error) => {
                self.error = error;
                return;
            }
        };
        if let Some(Selection::Operations {
            plan,
            checked,
            target,
            prune,
        }) = &mut self.selection
        {
            *plan = crate::plan::plan(&labels, target, *prune);
            *checked = vec![true; operation_count(plan)];
            self.error = "Plan updated — selections reset.".into();
        }
        self.workspace = WorkspaceMode::Select;
        self.button = None;
        self.repair_selection();
    }

    pub(super) fn confirm_finish(&mut self) {
        let payload = if self.session == SessionKind::Export {
            match self.selected_labels() {
                Ok(labels) => FinalSelection::Export(labels),
                Err(error) => {
                    self.error = error;
                    return;
                }
            }
        } else {
            let plan = self.selected_plan();
            if plan.is_empty() {
                return;
            }
            FinalSelection::Plan(plan)
        };
        self.modal = Some(Modal {
            kind: ModalKind::Finish(payload),
            choice: 1,
        });
        self.buttons.clear();
    }
}

fn operation_count(plan: &Plan) -> usize {
    plan.creates.len() + plan.updates.len() + plan.deletes.len()
}

fn operation_rows(
    plan: &Plan,
    target: &[Label],
    checked: &[bool],
) -> Vec<Candidate> {
    let mut values: Vec<_> = plan
        .creates
        .iter()
        .map(|l| (ActionGroup::Create, Draft::from(l)))
        .collect();
    values.extend(plan.updates.iter().map(|u| {
        let mut draft = Draft::from(&u.current);
        if u.current.color != u.desired.color {
            draft.color = format!(
                "{} -> {}",
                u.current.color.as_str(),
                u.desired.color.as_str()
            );
        }
        if u.current.description != u.desired.description {
            draft.description = format!(
                "{} -> {}",
                u.current.description, u.desired.description
            );
        }
        (ActionGroup::Update, draft)
    }));
    values.extend(plan.deletes.iter().map(|d| {
        let draft = target
            .iter()
            .find(|l| l.name == d.name)
            .map(Draft::from)
            .unwrap_or(Draft {
                name: d.name.clone(),
                color: String::new(),
                description: String::new(),
            });
        (ActionGroup::Delete, draft)
    }));
    values
        .into_iter()
        .enumerate()
        .map(|(i, (group, draft))| Candidate {
            id: i as EntryId,
            group: Some(group),
            draft,
            checked: checked.get(i).copied().unwrap_or(false),
        })
        .collect()
}
