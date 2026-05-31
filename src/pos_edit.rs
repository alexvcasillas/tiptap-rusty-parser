//! Position-addressed editing: apply a batch of flat-position [`PosEdit`]s to a
//! tree and recover an invertible [`Change`] patch.
//!
//! Where [`range`](crate::range) edits one block by inline `Position` and
//! [`block`](crate::block) restructures by index-path, this module addresses
//! edits by **flat ProseMirror positions** (`from`/`to` integers) — the scheme
//! the Tiptap AI Toolkit's `tiptapEdit` operations array uses. Each [`PosEdit`]
//! is resolved (via [`Node::resolve`] / [`Node::pos_to_inline`]) and executed,
//! and the whole batch is recovered as a [`Change`] list — so it replays and
//! [`invert`](crate::invert)s like any other patch.
//!
//! ```
//! use tiptap_rusty_parser::{Node, PosContent, PosEdit};
//!
//! // doc > paragraph("hello world")
//! let mut doc = Node::element("doc")
//!     .with_child(Node::element("paragraph").with_child(Node::text("hello world")));
//! let original = doc.clone();
//!
//! // Replace "world" (scalars 7..12 in flat coords: 1 open token + offset 6..11).
//! let patch = doc
//!     .apply_pos_edits(&[PosEdit::Replace {
//!         from: 7,
//!         to: 12,
//!         content: PosContent::Text { text: "there".into(), marks: None },
//!     }])
//!     .unwrap();
//! assert_eq!(doc.text_content(), "hello there");
//!
//! // The returned patch inverts to an undo that restores the original.
//! let undo = original.invert(&patch).unwrap();
//! let mut back = doc.clone();
//! back.apply(&undo).unwrap();
//! assert_eq!(back, original);
//! ```
//!
//! ## v1 scope
//! - **Same-block** edits work at any nesting depth (the block is resolved from
//!   the position, e.g. `doc>list>item>paragraph`).
//! - **Cross-block** delete/replace/mark spans are supported when the two
//!   endpoints sit in **sibling blocks under a common parent** (the common
//!   `doc>paragraph` case): the tail of the first block, any whole blocks
//!   between, and the head of the last block are removed, then the remainder is
//!   joined (ProseMirror `deleteRange` semantics).
//! - Spans whose endpoints are at **different depths or under different parents**
//!   return [`PosEditError::UnsupportedSpan`] (error-first; full cross-structure
//!   fitting is out of scope for v1).
//! - A batch must be **disjoint**; edits apply highest-position-first so the
//!   un-rebased positions stay valid. Overlapping spans return
//!   [`PosEditError::OverlappingEdits`] (arbitrary-order rebasing needs a
//!   position map — a later addition).

use crate::block::BlockError;
use crate::diff::{ApplyError, Change};
use crate::node::{Mark, Node};
use crate::normalize::{normalize_children, NormalizeOptions};
use crate::pos::PosError;
use crate::range::{ensure_boundary, resolve_range, Position, Range, RangeError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fmt;

/// Content carried by an [`PosEdit::Insert`] / [`PosEdit::Replace`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PosContent {
    /// Text (optionally marked) inserted into a block's inline content.
    Text {
        /// The text to insert.
        text: String,
        /// Marks to carry on the inserted text (`None` = unmarked).
        #[serde(skip_serializing_if = "Option::is_none", default)]
        marks: Option<Vec<Mark>>,
    },
    /// A run of nodes inserted at the resolved boundary.
    Nodes {
        /// The nodes to insert, in order.
        nodes: Vec<Node>,
    },
}

