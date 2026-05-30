//! Transactions: mutate a [`Node`] tree while recording a replayable,
//! invertible [`Change`] log in the same pass.
//!
//! A [`Transform`] borrows the tree mutably; each builder method both applies an
//! edit in place (via the same engine as [`apply`](crate::apply)) **and** records
//! the corresponding [`Change`]. Calling [`finish`](Transform::finish) returns
//! the recorded list, which — applied to a clone of the *original* tree —
//! reproduces the transformed tree, and whose [`invert`](crate::invert) is its
//! undo. This unifies the mutation API with [`diff`](crate::diff): instead of
//! editing and then diffing to recover a patch, you get the patch for free.
//!
//! ```
//! use tiptap_rusty_parser::{apply, Node};
//!
//! let mut doc = Node::element("doc").with_child(Node::element("paragraph"));
//! let original = doc.clone();
//!
//! let changes = {
//!     let mut tx = doc.transform();
//!     tx.set_attr(vec![0], "level", 1).unwrap();
//!     tx.insert(vec![], 1, Node::element("paragraph")).unwrap();
//!     tx.finish()
//! };
//!
//! // The recorded log reproduces `doc` from a clone of the original.
//! let mut replay = original.clone();
//! apply(&mut replay, &changes).unwrap();
//! assert_eq!(replay, doc);
//!
//! // …and inverts to an undo that restores the original.
//! let undo = original.invert(&changes).unwrap();
//! let mut back = doc.clone();
//! apply(&mut back, &undo).unwrap();
//! assert_eq!(back, original);
//! ```

use crate::diff::{apply, ApplyError, Change};
use crate::node::{Mark, Node};
use serde_json::Value;

/// A mutation transaction over a [`Node`] tree: edits apply in place and are
/// recorded as a [`Change`] log. Create with [`Node::transform`].
///
/// Each builder returns `Result<&mut Self, ApplyError>` so calls chain with `?`;
/// an error (e.g. a path that doesn't resolve) leaves the tree as mutated by the
/// changes recorded so far.
pub struct Transform<'a> {
    root: &'a mut Node,
    changes: Vec<Change>,
}

impl Node {
    /// Begin a [`Transform`] over this tree.
    pub fn transform(&mut self) -> Transform<'_> {
        Transform {
            root: self,
            changes: Vec::new(),
        }
    }
}

impl<'a> Transform<'a> {
    /// Apply `change` in place and record it.
    fn push(&mut self, change: Change) -> Result<&mut Self, ApplyError> {
        apply(self.root, std::slice::from_ref(&change))?;
        self.changes.push(change);
        Ok(self)
    }

    /// Set (insert or overwrite) attribute `key` on the node at `path`.
    pub fn set_attr(
        &mut self,
        path: Vec<usize>,
        key: impl Into<String>,
        value: impl Into<Value>,
    ) -> Result<&mut Self, ApplyError> {
        self.push(Change::SetAttr {
            path,
            key: key.into(),
            value: value.into(),
        })
    }

    /// Remove attribute `key` from the node at `path`.
    pub fn remove_attr(
        &mut self,
        path: Vec<usize>,
        key: impl Into<String>,
    ) -> Result<&mut Self, ApplyError> {
        self.push(Change::RemoveAttr {
            path,
            key: key.into(),
        })
    }

    /// Set the text payload of the node at `path` (`None` clears it).
    pub fn set_text(
        &mut self,
        path: Vec<usize>,
        text: Option<String>,
    ) -> Result<&mut Self, ApplyError> {
        self.push(Change::SetText { path, text })
    }

    /// Replace the whole mark list of the node at `path` (`None` clears it).
    pub fn set_marks(
        &mut self,
        path: Vec<usize>,
        marks: Option<Vec<Mark>>,
    ) -> Result<&mut Self, ApplyError> {
        self.push(Change::SetMarks { path, marks })
    }

    /// Set (insert or overwrite) unknown top-level field `key` on the node at `path`.
    pub fn set_extra(
        &mut self,
        path: Vec<usize>,
        key: impl Into<String>,
        value: impl Into<Value>,
    ) -> Result<&mut Self, ApplyError> {
        self.push(Change::SetExtra {
            path,
            key: key.into(),
            value: value.into(),
        })
    }

    /// Remove unknown top-level field `key` from the node at `path`.
    pub fn remove_extra(
        &mut self,
        path: Vec<usize>,
        key: impl Into<String>,
    ) -> Result<&mut Self, ApplyError> {
        self.push(Change::RemoveExtra {
            path,
            key: key.into(),
        })
    }

    /// Insert `node` as a child of the node at `path` (the parent), at `index`.
    pub fn insert(
        &mut self,
        path: Vec<usize>,
        index: usize,
        node: Node,
    ) -> Result<&mut Self, ApplyError> {
        self.push(Change::Insert { path, index, node })
    }

    /// Remove the child at `index` of the node at `path` (the parent).
    pub fn remove(&mut self, path: Vec<usize>, index: usize) -> Result<&mut Self, ApplyError> {
        self.push(Change::Remove { path, index })
    }

    /// Replace the node at `path` wholesale.
    pub fn replace(&mut self, path: Vec<usize>, node: Node) -> Result<&mut Self, ApplyError> {
        self.push(Change::Replace { path, node })
    }

    /// Relocate the child at `from` to `to` within the parent at `path`, without
    /// cloning its subtree. See [`Change::Move`].
    pub fn move_child(
        &mut self,
        path: Vec<usize>,
        from: usize,
        to: usize,
    ) -> Result<&mut Self, ApplyError> {
        self.push(Change::Move { path, from, to })
    }

    /// The changes recorded so far, without consuming the transaction.
    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    /// Finish the transaction, returning the recorded [`Change`] log.
    pub fn finish(self) -> Vec<Change> {
        self.changes
    }
}
