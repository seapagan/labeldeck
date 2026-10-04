use super::label;
use labeldeck::edit::{
    model::Document,
    plan::{Operation, plan, same_labels},
};
use std::collections::BTreeMap;

fn rename(doc: &mut Document, index: usize, name: &str) {
    let entry = doc.entries()[index].clone();
    let mut draft = entry.draft;
    draft.name = name.into();
    doc.commit(entry.id, draft).unwrap();
}

fn check(initial: &[&str], final_names: &[Option<&str>]) -> Vec<Operation> {
    let mut doc =
        Document::from_labels(initial.iter().map(|n| label(n)).collect());
    for (index, name) in final_names.iter().enumerate() {
        match name {
            Some(name) => rename(&mut doc, index, name),
            None => {
                doc.delete(doc.entries()[index].id);
            }
        }
    }
    let plan = plan(&doc).unwrap();
    let current = apply_operations(initial, &plan.operations);
    for (id, name) in current.values() {
        assert_eq!(Some(name.as_str()), final_names[*id]);
    }
    assert_eq!(current.len(), final_names.iter().flatten().count());
    plan.operations
}

fn apply_operations(
    initial: &[&str],
    operations: &[Operation],
) -> BTreeMap<String, (usize, String)> {
    let mut current: BTreeMap<String, (usize, String)> = initial
        .iter()
        .enumerate()
        .map(|(i, n)| (n.to_lowercase(), (i, n.to_string())))
        .collect();
    let mut deleting = false;
    for op in operations {
        match op {
            Operation::Rename {
                current_name,
                desired,
            }
            | Operation::TemporaryRename {
                current_name,
                desired,
            } => {
                assert!(!deleting);
                let (id, _) =
                    current.remove(&current_name.to_lowercase()).unwrap();
                assert!(
                    current
                        .insert(
                            desired.match_key(),
                            (id, desired.name.clone())
                        )
                        .is_none()
                );
            }
            Operation::Delete { name } => {
                deleting = true;
                current.remove(&name.to_lowercase()).unwrap();
            }
            _ => panic!("unexpected operation {op:?}"),
        }
    }
    current
}

#[test]
fn unchanged_and_added_then_deleted_are_noops() {
    let mut doc = Document::from_labels(vec![label("A")]);
    assert!(plan(&doc).unwrap().operations.is_empty());
    let id = doc.add();
    doc.delete(id);
    assert!(plan(&doc).unwrap().operations.is_empty());
}

#[test]
fn simple_and_case_only_renames_need_one_patch() {
    for name in ["B", "a", "café/%"] {
        let ops = check(&["A"], &[Some(name)]);
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0], Operation::Rename { .. }));
    }
}

#[test]
fn chain_runs_from_the_free_end_without_temporary_names() {
    let ops = check(&["A", "B", "C"], &[Some("B"), Some("C"), Some("D")]);
    let names: Vec<_> = ops
        .iter()
        .map(|op| match op {
            Operation::Rename { current_name, .. } => current_name.as_str(),
            _ => panic!(),
        })
        .collect();
    assert_eq!(names, ["C", "B", "A"]);
}

#[test]
fn cycles_use_one_temporary_name() {
    for (initial, final_names) in [
        (vec!["A", "B"], vec![Some("B"), Some("A")]),
        (vec!["A", "B", "C"], vec![Some("B"), Some("C"), Some("A")]),
        (vec!["Éclair", "界"], vec![Some("界"), Some("éclair")]),
    ] {
        let ops = check(&initial, &final_names);
        assert_eq!(ops.len(), initial.len() + 1);
        assert_eq!(
            ops.iter()
                .filter(|op| matches!(op, Operation::TemporaryRename { .. }))
                .count(),
            1
        );
    }
}

#[test]
fn deleted_blocker_moves_then_is_deleted_last() {
    let ops = check(&["A", "B"], &[Some("B"), None]);
    assert!(
        matches!(&ops[0], Operation::TemporaryRename { current_name, .. } if current_name == "B")
    );
    assert!(
        matches!(&ops[1], Operation::Rename { current_name, .. } if current_name == "A")
    );
    assert!(
        matches!(&ops[2], Operation::Delete { name } if name.starts_with("labeldeck-edit-tmp-"))
    );
}

#[test]
fn temporary_names_avoid_all_original_and_final_names_ignoring_case() {
    let ops = check(
        &["A", "B", "LABELDECK-EDIT-TMP-1", "X"],
        &[
            Some("B"),
            Some("A"),
            Some("LABELDECK-EDIT-TMP-1"),
            Some("labeldeck-edit-tmp-2"),
        ],
    );
    assert!(ops.iter().any(|op| matches!(op, Operation::TemporaryRename { desired, .. } if desired.name == "labeldeck-edit-tmp-3")));
    assert_eq!(
        ops,
        check(
            &["A", "B", "LABELDECK-EDIT-TMP-1", "X"],
            &[
                Some("B"),
                Some("A"),
                Some("LABELDECK-EDIT-TMP-1"),
                Some("labeldeck-edit-tmp-2")
            ]
        )
    );
}

#[test]
fn create_blocked_by_deleted_label_preserves_delete_last() {
    let mut doc = Document::from_labels(vec![label("B")]);
    doc.delete(doc.entries()[0].id);
    let id = doc.add();
    doc.commit(id, (&label("b")).into()).unwrap();
    let plan = plan(&doc).unwrap();
    assert!(matches!(
        plan.operations[0],
        Operation::TemporaryRename { .. }
    ));
    assert!(matches!(plan.operations[1], Operation::Create(_)));
    assert!(matches!(plan.operations[2], Operation::Delete { .. }));
    assert_eq!(plan.summary.created, 1);
    assert_eq!(plan.summary.deleted, 1);
    assert_eq!(plan.summary.renamed, 0);
}

#[test]
fn updates_and_combined_renames_are_not_redundant() {
    for rename_to in ["A", "B"] {
        let mut doc = Document::from_labels(vec![label("A")]);
        let mut draft = doc.entries()[0].draft.clone();
        draft.name = rename_to.into();
        draft.color = "abcdef".into();
        draft.description = "updated".into();
        doc.commit(doc.entries()[0].id, draft).unwrap();
        let plan = plan(&doc).unwrap();
        assert_eq!(plan.operations.len(), 1);
        assert_eq!(plan.summary.updated, usize::from(rename_to == "A"));
        assert_eq!(plan.summary.renamed, usize::from(rename_to == "B"));
        match &plan.operations[0] {
            Operation::Update { desired, .. }
            | Operation::Rename { desired, .. } => {
                assert_eq!(desired.color.as_str(), "abcdef");
                assert_eq!(desired.description, "updated");
            }
            _ => panic!(),
        }
    }
}

#[test]
fn invalid_final_document_is_rejected() {
    let mut doc = Document::from_labels(vec![label("A"), label("B")]);
    rename(&mut doc, 1, "a");
    assert!(plan(&doc).is_err());
    assert!(
        plan(&Document::from_labels(vec![]))
            .unwrap()
            .operations
            .is_empty()
    );
}

#[test]
fn semantic_equality_ignores_order_but_preserves_exact_values() {
    assert!(same_labels(
        &[label("A"), label("B")],
        &[label("B"), label("A")]
    ));
    assert!(!same_labels(&[label("A")], &[label("a")]));
    let mut changed = label("A");
    changed.description = "changed".into();
    assert!(!same_labels(&[label("A")], &[changed]));
    assert!(!same_labels(&[label("A")], &[]));
}