/// A single position-addressed edit. Offsets are **flat ProseMirror positions**.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PosEdit {
    /// Insert `content` at `pos`.
    Insert {
        /// Flat position to insert at.
        pos: usize,
        /// What to insert.
        content: PosContent,
    },
    /// Delete the flat range `[from, to)`.
    Delete {
        /// Start position (inclusive).
        from: usize,
        /// End position (exclusive).
        to: usize,
    },
    /// Replace the flat range `[from, to)` with `content`.
    Replace {
        /// Start position (inclusive).
        from: usize,
        /// End position (exclusive).
        to: usize,
        /// Replacement content.
        content: PosContent,
    },
    /// Add `mark` to text in the flat range `[from, to)`.
    AddMark {
        /// Start position (inclusive).
        from: usize,
        /// End position (exclusive).
        to: usize,
        /// The mark to add.
        mark: Mark,
    },
    /// Remove every mark of `mark_type` from text in `[from, to)`.
    RemoveMark {
        /// Start position (inclusive).
        from: usize,
        /// End position (exclusive).
        to: usize,
        /// The mark type to remove.
        mark_type: String,
    },
    /// Replace the whole attribute map of the block at (or containing) `pos`.
    SetBlockAttrs {
        /// A flat position before or inside the target block.
        pos: usize,
        /// The new attribute map (empty clears all attrs).
        attrs: Map<String, Value>,
    },
}

impl PosEdit {
    /// The `[lo, hi)` flat span this edit occupies (point edits have `lo == hi`).
    fn span(&self) -> (usize, usize) {
        match self {
            PosEdit::Insert { pos, .. } | PosEdit::SetBlockAttrs { pos, .. } => (*pos, *pos),
            PosEdit::Delete { from, to }
            | PosEdit::Replace { from, to, .. }
            | PosEdit::AddMark { from, to, .. }
            | PosEdit::RemoveMark { from, to, .. } => (*from, *to),
        }
    }
}

/// Why a [`Node::apply_pos_edits`] batch could not be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PosEditError {
    /// A flat position failed to resolve.
    Pos(PosError),
    /// An inline range op failed.
    Range(RangeError),
    /// A block-structural op failed.
    Block(BlockError),
    /// A recorded change failed to apply.
    Apply(ApplyError),
    /// A cross-block span whose endpoints aren't sibling blocks under a common
    /// parent (different depths/parents) — unsupported in v1.
    UnsupportedSpan {
        /// Start position.
        from: usize,
        /// End position.
        to: usize,
    },
    /// Two edits in the batch overlap; v1 requires disjoint spans.
    OverlappingEdits {
        /// Start of the overlapping edit.
        from: usize,
        /// End of the overlapping edit.
        to: usize,
    },
}

impl From<PosError> for PosEditError {
    fn from(e: PosError) -> Self {
        PosEditError::Pos(e)
    }
}
impl From<RangeError> for PosEditError {
    fn from(e: RangeError) -> Self {
        PosEditError::Range(e)
    }
}
impl From<BlockError> for PosEditError {
    fn from(e: BlockError) -> Self {
        PosEditError::Block(e)
    }
}
impl From<ApplyError> for PosEditError {
    fn from(e: ApplyError) -> Self {
        PosEditError::Apply(e)
    }
}

impl fmt::Display for PosEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PosEditError::Pos(e) => write!(f, "pos-edit: {e}"),
            PosEditError::Range(e) => write!(f, "pos-edit: {e}"),
            PosEditError::Block(e) => write!(f, "pos-edit: {e}"),
            PosEditError::Apply(e) => write!(f, "pos-edit: {e}"),
            PosEditError::UnsupportedSpan { from, to } => {
                write!(f, "pos-edit: unsupported cross-block span [{from},{to})")
            }
            PosEditError::OverlappingEdits { from, to } => {
                write!(f, "pos-edit: overlapping edit at [{from},{to})")
            }
        }
    }
}

impl std::error::Error for PosEditError {}

impl Node {
    /// Apply a batch of position-addressed [`PosEdit`]s and return the recovered,
    /// invertible [`Change`] patch (relative to `self` before the call).
    ///
    /// Edits are applied **highest-position-first** so their un-rebased flat
    /// positions stay valid; the batch must be **disjoint** (overlapping spans
    /// return [`PosEditError::OverlappingEdits`]). On any error `self` is left
    /// unchanged (edits run against a working clone, committed only on success).
    pub fn apply_pos_edits(&mut self, edits: &[PosEdit]) -> Result<Vec<Change>, PosEditError> {
        // Reject inverted spans up front so they surface as InvertedRange
        // (consistent with the inline range API) rather than as a spurious
        // UnsupportedSpan / overlap from the ordering pass below.
        for e in edits {
            let (lo, hi) = e.span();
            if lo > hi {
                return Err(PosEditError::Range(RangeError::InvertedRange));
            }
        }

        // Process highest-position-first; an edit never shifts positions below it.
        let mut order: Vec<usize> = (0..edits.len()).collect();
        order.sort_by(|&a, &b| edits[b].span().0.cmp(&edits[a].span().0));

        // Disjointness: each (lower) edit must end at or before the previous
        // (higher) edit starts.
        for k in 1..order.len() {
            let higher = edits[order[k - 1]].span();
            let lower = edits[order[k]].span();
            if lower.1 > higher.0 {
                return Err(PosEditError::OverlappingEdits {
                    from: lower.0,
                    to: lower.1,
                });
            }
        }

        let mut work = self.clone();
        for &i in &order {
            apply_one(&mut work, &edits[i])?;
        }
        let patch = self.diff(&work);
        *self = work;
        Ok(patch)
    }
}

