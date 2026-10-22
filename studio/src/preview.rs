//! Center pane: read-only rendered preview. Click a block to select it.
//! Unknown node types render as a dashed amber block and stay selectable.
//!
//! Layout rule: a block's frame adds NO padding or border. Selection and
//! hover outlines are drawn by an absolutely-positioned overlay, so nesting
//! depth never shifts text and list markers stay on their item's first line.

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use tiptap_rusty_parser::Node;

use crate::state::{fmt_path, is_unknown, node_type, DocumentState};
use crate::theme::Tokens;
use crate::tree::pane_header;

const BODY_SIZE: f32 = 15.;
const BODY_LINE: f32 = 1.55;
const LIST_TYPES: &[&str] = &["bulletList", "orderedList", "taskList"];

pub struct PreviewPanel {
    state: Entity<DocumentState>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    last_selection: Option<Vec<usize>>,
    _subs: Vec<Subscription>,
}

impl PreviewPanel {
    pub fn new(state: Entity<DocumentState>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&state, |this, state, cx| {
            let sel = state.read(cx).selection.clone();
            if sel != this.last_selection {
                // Reveal the top-level block that holds the selection.
                if let Some(&top) = sel.as_ref().and_then(|p| p.first()) {
                    this.scroll.scroll_to_item(top);
                }
                this.last_selection = sel;
            }
            cx.notify();
        });
        Self {
            state,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            last_selection: None,
            _subs: vec![sub],
        }
    }
}

struct Ctx<'a> {
    state: &'a Entity<DocumentState>,
    root: &'a Node,
    selection: Option<&'a [usize]>,
    mono: SharedString,
}

impl Render for PreviewPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        let focused = self.focus.contains_focused(window, cx);
        let focus = self.focus.clone();
        let state = self.state.clone();
        let s = self.state.read(cx);
        let ctx = Ctx {
            state: &state,
            root: s.root(),
            selection: s.selection.as_deref(),
            mono,
        };
        let mut path = Vec::new();
        let blocks: Vec<AnyElement> = s
            .root()
            .children()
            .iter()
            .enumerate()
            .map(|(i, child)| {
                path.push(i);
                // Direct children of the scroll container, so the scroll
                // handle can reveal them by index.
                let el = div()
                    .w_full()
                    .max_w(px(680.))
                    .child(block(child, &mut path, &ctx))
                    .into_any_element();
                path.pop();
                el
            })
            .collect();

        crate::nav::attach(div().id("preview-pane"), &state)
            .track_focus(&self.focus)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.focus(&focus, cx)
            })
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(Tokens::background())
            .child(
                pane_header("Preview")
                    .when(focused, |h| h.text_color(Tokens::accent()))
                    .child(
                        div()
                            .font_weight(FontWeight::NORMAL)
                            .text_color(Tokens::disabled())
                            .child("read-only · click to select · arrows to navigate"),
                    ),
            )
            .child(
                div()
                    .id("preview-scroll")
                    .track_scroll(&self.scroll)
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(14.))
                    .px(px(56.))
                    .py(px(40.))
                    .text_size(px(BODY_SIZE))
                    .line_height(relative(BODY_LINE))
                    .text_color(Tokens::foreground())
                    .children(blocks),
            )
    }
}

/// A block whose children are inline (text, hardBreak…) or none at all.
/// Only these show a hover outline; containers would light up as a stack.
fn is_textblock(node: &Node) -> bool {
    node.children()
        .iter()
        .all(|c| c.text.is_some() || c.child_count() == 0 && node_type(c) == "hardBreak")
}

