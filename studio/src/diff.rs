//! Diff mode: a second document B, the `Document::diff` change list from A to
//! B, both trees tinted by change kind, and a read-only before/after inspector.
//!
//! Change paths in a diff list are *live* (each assumes the earlier changes
//! ran), so every entry is translated once per recompute: `unmap_path` back to
//! A, `map_path` forward to B. Take B cherry-picks one change into A with
//! `rebase_change` + `Transform::apply_change` through `DocumentState::edit`,
//! so it is undoable; the diff then recomputes on observe.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::Value;
use tiptap_rusty_parser::{map_path, rebase_change, unmap_path, Change, Document, Node};

use crate::find::{FindState, MatchSet};
use crate::inspector::{chip, section};
use crate::state::{fmt_path, node_summary, node_type, short_value, text_preview, DocumentState};
use crate::theme::Tokens;
use crate::tree::{flatten, pane_header, render_row, Deco, Row, RowFn};

gpui_kit::actions!(diff, [PrevChange, NextChange, TakeB, Dismiss]);

pub const CONTEXT: &str = "Diff";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", PrevChange, Some(CONTEXT)),
        KeyBinding::new("down", NextChange, Some(CONTEXT)),
        KeyBinding::new("enter", TakeB, Some(CONTEXT)),
        KeyBinding::new("backspace", Dismiss, Some(CONTEXT)),
    ]);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tint {
    Changed,
    Added,
    Removed,
    Moved,
}

impl Tint {
    pub fn color(self) -> Hsla {
        match self {
            Tint::Changed => Tokens::unknown(),
            Tint::Added => Tokens::added(),
            Tint::Removed => Tokens::removed(),
            Tint::Moved => Tokens::moved(),
        }
    }
}

/// One change, pre-digested for the list and inspector.
pub struct Entry {
    pub tag: &'static str,
    pub tint: Tint,
    pub label: SharedString,
    pub from: Option<SharedString>,
    pub to: Option<SharedString>,
    /// The changed node in A / B, when it exists on that side.
    pub a_path: Option<Vec<usize>>,
    pub b_path: Option<Vec<usize>>,
    /// Fallback selection when the node exists only on the other side (its parent).
    pub a_focus: Option<Vec<usize>>,
    pub b_focus: Option<Vec<usize>>,
    /// Document-order sort key in A coordinates: (A path or insertion slot,
    /// rank: remove 0 < insert 1 < other 2). `None` when the change has no A
    /// anchor; it then sorts right after its raw predecessor.
    order_key: Option<(Vec<usize>, u8)>,
}

impl Entry {
    /// Identity for a Dismiss decision. Anchored on B (read-only, so its paths
    /// never shift) plus both values: edits elsewhere in A keep the decision,
    /// while a different value on either side is a new change to review.
    fn key(&self) -> String {
        format!(
            "{}|{:?}|{:?}|{:?}",
            self.tag,
            self.b_target(),
            self.from,
            self.to
        )
    }

    fn a_target(&self) -> Option<&Vec<usize>> {
        self.a_path.as_ref().or(self.a_focus.as_ref())
    }
    fn b_target(&self) -> Option<&Vec<usize>> {
        self.b_path.as_ref().or(self.b_focus.as_ref())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    A,
    B,
}

pub type DecoRow = (Row, Deco);

/// A node picked in tree A or B (not a change row): the target of
/// "Take all B changes in this subtree". Anchored on B when the node exists
/// there (B never changes, so the anchor survives edits to A).
pub struct Pick {
    pub a: Option<Vec<usize>>,
    pub b: Option<Vec<usize>>,
    /// Open (non-dismissed) changes inside the subtree.
    pub count: usize,
}

impl Pick {
    fn contains(&self, e: &Entry) -> bool {
        match (&self.b, &self.a) {
            (Some(pb), _) => e.b_target().is_some_and(|t| t.starts_with(pb)),
            (None, Some(pa)) => e.a_target().is_some_and(|t| t.starts_with(pa)),
            (None, None) => false,
        }
    }
}

#[derive(Default)]
struct Side {
    tint: HashMap<Vec<usize>, Tint>,
    pip: HashMap<Vec<usize>, Tint>,
    expanded: HashSet<Vec<usize>>,
    selection: Option<Vec<usize>>,
    rows: Rc<Vec<DecoRow>>,
}

impl Side {
    fn reset_tints(&mut self) {
        self.tint.clear();
        self.pip.clear();
    }

    fn mark(&mut self, path: Option<&Vec<usize>>, focus: Option<&Vec<usize>>, tint: Tint) {
        if let Some(p) = path {
            let t = self.tint.entry(p.clone()).or_insert(tint);
            if *t != tint {
                *t = Tint::Changed;
            }
            self.pip_ancestors(p, p.len(), tint);
        } else if let Some(p) = focus {
            self.pip_ancestors(p, p.len() + 1, tint);
        }
    }

    /// Faint pip on `path[..d]` for `d < end`; mixed kinds read as changed.
    fn pip_ancestors(&mut self, path: &[usize], end: usize, tint: Tint) {
        for d in 0..end {
            let t = self.pip.entry(path[..d].to_vec()).or_insert(tint);
            if *t != tint {
                *t = Tint::Changed;
            }
        }
    }

    /// Expand every ancestor of `path` (and `path` itself when `inclusive`).
    fn expand_to(&mut self, path: &[usize], inclusive: bool) {
        let end = path.len() + usize::from(inclusive);
        for d in 0..end {
            self.expanded.insert(path[..d].to_vec());
        }
    }

    fn deco(&self, path: &[usize], pane: Pane) -> Deco {
        if let Some(&t) = self.tint.get(path) {
            Deco {
                tint: Some(t.color()),
                pip: None,
                strike: pane == Pane::A && t == Tint::Removed,
                added: pane == Pane::B && t == Tint::Added,
            }
        } else if let Some(&t) = self.pip.get(path) {
            Deco {
                pip: Some(t.color()),
                ..Deco::NONE
            }
        } else {
            Deco::NONE
        }
    }

