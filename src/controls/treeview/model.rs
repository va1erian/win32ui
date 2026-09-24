#![forbid(unsafe_code)]

//! The tree view's typed, lazily-loaded data model: [`TreeModel`] and the
//! [`Node`] it returns.

use std::hash::Hash;

/// One node handed back by a [`TreeModel`].
///
/// The [`key`](Node::key) is the node's stable identity: the tree keeps it, and
/// `refresh` matches nodes to the model by key rather than rebuilding. Keys
/// therefore have to be unique among siblings (and ideally in the whole tree).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node<K> {
    /// Stable identity of the node.
    pub key: K,
    /// Displayed text.
    pub text: String,
    /// Whether to show an expand button before the node's children load.
    pub has_children: bool,
}

impl<K> Node<K> {
    /// A leaf: no expand button.
    pub fn leaf(key: K, text: impl Into<String>) -> Node<K> {
        Node {
            key,
            text: text.into(),
            has_children: false,
        }
    }

    /// A branch that can be expanded to load more children.
    pub fn branch(key: K, text: impl Into<String>) -> Node<K> {
        Node {
            key,
            text: text.into(),
            has_children: true,
        }
    }
}

/// Supplies a [`TreeView`](super::TreeView) with nodes.
///
/// The tree is loaded lazily: [`children`](TreeModel::children) is called for
/// the roots at construction, and for a node's children only when that node is
/// first expanded. `refresh` re-reads only the branches that are already
/// loaded, so an unopened branch costs nothing.
pub trait TreeModel {
    /// The node's stable identity.
    type Key: Clone + Eq + Hash;

    /// The children of `parent`, or the roots when `parent` is `None`.
    fn children(&self, parent: Option<&Self::Key>) -> Vec<Node<Self::Key>>;
}
