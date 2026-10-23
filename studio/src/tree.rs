//! Left pane: virtualized node tree with chevrons, summaries and selection.

use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::find::FindState;
use crate::schema_panel::SchemaState;
use crate::state::{fmt_path, is_unknown, node_summary, node_type, DocumentState};
use crate::theme::Tokens;

pub struct Row {
    pub path: Vec<usize>,
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
    pub is_text: bool,
    pub is_unknown: bool,
    pub type_name: SharedString,
    pub marks: SharedString,
    pub summary: SharedString,
    /// Has a schema violation (set by `TreePanel` from `SchemaState`).
    pub problem: bool,
}

/// A visible tree row before labelling. Labels (type, marks, summary) are
/// built only for the rows on screen, at render time.
struct TreeRow {
    path: Vec<usize>,
    expanded: bool,
}

/// What the flattened rows depend on: document structure, expansion version,
/// find match set (by pointer) and the "Only matches" filter. Selection-only
/// notifies leave it unchanged, so they skip the flatten.
#[derive(Clone, Copy, PartialEq)]
struct RowsKey {
    revision: u64,
    expanded: u64,
    matches: usize,
    filtering: bool,
}

pub struct TreePanel {
    state: Entity<DocumentState>,
    rows: Rc<Vec<TreeRow>>,
    rows_key: Option<RowsKey>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    last_selection: Option<Vec<usize>>,
    find: Entity<FindState>,
    /// Paths with a schema violation (see `schema_panel.rs`).
    problems: Rc<HashSet<Vec<usize>>>,
    _subs: Vec<Subscription>,
}

impl TreePanel {
    /// Decorate rows that have schema violations; re-flags on schema change.
    pub fn set_schema(&mut self, schema: Entity<SchemaState>, cx: &mut Context<Self>) {
        let sub = cx.observe(&schema, |this, schema, cx| {
            this.problems = schema.read(cx).paths();
            this.rebuild(cx);
            cx.notify();
        });
        self._subs.push(sub);
    }

    pub fn new(state: Entity<DocumentState>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&state, |this, _, cx| {
            this.rebuild(cx);
            cx.notify();
        });
        let focus = cx.focus_handle();
        let find = crate::find::find_state(&state, cx);
        find.update(cx, |f, _| f.return_focus = Some(focus.clone()));
        let find_sub = cx.observe(&find, |this, _, cx| {
            this.rebuild(cx);
            cx.notify();
        });
        let mut this = Self {
            state: state.clone(),
            rows: Rc::new(Vec::new()),
            rows_key: None,
            focus,
            scroll: UniformListScrollHandle::new(),
            last_selection: None,
            find,
            problems: Rc::default(),
            _subs: vec![sub, find_sub],
        };
        this.rebuild(cx);
        this
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Flatten visible paths when the document, expansion or find set changed
    /// (never on selection-only notifies, never per frame), then keep the
    /// selection in view.
    fn rebuild(&mut self, cx: &App) {
        let state = self.state.read(cx);
        let find = self.find.read(cx);
        let key = RowsKey {
            revision: state.structure_rev,
            expanded: state.expanded.version(),
            matches: Rc::as_ptr(&find.set) as usize,
            filtering: find.filtering(),
        };
        if self.rows_key != Some(key) {
            let _p = crate::profile::span("tree flatten");
            self.rows_key = Some(key);
            let mut rows = Vec::new();
            if find.filtering() {
                // "Only matches": matches plus their ancestors, ancestors expanded.
                let (set, anc) = (&find.set, &find.ancestors);
                flatten_paths(
                    state.root(),
                    &|p| anc.contains(p),
                    &mut Vec::new(),
                    &mut rows,
                );
                rows.retain(|r| set.contains(&r.path) || anc.contains(&r.path));
            } else {
                let is_expanded = |p: &[usize]| state.is_expanded(p);
                flatten_paths(state.root(), &is_expanded, &mut Vec::new(), &mut rows);
            }
            self.rows = Rc::new(rows);
        }
        // Keep the selected row in view whenever the selection moves
        // (keyboard, preview click, structural edit). Rows are in document
        // order = lexicographic path order, so binary search.
        if state.selection != self.last_selection {
            self.last_selection = state.selection.clone();
            if let Some(Ok(ix)) = state.selection.as_ref().map(|p| {
                self.rows
                    .binary_search_by(|r| r.path.as_slice().cmp(p.as_slice()))
            }) {
                self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
            }
        }
    }
}

/// Visible paths only (no labels): the cheap part of [`flatten`].
fn flatten_paths(
    node: &tiptap_rusty_parser::Node,
    is_expanded: &dyn Fn(&[usize]) -> bool,
    path: &mut Vec<usize>,
    out: &mut Vec<TreeRow>,
) {
    let expanded = is_expanded(path);
    out.push(TreeRow {
        path: path.clone(),
        expanded,
    });
    if expanded {
        for (i, c) in node.children().iter().enumerate() {
            path.push(i);
            flatten_paths(c, is_expanded, path, out);
            path.pop();
        }
    }
}

