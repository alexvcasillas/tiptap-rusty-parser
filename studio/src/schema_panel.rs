//! Schema validation: load a JSON `Schema`, validate on document change,
//! surface `Violation`s in the toolbar, tree, inspector and a Problems drawer.
//!
//! `SchemaState` re-validates only when the document revision, the file, or
//! the schema changes (never per frame) and caches violations by target path:
//! the offending child for content violations, otherwise the node itself.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use tiptap_rusty_parser::{Node, Schema, Violation, ViolationKind};

use crate::diff::DiffState;
use crate::state::{fmt_path, node_type, DocumentState};
use crate::theme::Tokens;

const STARTER_KIT: &str = include_str!("../assets/tiptap-starter-kit.schema.json");
const STARTER_KIT_NAME: &str = "starter-kit";

gpui_kit::actions!(schema, [NextProblem, PrevProblem]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("f8", NextProblem, Some("Studio")),
        KeyBinding::new("shift-f8", PrevProblem, Some("Studio")),
    ]);
}

#[derive(Clone)]
pub enum Source {
    File(PathBuf),
    StarterKit,
}

impl Source {
    fn name(&self) -> SharedString {
        match self {
            Source::File(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().to_string().into())
                .unwrap_or_else(|| "schema".into()),
            Source::StarterKit => STARTER_KIT_NAME.into(),
        }
    }
}

/// One Problems row, message prebuilt on recompute.
pub struct ProblemRow {
    pub target: Vec<usize>,
    pub message: SharedString,
}

pub struct SchemaState {
    doc: Entity<DocumentState>,
    diff: Option<Entity<DiffState>>,
    schema: Option<(Schema, Source)>,
    pub error: Option<SharedString>,
    /// Violations by target path (see module docs).
    pub by_path: HashMap<Vec<usize>, Vec<Violation>>,
    /// Target paths with a violation, shared with the tree.
    paths: Rc<HashSet<Vec<usize>>>,
    /// Distinct targets in document order (F8 order).
    targets: Vec<Vec<usize>>,
    rows: Rc<Vec<ProblemRow>>,
    /// Violation count of diff B, when a B is loaded.
    pub b_issues: Option<usize>,
    seen: Option<(u64, Option<PathBuf>)>,
    seen_b: Option<SharedString>,
    pub menu_open: bool,
    pub drawer_open: bool,
    _subs: Vec<Subscription>,
}

