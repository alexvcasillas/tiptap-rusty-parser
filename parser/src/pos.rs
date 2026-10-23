//! Flat ProseMirror integer positions over a [`Node`] tree.
//!
//! Tiptap/ProseMirror addresses every location in a document with a single
//! integer ("position"). This module implements that scheme on top of the
//! crate's index-path model so the two interoperate.
//!
//! ## Size rules (ProseMirror `nodeSize`)
//! - a **text** node has size = its Unicode-scalar length;
//! - a **leaf** node has size `1`;
//! - any other node has size `2 + content_size` (an open and a close token);
//! - the root/`doc` node is **not** wrapped in tokens: valid positions run
//!   `0..=content_size(root)`, with `0` just inside the root before its first child.
//!
//! Whether a node is a *leaf* (size 1) or an *empty container* (size 2, e.g. an
//! empty paragraph) **cannot be derived from JSON** — it's a schema property. A
//! [`LeafPolicy`] decides it: the default treats a small built-in set of Tiptap
//! atoms (`image`, `horizontalRule`, `hardBreak`) as leaves and everything else
//! as a container. Override it with an explicit type set when your schema differs.
//!
//! ```
//! use tiptap_rusty_parser::Node;
//! // doc > [ paragraph("hi"), horizontalRule, paragraph("ok") ]
//! let doc = Node::element("doc").with_children([
//!     Node::element("paragraph").with_text("hi"),
//!     Node::element("horizontalRule"),
//!     Node::element("paragraph").with_text("ok"),
//! ]);
//! assert_eq!(doc.pos_before(&[1]).unwrap(), 4);   // the rule sits at pos 4
//! assert_eq!(doc.pos_in_text(&[0, 0], 1).unwrap(), 2); // after "h" in "hi"
//! let r = doc.resolve(2).unwrap();
//! assert_eq!(r.path, vec![0]);                    // inside the first paragraph
//! assert_eq!(r.text_offset.unwrap().offset, 1);   // 1 scalar into "hi"
//! ```

use crate::node::Node;
use crate::range::Position;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;

/// Decides which nodes are ProseMirror **leaves** (size 1) vs empty containers
/// (size 2). Leafness isn't recoverable from JSON, so it's configured here.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LeafPolicy {
    /// Built-in Tiptap atoms: `image`, `horizontalRule`, `hardBreak`.
    #[default]
    Builtin,
    /// An explicit set of node-type names to treat as leaves.
    Types(HashSet<String>),
}

impl LeafPolicy {
    /// Build a policy from an explicit list of leaf type names.
    pub fn from_types<I, S>(types: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        LeafPolicy::Types(types.into_iter().map(Into::into).collect())
    }

    fn is_leaf_type(&self, ty: &str) -> bool {
        match self {
            LeafPolicy::Builtin => matches!(ty, "image" | "horizontalRule" | "hardBreak"),
            LeafPolicy::Types(set) => set.contains(ty),
        }
    }
}

/// A flat position resolved against a [`Node`] tree. All fields are owned
/// indices (no borrows), so it serializes and crosses FFI cleanly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedPos {
    /// The original flat position.
    pub pos: usize,
    /// Depth of the containing (parent) node; equals `path.len()`.
    pub depth: usize,
    /// Index-path from the root to the node containing this position.
    pub path: Vec<usize>,
    /// Offset of the position within the parent's content (flat units).
    pub parent_offset: usize,
    /// Index of the child at or immediately after the boundary.
    pub index: usize,
    /// Set when the position lies strictly inside a text node.
    pub text_offset: Option<TextPoint>,
}

impl ResolvedPos {
    /// Whether the position lies strictly inside a text node.
    pub fn is_in_text(&self) -> bool {
        self.text_offset.is_some()
    }

    /// The parent (containing) node, re-queried against `root`.
    pub fn parent<'a>(&self, root: &'a Node) -> Option<&'a Node> {
        root.node_at(&self.path)
    }
}

/// The text node and scalar offset a position falls inside.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextPoint {
    /// Index-path to the text node.
    pub path: Vec<usize>,
    /// Unicode-scalar offset within that text node.
    pub offset: usize,
}

/// A flat ProseMirror range `[from, to]` over a whole document. (Distinct from
/// the block-scoped [`Range`](crate::Range) used by inline range editing.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PosRange {
    /// Start position (inclusive).
    pub from: usize,
    /// End position (exclusive).
    pub to: usize,
}

impl PosRange {
    /// Construct a range (`from <= to` expected).
    pub fn new(from: usize, to: usize) -> Self {
        Self { from, to }
    }

