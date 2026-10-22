//! Right pane: Figma-style inspector for the selected node.
//! v1: identity, attrs (read-only values + remove), mark chips, structure, JSON.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use tiptap_rusty_parser::Node;

use crate::state::{fmt_path, is_unknown, node_type, short_value, DocumentState, BUILTIN_MARKS};
use crate::theme::Tokens;
use crate::tree::icon_button;

pub struct InspectorPanel {
    state: Entity<DocumentState>,
    _subs: Vec<Subscription>,
}

impl InspectorPanel {
    pub fn new(state: Entity<DocumentState>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&state, |_, _, cx| cx.notify());
        Self {
            state,
            _subs: vec![sub],
        }
    }
}

impl Render for InspectorPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        let state = self.state.clone();
        let s = self.state.read(cx);

        let mut pane = div()
            .flex()
            .flex_col()
            .w(px(320.))
            .flex_shrink_0()
            .h_full()
            .bg(Tokens::panel())
            .border_l_1()
            .border_color(Tokens::border());

        let Some(path) = s.selection.clone() else {
            return pane.child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .text_color(Tokens::disabled())
                    .child("Select a node in the tree or preview"),
            );
        };
        let Some(node) = s.selected() else {
            return pane;
        };
        let ty = node_type(node).to_string();
        let unknown = is_unknown(node);
        let is_text = ty == "text";
        let (undo_n, redo_n) = s.undo_depth();
        let last = s.last_change.clone().unwrap_or_else(|| "—".into());

        // header
        pane = pane.child(
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
                        .text_color(if unknown {
                            Tokens::unknown()
                        } else {
                            Tokens::foreground()
                        })
                        .child(ty.clone()),
                )
                .child(chip(fmt_path(&path), &mono))
                .when(unknown, |d| {
                    d.child(
                        div()
                            .text_size(px(11.))
                            .text_color(Tokens::muted())
                            .child("not in schema"),
                    )
                })
                .child(div().flex_1())
                .child(icon_button("copy-json", IconName::Copy, {
                    let state = state.clone();
                    move |_, _, cx| {
                        let json = state
                            .read(cx)
                            .selected()
                            .and_then(|n| serde_json::to_string_pretty(n).ok());
                        if let Some(json) = json {
                            cx.write_to_clipboard(ClipboardItem::new_string(json));
                        }
                    }
                })),
        );

        let mut body = div()
            .id("inspector-scroll")
            .flex_1()
            .overflow_y_scroll()
            .flex()
            .flex_col();

        // identity
        body = body.child(
            section("Identity", None)
                .child(field("type", value_text(ty.clone(), &mono), None))
                .child(field(
                    "path",
                    value_text(format!("doc › {}", fmt_path(&path)), &mono),
                    None,
                ))
                .child(field(
                    "content",
                    value_text(
                        if is_text {
                            format!(
                                "{} graphemes",
                                node.text.as_deref().map(|t| t.chars().count()).unwrap_or(0)
                            )
                        } else {
                            let (n, t) = count(node);
                            format!(
                                "{n} child{} · {t} text node{}",
                                if n == 1 { "" } else { "ren" },
                                if t == 1 { "" } else { "s" }
                            )
                        },
                        &mono,
                    ),
                    None,
                )),
        );

        // text
        if let Some(text) = node.text.as_deref() {
            body = body.child(
                section("Text", None).child(
                    div()
                        .px(px(8.))
                        .py(px(6.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(Tokens::input_border())
                        .bg(Tokens::background())
                        .text_size(px(13.))
                        .line_height(relative(1.5))
                        .child(text.to_string()),
                ),
            );
        }

        // attributes
        let attrs: Vec<(String, serde_json::Value)> = node
            .attrs
            .as_ref()
            .map(|a| a.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();
        let mut attrs_section = section("Attributes", None);
        if attrs.is_empty() {
            attrs_section = attrs_section.child(
                div()
                    .text_size(px(12.))
                    .text_color(Tokens::disabled())
                    .child("No attributes."),
            );
        }
        for (k, v) in attrs {
            let remove = {
                let state = state.clone();
                let path = path.clone();
                let key = k.clone();
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    state.update(cx, |s, cx| s.remove_attr(path.clone(), &key, cx));
                }
            };
            let kind = match &v {
                serde_json::Value::Number(_) => "number",
                serde_json::Value::Bool(_) => "bool",
                serde_json::Value::String(_) => "string",
                serde_json::Value::Null => "null",
                _ => "json",
            };
            attrs_section = attrs_section.child(field(
                k.clone(),
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(value_box(short_value(&v), &mono))
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(Tokens::disabled())
                            .child(kind),
                    ),
                Some(Box::new(remove)),
            ));
        }
        body = body.child(attrs_section);

        // marks
        if is_text {
            let present: Vec<String> = node
                .marks
                .as_ref()
                .map(|m| m.iter().map(|m| m.mark_type.clone()).collect())
                .unwrap_or_default();
            let mut all: Vec<String> = BUILTIN_MARKS.iter().map(|s| s.to_string()).collect();
            for m in &present {
                if !all.contains(m) {
                    all.push(m.clone());
                }
            }
            let mut chips = div().flex().flex_wrap().gap(px(6.));
            for (i, m) in all.into_iter().enumerate() {
                let on = present.contains(&m);
                let state = state.clone();
                let path = path.clone();
                let name = m.clone();
                chips = chips.child(
                    div()
                        .id(ElementId::NamedInteger("mark-chip".into(), i as u64))
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .h(px(28.))
                        .px(px(10.))
                        .rounded(px(14.))
                        .border_1()
                        .cursor_pointer()
                        .text_size(px(12.))
                        .border_color(if on {
                            Tokens::accent()
                        } else {
                            Tokens::input_border()
                        })
                        .bg(if on {
                            Tokens::accent_fill()
                        } else {
                            Tokens::button()
                        })
                        .text_color(if on {
                            Tokens::foreground()
                        } else {
                            Tokens::secondary()
                        })
                        .hover(|s| s.bg(Tokens::raised()))
                        .child(mark_icon(&m))
                        .child(m)
                        .on_click(move |_, _, cx| {
                            state.update(cx, |s, cx| s.toggle_mark(path.clone(), &name, cx))
                        }),
                );
            }
            body = body.child(
                section(
                    "Marks",
                    Some(
                        format!(
                            "{} of {}",
                            present.len(),
                            BUILTIN_MARKS.len().max(present.len())
                        )
                        .into(),
                    ),
                )
                .child(chips),
            );
            // per-mark attrs
            for m in node.marks.as_deref().unwrap_or(&[]) {
                if let Some(attrs) = &m.attrs {
                    let mut sec = section(format!("{} attrs", m.mark_type), None);
                    for (k, v) in attrs {
                        sec = sec.child(field(k.clone(), value_box(short_value(v), &mono), None));
                    }
                    body = body.child(sec);
                }
            }
        }

        // structure
        let can_move = !path.is_empty();
        let act = |label: &'static str, id: &'static str, f: StateFn| {
            let state = state.clone();
            Button::new(id)
                .outline()
                .small()
                .label(label)
                .on_click(move |_, _, cx| state.update(cx, |s, cx| f(s, cx)))
        };
        body = body.child(
            section("Structure", None)
                .child(
                    div()
                        .grid()
                        .grid_cols(2)
                        .gap(px(6.))
                        .child(act(
                            "Wrap in blockquote",
                            "wrap",
                            Box::new(|s, cx| s.wrap_selected("blockquote", cx)),
                        ))
                        .child(act("Lift", "lift", Box::new(|s, cx| s.lift_selected(cx))))
                        .child(act(
                            "Move up",
                            "move-up",
                            Box::new(|s, cx| s.move_selected(-1, cx)),
                        ))
                        .child(act(
                            "Move down",
                            "move-down",
                            Box::new(|s, cx| s.move_selected(1, cx)),
                        )),
                )
                .when(can_move, |d| {
                    let state = state.clone();
                    d.child(
                        Button::new("delete-node")
                            .danger()
                            .outline()
                            .small()
                            .icon(Icon::new(IconName::Trash).xsmall())
                            .label("Delete node")
                            .on_click(move |_, _, cx| {
                                state.update(cx, |s, cx| s.delete_selected(cx))
                            }),
                    )
                }),
        );

        // json
        let json = serde_json::to_string_pretty(&shallow(node)).unwrap_or_default();
        body = body.child(
            section("JSON", Some("preserve_order".into())).child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(6.))
                    .bg(Tokens::code_bg())
                    .border_1()
                    .border_color(Tokens::border())
                    .font_family(mono.clone())
                    .text_size(px(11.))
                    .line_height(relative(1.5))
                    .text_color(Tokens::secondary())
                    .child(json),
            ),
        );

        pane.child(body).child(
            div()
                .flex()
                .gap(px(12.))
                .px(px(12.))
                .py(px(8.))
                .border_t_1()
                .border_color(Tokens::border())
                .font_family(mono)
                .text_size(px(11.))
                .text_color(Tokens::muted())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(format!("last: {last}")),
                )
                .child(format!("undo {undo_n} · redo {redo_n}")),
        )
    }
}

