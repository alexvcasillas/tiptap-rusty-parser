//! Structural diff: compute a path-addressed [`Change`] list between two
//! [`Node`] trees, and [`apply`] it to reproduce the target.
//!
//! ```
//! use tiptap_rusty_parser::Document;
//!
//! let a = Document::from_json_str(
//!     r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"hi"}]}]}"#,
//! ).unwrap();
//! let b = Document::from_json_str(
//!     r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"bye"}]}]}"#,
//! ).unwrap();
//!
//! let changes = a.diff(&b);          // -> Vec<Change>
//! let mut c = a.clone();
//! c.apply(&changes).unwrap();        // reproduce `b`
//! assert_eq!(c, b);
//! ```
//!
//! ## Path convention
//! Node-local changes ([`Change::SetAttr`], `RemoveAttr`, `SetText`, `SetMarks`,
//! `SetExtra`, `RemoveExtra`, `Replace`) address the **target node** by its
//! index path. Child-list changes ([`Change::Insert`], [`Change::Remove`])
//! address the **parent** node, with `index` selecting the child — mirroring
//! [`Node::insert_child`] / [`Node::remove_child`].
//!
//! ## Apply contract
//! [`apply`] executes changes strictly in order; child `index` values are
//! interpreted against the *live* (already-partially-mutated) list. A list
//! produced by [`diff`] always reproduces the target exactly:
//! `apply(&mut a, &diff(a, b))` yields `b`.
//!
//! Empty-vs-absent container shapes (e.g. `"content":[]` vs no `content`) are
//! preserved: when the field/child ops can't express the difference, the node
//! is replaced wholesale so the round-trip stays exact.
//!
//! ## v1 limitations
//! - **No move detection**: a child relocated within a list is emitted as a
//!   [`Change::Remove`] + [`Change::Insert`] (its subtree is cloned).
//! - Child matching is LCS-by-equality; pathological reorders degrade to
//!   remove+insert (still correct, just not minimal).

use crate::node::{Mark, Node};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fmt;

/// A single structural change between two [`Node`] trees.
///
/// Serializes as a tagged object, e.g. `{"op":"setText","path":[0,0],"text":"hi"}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Change {
    /// Set (insert or overwrite) attribute `key` on the node at `path`.
    SetAttr {
        /// Index path of the target node.
        path: Vec<usize>,
        /// Attribute key.
        key: String,
        /// New attribute value.
        value: Value,
    },
    /// Remove attribute `key` from the node at `path`.
    RemoveAttr {
        /// Index path of the target node.
        path: Vec<usize>,
        /// Attribute key.
        key: String,
    },
    /// Set the text payload of the node at `path` (`None` clears it).
    SetText {
        /// Index path of the target node.
        path: Vec<usize>,
        /// New text payload, or `None` to clear.
        text: Option<String>,
    },
    /// Replace the whole mark list of the node at `path` (`None` clears it).
    SetMarks {
        /// Index path of the target node.
        path: Vec<usize>,
        /// New mark list, or `None` to clear.
        marks: Option<Vec<Mark>>,
    },
    /// Set (insert or overwrite) unknown top-level field `key` on the node at `path`.
    SetExtra {
        /// Index path of the target node.
        path: Vec<usize>,
        /// Field key.
        key: String,
        /// New field value.
        value: Value,
    },
    /// Remove unknown top-level field `key` from the node at `path`.
    RemoveExtra {
        /// Index path of the target node.
        path: Vec<usize>,
        /// Field key.
        key: String,
    },
    /// Insert `node` as a child of the node at `path` (the parent), at `index`.
    Insert {
        /// Index path of the **parent** node.
        path: Vec<usize>,
        /// Child position to insert at.
        index: usize,
        /// The node to insert.
        node: Node,
    },
    /// Remove the child at `index` of the node at `path` (the parent).
    Remove {
        /// Index path of the **parent** node.
        path: Vec<usize>,
        /// Child position to remove.
        index: usize,
    },
    /// Replace the node at `path` wholesale (used when its `type` changes).
    Replace {
        /// Index path of the target node (empty = root).
        path: Vec<usize>,
        /// The replacement node.
        node: Node,
    },
}

