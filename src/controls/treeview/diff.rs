#![forbid(unsafe_code)]

//! Keyed refresh: reconciling the materialized tree against the model.
//!
//! [`reconcile`] is a pure function — no Win32, no model calls — so the
//! "keep expansion and selection, update only what changed" behaviour is
//! unit-tested rather than inferred from a running control.

use super::model::TreeModel;

/// A node the control has materialized (inserted natively).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry<K> {
    pub(crate) key: K,
    pub(crate) text: String,
    pub(crate) has_children: bool,
    /// `None` until the branch has been expanded once; `Some` (possibly empty)
    /// once its children have been loaded.
    pub(crate) children: Option<Vec<Entry<K>>>,
    /// The nonzero token stored in the native item's `lParam`.
    pub(crate) token: i64,
}

/// A node the model currently wants, with its already-loaded descendants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Desired<K> {
    pub(crate) key: K,
    pub(crate) text: String,
    pub(crate) has_children: bool,
    /// `Some` only when the old tree had this branch loaded, so the model was
    /// queried for its children too.
    pub(crate) children: Option<Vec<Desired<K>>>,
}

/// One edit at one tree level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Edit<K> {
    /// Same key in both trees: keep the native item, refreshing its text and
    /// children flag, and reconcile its loaded children.
    Keep {
        key: K,
        text: String,
        has_children: bool,
        children: Vec<Edit<K>>,
    },
    /// A key the native tree does not have: insert the whole subtree.
    Insert(Desired<K>),
    /// A key the model no longer has: delete the subtree.
    Remove { key: K },
}

/// Reconciles one level of the materialized tree (`existing`) against the
/// desired one, matching by key.
///
/// Nodes with the same key are [`Edit::Keep`]s (so their native handles, and
/// with them expansion and selection, survive), new keys are [`Edit::Insert`]s
/// and gone keys are [`Edit::Remove`]s. Kept branches recurse into their
/// already-loaded children. Pure and allocation-light; it neither reads nor
/// writes the control.
pub(crate) fn reconcile<K: Clone + PartialEq>(
    existing: &[Entry<K>],
    desired: &[Desired<K>],
) -> Vec<Edit<K>> {
    let mut edits = Vec::with_capacity(desired.len());
    let mut matched = vec![false; existing.len()];

    for wanted in desired {
        match existing.iter().position(|entry| entry.key == wanted.key) {
            Some(index) => {
                matched[index] = true;
                let entry = &existing[index];
                let children = match (&entry.children, &wanted.children) {
                    (Some(old), Some(new)) => reconcile(old, new),
                    _ => Vec::new(),
                };
                edits.push(Edit::Keep {
                    key: wanted.key.clone(),
                    text: wanted.text.clone(),
                    has_children: wanted.has_children,
                    children,
                });
            }
            None => edits.push(Edit::Insert(wanted.clone())),
        }
    }

    for (index, entry) in existing.iter().enumerate() {
        if !matched[index] {
            edits.push(Edit::Remove {
                key: entry.key.clone(),
            });
        }
    }
    edits
}

