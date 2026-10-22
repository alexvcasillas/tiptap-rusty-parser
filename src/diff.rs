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
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
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
    /// Splice a text node's payload: replace the scalar range
    /// `[from, from+len_del)` with `insert`. Offsets count Unicode scalars.
    /// Emitted by [`Node::diff_with`](crate::Node::diff_with) in inline/smart
    /// granularity instead of a whole-string [`SetText`](Change::SetText).
    ///
    /// Appended last (rather than grouped next to [`SetText`](Change::SetText))
    /// so existing variants keep their discriminants — only the new-variant
    /// break remains, not a discriminant shift for downstream `as` casts.
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

    // Equality classes: one id per distinct child value, so alignment and move
    // pairing compare integers instead of whole subtrees (O(size) once).
    let (ida, idb) = intern(am, bm);
    let steps = lcs_align(&ida, &idb);

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
    // an insertion. Greedy/FIFO over equal values -> stable on duplicates: each
    // insertion takes the first unused deletion of its class (per-class queues).
    let mut dels_by_id: HashMap<u32, std::collections::VecDeque<usize>> = HashMap::new();
    for (dels, _) in &gaps {
        for &d in dels {
            dels_by_id.entry(ida[d]).or_default().push_back(d);
        }
    }
    let mut bm_move_src: HashMap<usize, usize> = HashMap::new(); // bm_j -> am_d
    let mut am_moved: HashSet<usize> = HashSet::new();
    for (_, inss) in &gaps {
        for &j in inss {
            if let Some(d) = dels_by_id.get_mut(&idb[j]).and_then(|q| q.pop_front()) {
                bm_move_src.insert(j, d);
                am_moved.insert(d);
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

    // Target key sequence for the middle (what the live list must become), as
    // dense ids: `Orig(i)` -> `i`, `New(j)` -> `am.len() + j`. Plus an
    // id -> target-index lookup used to place relocated nodes minimally.
    let n_orig = am.len() as u32;
    let mut target: Vec<u32> = Vec::with_capacity(bm.len());
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
        target.push(match key {
            Key::Orig(i) => i as u32,
            Key::New(j) => n_orig + j as u32,
        });
    }
    let n_ids = am.len() + bm.len();
    let mut target_pos = vec![u32::MAX; n_ids];
    for (t, &k) in target.iter().enumerate() {
        target_pos[k as usize] = t as u32;
    }
    let mut is_moved = vec![false; n_ids];
    for &d in &am_moved {
        is_moved[d] = true;
    }

    // 1. Plain removes. `removed_am` is ascending (gap order, then index order
    //    within a gap), so the live index is the original minus prior removals.
    for (k, &d) in removed_am.iter().enumerate() {
        out.push(Change::Remove {
            path: path.clone(),
            index: start + d - k,
        });
    }
    let mut is_removed = vec![false; am.len()];
    for &d in &removed_am {
        is_removed[d] = true;
    }
    let post_remove = || (0..n_orig).filter(|&i| !is_removed[i as usize]);

    // 2. Simulate the live list and emit ops whose indices are valid by
    //    construction. Absolute index = start + position in the middle.
    let mark = out.len();
    if !settle_sorted(
        bm,
        &target,
        &target_pos,
        &is_moved,
        am_moved.len(),
        post_remove(),
        start,
        path,
        out,
    ) {
        // The sorted settle cascaded (re-moving nodes it already placed, which
        // blows up on many value-equal reordered siblings): redo it with
        // exactly one Move per relocated node.
        out.truncate(mark);
        settle_cursor(bm, &target, &is_moved, post_remove(), start, path, out);
    }

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

/// Settle each target position left to right, maintaining the invariant
/// `live[..p] == target[..p]`. New nodes are inserted; otherwise relocate one
/// genuinely-moved node (the displaced occupant, or the wanted node if it's the
/// one that moved) to its sorted destination, never disturbing the matched
/// skeleton. Usually one Move per moved node; returns `false` (output partial)
/// once more than `budget` Moves would be needed, i.e. it started cascading.
#[allow(clippy::too_many_arguments)]
fn settle_sorted(
    bm: &[Node],
    target: &[u32],
    target_pos: &[u32],
    is_moved: &[bool],
    budget: usize,
    live0: impl Iterator<Item = u32>,
    start: usize,
    path: &[usize],
    out: &mut Vec<Change>,
) -> bool {
    let n_orig = is_moved.len() - bm.len();
    let mut live = LiveList::new(live0, is_moved.len());
    // `present.prefix(t)` = live elements whose target index is < t.
    let mut present = Fenwick::new(target.len());
    for (t, &k) in target.iter().enumerate() {
        if (k as usize) < n_orig {
            present.add(t);
        }
    }
    let mut moves = 0;
    let mut p = 0;
    while p < target.len() {
        let want = target[p];
        let cur = live.get(p);
        if cur == Some(want) {
            p += 1;
            continue;
        }
        if want as usize >= n_orig {
            out.push(Change::Insert {
                path: path.to_vec(),
                index: start + p,
                node: bm[want as usize - n_orig].clone(),
            });
            live.insert(p, want);
            present.add(p);
            p += 1;
            continue;
        }
        if moves == budget {
            return false;
        }
        moves += 1;
        // `want` is an original node not yet at `p`. Move the displaced occupant
        // if it is the relocated one, else pull `want` (itself relocated) here.
        let cur = cur.expect("original node missing from live list");
        let mover = if is_moved[cur as usize] { cur } else { want };
        let from = live.index_of(mover);
        // Destination = number of currently-present elements that precede it in
        // the target order (the mover itself is not counted).
        let dest = present.prefix(target_pos[mover as usize] as usize);
        out.push(Change::Move {
            path: path.to_vec(),
            from: start + from,
            to: start + dest,
        });
        let k = live.remove(from);
        live.insert(dest, k);
        // Re-check `p` (the move may or may not have settled this slot).
    }
    true
}

/// Fallback settle with exactly one Move per relocated node. Live list =
/// placed nodes (a prefix of `target`) interleaved with not-yet-placed
/// originals, all placed ones before `cur`; past `cur` only unplaced originals,
/// in original order. Non-moved originals keep their relative order in
/// `target`, so they are reached by advancing `cur`; moved ones are pulled to it.
fn settle_cursor(
    bm: &[Node],
    target: &[u32],
    is_moved: &[bool],
    live0: impl Iterator<Item = u32>,
    start: usize,
    path: &[usize],
    out: &mut Vec<Change>,
) {
    let n_orig = is_moved.len() - bm.len();
    let mut live = LiveList::new(live0, is_moved.len());
    let mut cur = 0;
    for &want in target {
        if want as usize >= n_orig {
            out.push(Change::Insert {
                path: path.to_vec(),
                index: start + cur,
                node: bm[want as usize - n_orig].clone(),
            });
            live.insert(cur, want);
            cur += 1;
            continue;
        }
        let at = live.index_of(want);
        if !is_moved[want as usize] {
            debug_assert!(at >= cur, "non-moved node behind the cursor");
            cur = at + 1;
            continue;
        }
        if at == cur {
            cur += 1;
            continue;
        }
        live.remove(at);
        if at < cur {
            cur -= 1;
        }
        out.push(Change::Move {
            path: path.to_vec(),
            from: start + at,
            to: start + cur,
        });
        live.insert(cur, want);
        cur += 1;
    }
    debug_assert_eq!(
        live.to_vec(),
        target,
        "diff move simulation diverged from target"
    );
}

/// Order-statistic list of key ids for the move simulation, split into
/// ~√n-sized blocks so `get`/`index_of`/`insert`/`remove` cost O(√n) instead
/// of O(n).
struct LiveList {
    blocks: Vec<Vec<u32>>,
    /// Key id -> index of the block holding it.
    block_of: Vec<u32>,
    cap: usize,
}

impl LiveList {
    fn new(keys: impl Iterator<Item = u32>, n_ids: usize) -> Self {
        let mut l = LiveList {
            blocks: vec![keys.collect()],
            block_of: vec![0; n_ids],
            cap: 16,
        };
        l.rebuild();
        l
    }

    /// Re-chunk into blocks of `cap` (≈ √len) and refresh `block_of`.
    fn rebuild(&mut self) {
        let all = self.to_vec();
        self.cap = (all.len() as f64).sqrt() as usize + 16;
        self.blocks = all.chunks(self.cap).map(<[u32]>::to_vec).collect();
        if self.blocks.is_empty() {
            self.blocks.push(Vec::new());
        }
        for (b, blk) in self.blocks.iter().enumerate() {
            for &k in blk {
                self.block_of[k as usize] = b as u32;
            }
        }
    }

    fn to_vec(&self) -> Vec<u32> {
        self.blocks.concat()
    }

    /// Block and in-block offset of live index `idx` (`idx == len` maps to the
    /// end of the last block).
    fn locate(&self, mut idx: usize) -> (usize, usize) {
        let last = self.blocks.len() - 1;
        for (b, blk) in self.blocks.iter().enumerate() {
            if idx < blk.len() || b == last {
                return (b, idx);
            }
            idx -= blk.len();
        }
        unreachable!()
    }

    fn get(&self, idx: usize) -> Option<u32> {
        let (b, o) = self.locate(idx);
        self.blocks[b].get(o).copied()
    }

    fn index_of(&self, key: u32) -> usize {
        let b = self.block_of[key as usize] as usize;
        let before: usize = self.blocks[..b].iter().map(Vec::len).sum();
        before
            + self.blocks[b]
                .iter()
                .position(|&k| k == key)
                .expect("key in its block")
    }

    fn remove(&mut self, idx: usize) -> u32 {
        let (b, o) = self.locate(idx);
        self.blocks[b].remove(o)
    }

    fn insert(&mut self, idx: usize, key: u32) {
        let (b, o) = self.locate(idx);
        self.blocks[b].insert(o, key);
        self.block_of[key as usize] = b as u32;
        if self.blocks[b].len() > 2 * self.cap {
            self.rebuild();
        }
    }
}

/// Fenwick (binary indexed) tree of counts over target indices.
struct Fenwick(Vec<u32>);

impl Fenwick {
    fn new(n: usize) -> Self {
        Fenwick(vec![0; n])
    }

    /// Count one entry at index `i` (0-based tree: `self.0[k]` covers
    /// `k & (k + 1) ..= k`).
    fn add(&mut self, mut i: usize) {
        while i < self.0.len() {
            self.0[i] += 1;
            i |= i + 1;
        }
    }

    /// Count of entries at indices `< i`.
    fn prefix(&self, mut i: usize) -> usize {
        let mut s = 0;
        while i > 0 {
            s += self.0[i - 1] as usize;
            i &= i - 1;
        }
        s
    }
}

/// Below this many DP cells (`m·n`) the exact table LCS runs (small lists, the
/// common case); above it Myers' O((m+n)·D) greedy LCS, which is linear for the
/// typical "few edits in a long list". Both produce a longest common
/// subsequence; tie-breaking between equally long alignments may differ.
const DP_CELLS: usize = 1 << 16;

/// Myers gives up past this many edits (its trace is O(D²)); then the table LCS
/// runs if it fits in [`DP_FALLBACK_CELLS`], else no anchors are kept.
const MYERS_MAX_D: usize = 4096;
const DP_FALLBACK_CELLS: usize = 1 << 24;

/// Longest-common-subsequence alignment of two child slices, by equality class
/// (see [`intern`]).
fn lcs_align(a: &[u32], b: &[u32]) -> Vec<Step> {
    let (m, n) = (a.len(), b.len());
    if m == 0 {
        return (0..n).map(Step::Ins).collect();
    }
    if n == 0 {
        return (0..m).map(Step::Del).collect();
    }
    if m.saturating_mul(n) <= DP_CELLS {
        return lcs_table(a, b);
    }
    if let Some(steps) = myers(a, b, MYERS_MAX_D) {
        return steps;
    }
    if m.saturating_mul(n) <= DP_FALLBACK_CELLS {
        return lcs_table(a, b);
    }
    // Pathological (huge and mostly different): no anchors. Still correct:
    // the gap pairs everything positionally as modify/replace.
    (0..m).map(Step::Del).chain((0..n).map(Step::Ins)).collect()
}

/// Exact LCS by dynamic programming, O(m·n) time and space.
fn lcs_table(a: &[u32], b: &[u32]) -> Vec<Step> {
    let (m, n) = (a.len(), b.len());
    // dp[i][j] = LCS length of a[i..] vs b[j..], flattened row-major.
    let w = n + 1;
    let mut dp = vec![0u32; (m + 1) * w];
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            dp[i * w + j] = if a[i] == b[j] {
                dp[(i + 1) * w + j + 1] + 1
            } else {
                dp[(i + 1) * w + j].max(dp[i * w + j + 1])
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
        } else if dp[(i + 1) * w + j] >= dp[i * w + j + 1] {
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

/// Myers' greedy shortest-edit-script (= LCS) alignment, O((m+n)·D) time and
/// O(D²) trace space. `None` if more than `max_d` edits are needed.
fn myers(a: &[u32], b: &[u32], max_d: usize) -> Option<Vec<Step>> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max_d = max_d.min(a.len() + b.len()) as isize;
    let off = max_d + 1;
    // v[k + off] = furthest x on diagonal k = x - y.
    let mut v = vec![0isize; (2 * off + 1) as usize];
    // trace[d] = v[-d..=d] before round d.
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut end_d = None;
    'outer: for d in 0..=max_d {
        trace.push(v[(off - d) as usize..=(off + d) as usize].to_vec());
        let mut k = -d;
        while k <= d {
            let down = k == -d || (k != d && v[(off + k - 1) as usize] < v[(off + k + 1) as usize]);
            let mut x = if down {
                v[(off + k + 1) as usize]
            } else {
                v[(off + k - 1) as usize] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[(off + k) as usize] = x;
            if x >= n && y >= m {
                end_d = Some(d);
                break 'outer;
            }
            k += 2;
        }
    }
    end_d?;
    // Backtrack from (n, m) through the saved rounds.
    let mut steps = Vec::with_capacity((n + m) as usize);
    let (mut x, mut y) = (n, m);
    for d in (0..trace.len() as isize).rev() {
        if d == 0 {
            while x > 0 && y > 0 {
                steps.push(Step::Match);
                x -= 1;
                y -= 1;
            }
            break;
        }
        let vd = &trace[d as usize];
        let get = |k: isize| vd[(k + d) as usize];
        let k = x - y;
        let prev_k = if k == -d || (k != d && get(k - 1) < get(k + 1)) {
            k + 1
        } else {
            k - 1
        };
        let prev_x = get(prev_k);
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            steps.push(Step::Match);
            x -= 1;
            y -= 1;
        }
        if x == prev_x {
            steps.push(Step::Ins((y - 1) as usize));
        } else {
            steps.push(Step::Del((x - 1) as usize));
        }
        x = prev_x;
        y = prev_y;
    }
    steps.reverse();
    Some(steps)
}

/// Map every child of `a` and `b` to an equality-class id: equal ids iff the
/// nodes are equal (`==`). Hash-bucketed, each node is hashed once and compared
/// deeply only against its bucket's representatives, so the cost is O(total
/// size) instead of O(m·n) subtree comparisons.
fn intern(a: &[Node], b: &[Node]) -> (Vec<u32>, Vec<u32>) {
    let mut buckets: HashMap<u64, Vec<(u32, &Node)>> = HashMap::with_capacity(a.len() + b.len());
    let mut next = 0u32;
    let mut out_a = Vec::with_capacity(a.len());
    let mut out_b = Vec::with_capacity(b.len());
    for (side, nodes) in [(0, a), (1, b)] {
        for n in nodes {
            let bucket = buckets.entry(node_hash(n)).or_default();
            let found = bucket.iter().find(|(_, rep)| *rep == n).map(|(i, _)| *i);
            let i = found.unwrap_or_else(|| {
                let i = next;
                next += 1;
                bucket.push((i, n));
                i
            });
            if side == 0 {
                out_a.push(i);
            } else {
                out_b.push(i);
            }
        }
    }
    (out_a, out_b)
}

/// Structural hash consistent with `Node: PartialEq` (maps hash
/// order-insensitively, like their equality).
fn node_hash(n: &Node) -> u64 {
    let mut h = Fx::default();
    hash_node(n, &mut h);
    h.0
}

#[derive(Default)]
struct Fx(u64);

impl Fx {
    #[inline]
    fn add(&mut self, x: u64) {
        self.0 = (self.0.rotate_left(5) ^ x).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    #[inline]
    fn bytes(&mut self, s: &[u8]) {
        let (chunks, rest) = s.as_chunks::<8>();
        for c in chunks {
            self.add(u64::from_le_bytes(*c));
        }
        let mut tail = [0u8; 8];
        tail[..rest.len()].copy_from_slice(rest);
        self.add(u64::from_le_bytes(tail) ^ ((s.len() as u64) << 56));
    }
    #[inline]
    fn opt_str(&mut self, s: Option<&str>) {
        match s {
            None => self.add(0x9e37),
            Some(s) => self.bytes(s.as_bytes()),
        }
    }
}

fn hash_node(n: &Node, h: &mut Fx) {
    h.opt_str(n.node_type.as_deref());
    hash_opt_map(n.attrs.as_ref(), h);
    match &n.content {
        None => h.add(1),
        Some(c) => {
            h.add(2 ^ ((c.len() as u64) << 8));
            for child in c {
                hash_node(child, h);
            }
        }
    }
    match &n.marks {
        None => h.add(3),
        Some(ms) => {
            h.add(4 ^ ((ms.len() as u64) << 8));
            for m in ms {
                h.bytes(m.mark_type.as_bytes());
                hash_opt_map(m.attrs.as_ref(), h);
                hash_map(&m.extra, h);
            }
        }
    }
    h.opt_str(n.text.as_deref());
    hash_map(&n.extra, h);
}

fn hash_opt_map(m: Option<&Map<String, Value>>, h: &mut Fx) {
    match m {
        None => h.add(5),
        Some(m) => hash_map(m, h),
    }
}

/// Order-insensitive: sum of independent per-entry hashes.
fn hash_map(m: &Map<String, Value>, h: &mut Fx) {
    let mut sum = 0u64;
    for (k, v) in m {
        let mut e = Fx::default();
        e.bytes(k.as_bytes());
        hash_value(v, &mut e);
        sum = sum.wrapping_add(e.0);
    }
    h.add(6 ^ ((m.len() as u64) << 8));
    h.add(sum);
}

fn hash_value(v: &Value, h: &mut Fx) {
    match v {
        Value::Null => h.add(10),
        Value::Bool(b) => h.add(11 + *b as u64),
        Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                h.add(13);
                h.add(u);
            } else if let Some(i) = n.as_i64() {
                h.add(14);
                h.add(i as u64);
            } else {
                // Float: `==` treats 0.0 and -0.0 as equal.
                let f = n.as_f64().unwrap_or(0.0);
                h.add(15);
                h.add(if f == 0.0 { 0 } else { f.to_bits() });
            }
        }
        Value::String(s) => {
            h.add(16);
            h.bytes(s.as_bytes());
        }
        Value::Array(a) => {
            h.add(17 ^ ((a.len() as u64) << 8));
            for x in a {
                hash_value(x, h);
            }
        }
        Value::Object(m) => {
            h.add(18);
            hash_map(m, h);
        }
    }
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
            // Validate against the current text *without* mutating, so an
            // out-of-range or overflowing splice leaves the node untouched.
            // Scalar offsets translate to byte offsets (scalar-safe); a scalar
            // index equal to the length maps to the byte length (end splice).
            let (start, end) = {
                let cur = node.text.as_deref().unwrap_or("");
                let scalars = cur.chars().count();
                let end_scalar = match from.checked_add(*len_del) {
                    Some(e) if *from <= scalars && e <= scalars => e,
                    _ => return Err(ApplyError { path: path.clone() }),
                };
                let start = cur.char_indices().nth(*from).map_or(cur.len(), |(b, _)| b);
                let end = cur
                    .char_indices()
                    .nth(end_scalar)
                    .map_or(cur.len(), |(b, _)| b);
                (start, end)
            };
            // A pure no-op (nothing removed, nothing inserted) must not
            // materialize a `None` text field into `Some("")` — the model
            // preserves missing-vs-empty for faithful round-tripping.
            if start != end || !insert.is_empty() {
                node.text
                    .get_or_insert_with(String::new)
                    .replace_range(start..end, insert);
            }
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