    fn rebuild(&mut self, root: &Node, pane: Pane, only_changed: bool) {
        let rows = flatten(root, &|p| {
            self.expanded.contains(p) || (only_changed && self.pip.contains_key(p))
        });
        let rows = rows
            .into_iter()
            .map(|r| {
                let d = self.deco(&r.path, pane);
                (r, d)
            })
            .filter(|(_, d)| !only_changed || !d.is_none())
            .collect();
        self.rows = Rc::new(rows);
    }
}

pub struct DiffState {
    a: Entity<DocumentState>,
    b: Option<Document>,
    b_file: Option<PathBuf>,
    /// Display name of a B that has no file (opened from a Document Server).
    b_origin: Option<SharedString>,
    /// Path given on the command line; fallback when the file prompt fails.
    pub cli_b: Option<PathBuf>,
    pub error: Option<SharedString>,
    changes: Vec<Change>,
    entries: Rc<Vec<Entry>>,
    /// Dismiss decisions ("leave A as it is"), by `Entry::key`. Cleared when
    /// B changes.
    kept: HashSet<String>,
    /// `kept_flags[i]`: entry `i` is dismissed.
    kept_flags: Rc<Vec<bool>>,
    /// Every entry index in document order (see `Entry::order_key`). The
    /// raw `changes` order is kept: `rebase_change` depends on it.
    order: Vec<usize>,
    /// Entry indices the change list shows, in document order.
    visible: Rc<Vec<usize>>,
    /// Show dismissed changes in the list (to restore one).
    pub show_dismissed: bool,
    pub selected: Option<usize>,
    /// Node picked in a tree, for the subtree take.
    pub pick: Option<Pick>,
    /// Scrolling one tree scrolls the other to the corresponding row.
    pub sync_scroll: bool,
    json: SharedString,
    side_a: Side,
    side_b: Side,
    pub only_changed: bool,
    seen_revision: Option<u64>,
    /// (+ inserted, − removed, ~ modified/moved)
    pub counts: (usize, usize, usize),
    /// A's find state (toolbar query). Its matches decorate tree A; the same
    /// query runs against B for tree B.
    find: Entity<FindState>,
    find_a: MatchSet,
    find_b: MatchSet,
    /// Query generation `find_b` was computed for.
    find_gen: u64,
    /// A find query is active (pane headers show match counts).
    find_active: bool,
    /// A's selection as last seen, to follow find next/prev (which selects
    /// in A's `DocumentState`) without echoing our own pushes.
    seen_a_selection: Option<Vec<usize>>,
    _subs: Vec<Subscription>,
}

impl DiffState {
    pub fn new(a: Entity<DocumentState>, b: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&a, |this, a, cx| {
            let a = a.read(cx);
            let moved = a.selection != this.seen_a_selection;
            if moved {
                this.seen_a_selection = a.selection.clone();
            }
            if this.seen_revision != Some(a.revision) {
                this.recompute(a, false);
            } else if moved && this.b.is_some() {
                // Selection-only: find next/prev selected a match in A's
                // `DocumentState`; tree A follows. Other moves (Inspect
                // arrow keys) are ignored: a follow re-flattens both trees.
                if let Some(p) = a.selection.clone() {
                    if this.find_a.set.contains(&p) && this.side_a.selection.as_ref() != Some(&p) {
                        this.select_path_in(Pane::A, &p, a);
                    }
                }
            }
            cx.notify();
        });
        let find = crate::find::find_state(&a, cx);
        let find_sub = cx.observe(&find, |this, find, cx| {
            let f = find.read(cx);
            this.find_a = f.match_set();
            if f.query_generation() != this.find_gen {
                this.refind_b(cx);
            }
            cx.notify();
        });
        let mut this = Self {
            a: a.clone(),
            b: None,
            b_file: None,
            b_origin: None,
            cli_b: b.clone(),
            error: None,
            changes: Vec::new(),
            entries: Rc::new(Vec::new()),
            kept: HashSet::new(),
            kept_flags: Rc::new(Vec::new()),
            order: Vec::new(),
            visible: Rc::new(Vec::new()),
            show_dismissed: false,
            selected: None,
            pick: None,
            sync_scroll: true,
            json: SharedString::default(),
            side_a: Side::default(),
            side_b: Side::default(),
            only_changed: false,
            seen_revision: None,
            counts: (0, 0, 0),
            find,
            find_a: MatchSet::default(),
            find_b: MatchSet::default(),
            find_gen: 0,
            find_active: false,
            seen_a_selection: a.read(cx).selection.clone(),
            _subs: vec![sub, find_sub],
        };
        if let Some(p) = b {
            this.load_b_inner(&p, a.read(cx));
        }
        this.refind_b(cx);
        this
    }

    pub fn has_b(&self) -> bool {
        self.b.is_some()
    }

    /// Root of document B, when loaded (schema panel validates it).
    pub fn b_root(&self) -> Option<&Node> {
        self.b.as_ref().map(Document::root)
    }

