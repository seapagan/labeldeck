//! Collision-safe, identity-aware plans; destructive operations are last.

use std::collections::{BTreeMap, BTreeSet};

use super::model::{Document, EntryId};
use crate::labels::Label;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    Create(Label),
    Update {
        current_name: String,
        desired: Label,
    },
    Rename {
        current_name: String,
        desired: Label,
    },
    TemporaryRename {
        current_name: String,
        desired: Label,
    },
    Delete {
        name: String,
    },
}

impl Operation {
    pub fn description(&self) -> String {
        match self {
            Self::Create(label) => format!("create label {:?}", label.name),
            Self::Update { current_name, .. } => {
                format!("update label {current_name:?} (colour/description)")
            }
            Self::Rename {
                current_name,
                desired,
            } => format!(
                "rename label {current_name:?} to {:?} (with desired colour/description)",
                desired.name
            ),
            Self::TemporaryRename {
                current_name,
                desired,
            } => format!(
                "temporarily rename label {current_name:?} to {:?}",
                desired.name
            ),
            Self::Delete { name } => format!("delete label {name:?}"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeSummary {
    pub created: usize,
    pub renamed: usize,
    pub updated: usize,
    pub deleted: usize,
}

#[derive(Debug, Clone, Default)]
pub struct EditPlan {
    pub operations: Vec<Operation>,
    pub summary: ChangeSummary,
}

struct Planner {
    current: BTreeMap<EntryId, Label>,
    occupied: BTreeMap<String, EntryId>,
    reserved: BTreeSet<String>,
    deleting: BTreeSet<EntryId>,
    counter: usize,
    result: EditPlan,
}

impl Planner {
    fn new(document: &Document) -> Result<Self, String> {
        let labels = document.labels()?;
        let current: BTreeMap<_, _> = document
            .entries()
            .iter()
            .filter_map(|e| e.original.clone().map(|label| (e.id, label)))
            .collect();
        let occupied = current
            .iter()
            .map(|(&id, label)| (label.match_key(), id))
            .collect();
        let mut reserved: BTreeSet<_> =
            current.values().map(Label::match_key).collect();
        reserved.extend(labels.iter().map(Label::match_key));
        let deleting = document
            .entries()
            .iter()
            .filter(|e| e.deleted && e.original.is_some())
            .map(|e| e.id)
            .collect();
        Ok(Self {
            current,
            occupied,
            reserved,
            deleting,
            counter: 0,
            result: EditPlan::default(),
        })
    }

    fn plan_entries(&mut self, document: &Document) -> Result<(), String> {
        let mut renames = BTreeMap::new();
        let mut creates = Vec::new();
        let mut updates = Vec::new();
        for entry in document.entries().iter().filter(|e| !e.deleted) {
            let desired = entry.draft.label()?;
            match &entry.original {
                None => creates.push(desired),
                Some(original) if original.name != desired.name => {
                    renames.insert(entry.id, desired);
                }
                Some(original) if original != &desired => {
                    updates.push(Operation::Update {
                        current_name: original.name.clone(),
                        desired,
                    })
                }
                _ => {}
            }
        }
        self.result.summary = ChangeSummary {
            created: creates.len(),
            renamed: renames.len(),
            updated: updates.len(),
            deleted: self.deleting.len(),
        };
        self.renames(renames);
        self.result.operations.extend(updates);
        self.append_creates(creates);
        Ok(())
    }

    fn append_creates(&mut self, creates: Vec<Label>) {
        for label in creates {
            if let Some(&blocker) = self.occupied.get(&label.match_key()) {
                // Final uniqueness guarantees only a deleted identity can block a create.
                self.relocate(blocker);
            }
            self.result.operations.push(Operation::Create(label));
        }
    }

    fn append_deletes(&mut self) {
        for id in &self.deleting {
            self.result.operations.push(Operation::Delete {
                name: self.current[id].name.clone(),
            });
        }
    }

    fn move_to(&mut self, id: EntryId, desired: Label, temporary: bool) {
        let old = self
            .current
            .insert(id, desired.clone())
            .expect("existing identity");
        self.occupied.remove(&old.match_key());
        self.occupied.insert(desired.match_key(), id);
        let operation = if temporary {
            Operation::TemporaryRename {
                current_name: old.name,
                desired,
            }
        } else {
            Operation::Rename {
                current_name: old.name,
                desired,
            }
        };
        self.result.operations.push(operation);
    }

    fn relocate(&mut self, id: EntryId) {
        let name = loop {
            self.counter += 1;
            let name = format!("labeldeck-edit-tmp-{}", self.counter);
            if self.reserved.insert(name.to_lowercase()) {
                break name;
            }
        };
        let mut desired = self.current[&id].clone();
        desired.name = name;
        self.move_to(id, desired, true);
    }

    fn renames(&mut self, mut pending: BTreeMap<EntryId, Label>) {
        while !pending.is_empty() {
            let ready = pending
                .iter()
                .find(|(id, desired)| {
                    self.occupied
                        .get(&desired.match_key())
                        .is_none_or(|owner| owner == *id)
                })
                .map(|(&id, _)| id);
            if let Some(id) = ready {
                self.move_to(
                    id,
                    pending.remove(&id).expect("pending identity"),
                    false,
                );
                continue;
            }
            let blocker = pending
                .values()
                .filter_map(|desired| self.occupied.get(&desired.match_key()))
                .find(|id| self.deleting.contains(id))
                .copied();
            let id = blocker.unwrap_or_else(|| {
                *pending.first_key_value().expect("nonempty pending").0
            });
            self.relocate(id);
        }
    }
}

pub fn plan(document: &Document) -> Result<EditPlan, String> {
    let mut planner = Planner::new(document)?;
    planner.plan_entries(document)?;
    planner.append_deletes();
    Ok(planner.result)
}

pub fn same_labels(left: &[Label], right: &[Label]) -> bool {
    fn sorted(labels: &[Label]) -> Vec<(&str, &str, &str)> {
        let mut values: Vec<_> = labels
            .iter()
            .map(|l| {
                (l.name.as_str(), l.color.as_str(), l.description.as_str())
            })
            .collect();
        values.sort_unstable();
        values
    }
    sorted(left) == sorted(right)
}
