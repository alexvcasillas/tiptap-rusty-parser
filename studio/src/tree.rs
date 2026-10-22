//! Left pane: virtualized node tree with chevrons, summaries and selection.

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;

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
}

pub struct TreePanel {
    state: Entity<DocumentState>,
    rows: Rc<Vec<Row>>,
    _subs: Vec<Subscription>,
}

impl TreePanel {
    pub fn new(state: Entity<DocumentState>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&state, |this, state, cx| {
            this.rebuild(state.read(cx));
            cx.notify();
        });
        let mut this = Self {
            state: state.clone(),
            rows: Rc::new(Vec::new()),
            _subs: vec![sub],
        };
        this.rebuild(state.read(cx));
        this
    }

    /// Flatten visible nodes. Runs on document/selection change, never per frame.
    fn rebuild(&mut self, state: &DocumentState) {
        let mut rows = Vec::new();
        let mut path = Vec::new();
        flatten(state.root(), state, &mut path, &mut rows);
        self.rows = Rc::new(rows);
    }
}

fn flatten(
    node: &tiptap_rusty_parser::Node,
    state: &DocumentState,
    path: &mut Vec<usize>,
    out: &mut Vec<Row>,
) {
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
    let expanded = state.is_expanded(path);
    out.push(Row {
        path: path.clone(),
        depth: path.len(),
        has_children: node.child_count() > 0,
        expanded,
        is_text,
        is_unknown: is_unknown(node),
        type_name: type_name.into(),
        marks: marks.into(),
        summary: summary.into(),
    });
    if expanded {
        for (i, c) in node.children().iter().enumerate() {
            path.push(i);
            flatten(c, state, path, out);
            path.pop();
        }
    }
}

impl Render for TreePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows.clone();
        let state = self.state.clone();
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

        div()
            .flex()
            .flex_col()
            .w(px(280.))
            .flex_shrink_0()
            .h_full()
            .bg(Tokens::panel())
            .border_r_1()
            .border_color(Tokens::border())
            .child(
                pane_header("Document").child(
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
                uniform_list("tree-rows", count, move |range, _window, _cx| {
                    range
                        .map(|i| {
                            let row = &rows[i];
                            let selected = selection.as_deref() == Some(row.path.as_slice());
                            render_row(i, row, selected, &mono, &state)
                        })
                        .collect()
                })
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

fn render_row(
    i: usize,
    row: &Row,
    selected: bool,
    mono: &SharedString,
    state: &Entity<DocumentState>,
) -> impl IntoElement {
    let path = row.path.clone();
    let chevron_path = row.path.clone();
    let select_state = state.clone();
    let chevron_state = state.clone();
    let type_color = if row.is_unknown {
        Tokens::unknown()
    } else if row.is_text {
        Tokens::muted()
    } else {
        Tokens::foreground()
    };

    div()
        .id(ElementId::NamedInteger("tree-row".into(), i as u64))
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
        .hover(|s| s.bg(Tokens::raised()))
        .when(selected, |s| {
            s.bg(Tokens::accent_fill())
                .border_1()
                .border_color(Tokens::accent())
        })
        .on_click(move |_, _, cx| {
            select_state.update(cx, |s, cx| s.select(Some(path.clone()), cx));
        })
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
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            chevron_state
                                .update(cx, |s, cx| s.toggle_expanded(chevron_path.clone(), cx));
                        })
                }),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_color(type_color)
                .child(row.type_name.clone()),
        )
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