/// Error from [`apply`] when a change can't be located (no node at the path, or
/// a child index out of range). Lists produced by [`diff`] never fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyError {
    /// The path that could not be resolved.
    pub path: Vec<usize>,
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "apply: no node at path {:?}", self.path)
    }
}

impl std::error::Error for ApplyError {}

impl Node {
    /// Structural diff from `self` to `other`: a [`Change`] list that, when
    /// [`applied`](apply) to a clone of `self`, reproduces `other`.
    pub fn diff(&self, other: &Node) -> Vec<Change> {
        let mut out = Vec::new();
        let mut path = Vec::new();
        diff_node(self, other, &mut path, &mut out);
        out
    }

    /// Apply `changes` to `self` in order. See [`apply`].
    pub fn apply(&mut self, changes: &[Change]) -> std::result::Result<(), ApplyError> {
        apply(self, changes)
    }

    /// Invert `changes` relative to `self` (the pre-image). See [`invert`].
    pub fn invert(&self, changes: &[Change]) -> std::result::Result<Vec<Change>, ApplyError> {
        invert(self, changes)
    }
}

/// Structural diff between two nodes. Free-function form of [`Node::diff`].
pub fn diff(a: &Node, b: &Node) -> Vec<Change> {
    a.diff(b)
}

/// Apply a [`Change`] list to `root` in order, mutating it in place.
///
/// Applying `diff(a, b)` to a clone of `a` reproduces `b` exactly. Returns
/// [`ApplyError`] only for externally-authored lists whose paths/indices don't
/// resolve.
pub fn apply(root: &mut Node, changes: &[Change]) -> std::result::Result<(), ApplyError> {
    for change in changes {
        apply_one(root, change)?;
    }
    Ok(())
}

/// Invert a change list: produce the reverse changes that, applied to the
/// result of `apply(base, changes)`, restore `base` — the basis for undo.
///
/// Computed as `diff(apply(base, changes), base)`: replay the forward changes,
/// then diff back to `base`. This reuses the diff round-trip guarantee (so it
/// handles every value/shape edge exactly) and yields a minimal undo list.
/// Errors only if `changes` itself doesn't apply to `base`.
///
/// ```
/// use tiptap_rusty_parser::Document;
/// let a = Document::from_json_str(r#"{"type":"doc","content":[{"type":"paragraph"}]}"#).unwrap();
/// let b = Document::from_json_str(r#"{"type":"doc","content":[{"type":"heading"}]}"#).unwrap();
/// let forward = a.diff(&b);
/// let undo = a.invert(&forward).unwrap();
/// let mut c = b.clone();
/// c.apply(&undo).unwrap();
/// assert_eq!(c, a);
/// ```
pub fn invert(base: &Node, changes: &[Change]) -> std::result::Result<Vec<Change>, ApplyError> {
    let mut result = base.clone();
    apply(&mut result, changes)?;
    Ok(result.diff(base))
}

// ---- diff internals -----------------------------------------------------

fn diff_node(a: &Node, b: &Node, path: &mut Vec<usize>, out: &mut Vec<Change>) {
    if a == b {
        return; // prune identical subtrees (the main perf lever)
    }
    if a.node_type != b.node_type
        || empty_shape_mismatch(
            a.attrs.as_ref().map(Map::is_empty),
            b.attrs.as_ref().map(Map::is_empty),
        )
        || empty_shape_mismatch(
            a.content.as_ref().map(Vec::is_empty),
            b.content.as_ref().map(Vec::is_empty),
        )
    {
        // Type change, or an empty-vs-None container shape the field/child ops
        // can't express (e.g. `"content":[]` -> absent) -> wholesale replace.
        out.push(Change::Replace {
            path: path.clone(),
            node: b.clone(),
        });
        return;
    }
    diff_attrs(a.attrs.as_ref(), b.attrs.as_ref(), path, out);
    if a.text != b.text {
        out.push(Change::SetText {
            path: path.clone(),
            text: b.text.clone(),
        });
    }
    if a.marks != b.marks {
        out.push(Change::SetMarks {
            path: path.clone(),
            marks: b.marks.clone(),
        });
    }
    diff_extra(&a.extra, &b.extra, path, out);
    diff_children(a.children(), b.children(), path, out);
}