impl SchemaState {
    pub fn new(doc: Entity<DocumentState>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&doc, |this, doc, cx| {
            let d = doc.read(cx);
            let key = (d.revision, d.file.clone());
            if this.seen.as_ref() != Some(&key) {
                this.seen = Some(key);
                this.recompute(cx);
            }
        });
        Self {
            doc,
            diff: None,
            schema: None,
            error: None,
            by_path: HashMap::new(),
            paths: Rc::new(HashSet::new()),
            targets: Vec::new(),
            rows: Rc::new(Vec::new()),
            b_issues: None,
            seen: None,
            seen_b: None,
            menu_open: false,
            drawer_open: true,
            _subs: vec![sub],
        }
    }

    /// Also count violations of diff B (recomputed when B or the schema changes).
    pub fn watch_diff(&mut self, diff: Entity<DiffState>, cx: &mut Context<Self>) {
        let sub = cx.observe(&diff, |this, diff, cx| {
            let name = diff.read(cx).has_b().then(|| diff.read(cx).b_name());
            if name != this.seen_b {
                this.seen_b = name;
                this.recompute_b(cx);
            }
        });
        self._subs.push(sub);
        self.diff = Some(diff);
        self.recompute_b(cx);
    }

    pub fn is_loaded(&self) -> bool {
        self.schema.is_some()
    }

    pub fn source_name(&self) -> Option<SharedString> {
        self.schema.as_ref().map(|(_, s)| s.name())
    }

    pub fn issue_count(&self) -> usize {
        self.rows.len()
    }

    pub fn paths(&self) -> Rc<HashSet<Vec<usize>>> {
        self.paths.clone()
    }

    pub fn rows(&self) -> Rc<Vec<ProblemRow>> {
        self.rows.clone()
    }

    pub fn can_reload(&self) -> bool {
        matches!(self.schema, Some((_, Source::File(_))))
    }

    // ---- loading ------------------------------------------------------------

    pub fn load_file(&mut self, path: &Path, cx: &mut Context<Self>) {
        let parsed = std::fs::read_to_string(path)
            .map_err(anyhow::Error::from)
            .and_then(|s| Schema::from_json_str(&s).map_err(anyhow::Error::from));
        match parsed {
            Ok(schema) => self.set(schema, Source::File(path.to_path_buf()), cx),
            Err(e) => {
                eprintln!("tiptap-studio: schema {}: {e}", path.display());
                self.schema = None;
                self.error = Some(format!("{}: {e}", path.display()).into());
                self.recompute(cx);
            }
        }
    }

    /// Load `path` as a schema if its JSON has a top-level `nodes` key.
    /// For a drop handler: returns false (and changes nothing) otherwise.
    #[allow(dead_code)]
    pub fn try_load_dropped(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        let is_schema = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .is_some_and(|v| v.get("nodes").is_some_and(|n| n.is_object()));
        if is_schema {
            self.load_file(path, cx);
        }
        is_schema
    }

    pub fn use_starter_kit(&mut self, cx: &mut Context<Self>) {
        match Schema::from_json_str(STARTER_KIT) {
            Ok(schema) => self.set(schema, Source::StarterKit, cx),
            Err(e) => self.error = Some(format!("bundled schema: {e}").into()),
        }
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if let Some((_, Source::File(p))) = &self.schema {
            let p = p.clone();
            self.load_file(&p, cx);
        }
    }

    pub fn unload(&mut self, cx: &mut Context<Self>) {
        self.schema = None;
        self.error = None;
        self.recompute(cx);
    }

    fn set(&mut self, schema: Schema, source: Source, cx: &mut Context<Self>) {
        self.schema = Some((schema, source));
        self.error = None;
        self.recompute(cx);
    }

    // ---- validation ---------------------------------------------------------

    fn recompute(&mut self, cx: &mut Context<Self>) {
        let doc = self.doc.clone();
        let root = doc.read(cx).root();
        let violations = match &self.schema {
            Some((schema, _)) => root.validate(schema),
            None => Vec::new(),
        };
        let mut by_path: HashMap<Vec<usize>, Vec<Violation>> = HashMap::new();
        let mut rows = Vec::with_capacity(violations.len());
        for v in violations {
            let target = target(&v);
            rows.push(ProblemRow {
                target: target.clone(),
                message: message(&v, root).into(),
            });
            by_path.entry(target).or_default().push(v);
        }
        // Pre-order: lexicographic order on index paths is document order.
        rows.sort_by(|a, b| a.target.cmp(&b.target));
        let mut targets: Vec<Vec<usize>> = by_path.keys().cloned().collect();
        targets.sort();
        self.paths = Rc::new(by_path.keys().cloned().collect());
        self.by_path = by_path;
        self.targets = targets;
        self.rows = Rc::new(rows);
        self.recompute_b(cx);
        cx.notify();
    }

    fn recompute_b(&mut self, cx: &mut Context<Self>) {
        let count = match (&self.schema, &self.diff) {
            (Some((schema, _)), Some(diff)) => {
                diff.read(cx).b_root().map(|b| b.validate(schema).len())
            }
            _ => None,
        };
        if count != self.b_issues {
            self.b_issues = count;
            cx.notify();
        }
    }

    /// F8 / shift-F8: select the next / previous problem after the selection
    /// (document order, wrapping).
    pub fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.targets.is_empty() {
            return;
        }
        let sel = self.doc.read(cx).selection.clone();
        let t = &self.targets;
        let pick = match (&sel, delta > 0) {
            (None, true) => t.first(),
            (None, false) => t.last(),
            (Some(s), true) => t.iter().find(|p| *p > s).or(t.first()),
            (Some(s), false) => t.iter().rev().find(|p| *p < s).or(t.last()),
        }
        .cloned();
        if let Some(p) = pick {
            self.doc.update(cx, |d, cx| d.select(Some(p), cx));
        }
    }

    /// Select a problem's node.
    pub fn select(&mut self, target: &[usize], cx: &mut Context<Self>) {
        let p = target.to_vec();
        self.doc.update(cx, |d, cx| d.select(Some(p), cx));
    }
}