fn count(node: &Node) -> (usize, usize) {
    let mut t = 0;
    node.walk(&mut |n| {
        if n.text.is_some() {
            t += 1;
        }
    });
    (node.child_count(), t)
}

/// Node with nested content collapsed to `[ … ]` for the JSON peek.
fn shallow(node: &Node) -> serde_json::Value {
    let mut v = serde_json::to_value(node).unwrap_or_default();
    if let Some(obj) = v.as_object_mut() {
        if let Some(c) = obj.get_mut("content") {
            let n = c.as_array().map(Vec::len).unwrap_or(0);
            *c = serde_json::Value::String(format!("[ … {n} ]"));
        }
    }
    v
}

fn mark_icon(m: &str) -> Icon {
    let name = match m {
        "bold" => IconName::Bold,
        "italic" => IconName::Italic,
        "underline" => IconName::Underline,
        "strike" => IconName::Strikethrough,
        "code" => IconName::Code,
        "link" => IconName::Link,
        _ => IconName::Box,
    };
    Icon::new(name).xsmall()
}

pub fn chip(text: impl Into<SharedString>, mono: &SharedString) -> Div {
    div()
        .px(px(6.))
        .py(px(1.))
        .rounded(px(4.))
        .bg(Tokens::raised())
        .font_family(mono.clone())
        .text_size(px(11.))
        .text_color(Tokens::muted())
        .child(text.into())
}

