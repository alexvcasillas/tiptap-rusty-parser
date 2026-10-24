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
//! The undo is recorded too: each change captures the pre-change state it
//! overwrites (old attr/text/marks value, the removed or replaced subtree, …)
//! as it applies, so [`finish_with_inverse`](Transform::finish_with_inverse)
//! returns the inverse without cloning or re-diffing the tree. Cost is
//! proportional to the edit, not the document.
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
//!
//! // Same undo, recorded incrementally (no clone of `original` needed).
//! let mut doc2 = original.clone();
//! let (_, undo2) = {
//!     let mut tx = doc2.transform();
//!     tx.set_attr(vec![0], "level", 1).unwrap();
//!     tx.insert(vec![], 1, Node::element("paragraph")).unwrap();
//!     tx.finish_with_inverse()
//! };
//! apply(&mut doc2, &undo2).unwrap();
//! assert_eq!(doc2, original);
//! ```

use crate::block::{BlockError, BlockRange};
use crate::diff::{apply, diff_at, ApplyError, Change};
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
    /// Inverse of each recorded step, in application order (undo = reversed).
    undo: Vec<Change>,
}

impl Node {
    /// Begin a [`Transform`] over this tree.
    pub fn transform(&mut self) -> Transform<'_> {
        Transform {
            root: self,
            changes: Vec::new(),
            undo: Vec::new(),
        }
    }

    /// Apply `changes` in order, consuming them, and return their inverse
    /// (applied to the result it restores `self`). The inverse is captured
    /// while applying, from the state each change overwrites: no tree clone,
    /// no diff, and moved-in nodes are not cloned. Atomic: on error the
    /// already-applied prefix is rolled back and the tree is unchanged.
    ///
    /// ```
    /// use tiptap_rusty_parser::{Change, Node};
    /// let mut doc = Node::element("doc").with_child(Node::element("paragraph"));
    /// let original = doc.clone();
    /// let undo = doc
    ///     .apply_inverting(vec![Change::Remove { path: vec![], index: 0 }])
    ///     .unwrap();
    /// assert_eq!(doc.child_count(), 0);
    /// let redo = doc.apply_inverting(undo).unwrap();
    /// assert_eq!(doc, original);
    /// assert_eq!(redo, vec![Change::Remove { path: vec![], index: 0 }]);
    /// ```
    pub fn apply_inverting(&mut self, changes: Vec<Change>) -> Result<Vec<Change>, ApplyError> {
        let mut undo = Vec::with_capacity(changes.len());
        for change in changes {
            match apply_inverting_one(self, change) {
                Ok(inv) => undo.extend(inv),
                Err(e) => {
                    rollback(self, undo);
                    return Err(e);
                }
            }
        }
        undo.reverse();
        Ok(undo)
    }
}

/// Undo `undo` (inverse steps in application order) on `root`, newest first.
fn rollback(root: &mut Node, undo: Vec<Change>) {
    for change in undo.into_iter().rev() {
        // Inverses of applied steps always resolve.
        let _ = apply_inverting_one(root, change);
    }
}

fn at_mut<'n>(root: &'n mut Node, path: &[usize]) -> Result<&'n mut Node, ApplyError> {
    root.node_at_mut(path).ok_or_else(|| ApplyError {
        path: path.to_vec(),
    })
}

fn child_err(path: &[usize], index: usize) -> ApplyError {
    let mut p = path.to_vec();
    p.push(index);
    ApplyError { path: p }
}

