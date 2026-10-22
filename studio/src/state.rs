//! Shared document entity: parser `Document` + selection + expansion + undo/redo.
//!
//! Every mutation goes through [`DocumentState::edit`], which runs a `Transform`,
//! keeps its `Change` log and stores the inverse for undo. Panels never touch
//! `&mut Node` directly.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui_kit::*;
use serde_json::Value;
use tiptap_rusty_parser::{invert, Change, Document, Mark, Node, Transform};

const SAMPLE: &str = include_str!("../assets/sample.json");

/// Core Tiptap/ProseMirror node types the preview knows how to draw.
/// Anything else renders as an "unknown" block and is still selectable.
pub const KNOWN_TYPES: &[&str] = &[
    "doc",
    "paragraph",
    "text",
    "heading",
    "bulletList",
    "orderedList",
    "listItem",
    "blockquote",
    "codeBlock",
    "horizontalRule",
    "hardBreak",
    "image",
    "table",
    "tableRow",
    "tableCell",
    "tableHeader",
    "taskList",
    "taskItem",
];

pub const BUILTIN_MARKS: &[&str] = &["bold", "italic", "underline", "strike", "code", "link"];

pub struct DocumentState {
    pub doc: Document,
    pub file: Option<PathBuf>,
    pub selection: Option<Vec<usize>>,
    pub expanded: HashSet<Vec<usize>>,
    pub dirty: bool,
    pub last_change: Option<SharedString>,
    /// Bumped on every document mutation; observers skip selection-only notifies.
    pub revision: u64,
    undo: Vec<Vec<Change>>,
    redo: Vec<Vec<Change>>,
}

impl DocumentState {
    pub fn load(path: Option<&Path>) -> anyhow::Result<Self> {
        let doc = match path {
            Some(p) => Document::from_json_str(&std::fs::read_to_string(p)?)?,
            None => Document::from_json_str(SAMPLE)?,
        };
        let mut s = Self {
            doc,
            file: path.map(Path::to_path_buf),
            selection: None,
            expanded: HashSet::new(),
            dirty: false,
            last_change: None,
            revision: 0,
            undo: Vec::new(),
            redo: Vec::new(),
        };
        s.expand_to_depth(1);
        Ok(s)
    }

    pub fn root(&self) -> &Node {
        self.doc.root()
    }