    /// A collapsed (empty) range at `at`.
    pub fn collapsed(at: usize) -> Self {
        Self { from: at, to: at }
    }

    /// Whether the range is empty.
    pub fn is_empty(&self) -> bool {
        self.to <= self.from
    }
}

/// Why a flat-position operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PosError {
    /// `pos` is past the end of the document.
    OutOfRange {
        /// The offending position.
        pos: usize,
        /// The document's content size (max valid position).
        size: usize,
    },
    /// An index-path didn't resolve to a node.
    PathNotFound {
        /// The unresolved path.
        path: Vec<usize>,
    },
    /// A child index in a path is out of range for its parent.
    OffsetOutOfRange {
        /// The path being resolved.
        path: Vec<usize>,
        /// The offending child index.
        offset: usize,
    },
}

impl fmt::Display for PosError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PosError::OutOfRange { pos, size } => {
                write!(f, "pos: {pos} out of range (document size {size})")
            }
            PosError::PathNotFound { path } => write!(f, "pos: no node at path {path:?}"),
            PosError::OffsetOutOfRange { path, offset } => {
                write!(f, "pos: child index {offset} out of range at {path:?}")
            }
        }
    }
}

impl std::error::Error for PosError {}

impl Node {
    /// ProseMirror node size with the default [`LeafPolicy`].
    pub fn node_size(&self) -> usize {
        self.node_size_with(&LeafPolicy::Builtin)
    }

    /// ProseMirror node size under `policy`.
    pub fn node_size_with(&self, policy: &LeafPolicy) -> usize {
        if let Some(t) = &self.text {
            return t.chars().count();
        }
        if self.is_leaf_with(policy) {
            return 1;
        }
        2 + self.content_size_with(policy)
    }

    /// Sum of child sizes (the count of inner positions) with the default policy.
    pub fn content_size(&self) -> usize {
        self.content_size_with(&LeafPolicy::Builtin)
    }

    /// Sum of child sizes under `policy`.
    pub fn content_size_with(&self, policy: &LeafPolicy) -> usize {
        self.children()
            .iter()
            .map(|c| c.node_size_with(policy))
            .sum()
    }

    /// Whether this node is a ProseMirror leaf under the default policy.
    pub fn is_leaf(&self) -> bool {
        self.is_leaf_with(&LeafPolicy::Builtin)
    }

    /// Whether this node is a ProseMirror leaf under `policy` (non-text, and its
    /// type is in the leaf set).
    pub fn is_leaf_with(&self, policy: &LeafPolicy) -> bool {
        self.text.is_none()
            && self
                .node_type
                .as_deref()
                .is_some_and(|t| policy.is_leaf_type(t))
    }

    /// Resolve a flat position with the default [`LeafPolicy`].
    pub fn resolve(&self, pos: usize) -> Result<ResolvedPos, PosError> {
        self.resolve_with(pos, &LeafPolicy::Builtin)
    }

