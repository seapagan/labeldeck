use labeldeck::edit::model::{Document, Draft, matches_filter, visible_ids};

use super::label;

fn document() -> Document {
    Document::from_labels(vec![label("bug")])
}

#[test]
fn add_starts_empty_with_neutral_colour_and_stable_id() {
    let mut doc = document();
    let id = doc.add();
    let entry = doc.entries().last().unwrap();
    assert_eq!(entry.id, id);
    assert_eq!(entry.draft.color, "ededed");
    assert!(entry.original.is_none());
    assert!(doc.labels().is_err());
    assert!(doc.undo());
    assert_ne!(doc.add(), id);
}

#[test]
fn rename_and_case_only_rename_preserve_original_identity() {
    for name in ["renamed", "Bug"] {
        let mut doc = document();
        let entry = doc.entries()[0].clone();
        let mut draft = entry.draft.clone();
        draft.name = name.into();
        assert!(doc.commit(entry.id, draft).unwrap());
        assert_eq!(doc.entries()[0].id, entry.id);
        assert_eq!(doc.entries()[0].original, Some(label("bug")));
        assert_eq!(doc.labels().unwrap()[0].name, name);
    }
}

#[test]
fn field_commit_has_one_history_entry_and_normalizes_colour() {
    let mut doc = document();
    let id = doc.entries()[0].id;
    let mut draft = doc.entries()[0].draft.clone();
    for c in "updated".chars() {
        draft.description.push(c);
    }
    assert!(!doc.can_undo());
    draft.color = "#AbC123".into();
    doc.commit(id, draft).unwrap();
    assert_eq!(doc.labels().unwrap()[0].color.as_str(), "abc123");
    assert_eq!(doc.labels().unwrap()[0].description, "updated");
    assert!(doc.undo());
    assert!(!doc.can_undo());
    assert_eq!(doc.labels().unwrap(), vec![label("bug")]);
    assert!(doc.redo());
    assert_eq!(doc.labels().unwrap()[0].description, "updated");
}

#[test]
fn unchanged_commit_does_not_record_history() {
    let mut doc = document();
    let entry = doc.entries()[0].clone();
    assert!(!doc.commit(entry.id, entry.draft).unwrap());
    assert!(!doc.can_undo());
    assert!(!doc.undo());
    assert!(!doc.redo());
}

#[test]
fn validated_creation_rejects_invalid_drafts_without_history() {
    let mut doc = document();
    let original = doc.entries()[0].draft.clone();
    let mut draft = original.clone();
    draft.color = "bad".into();
    assert!(doc.create(draft).is_err());
    assert_eq!(doc.entries().len(), 1);
    assert!(!doc.can_undo());
    let id = doc.create(original).unwrap();
    assert_eq!(id, 1);
    assert!(doc.undo());
    assert_eq!(doc.labels().unwrap(), vec![label("bug")]);
    assert!(!doc.can_undo());
}

#[test]
fn delete_and_history_restore_document() {
    let mut doc = document();
    let id = doc.entries()[0].id;
    assert!(doc.delete(id));
    assert!(doc.labels().unwrap().is_empty());
    assert!(!doc.delete(id));
    assert!(doc.undo());
    assert_eq!(doc.labels().unwrap(), vec![label("bug")]);
    assert!(doc.redo());
    assert!(doc.labels().unwrap().is_empty());
}

#[test]
fn new_mutation_after_undo_clears_redo() {
    let mut doc = document();
    doc.delete(doc.entries()[0].id);
    doc.undo();
    assert!(doc.can_redo());
    doc.add();
    assert!(!doc.can_redo());
}

#[test]
fn added_then_deleted_has_no_final_label() {
    let mut doc = document();
    let id = doc.add();
    assert!(doc.delete(id));
    assert_eq!(doc.labels().unwrap(), vec![label("bug")]);
}

#[test]
fn final_duplicates_fail_but_intermediate_renames_can_be_staged() {
    let mut doc = Document::from_labels(vec![label("bug"), label("docs")]);
    let entry = doc.entries()[1].clone();
    let mut draft = entry.draft;
    draft.name = "BUG".into();
    doc.commit(entry.id, draft).unwrap();
    assert!(doc.labels().unwrap_err().contains("duplicate"));
    doc.delete(doc.entries()[0].id);
    assert!(doc.labels().is_ok());
}

#[test]
fn commits_validate_domain_boundaries_without_changing_history() {
    let invalid = [
        Draft {
            name: String::new(),
            color: "ededed".into(),
            description: String::new(),
        },
        Draft {
            name: "界".repeat(51),
            color: "ededed".into(),
            description: String::new(),
        },
        Draft {
            name: "bug".into(),
            color: "fff".into(),
            description: String::new(),
        },
        Draft {
            name: "bug".into(),
            color: "ededed".into(),
            description: "界".repeat(101),
        },
    ];
    for draft in invalid {
        let mut doc = document();
        assert!(doc.commit(doc.entries()[0].id, draft).is_err());
        assert!(!doc.can_undo());
    }
    let mut doc = document();
    doc.commit(
        doc.entries()[0].id,
        Draft {
            name: "界".repeat(50),
            color: "ffffff".into(),
            description: "界".repeat(100),
        },
    )
    .unwrap();
    assert!(doc.labels().is_ok());
}

#[test]
fn unknown_entries_cannot_be_committed_or_deleted() {
    let mut doc = document();
    assert!(doc.commit(999, doc.entries()[0].draft.clone()).is_err());
    assert!(!doc.delete(999));
}

#[test]
fn filtering_matches_name_description_case_and_empty() {
    let mut doc = document();
    let mut draft = doc.entries()[0].draft.clone();
    draft.description = "Documentation Éclair".into();
    doc.commit(doc.entries()[0].id, draft.clone()).unwrap();
    for query in ["BUG", "DOC", "éCLAIR", ""] {
        assert!(matches_filter(&draft, query));
        assert_eq!(visible_ids(&doc, query), vec![doc.entries()[0].id]);
    }
    assert!(visible_ids(&doc, "missing").is_empty());
    doc.delete(doc.entries()[0].id);
    assert!(visible_ids(&doc, "").is_empty());
}