    pub fn file_name(&self) -> SharedString {
        self.file
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string().into())
            .unwrap_or_else(|| "assets/sample.json".into())
    }

    pub fn selected(&self) -> Option<&Node> {
        self.selection
            .as_deref()
            .and_then(|p| self.root().node_at(p))
    }

    /// (node count, text node count, unknown-type count)
    pub fn stats(&self) -> (usize, usize, usize) {
        let (mut n, mut t, mut u) = (0, 0, 0);
        self.root().walk(&mut |node| {
            n += 1;
            match node.node_type.as_deref() {
                Some("text") => t += 1,
                Some(ty) if !KNOWN_TYPES.contains(&ty) => u += 1,
                _ => {}
            }
        });
        (n, t, u)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_depth(&self) -> (usize, usize) {
        (self.undo.len(), self.redo.len())
    }

    // ---- selection / expansion -------------------------------------------------

    pub fn select(&mut self, path: Option<Vec<usize>>, cx: &mut Context<Self>) {
        if self.selection != path {
            if let Some(p) = &path {
                for d in 0..p.len() {
                    self.expanded.insert(p[..d].to_vec());
                }
            }
            self.selection = path;
            cx.notify();
        }
    }

    pub fn toggle_expanded(&mut self, path: Vec<usize>, cx: &mut Context<Self>) {
        if !self.expanded.remove(&path) {
            self.expanded.insert(path);
        }
        cx.notify();
    }

    pub fn is_expanded(&self, path: &[usize]) -> bool {
        self.expanded.contains(path)
    }

    pub fn expand_to_depth(&mut self, depth: usize) {
        fn go(node: &Node, path: &mut Vec<usize>, depth: usize, out: &mut HashSet<Vec<usize>>) {
            if path.len() > depth {
                return;
            }
            out.insert(path.clone());
            for (i, c) in node.children().iter().enumerate() {
                path.push(i);
                go(c, path, depth, out);
                path.pop();
            }
        }
        let mut set = std::mem::take(&mut self.expanded);
        go(self.root(), &mut Vec::new(), depth, &mut set);
        self.expanded = set;
    }

    pub fn expand_all(&mut self, cx: &mut Context<Self>) {
        self.expand_to_depth(usize::MAX);
        cx.notify();
    }

    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.expanded.clear();
        self.expanded.insert(Vec::new());
        cx.notify();
    }

    // ---- mutation ---------------------------------------------------------------

    /// Run one transaction. The root is cloned once before the transform so
    /// `invert` has a base; incremental inverses inside `Transform` are a
    /// later crate feature.
    pub fn edit<E: std::fmt::Display>(
        &mut self,
        label: impl Into<SharedString>,
        f: impl FnOnce(&mut Transform<'_>) -> Result<(), E>,
        cx: &mut Context<Self>,
    ) -> bool {
        let before = self.root().clone();
        let mut t = self.doc.root_mut().transform();
        if let Err(e) = f(&mut t) {
            eprintln!("edit failed: {e}");
            *self.doc.root_mut() = before;
            return false;
        }
        let changes = t.finish();
        if changes.is_empty() {
            return false;
        }
        match invert(&before, &changes) {
            Ok(inverse) => {
                self.undo.push(inverse);
                self.redo.clear();
            }
            Err(e) => eprintln!("invert failed, undo unavailable for this edit: {e}"),
        }
        self.after_edit(label.into(), cx);
        true
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(inverse) = self.undo.pop() else {
            return;
        };
        if let Ok(redo) = invert(self.root(), &inverse) {
            self.redo.push(redo);
        }
        if let Err(e) = self.doc.apply(&inverse) {
            eprintln!("undo failed: {e}");
        }
        self.after_edit("undo".into(), cx);
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        let Some(changes) = self.redo.pop() else {
            return;
        };
        if let Ok(inverse) = invert(self.root(), &changes) {
            self.undo.push(inverse);
        }
        if let Err(e) = self.doc.apply(&changes) {
            eprintln!("redo failed: {e}");
        }
        self.after_edit("redo".into(), cx);
    }

    fn after_edit(&mut self, label: SharedString, cx: &mut Context<Self>) {
        self.dirty = true;
        self.revision += 1;
        self.last_change = Some(label);
        if let Some(p) = &self.selection {
            if self.root().node_at(p).is_none() {
                self.selection = None;
            }
        }
        cx.notify();
    }

    // ---- convenience edits used by panels ---------------------------------------

    pub fn set_attr(&mut self, path: Vec<usize>, key: &str, value: Value, cx: &mut Context<Self>) {
        let label = format!("SetAttr {} .{key}", fmt_path(&path));
        let key = key.to_string();
        self.edit(label, |t| t.set_attr(path, key, value).map(|_| ()), cx);
    }

    pub fn set_text(&mut self, path: Vec<usize>, text: String, cx: &mut Context<Self>) {
        let label = format!("SetText {}", fmt_path(&path));
        self.edit(label, |t| t.set_text(path, Some(text)).map(|_| ()), cx);
    }

    /// Set one attr on the first mark of `mark_type`; rebuilds the mark list.
    pub fn set_mark_attr(
        &mut self,
        path: Vec<usize>,
        mark_type: &str,
        key: &str,
        value: Value,
        cx: &mut Context<Self>,
    ) {
        let Some(node) = self.root().node_at(&path) else {
            return;
        };
        let mut marks = node.marks.clone().unwrap_or_default();
        let Some(mark) = marks.iter_mut().find(|m| m.mark_type == mark_type) else {
            return;
        };
        mark.attrs
            .get_or_insert_with(Default::default)
            .insert(key.to_string(), value);
        let label = format!("SetMarkAttr {} {mark_type}.{key}", fmt_path(&path));
        self.edit(label, |t| t.set_marks(path, Some(marks)).map(|_| ()), cx);
    }

    /// Append `mark` (replacing an existing mark of the same type).
    pub fn add_mark(&mut self, path: Vec<usize>, mark: Mark, cx: &mut Context<Self>) {
        let Some(node) = self.root().node_at(&path) else {
            return;
        };
        let mut marks = node.marks.clone().unwrap_or_default();
        marks.retain(|m| m.mark_type != mark.mark_type);
        let label = format!("AddMark {} {}", fmt_path(&path), mark.mark_type);
        marks.push(mark);
        self.edit(label, |t| t.set_marks(path, Some(marks)).map(|_| ()), cx);
    }

    pub fn remove_attr(&mut self, path: Vec<usize>, key: &str, cx: &mut Context<Self>) {
        let label = format!("RemoveAttr {} .{key}", fmt_path(&path));
        let key = key.to_string();
        self.edit(label, |t| t.remove_attr(path, key).map(|_| ()), cx);
    }

    pub fn toggle_mark(&mut self, path: Vec<usize>, mark_type: &str, cx: &mut Context<Self>) {
        let Some(node) = self.root().node_at(&path) else {
            return;
        };
        let mut marks = node.marks.clone().unwrap_or_default();
        let label = if let Some(i) = marks.iter().position(|m| m.mark_type == mark_type) {
            marks.remove(i);
            format!("RemoveMark {} {mark_type}", fmt_path(&path))
        } else {
            marks.push(Mark::new(mark_type));
            format!("AddMark {} {mark_type}", fmt_path(&path))
        };
        let marks = if marks.is_empty() { None } else { Some(marks) };
        self.edit(label, |t| t.set_marks(path, marks).map(|_| ()), cx);
    }

    pub fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.selection.clone() else {
            return;
        };
        let Some((&index, parent)) = path.split_last() else {
            return;
        };
        let parent = parent.to_vec();
        let label = format!("Remove {}", fmt_path(&path));
        if self.edit(label, |t| t.remove(parent.clone(), index).map(|_| ()), cx) {
            self.selection = Some(parent);
        }
    }

    /// Whether `move_selected(delta)` would land inside the parent.
    pub fn can_move(&self, delta: isize) -> bool {
        let Some(path) = self.selection.as_deref() else {
            return false;
        };
        let Some((&from, parent)) = path.split_last() else {
            return false;
        };
        let count = self
            .root()
            .node_at(parent)
            .map(Node::child_count)
            .unwrap_or(0);
        let to = from as isize + delta;
        to >= 0 && (to as usize) < count
    }

    pub fn move_selected(&mut self, delta: isize, cx: &mut Context<Self>) {
        if !self.can_move(delta) {
            return;
        }
        let Some(path) = self.selection.clone() else {
            return;
        };
        let Some((&from, parent)) = path.split_last() else {
            return;
        };
        let parent = parent.to_vec();
        let to = (from as isize + delta) as usize;
        let label = format!("Move {} → {to}", fmt_path(&path));
        if self.edit(
            label,
            |t| t.move_child(parent.clone(), from, to).map(|_| ()),
            cx,
        ) {
            let mut p = parent;
            p.push(to);
            self.selection = Some(p);
        }
    }

    /// `Node::lift` needs a grandparent.
    pub fn can_lift(&self) -> bool {
        self.selection.as_ref().is_some_and(|p| p.len() >= 2)
    }

    pub fn lift_selected(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.selection.clone() else {
            return;
        };
        let n = path.len();
        if n < 2 {
            return;
        }
        let label = format!("Lift {}", fmt_path(&path));
        if self.edit(label, |t| t.lift(path.clone()).map(|_| ()), cx) {
            // Lands in the grandparent after the parent's left remainder, if any.
            let mut p = path[..n - 2].to_vec();
            p.push(path[n - 2] + usize::from(path[n - 1] > 0));
            self.selection = Some(p);
        }
    }

    /// Wrapping the root would insert an empty wrapper; refuse it.
    pub fn can_wrap(&self) -> bool {
        self.selection.as_ref().is_some_and(|p| !p.is_empty())
    }

    /// Wrap the selection in `wrappers`, innermost first (`["listItem", "bulletList"]`),
    /// as one undo step. Selection moves to the outermost wrapper.
    pub fn wrap_selected(&mut self, wrappers: &[&str], cx: &mut Context<Self>) {
        let Some(path) = self.selection.clone() else {
            return;
        };
        if path.is_empty() || wrappers.is_empty() {
            return;
        }
        let shown: Vec<&str> = wrappers.iter().rev().copied().collect();
        let label = format!("Wrap {} in {}", fmt_path(&path), shown.join(" › "));
        let types: Vec<String> = wrappers.iter().map(|w| w.to_string()).collect();
        let ok = self.edit(
            label,
            |t| {
                for ty in types {
                    t.wrap(path.clone(), ty, None)?;
                }
                Ok::<(), tiptap_rusty_parser::BlockError>(())
            },
            cx,
        );
        if ok {
            // Reveal the wrapped node through every new level.
            let mut p = path.clone();
            for _ in 0..wrappers.len() {
                self.expanded.insert(p.clone());
                p.push(0);
            }
            self.selection = Some(path);
        }
    }

    /// `(block, at)` for `Transform::split_block`: a block with ≥2 children splits
    /// at its midpoint; a leaf splits its parent before itself. `None` when the
    /// split would only produce an empty block (root, single child, first leaf).
    pub fn split_target(&self) -> Option<(Vec<usize>, usize)> {
        let path = self.selection.as_deref()?;
        let node = self.root().node_at(path)?;
        match node.content.as_deref() {
            Some(c) if c.len() >= 2 && !path.is_empty() => Some((path.to_vec(), c.len() / 2)),
            Some(_) => None,
            None => match path.split_last() {
                Some((&i, parent)) if i > 0 && !parent.is_empty() => Some((parent.to_vec(), i)),
                _ => None,
            },
        }
    }

    pub fn split_selected(&mut self, cx: &mut Context<Self>) {
        let Some((block, at)) = self.split_target() else {
            return;
        };
        let label = format!("Split {} at {at}", fmt_path(&block));
        if self.edit(
            label,
            |t| t.split_block(block.clone(), at, 0).map(|_| ()),
            cx,
        ) {
            self.selection = Some(block);
        }
    }

    /// `(parent, index)` for `Transform::join_blocks` merging the next sibling into
    /// the selection. `None` without a next sibling, or when either side is a leaf
    /// (the crate would silently drop the leaf's payload).
    pub fn join_target(&self) -> Option<(Vec<usize>, usize)> {
        let path = self.selection.as_deref()?;
        let (&i, parent) = path.split_last()?;
        let parent_node = self.root().node_at(parent)?;
        let kids = parent_node.children();
        let (left, right) = (kids.get(i)?, kids.get(i + 1)?);
        (left.content.is_some() && right.content.is_some()).then(|| (parent.to_vec(), i + 1))
    }

    pub fn join_selected(&mut self, cx: &mut Context<Self>) {
        let Some((parent, index)) = self.join_target() else {
            return;
        };
        let mut left = parent.clone();
        left.push(index - 1);
        let mut right = parent.clone();
        right.push(index);
        let label = format!("Join {}+{}", fmt_path(&left), fmt_path(&right));
        if self.edit(
            label,
            |t| t.join_blocks(parent.clone(), index).map(|_| ()),
            cx,
        ) {
            self.selection = Some(left);
        }
    }

    pub fn to_json_pretty(&self) -> String {
        self.doc.to_string_pretty().unwrap_or_default()
    }
}