// ---- internals ----------------------------------------------------------

fn block_mut<'a>(root: &'a mut Node, path: &[usize]) -> Result<&'a mut Node, PosEditError> {
    root.node_at_mut(path).ok_or_else(|| {
        PosEditError::Pos(PosError::PathNotFound {
            path: path.to_vec(),
        })
    })
}

fn apply_one(work: &mut Node, edit: &PosEdit) -> Result<(), PosEditError> {
    match edit {
        PosEdit::Insert { pos, content } => insert_at(work, *pos, content),
        PosEdit::Delete { from, to } => splice(work, *from, *to, None),
        PosEdit::Replace { from, to, content } => splice(work, *from, *to, Some(content)),
        PosEdit::AddMark { from, to, mark } => {
            mark_span(work, *from, *to, &MarkOp::Add(mark.clone()))
        }
        PosEdit::RemoveMark {
            from,
            to,
            mark_type,
        } => mark_span(work, *from, *to, &MarkOp::Remove(mark_type.clone())),
        PosEdit::SetBlockAttrs { pos, attrs } => set_block_attrs(work, *pos, attrs.clone()),
    }
}

fn insert_at(work: &mut Node, pos: usize, content: &PosContent) -> Result<(), PosEditError> {
    match content {
        PosContent::Text { text, marks } => {
            let (block, inline) = work.pos_to_inline(pos)?;
            block_mut(work, &block)?.insert_text(inline, text, marks.as_deref())?;
            Ok(())
        }
        PosContent::Nodes { nodes } => {
            let r = work.resolve(pos)?;
            let (block_path, idx) = match &r.text_offset {
                // Mid-text: split the text node so nodes land on a boundary.
                Some(tp) => {
                    let bp = r.path.clone();
                    let i = ensure_boundary(
                        block_mut(work, &bp)?.children_mut(),
                        Position::new(r.index, tp.offset),
                    )?;
                    (bp, i)
                }
                None => (r.path.clone(), r.index),
            };
            let parent = block_mut(work, &block_path)?;
            for (k, n) in nodes.iter().enumerate() {
                parent.insert_child(idx + k, n.clone());
            }
            Ok(())
        }
    }
}

fn splice(
    work: &mut Node,
    from: usize,
    to: usize,
    content: Option<&PosContent>,
) -> Result<(), PosEditError> {
    let (fb, fi) = work.pos_to_inline(from)?;
    let (tb, ti) = work.pos_to_inline(to)?;

    if fb == tb {
        return splice_same_block(block_mut(work, &fb)?, fi, ti, content);
    }

    // Cross-block: only sibling blocks under a common parent (v1).
    let (parent, a, b) =
        sibling_blocks(&fb, &tb).ok_or(PosEditError::UnsupportedSpan { from, to })?;

    // 1. Trim the first block's tail, then append the replacement content.
    {
        let block_a = block_mut(work, &fb)?;
        let end = Position::new(block_a.children().len(), 0);
        block_a.delete_range(Range::new(fi, end))?;
        append_content(block_a, content)?;
    }
    // 2. Trim the last block's head.
    block_mut(work, &tb)?.delete_range(Range::new(Position::new(0, 0), ti))?;
    // 3. Drop the whole blocks between, then join the last into the first.
    block_mut(work, &parent)?.children_mut().drain(a + 1..b);
    work.join_blocks(&parent, a + 1)?;
    Ok(())
}

