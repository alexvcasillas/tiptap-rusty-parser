//! Flat-position mapping: carry a position or range through a batch of edits.
//!
//! A [`PosMap`] is a ProseMirror-style position map built from the replaced
//! spans of a disjoint edit batch (the `(from, old_len, new_len)` deltas a
//! [`PosEdit`](crate::PosEdit) batch produces). [`PosMap::map`] carries a flat
//! position from the **pre-edit** coordinate system to the **post-edit** one —
//! so a cursor, selection, decoration, or streamed-suggestion range stays
//! anchored across an [`apply_pos_edits`](crate::Node::apply_pos_edits) call.
//!
//! ```
//! use tiptap_rusty_parser::{Assoc, Node, PosContent, PosEdit};
//!
//! // doc > paragraph("hello world"); insert "big " before "world" (pos 7).
//! let mut doc = Node::element("doc")
//!     .with_child(Node::element("paragraph").with_child(Node::text("hello world")));
//! let (_patch, map) = doc
//!     .apply_pos_edits_mapped(&[PosEdit::Insert {
//!         pos: 7,
//!         content: PosContent::Text { text: "big ".into(), marks: None },
//!     }])
//!     .unwrap();
//!
//! assert_eq!(map.map(3, Assoc::Left), 3);   // before the edit: unchanged
//! assert_eq!(map.map(7, Assoc::Right), 11); // at the edit, after inserted text
//! assert_eq!(map.map(13, Assoc::Left), 17); // after the edit: shifted by +4
//! ```

use crate::pos::PosRange;
use crate::pos_edit::PosEdit;
use serde::{Deserialize, Serialize};

/// Which edge a position lying inside a replaced span maps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Assoc {
    /// Bias toward the **start** of the replacement (left edge). The default —
    /// a position stays before content inserted at it.
    #[default]
    Left,
    /// Bias toward the **end** of the replacement (right edge) — a position
    /// moves past content inserted at it.
    Right,
}

/// One replaced span in **old** coordinates: `[start, start + old_len)` became
/// `new_len` units wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Step {
    start: usize,
    old_len: usize,
    new_len: usize,
}

/// A flat position map over a batch of **disjoint** replacements, mapping
/// pre-edit positions to post-edit ones. See the [module docs](self).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PosMap {
    /// Steps in old coordinates, kept sorted ascending by `start`. Callers are
    /// expected to keep them disjoint (`push` does not enforce it); overlapping
    /// steps still map safely but to unspecified edges.
    steps: Vec<Step>,
}

impl PosMap {
    /// An empty map (the identity: every position maps to itself).
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether this map is the identity — it holds no steps, so every position
    /// maps to itself. (A same-length step still counts: it maps interior
    /// positions to an edge, so it is *not* empty.)
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Record a replacement: `[start, start + old_len)` (old coords) became
    /// `new_len` units. A no-op span (`old_len == new_len == 0`) is ignored.
    /// Steps are expected disjoint; they're kept sorted by `start`.
    pub fn push(&mut self, start: usize, old_len: usize, new_len: usize) {
        if old_len == 0 && new_len == 0 {
            return;
        }
        let step = Step {
            start,
            old_len,
            new_len,
        };
        let i = self.steps.partition_point(|s| s.start < start);
        self.steps.insert(i, step);
    }

    /// Map a flat position from pre-edit to post-edit coordinates. `assoc`
    /// decides which edge a position inside a replaced span lands on.
    pub fn map(&self, pos: usize, assoc: Assoc) -> usize {
        let mut diff: i64 = 0;
        for s in &self.steps {
            if pos < s.start {
                break; // sorted: this step and all later ones start after `pos`
            }
            let old_end = s.start + s.old_len;
            if pos > old_end {
                diff += s.new_len as i64 - s.old_len as i64;
                continue;
            }
            // `s.start <= pos <= old_end`: inside (or at a boundary of) the span.
            let into = match assoc {
                Assoc::Left => 0,
                Assoc::Right => s.new_len as i64,
            };
            return (s.start as i64 + diff + into) as usize;
        }
        (pos as i64 + diff) as usize
    }

    /// Map both endpoints of a [`PosRange`] with the same `assoc` (so a
    /// collapsed range stays collapsed).
    pub fn map_range(&self, range: PosRange, assoc: Assoc) -> PosRange {
        PosRange::new(self.map(range.from, assoc), self.map(range.to, assoc))
    }

    /// Build a map from a **disjoint** batch of edits (the same batch passed to
    /// [`apply_pos_edits`](crate::Node::apply_pos_edits)). Positions are taken in
    /// the pre-edit coordinate system; mark/attr edits contribute no step.
    pub fn from_pos_edits(edits: &[PosEdit]) -> Self {
        let mut map = PosMap::new();
        for e in edits {
            match e {
                PosEdit::Insert { pos, content } => map.push(*pos, 0, content.flat_len()),
                PosEdit::Delete { from, to } => map.push(*from, to.saturating_sub(*from), 0),
                PosEdit::Replace { from, to, content } => {
                    map.push(*from, to.saturating_sub(*from), content.flat_len())
                }
                // Marks and attrs don't change flat sizes.
                PosEdit::AddMark { .. }
                | PosEdit::RemoveMark { .. }
                | PosEdit::SetBlockAttrs { .. } => {}
            }
        }
        map
    }
}