/// Apply one change in place and return the change that undoes it (`None`
/// for a no-op). Same semantics and errors as [`apply`]; on error the tree is
/// untouched.
///
/// Container shapes round-trip exactly: where the inverse op would normalize a
/// present-but-empty `attrs: {}` / `content: []` to absent, the pre-change node
/// is restored with a [`Change::Replace`] instead (a clone of that one node,
/// only in that rare shape).
fn apply_inverting_one(root: &mut Node, change: Change) -> Result<Option<Change>, ApplyError> {
    let empty_attrs = |n: &Node| matches!(&n.attrs, Some(a) if a.is_empty());
    Ok(match change {
        Change::SetAttr { path, key, value } => {
            let node = at_mut(root, &path)?;
            if empty_attrs(node) {
                let before = node.clone();
                node.set_attr(key, value);
                Some(Change::Replace { path, node: before })
            } else {
                Some(match node.set_attr(key.clone(), value) {
                    Some(prev) => Change::SetAttr {
                        path,
                        key,
                        value: prev,
                    },
                    None => Change::RemoveAttr { path, key },
                })
            }
        }
        Change::RemoveAttr { path, key } => {
            let node = at_mut(root, &path)?;
            if empty_attrs(node) {
                // `remove_attr` normalizes `{}` to absent.
                let before = node.clone();
                node.remove_attr(&key);
                Some(Change::Replace { path, node: before })
            } else {
                node.remove_attr(&key)
                    .map(|value| Change::SetAttr { path, key, value })
            }
        }
        Change::SetText { path, text } => {
            let node = at_mut(root, &path)?;
            let prev = std::mem::replace(&mut node.text, text);
            Some(Change::SetText { path, text: prev })
        }
        Change::SpliceText {
            path,
            from,
            len_del,
            insert,
        } => {
            let prev = at_mut(root, &path)?.text.clone();
            let splice = Change::SpliceText {
                path,
                from,
                len_del,
                insert,
            };
            apply(root, std::slice::from_ref(&splice))?;
            let Change::SpliceText { path, insert, .. } = splice else {
                unreachable!()
            };
            match prev {
                // A no-op splice on absent text keeps it absent.
                None if root.node_at(&path).is_some_and(|n| n.text.is_none()) => None,
                None => Some(Change::SetText { path, text: None }),
                Some(t) => Some(Change::SpliceText {
                    path,
                    from,
                    len_del: insert.chars().count(),
                    insert: t.chars().skip(from).take(len_del).collect(),
                }),
            }
        }
        Change::SetMarks { path, marks } => {
            let node = at_mut(root, &path)?;
            let prev = std::mem::replace(&mut node.marks, marks);
            Some(Change::SetMarks { path, marks: prev })
        }
        Change::SetExtra { path, key, value } => {
            let node = at_mut(root, &path)?;
            Some(match node.extra_mut().insert(key.clone(), value) {
                Some(prev) => Change::SetExtra {
                    path,
                    key,
                    value: prev,
                },
                None => Change::RemoveExtra { path, key },
            })
        }
        Change::RemoveExtra { path, key } => {
            let node = at_mut(root, &path)?;
            node.remove_extra(&key)
                .map(|value| Change::SetExtra { path, key, value })
        }
        Change::Insert { path, index, node } => {
            let parent = at_mut(root, &path)?;
            if index > parent.child_count() {
                return Err(child_err(&path, index));
            }
            if matches!(&parent.content, Some(c) if c.is_empty()) {
                // `Remove` would normalize `[]` to absent; the parent is
                // childless, so this clone is shallow.
                let before = parent.clone();
                parent.insert_child(index, node);
                Some(Change::Replace { path, node: before })
            } else {
                parent.insert_child(index, node);
                Some(Change::Remove { path, index })
            }
        }
        Change::Remove { path, index } => {
            let parent = at_mut(root, &path)?;
            let node = parent
                .remove_child(index)
                .ok_or_else(|| child_err(&path, index))?;
            Some(Change::Insert { path, index, node })
        }
        Change::Replace { path, node } => {
            let old = match path.split_last() {
                None => std::mem::replace(root, node),
                Some((&last, parent)) => at_mut(root, parent)?
                    .replace_child(last, node)
                    .ok_or_else(|| ApplyError { path: path.clone() })?,
            };
            Some(Change::Replace { path, node: old })
        }
        Change::Move { path, from, to } => {
            let mv = Change::Move { path, from, to };
            apply(root, std::slice::from_ref(&mv))?;
            let Change::Move { path, .. } = mv else {
                unreachable!()
            };
            Some(Change::Move {
                path,
                from: to,
                to: from,
            })
        }
    })
}

