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
use gpui_kit::component::{ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::Value;
use tiptap_rusty_parser::{map_path, rebase_change, unmap_path, Change, Document, Node};

use crate::inspector::{chip, section};
use crate::state::{fmt_path, node_summary, node_type, short_value, text_preview, DocumentState};
use crate::theme::Tokens;
use crate::tree::{flatten, pane_header, render_row, Deco, Row, RowFn};

gpui_kit::actions!(diff, [PrevChange, NextChange, TakeB, KeepA]);

pub const CONTEXT: &str = "Diff";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", PrevChange, Some(CONTEXT)),
        KeyBinding::new("down", NextChange, Some(CONTEXT)),
        KeyBinding::new("enter", TakeB, Some(CONTEXT)),
        KeyBinding::new("backspace", KeepA, Some(CONTEXT)),
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
}

impl Entry {
    /// Identity for a Keep A decision. Anchored on B (read-only, so its paths
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
    /// Path given on the command line; fallback when the file prompt fails.
    pub cli_b: Option<PathBuf>,
    pub error: Option<SharedString>,
    changes: Vec<Change>,
    entries: Rc<Vec<Entry>>,
    /// Keep A decisions, by `Entry::key`. Cleared when B changes.
    kept: HashSet<String>,
    /// `kept_flags[i]`: entry `i` is resolved as Keep A.
    kept_flags: Rc<Vec<bool>>,
    pub selected: Option<usize>,
    json: SharedString,
    side_a: Side,
    side_b: Side,
    pub only_changed: bool,
    seen_revision: Option<u64>,
    /// (+ inserted, − removed, ~ modified/moved)
    pub counts: (usize, usize, usize),
    _subs: Vec<Subscription>,
}

