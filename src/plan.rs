//! Diff/planning engine.
//!
//! Turns a canonical label set plus the current remote state into a
//! concrete, deterministic plan. The plan is the single source of truth
//! shared by `diff` (report) and `sync` (execute), so both commands always
//! agree on what would happen.

use crate::labels::Label;

/// An in-place update of an existing remote label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    /// The label name exactly as it exists on the target repository.
    /// Updates never rename, so issue/PR associations are preserved.
    pub current_name: String,
    /// The desired colour and description from the canonical set.
    pub desired: Label,
}

/// A deletion of a target-only label. Only ever planned when pruning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deletion {
    pub name: String,
}

/// The complete set of operations implied by a canonical/remote pair.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Canonical labels missing from the target, sorted by name.
    pub creates: Vec<Label>,
    /// Existing labels whose colour or description differs, sorted by name.
    pub updates: Vec<Update>,
    /// Target-only labels to delete, sorted by name. Empty unless pruning.
    pub deletes: Vec<Deletion>,
    /// Names of labels already identical on both sides, sorted.
    pub unchanged: Vec<String>,
    /// Target-only labels that will be kept. Empty when pruning.
    pub retained: Vec<String>,
}

impl Plan {
    /// True when the canonical set and target already agree (ignoring
    /// target-only labels when they are retained rather than deleted).
    pub fn is_empty(&self) -> bool {
        self.creates.is_empty()
            && self.updates.is_empty()
            && self.deletes.is_empty()
    }
}