    pub fn b_name(&self) -> SharedString {
        if let (None, Some(o)) = (&self.b_file, &self.b_origin) {
            return o.clone();
        }
        self.b_file
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string().into())
            .unwrap_or_else(|| "—".into())
    }

    /// Changes still to resolve (dismissed ones excluded).
    pub fn total(&self) -> usize {
        self.changes.len() - self.kept_count()
    }

    pub fn kept_count(&self) -> usize {
        self.kept_flags.iter().filter(|k| **k).count()
    }

    fn is_kept(&self, i: usize) -> bool {
        self.kept_flags.get(i).copied().unwrap_or(false)
    }

    pub fn load_b(&mut self, path: &Path, cx: &mut Context<Self>) {
        let a = self.a.clone();
        self.load_b_inner(path, a.read(cx));
        self.refind_b(cx);
        cx.notify();
    }

    /// Run the find query against B. On B or query change only, never per frame.
    fn refind_b(&mut self, cx: &App) {
        let f = self.find.read(cx);
        self.find_gen = f.query_generation();
        self.find_active = f.query().is_some();
        self.find_b = match (f.query(), self.b.as_ref()) {
            (Some(q), Some(b)) => {
                let _p = crate::profile::span("find B");
                MatchSet::from_paths(&q.find_paths(b.root()))
            }
            _ => MatchSet::default(),
        };
    }

    /// Find matches for one tree.
    pub fn matches(&self, pane: Pane) -> &MatchSet {
        match pane {
            Pane::A => &self.find_a,
            Pane::B => &self.find_b,
        }
    }

    /// Select `path` in A's `DocumentState` too (find counter, next/prev
    /// from here, Inspect selection), without echoing it back to us.
    fn push_a_selection(&mut self, path: Option<Vec<usize>>, cx: &mut Context<Self>) {
        let Some(p) = path else { return };
        self.seen_a_selection = Some(p.clone());
        self.a.update(cx, |s, cx| s.select(Some(p), cx));
    }

    /// Load B from an in-memory document (no file), e.g. a server version.
    pub fn load_b_document(&mut self, doc: Document, origin: SharedString, cx: &mut Context<Self>) {
        let a = self.a.clone();
        self.b = Some(doc);
        self.b_file = None;
        self.b_origin = Some(origin);
        self.error = None;
        self.selected = None;
        self.kept.clear();
        self.side_a = Side::default();
        self.side_b = Side::default();
        self.side_a.expanded.insert(Vec::new());
        self.side_b.expanded.insert(Vec::new());
        self.recompute(a.read(cx), true);
        self.refind_b(cx);
        cx.notify();
    }

    fn load_b_inner(&mut self, path: &Path, a: &DocumentState) {
        let parsed = std::fs::read_to_string(path)
            .map_err(anyhow::Error::from)
            .and_then(|s| Document::from_json_str(&s).map_err(anyhow::Error::from));
        match parsed {
            Ok(doc) => {
                self.b = Some(doc);
                self.b_file = Some(path.to_path_buf());
                self.b_origin = None;
                self.error = None;
                self.reset_view();
                self.recompute(a, true);
            }
            Err(e) => {
                eprintln!("tiptap-studio: open B {}: {e}", path.display());
                self.error = Some(format!("{}: {e}", path.display()).into());
            }
        }
    }

    /// Fresh view state for a new A/B pair: no selection, no decisions.
    fn reset_view(&mut self) {
        self.selected = None;
        self.pick = None;
        self.kept.clear();
        self.side_a = Side::default();
        self.side_b = Side::default();
        self.side_a.expanded.insert(Vec::new());
        self.side_b.expanded.insert(Vec::new());
    }

    /// Re-diff A against B and rebuild every derived structure. Runs on document
    /// change only (revision guard), never per frame.
    fn recompute(&mut self, a: &DocumentState, expand: bool) {
        let _p = crate::profile::span("diff recompute");
        self.seen_revision = Some(a.revision);
        let Some(b) = self.b.as_ref() else {
            return;
        };
        self.changes = a.doc.diff(b);
        let (ra, rb) = (a.root(), b.root());
        let entries: Vec<Entry> = (0..self.changes.len())
            .map(|i| describe(i, &self.changes, ra, rb))
            .collect();
        self.order = document_order(&entries);
        self.entries = Rc::new(entries);
        if let Some(p) = &mut self.pick {
            if let Some(pb) = &p.b {
                p.a = unmap_path(pb, &self.changes);
            }
            if p.b.is_none() && p.a.as_deref().and_then(|x| a.root().node_at(x)).is_none() {
                self.pick = None;
            }
        }
        self.retint(a, expand);
    }

    /// Kept flags, tints and counts from the current entries; no re-diff.
    fn retint(&mut self, a: &DocumentState, expand: bool) {
        let entries = self.entries.clone();
        self.kept_flags = Rc::new(
            entries
                .iter()
                .map(|e| self.kept.contains(&e.key()))
                .collect(),
        );
        self.side_a.reset_tints();
        self.side_b.reset_tints();
        let mut counts = (0, 0, 0);
        for (e, &kept) in entries.iter().zip(self.kept_flags.iter()) {
            if kept {
                continue;
            }
            match e.tint {
                Tint::Added => counts.0 += 1,
                Tint::Removed => counts.1 += 1,
                _ => counts.2 += 1,
            }
            self.side_a
                .mark(e.a_path.as_ref(), e.a_focus.as_ref(), e.tint);
            self.side_b
                .mark(e.b_path.as_ref(), e.b_focus.as_ref(), e.tint);
            if expand {
                if let Some(p) = e.a_target() {
                    self.side_a.expand_to(p, e.a_path.is_none());
                }
                if let Some(p) = e.b_target() {
                    self.side_b.expand_to(p, e.b_path.is_none());
                }
            }
        }
        self.counts = counts;
        let show = self.show_dismissed;
        self.visible = Rc::new(
            self.order
                .iter()
                .copied()
                .filter(|&i| show || !self.kept_flags[i])
                .collect(),
        );
        // A selection that is gone or hidden moves to the next visible change
        // (in display order).
        if let Some(i) = self.selected {
            let i = i.min(entries.len().saturating_sub(1));
            let at = self.order.iter().position(|&j| j == i).unwrap_or(0);
            self.selected = self.order[at..]
                .iter()
                .copied()
                .find(|&j| show || !self.kept_flags[j])
                .or_else(|| self.visible.last().copied());
        }
        if let Some(p) = &mut self.pick {
            p.count = entries
                .iter()
                .zip(self.kept_flags.iter())
                .filter(|(e, &k)| !k && p.contains(e))
                .count();
        }
        self.sync_selection(a);
    }

    /// Position of entry `i` in the visible list.
    pub fn visible_pos(&self, i: usize) -> Option<usize> {
        self.visible.iter().position(|&j| j == i)
    }

    pub fn toggle_show_dismissed(&mut self, cx: &mut Context<Self>) {
        self.show_dismissed = !self.show_dismissed;
        self.with_a(cx, |this, a| this.retint(a, false));
    }

    /// Point both trees at the selected change and refresh rows + JSON.
    fn sync_selection(&mut self, a: &DocumentState) {
        self.json = SharedString::default();
        if let Some(e) = self.selected.and_then(|i| self.entries.get(i)) {
            let (pa, pb) = (e.a_target().cloned(), e.b_target().cloned());
            if let Some(p) = &pa {
                self.side_a.expand_to(p, false);
            }
            if let Some(p) = &pb {
                self.side_b.expand_to(p, false);
            }
            self.side_a.selection = pa;
            self.side_b.selection = pb;
            self.json = self.change_json(self.selected);
        }
        self.rebuild_rows(a);
    }

    /// Change `i` as Take B would apply it: rebased onto A's own paths.
    fn change_json(&self, i: Option<usize>) -> SharedString {
        i.and_then(|i| rebase_change(self.changes.get(i)?, &self.changes[..i]))
            .and_then(|c| serde_json::to_string_pretty(&c).ok())
            .unwrap_or_default()
            .into()
    }

    fn rebuild_rows(&mut self, a: &DocumentState) {
        let Some(b) = self.b.as_ref() else {
            return;
        };
        self.side_a.rebuild(a.root(), Pane::A, self.only_changed);
        self.side_b.rebuild(b.root(), Pane::B, self.only_changed);
    }

    fn with_a(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Self, &DocumentState)) {
        let a = self.a.clone();
        f(self, a.read(cx));
        cx.notify();
    }

    pub fn select_change(&mut self, i: usize, cx: &mut Context<Self>) {
        if i >= self.entries.len() {
            return;
        }
        self.selected = Some(i);
        self.pick = None;
        self.with_a(cx, |this, a| this.sync_selection(a));
        self.push_a_selection(self.side_a.selection.clone(), cx);
    }

    pub fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.visible.len();
        if n == 0 {
            return;
        }
        let pos = match self.selected.and_then(|i| self.visible_pos(i)) {
            Some(p) => (p as isize + delta).clamp(0, n as isize - 1) as usize,
            None if delta < 0 => n - 1,
            None => 0,
        };
        let next = self.visible[pos];
        self.select_change(next, cx);
    }

    /// Click in a tree: select there, map the path to the other tree and pick
    /// the change that touches it (if any).
    pub fn select_path(&mut self, pane: Pane, path: &[usize], cx: &mut Context<Self>) {
        let a = self.a.clone();
        self.select_path_in(pane, path, a.read(cx));
        cx.notify();
        self.push_a_selection(self.side_a.selection.clone(), cx);
    }

    fn select_path_in(&mut self, pane: Pane, path: &[usize], a: &DocumentState) {
        let path = path.to_vec();
        // (A path, B path)
        let (here, other) = match pane {
            Pane::A => (Some(path.clone()), map_path(&path, &self.changes)),
            Pane::B => (unmap_path(&path, &self.changes), Some(path.clone())),
        };
        self.selected = self.entries.iter().position(|e| match pane {
            Pane::A => e.a_path.as_ref() == Some(&path),
            Pane::B => e.b_path.as_ref() == Some(&path),
        });
        self.json = self.change_json(self.selected);
        let kept = &self.kept_flags;
        let mut pick = Pick {
            a: here.clone(),
            b: other.clone(),
            count: 0,
        };
        pick.count = self
            .entries
            .iter()
            .enumerate()
            .filter(|(i, e)| !kept.get(*i).copied().unwrap_or(false) && pick.contains(e))
            .count();
        self.pick = Some(pick);
        let (pa, pb) = (here, other);
        if let Some(p) = &pa {
            self.side_a.expand_to(p, false);
        }
        if let Some(p) = &pb {
            self.side_b.expand_to(p, false);
        }
        self.side_a.selection = pa;
        self.side_b.selection = pb;
        self.rebuild_rows(a);
    }

    pub fn toggle(&mut self, pane: Pane, path: &[usize], cx: &mut Context<Self>) {
        let side = match pane {
            Pane::A => &mut self.side_a,
            Pane::B => &mut self.side_b,
        };
        if !side.expanded.remove(path) {
            side.expanded.insert(path.to_vec());
        }
        self.with_a(cx, |this, a| this.rebuild_rows(a));
    }

    pub fn set_only_changed(&mut self, on: bool, cx: &mut Context<Self>) {
        self.only_changed = on;
        self.with_a(cx, |this, a| this.rebuild_rows(a));
    }

    /// Apply the selected change to A as one undoable edit.
    pub fn take_b(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.selected else {
            return;
        };
        let Some(change) = self
            .changes
            .get(i)
            .and_then(|c| rebase_change(c, &self.changes[..i]))
        else {
            return;
        };
        let label = format!("Take B {}", self.entries[i].label);
        self.a.update(cx, |s, cx| {
            s.edit(label, |t| t.apply_change(change).map(|_| ()), cx);
        });
    }

    /// Dismiss: decide "leave A as it is" for the selected change (or restore
    /// a dismissed one). Dismissed changes leave the list, the counts and
    /// Apply all; selection moves to the next open change.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.selected else {
            return;
        };
        let Some(key) = self.entries.get(i).map(Entry::key) else {
            return;
        };
        let now_kept = !self.kept.remove(&key);
        if now_kept {
            self.kept.insert(key);
        }
        // Hidden mode: `retint` already moved the selection past the
        // now-hidden change. Showing dismissed ones: step to the next open one.
        self.with_a(cx, |this, a| this.retint(a, false));
        if now_kept && self.show_dismissed {
            let n = self.order.len();
            let at = self.order.iter().position(|&j| j == i).unwrap_or(0);
            if let Some(next) = (1..n)
                .map(|d| self.order[(at + d) % n])
                .find(|&j| !self.is_kept(j))
            {
                self.select_change(next, cx);
            }
        }
    }

    pub fn apply_all(&mut self, cx: &mut Context<Self>) {
        if self.total() == 0 {
            return;
        }
        let changes = if self.kept_count() == 0 {
            self.changes.clone()
        } else {
            let Some(c) = self.unkept_changes(None, cx) else {
                return;
            };
            c
        };
        let label = format!("Apply all from {}", self.b_name());
        self.a.update(cx, |s, cx| {
            s.edit(
                label,
                |t| {
                    for c in changes {
                        t.apply_change(c)?;
                    }
                    Ok::<(), tiptap_rusty_parser::ApplyError>(())
                },
                cx,
            );
        });
    }
}

