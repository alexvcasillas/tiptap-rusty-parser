//! Character-level (inline) text diffing, and the granularity knob for
//! [`Node::diff_with`](crate::Node::diff_with).
//!
//! The structural [`diff`](crate::diff) replaces a changed text node wholesale
//! ([`Change::SetText`](crate::Change::SetText)). For AI/review flows you often
//! want *character-level* edits instead — [`Change::SpliceText`](crate::Change::SpliceText)
//! islands — so suggestions read as "inserted X / deleted Y" rather than
//! "replaced the whole paragraph". The islands are minimal for typical edits
//! (a single delete+insert fallback bounds cost on very large changed runs —
//! see [`diff_text`]). [`diff_text`] computes the segments; [`DiffGranularity`]
//! selects block vs inline vs a smart blend.

use crate::diff::Change;
use serde::{Deserialize, Serialize};

/// Cap on the LCS table size (`|a| * |b|`); past it, a changed run is emitted as
/// a single delete+insert instead of a minimal character diff (bounds cost).
const LCS_GUARD: usize = 1 << 20;

/// One segment of a character-level text diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SegKind {
    /// Text common to both sides.
    Keep,
    /// Text present only in the new string.
    Insert,
    /// Text present only in the old string.
    Delete,
}

/// A run of characters classified by a text diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSegment {
    /// Whether this run was kept, inserted, or deleted.
    pub kind: SegKind,
    /// The run's text.
    pub text: String,
}

/// How [`Node::diff_with`](crate::Node::diff_with) treats changed text nodes.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DiffGranularity {
    /// Whole-node text replacement (`SetText`) — the default [`diff`](crate::diff) behavior.
    #[default]
    Block,
    /// Character-level `SpliceText` islands.
    Inline,
    /// Inline, but fall back to a whole `SetText` once the changed fraction
    /// exceeds `replace_threshold` (0.0–1.0).
    Smart {
        /// Changed-scalar fraction above which a whole replacement is preferred.
        replace_threshold: f32,
    },
}

/// Options for [`Node::diff_with`](crate::Node::diff_with).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DiffOptions {
    /// Text-node diff granularity.
    pub text: DiffGranularity,
}

/// Character-level (Unicode-scalar) diff of two strings.
///
/// Trims the common prefix/suffix, then runs an LCS over the changed middle to
/// produce minimal segments. For very large changed runs (when the middle's
/// `|a| * |b|` exceeds [`LCS_GUARD`]) it falls back to a single delete + insert
/// to bound cost — correct, but not minimal for those pathological inputs.
///
/// ```
/// use tiptap_rusty_parser::{diff_text, SegKind};
/// let segs = diff_text("kitten", "sitting");
/// // The common run "itt" survives as a Keep segment.
/// assert!(segs.iter().any(|s| s.kind == SegKind::Keep && s.text.contains("itt")));
/// ```
pub fn diff_text(a: &str, b: &str) -> Vec<TextSegment> {
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let mut raw: Vec<(SegKind, String)> = Vec::new();

    // Common prefix / suffix (linear; handles the common "small middle edit").
    let mut p = 0;
    while p < ac.len() && p < bc.len() && ac[p] == bc[p] {
        p += 1;
    }
    let mut ea = ac.len();
    let mut eb = bc.len();
    while ea > p && eb > p && ac[ea - 1] == bc[eb - 1] {
        ea -= 1;
        eb -= 1;
    }

    if p > 0 {
        raw.push((SegKind::Keep, ac[..p].iter().collect()));
    }
    let (ma, mb) = (&ac[p..ea], &bc[p..eb]);
    if ma.is_empty() && mb.is_empty() {
        // unchanged middle
    } else if ma.is_empty() {
        raw.push((SegKind::Insert, mb.iter().collect()));
    } else if mb.is_empty() {
        raw.push((SegKind::Delete, ma.iter().collect()));
    } else if ma.len().saturating_mul(mb.len()) > LCS_GUARD {
        raw.push((SegKind::Delete, ma.iter().collect()));
        raw.push((SegKind::Insert, mb.iter().collect()));
    } else {
        lcs_segments(ma, mb, &mut raw);
    }
    if ea < ac.len() {
        raw.push((SegKind::Keep, ac[ea..].iter().collect()));
    }

    coalesce(raw)
}

/// LCS-backtrack of two char slices, pushing per-char `(kind, char-as-string)`
/// runs (merged by [`coalesce`]).
fn lcs_segments(a: &[char], b: &[char], out: &mut Vec<(SegKind, String)>) {
    let (m, n) = (a.len(), b.len());
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
    let (mut i, mut j) = (0usize, 0usize);
    let push = |out: &mut Vec<(SegKind, String)>, k: SegKind, c: char| {
        out.push((k, c.to_string()));
    };
    while i < m && j < n {
        if a[i] == b[j] {
            push(out, SegKind::Keep, a[i]);
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            push(out, SegKind::Delete, a[i]);
            i += 1;
        } else {
            push(out, SegKind::Insert, b[j]);
            j += 1;
        }
    }
    while i < m {
        push(out, SegKind::Delete, a[i]);
        i += 1;
    }
    while j < n {
        push(out, SegKind::Insert, b[j]);
        j += 1;
    }
}

/// Merge adjacent same-kind runs, dropping empties.
fn coalesce(raw: Vec<(SegKind, String)>) -> Vec<TextSegment> {
    let mut out: Vec<TextSegment> = Vec::new();
    for (kind, text) in raw {
        if text.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.kind == kind => last.text.push_str(&text),
            _ => out.push(TextSegment { kind, text }),
        }
    }
    out
}

/// Total scalars touched (inserted + deleted) by a segment list.
pub(crate) fn changed_scalars(segs: &[TextSegment]) -> usize {
    segs.iter()
        .filter(|s| s.kind != SegKind::Keep)
        .map(|s| s.text.chars().count())
        .sum()
}

/// Convert a segment list (a→b) into minimal `SpliceText` changes on `path`.
/// Each maximal non-`Keep` island between kept runs becomes one splice.
///
/// Splices carry **original-string** scalar offsets, so they're emitted
/// **highest-offset first**: applied in that order, each edit leaves every
/// lower offset untouched, so the un-rebased offsets stay valid.
pub(crate) fn splices_from_segments(path: &[usize], segs: &[TextSegment], out: &mut Vec<Change>) {
    let mut islands: Vec<Change> = Vec::new();
    let mut cursor = 0usize; // scalar offset in the original (old) string
    let mut i = 0;
    while i < segs.len() {
        if segs[i].kind == SegKind::Keep {
            cursor += segs[i].text.chars().count();
            i += 1;
            continue;
        }
        let from = cursor;
        let mut len_del = 0usize;
        let mut insert = String::new();
        while i < segs.len() && segs[i].kind != SegKind::Keep {
            match segs[i].kind {
                SegKind::Delete => {
                    let n = segs[i].text.chars().count();
                    len_del += n;
                    cursor += n;
                }
                SegKind::Insert => insert.push_str(&segs[i].text),
                SegKind::Keep => unreachable!(),
            }
            i += 1;
        }
        islands.push(Change::SpliceText {
            path: path.to_vec(),
            from,
            len_del,
            insert,
        });
    }
    out.extend(islands.into_iter().rev());
}