impl<'a> Transform<'a> {
    /// Apply `change` in place, record it and its inverse.
    fn push(&mut self, change: Change) -> Result<&mut Self, ApplyError> {
        let inv = apply_inverting_one(self.root, change.clone())?;
        self.changes.push(change);
        self.undo.extend(inv);
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

    /// Apply an already-built [`Change`] (e.g. one picked from a
    /// [`diff`](crate::diff) list) in place and record it. Lets callers
    /// cherry-pick changes from a diff into an invertible transaction.
    pub fn apply_change(&mut self, change: Change) -> Result<&mut Self, ApplyError> {
        self.push(change)
    }

    /// The changes recorded so far, without consuming the transaction.
    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    /// Finish the transaction, returning the recorded [`Change`] log.
    pub fn finish(self) -> Vec<Change> {
        self.changes
    }

    /// The undo of everything recorded so far: applied to the current tree it
    /// restores the tree as it was when the transaction began. Equivalent to
    /// [`invert`](crate::invert) against the original, without needing it.
    pub fn inverse(&self) -> Vec<Change> {
        self.undo.iter().rev().cloned().collect()
    }

    /// Finish the transaction, returning `(changes, inverse)`. See
    /// [`inverse`](Transform::inverse).
    pub fn finish_with_inverse(self) -> (Vec<Change>, Vec<Change>) {
        let mut undo = self.undo;
        undo.reverse();
        (self.changes, undo)
    }

    /// Abandon the transaction: undo every recorded change, restoring the
    /// tree as it was when the transaction began (e.g. after a builder error).
    pub fn rollback(self) {
        rollback(self.root, self.undo);
    }
}

/// Block-structural builders. Unlike the field/child ops above (which map to a
/// single [`Change`]), these restructure the tree and are recorded by running
/// the in-place edit and recovering the patch via [`diff`](crate::diff). Only
/// the smallest subtree the op can touch (its *scope*: the block, its parent or
/// grandparent) is cloned and diffed, so the cost is proportional to that
/// subtree, not the document; the direct [`Node`] methods stay clone-free.
/// The inverse is a [`Change::Replace`] of the scope with its pre-op clone.
impl Transform<'_> {
    /// Run `f` on the root, recording the patch of the subtree at `scope`
    /// (which must contain every node `f` changes and keep its own path).
    /// On error the scope is restored, so a failed op leaves no trace.
    fn record_structural<F>(&mut self, scope: &[usize], f: F) -> Result<&mut Self, BlockError>
    where
        F: FnOnce(&mut Node) -> Result<(), BlockError>,
    {
        // An unresolvable scope means `f` will fail its own path check; fall
        // back to the root so the error comes from `f`.
        let scope = if self.root.node_at(scope).is_some() {
            scope
        } else {
            &[]
        };
        let before = self.root.node_at(scope).cloned().unwrap_or_default();
        if let Err(e) = f(self.root) {
            if let Some(n) = self.root.node_at_mut(scope) {
                *n = before;
            }
            return Err(e);
        }
        let after = self
            .root
            .node_at(scope)
            .ok_or_else(|| BlockError::PathNotFound {
                path: scope.to_vec(),
            })?;
        let patch = diff_at(&before, after, scope);
        if !patch.is_empty() {
            self.changes.extend(patch);
            self.undo.push(Change::Replace {
                path: scope.to_vec(),
                node: before,
            });
        }
        Ok(self)
    }

    /// Record [`Node::set_block_type`].
    pub fn set_block_type(
        &mut self,
        path: Vec<usize>,
        new_type: impl Into<String>,
        attrs: Option<Map<String, Value>>,
    ) -> Result<&mut Self, BlockError> {
        self.record_structural(&path.clone(), |root| {
            root.set_block_type(&path, new_type, attrs)
        })
    }

    /// Record [`Node::split_block`].
    pub fn split_block(
        &mut self,
        path: Vec<usize>,
        at: usize,
        depth: usize,
    ) -> Result<&mut Self, BlockError> {
        let scope = split_scope(&path, depth).to_vec();
        self.record_structural(&scope, |root| root.split_block(&path, at, depth))
    }

    /// Record [`Node::split_block_at`].
    pub fn split_block_at(
        &mut self,
        path: Vec<usize>,
        pos: Position,
        depth: usize,
    ) -> Result<&mut Self, BlockError> {
        let scope = split_scope(&path, depth).to_vec();
        self.record_structural(&scope, |root| root.split_block_at(&path, pos, depth))
    }

    /// Record [`Node::join_blocks`].
    pub fn join_blocks(
        &mut self,
        parent: Vec<usize>,
        index: usize,
    ) -> Result<&mut Self, BlockError> {
        self.record_structural(&parent.clone(), |root| root.join_blocks(&parent, index))
    }

    /// Record [`Node::wrap`].
    pub fn wrap(
        &mut self,
        path: Vec<usize>,
        wrapper_type: impl Into<String>,
        attrs: Option<Map<String, Value>>,
    ) -> Result<&mut Self, BlockError> {
        // The wrapper takes the block's place: same path, same parent count.
        self.record_structural(&path.clone(), |root| root.wrap(&path, wrapper_type, attrs))
    }

    /// Record [`Node::wrap_range`].
    pub fn wrap_range(
        &mut self,
        range: BlockRange,
        wrapper_type: impl Into<String>,
        attrs: Option<Map<String, Value>>,
    ) -> Result<&mut Self, BlockError> {
        self.record_structural(&range.parent.clone(), |root| {
            root.wrap_range(&range, wrapper_type, attrs)
        })
    }

    /// Record [`Node::lift`].
    pub fn lift(&mut self, path: Vec<usize>) -> Result<&mut Self, BlockError> {
        // Splits the parent inside the grandparent.
        let scope = path[..path.len().saturating_sub(2)].to_vec();
        self.record_structural(&scope, |root| root.lift(&path))
    }
}

/// Host of the new sibling [`Node::split_block`] inserts: the parent of the
/// topmost split block (`depth` ancestors up, clamped below the root).
fn split_scope(path: &[usize], depth: usize) -> &[usize] {
    let k = depth.min(path.len().saturating_sub(1));
    &path[..path.len().saturating_sub(1 + k)]
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
        self.record_structural(&block_path.clone(), move |root| {
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