fn block(node: &Node, path: &mut Vec<usize>, ctx: &Ctx) -> AnyElement {
    let ty = node_type(node);
    let body: AnyElement = match ty {
        "heading" => {
            let level = node.attr("level").and_then(|v| v.as_u64()).unwrap_or(1);
            let size = match level {
                1 => 30.,
                2 => 21.,
                3 => 18.,
                _ => 16.,
            };
            div()
                .text_size(px(size))
                .line_height(relative(1.25))
                .font_weight(FontWeight::SEMIBOLD)
                .child(inline(node, ctx))
                .into_any_element()
        }
        "paragraph" => div().child(inline(node, ctx)).into_any_element(),
        t if LIST_TYPES.contains(&t) => list(node, path, ctx).into_any_element(),
        "blockquote" => div()
            .pl(px(16.))
            .border_l_3()
            .border_color(Tokens::input_border())
            .text_color(Tokens::secondary())
            .italic()
            .child(children(node, path, ctx, 10.))
            .into_any_element(),
        "horizontalRule" => div()
            .py(px(6.))
            .child(div().h(px(1.)).bg(Tokens::rule()))
            .into_any_element(),
        "codeBlock" => div()
            .px(px(16.))
            .py(px(14.))
            .rounded(px(8.))
            .bg(Tokens::code_bg())
            .border_1()
            .border_color(Tokens::border())
            .font_family(ctx.mono.clone())
            .text_size(px(13.))
            .line_height(relative(1.5))
            .text_color(Tokens::secondary())
            .child(crate::state::text_preview(node, usize::MAX))
            .into_any_element(),
        "image" => div()
            .px(px(12.))
            .py(px(24.))
            .rounded(px(6.))
            .border_1()
            .border_dashed()
            .border_color(Tokens::input_border())
            .text_color(Tokens::muted())
            .text_size(px(12.))
            .font_family(ctx.mono.clone())
            .child(format!(
                "image {}",
                node.attr("src").and_then(|v| v.as_str()).unwrap_or("")
            ))
            .into_any_element(),
        "hardBreak" => div().h(px(8.)).into_any_element(),
        _ if is_unknown(node) => {
            let first_attr = node
                .attrs
                .as_ref()
                .and_then(|a| a.iter().next())
                .map(|(k, v)| format!(" · {k}={}", crate::state::short_value(v)))
                .unwrap_or_default();
            div()
                .px(px(16.))
                .py(px(14.))
                .rounded(px(6.))
                .border_1()
                .border_dashed()
                .border_color(Tokens::unknown())
                .bg(Tokens::unknown_fill())
                .child(
                    div()
                        .mb(px(6.))
                        .font_family(ctx.mono.clone())
                        .text_size(px(10.))
                        .line_height(px(14.))
                        .text_color(Tokens::unknown())
                        .child(format!("{}{first_attr}", ty.to_uppercase())),
                )
                .child(children(node, path, ctx, 10.))
                .into_any_element()
        }
        // known container we have no special style for (listItem outside a
        // list, table…)
        _ => children(node, path, ctx, 10.).into_any_element(),
    };
    frame(node, path, ctx, body)
}

/// Click target + selection/hover overlay around a block. Adds no layout.
fn frame(node: &Node, path: &[usize], ctx: &Ctx, body: impl IntoElement) -> AnyElement {
    let selected = ctx.selection == Some(path);
    let hoverable = is_textblock(node);
    let group: SharedString = format!("blk{}", fmt_path(path)).into();
    let id_path = path.to_vec();
    let state = ctx.state.clone();

    div()
        .id(ElementId::Name(group.clone()))
        .group(group.clone())
        .relative()
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            state.update(cx, |s, cx| s.select(Some(id_path.clone()), cx));
        })
        .child(body)
        .when(selected || hoverable, |d| {
            d.child(
                div()
                    .absolute()
                    .top(px(-4.))
                    .bottom(px(-4.))
                    .left(px(-8.))
                    .right(px(-8.))
                    .rounded(px(6.))
                    .border_1()
                    .when(selected, |o| o.border_color(Tokens::accent()))
                    .when(!selected, |o| {
                        o.border_dashed()
                            .border_color(transparent_black())
                            .group_hover(group.clone(), |s| s.border_color(Tokens::ghost()))
                    }),
            )
        })
        .when(selected, |d| {
            d.child(
                div()
                    .absolute()
                    .top(px(-12.))
                    .right(px(0.))
                    .px(px(6.))
                    .rounded(px(4.))
                    .bg(Tokens::accent())
                    .text_color(white())
                    .text_size(px(10.))
                    .line_height(px(16.))
                    .font_family(ctx.mono.clone())
                    .child(format!("{} · {}", node_type(node), fmt_path(path))),
            )
        })
        .into_any_element()
}

/// How many list nodes enclose `path` (0 for a top-level list).
fn list_depth(root: &Node, path: &[usize]) -> usize {
    let mut n = root;
    let mut depth = 0;
    for &i in &path[..path.len().saturating_sub(1)] {
        let Some(c) = n.child(i) else { break };
        if LIST_TYPES.contains(&node_type(c)) {
            depth += 1;
        }
        n = c;
    }
    depth
}