/// Where a violation is shown: the offending child for content violations,
/// otherwise the node at `path`.
pub fn target(v: &Violation) -> Vec<usize> {
    let mut p = v.path.clone();
    if let Some(i) = v.child {
        p.push(i);
    }
    p
}

/// Plain-language sentence for a violation; code spans in backticks.
pub fn message(v: &Violation, root: &Node) -> String {
    let node = root.node_at(&v.path);
    let ty = node.map(node_type).unwrap_or("?");
    let mark_ty = v
        .mark
        .and_then(|i| node?.marks.as_ref()?.get(i))
        .map(|m| m.mark_type.as_str());
    let owner = match mark_ty {
        Some(m) => format!("Mark `{m}`"),
        None => format!("`{ty}`"),
    };
    match &v.kind {
        ViolationKind::MissingNodeType => "Node has no `type`".into(),
        ViolationKind::UnknownNodeType(t) => format!("Unknown node type `{t}`"),
        ViolationKind::DisallowedChild { parent, child } => {
            format!("`{parent}` cannot contain `{child}`")
        }
        ViolationKind::InvalidContent { parent, expr } => {
            let child = v.child.and_then(|i| node?.children().get(i)).map(node_type);
            match child {
                Some(c) => format!("`{parent}` cannot contain `{c}` here (expected `{expr}`)"),
                None if node.is_some_and(|n| n.child_count() == 0) => {
                    format!("`{parent}` is empty (expected `{expr}`)")
                }
                None => format!("`{parent}` needs more content (expected `{expr}`)"),
            }
        }
        ViolationKind::UnknownMark(m) => format!("Unknown mark `{m}`"),
        ViolationKind::DisallowedMark { node, mark } => {
            format!("Mark `{mark}` is not allowed on `{node}`")
        }
        ViolationKind::MissingAttr { key } => format!("{owner} requires attribute `{key}`"),
        ViolationKind::UnknownAttr { key } => {
            format!("{owner} does not allow attribute `{key}`")
        }
        #[allow(unreachable_patterns)]
        other => other.to_string(),
    }
}

/// Message text with backticks stripped and code spans brightened.
pub fn rich(msg: &str) -> StyledText {
    let mut text = String::with_capacity(msg.len());
    let mut highlights = Vec::new();
    let mut code_start = None;
    for ch in msg.chars() {
        if ch == '`' {
            match code_start.take() {
                Some(s) => highlights.push((
                    s..text.len(),
                    HighlightStyle {
                        color: Some(Tokens::moved()),
                        ..Default::default()
                    },
                )),
                None => code_start = Some(text.len()),
            }
        } else {
            text.push(ch);
        }
    }
    StyledText::new(text).with_highlights(highlights)
}

// ---- toolbar --------------------------------------------------------------

/// Toolbar `Schema` item: a button showing source and issue count, opening a
/// menu with Open… / StarterKit / Reload / Unload.
pub fn toolbar_item(schema: &Entity<SchemaState>, cx: &App) -> impl IntoElement {
    let s = schema.read(cx);
    let n = s.issue_count();
    let (label, icon): (SharedString, Icon) = match (s.source_name(), &s.error) {
        (Some(name), _) => (
            format!(
                "Schema · {} · {n} issue{}",
                short_name(&name),
                if n == 1 { "" } else { "s" }
            )
            .into(),
            if n == 0 {
                Icon::new(IconName::ShieldCheck).text_color(Tokens::added())
            } else {
                Icon::new(IconName::ShieldAlert).text_color(Tokens::removed())
            },
        ),
        (None, Some(_)) => (
            "Schema · error".into(),
            Icon::new(IconName::ShieldX).text_color(Tokens::removed()),
        ),
        (None, None) => ("Schema · none".into(), Icon::new(IconName::ShieldCheck)),
    };
    let trigger = Button::new("schema")
        .ghost()
        .small()
        .icon(icon.small())
        .label(label)
        .when_some(s.source_name(), |b, name| b.tooltip(name));
    let this = schema.clone();
    Popover::new("schema-menu")
        .trigger(trigger)
        .open(s.menu_open)
        .on_open_change({
            let this = this.clone();
            move |open, _, cx| {
                this.update(cx, |s, cx| {
                    s.menu_open = *open;
                    cx.notify();
                })
            }
        })
        .content(move |_, _, cx| menu(&this, cx))
}