pub fn section(title: impl Into<SharedString>, right: Option<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .px(px(16.))
        .py(px(14.))
        .border_b_1()
        .border_color(Tokens::rule())
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(Tokens::muted())
                .child(title.into().to_uppercase())
                .when_some(right, |d, r| {
                    d.child(div().font_weight(FontWeight::NORMAL).child(r))
                }),
        )
}

type RemoveFn = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type StateFn = Box<dyn Fn(&mut DocumentState, &mut Context<DocumentState>)>;

/// `key | value | ×` row, 96px key column.
pub fn field(
    key: impl Into<SharedString>,
    value: impl IntoElement,
    remove: Option<RemoveFn>,
) -> Div {
    let key: SharedString = key.into();
    let id: SharedString = format!("rm-{key}").into();
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .w(px(96.))
                .flex_shrink_0()
                .text_size(px(12.))
                .text_color(Tokens::secondary())
                .overflow_hidden()
                .text_ellipsis()
                .child(key),
        )
        .child(div().flex_1().min_w_0().child(value))
        .child(match remove {
            Some(f) => div()
                .id(ElementId::Name(id))
                .size(px(20.))
                .flex_shrink_0()
                .rounded(px(4.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_color(Tokens::muted())
                .hover(|s| s.bg(Tokens::raised()).text_color(Tokens::removed()))
                .on_click(move |e, w, cx| f(e, w, cx))
                .child(Icon::new(IconName::X).xsmall())
                .into_any_element(),
            None => div().size(px(20.)).flex_shrink_0().into_any_element(),
        })
}

fn value_text(text: impl Into<SharedString>, mono: &SharedString) -> Div {
    div()
        .font_family(mono.clone())
        .text_size(px(12.))
        .text_color(Tokens::muted())
        .child(text.into())
}

fn value_box(text: impl Into<SharedString>, mono: &SharedString) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .h(px(28.))
        .px(px(8.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .border_1()
        .border_color(Tokens::input_border())
        .bg(Tokens::background())
        .font_family(mono.clone())
        .text_size(px(12.))
        .overflow_hidden()
        .text_ellipsis()
        .whitespace_nowrap()
        .child(text.into())
}