impl DiffState {
    pub fn new(a: Entity<DocumentState>, b: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&a, |this, a, cx| {
            let a = a.read(cx);
            if this.seen_revision != Some(a.revision) {
                this.recompute(a, false);
            }
            // Selection-only notifies change nothing here.
            cx.notify();
        });
        let mut this = Self {
            a: a.clone(),
            b: None,
            b_file: None,
            cli_b: b.clone(),
            error: None,
            changes: Vec::new(),
            entries: Rc::new(Vec::new()),
            kept: HashSet::new(),
            kept_flags: Rc::new(Vec::new()),
            selected: None,
            json: SharedString::default(),
            side_a: Side::default(),
            side_b: Side::default(),
            only_changed: false,
            seen_revision: None,
            counts: (0, 0, 0),
            _subs: vec![sub],
        };
        if let Some(p) = b {
            this.load_b_inner(&p, a.read(cx));
        }
        this
    }

    pub fn has_b(&self) -> bool {
        self.b.is_some()
    }

    pub fn b_name(&self) -> SharedString {
        self.b_file
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string().into())
            .unwrap_or_else(|| "—".into())
    }

    /// Changes still to resolve (kept ones excluded).
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
                self.error = None;
                self.selected = None;
                self.kept.clear();
                self.side_a = Side::default();
                self.side_b = Side::default();
                self.side_a.expanded.insert(Vec::new());
                self.side_b.expanded.insert(Vec::new());
                self.recompute(a, true);
            }
            Err(e) => {
                eprintln!("tiptap-studio: open B {}: {e}", path.display());
                self.error = Some(format!("{}: {e}", path.display()).into());
            }
        }
    }

    /// Re-diff A against B and rebuild every derived structure. Runs on document
    /// change only (revision guard), never per frame.
    fn recompute(&mut self, a: &DocumentState, expand: bool) {
        self.seen_revision = Some(a.revision);
        let Some(b) = self.b.as_ref() else {
            return;
        };
        self.changes = a.doc.diff(b);
        let (ra, rb) = (a.root(), b.root());
        let entries: Vec<Entry> = (0..self.changes.len())
            .map(|i| describe(i, &self.changes, ra, rb))
            .collect();
        self.entries = Rc::new(entries);
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
        if self.entries.is_empty() {
            self.selected = None;
        } else if let Some(i) = self.selected {
            self.selected = Some(i.min(self.entries.len() - 1));
        }
        self.sync_selection(a);
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
        self.with_a(cx, |this, a| this.sync_selection(a));
    }

    pub fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.entries.len();
        if n == 0 {
            return;
        }
        let next = match self.selected {
            Some(i) => (i as isize + delta).clamp(0, n as isize - 1) as usize,
            None if delta < 0 => n - 1,
            None => 0,
        };
        self.select_change(next, cx);
    }

    /// Click in a tree: select there, map the path to the other tree and pick
    /// the change that touches it (if any).
    pub fn select_path(&mut self, pane: Pane, path: &[usize], cx: &mut Context<Self>) {
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
        let (pa, pb) = (here, other);
        if let Some(p) = &pa {
            self.side_a.expand_to(p, false);
        }
        if let Some(p) = &pb {
            self.side_b.expand_to(p, false);
        }
        self.side_a.selection = pa;
        self.side_b.selection = pb;
        self.with_a(cx, |this, a| this.rebuild_rows(a));
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

    /// Keep A: resolve the selected change in A's favour (or un-resolve a
    /// kept one) and move to the next unresolved change.
    pub fn keep_a(&mut self, cx: &mut Context<Self>) {
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
        self.with_a(cx, |this, a| this.retint(a, false));
        if now_kept {
            let n = self.entries.len();
            if let Some(next) = (1..n).map(|d| (i + d) % n).find(|&j| !self.is_kept(j)) {
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
            let Some(c) = self.unkept_changes(cx) else {
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
    /// Every unresolved change as one live sequence, skipping kept ones.
    /// Picks the first unkept change, applies it to a scratch copy of A,
    /// re-diffs, repeats: each pick is rebased against the changes before it
    /// in its own diff, so the result replays exactly onto A. Costs one diff
    /// per applied change; only used when something is kept.
    fn unkept_changes(&self, cx: &App) -> Option<Vec<Change>> {
        let b = self.b.as_ref()?.root();
        let mut work = self.a.read(cx).root().clone();
        let mut out = Vec::new();
        for _ in 0..10_000 {
            let changes = work.diff(b);
            let pick = (0..changes.len())
                .find(|&i| !self.kept.contains(&describe(i, &changes, &work, b).key()));
            let Some(i) = pick else { break };
            let c = rebase_change(&changes[i], &changes[..i])?;
            work.apply(std::slice::from_ref(&c)).ok()?;
            out.push(c);
        }
        Some(out)
    }
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

fn node_line(n: &Node) -> SharedString {
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
                Some(val(na.and_then(|n| n.extra.get(key)))),
                Some(short_value(value).into()),
            ),
            Change::RemoveExtra { key, .. } => (
                Some(key),
                Some(val(na.and_then(|n| n.extra.get(key)))),
                Some("∅".into()),
            ),
            Change::Replace { node, .. } => (
                None,
                na.map(|n| node_type(n).to_string().into()),
                Some(node_type(node).to_string().into()),
            ),
            Change::Insert { node, .. } => (None, None, Some(node_line(node))),
            Change::Remove { .. } => (None, na.map(node_line), None),
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
            self.scroll_list.scroll_to_item(i, ScrollStrategy::Nearest);
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
        let selected = d.selected;
        let total = entries.len();
        let open = d.total();
        let kept_n = d.kept_count();
        let diff = self.diff.clone();
        let focus = self.focus.clone();
        let mono2 = mono.clone();
        let list = uniform_list("diff-changes", total, move |range, _, _| {
            range
                .map(|i| {
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
        let pos = match selected {
            Some(i) => format!("{} / {total}", i + 1),
            None => format!("– / {total}"),
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
            .child(pane_header("Changes").child(div().child(if kept_n > 0 {
                format!("{open} · {kept_n} kept")
            } else {
                format!("{open}")
            })))
            .child(if total == 0 {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .text_color(Tokens::disabled())
                    .child("Documents are identical")
                    .into_any_element()
            } else {
                list.into_any_element()
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
                            .child("↑↓ step · ⏎ take B · ⌫ keep A"),
                    ),
            )
    }

    fn tree_pane(&self, pane: Pane, d: &DiffState, file: SharedString, mono: &SharedString) -> Div {
        let side = match pane {
            Pane::A => &d.side_a,
            Pane::B => &d.side_b,
        };
        let rows = side.rows.clone();
        let selection = side.selection.clone();
        let count = rows.len();
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
            .child(
                pane_header(format!("{letter} · {file}")).child(
                    div()
                        .font_family(mono.clone())
                        .child(format!("{count} rows")),
                ),
            )
            .child(
                uniform_list(id, count, move |range, _, _| {
                    range
                        .map(|i| {
                            let (row, deco) = &rows[i];
                            let selected = selection.as_deref() == Some(row.path.as_slice());
                            render_row(
                                row_id, i, row, selected, deco, &mono2, &on_select, &on_toggle,
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
        let aa = na.and_then(|n| n.attrs.as_ref()).unwrap_or(&empty);
        let ba = nb.and_then(|n| n.attrs.as_ref()).unwrap_or(&empty);
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
                    "kept A".into()
                } else {
                    format!("{} / {}", i + 1, d.entries.len()).into()
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
                            .label(if is_kept { "Unkeep" } else { "Keep A" })
                            .flex_1()
                            .on_click(move |_, _, cx| keep.update(cx, |d, cx| d.keep_a(cx))),
                    )
                    .child(
                        Button::new("diff-take-b")
                            .primary()
                            .small()
                            .label("Take B")
                            .flex_1()
                            .on_click(move |_, _, cx| take.update(cx, |d, cx| d.take_b(cx))),
                    ),
            ),
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
                            .child("kept A"),
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
                cx.listener(|this, _: &KeepA, _, cx| this.diff.update(cx, |d, cx| d.keep_a(cx))),
            )
            .flex()
            .flex_1()
            .min_h_0()
            .size_full();
        if !self.diff.read(cx).has_b() {
            return root.child(self.empty(cx));
        }
        let d = self.diff.read(cx);
        let a = d.a.read(cx);
        let a_file = a.file_name();
        let b_file = d.b_name();
        root.child(self.changes_pane(d, &mono))
            .child(self.tree_pane(Pane::A, d, a_file, &mono))
            .child(self.tree_pane(Pane::B, d, b_file, &mono))
            .child(self.inspector_pane(d, a, &mono))
    }
}
