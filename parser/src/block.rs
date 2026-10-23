//! Block-level structural editing: split, join, wrap, lift, retype.
//!
//! Where [`range`](crate::range) edits the inline content *within* one block,
//! these ops restructure the **block tree itself**, addressed by index-path.
//! They operate directly on the mutable tree (no clones on the direct path);
//! for a recorded, invertible patch use the matching [`Transform`](crate::Transform)
//! builders, which run the op and recover a [`Change`](crate::Change) list via
//! [`diff`](crate::diff).
//!
//! A contiguous run of sibling blocks is a [`BlockRange`].
//!
//! ```
//! use tiptap_rusty_parser::Node;
//! // doc > [paragraph "a", paragraph "b"]; wrap them in a blockquote.
//! let mut doc = Node::element("doc").with_children([
//!     Node::element("paragraph").with_text("a"),
//!     Node::element("paragraph").with_text("b"),
//! ]);
//! doc.wrap_range(
//!     &tiptap_rusty_parser::BlockRange::new(vec![], 0, 2),
//!     "blockquote",
//!     None,
//! ).unwrap();
//! assert_eq!(doc.child_count(), 1);
//! assert_eq!(doc.child(0).unwrap().node_type.as_deref(), Some("blockquote"));
//! assert_eq!(doc.child(0).unwrap().child_count(), 2);
//! ```

use crate::node::Node;
use crate::normalize::{normalize_children, NormalizeOptions};
use crate::range::{ensure_boundary, Position, RangeError};
use serde_json::{Map, Value};
use std::fmt;

/// A contiguous run of sibling blocks `[start, end)` under a common parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockRange {
    /// Index-path to the shared parent node.
    pub parent: Vec<usize>,
    /// First child index (inclusive).
    pub start: usize,
    /// One-past-last child index (exclusive).
    pub end: usize,
}

impl BlockRange {
    /// Construct a range.
    pub fn new(parent: Vec<usize>, start: usize, end: usize) -> Self {
        Self { parent, start, end }
    }

    /// The single-block range for a node `path` (`parent = path[..n-1]`, the run
    /// `[i, i+1)`). An empty path yields an empty range at the root.
    pub fn single(path: &[usize]) -> Self {
        match path.split_last() {
            Some((&i, parent)) => Self::new(parent.to_vec(), i, i + 1),
            None => Self::new(Vec::new(), 0, 0),
        }
    }

    /// Number of blocks in the run.
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Whether the run is empty.
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// Why a block-structural operation could not be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockError {
    /// No node exists at the given path.
    PathNotFound {
        /// The unresolved path.
        path: Vec<usize>,
    },
    /// A child index is out of range for the parent.
    IndexOutOfRange {
        /// The parent path.
        parent: Vec<usize>,
        /// The offending index.
        index: usize,
    },
    /// The operation needs a parent (and sometimes a grandparent) the path lacks.
    NoParent,
    /// `join_blocks` was asked to merge index 0 (no previous sibling).
    NoPreviousSibling {
        /// The path that has no previous sibling.
        path: Vec<usize>,
    },
    /// A [`BlockRange`] is inverted or extends past the parent's children.
    InvalidRange {
        /// The parent path.
        parent: Vec<usize>,
        /// Start index.
        start: usize,
        /// End index.
        end: usize,
    },
    /// A mid-text split boundary could not be resolved (forwarded from [`range`](crate::range)).
    Range(RangeError),
}

impl From<RangeError> for BlockError {
    fn from(e: RangeError) -> Self {
        BlockError::Range(e)
    }
}

impl fmt::Display for BlockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockError::PathNotFound { path } => write!(f, "block: no node at path {path:?}"),
            BlockError::IndexOutOfRange { parent, index } => {
                write!(f, "block: index {index} out of range under {parent:?}")
            }
            BlockError::NoParent => write!(f, "block: operation requires a parent node"),
            BlockError::NoPreviousSibling { path } => {
                write!(f, "block: no previous sibling to join at {path:?}")
            }
            BlockError::InvalidRange { parent, start, end } => {
                write!(f, "block: invalid range [{start},{end}) under {parent:?}")
            }
            BlockError::Range(e) => write!(f, "block: {e}"),
        }
    }
}