pub fn fmt_path(path: &[usize]) -> String {
    let inner: Vec<String> = path.iter().map(usize::to_string).collect();
    format!("[{}]", inner.join(","))
}

pub fn node_type(node: &Node) -> &str {
    node.node_type.as_deref().unwrap_or("?")
}

pub fn is_unknown(node: &Node) -> bool {
    !KNOWN_TYPES.contains(&node_type(node))
}

/// Concatenated text of all descendant text nodes, truncated for labels.
pub fn text_preview(node: &Node, max: usize) -> String {
    let mut s = String::new();
    node.walk(&mut |n| {
        if let Some(t) = n.text.as_deref() {
            if s.chars().count() < max {
                s.push_str(t);
            }
        }
    });
    if s.chars().count() > max {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    } else {
        s
    }
}

/// Short row summary: `level=1 · "Title"`, `3 items`, `language=rust`.
pub fn node_summary(node: &Node) -> String {
    let ty = node_type(node);
    match ty {
        "text" => {
            let marks = node
                .marks
                .as_ref()
                .map(|m| {
                    m.iter()
                        .map(|m| m.mark_type.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            let t = text_preview(node, 32);
            if marks.is_empty() {
                format!("\"{t}\"")
            } else {
                format!("{marks} \"{t}\"")
            }
        }
        "heading" => format!(
            "level={} · \"{}\"",
            attr_str(node, "level"),
            text_preview(node, 24)
        ),
        "paragraph" => format!("\"{}\"", text_preview(node, 32)),
        "bulletList" | "orderedList" | "taskList" => format!("{} items", node.child_count()),
        "codeBlock" => format!("language={}", attr_str(node, "language")),
        "horizontalRule" | "hardBreak" => String::new(),
        _ => {
            let first = node.attrs.as_ref().and_then(|a| a.iter().next());
            let mut s = match first {
                Some((k, v)) => format!("{k}={}", short_value(v)),
                None => String::new(),
            };
            let n = node.child_count();
            if n > 0 {
                if !s.is_empty() {
                    s.push_str(" · ");
                }
                s.push_str(&format!("{n} child{}", if n == 1 { "" } else { "ren" }));
            }
            if is_unknown(node) {
                if !s.is_empty() {
                    s.push_str(" · ");
                }
                s.push_str("unknown");
            }
            s
        }
    }
}

fn attr_str(node: &Node, key: &str) -> String {
    node.attr(key)
        .map(short_value)
        .unwrap_or_else(|| "∅".into())
}

pub fn short_value(v: &Value) -> String {
    match v {
        Value::String(s) => format!("\"{s}\""),
        Value::Object(_) => "{ … }".into(),
        Value::Array(a) => format!("[{}]", a.len()),
        other => other.to_string(),
    }
}

// ---- keyboard navigation (see nav.rs) -------------------------------------------

impl DocumentState {
    /// Paths of the rows the tree currently shows, in document order.
    pub fn visible_paths(&self) -> Vec<Vec<usize>> {
        fn go(node: &Node, s: &DocumentState, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
            out.push(path.clone());
            if s.is_expanded(path) {
                for (i, c) in node.children().iter().enumerate() {
                    path.push(i);
                    go(c, s, path, out);
                    path.pop();
                }
            }
        }
        let mut out = Vec::new();
        go(self.root(), self, &mut Vec::new(), &mut out);
        out
    }

    /// ↑/↓: previous/next visible row. With no selection, starts at the ends.
    pub fn nav_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let rows = self.visible_paths();
        if rows.is_empty() {
            return;
        }
        let next = match self
            .selection
            .as_ref()
            .and_then(|p| rows.iter().position(|r| r == p))
        {
            Some(i) => (i as isize + delta).clamp(0, rows.len() as isize - 1) as usize,
            None if delta < 0 => rows.len() - 1,
            None => 0,
        };
        self.select(Some(rows[next].clone()), cx);
    }

    /// ←: collapse an expanded node, otherwise move to its parent.
    pub fn nav_out(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.selection.clone() else {
            return self.nav_step(1, cx);
        };
        let has_children = self.selected().is_some_and(|n| n.child_count() > 0);
        if has_children && self.is_expanded(&path) {
            self.expanded.remove(&path);
            cx.notify();
        } else if let Some((_, parent)) = path.split_last() {
            self.select(Some(parent.to_vec()), cx);
        }
    }

    /// →: expand a collapsed node, otherwise step into its first child.
    pub fn nav_in(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.selection.clone() else {
            return self.nav_step(1, cx);
        };
        if !self.selected().is_some_and(|n| n.child_count() > 0) {
            return;
        }
        if !self.is_expanded(&path) {
            self.expanded.insert(path);
            cx.notify();
        } else {
            let mut child = path;
            child.push(0);
            self.select(Some(child), cx);
        }
    }

    /// Home/End.
    pub fn nav_edge(&mut self, last: bool, cx: &mut Context<Self>) {
        let rows = self.visible_paths();
        let pick = if last { rows.last() } else { rows.first() };
        if let Some(p) = pick.cloned() {
            self.select(Some(p), cx);
        }
    }
}

// ---- saving ----------------------------------------------------------------------

impl DocumentState {
    /// Write the document (pretty JSON, key order preserved) to `path`; on
    /// success it becomes the current file and the dirty flag clears.
    pub fn save_to(&mut self, path: &Path, cx: &mut Context<Self>) {
        let result = self
            .doc
            .to_string_pretty()
            .map_err(anyhow::Error::from)
            .and_then(|mut json| {
                json.push('\n');
                std::fs::write(path, json).map_err(anyhow::Error::from)
            });
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        match result {
            Ok(()) => {
                self.file = Some(path.to_path_buf());
                self.dirty = false;
                self.last_change = Some(format!("Saved {name}").into());
            }
            Err(e) => {
                eprintln!("tiptap-studio: save {}: {e}", path.display());
                self.last_change = Some(format!("Save failed: {e}").into());
            }
        }
        cx.notify();
    }
}

// ---- file loading (files.rs) ------------------------------------------------------

impl DocumentState {
    /// Wrap an already parsed document; `files.rs` owns reading and error reporting.
    pub fn from_doc(doc: Document, file: Option<PathBuf>) -> Self {
        let mut s = Self {
            doc,
            file,
            selection: None,
            expanded: HashSet::new(),
            dirty: false,
            last_change: None,
            revision: 0,
            undo: Vec::new(),
            redo: Vec::new(),
        };
        s.expand_to_depth(1);
        s
    }

    /// Swap in a freshly loaded document and clear undo/redo. `keep_view`
    /// (reload) keeps expansion and a selection that still resolves; otherwise
    /// (open) both reset. The revision bump makes every observer re-derive.
    pub fn replace_doc(
        &mut self,
        doc: Document,
        file: Option<PathBuf>,
        keep_view: bool,
        cx: &mut Context<Self>,
    ) {
        let name = file
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        self.doc = doc;
        self.file = file;
        self.undo.clear();
        self.redo.clear();
        self.dirty = false;
        self.revision += 1;
        if keep_view {
            if let Some(p) = &self.selection {
                if self.doc.root().node_at(p).is_none() {
                    self.selection = None;
                }
            }
            self.last_change = Some(format!("Reloaded {name}").into());
        } else {
            self.selection = None;
            self.expanded.clear();
            self.expand_to_depth(1);
            self.last_change = Some(format!("Opened {name}").into());
        }
        cx.notify();
    }
}