    /// Resolve a flat position into a [`ResolvedPos`] under `policy`.
    pub fn resolve_with(&self, pos: usize, policy: &LeafPolicy) -> Result<ResolvedPos, PosError> {
        let total = self.content_size_with(policy);
        if pos > total {
            return Err(PosError::OutOfRange { pos, size: total });
        }
        let mut path: Vec<usize> = Vec::new();
        let mut offset = pos;
        'walk: loop {
            let node = self.node_at(&path).expect("resolved path stays valid");
            let children = node.children();
            let mut i = 0usize;
            let mut rem = offset;
            loop {
                if i == children.len() || rem == 0 {
                    return Ok(ResolvedPos {
                        pos,
                        depth: path.len(),
                        path,
                        parent_offset: offset,
                        index: i,
                        text_offset: None,
                    });
                }
                let child = &children[i];
                let cs = child.node_size_with(policy);
                if rem < cs {
                    if child.text.is_some() {
                        let mut tpath = path.clone();
                        tpath.push(i);
                        return Ok(ResolvedPos {
                            pos,
                            depth: path.len(),
                            path,
                            parent_offset: offset,
                            index: i,
                            text_offset: Some(TextPoint {
                                path: tpath,
                                offset: rem,
                            }),
                        });
                    }
                    // Descend into a non-text container, crossing its open token.
                    path.push(i);
                    offset = rem - 1;
                    continue 'walk;
                }
                rem -= cs;
                i += 1;
            }
        }
    }

    /// Flat position of the boundary just *before* the node at `path` (default policy).
    pub fn pos_before(&self, path: &[usize]) -> Result<usize, PosError> {
        self.pos_before_with(path, &LeafPolicy::Builtin)
    }

    /// Flat position of the boundary just *before* the node at `path` under `policy`.
    pub fn pos_before_with(&self, path: &[usize], policy: &LeafPolicy) -> Result<usize, PosError> {
        let mut acc = 0usize;
        for depth in 0..path.len() {
            let parent = self
                .node_at(&path[..depth])
                .ok_or_else(|| PosError::PathNotFound {
                    path: path.to_vec(),
                })?;
            let children = parent.children();
            let i = path[depth];
            // Every index must address a real child (no node exists at
            // `children.len()`, nor under a leaf/text node).
            if i >= children.len() {
                return Err(PosError::OffsetOutOfRange {
                    path: path.to_vec(),
                    offset: i,
                });
            }
            for child in &children[..i] {
                acc += child.node_size_with(policy);
            }
            // Entering a container child (every level except the last) crosses
            // its open token.
            if depth + 1 < path.len() {
                acc += 1;
            }
        }
        Ok(acc)
    }

    /// Flat position just *after* the node at `path` (default policy).
    pub fn pos_after(&self, path: &[usize]) -> Result<usize, PosError> {
        self.pos_after_with(path, &LeafPolicy::Builtin)
    }

    /// Flat position just *after* the node at `path` under `policy`.
    pub fn pos_after_with(&self, path: &[usize], policy: &LeafPolicy) -> Result<usize, PosError> {
        let before = self.pos_before_with(path, policy)?;
        let node = self.node_at(path).ok_or_else(|| PosError::PathNotFound {
            path: path.to_vec(),
        })?;
        Ok(before + node.node_size_with(policy))
    }

    /// Flat position at scalar `offset` inside the text node at `text_path`.
    /// Errors if `text_path` is not a text node or `offset` exceeds its length.
    pub fn pos_in_text(&self, text_path: &[usize], offset: usize) -> Result<usize, PosError> {
        let node = self
            .node_at(text_path)
            .ok_or_else(|| PosError::PathNotFound {
                path: text_path.to_vec(),
            })?;
        let len = match &node.text {
            Some(t) => t.chars().count(),
            None => {
                return Err(PosError::OffsetOutOfRange {
                    path: text_path.to_vec(),
                    offset,
                })
            }
        };
        if offset > len {
            return Err(PosError::OffsetOutOfRange {
                path: text_path.to_vec(),
                offset,
            });
        }
        Ok(self.pos_before(text_path)? + offset)
    }

    /// Map a flat position to the `(block path, inline Position)` pair used by
    /// the inline range-editing API ([`Node::insert_text`] etc.), with the
    /// default policy. The block path is the resolved container.
    pub fn pos_to_inline(&self, pos: usize) -> Result<(Vec<usize>, Position), PosError> {
        let r = self.resolve(pos)?;
        let inline = match &r.text_offset {
            Some(tp) => Position::new(r.index, tp.offset),
            None => Position::new(r.index, 0),
        };
        Ok((r.path, inline))
    }

    /// Inverse of [`Node::pos_to_inline`]: the flat position for a block-local
    /// inline [`Position`] (default policy).
    pub fn inline_to_pos(&self, block_path: &[usize], inline: Position) -> Result<usize, PosError> {
        let policy = LeafPolicy::Builtin;
        // Just inside the block (past its open token).
        let mut acc = self.pos_before_with(block_path, &policy)? + 1;
        let block = self
            .node_at(block_path)
            .ok_or_else(|| PosError::PathNotFound {
                path: block_path.to_vec(),
            })?;
        let children = block.children();
        if inline.child > children.len() {
            return Err(PosError::OffsetOutOfRange {
                path: block_path.to_vec(),
                offset: inline.child,
            });
        }
        // Validate the inline offset against the target child (mirrors the
        // inline range API: text → within length; non-text/end → offset 0 only).
        let offset_ok = match children.get(inline.child) {
            Some(child) => match &child.text {
                Some(t) => inline.offset <= t.chars().count(),
                None => inline.offset == 0,
            },
            None => inline.offset == 0, // child == len (end boundary)
        };
        if !offset_ok {
            return Err(PosError::OffsetOutOfRange {
                path: block_path.to_vec(),
                offset: inline.offset,
            });
        }
        for child in &children[..inline.child] {
            acc += child.node_size_with(&policy);
        }
        Ok(acc + inline.offset)
    }
}