impl std::error::Error for BlockError {}

/// Clone a node's metadata (type/attrs/marks/extra) with fresh `content`.
fn with_content(meta: &Node, content: Vec<Node>) -> Node {
    Node {
        node_type: meta.node_type.clone(),
        attrs: meta.attrs.clone(),
        marks: meta.marks.clone(),
        extra: meta.extra.clone(),
        content: Some(content),
        text: None,
    }
}

impl Node {
    /// Change the type (and attrs) of the block at `path` **in place, keeping its
    /// content** — unlike a wholesale [`Change::Replace`](crate::Change::Replace).
    /// `attrs = None` clears attrs. The root (`&[]`) may be retyped.
    pub fn set_block_type(
        &mut self,
        path: &[usize],
        new_type: impl Into<String>,
        attrs: Option<Map<String, Value>>,
    ) -> Result<(), BlockError> {
        let node = self.at_block(path)?;
        node.node_type = Some(new_type.into());
        node.attrs = attrs;
        Ok(())
    }

    /// Split the block at `path` at child-boundary index `at` into two siblings:
    /// the left keeps children `[..at]`, a new right sibling takes `[at..]`
    /// (same type/attrs/marks/extra). `depth` also splits that many ancestors
    /// (ProseMirror-style), clamped at the root.
    pub fn split_block(
        &mut self,
        path: &[usize],
        at: usize,
        depth: usize,
    ) -> Result<(), BlockError> {
        if path.is_empty() {
            return Err(BlockError::NoParent);
        }
        let block = self.node_at(path).ok_or_else(|| BlockError::PathNotFound {
            path: path.to_vec(),
        })?;
        let len = block.children().len();
        if at > len {
            return Err(BlockError::IndexOutOfRange {
                parent: path.to_vec(),
                index: at,
            });
        }

        // Detach the right portion of the target block.
        let block_mut = self.node_at_mut(path).unwrap();
        let right_children = block_mut.children_mut().split_off(at);
        normalize_children(block_mut.children_mut(), &NormalizeOptions::default());
        let mut new_node = with_content(block_mut, right_children);
        normalize_children(
            new_node.content.as_mut().unwrap(),
            &NormalizeOptions::default(),
        );

        // Walk up `depth` levels, pulling following siblings into the new node and
        // splitting each ancestor. Stop if there's no grandparent left to host it.
        let mut cur = path.to_vec();
        for _ in 0..depth {
            if cur.len() < 2 {
                break; // parent is the root: cannot split it
            }
            let pidx = *cur.last().unwrap();
            let ppath = &cur[..cur.len() - 1];
            let parent = self.node_at_mut(ppath).unwrap();
            let following = parent.children_mut().split_off(pidx + 1);
            let mut content = Vec::with_capacity(following.len() + 1);
            content.push(new_node);
            content.extend(following);
            new_node = with_content(parent, content);
            cur.truncate(cur.len() - 1);
        }

        // Insert `new_node` as the next sibling of the block at `cur`.
        let insert_idx = *cur.last().unwrap() + 1;
        let host = self.node_at_mut(&cur[..cur.len() - 1]).unwrap();
        host.insert_child(insert_idx, new_node);
        Ok(())
    }

    /// Split the block at `path` at an inline [`Position`] (materializing a
    /// mid-text boundary first), then split like [`Node::split_block`].
    pub fn split_block_at(
        &mut self,
        path: &[usize],
        pos: Position,
        depth: usize,
    ) -> Result<(), BlockError> {
        // Validate up front so a failed precondition never leaves a half-split
        // text node behind (ensure_boundary below mutates).
        if path.is_empty() {
            return Err(BlockError::NoParent);
        }
        let at = {
            let block = self
                .node_at_mut(path)
                .ok_or_else(|| BlockError::PathNotFound {
                    path: path.to_vec(),
                })?;
            ensure_boundary(block.children_mut(), pos)?
        };
        self.split_block(path, at, depth)
    }