fn splice_same_block(
    block: &mut Node,
    from: Position,
    to: Position,
    content: Option<&PosContent>,
) -> Result<(), PosEditError> {
    match content {
        None => block.delete_range(Range::new(from, to))?,
        Some(PosContent::Text { text, marks }) => {
            block.replace_range(Range::new(from, to), text, marks.as_deref())?
        }
        Some(PosContent::Nodes { nodes }) => {
            let children = block.children_mut();
            let (s, e) = resolve_range(children, Range::new(from, to))?;
            children.drain(s..e);
            for (k, n) in nodes.iter().enumerate() {
                children.insert(s + k, n.clone());
            }
            normalize_children(children, &NormalizeOptions::default());
        }
    }
    Ok(())
}

/// Append `content` to the end of a block's inline content (used at the seam of
/// a cross-block replace).
fn append_content(block: &mut Node, content: Option<&PosContent>) -> Result<(), PosEditError> {
    match content {
        None => {}
        Some(PosContent::Text { text, marks }) => {
            let at = Position::new(block.children().len(), 0);
            block.insert_text(at, text, marks.as_deref())?;
        }
        Some(PosContent::Nodes { nodes }) => {
            let at = block.children().len();
            for (k, n) in nodes.iter().enumerate() {
                block.insert_child(at + k, n.clone());
            }
        }
    }
    Ok(())
}

enum MarkOp {
    Add(Mark),
    Remove(String),
}

fn apply_mark(block: &mut Node, range: Range, op: &MarkOp) -> Result<(), RangeError> {
    match op {
        MarkOp::Add(m) => block.add_mark_range(range, m.clone()),
        MarkOp::Remove(t) => block.remove_mark_range(range, t),
    }
}

fn mark_span(work: &mut Node, from: usize, to: usize, op: &MarkOp) -> Result<(), PosEditError> {
    let (fb, fi) = work.pos_to_inline(from)?;
    let (tb, ti) = work.pos_to_inline(to)?;

    if fb == tb {
        apply_mark(block_mut(work, &fb)?, Range::new(fi, ti), op)?;
        return Ok(());
    }

    let (parent, a, b) =
        sibling_blocks(&fb, &tb).ok_or(PosEditError::UnsupportedSpan { from, to })?;

    // First block: from `fi` to its end.
    {
        let block_a = block_mut(work, &fb)?;
        let end = Position::new(block_a.children().len(), 0);
        apply_mark(block_a, Range::new(fi, end), op)?;
    }
    // Whole blocks in between.
    for k in (a + 1)..b {
        let mut p = parent.clone();
        p.push(k);
        let blk = block_mut(work, &p)?;
        let end = Position::new(blk.children().len(), 0);
        apply_mark(blk, Range::new(Position::new(0, 0), end), op)?;
    }
    // Last block: start to `ti`.
    apply_mark(
        block_mut(work, &tb)?,
        Range::new(Position::new(0, 0), ti),
        op,
    )?;
    Ok(())
}

fn set_block_attrs(
    work: &mut Node,
    pos: usize,
    attrs: Map<String, Value>,
) -> Result<(), PosEditError> {
    let r = work.resolve(pos)?;
    // Target the node that *begins* at `pos` (the "position before a node"
    // convention): descend to the child at the boundary only when it's a real
    // node (non-text). An inline-text boundary inside a block has
    // `text_offset == None` too, but `index` points at an inline child — there
    // we target the containing block instead.
    let mut target = r.path.clone();
    if r.text_offset.is_none() {
        let parent = block_mut(work, &r.path)?;
        let descend = parent
            .children()
            .get(r.index)
            .is_some_and(|c| c.node_type.as_deref() != Some("text"));
        if descend {
            target.push(r.index);
        }
    }
    let node = block_mut(work, &target)?;
    node.attrs = if attrs.is_empty() { None } else { Some(attrs) };
    Ok(())
}

/// If `fb` and `tb` are sibling blocks under a common parent (same depth, same
/// prefix, `fb` strictly before `tb`), return `(parent_path, a, b)`.
fn sibling_blocks(fb: &[usize], tb: &[usize]) -> Option<(Vec<usize>, usize, usize)> {
    let ((&a, fp), (&b, tp)) = (fb.split_last()?, tb.split_last()?);
    if fp == tp && a < b {
        Some((fp.to_vec(), a, b))
    } else {
        None
    }
}