/// Whether an `Option<container>` shape difference (where the arg is
/// `Some(is_empty)` / `None`) can't be reconciled by the key/child ops, which
/// normalize emptied containers to `None`. A present-but-empty container
/// (`Some(true)`, e.g. parsed from `[]`/`{}`) needs an exact match on the other
/// side; otherwise the node must be replaced wholesale to round-trip exactly.
fn empty_shape_mismatch(a_is_empty: Option<bool>, b_is_empty: Option<bool>) -> bool {
    match b_is_empty {
        Some(true) => a_is_empty != Some(true),
        None => a_is_empty == Some(true),
        Some(false) => false,
    }
}

fn diff_attrs(
    a: Option<&Map<String, Value>>,
    b: Option<&Map<String, Value>>,
    path: &mut [usize],
    out: &mut Vec<Change>,
) {
    let empty = Map::new();
    let am = a.unwrap_or(&empty);
    let bm = b.unwrap_or(&empty);
    for (k, v) in bm {
        if am.get(k) != Some(v) {
            out.push(Change::SetAttr {
                path: path.to_vec(),
                key: k.clone(),
                value: v.clone(),
            });
        }
    }
    for k in am.keys() {
        if !bm.contains_key(k) {
            out.push(Change::RemoveAttr {
                path: path.to_vec(),
                key: k.clone(),
            });
        }
    }
}

fn diff_extra(
    am: &Map<String, Value>,
    bm: &Map<String, Value>,
    path: &mut [usize],
    out: &mut Vec<Change>,
) {
    for (k, v) in bm {
        if am.get(k) != Some(v) {
            out.push(Change::SetExtra {
                path: path.to_vec(),
                key: k.clone(),
                value: v.clone(),
            });
        }
    }
    for k in am.keys() {
        if !bm.contains_key(k) {
            out.push(Change::RemoveExtra {
                path: path.to_vec(),
                key: k.clone(),
            });
        }
    }
}

/// One LCS-alignment step over the (trimmed) middle child slices.
enum Step {
    Match,
    Del(usize),
    Ins(usize),
}

fn diff_children(a: &[Node], b: &[Node], path: &mut Vec<usize>, out: &mut Vec<Change>) {
    // Trim common prefix/suffix (cheap; shrinks the LCS DP and handles the
    // common append/prepend cases in linear time).
    let mut start = 0;
    while start < a.len() && start < b.len() && a[start] == b[start] {
        start += 1;
    }
    let mut ea = a.len();
    let mut eb = b.len();
    while ea > start && eb > start && a[ea - 1] == b[eb - 1] {
        ea -= 1;
        eb -= 1;
    }

    let am = &a[start..ea];
    let bm = &b[start..eb];
    if am.is_empty() && bm.is_empty() {
        return;
    }

    let steps = lcs_align(am, bm);
    let mut cursor = start; // position in the live list (prefix kept at 0..start)
    let mut dels: Vec<usize> = Vec::new();
    let mut inss: Vec<usize> = Vec::new();
    for step in steps {
        match step {
            Step::Match => {
                flush_gap(am, bm, &dels, &inss, path, &mut cursor, out);
                dels.clear();
                inss.clear();
                cursor += 1; // matched child kept in place
            }
            Step::Del(i) => dels.push(i),
            Step::Ins(j) => inss.push(j),
        }
    }
    flush_gap(am, bm, &dels, &inss, path, &mut cursor, out);
}