/// Flatten the visible rows of `root`, descending only where `is_expanded`.
/// Shared by the Inspect tree and both Diff trees.
pub fn flatten(
    root: &tiptap_rusty_parser::Node,
    is_expanded: &dyn Fn(&[usize]) -> bool,
) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut path = Vec::new();
    flatten_into(root, is_expanded, &mut path, &mut rows);
    rows
}

fn flatten_into(
    node: &tiptap_rusty_parser::Node,
    is_expanded: &dyn Fn(&[usize]) -> bool,
    path: &mut Vec<usize>,
    out: &mut Vec<Row>,
) {
    let expanded = is_expanded(path);
    out.push(row_for(node, path, expanded));
    if expanded {
        for (i, c) in node.children().iter().enumerate() {
            path.push(i);
            flatten_into(c, is_expanded, path, out);
            path.pop();
        }
    }
}

/// The labelled row for `node` at `path`.
fn row_for(node: &tiptap_rusty_parser::Node, path: &[usize], expanded: bool) -> Row {
    let ty = node_type(node);
    let is_text = ty == "text";
    let (type_name, marks, summary) = if is_text {
        let marks = node
            .marks
            .as_ref()
            .map(|m| {
                m.iter()
                    .map(|m| m.mark_type.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        let text = crate::state::text_preview(node, 32);
        (ty.to_string(), marks, format!("\"{text}\""))
    } else {
        (ty.to_string(), String::new(), node_summary(node))
    };
    Row {
        path: path.to_vec(),
        depth: path.len(),
        has_children: node.child_count() > 0,
        expanded,
        is_text,
        is_unknown: is_unknown(node),
        type_name: type_name.into(),
        marks: marks.into(),
        summary: summary.into(),
        problem: false,
    }
}

impl Render for TreePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows.clone();
        let state = self.state.clone();
        let focus = self.focus.clone();
        let focused = self.focus.contains_focused(window, cx);
        let (selection, footer) = {
            let s = state.read(cx);
            let footer: SharedString = match s.selection.as_deref() {
                Some(p) => {
                    let children = s.selected().map(|n| n.child_count()).unwrap_or(0);
                    format!(
                        "path {} · depth {} · {} child{}",
                        fmt_path(p),
                        p.len(),
                        children,
                        if children == 1 { "" } else { "ren" }
                    )
                    .into()
                }
                None => "no selection".into(),
            };
            (s.selection.clone(), footer)
        };
        let mono = cx.theme().mono_font_family.clone();
        let count = rows.len();
        let find = self.find.clone();
        let state_rows = state.clone();
        let on_select: RowFn = Rc::new({
            let state = state.clone();
            move |path, window, cx| {
                window.focus(&focus, cx);
                state.update(cx, |s, cx| s.select(Some(path.to_vec()), cx))
            }
        });
        let on_toggle: RowFn = Rc::new({
            let state = state.clone();
            move |path, _, cx| state.update(cx, |s, cx| s.toggle_expanded(path.to_vec(), cx))
        });

        crate::nav::attach(div().id("tree-pane"), &state)
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .w_full()
            .flex_shrink_0()
            .h_full()
            .bg(Tokens::panel())
            .border_r_1()
            .border_color(Tokens::border())
            .child(
                pane_header("Document")
                    .when(focused, |h| h.text_color(Tokens::accent()))
                    .child(
                        div()
                            .flex()
                            .gap(px(2.))
                            .child(icon_button("collapse-all", IconName::ChevronDown, {
                                let state = state.clone();
                                move |_, _, cx| state.update(cx, |s, cx| s.collapse_all(cx))
                            }))
                            .child(icon_button("expand-all", IconName::ChevronRight, {
                                let state = state.clone();
                                move |_, _, cx| state.update(cx, |s, cx| s.expand_all(cx))
                            })),
                    ),
            )
            .child(
                uniform_list("tree-rows", count, {
                    let problems = self.problems.clone();
                    move |range, _window, cx| {
                        let find = find.read(cx);
                        let root = state_rows.read(cx).root();
                        range
                            .filter_map(|i| {
                                let r = &rows[i];
                                let mut row = row_for(root.node_at(&r.path)?, &r.path, r.expanded);
                                // Schema violations (schema_panel.rs), visible rows only.
                                row.problem = problems.contains(&row.path);
                                let row = &row;
                                let hit = find.row_match(&row.path, row.expanded);
                                let selected = selection.as_deref() == Some(row.path.as_slice());
                                Some(
                                    render_row(
                                        "tree-row",
                                        i,
                                        row,
                                        selected,
                                        &Deco::NONE,
                                        &mono,
                                        &on_select,
                                        &on_toggle,
                                    )
                                    .map(|r| crate::find::decorate_row(r, hit, selected)),
                                )
                            })
                            .collect::<Vec<_>>()
                    }
                })
                .track_scroll(&self.scroll)
                .flex_1()
                .p(px(6.)),
            )
            .child(
                div()
                    .px(px(12.))
                    .py(px(8.))
                    .border_t_1()
                    .border_color(Tokens::border())
                    .text_size(px(11.))
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(Tokens::muted())
                    .child(footer),
            )
    }
}

/// Row click / chevron callback: `(path, window, cx)`.
pub type RowFn = Rc<dyn Fn(&[usize], &mut Window, &mut App)>;

/// Diff decoration for a tree row: a row tint (14% fill + 6px pip) and/or a
/// faint ancestor pip, plus struck-through (removed) or added type styling.
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Deco {
    pub tint: Option<Hsla>,
    pub pip: Option<Hsla>,
    pub strike: bool,
    pub added: bool,
}

impl Deco {
    pub const NONE: Deco = Deco {
        tint: None,
        pip: None,
        strike: false,
        added: false,
    };

    pub fn is_none(&self) -> bool {
        self.tint.is_none() && self.pip.is_none()
    }
}

/// One 26px tree row. `id_prefix` keeps element ids unique across several
/// trees in one window; `on_select`/`on_toggle` receive the row path.
#[allow(clippy::too_many_arguments)]
pub fn render_row(
    id_prefix: &'static str,
    i: usize,
    row: &Row,
    selected: bool,
    deco: &Deco,
    mono: &SharedString,
    on_select: &RowFn,
    on_toggle: &RowFn,
) -> Stateful<Div> {
    let path = row.path.clone();
    let chevron_path = row.path.clone();
    let on_select = on_select.clone();
    let on_toggle = on_toggle.clone();
    let type_color = if deco.strike {
        Tokens::removed()
    } else if deco.added {
        Tokens::added()
    } else if row.is_unknown {
        Tokens::unknown()
    } else if row.is_text {
        Tokens::muted()
    } else {
        Tokens::foreground()
    };
    let fill = deco.tint.map(|mut c| {
        c.a = 0.14;
        c
    });
    let pip = deco.tint.or(deco.pip.map(|mut c| {
        c.a = 0.45;
        c
    }));

    div()
        .id(ElementId::NamedInteger(id_prefix.into(), i as u64))
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(26.))
        .pr(px(8.))
        .pl(px(16. * row.depth as f32))
        .rounded(px(5.))
        .cursor_pointer()
        .font_family(mono.clone())
        .text_size(px(12.))
        .whitespace_nowrap()
        .overflow_hidden()
        .when_some(fill, |s, c| s.bg(c))
        .hover(|s| s.bg(Tokens::raised()))
        .when(selected, |s| {
            s.bg(Tokens::accent_fill())
                .border_1()
                .border_color(Tokens::accent())
        })
        .on_click(move |_, window, cx| on_select(&path, window, cx))
        .child(
            div()
                .id(ElementId::NamedInteger("tree-chevron".into(), i as u64))
                .size(px(16.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .text_color(Tokens::muted())
                .when(row.has_children, |d| {
                    d.cursor_pointer()
                        .child(
                            Icon::new(if row.expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .xsmall(),
                        )
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            on_toggle(&chevron_path, window, cx);
                        })
                }),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_color(type_color)
                .when(deco.strike, |d| d.line_through())
                .when(row.problem, |d| {
                    d.underline()
                        .text_decoration_color(Tokens::removed())
                        .text_decoration_wavy()
                })
                .child(row.type_name.clone()),
        )
        // schema violation marker, right after the type: rows are wider than
        // the pane, so a row-end marker would be clipped. Separate from diff
        // pips and find matches.
        .when(row.problem, |d| {
            d.child(
                div()
                    .flex_shrink_0()
                    .text_color(Tokens::removed())
                    .child(Icon::new(IconName::TriangleAlert).xsmall()),
            )
        })
        .when(!row.marks.is_empty(), |d| {
            d.child(
                div()
                    .flex_shrink_0()
                    .text_color(Tokens::moved())
                    .child(row.marks.clone()),
            )
        })
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .text_color(Tokens::muted())
                .child(row.summary.clone()),
        )
        .when_some(pip, |d, c| {
            d.child(div().flex_1())
                .child(div().size(px(6.)).flex_shrink_0().rounded(px(3.)).bg(c))
        })
}

/// 34px uppercase section header shared by every pane.
pub fn pane_header(title: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .h(px(34.))
        .flex_shrink_0()
        .px(px(12.))
        .border_b_1()
        .border_color(Tokens::border())
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(Tokens::muted())
        .child(div().child(title.into().to_uppercase()))
}

pub fn icon_button(
    id: &'static str,
    icon: IconName,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(20.))
        .rounded(px(4.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(Tokens::muted())
        .hover(|s| s.bg(Tokens::raised()).text_color(Tokens::foreground()))
        .on_click(on_click)
        .child(Icon::new(icon).xsmall())
}
