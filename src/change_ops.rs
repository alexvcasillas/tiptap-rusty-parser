//! Utilities over [`Change`] lists: compose, compact, and position mapping.
//!
//! These complement [`diff`](crate::diff)/[`apply`](crate::apply)/[`invert`](crate::invert):
//! - [`compose`] concatenates two change lists into one apply-equivalent patch.
//! - [`compact`] coalesces redundant node-local ops (safely — structural ops act
//!   as barriers, since [`apply`] interprets child indices against the live tree).
//! - [`map_path`] carries an index-path through a change list (the basis for
//!   mapping a selection or decoration across an edit).
//!
//! Operational-transform style *rebasing* of concurrent edits is intentionally
//! out of scope: it needs a bidirectional position map with insertion tie-breaks
//! and stable node identity that the value/live-index [`Change`] model doesn't
//! provide.

use crate::diff::Change;
use std::collections::HashMap;

/// A change list apply-equivalent to running `a` then `b`.
///
/// Because [`apply`](crate::apply) folds changes in order against the live tree,
/// `b`'s paths/indices already assume `a` has run — so plain concatenation is
/// the semantic identity. `compose` returns that concatenation in [`compact`]ed
/// form (redundant node-local ops coalesced).
///
/// ```
/// use tiptap_rusty_parser::{apply, compose, Node};
/// let base = Node::element("doc").with_child(Node::text("x"));
/// let a = vec![tiptap_rusty_parser::Change::SetText { path: vec![0], text: Some("y".into()) }];
/// let b = vec![tiptap_rusty_parser::Change::SetText { path: vec![0], text: Some("z".into()) }];
/// let composed = compose(&a, &b);
/// assert_eq!(composed.len(), 1); // the two SetTexts coalesced to the last
/// let mut t = base.clone();
/// apply(&mut t, &composed).unwrap();
/// assert_eq!(t.text_content(), "z");
/// ```
pub fn compose(a: &[Change], b: &[Change]) -> Vec<Change> {
    let mut all = Vec::with_capacity(a.len() + b.len());
    all.extend_from_slice(a);
    all.extend_from_slice(b);
    compact(&all)
}

/// Identifies a node-local *field* write, so repeated writes to the same field
/// can be coalesced (last-wins). Structural ops have no key.
#[derive(PartialEq, Eq, Hash)]
enum FieldKey {
    Text(Vec<usize>),
    Marks(Vec<usize>),
    Attr(Vec<usize>, String),
    Extra(Vec<usize>, String),
}

fn field_key(change: &Change) -> Option<FieldKey> {
    match change {
        Change::SetText { path, .. } => Some(FieldKey::Text(path.clone())),
        Change::SetMarks { path, .. } => Some(FieldKey::Marks(path.clone())),
        Change::SetAttr { path, key, .. } | Change::RemoveAttr { path, key } => {
            Some(FieldKey::Attr(path.clone(), key.clone()))
        }
        Change::SetExtra { path, key, .. } | Change::RemoveExtra { path, key } => {
            Some(FieldKey::Extra(path.clone(), key.clone()))
        }
        // Structural: Insert / Remove / Replace / Move.
        _ => None,
    }
}