/// Toolbar-sized schema name: no `.schema.json` / `.json`, at most 20 chars.
fn short_name(name: &str) -> String {
    let stem = name
        .strip_suffix(".schema.json")
        .or_else(|| name.strip_suffix(".json"))
        .unwrap_or(name);
    if stem.chars().count() > 20 {
        format!("{}…", stem.chars().take(19).collect::<String>())
    } else {
        stem.to_string()
    }
}

fn menu(this: &Entity<SchemaState>, cx: &App) -> Div {
    let mono = cx.theme().mono_font_family.clone();
    let s = this.read(cx);
    let mut list = div().flex().flex_col().gap(px(2.)).w(px(240.));
    if let Some(name) = s.source_name() {
        let n = s.issue_count();
        list = list.child(
            div()
                .px(px(8.))
                .pb(px(6.))
                .mb(px(4.))
                .border_b_1()
                .border_color(Tokens::rule())
                .font_family(mono.clone())
                .text_size(px(11.))
                .text_color(Tokens::muted())
                .child(format!(
                    "{name} · {n} issue{}",
                    if n == 1 { "" } else { "s" }
                )),
        );
    } else if let Some(err) = &s.error {
        list = list.child(
            div()
                .px(px(8.))
                .pb(px(6.))
                .mb(px(4.))
                .border_b_1()
                .border_color(Tokens::rule())
                .text_size(px(11.))
                .text_color(Tokens::removed())
                .child(err.clone()),
        );
    }
    let item = |id: &'static str, icon: IconName, label: &'static str, enabled: bool| {
        let this = this.clone();
        Button::new(id)
            .ghost()
            .small()
            .w_full()
            .justify_start()
            .icon(Icon::new(icon).small())
            .label(label)
            .disabled(!enabled)
            .on_click(move |_, _, cx: &mut App| {
                this.update(cx, |s, cx| {
                    s.menu_open = false;
                    match id {
                        "schema-reload" => s.reload(cx),
                        "schema-unload" => s.unload(cx),
                        "schema-starter" => s.use_starter_kit(cx),
                        _ => {}
                    }
                    cx.notify();
                });
                if id == "schema-open" {
                    open_dialog(this.clone(), cx);
                }
            })
    };
    let loaded = s.is_loaded();
    list = list
        .child(item(
            "schema-open",
            IconName::FolderOpen,
            if loaded {
                "Open another…"
            } else {
                "Open schema…"
            },
            true,
        ))
        .child(item(
            "schema-starter",
            IconName::Package,
            "Use StarterKit schema",
            true,
        ));
    if loaded {
        list = list
            .child(item(
                "schema-reload",
                IconName::RefreshCw,
                "Reload",
                s.can_reload(),
            ))
            .child(item("schema-unload", IconName::ShieldOff, "Unload", true));
    }
    list
}

/// `prompt_for_paths` for a `.json` schema (fails headless; use `--schema`).
pub fn open_dialog(schema: Entity<SchemaState>, cx: &mut App) {
    let rx = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Open schema".into()),
    });
    cx.spawn(async move |cx| match rx.await {
        Ok(Ok(Some(paths))) => {
            if let Some(p) = paths.into_iter().next() {
                schema.update(cx, |s, cx| s.load_file(&p, cx));
            }
        }
        Ok(Ok(None)) => {}
        Ok(Err(e)) => eprintln!("tiptap-studio: schema prompt failed: {e}"),
        Err(_) => {}
    })
    .detach();
}