/// Emit ops for a gap of unmatched children. Same-type del/ins pairs recurse
/// (a modify-in-place); otherwise they become remove+insert. Indices are
/// against the live list (see module docs).
fn flush_gap(
    am: &[Node],
    bm: &[Node],
    dels: &[usize],
    inss: &[usize],
    path: &mut Vec<usize>,
    cursor: &mut usize,
    out: &mut Vec<Change>,
) {
    let pairs = dels.len().min(inss.len());
    for k in 0..pairs {
        let (ai, bj) = (dels[k], inss[k]);
        if am[ai].node_type == bm[bj].node_type {
            // same type: recurse for a minimal in-place modify
            path.push(*cursor);
            diff_node(&am[ai], &bm[bj], path, out);
            path.pop();
        } else {
            // type changed: replace the child wholesale (1 op, vs remove+insert)
            let mut child = path.clone();
            child.push(*cursor);
            out.push(Change::Replace {
                path: child,
                node: bm[bj].clone(),
            });
        }
        *cursor += 1;
    }
    for _ in &dels[pairs..] {
        out.push(Change::Remove {
            path: path.clone(),
            index: *cursor,
        }); // no cursor advance: the next child shifts into this slot
    }
    for &bj in &inss[pairs..] {
        out.push(Change::Insert {
            path: path.clone(),
            index: *cursor,
            node: bm[bj].clone(),
        });
        *cursor += 1;
    }
}

/// Longest-common-subsequence alignment of two child slices, by node equality.
fn lcs_align(a: &[Node], b: &[Node]) -> Vec<Step> {
    let (m, n) = (a.len(), b.len());
    if m == 0 {
        return (0..n).map(Step::Ins).collect();
    }
    if n == 0 {
        return (0..m).map(Step::Del).collect();
    }
    // dp[i][j] = LCS length of a[i..] vs b[j..].
    let mut dp = vec![vec![0u32; n + 1]; m + 1];
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut steps = Vec::with_capacity(m.max(n));
    let (mut i, mut j) = (0usize, 0usize);
    while i < m && j < n {
        if a[i] == b[j] {
            steps.push(Step::Match);
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            steps.push(Step::Del(i));
            i += 1;
        } else {
            steps.push(Step::Ins(j));
            j += 1;
        }
    }
    while i < m {
        steps.push(Step::Del(i));
        i += 1;
    }
    while j < n {
        steps.push(Step::Ins(j));
        j += 1;
    }
    steps
}

// ---- apply internals ----------------------------------------------------

fn node_at_mut<'a>(
    root: &'a mut Node,
    path: &[usize],
) -> std::result::Result<&'a mut Node, ApplyError> {
    root.node_at_mut(path).ok_or_else(|| ApplyError {
        path: path.to_vec(),
    })
}

fn apply_one(root: &mut Node, change: &Change) -> std::result::Result<(), ApplyError> {
    match change {
        Change::SetAttr { path, key, value } => {
            node_at_mut(root, path)?.set_attr(key.clone(), value.clone());
        }
        Change::RemoveAttr { path, key } => {
            node_at_mut(root, path)?.remove_attr(key);
        }
        Change::SetText { path, text } => {
            node_at_mut(root, path)?.text = text.clone();
        }
        Change::SetMarks { path, marks } => {
            node_at_mut(root, path)?.marks = marks.clone();
        }
        Change::SetExtra { path, key, value } => {
            node_at_mut(root, path)?
                .extra
                .insert(key.clone(), value.clone());
        }
        Change::RemoveExtra { path, key } => {
            node_at_mut(root, path)?.extra.remove(key);
        }
        Change::Insert { path, index, node } => {
            let parent = node_at_mut(root, path)?;
            if *index > parent.child_count() {
                let mut p = path.clone();
                p.push(*index);
                return Err(ApplyError { path: p });
            }
            parent.insert_child(*index, node.clone());
        }
        Change::Remove { path, index } => {
            if node_at_mut(root, path)?.remove_child(*index).is_none() {
                let mut p = path.clone();
                p.push(*index);
                return Err(ApplyError { path: p });
            }
        }
        Change::Replace { path, node } => {
            if path.is_empty() {
                *root = node.clone();
            } else {
                let (parent_path, last) = path.split_at(path.len() - 1);
                let parent = node_at_mut(root, parent_path)?;
                if parent.replace_child(last[0], node.clone()).is_none() {
                    return Err(ApplyError { path: path.clone() });
                }
            }
        }
    }
    Ok(())
}