impl DiffState {
    /// Every unresolved change as one live sequence, skipping kept ones
    /// (and, with `within`, changes outside that subtree).
    /// Picks the first unkept change, applies it to a scratch copy of A,
    /// re-diffs, repeats: each pick is rebased against the changes before it
    /// in its own diff, so the result replays exactly onto A. Costs one diff
    /// per applied change; only used when something is kept or filtered.
    fn unkept_changes(&self, within: Option<&Pick>, cx: &App) -> Option<Vec<Change>> {
        let b = self.b.as_ref()?.root();
        let mut work = self.a.read(cx).root().clone();
        let mut out = Vec::new();
        // Without a B anchor the subtree is A-only (removed in B): its A path
        // shifts as changes apply, so only the first match is safe to take.
        let once = within.is_some_and(|p| p.b.is_none());
        for _ in 0..10_000 {
            let changes = work.diff(b);
            let pick = (0..changes.len()).find(|&i| {
                let e = describe(i, &changes, &work, b);
                !self.kept.contains(&e.key()) && within.is_none_or(|p| p.contains(&e))
            });
            let Some(i) = pick else { break };
            let c = rebase_change(&changes[i], &changes[..i])?;
            work.apply(std::slice::from_ref(&c)).ok()?;
            out.push(c);
            if once {
                break;
            }
        }
        Some(out)
    }

    /// Apply every open change inside the picked subtree as one undo step.
    pub fn take_subtree(&mut self, cx: &mut Context<Self>) {
        let Some(pick) = self.pick.as_ref().filter(|p| p.count > 0) else {
            return;
        };
        let Some(changes) = self.unkept_changes(Some(pick), cx) else {
            return;
        };
        if changes.is_empty() {
            return;
        }
        let at = pick
            .a
            .as_ref()
            .or(pick.b.as_ref())
            .map(|p| fmt_path(p))
            .unwrap_or_default();
        let label = format!("Take B subtree {at} ({})", changes.len());
        // Inspector then shows the picked node, not an unrelated change.
        self.selected = None;
        self.a.update(cx, |s, cx| {
            s.edit(
                label,
                |t| {
                    for c in changes {
                        t.apply_change(c)?;
                    }
                    Ok::<(), tiptap_rusty_parser::ApplyError>(())
                },
                cx,
            );
        });
    }

    pub fn set_sync_scroll(&mut self, on: bool, cx: &mut Context<Self>) {
        self.sync_scroll = on;
        cx.notify();
    }

    /// Swap A and B: B becomes the editable document, A the read-only source.
    /// Undo history and Dismiss decisions reset (they describe the old pair).
    pub fn swap(&mut self, cx: &mut Context<Self>) {
        let Some(b_doc) = self.b.take() else {
            return;
        };
        let b_file = self.b_file.take();
        let a = self.a.clone();
        let (old_doc, old_file) = a.update(cx, |s, cx| {
            let old_doc = std::mem::replace(&mut s.doc, Document::new(Node::default()));
            let old_file = s.file.take();
            // Same path as Open: fresh undo stack, view reset, revision bump.
            s.replace_doc(b_doc, b_file, false, cx);
            s.last_change = Some("Swapped A and B".into());
            (old_doc, old_file)
        });
        self.b = Some(old_doc);
        self.b_file = old_file;
        self.reset_view();
        self.with_a(cx, |this, a| this.recompute(a, true));
        self.refind_b(cx);
    }
}

pub const SWAP_TOOLTIP: &str =
    "Swap A and B: B becomes the editable A. Resets undo history and dismissed changes";

/// Toolbar `Swap` click handler.
pub fn swap_handler(diff: &Entity<DiffState>) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
    let diff = diff.clone();
    move |_, window, cx| swap(diff.clone(), window, cx)
}

/// Toolbar `Sync scroll` checkbox handler.
pub fn sync_scroll_handler(diff: &Entity<DiffState>) -> impl Fn(&bool, &mut Window, &mut App) {
    let diff = diff.clone();
    move |on, _, cx| diff.update(cx, |d, cx| d.set_sync_scroll(*on, cx))
}

/// Swap button: asks first when A has unsaved edits (Save / Swap anyway / Cancel).
pub fn swap(diff: Entity<DiffState>, window: &mut Window, cx: &mut App) {
    let a = diff.read(cx).a.clone();
    if !diff.read(cx).has_b() {
        return;
    }
    if !a.read(cx).dirty {
        diff.update(cx, |d, cx| d.swap(cx));
        return;
    }
    let rx = window.prompt(
        PromptLevel::Warning,
        "A has unsaved changes",
        Some("B becomes editable; undo resets."),
        &["Save", "Swap anyway", "Cancel"],
        cx,
    );
    cx.spawn(async move |cx| match rx.await {
        Ok(0) => {
            let file = a.read_with(cx, |s, _| s.file.clone());
            let path = match file {
                Some(p) => Some(p),
                None => {
                    let dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                    let rx = cx.update(|cx| cx.prompt_for_new_path(&dir, Some("document.json")));
                    match rx.await {
                        Ok(Ok(p)) => p,
                        _ => None,
                    }
                }
            };
            let Some(path) = path else { return };
            a.update(cx, |s, cx| s.save_to(&path, cx));
            // Save failed: keep the pair as it is.
            if !a.read_with(cx, |s, _| s.dirty) {
                diff.update(cx, |d, cx| d.swap(cx));
            }
        }
        Ok(1) => diff.update(cx, |d, cx| d.swap(cx)),
        _ => {}
    })
    .detach();
}