/// Builds the desired tree for `parent`, recursing only into branches the old
/// tree already had loaded (`existing`). Unloaded branches stay leaves, so
/// `refresh` never pulls more from the model than was already on screen.
pub(crate) fn desired_tree<K, Model>(
    model: &Model,
    parent: Option<&K>,
    existing: &[Entry<K>],
) -> Vec<Desired<K>>
where
    K: Clone + PartialEq,
    Model: TreeModel<Key = K> + ?Sized,
{
    model
        .children(parent)
        .into_iter()
        .map(|node| {
            let children = existing
                .iter()
                .find(|entry| entry.key == node.key)
                .and_then(|entry| entry.children.as_ref())
                .map(|old| desired_tree(model, Some(&node.key), old));
            Desired {
                key: node.key,
                text: node.text,
                has_children: node.has_children,
                children,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::treeview::model::Node;

    fn entry<'a>(
        key: &'a str,
        text: &str,
        children: Option<Vec<Entry<&'a str>>>,
    ) -> Entry<&'a str> {
        Entry {
            key,
            text: text.to_string(),
            has_children: children.is_some(),
            children,
            token: 0,
        }
    }

    fn desired<'a>(
        key: &'a str,
        text: &str,
        children: Option<Vec<Desired<&'a str>>>,
    ) -> Desired<&'a str> {
        Desired {
            key,
            text: text.to_string(),
            has_children: children.is_some(),
            children,
        }
    }

    #[test]
    fn unchanged_tree_is_all_keeps() {
        let existing = vec![entry("a", "A", None), entry("b", "B", None)];
        let wanted = vec![desired("a", "A", None), desired("b", "B", None)];
        let edits = reconcile(&existing, &wanted);
        assert_eq!(
            edits,
            vec![
                Edit::Keep {
                    key: "a",
                    text: "A".to_string(),
                    has_children: false,
                    children: Vec::new()
                },
                Edit::Keep {
                    key: "b",
                    text: "B".to_string(),
                    has_children: false,
                    children: Vec::new()
                },
            ]
        );
    }

    #[test]
    fn reordering_is_keeps_in_the_new_order() {
        let existing = vec![entry("a", "A", None), entry("b", "B", None)];
        let wanted = vec![desired("b", "B", None), desired("a", "A", None)];
        let edits = reconcile(&existing, &wanted);
        let keys: Vec<&str> = edits
            .iter()
            .map(|edit| match edit {
                Edit::Keep { key, .. } => *key,
                _ => panic!("expected keeps"),
            })
            .collect();
        assert_eq!(keys, ["b", "a"]);
    }

    #[test]
    fn inserts_come_in_position_and_removes_at_the_end() {
        let existing = vec![entry("a", "A", None), entry("gone", "G", None)];
        let wanted = vec![desired("a", "A", None), desired("new", "N", None)];
        let edits = reconcile(&existing, &wanted);
        assert_eq!(
            edits,
            vec![
                Edit::Keep {
                    key: "a",
                    text: "A".to_string(),
                    has_children: false,
                    children: Vec::new()
                },
                Edit::Insert(desired("new", "N", None)),
                Edit::Remove { key: "gone" },
            ]
        );
    }

    #[test]
    fn changed_text_is_a_keep_with_new_text() {
        let existing = vec![entry("inbox", "INBOX (3)", None)];
        let wanted = vec![desired("inbox", "INBOX (4)", None)];
        assert_eq!(
            reconcile(&existing, &wanted),
            vec![Edit::Keep {
                key: "inbox",
                text: "INBOX (4)".to_string(),
                has_children: false,
                children: Vec::new()
            }]
        );
    }

    #[test]
    fn loaded_children_recurse_by_key() {
        let existing = vec![entry(
            "a",
            "A",
            Some(vec![entry("x", "X", None), entry("y", "Y", None)]),
        )];
        let wanted = vec![desired(
            "a",
            "A",
            Some(vec![desired("x", "X", None), desired("z", "Z", None)]),
        )];
        let edits = reconcile(&existing, &wanted);
        assert_eq!(
            edits,
            vec![Edit::Keep {
                key: "a",
                text: "A".to_string(),
                has_children: true,
                children: vec![
                    Edit::Keep {
                        key: "x",
                        text: "X".to_string(),
                        has_children: false,
                        children: Vec::new()
                    },
                    Edit::Insert(desired("z", "Z", None)),
                    Edit::Remove { key: "y" },
                ],
            }]
        );
    }

    #[test]
    fn unloaded_branch_does_not_recurse() {
        // The model produced a `Desired` with no children because the old
        // entry was unloaded; reconcile must keep the branch without emitting
        // any child edit (nothing is shown below it yet).
        let existing = vec![entry("a", "A", None)];
        let wanted = vec![desired("a", "A", None)];
        let edits = reconcile(&existing, &wanted);
        assert_eq!(
            edits,
            vec![Edit::Keep {
                key: "a",
                text: "A".to_string(),
                has_children: false,
                children: Vec::new()
            }]
        );
    }

    struct FakeModel;

    impl TreeModel for FakeModel {
        type Key = &'static str;

        fn children(&self, parent: Option<&&'static str>) -> Vec<Node<&'static str>> {
            match parent {
                None => vec![Node::branch("a", "A"), Node::leaf("b", "B")],
                Some(&"a") => vec![Node::leaf("x", "X")],
                _ => Vec::new(),
            }
        }
    }

    #[test]
    fn desired_tree_only_loads_already_loaded_branches() {
        // "a" was loaded, "b" was not: the roots still come back, but only
        // "a" recurses into the model.
        let existing = vec![entry("a", "A", Some(vec![entry("x", "X", None)]))];
        let wanted = desired_tree(&FakeModel, None, &existing);
        assert_eq!(wanted.len(), 2, "both roots are wanted");
        assert_eq!(
            wanted[0],
            desired("a", "A", Some(vec![desired("x", "X", None)]))
        );
        assert_eq!(wanted[1], desired("b", "B", None));
    }
}
