//! Center pane: read-only rendered preview. Click a block to select it.
//! Unknown node types render as a dashed amber block and stay selectable.

use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::*;
use gpui_kit::*;
use tiptap_rusty_parser::Node;

use crate::state::{fmt_path, is_unknown, node_type, DocumentState};
use crate::theme::Tokens;
use crate::tree::pane_header;

pub struct PreviewPanel {
    state: Entity<DocumentState>,
    _subs: Vec<Subscription>,
}

impl PreviewPanel {
    pub fn new(state: Entity<DocumentState>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&state, |_, _, cx| cx.notify());
        Self {
            state,
            _subs: vec![sub],
        }
    }
}

struct Ctx<'a> {
    state: &'a Entity<DocumentState>,
    selection: Option<&'a [usize]>,
    mono: SharedString,
}

impl Render for PreviewPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        let state = self.state.clone();
        let s = self.state.read(cx);
        let ctx = Ctx {
            state: &state,
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
                let el = block(child, &mut path, &ctx);
                path.pop();
                el
            })
            .collect();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(Tokens::background())
            .child(
                pane_header("Preview").child(
                    div()
                        .font_weight(FontWeight::NORMAL)
                        .text_color(Tokens::disabled())
                        .child("read-only · click to select"),
                ),
            )
            .child(
                div()
                    .id("preview-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .px(px(56.))
                    .py(px(40.))
                    .child(
                        div()
                            .max_w(px(680.))
                            .mx_auto()
                            .flex()
                            .flex_col()
                            .gap(px(14.))
                            .text_size(px(15.))
                            .line_height(relative(1.55))
                            .text_color(Tokens::foreground())
                            .children(blocks),
                    ),
            )
    }
}

fn block(node: &Node, path: &mut Vec<usize>, ctx: &Ctx) -> AnyElement {
    let ty = node_type(node);
    let selected = ctx.selection == Some(path.as_slice());
    let unknown = is_unknown(node);
    let id_path = path.clone();
    let state = ctx.state.clone();

    let mut wrapper = div()
        .id(ElementId::Name(format!("blk{}", fmt_path(path)).into()))
        .relative()
        .rounded(px(6.))
        .px(px(10.))
        .py(px(4.))
        .mx(px(-10.))
        .cursor_pointer()
        .border_1()
        .border_color(transparent_black())
        .hover(|s| s.border_dashed().border_color(Tokens::ghost()))
        .when(selected, |s| s.border_color(Tokens::accent()))
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            state.update(cx, |s, cx| s.select(Some(id_path.clone()), cx));
        });

    if selected {
        wrapper = wrapper.child(
            div()
                .absolute()
                .top(px(-10.))
                .right(px(8.))
                .px(px(6.))
                .rounded(px(4.))
                .bg(Tokens::accent())
                .text_color(white())
                .text_size(px(10.))
                .line_height(px(16.))
                .font_family(ctx.mono.clone())
                .child(format!("{ty} · {}", fmt_path(path))),
        );
    }

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
        "bulletList" | "orderedList" | "taskList" => div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .pl(px(22.))
            .children(node.children().iter().enumerate().map(|(i, item)| {
                path.push(i);
                let el = div()
                    .flex()
                    .gap(px(8.))
                    .child(div().flex_shrink_0().text_color(Tokens::muted()).child(
                        if ty == "orderedList" {
                            format!("{}.", i + 1)
                        } else {
                            "•".into()
                        },
                    ))
                    .child(div().flex_1().min_w_0().child(block(item, path, ctx)));
                path.pop();
                el
            }))
            .into_any_element(),
        "blockquote" => div()
            .pl(px(16.))
            .border_l_3()
            .border_color(Tokens::input_border())
            .text_color(Tokens::secondary())
            .italic()
            .child(children(node, path, ctx))
            .into_any_element(),
        "horizontalRule" => div()
            .h(px(1.))
            .my(px(4.))
            .bg(Tokens::rule())
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
        _ if unknown => {
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
                        .text_color(Tokens::unknown())
                        .child(format!("{}{first_attr}", ty.to_uppercase())),
                )
                .child(children(node, path, ctx))
                .into_any_element()
        }
        // known container we have no special style for (listItem, table…)
        _ => children(node, path, ctx).into_any_element(),
    };

    wrapper.child(body).into_any_element()
}

fn children(node: &Node, path: &mut Vec<usize>, ctx: &Ctx) -> Div {
    let mut d = div().flex().flex_col().gap(px(10.));
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