/// Entry indices sorted into document order by `Entry::order_key`; keyless
/// entries follow their raw predecessor. Stable on ties (raw order).
fn document_order(entries: &[Entry]) -> Vec<usize> {
    let mut last: (Vec<usize>, u8) = (Vec::new(), 0);
    let mut keyed: Vec<((Vec<usize>, u8), usize)> = Vec::with_capacity(entries.len());
    for (i, e) in entries.iter().enumerate() {
        let k = match &e.order_key {
            Some(k) => {
                last = k.clone();
                k.clone()
            }
            None => (last.0.clone(), 3),
        };
        keyed.push((k, i));
    }
    keyed.sort();
    keyed.into_iter().map(|(_, i)| i).collect()
}

/// Open B through the native file prompt; on failure fall back to the CLI path.
pub fn open_b(diff: Entity<DiffState>, cx: &mut App) {
    let rx = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Open B".into()),
    });
    cx.spawn(async move |cx| {
        let picked = match rx.await {
            Ok(Ok(Some(paths))) => paths.into_iter().next(),
            Ok(Ok(None)) => return, // cancelled
            Ok(Err(e)) => {
                eprintln!("tiptap-studio: file prompt failed: {e}");
                None
            }
            Err(_) => None,
        };
        let fallback = diff.read_with(cx, |d, _| d.cli_b.clone());
        if let Some(p) = picked.or(fallback) {
            diff.update(cx, |d, cx| d.load_b(&p, cx));
        }
    })
    .detach();
}

// ---- change description ------------------------------------------------------

fn child(p: &[usize], k: usize) -> Vec<usize> {
    let mut v = p.to_vec();
    v.push(k);
    v
}

fn quote(n: Option<&Node>) -> Option<SharedString> {
    n.map(|n| format!("\"{}\"", text_preview(n, 24)).into())
}

fn marks_str(marks: Option<&[tiptap_rusty_parser::Mark]>) -> SharedString {
    match marks {
        Some(m) if !m.is_empty() => m
            .iter()
            .map(|m| m.mark_type.as_str())
            .collect::<Vec<_>>()
            .join(" ")
            .into(),
        _ => "∅".into(),
    }
}

fn val(v: Option<&Value>) -> SharedString {
    v.map(short_value).unwrap_or_else(|| "∅".into()).into()
}

/// Second line for an inserted/removed node: its text, else its summary.
fn node_preview(n: &Node) -> SharedString {
    let text = text_preview(n, 60);
    if !text.is_empty() {
        return format!("\"{text}\"").into();
    }
    let s = node_summary(n);
    if s.is_empty() {
        node_type(n).to_string().into()
    } else {
        format!("{} {s}", node_type(n)).into()
    }
}

/// `_` arms stay on purpose: new `Change` variants must not break the studio.
#[allow(unreachable_patterns)]
fn describe(i: usize, changes: &[Change], a: &Node, b: &Node) -> Entry {
    let prior = &changes[..i];
    let after = &changes[i + 1..];
    let at_a = |p: &[usize]| unmap_path(p, prior);
    let at_b = |p: &[usize]| map_path(p, after);
    use Tint::*;
    let (tag, tint, a_path, b_path, a_focus, b_focus) = match &changes[i] {
        Change::SetAttr { path, .. } | Change::RemoveAttr { path, .. } => {
            ("attr", Changed, at_a(path), at_b(path), None, None)
        }
        Change::SetText { path, .. } | Change::SpliceText { path, .. } => {
            ("text", Changed, at_a(path), at_b(path), None, None)
        }
        Change::SetMarks { path, .. } => ("marks", Changed, at_a(path), at_b(path), None, None),
        Change::SetExtra { path, .. } | Change::RemoveExtra { path, .. } => {
            ("extra", Changed, at_a(path), at_b(path), None, None)
        }
        Change::Replace { path, .. } => ("replace", Changed, at_a(path), at_b(path), None, None),
        Change::Insert { path, index, .. } => (
            "insert",
            Added,
            None,
            at_b(&child(path, *index)),
            at_a(path),
            None,
        ),
        Change::Remove { path, index } => (
            "remove",
            Removed,
            at_a(&child(path, *index)),
            None,
            None,
            at_b(path),
        ),
        Change::Move { path, from, to } => (
            "move",
            Moved,
            at_a(&child(path, *from)),
            at_b(&child(path, *to)),
            None,
            None,
        ),
        _ => ("change", Changed, None, None, None, None),
    };
    let na = a_path.as_deref().and_then(|p| a.node_at(p));
    let nb = b_path.as_deref().and_then(|p| b.node_at(p));
    let (key, from, to): (Option<&str>, Option<SharedString>, Option<SharedString>) =
        match &changes[i] {
            Change::SetAttr { key, value, .. } => (
                Some(key),
                Some(val(na.and_then(|n| n.attr(key)))),
                Some(short_value(value).into()),
            ),
            Change::RemoveAttr { key, .. } => (
                Some(key),
                Some(val(na.and_then(|n| n.attr(key)))),
                Some("∅".into()),
            ),
            Change::SetText { .. } | Change::SpliceText { .. } => (None, quote(na), quote(nb)),
            Change::SetMarks { marks, .. } => (
                None,
                Some(marks_str(na.and_then(|n| n.marks.as_deref()))),
                Some(marks_str(marks.as_deref())),
            ),
            Change::SetExtra { key, value, .. } => (
                Some(key),
                Some(val(na.and_then(|n| n.extra_field(key)))),
                Some(short_value(value).into()),
            ),
            Change::RemoveExtra { key, .. } => (
                Some(key),
                Some(val(na.and_then(|n| n.extra_field(key)))),
                Some("∅".into()),
            ),
            Change::Replace { node, .. } => (
                None,
                na.map(|n| node_type(n).to_string().into()),
                Some(node_type(node).to_string().into()),
            ),
            Change::Insert { node, .. } => (None, None, Some(node_preview(node))),
            Change::Remove { .. } => (None, na.map(node_preview), None),
            Change::Move { .. } => (
                None,
                a_path
                    .as_ref()
                    .and_then(|p| p.last())
                    .map(|i| format!("#{i}").into()),
                b_path
                    .as_ref()
                    .and_then(|p| p.last())
                    .map(|i| format!("#{i}").into()),
            ),
            _ => (None, None, None),
        };
    // Rebase a node-free copy of the change: its path is then in A's own
    // coordinates (slots for inserts), without cloning an inserted subtree.
    let order_key = match &changes[i] {
        Change::Insert { path, index, .. } => rebase_change(
            &Change::Insert {
                path: path.clone(),
                index: *index,
                node: Node::default(),
            },
            prior,
        )
        .and_then(|c| match c {
            Change::Insert { path, index, .. } => Some((child(&path, index), 1)),
            _ => None,
        }),
        Change::Remove { .. } => a_path.clone().map(|p| (p, 0)),
        _ => a_path.clone().map(|p| (p, 2)),
    };
    let shown = a_path.as_ref().or(b_path.as_ref());
    let ty = na.or(nb).map(node_type).unwrap_or("?");
    let label = format!(
        "{} {ty}{}",
        shown.map(|p| fmt_path(p)).unwrap_or_else(|| "?".into()),
        key.map(|k| format!(" · {k}")).unwrap_or_default()
    );
    Entry {
        tag,
        tint,
        label: label.into(),
        from,
        to,
        a_path,
        b_path,
        a_focus,
        b_focus,
        order_key,
    }
}