/// Coalesce redundant changes while preserving the [`apply`](crate::apply)
/// result. Only **safe** reductions are performed:
///
/// - repeated writes to the same node field (`SetText`/`SetMarks`, or
///   `SetAttr`/`RemoveAttr` / `SetExtra`/`RemoveExtra` on the same key) collapse
///   to the **last** one (each write fully determines that field's final value);
/// - an `Insert` immediately followed by a `Remove` of that same just-inserted
///   child cancels out.
///
/// Structural ops (`Insert`/`Remove`/`Move`/`Replace`) act as **barriers**:
/// because [`apply`] resolves indices against the live tree, field ops are never
/// coalesced across them. `compact(c)` is never longer than `c`.
///
/// Precondition: this preserves the `apply` result for change lists that apply
/// **cleanly** to their intended base (as produced by [`diff`](crate::diff) /
/// [`Transform`](crate::Transform)). For a deliberately invalid list it can
/// differ — e.g. an out-of-bounds `Insert` immediately followed by its matching
/// `Remove` cancels here, whereas applying the original would error; validity
/// can't be checked from a change list alone.
pub fn compact(changes: &[Change]) -> Vec<Change> {
    let mut out: Vec<Change> = Vec::with_capacity(changes.len());
    // (field key) -> index in `out` of the last kept write, within the current
    // barrier segment. Cleared whenever a structural op is emitted.
    let mut last: HashMap<FieldKey, usize> = HashMap::new();

    for change in changes {
        match field_key(change) {
            Some(key) => {
                if let Some(&idx) = last.get(&key) {
                    out[idx] = change.clone(); // last-wins, kept in place (segment-local)
                } else {
                    last.insert(key, out.len());
                    out.push(change.clone());
                }
            }
            None => {
                // Adjacent Insert+Remove of the same child cancels out.
                if let Change::Remove { path, index } = change {
                    if let Some(Change::Insert {
                        path: ip,
                        index: ii,
                        ..
                    }) = out.last()
                    {
                        if ip == path && ii == index {
                            out.pop();
                            last.clear();
                            continue;
                        }
                    }
                }
                out.push(change.clone());
                last.clear(); // structural barrier
            }
        }
    }
    out
}

/// If `prefix` is a strict ancestor level of `q`, return that level's depth
/// (so the structural op at parent `prefix` acts on `q[depth]`).
fn affected_level(q: &[usize], prefix: &[usize]) -> Option<usize> {
    if prefix.len() < q.len() && q[..prefix.len()] == *prefix {
        Some(prefix.len())
    } else {
        None
    }
}

/// Carry an index-path through a change list: where does the node originally at
/// `path` end up after applying `changes`? Returns `None` if it was removed or
/// replaced away.
///
/// Field changes never move nodes. Structural changes shift the index components
/// of `path` at the level they act on (an `Insert` at or before the index shifts
/// it right; a `Remove`/`Move` shifts accordingly; removing or replacing a node
/// on the path drops it). This mirrors [`apply`](crate::apply)'s live-index
/// semantics, so the mapped path addresses the same node in the applied tree.
///
/// ```
/// use tiptap_rusty_parser::{map_path, Change};
/// // A sibling inserted before index 1 shifts it to 2.
/// let changes = vec![Change::Insert { path: vec![], index: 0, node: Default::default() }];
/// assert_eq!(map_path(&[1, 0], &changes), Some(vec![2, 0]));
/// // Removing the node on the path drops it.
/// let removed = vec![Change::Remove { path: vec![], index: 1 }];
/// assert_eq!(map_path(&[1, 0], &removed), None);
/// ```
pub fn map_path(path: &[usize], changes: &[Change]) -> Option<Vec<usize>> {
    let mut q = path.to_vec();
    for change in changes {
        match change {
            Change::Insert { path: p, index, .. } => {
                if let Some(d) = affected_level(&q, p) {
                    if *index <= q[d] {
                        q[d] += 1;
                    }
                }
            }
            Change::Remove { path: p, index } => {
                if let Some(d) = affected_level(&q, p) {
                    if *index < q[d] {
                        q[d] -= 1;
                    } else if *index == q[d] {
                        return None; // the node on the path was removed
                    }
                }
            }
            Change::Move { path: p, from, to } => {
                if let Some(d) = affected_level(&q, p) {
                    let k = q[d];
                    if *from == k {
                        // The node on the path is the one being moved.
                        q[d] = *to;
                    } else {
                        // remove(from) then insert(to), in post-removal coords.
                        let after_remove = if *from < k { k - 1 } else { k };
                        q[d] = if *to <= after_remove {
                            after_remove + 1
                        } else {
                            after_remove
                        };
                    }
                }
            }
            // Replacing a node at or above `q` discards `q`'s subtree.
            Change::Replace { path: p, .. } if p.len() <= q.len() && q[..p.len()] == *p => {
                return None;
            }
            // Field ops (and replaces elsewhere) never relocate a node.
            _ => {}
        }
    }
    Some(q)
}
