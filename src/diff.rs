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
//! ## Move detection
//! A child relocated within a list is emitted as a single [`Change::Move`]
//! (no subtree clone) instead of a remove+insert: after LCS alignment, leftover
//! deletions and insertions that are *equal by value* are paired as moves, and
//! their live indices are derived by simulating the op stream — so the result
//! reproduces the target exactly regardless of how moves interleave with plain
//! inserts/removes. Pairing is greedy/FIFO, so duplicate-equal children give a
//! correct (if not always minimal) result. [`invert`] needs no special handling:
//! it re-diffs the reverse direction, re-deriving the inverse moves.
//!
//! ## v1 limitations
//! - Matching is LCS-by-equality; modifies are paired positionally within the
//!   gaps between matched anchors (still correct, just not always minimal).

use crate::node::{Mark, Node};
use crate::text_diff::{self, DiffGranularity, DiffOptions};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
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
    /// Splice a text node's payload: replace the scalar range
    /// `[from, from+len_del)` with `insert`. Offsets count Unicode scalars.
    /// Emitted by [`Node::diff_with`](crate::Node::diff_with) in inline/smart
    /// granularity instead of a whole-string [`SetText`](Change::SetText).
    SpliceText {
        /// Index path of the target text node.
        path: Vec<usize>,
        /// Scalar offset where the splice starts.
        from: usize,
        /// Number of scalars removed.
        len_del: usize,
        /// Text inserted at `from`.
        insert: String,
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
    /// Relocate a child within the same parent's list, without cloning its
    /// subtree. `from`/`to` are interpreted against the *live* list: the child
    /// at `from` is removed first, then re-inserted so it lands at index `to`
    /// in the post-removal list — observationally `Remove{from}` + `Insert{to}`,
    /// so it composes with the same live-index cursor model as the other ops.
    Move {
        /// Index path of the **parent** node.
        path: Vec<usize>,
        /// Live index of the child to move (before this op).
        from: usize,
        /// Target index in the list after the child is removed.
        to: usize,
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
        self.diff_with(other, &DiffOptions::default())
    }

    /// Structural diff with [`DiffOptions`] — e.g. inline/smart text granularity
    /// (character-level [`Change::SpliceText`] instead of whole [`Change::SetText`]).
    pub fn diff_with(&self, other: &Node, opts: &DiffOptions) -> Vec<Change> {
        let mut out = Vec::new();
        let mut path = Vec::new();
        diff_node(self, other, &mut path, &mut out, opts);
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
/// then diff back to `base`. This reuses the diff round-trip guarantee, so it
/// handles every value/shape edge exactly — subject to the same non-minimality
/// caveats as [`diff`] (e.g. no move detection). Errors only if `changes`
/// itself doesn't apply to `base`.
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

fn diff_node(a: &Node, b: &Node, path: &mut Vec<usize>, out: &mut Vec<Change>, opts: &DiffOptions) {
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
        diff_text_field(a.text.as_deref(), b.text.as_deref(), path, out, opts);
    }
    if a.marks != b.marks {
        out.push(Change::SetMarks {
            path: path.clone(),
            marks: b.marks.clone(),
        });
    }
    diff_extra(&a.extra, &b.extra, path, out);
    diff_children(a.children(), b.children(), path, out, opts);
}

/// Emit the change(s) for a differing text payload, honoring the granularity.
fn diff_text_field(
    a: Option<&str>,
    b: Option<&str>,
    path: &mut [usize],
    out: &mut Vec<Change>,
    opts: &DiffOptions,
) {
    // Character-level splices only make sense when both sides are present text.
    if let (Some(at), Some(bt)) = (a, b) {
        match opts.text {
            DiffGranularity::Inline => {
                let segs = text_diff::diff_text(at, bt);
                text_diff::splices_from_segments(path, &segs, out);
                return;
            }
            DiffGranularity::Smart { replace_threshold } => {
                let segs = text_diff::diff_text(at, bt);
                let total = at.chars().count() + bt.chars().count();
                let changed = text_diff::changed_scalars(&segs);
                if total == 0 || (changed as f32 / total as f32) <= replace_threshold {
                    text_diff::splices_from_segments(path, &segs, out);
                    return;
                }
            }
            DiffGranularity::Block => {}
        }
    }
    out.push(Change::SetText {
        path: path.to_vec(),
        text: b.map(str::to_owned),
    });
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

/// A live-list element identity used by the move simulation: either an original
/// `a`-child (reused/relocated/modified in place) or a freshly inserted node.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Key {
    Orig(usize),
    New(usize),
}

/// Positional pairing of a leftover deletion with a leftover insertion in the
/// same gap: a same-type modify (recurse) or a different-type wholesale replace.
enum Role {
    Modify(usize),
    Replace(usize),
}

fn diff_children(
    a: &[Node],
    b: &[Node],
    path: &mut Vec<usize>,
    out: &mut Vec<Change>,
    opts: &DiffOptions,
) {
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

    // Reconstruct the alignment: which bm index each am index matched (kept in
    // place), plus the per-gap unmatched deletions/insertions between anchors.
    let mut gaps: Vec<(Vec<usize>, Vec<usize>)> = Vec::new();
    let mut cur_dels: Vec<usize> = Vec::new();
    let mut cur_inss: Vec<usize> = Vec::new();
    let mut bm_match_src: HashMap<usize, usize> = HashMap::new(); // bm_j -> am_i
    let (mut ai, mut bj) = (0usize, 0usize);
    for step in &steps {
        match step {
            Step::Match => {
                gaps.push((std::mem::take(&mut cur_dels), std::mem::take(&mut cur_inss)));
                bm_match_src.insert(bj, ai);
                ai += 1;
                bj += 1;
            }
            Step::Del(i) => {
                cur_dels.push(*i);
                ai += 1;
            }
            Step::Ins(j) => {
                cur_inss.push(*j);
                bj += 1;
            }
        }
    }
    gaps.push((cur_dels, cur_inss));

    // Move detection: pair value-equal nodes that LCS split into a deletion and
    // an insertion. Greedy/FIFO over equal values -> stable on duplicates.
    let all_dels: Vec<usize> = gaps.iter().flat_map(|(d, _)| d.iter().copied()).collect();
    let mut del_used = vec![false; all_dels.len()];
    let mut bm_move_src: HashMap<usize, usize> = HashMap::new(); // bm_j -> am_d
    let mut am_moved: HashSet<usize> = HashSet::new();
    for (_, inss) in &gaps {
        for &j in inss {
            if let Some(slot) = all_dels
                .iter()
                .enumerate()
                .find(|(slot, &d)| !del_used[*slot] && am[d] == bm[j])
                .map(|(slot, _)| slot)
            {
                del_used[slot] = true;
                bm_move_src.insert(j, all_dels[slot]);
                am_moved.insert(all_dels[slot]);
            }
        }
    }

    // Per-gap modify/replace pairing on the remaining (non-moved) leftovers.
    let mut bm_role: HashMap<usize, Role> = HashMap::new();
    let mut removed_am: Vec<usize> = Vec::new();
    for (dels, inss) in &gaps {
        let rem_dels: Vec<usize> = dels
            .iter()
            .copied()
            .filter(|d| !am_moved.contains(d))
            .collect();
        let rem_inss: Vec<usize> = inss
            .iter()
            .copied()
            .filter(|j| !bm_move_src.contains_key(j))
            .collect();
        let pairs = rem_dels.len().min(rem_inss.len());
        for k in 0..pairs {
            let (d, j) = (rem_dels[k], rem_inss[k]);
            if am[d].node_type == bm[j].node_type {
                bm_role.insert(j, Role::Modify(d)); // same type -> recurse in place
            } else {
                bm_role.insert(j, Role::Replace(d)); // type change -> wholesale
            }
        }
        removed_am.extend_from_slice(&rem_dels[pairs..]);
    }

    // Target key sequence for the middle (what the live list must become), plus
    // a key -> target-index lookup used to place relocated nodes minimally.
    let mut target: Vec<Key> = Vec::with_capacity(bm.len());
    for j in 0..bm.len() {
        let key = if let Some(&i) = bm_match_src.get(&j) {
            Key::Orig(i)
        } else if let Some(&d) = bm_move_src.get(&j) {
            Key::Orig(d)
        } else {
            match bm_role.get(&j) {
                Some(Role::Modify(d) | Role::Replace(d)) => Key::Orig(*d),
                None => Key::New(j),
            }
        };
        target.push(key);
    }
    let target_pos: HashMap<Key, usize> = target.iter().enumerate().map(|(i, &k)| (k, i)).collect();
    let is_moved = |k: Key| matches!(k, Key::Orig(x) if am_moved.contains(&x));

    // Simulate the live list and emit ops whose indices are valid by
    // construction. Absolute index = start + position in the middle.
    let mut live: Vec<Key> = (0..am.len()).map(Key::Orig).collect();

    // 1. Plain removes (indices computed against the shrinking live list).
    for &d in &removed_am {
        let idx = live.iter().position(|&k| k == Key::Orig(d)).unwrap();
        out.push(Change::Remove {
            path: path.clone(),
            index: start + idx,
        });
        live.remove(idx);
    }

    // 2. Settle each target position left to right, maintaining the invariant
    //    `live[..p] == target[..p]`. New nodes are inserted; otherwise we
    //    relocate exactly one genuinely-moved node (the displaced occupant, or
    //    the wanted node if it's the one that moved) to its sorted destination —
    //    one Move per moved node, never disturbing the matched skeleton.
    let mut p = 0;
    while p < target.len() {
        let want = target[p];
        if live.get(p).copied() == Some(want) {
            p += 1;
            continue;
        }
        if let Key::New(j) = want {
            out.push(Change::Insert {
                path: path.clone(),
                index: start + p,
                node: bm[j].clone(),
            });
            live.insert(p, want);
            p += 1;
            continue;
        }
        // `want` is an original node not yet at `p`. Move the displaced occupant
        // if it is the relocated one, else pull `want` (itself relocated) here.
        let cur = live[p];
        let mover = if is_moved(cur) { cur } else { want };
        let from = live.iter().position(|&k| k == mover).unwrap();
        let tmover = target_pos[&mover];
        // Destination = number of currently-present elements that precede it in
        // the target order (excluding the mover itself).
        let dest = live
            .iter()
            .filter(|&&k| k != mover && target_pos[&k] < tmover)
            .count();
        out.push(Change::Move {
            path: path.clone(),
            from: start + from,
            to: start + dest,
        });
        let k = live.remove(from);
        live.insert(dest, k);
        // Re-check `p` (the move may or may not have settled this slot).
    }
    debug_assert_eq!(live, target, "diff move simulation diverged from target");

    // 3. Value fixes for modify/replace pairs, at their final positions (the
    //    structural layout is now settled, so index = start + bm index).
    for (j, target_node) in bm.iter().enumerate() {
        match bm_role.get(&j) {
            Some(Role::Modify(d)) => {
                path.push(start + j);
                diff_node(&am[*d], target_node, path, out, opts);
                path.pop();
            }
            Some(Role::Replace(_)) => {
                let mut child = path.clone();
                child.push(start + j);
                out.push(Change::Replace {
                    path: child,
                    node: target_node.clone(),
                });
            }
            None => {}
        }
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
        Change::SpliceText {
            path,
            from,
            len_del,
            insert,
        } => {
            let node = node_at_mut(root, path)?;
            let s = node.text.get_or_insert_with(String::new);
            // Translate scalar offsets to byte offsets (scalar-safe). A scalar
            // index equal to the length maps to the byte length (end splice);
            // anything past that is out of range.
            let scalars = s.chars().count();
            if *from > scalars || from + len_del > scalars {
                return Err(ApplyError { path: path.clone() });
            }
            let start = s.char_indices().nth(*from).map_or(s.len(), |(b, _)| b);
            let end = s
                .char_indices()
                .nth(from + len_del)
                .map_or(s.len(), |(b, _)| b);
            s.replace_range(start..end, insert);
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
        Change::Move { path, from, to } => {
            let parent = node_at_mut(root, path)?;
            let len = parent.child_count();
            // `from` indexes the live list; `to` indexes the list after removal,
            // so its valid range is 0..=len-1.
            if *from >= len || *to >= len {
                let mut p = path.clone();
                p.push(if *from >= len { *from } else { *to });
                return Err(ApplyError { path: p });
            }
            let node = parent.remove_child(*from).expect("from bounds-checked");
            parent.insert_child(*to, node);
        }
    }
    Ok(())
}