// ---- view ---------------------------------------------------------------------

pub struct DiffPanel {
    diff: Entity<DiffState>,
    focus: FocusHandle,
    scroll_list: UniformListScrollHandle,
    scroll_a: UniformListScrollHandle,
    scroll_b: UniformListScrollHandle,
    last: (Option<usize>, Option<Vec<usize>>, Option<Vec<usize>>),
    _subs: Vec<Subscription>,
}

impl DiffPanel {
    pub fn new(diff: Entity<DiffState>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&diff, |this, diff, cx| {
            this.follow_selection(diff.read(cx));
            cx.notify();
        });
        Self {
            diff,
            focus: cx.focus_handle(),
            scroll_list: UniformListScrollHandle::new(),
            scroll_a: UniformListScrollHandle::new(),
            scroll_b: UniformListScrollHandle::new(),
            last: (None, None, None),
            _subs: vec![sub],
        }
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Keep the selected change and both selected rows in view.
    fn follow_selection(&mut self, d: &DiffState) {
        let now = (
            d.selected,
            d.side_a.selection.clone(),
            d.side_b.selection.clone(),
        );
        if now == self.last {
            return;
        }
        if let Some(i) = now.0 {
            if let Some(pos) = d.visible_pos(i) {
                self.scroll_list
                    .scroll_to_item(pos, ScrollStrategy::Nearest);
            }
        }
        for (sel, side, scroll) in [
            (&now.1, &d.side_a, &self.scroll_a),
            (&now.2, &d.side_b, &self.scroll_b),
        ] {
            if let Some(ix) = sel
                .as_ref()
                .and_then(|p| side.rows.iter().position(|(r, _)| &r.path == p))
            {
                scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
            }
        }
        self.last = now;
    }

    /// Sync scroll: after a wheel event on `from`, scroll the other tree so
    /// the row for the same node (via `map_path` / `unmap_path`) sits at the
    /// same height. Falls back to the proportional offset when no row near
    /// the top maps. Event-driven (never per frame); setting an offset fires
    /// no wheel event, so there is no feedback loop.
    fn sync_from(&mut self, from: Pane, cx: &mut Context<Self>) {
        let d = self.diff.read(cx);
        if !d.sync_scroll || !d.has_b() {
            return;
        }
        let (src_rows, dst_rows, src, dst) = match from {
            Pane::A => (
                &d.side_a.rows,
                &d.side_b.rows,
                &self.scroll_a,
                &self.scroll_b,
            ),
            Pane::B => (
                &d.side_b.rows,
                &d.side_a.rows,
                &self.scroll_b,
                &self.scroll_a,
            ),
        };
        let (y, src_max, item_h) = {
            let st = src.0.borrow();
            let Some(size) = st.last_item_size else {
                return;
            };
            let n = src_rows.len().max(1) as f64;
            (
                -st.base_handle.offset().y.to_f64(),
                (size.contents.height - size.item.height).to_f64().max(0.),
                (size.contents.height.to_f64() / n).max(1.),
            )
        };
        let Some(dst_size) = dst.0.borrow().last_item_size else {
            return;
        };
        let dst_max = (dst_size.contents.height - dst_size.item.height)
            .to_f64()
            .max(0.);
        let top = (y / item_h).floor().max(0.) as usize;
        let frac = y - top as f64 * item_h;
        let find = |path: Vec<usize>| -> Option<usize> {
            let mut p = path;
            loop {
                if let Some(j) = dst_rows.iter().position(|(r, _)| r.path == p) {
                    return Some(j);
                }
                p.pop()?;
            }
        };
        let mapped = (0..16).find_map(|k| {
            let (row, _) = src_rows.get(top + k)?;
            let other = match from {
                Pane::A => map_path(&row.path, &d.changes),
                Pane::B => unmap_path(&row.path, &d.changes),
            }?;
            let j = find(other)?;
            Some(j as f64 - k as f64)
        });
        let target = match mapped {
            Some(j) => j * item_h + frac,
            None if src_max > 0. => y / src_max * dst_max,
            None => 0.,
        }
        .clamp(0., dst_max);
        let st = dst.0.borrow();
        let x = st.base_handle.offset().x;
        st.base_handle.set_offset(point(x, px(-target as f32)));
        cx.notify();
    }

    fn empty(&self, cx: &mut Context<Self>) -> Div {
        let diff = self.diff.clone();
        let error = self.diff.read(cx).error.clone();
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .child(
                div()
                    .text_color(Tokens::ghost())
                    .child(Icon::new(IconName::GitCompare).large()),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(Tokens::muted())
                    .child("Open a second document to diff"),
            )
            .child(
                Button::new("empty-open-b")
                    .outline()
                    .small()
                    .icon(Icon::new(IconName::FolderOpen).small())
                    .label("Open B…")
                    .on_click(move |_, _, cx| open_b(diff.clone(), cx)),
            )
            .when_some(error, |d, e| {
                d.child(
                    div()
                        .text_size(px(11.))
                        .text_color(Tokens::removed())
                        .child(e),
                )
            })
    }