// ---- inspector ------------------------------------------------------------

/// `SCHEMA` section for the selected node, when a schema is loaded.
pub fn inspector_section(schema: &SchemaState, path: &[usize]) -> Option<AnyElement> {
    let name = schema.source_name()?;
    let rows = schema.rows();
    let mine: Vec<&ProblemRow> = rows.iter().filter(|r| r.target == path).collect();
    let mut sec = crate::inspector::section("Schema", Some(name));
    if mine.is_empty() {
        sec = sec.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(12.))
                .text_color(Tokens::added())
                .child(Icon::new(IconName::CircleCheck).xsmall())
                .child("No issues on this node"),
        );
    } else {
        for r in mine {
            sec = sec.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(6.))
                    .text_size(px(12.))
                    .line_height(relative(1.45))
                    .text_color(Tokens::secondary())
                    .child(
                        div()
                            .flex_shrink_0()
                            .pt(px(2.))
                            .text_color(Tokens::removed())
                            .child(Icon::new(IconName::TriangleAlert).xsmall()),
                    )
                    .child(div().flex_1().min_w_0().child(rich(&r.message))),
            );
        }
    }
    Some(sec.into_any_element())
}

// ---- Problems drawer ------------------------------------------------------

const PROBLEM_ROW_H: f32 = 24.;
const DRAWER_MAX_ROWS: usize = 6;

pub struct ProblemsPanel {
    schema: Entity<SchemaState>,
    doc: Entity<DocumentState>,
    scroll: UniformListScrollHandle,
    last_selection: Option<Vec<usize>>,
    _subs: Vec<Subscription>,
}

impl ProblemsPanel {
    pub fn new(
        schema: Entity<SchemaState>,
        doc: Entity<DocumentState>,
        cx: &mut Context<Self>,
    ) -> Self {
        let s1 = cx.observe(&schema, |_, _, cx| cx.notify());
        let s2 = cx.observe(&doc, |this: &mut Self, doc, cx| {
            let sel = doc.read(cx).selection.clone();
            if sel != this.last_selection {
                if let Some(ix) = sel.as_ref().and_then(|p| {
                    this.schema
                        .read(cx)
                        .rows
                        .iter()
                        .position(|r| &r.target == p)
                }) {
                    this.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
                }
                this.last_selection = sel;
                cx.notify();
            }
        });
        Self {
            schema,
            doc,
            scroll: UniformListScrollHandle::new(),
            last_selection: None,
            _subs: vec![s1, s2],
        }
    }
}

