//! Canonicalize a [`Node`] tree: merge adjacent equal-mark text, drop empty
//! text nodes, and (opt-in) prune empty non-text nodes.
//!
//! Normalization produces a stable canonical form: smaller [`diff`](crate::diff)
//! output, cleaner roundtrips, and a single representation for trees that are
//! semantically identical but split differently (e.g. `"foo"` as one text node
//! vs `"fo"`+`"o"` with the same marks). Tuned with [`NormalizeOptions`] — a
//! plain data struct (no closures), so the surface works over WASM/FFI.
//!
//! `normalize` is idempotent: `normalize(normalize(x)) == normalize(x)`.
//!
//! ```
//! use tiptap_rusty_parser::Document;
//! let mut doc = Document::from_json_str(
//!     r#"{"type":"doc","content":[{"type":"paragraph","content":[
//!         {"type":"text","text":"foo"},
//!         {"type":"text","text":"bar"},
//!         {"type":"text","text":""}
//!     ]}]}"#,
//! ).unwrap();
//! doc.normalize();
//! assert_eq!(doc.text_content(), "foobar");
//! // The three text nodes collapsed into one.
//! assert_eq!(doc.children()[0].child_count(), 1);
//! ```

use crate::node::Node;
use serde::{Deserialize, Serialize};

/// Options controlling [`Node::normalize_with`]. [`Default`] is the sensible
/// hygiene pass: merge adjacent text and drop empty text, but keep empty nodes
/// (an empty paragraph is structurally valid).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NormalizeOptions {
    /// Merge consecutive text nodes that are identical except for `text`
    /// (same `marks`, `attrs`, `extra`) by concatenating their `text`.
    pub merge_adjacent_text: bool,
    /// Drop text nodes whose `text` is empty (`""` or absent).
    pub remove_empty_text: bool,
    /// Drop non-text nodes whose `content` is an empty list (`Some([])`).
    /// Off by default: an empty paragraph is valid. Absent (`None`) content is
    /// always left untouched, preserving the empty-vs-missing distinction.
    pub remove_empty_nodes: bool,
}

impl Default for NormalizeOptions {
    fn default() -> Self {
        Self {
            merge_adjacent_text: true,
            remove_empty_text: true,
            remove_empty_nodes: false,
        }
    }
}

impl Node {
    /// Normalize this subtree in place with default [`NormalizeOptions`].
    pub fn normalize(&mut self) {
        self.normalize_with(&NormalizeOptions::default());
    }

    /// Normalize this subtree in place with custom [`NormalizeOptions`].
    pub fn normalize_with(&mut self, opts: &NormalizeOptions) {
        if let Some(children) = self.content.as_mut() {
            // Bottom-up: normalize each child first, so empties/merges that
            // surface deeper propagate before this level compacts.
            for child in children.iter_mut() {
                child.normalize_with(opts);
            }
            normalize_children(children, opts);
        }
    }
}

#[inline]
fn is_text(n: &Node) -> bool {
    n.node_type.as_deref() == Some("text")
}

#[inline]
fn is_empty_text(n: &Node) -> bool {
    is_text(n) && n.text.as_deref().unwrap_or("").is_empty()
}

/// True if two text nodes differ only in their `text` payload.
#[inline]
fn mergeable(a: &Node, b: &Node) -> bool {
    is_text(a) && is_text(b) && a.marks == b.marks && a.attrs == b.attrs && a.extra == b.extra
}

/// Canonicalize one parent's child list in place. Shared building block reused
/// by inline range editing (merge-after-edit).
pub(crate) fn normalize_children(children: &mut Vec<Node>, opts: &NormalizeOptions) {
    if opts.remove_empty_text {
        children.retain(|c| !is_empty_text(c));
    }
    if opts.merge_adjacent_text {
        let mut i = 0;
        while i + 1 < children.len() {
            if mergeable(&children[i], &children[i + 1]) {
                let next = children.remove(i + 1);
                if let Some(t) = next.text {
                    children[i]
                        .text
                        .get_or_insert_with(String::new)
                        .push_str(&t);
                }
            } else {
                i += 1;
            }
        }
    }
    if opts.remove_empty_nodes {
        children.retain(|c| is_text(c) || !c.content.as_ref().is_some_and(Vec::is_empty));
    }
}