    fn changes_pane(&self, d: &DiffState, mono: &SharedString) -> Stateful<Div> {
        let entries = d.entries.clone();
        let kept = d.kept_flags.clone();
        let visible = d.visible.clone();
        let selected = d.selected;
        let shown = visible.len();
        let open = d.total();
        let kept_n = d.kept_count();
        let show_dismissed = d.show_dismissed;
        let diff = self.diff.clone();
        let toggle = self.diff.clone();
        let focus = self.focus.clone();
        let mono2 = mono.clone();
        let list = uniform_list("diff-changes", shown, move |range, _, _| {
            range
                .map(|pos| {
                    let i = visible[pos];
                    let diff = diff.clone();
                    let focus = focus.clone();
                    let k = kept.get(i).copied().unwrap_or(false);
                    change_row(i, &entries[i], selected == Some(i), k, &mono2).on_click(
                        move |_, window, cx| {
                            window.focus(&focus, cx);
                            diff.update(cx, |d, cx| d.select_change(i, cx));
                        },
                    )
                })
                .collect()
        })
        .track_scroll(&self.scroll_list)
        .flex_1()
        .p(px(6.));
        let pos = match selected.and_then(|i| d.visible_pos(i)) {
            Some(p) => format!("{} / {shown}", p + 1),
            None => format!("– / {shown}"),
        };
        let empty_msg = if kept_n > 0 {
            "All changes resolved"
        } else {
            "Documents are identical"
        };
        div()
            .id("diff-changes-pane")
            .flex()
            .flex_col()
            .w(px(300.))
            .flex_shrink_0()
            .h_full()
            .bg(Tokens::panel())
            .border_r_1()
            .border_color(Tokens::border())
            .child(pane_header("Changes").child(div().child(format!("{open}"))))
            .child(if shown == 0 {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .text_color(Tokens::disabled())
                    .child(empty_msg)
                    .into_any_element()
            } else {
                list.into_any_element()
            })
            .when(kept_n > 0, |d| {
                d.child(
                    div()
                        .id("diff-show-dismissed")
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(12.))
                        .py(px(8.))
                        .border_t_1()
                        .border_color(Tokens::border())
                        .cursor_pointer()
                        .text_size(px(12.))
                        .text_color(Tokens::muted())
                        .hover(|s| s.bg(Tokens::raised()).text_color(Tokens::foreground()))
                        .on_click(move |_, _, cx| {
                            toggle.update(cx, |d, cx| d.toggle_show_dismissed(cx))
                        })
                        .child(
                            Icon::new(if show_dismissed {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .xsmall(),
                        )
                        .child(format!(
                            "{kept_n} dismissed · {}",
                            if show_dismissed { "hide" } else { "show" }
                        )),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_between()
                    .px(px(12.))
                    .py(px(8.))
                    .border_t_1()
                    .border_color(Tokens::border())
                    .text_size(px(11.))
                    .font_family(mono.clone())
                    .text_color(Tokens::muted())
                    .child(pos)
                    .child(
                        div()
                            .text_color(Tokens::disabled())
                            .child("↑↓ step · ⏎ take B · ⌫ dismiss"),
                    ),
            )
    }

    fn tree_pane(
        &self,
        pane: Pane,
        d: &DiffState,
        file: SharedString,
        mono: &SharedString,
        on_wheel: impl Fn(&ScrollWheelEvent, &mut Window, &mut App) + 'static,
    ) -> Div {
        let side = match pane {
            Pane::A => &d.side_a,
            Pane::B => &d.side_b,
        };
        let rows = side.rows.clone();
        let selection = side.selection.clone();
        let count = rows.len();
        let matches = d.matches(pane).clone();
        let found = d
            .find_active
            .then(|| format!(" · {} found", matches.set.len()));
        let focus = self.focus.clone();
        let on_select: RowFn = Rc::new({
            let diff = self.diff.clone();
            move |path, window, cx| {
                window.focus(&focus, cx);
                diff.update(cx, |d, cx| d.select_path(pane, path, cx));
            }
        });
        let on_toggle: RowFn = Rc::new({
            let diff = self.diff.clone();
            move |path, _, cx| diff.update(cx, |d, cx| d.toggle(pane, path, cx))
        });
        let (id, row_id, letter, scroll) = match pane {
            Pane::A => ("diff-tree-a", "diff-row-a", "A", &self.scroll_a),
            Pane::B => ("diff-tree-b", "diff-row-b", "B", &self.scroll_b),
        };
        let mono2 = mono.clone();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(Tokens::panel())
            .border_r_1()
            .border_color(Tokens::border())
            .on_scroll_wheel(on_wheel)
            .child(
                pane_header(format!("{letter} · {file}")).child(
                    div()
                        .font_family(mono.clone())
                        .child(format!("{count} rows{}", found.unwrap_or_default())),
                ),
            )
            .child(
                uniform_list(id, count, move |range, _, _| {
                    range
                        .map(|i| {
                            let (row, deco) = &rows[i];
                            let selected = selection.as_deref() == Some(row.path.as_slice());
                            let hit = matches.row_match(&row.path, row.expanded);
                            // Same find decoration as the Inspect tree; a
                            // diff-tinted row keeps its tint (bar only).
                            crate::find::decorate_row(
                                render_row(
                                    row_id, i, row, selected, deco, &mono2, &on_select, &on_toggle,
                                ),
                                hit,
                                selected || deco.tint.is_some(),
                            )
                            .w_full()
                        })
                        .collect()
                })
                .track_scroll(scroll)
                .flex_1()
                .p(px(6.)),
            )
    }

    fn inspector_pane(
        &self,
        d: &DiffState,
        a: &DocumentState,
        mono: &SharedString,
    ) -> Stateful<Div> {
        let pane = div()
            .id("diff-inspector")
            .flex()
            .flex_col()
            .w(px(300.))
            .flex_shrink_0()
            .h_full()
            .overflow_y_scroll()
            .bg(Tokens::panel());
        let Some((i, e)) = d.selected.and_then(|i| d.entries.get(i).map(|e| (i, e))) else {
            if let Some(p) = &d.pick {
                let node =
                    p.a.as_deref()
                        .and_then(|x| a.root().node_at(x))
                        .or_else(|| {
                            p.b.as_deref()
                                .and_then(|x| d.b.as_ref().and_then(|b| b.root().node_at(x)))
                        });
                let ty = node.map(node_type).unwrap_or("?").to_string();
                let path = p.a.as_ref().or(p.b.as_ref());
                return pane
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .h(px(34.))
                            .flex_shrink_0()
                            .px(px(16.))
                            .border_b_1()
                            .border_color(Tokens::border())
                            .child(
                                div()
                                    .font_family(mono.clone())
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(ty),
                            )
                            .when_some(path, |d, p| d.child(chip(fmt_path(p), mono))),
                    )
                    .child(
                        section("Resolve", Some("no change on this node".into()))
                            .child(self.subtree_button(p)),
                    );
            }
            return pane.child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .text_color(Tokens::disabled())
                    .child("Select a change"),
            );
        };
        let na = e.a_path.as_deref().and_then(|p| a.root().node_at(p));
        let nb = e
            .b_path
            .as_deref()
            .and_then(|p| d.b.as_ref().and_then(|b| b.root().node_at(p)));
        let ty = na.or(nb).map(node_type).unwrap_or("?").to_string();
        let path = e.a_path.as_ref().or(e.b_path.as_ref());

        // header: type · path chip · tag
        let mut pane = pane.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(34.))
                .flex_shrink_0()
                .px(px(16.))
                .border_b_1()
                .border_color(Tokens::border())
                .child(
                    div()
                        .font_family(mono.clone())
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .when(e.tint == Tint::Removed, |d| d.line_through())
                        .text_color(match e.tint {
                            Tint::Removed => Tokens::removed(),
                            Tint::Added => Tokens::added(),
                            _ => Tokens::foreground(),
                        })
                        .child(ty),
                )
                .when_some(path, |d, p| d.child(chip(fmt_path(p), mono)))
                .child(div().flex_1())
                .child(tag_chip(e.tag, e.tint, mono)),
        );