impl Render for ProblemsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.schema.read(cx);
        if !s.is_loaded() {
            return div().id("problems");
        }
        let rows = s.rows();
        let open = s.drawer_open;
        let count = rows.len();
        let selection = self.doc.read(cx).selection.clone();
        let mono = cx.theme().mono_font_family.clone();
        let schema = self.schema.clone();
        let toggle = self.schema.clone();

        let header = div()
            .id("problems-header")
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(30.))
            .flex_shrink_0()
            .px(px(12.))
            .cursor_pointer()
            .text_size(px(11.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(Tokens::muted())
            .hover(|d| d.bg(Tokens::raised()))
            .on_click(move |_, _, cx| {
                toggle.update(cx, |s, cx| {
                    s.drawer_open = !s.drawer_open;
                    cx.notify();
                })
            })
            .child(
                Icon::new(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronUp
                })
                .xsmall(),
            )
            .child("PROBLEMS")
            .child(
                div()
                    .px(px(6.))
                    .rounded(px(4.))
                    .font_family(mono.clone())
                    .bg(if count == 0 {
                        Tokens::raised()
                    } else {
                        let mut c = Tokens::removed();
                        c.a = 0.18;
                        c
                    })
                    .text_color(if count == 0 {
                        Tokens::added()
                    } else {
                        Tokens::removed()
                    })
                    .child(count.to_string()),
            )
            .child(div().flex_1())
            .child(
                div()
                    .font_weight(FontWeight::NORMAL)
                    .text_color(Tokens::disabled())
                    .child("F8 next · ⇧F8 prev"),
            );

        let mut drawer = div()
            .id("problems")
            .flex()
            .flex_col()
            .flex_shrink_0()
            .bg(Tokens::panel())
            .border_t_1()
            .border_color(Tokens::border())
            .child(header);
        if open {
            if count == 0 {
                drawer = drawer.child(
                    div()
                        .px(px(14.))
                        .pb(px(10.))
                        .text_size(px(12.))
                        .text_color(Tokens::added())
                        .child("Document is valid against the schema"),
                );
            } else {
                let h = PROBLEM_ROW_H * count.min(DRAWER_MAX_ROWS) as f32 + 8.;
                drawer = drawer.child(
                    uniform_list("problem-rows", count, move |range, _, _| {
                        range
                            .map(|i| {
                                let r = &rows[i];
                                let selected = selection.as_deref() == Some(r.target.as_slice());
                                let target = r.target.clone();
                                let schema = schema.clone();
                                div()
                                    .id(ElementId::NamedInteger("problem".into(), i as u64))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .h(px(PROBLEM_ROW_H))
                                    .px(px(10.))
                                    .rounded(px(5.))
                                    .cursor_pointer()
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .text_size(px(12.))
                                    .text_color(Tokens::secondary())
                                    .hover(|d| d.bg(Tokens::raised()))
                                    .when(selected, |d| d.bg(Tokens::accent_fill()))
                                    .on_click(move |_, _, cx| {
                                        schema.update(cx, |s, cx| s.select(&target, cx))
                                    })
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_color(Tokens::removed())
                                            .child(Icon::new(IconName::TriangleAlert).xsmall()),
                                    )
                                    .child(
                                        div()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .child(rich(&r.message)),
                                    )
                                    .child(div().flex_1())
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .font_family(mono.clone())
                                            .text_size(px(11.))
                                            .text_color(Tokens::disabled())
                                            .child(fmt_path(&r.target)),
                                    )
                            })
                            .collect()
                    })
                    .track_scroll(&self.scroll)
                    .h(px(h))
                    .px(px(6.))
                    .pb(px(6.)),
                );
            }
        }
        drawer
    }
}

#[cfg(test)]
mod tests {
    // No glob import: `gpui_kit::*` would shadow `#[test]`.
    use super::{message, STARTER_KIT};
    use tiptap_rusty_parser::{Document, Schema, ViolationKind};

    #[test]
    fn starter_kit_flags_only_callout_in_sample() {
        let schema = Schema::from_json_str(STARTER_KIT).unwrap();
        let doc = Document::from_json_str(include_str!("../assets/sample.json")).unwrap();
        let v = doc.validate(&schema);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].kind, ViolationKind::UnknownNodeType("callout".into()));
        assert_eq!(message(&v[0], doc.root()), "Unknown node type `callout`");
    }

    #[test]
    fn messages_name_the_owner() {
        let schema = Schema::from_json_str(STARTER_KIT).unwrap();
        let doc = Document::from_json_str(
            r#"{"type":"doc","content":[
              {"type":"paragraph","content":[{"type":"heading","attrs":{"level":1}}]},
              {"type":"heading","attrs":{"level":2,"color":"red"},"content":[
                {"type":"text","text":"x","marks":[{"type":"glow"},{"type":"link","attrs":{"href":"u","foo":1}}]}
              ]},
              {"type":"bulletList"}
            ]}"#,
        )
        .unwrap();
        let msgs: Vec<String> = doc
            .validate(&schema)
            .iter()
            .map(|v| message(v, doc.root()))
            .collect();
        for want in [
            "`paragraph` cannot contain `heading` here (expected `inline*`)",
            "`heading` does not allow attribute `color`",
            "Unknown mark `glow`",
            "Mark `link` does not allow attribute `foo`",
            "`bulletList` is empty (expected `listItem+`)",
        ] {
            assert!(msgs.iter().any(|m| m == want), "{want} not in {msgs:?}");
        }
    }
}