    /// Merge the block at `parent[index]` into its previous sibling
    /// `parent[index-1]` (appending its content, re-merging inline text at the
    /// seam). The previous sibling's type/attrs win on a mismatch.
    pub fn join_blocks(&mut self, parent: &[usize], index: usize) -> Result<(), BlockError> {
        if index == 0 {
            let mut path = parent.to_vec();
            path.push(0);
            return Err(BlockError::NoPreviousSibling { path });
        }
        let parent_node = self
            .node_at_mut(parent)
            .ok_or_else(|| BlockError::PathNotFound {
                path: parent.to_vec(),
            })?;
        let children = match parent_node.content.as_mut() {
            Some(c) if index < c.len() => c,
            _ => {
                return Err(BlockError::IndexOutOfRange {
                    parent: parent.to_vec(),
                    index,
                })
            }
        };
        let right = children.remove(index);
        if let Some(rc) = right.content {
            if !rc.is_empty() {
                let left = &mut children[index - 1];
                left.children_mut().extend(rc);
                normalize_children(left.children_mut(), &NormalizeOptions::default());
            }
        }
        Ok(())
    }

    /// Wrap the single block at `path` in a new parent of `wrapper_type`.
    pub fn wrap(
        &mut self,
        path: &[usize],
        wrapper_type: impl Into<String>,
        attrs: Option<Map<String, Value>>,
    ) -> Result<(), BlockError> {
        self.wrap_range(&BlockRange::single(path), wrapper_type, attrs)
    }

    /// Wrap a contiguous run of sibling blocks (one level) in a new parent of
    /// `wrapper_type`. Nested structures (e.g. `bulletList > listItem`) are
    /// composed by the caller.
    pub fn wrap_range(
        &mut self,
        range: &BlockRange,
        wrapper_type: impl Into<String>,
        attrs: Option<Map<String, Value>>,
    ) -> Result<(), BlockError> {
        let parent = self
            .node_at_mut(&range.parent)
            .ok_or_else(|| BlockError::PathNotFound {
                path: range.parent.clone(),
            })?;
        let children = parent.children_mut();
        if range.start > range.end || range.end > children.len() {
            return Err(BlockError::InvalidRange {
                parent: range.parent.clone(),
                start: range.start,
                end: range.end,
            });
        }
        let run: Vec<Node> = children
            .splice(range.start..range.end, std::iter::empty())
            .collect();
        let wrapper = Node {
            node_type: Some(wrapper_type.into()),
            attrs,
            content: Some(run),
            ..Node::default()
        };
        children.insert(range.start, wrapper);
        Ok(())
    }

    /// Lift the block at `path` out of its parent into its grandparent,
    /// splitting the parent around it so preceding/following siblings are kept
    /// (a sole-child parent simply collapses to the lifted node).
    pub fn lift(&mut self, path: &[usize]) -> Result<(), BlockError> {
        let n = path.len();
        if n < 2 {
            return Err(BlockError::NoParent);
        }
        self.node_at(path).ok_or_else(|| BlockError::PathNotFound {
            path: path.to_vec(),
        })?;
        let child_idx = path[n - 1];
        let parent_idx = path[n - 2];

        // Mutate the parent: split its children around `child_idx`.
        let parent = self.node_at_mut(&path[..n - 1]).unwrap();
        let mut tail = parent.children_mut().split_off(child_idx);
        let moved = tail.remove(0);
        let right_children = tail;
        let left_children = std::mem::take(parent.content.as_mut().unwrap());

        // Rebuild [left?, moved, right?] from the parent's metadata.
        let mut insert: Vec<Node> = Vec::with_capacity(3);
        if !left_children.is_empty() {
            insert.push(with_content(parent, left_children));
        }
        insert.push(moved);
        if !right_children.is_empty() {
            insert.push(with_content(parent, right_children));
        }

        // Replace the (now-gutted) parent in the grandparent with that run.
        let gp = self.node_at_mut(&path[..n - 2]).unwrap();
        gp.children_mut().splice(parent_idx..parent_idx + 1, insert);
        Ok(())
    }

    /// Resolve a block path mutably or return [`BlockError::PathNotFound`].
    fn at_block(&mut self, path: &[usize]) -> Result<&mut Node, BlockError> {
        self.node_at_mut(path)
            .ok_or_else(|| BlockError::PathNotFound {
                path: path.to_vec(),
            })
    }
}