enum Marker {
    Text(SharedString),
    Check(bool),
}

fn list(node: &Node, path: &mut Vec<usize>, ctx: &Ctx) -> Div {
    let ty = node_type(node);
    let start = node.attr("start").and_then(|v| v.as_u64()).unwrap_or(1);
    let bullet = ["•", "◦", "▪"][list_depth(ctx.root, path) % 3];
    let mut d = div().flex().flex_col().gap(px(4.));
    for (i, item) in node.children().iter().enumerate() {
        let marker = match ty {
            "orderedList" => Marker::Text(format!("{}.", start + i as u64).into()),
            "taskList" => Marker::Check(
                item.attr("checked")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            ),
            _ => Marker::Text(bullet.into()),
        };
        path.push(i);
        d = d.child(list_item(item, path, ctx, marker));
        path.pop();
    }
    d
}

/// Marker column + content column. The marker inherits the body text style,
/// so its line box is the content's first line box and they share a baseline.
fn list_item(node: &Node, path: &mut Vec<usize>, ctx: &Ctx, marker: Marker) -> AnyElement {
    let line = px(BODY_SIZE * BODY_LINE);
    let marker_el = match marker {
        Marker::Text(t) => div()
            .w(px(28.))
            .flex_shrink_0()
            .flex()
            .justify_end()
            .pr(px(10.))
            .text_color(Tokens::muted())
            .child(t),
        Marker::Check(checked) => div()
            .w(px(28.))
            .h(line)
            .flex_shrink_0()
            .flex()
            .items_center()
            .child(
                div()
                    .size(px(15.))
                    .rounded(px(4.))
                    .border_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_color(if checked {
                        Tokens::accent()
                    } else {
                        Tokens::input_border()
                    })
                    .when(checked, |b| {
                        b.bg(Tokens::accent())
                            .text_color(white())
                            .child(Icon::new(IconName::Check).xsmall())
                    }),
            ),
    };
    let body = div().flex().items_start().child(marker_el).child(
        div()
            .flex_1()
            .min_w_0()
            .child(children(node, path, ctx, 6.)),
    );
    frame(node, path, ctx, body)
}

fn children(node: &Node, path: &mut Vec<usize>, ctx: &Ctx, gap: f32) -> Div {
    let mut d = div().flex().flex_col().gap(px(gap));
    for (i, c) in node.children().iter().enumerate() {
        path.push(i);
        d = d.child(block(c, path, ctx));
        path.pop();
    }
    d
}

/// Inline content: one `StyledText` per block so words flow across mark
/// boundaries; marks become byte-range highlights.
fn inline(node: &Node, ctx: &Ctx) -> StyledText {
    let mut text = String::new();
    let mut highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    for c in node.children() {
        let start = text.len();
        match c.text.as_deref() {
            Some(t) => text.push_str(t),
            None => text.push_str(&format!("⟨{}⟩", node_type(c))),
        }
        let range = start..text.len();
        let mut style = HighlightStyle::default();
        let mut styled = c.text.is_none();
        if styled {
            style.color = Some(Tokens::unknown());
        }
        for m in c.marks.as_deref().unwrap_or(&[]) {
            styled = true;
            match m.mark_type.as_str() {
                "bold" => style.font_weight = Some(FontWeight::BOLD),
                "italic" => style.font_style = Some(FontStyle::Italic),
                "underline" => {
                    style.underline = Some(UnderlineStyle {
                        thickness: px(1.),
                        color: None,
                        wavy: false,
                    })
                }
                "strike" => {
                    style.strikethrough = Some(StrikethroughStyle {
                        thickness: px(1.),
                        color: None,
                    })
                }
                "code" => {
                    style.background_color = Some(Tokens::button());
                    style.color = Some(Tokens::secondary());
                }
                "link" => {
                    style.color = Some(Tokens::moved());
                    style.underline = Some(UnderlineStyle {
                        thickness: px(1.),
                        color: Some(Tokens::moved()),
                        wavy: false,
                    });
                }
                _ => style.background_color = Some(Tokens::unknown_fill()),
            }
        }
        if styled {
            highlights.push((range, style));
        }
    }
    let _ = ctx;
    StyledText::new(text).with_highlights(highlights)
}
