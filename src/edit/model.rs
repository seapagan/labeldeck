//! Stable label identities and document-only snapshot history.

use std::collections::HashSet;

use crate::labels::{Label, LabelColor};

pub type EntryId = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub name: String,
    pub color: String,
    pub description: String,
}

impl From<&Label> for Draft {
    fn from(label: &Label) -> Self {
        Self {
            name: label.name.clone(),
            color: label.color.as_str().into(),
            description: label.description.clone(),
        }
    }
}

impl Draft {
    pub fn label(&self) -> Result<Label, String> {
        let label = Label {
            name: self.name.clone(),
            color: LabelColor::parse(&self.color)?,
            description: self.description.clone(),
        };
        label.validate()?;
        Ok(label)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorEntry {
    pub id: EntryId,
    pub original: Option<Label>,
    pub draft: Draft,
    pub deleted: bool,
}

#[derive(Debug, Clone)]
pub struct Document {
    entries: Vec<EditorEntry>,
    next_id: EntryId,
    undo: Vec<Vec<EditorEntry>>,
    redo: Vec<Vec<EditorEntry>>,
}

impl Document {
    pub fn from_labels(labels: Vec<Label>) -> Self {
        let entries: Vec<_> = labels
            .into_iter()
            .enumerate()
            .map(|(id, original)| EditorEntry {
                id: id as EntryId,
                draft: Draft::from(&original),
                original: Some(original),
                deleted: false,
            })
            .collect();
        Self {
            next_id: entries.len() as EntryId,
            entries,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn entries(&self) -> &[EditorEntry] {
        &self.entries
    }

    pub fn labels(&self) -> Result<Vec<Label>, String> {
        let mut names = HashSet::new();
        self.entries.iter().filter(|entry| !entry.deleted).map(|entry| {
            let label = entry.draft.label()?;
            if !names.insert(label.match_key()) {
                return Err(format!("duplicate label name {:?}: names must be unique ignoring case", label.name));
            }
            Ok(label)
        }).collect()
    }

    fn remember(&mut self) {
        self.undo.push(self.entries.clone());
        self.redo.clear();
    }

    pub fn add(&mut self) -> EntryId {
        self.remember();
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(EditorEntry {
            id,
            original: None,
            deleted: false,
            draft: Draft {
                name: String::new(),
                color: "ededed".into(),
                description: String::new(),
            },
        });
        id
    }

    /// Commit a pending new label with one creation snapshot, after validation.
    pub fn create(&mut self, draft: Draft) -> Result<EntryId, String> {
        let draft = Draft::from(&draft.label()?);
        let id = self.add();
        self.entries.last_mut().expect("new entry").draft = draft;
        Ok(id)
    }

    pub fn delete(&mut self, id: EntryId) -> bool {
        let Some(index) =
            self.entries.iter().position(|e| e.id == id && !e.deleted)
        else {
            return false;
        };
        self.remember();
        self.entries[index].deleted = true;
        true
    }

    /// Validate a staged row; final uniqueness is checked at Apply so
    /// users can stage swaps and chains through temporarily duplicate names.
    pub fn commit(
        &mut self,
        id: EntryId,
        draft: Draft,
    ) -> Result<bool, String> {
        let Some(index) =
            self.entries.iter().position(|e| e.id == id && !e.deleted)
        else {
            return Err("the selected label no longer exists".into());
        };
        let draft = Draft::from(&draft.label()?);
        if self.entries[index].draft == draft {
            return Ok(false);
        }
        self.remember();
        self.entries[index].draft = draft;
        Ok(true)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo
            .push(std::mem::replace(&mut self.entries, previous));
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.entries, next));
        true
    }
}

pub fn matches_filter(draft: &Draft, query: &str) -> bool {
    let query = query.to_lowercase();
    draft.name.to_lowercase().contains(&query)
}

pub fn visible_ids(document: &Document, query: &str) -> Vec<EntryId> {
    document
        .entries()
        .iter()
        .filter(|e| !e.deleted && matches_filter(&e.draft, query))
        .map(|e| e.id)
        .collect()
}