        // ATTRIBUTES A → B
        let empty = serde_json::Map::new();
        let aa = na.and_then(|n| n.attrs.as_deref()).unwrap_or(&empty);
        let ba = nb.and_then(|n| n.attrs.as_deref()).unwrap_or(&empty);
        let mut keys: Vec<&String> = aa.keys().collect();
        for k in ba.keys() {
            if !aa.contains_key(k) {
                keys.push(k);
            }
        }
        let mut attrs = section("Attributes A → B", Some(format!("{}", keys.len()).into()));
        if keys.is_empty() {
            attrs = attrs.child(
                div()
                    .text_size(px(12.))
                    .text_color(Tokens::disabled())
                    .child("no attributes"),
            );
        }
        for k in keys {
            let (old, new) = (aa.get(k), ba.get(k));
            let same = old == new || na.is_none() || nb.is_none();
            let value = if same {
                div()
                    .font_family(mono.clone())
                    .text_size(px(12.))
                    .text_color(Tokens::muted())
                    .child(val(old.or(new)))
            } else {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .font_family(mono.clone())
                    .text_size(px(12.))
                    .child(
                        div()
                            .text_color(Tokens::removed())
                            .line_through()
                            .child(val(old)),
                    )
                    .child(div().text_color(Tokens::added()).child(val(new)))
            };
            attrs = attrs.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .child(
                        div()
                            .w(px(96.))
                            .flex_shrink_0()
                            .text_size(px(12.))
                            .text_color(if same {
                                Tokens::secondary()
                            } else {
                                Tokens::unknown()
                            })
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(k.clone()),
                    )
                    .child(div().flex_1().min_w_0().child(value)),
            );
        }
        pane = pane.child(attrs);

        // VALUE A → B (text, marks, inserted/removed node line…)
        if e.from.is_some() || e.to.is_some() {
            pane = pane.child(
                section("Value A → B", None).child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .font_family(mono.clone())
                        .text_size(px(12.))
                        .when_some(e.from.clone(), |d, f| {
                            d.child(div().text_color(Tokens::removed()).line_through().child(f))
                        })
                        .when_some(e.to.clone(), |d, t| {
                            d.child(div().text_color(Tokens::added()).child(t))
                        }),
                ),
            );
        }

        // RESOLVE
        let (keep, take) = (self.diff.clone(), self.diff.clone());
        let is_kept = d.is_kept(i);
        pane = pane.child(
            section(
                "Resolve",
                Some(if is_kept {
                    "dismissed".into()
                } else {
                    match d.visible_pos(i) {
                        Some(p) => format!("{} / {}", p + 1, d.visible.len()).into(),
                        None => SharedString::default(),
                    }
                }),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        Button::new("diff-keep-a")
                            .outline()
                            .small()
                            .label(if is_kept { "Restore" } else { "Dismiss" })
                            .flex_1()
                            .on_click(move |_, _, cx| keep.update(cx, |d, cx| d.dismiss(cx))),
                    )
                    .child(
                        Button::new("diff-take-b")
                            .primary()
                            .small()
                            .label("Take B")
                            .flex_1()
                            .on_click(move |_, _, cx| take.update(cx, |d, cx| d.take_b(cx))),
                    ),
            )
            .when_some(d.pick.as_ref(), |s, p| s.child(self.subtree_button(p))),
        );

        // CHANGE (pretty JSON)
        pane.child(
            section("Change", None).child(
                div()
                    .p(px(10.))
                    .rounded(px(6.))
                    .bg(Tokens::code_bg())
                    .border_1()
                    .border_color(Tokens::border())
                    .font_family(mono.clone())
                    .text_size(px(11.))
                    .text_color(Tokens::secondary())
                    .child(d.json.clone()),
            ),
        )
    }
}

impl DiffPanel {
    /// "Take all B changes in this subtree": one undo step.
    fn subtree_button(&self, p: &Pick) -> Button {
        let diff = self.diff.clone();
        Button::new("diff-take-subtree")
            .outline()
            .small()
            .w_full()
            .icon(Icon::new(IconName::ListTree).small())
            .label(match p.count {
                0 => "No B changes in subtree".to_string(),
                n => format!("Take all B in subtree ({n})"),
            })
            .tooltip("Take all B changes in this subtree (one undo step)")
            .disabled(p.count == 0)
            .on_click(move |_, _, cx| diff.update(cx, |d, cx| d.take_subtree(cx)))
    }
}

fn tag_chip(tag: &'static str, tint: Tint, mono: &SharedString) -> Div {
    let mut fill = tint.color();
    fill.a = 0.16;
    div()
        .flex_shrink_0()
        .px(px(6.))
        .py(px(1.))
        .rounded(px(4.))
        .bg(fill)
        .font_family(mono.clone())
        .text_size(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tint.color())
        .child(tag)
}

fn change_row(
    i: usize,
    e: &Entry,
    selected: bool,
    kept: bool,
    mono: &SharedString,
) -> Stateful<Div> {
    div()
        .id(ElementId::NamedInteger("diff-change".into(), i as u64))
        .w_full()
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(3.))
        .h(px(46.))
        .px(px(8.))
        .rounded(px(5.))
        .cursor_pointer()
        .overflow_hidden()
        .whitespace_nowrap()
        .hover(|s| s.bg(Tokens::raised()))
        .when(kept, |s| s.opacity(0.45))
        .when(selected, |s| {
            s.bg(Tokens::accent_fill())
                .border_1()
                .border_color(Tokens::accent())
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(tag_chip(e.tag, e.tint, mono))
                .when(kept, |d| {
                    d.child(
                        div()
                            .flex_shrink_0()
                            .px(px(5.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(Tokens::input_border())
                            .font_family(mono.clone())
                            .text_size(px(10.))
                            .text_color(Tokens::muted())
                            .child("dismissed"),
                    )
                })
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .font_family(mono.clone())
                        .text_size(px(12.))
                        .text_color(Tokens::foreground())
                        .child(e.label.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .min_w_0()
                .overflow_hidden()
                .font_family(mono.clone())
                .text_size(px(11.))
                .when_some(e.from.clone(), |d, f| {
                    d.child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_color(Tokens::removed())
                            .child(f),
                    )
                })
                .when(e.from.is_some() && e.to.is_some(), |d| {
                    d.child(
                        div()
                            .flex_shrink_0()
                            .text_color(Tokens::disabled())
                            .child("→"),
                    )
                })
                .when_some(e.to.clone(), |d, t| {
                    d.child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_color(Tokens::added())
                            .child(t),
                    )
                }),
        )
}

impl Render for DiffPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        let root = div()
            .id("diff-pane")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &PrevChange, _, cx| {
                this.diff.update(cx, |d, cx| d.step(-1, cx))
            }))
            .on_action(cx.listener(|this, _: &NextChange, _, cx| {
                this.diff.update(cx, |d, cx| d.step(1, cx))
            }))
            .on_action(
                cx.listener(|this, _: &TakeB, _, cx| this.diff.update(cx, |d, cx| d.take_b(cx))),
            )
            .on_action(
                cx.listener(|this, _: &Dismiss, _, cx| this.diff.update(cx, |d, cx| d.dismiss(cx))),
            )
            .flex()
            .flex_1()
            .min_h_0()
            .size_full();
        if !self.diff.read(cx).has_b() {
            return root.child(self.empty(cx));
        }
        let wheel = |pane: Pane, cx: &mut Context<Self>| {
            let this = cx.entity().downgrade();
            move |_: &ScrollWheelEvent, _: &mut Window, cx: &mut App| {
                // After the list applied the delta (dispatch order is not ours).
                let this = this.clone();
                cx.defer(move |cx| {
                    this.update(cx, |p, cx| p.sync_from(pane, cx)).ok();
                });
            }
        };
        let (wheel_a, wheel_b) = (wheel(Pane::A, cx), wheel(Pane::B, cx));
        let d = self.diff.read(cx);
        let a = d.a.read(cx);
        let a_file = a.file_name();
        let b_file = d.b_name();
        root.child(crate::layout::diff_row(
            self.changes_pane(d, &mono),
            self.tree_pane(Pane::A, d, a_file, &mono, wheel_a),
            self.tree_pane(Pane::B, d, b_file, &mono, wheel_b),
            self.inspector_pane(d, a, &mono),
            cx,
        ))
    }
}
