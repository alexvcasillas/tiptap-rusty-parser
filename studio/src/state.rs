//! Shared document entity: parser `Document` + selection + expansion + undo/redo.
//!
//! Every mutation goes through [`DocumentState::edit`], which runs a `Transform`,
//! keeps its `Change` log and stores the inverse for undo. Panels never touch
//! `&mut Node` directly.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui_kit::*;
use serde_json::Value;
use tiptap_rusty_parser::{invert, Change, Document, Node, Transform};

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
        self.last_change = Some(label);
        if let Some(p) = &self.selection {
            if self.root().node_at(p).is_none() {
                self.selection = None;
            }
        }
        cx.notify();
    }

    // ---- convenience edits used by panels ---------------------------------------

    /// Used by the typed attribute editors (next phase).
    #[allow(dead_code)]
    pub fn set_attr(&mut self, path: Vec<usize>, key: &str, value: Value, cx: &mut Context<Self>) {
        let label = format!("SetAttr {} .{key}", fmt_path(&path));
        let key = key.to_string();
        self.edit(label, |t| t.set_attr(path, key, value).map(|_| ()), cx);
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
            marks.push(tiptap_rusty_parser::Mark::new(mark_type));
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

    pub fn move_selected(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(path) = self.selection.clone() else {
            return;
        };
        let Some((&from, parent)) = path.split_last() else {
            return;
        };
        let parent = parent.to_vec();
        let count = self
            .root()
            .node_at(&parent)
            .map(Node::child_count)
            .unwrap_or(0);
        let to = from as isize + delta;
        if to < 0 || to as usize >= count {
            return;
        }
        let to = to as usize;
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

    pub fn lift_selected(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.selection.clone() else {
            return;
        };
        let label = format!("Lift {}", fmt_path(&path));
        self.edit(label, |t| t.lift(path).map(|_| ()), cx);
    }

    pub fn wrap_selected(&mut self, wrapper: &str, cx: &mut Context<Self>) {
        let Some(path) = self.selection.clone() else {
            return;
        };
        let label = format!("Wrap {} in {wrapper}", fmt_path(&path));
        let wrapper = wrapper.to_string();
        self.edit(label, |t| t.wrap(path, wrapper, None).map(|_| ()), cx);
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