/// Compute the plan that moves `remote` to match `canonical`.
///
/// Matching is case-insensitive, mirroring GitHub's per-repository label
/// name uniqueness. A matched label is updated in place only when its
/// colour or description differs; the remote name (including its case) is
/// never changed, so a case-only difference is treated as unchanged.
/// Target-only labels become deletions when `prune` is set and are
/// otherwise recorded as retained.
pub fn plan(canonical: &[Label], remote: &[Label], prune: bool) -> Plan {
    let mut result = Plan::default();
    let remote_by_key: std::collections::HashMap<String, &Label> = remote
        .iter()
        .map(|label| (label.match_key(), label))
        .collect();

    for want in canonical {
        match remote_by_key.get(&want.match_key()) {
            None => result.creates.push(want.clone()),
            Some(existing) => {
                if existing.color != want.color
                    || existing.description != want.description
                {
                    result.updates.push(Update {
                        current_name: existing.name.clone(),
                        desired: want.clone(),
                    });
                } else {
                    result.unchanged.push(existing.name.clone());
                }
            }
        }
    }

    let canonical_keys: std::collections::HashSet<String> =
        canonical.iter().map(Label::match_key).collect();
    for existing in remote {
        if !canonical_keys.contains(&existing.match_key()) {
            if prune {
                result.deletes.push(Deletion {
                    name: existing.name.clone(),
                });
            } else {
                result.retained.push(existing.name.clone());
            }
        }
    }

    result.creates.sort_by(|a, b| a.name.cmp(&b.name));
    result
        .updates
        .sort_by(|a, b| a.current_name.cmp(&b.current_name));
    result.deletes.sort_by(|a, b| a.name.cmp(&b.name));
    result.unchanged.sort();
    result.retained.sort();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::LabelColor;

    fn label(name: &str, color: &str, description: &str) -> Label {
        Label {
            name: name.to_string(),
            color: LabelColor::parse(color).unwrap(),
            description: description.to_string(),
        }
    }

    #[test]
    fn identical_sets_produce_no_operations() {
        let canonical = vec![
            label("bug", "d73a4a", "broken"),
            label("docs", "0075ca", ""),
        ];
        let remote = canonical.clone();
        let result = plan(&canonical, &remote, false);
        assert!(result.is_empty());
        assert_eq!(result.unchanged.len(), 2);
        assert_eq!(result.unchanged, ["bug", "docs"]);
    }

    #[test]
    fn identical_up_to_case_is_unchanged() {
        let canonical = vec![label("Bug", "d73a4a", "broken")];
        let remote = vec![label("bug", "D73A4A", "broken")];
        let result = plan(&canonical, &remote, false);
        assert!(result.is_empty());
        assert_eq!(result.unchanged, ["bug"]);
    }

    #[test]
    fn null_and_empty_descriptions_match() {
        // Remote `null` descriptions arrive as empty strings, so both
        // sides normalise to "" and no update is planned.
        let canonical = vec![label("docs", "0075ca", "")];
        let remote = vec![label("docs", "0075ca", "")];
        assert!(plan(&canonical, &remote, false).is_empty());
    }

    #[test]
    fn missing_canonical_labels_are_created() {
        let canonical =
            vec![label("bug", "d73a4a", ""), label("new", "00ff00", "fresh")];
        let remote = vec![label("bug", "d73a4a", "")];
        let result = plan(&canonical, &remote, false);
        assert_eq!(result.creates.len(), 1);
        assert_eq!(result.creates[0].name, "new");
        assert!(result.updates.is_empty());
    }

    #[test]
    fn changed_colour_or_description_updates_in_place() {
        let canonical = vec![
            label("bug", "ff0000", "broken badly"),
            label("docs", "0075ca", "new description"),
        ];
        let remote = vec![
            label("bug", "d73a4a", "broken"),
            label("docs", "0075ca", "old description"),
        ];
        let result = plan(&canonical, &remote, false);
        assert_eq!(result.updates.len(), 2);
        assert_eq!(result.updates[0].current_name, "bug");
        assert_eq!(result.updates[0].desired.color.as_str(), "FF0000");
        assert_eq!(result.updates[1].current_name, "docs");
    }

    #[test]
    fn remote_name_case_is_preserved_in_updates() {
        let canonical = vec![label("BUG", "ff0000", "broken badly")];
        let remote = vec![label("bug", "d73a4a", "broken")];
        let result = plan(&canonical, &remote, false);
        assert_eq!(result.updates[0].current_name, "bug");
        assert_eq!(result.updates[0].desired.name, "BUG");
    }

    #[test]
    fn target_only_labels_are_retained_without_prune() {
        let canonical = vec![label("bug", "d73a4a", "")];
        let remote =
            vec![label("bug", "d73a4a", ""), label("legacy", "bbbccc", "")];
        let result = plan(&canonical, &remote, false);
        assert!(result.deletes.is_empty());
        assert_eq!(result.retained, ["legacy"]);
    }

    #[test]
    fn target_only_labels_are_deleted_with_prune() {
        let canonical = vec![label("bug", "d73a4a", "")];
        let remote = vec![
            label("bug", "d73a4a", ""),
            label("legacy", "bbbccc", ""),
            label("ancient", "aaa222", ""),
        ];
        let result = plan(&canonical, &remote, true);
        assert!(result.retained.is_empty());
        assert_eq!(
            result
                .deletes
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>(),
            ["ancient", "legacy"]
        );
    }

    #[test]
    fn empty_canonical_with_prune_deletes_everything() {
        let remote =
            vec![label("bug", "d73a4a", ""), label("docs", "0075ca", "")];
        let result = plan(&[], &remote, true);
        assert_eq!(result.deletes.len(), 2);
        assert!(result.is_empty() == false);
    }

    #[test]
    fn mixed_plan_separates_all_groups() {
        let canonical = vec![
            label("bug", "ff0000", "changed"), // update
            label("docs", "0075ca", "documentation"), // unchanged
            label("feature", "a2eeef", "new idea"), // create
        ];
        let remote = vec![
            label("bug", "d73a4a", "broken"),
            label("docs", "0075ca", "documentation"),
            label("stale", "cccddd", "old"),
        ];
        let result = plan(&canonical, &remote, true);
        assert_eq!(result.creates.len(), 1);
        assert_eq!(result.updates.len(), 1);
        assert_eq!(result.deletes.len(), 1);
        assert_eq!(result.unchanged, ["docs"]);
        assert!(result.retained.is_empty());
    }

    #[test]
    fn operations_are_deterministically_ordered() {
        let canonical_unsorted = vec![
            label("zebra", "111111", ""),
            label("ant", "222222", ""),
            label("mid", "333333", ""),
        ];
        let first = plan(&canonical_unsorted, &[], false);
        let second = plan(&canonical_unsorted, &[], false);
        assert_eq!(first, second);
        assert_eq!(
            first
                .creates
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            ["ant", "mid", "zebra"]
        );
    }

    #[test]
    fn prune_disabled_plan_is_not_empty_when_deletes_would_apply() {
        // Sanity in the other direction: with prune off, target-only
        // labels must not make the plan "non-empty" for exit-code purposes.
        let canonical = vec![label("bug", "d73a4a", "")];
        let remote =
            vec![label("bug", "d73a4a", ""), label("legacy", "bbbccc", "")];
        let result = plan(&canonical, &remote, false);
        assert!(result.is_empty(), "retained extras are not changes");
    }
}
