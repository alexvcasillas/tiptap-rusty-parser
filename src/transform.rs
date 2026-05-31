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

use crate::block::{BlockError, BlockRange};
use crate::diff::{apply, ApplyError, Change};
use crate::node::{Mark, Node};
use crate::range::{Position, Range};
use serde_json::{Map, Value};

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

/// Block-structural builders. Unlike the field/child ops above (which map to a
/// single [`Change`]), these restructure the tree and are recorded by running
/// the in-place edit and recovering the patch via [`diff`](crate::diff) — so
/// they clone the tree once (only on this transaction path; the direct
/// [`Node`] methods stay clone-free).
impl Transform<'_> {
    fn record_structural<F>(&mut self, f: F) -> Result<&mut Self, BlockError>
    where
        F: FnOnce(&mut Node) -> Result<(), BlockError>,
    {
        let before = self.root.clone();
        f(self.root)?;
        let patch = before.diff(self.root);
        self.changes.extend(patch);
        Ok(self)
    }

    /// Record [`Node::set_block_type`].
    pub fn set_block_type(
        &mut self,
        path: Vec<usize>,
        new_type: impl Into<String>,
        attrs: Option<Map<String, Value>>,
    ) -> Result<&mut Self, BlockError> {
        self.record_structural(|root| root.set_block_type(&path, new_type, attrs))
    }

    /// Record [`Node::split_block`].
    pub fn split_block(
        &mut self,
        path: Vec<usize>,
        at: usize,
        depth: usize,
    ) -> Result<&mut Self, BlockError> {
        self.record_structural(|root| root.split_block(&path, at, depth))
    }

    /// Record [`Node::split_block_at`].
    pub fn split_block_at(
        &mut self,
        path: Vec<usize>,
        pos: Position,
        depth: usize,
    ) -> Result<&mut Self, BlockError> {
        self.record_structural(|root| root.split_block_at(&path, pos, depth))
    }

    /// Record [`Node::join_blocks`].
    pub fn join_blocks(
        &mut self,
        parent: Vec<usize>,
        index: usize,
    ) -> Result<&mut Self, BlockError> {
        self.record_structural(|root| root.join_blocks(&parent, index))
    }

    /// Record [`Node::wrap`].
    pub fn wrap(
        &mut self,
        path: Vec<usize>,
        wrapper_type: impl Into<String>,
        attrs: Option<Map<String, Value>>,
    ) -> Result<&mut Self, BlockError> {
        self.record_structural(|root| root.wrap(&path, wrapper_type, attrs))
    }

    /// Record [`Node::wrap_range`].
    pub fn wrap_range(
        &mut self,
        range: BlockRange,
        wrapper_type: impl Into<String>,
        attrs: Option<Map<String, Value>>,
    ) -> Result<&mut Self, BlockError> {
        self.record_structural(|root| root.wrap_range(&range, wrapper_type, attrs))
    }

    /// Record [`Node::lift`].
    pub fn lift(&mut self, path: Vec<usize>) -> Result<&mut Self, BlockError> {
        self.record_structural(|root| root.lift(&path))
    }
}

/// Inline range builders: edit the inline content of the block at `block_path`
/// (recorded, like the block builders, by running the edit and recovering the
/// patch via [`diff`](crate::diff)). They wrap the [`range`](crate::range) API
/// so range edits join the recorded/invertible transaction.
impl Transform<'_> {
    fn at_block<F>(&mut self, block_path: Vec<usize>, f: F) -> Result<&mut Self, BlockError>
    where
        F: FnOnce(&mut Node) -> Result<(), BlockError>,
    {
        self.record_structural(move |root| {
            let block = root
                .node_at_mut(&block_path)
                .ok_or_else(|| BlockError::PathNotFound {
                    path: block_path.clone(),
                })?;
            f(block)
        })
    }

    /// Record [`Node::insert_text`] at `pos` in the block at `block_path`.
    pub fn insert_text_at(
        &mut self,
        block_path: Vec<usize>,
        pos: Position,
        text: impl Into<String>,
        marks: Option<Vec<Mark>>,
    ) -> Result<&mut Self, BlockError> {
        let text = text.into();
        self.at_block(block_path, move |block| {
            block.insert_text(pos, &text, marks.as_deref())?;
            Ok(())
        })
    }

    /// Record [`Node::delete_range`] over `range` in the block at `block_path`.
    pub fn delete_range_in(
        &mut self,
        block_path: Vec<usize>,
        range: Range,
    ) -> Result<&mut Self, BlockError> {
        self.at_block(block_path, move |block| {
            block.delete_range(range)?;
            Ok(())
        })
    }

    /// Record [`Node::replace_range`] over `range` in the block at `block_path`.
    pub fn replace_range_in(
        &mut self,
        block_path: Vec<usize>,
        range: Range,
        text: impl Into<String>,
        marks: Option<Vec<Mark>>,
    ) -> Result<&mut Self, BlockError> {
        let text = text.into();
        self.at_block(block_path, move |block| {
            block.replace_range(range, &text, marks.as_deref())?;
            Ok(())
        })
    }

    /// Record [`Node::add_mark_range`] over `range` in the block at `block_path`.
    pub fn add_mark_range_in(
        &mut self,
        block_path: Vec<usize>,
        range: Range,
        mark: Mark,
    ) -> Result<&mut Self, BlockError> {
        self.at_block(block_path, move |block| {
            block.add_mark_range(range, mark)?;
            Ok(())
        })
    }

    /// Record [`Node::remove_mark_range`] over `range` in the block at `block_path`.
    pub fn remove_mark_range_in(
        &mut self,
        block_path: Vec<usize>,
        range: Range,
        mark_type: impl Into<String>,
    ) -> Result<&mut Self, BlockError> {
        let mark_type = mark_type.into();
        self.at_block(block_path, move |block| {
            block.remove_mark_range(range, &mark_type)?;
            Ok(())
        })
    }
}
