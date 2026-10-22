//! Right pane: Figma-style inspector for the selected node.
//! Identity, typed attribute editors, text editing, mark chips + mark attrs,
//! structure, JSON peek.

use std::collections::HashMap;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{
    Input, InputEvent, InputState, NumberInput, NumberInputEvent, StepAction, Textarea,
    TextareaState,
};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::Value;
use tiptap_rusty_parser::{Mark, Node};

use crate::state::{fmt_path, is_unknown, node_type, DocumentState, BUILTIN_MARKS};
use crate::theme::Tokens;
use crate::tree::icon_button;

/// Wrap presets: `(label, wrappers innermost-first)`.
const WRAP_PRESETS: &[(&str, &[&str])] = &[
    ("blockquote", &["blockquote"]),
    ("bulletList › listItem", &["listItem", "bulletList"]),
    ("orderedList › listItem", &["listItem", "orderedList"]),
    ("callout", &["callout"]),
];

/// Value class an attribute editor is picked by.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Str,
    Num,
    Bool,
    Json,
}

impl Kind {
    fn of(v: &Value) -> Self {
        match v {
            Value::String(_) => Kind::Str,
            Value::Number(_) => Kind::Num,
            Value::Bool(_) => Kind::Bool,
            _ => Kind::Json,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Kind::Str => "string",
            Kind::Num => "number",
            Kind::Bool => "bool",
            Kind::Json => "json",
        }
    }

    fn default_value(self) -> Value {
        match self {
            Kind::Str => Value::String(String::new()),
            Kind::Num => Value::from(0),
            Kind::Bool => Value::Bool(false),
            Kind::Json => Value::Object(Default::default()),
        }
    }
}

const NEW_KINDS: [Kind; 4] = [Kind::Str, Kind::Num, Kind::Bool, Kind::Json];

/// What a field writes to when it commits.
#[derive(Clone)]
enum Target {
    Attr(String),
    Text,
    MarkAttr(String, String),
}

enum Editor {
    Line(Entity<InputState>),
    Number(Entity<InputState>),
    Area(Entity<TextareaState>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditorKind {
    Line,
    Number,
    Area,
}

impl Editor {
    fn kind(&self) -> EditorKind {
        match self {
            Editor::Line(_) => EditorKind::Line,
            Editor::Number(_) => EditorKind::Number,
            Editor::Area(_) => EditorKind::Area,
        }
    }

    fn focused(&self, window: &Window, cx: &App) -> bool {
        match self {
            Editor::Line(s) | Editor::Number(s) => s.read(cx).focus_handle(cx).is_focused(window),
            Editor::Area(s) => s.read(cx).focus_handle(cx).is_focused(window),
        }
    }

    fn set(&self, text: String, window: &mut Window, cx: &mut App) {
        match self {
            Editor::Line(s) | Editor::Number(s) => {
                s.update(cx, |s, cx| s.set_value(text, window, cx))
            }
            Editor::Area(s) => s.update(cx, |s, cx| s.set_value(text, window, cx)),
        }
    }
}

/// One cached editor state per (selected node, target).
struct Field {
    editor: Editor,
    target: Target,
    /// Document value last pushed into or committed from the editor.
    last: Value,
    invalid: bool,
    _subs: Vec<Subscription>,
}

struct AddMark {
    ty: Entity<InputState>,
    key: Entity<InputState>,
    val: Entity<InputState>,
    _sub: Subscription,
}

pub struct InspectorPanel {
    state: Entity<DocumentState>,
    wrap_open: bool,
    wrap_input: Option<Entity<InputState>>,
    fields: HashMap<String, Field>,
    fields_path: Option<Vec<usize>>,
    seen: Vec<String>,
    new_attr_key: Option<Entity<InputState>>,
    new_attr_kind: Kind,
    add_mark: Option<AddMark>,
    add_mark_open: bool,
    _subs: Vec<Subscription>,
}

impl InspectorPanel {
    pub fn new(state: Entity<DocumentState>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&state, |_, _, cx| cx.notify());
        Self {
            state,
            wrap_open: false,
            wrap_input: None,
            fields: HashMap::new(),
            fields_path: None,
            seen: Vec::new(),
            new_attr_key: None,
            new_attr_kind: Kind::Str,
            add_mark: None,
            add_mark_open: false,
            _subs: vec![sub],
        }
    }

    /// Free-text wrapper type input for the "Wrap in…" chooser, created once.
    fn wrap_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(input) = &self.wrap_input {
            return input.clone();
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("custom type…"));
        let sub = cx.subscribe(&input, |this, input, ev: &InputEvent, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                let ty = input.read(cx).value().trim().to_string();
                this.wrap_custom(ty, cx);
            }
        });
        self._subs.push(sub);
        self.wrap_input = Some(input.clone());
        input
    }

    fn wrap_in(&mut self, wrappers: &[&str], cx: &mut Context<Self>) {
        self.state.update(cx, |s, cx| s.wrap_selected(wrappers, cx));
        self.wrap_open = false;
        cx.notify();
    }

    fn wrap_custom(&mut self, ty: String, cx: &mut Context<Self>) {
        if ty.is_empty() {
            return;
        }
        self.wrap_in(&[&ty], cx);
    }

    /// `Wrap in…` chooser: preset rows + free-text type.
    fn wrap_chooser(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let input = self.wrap_input(window, cx);
        let trigger = Button::new("wrap")
            .outline()
            .small()
            .label("Wrap in…")
            .icon(Icon::new(IconName::ChevronDown).xsmall());
        Popover::new("wrap-popover")
            .trigger(trigger)
            .open(self.wrap_open)
            .on_open_change({
                let this = this.clone();
                move |open, _, cx| {
                    this.update(cx, |p, cx| {
                        p.wrap_open = *open;
                        cx.notify();
                    })
                }
            })
            .content(move |_, _, cx| {
                let mono = cx.theme().mono_font_family.clone();
                let mut list = div().flex().flex_col().gap(px(2.)).w(px(210.));
                for (i, (label, wrappers)) in WRAP_PRESETS.iter().enumerate() {
                    let this = this.clone();
                    list = list.child(
                        div()
                            .id(ElementId::NamedInteger("wrap-preset".into(), i as u64))
                            .h(px(28.))
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .rounded(px(5.))
                            .cursor_pointer()
                            .font_family(mono.clone())
                            .text_size(px(12.))
                            .text_color(Tokens::foreground())
                            .hover(|s| s.bg(Tokens::raised()))
                            .child(*label)
                            .on_click(move |_, _, cx| {
                                this.update(cx, |p, cx| p.wrap_in(wrappers, cx))
                            }),
                    );
                }
                let this = this.clone();
                let value_input = input.clone();
                list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .mt(px(6.))
                        .pt(px(8.))
                        .border_t_1()
                        .border_color(Tokens::rule())
                        .child(div().flex_1().min_w_0().child(Input::new(&input).small()))
                        .child(
                            Button::new("wrap-custom")
                                .primary()
                                .small()
                                .label("Wrap")
                                .on_click(move |_, _, cx| {
                                    let ty = value_input.read(cx).value().trim().to_string();
                                    this.update(cx, |p, cx| p.wrap_custom(ty, cx))
                                }),
                        ),
                )
            })
    }

    /// Ensure the editor for `id` exists and mirrors `value`, then render it.
    fn editor(
        &mut self,
        id: String,
        target: Target,
        value: &Value,
        path: &[usize],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let want = match (&target, Kind::of(value)) {
            (Target::Text, _) => EditorKind::Area,
            (_, Kind::Str) => EditorKind::Line,
            (_, Kind::Num) => EditorKind::Number,
            _ => EditorKind::Area,
        };
        let stale = self.fields.get(&id).is_none_or(|f| f.editor.kind() != want);
        if stale {
            let text: SharedString = text_of(value).into();
            let editor = match want {
                EditorKind::Line => {
                    Editor::Line(cx.new(|cx| InputState::new(window, cx).default_value(text)))
                }
                EditorKind::Number => Editor::Number(cx.new(|cx| {
                    let mut s = InputState::new(window, cx).default_value(text);
                    // emit `Step` events instead of stepping internally
                    s.set_step(None, window, cx);
                    s
                })),
                EditorKind::Area => {
                    Editor::Area(cx.new(|cx| TextareaState::new(window, cx).default_value(text)))
                }
            };
            let subs = self.subscribe(&id, &editor, path.to_vec(), window, cx);
            self.fields.insert(
                id.clone(),
                Field {
                    editor,
                    target,
                    last: value.clone(),
                    invalid: false,
                    _subs: subs,
                },
            );
        } else if let Some(f) = self.fields.get_mut(&id) {
            if f.last != *value && !f.editor.focused(window, cx) {
                f.editor.set(text_of(value), window, cx);
                f.last = value.clone();
                f.invalid = false;
            }
        }
        self.seen.push(id.clone());
        let f = &self.fields[&id];
        let border = if f.invalid {
            Tokens::removed()
        } else {
            Tokens::input_border()
        };
        // values and JSON in mono; free text keeps the UI font
        let font = (!matches!(f.target, Target::Text)).then(|| cx.theme().mono_font_family.clone());
        let el = match &f.editor {
            Editor::Line(s) => Input::new(s).small().into_any_element(),
            Editor::Number(s) => div()
                .rounded(px(6.))
                .when(f.invalid, |d| d.border_1().border_color(border))
                .child(NumberInput::new(s).small())
                .into_any_element(),
            Editor::Area(s) => div()
                .rounded(px(6.))
                .border_1()
                .border_color(border)
                .child(Textarea::new(s).small().bordered(false).h(px(64.)))
                .into_any_element(),
        };
        div()
            .when_some(font, |d, f| d.font_family(f))
            .child(el)
            .into_any_element()
    }

    fn subscribe(
        &mut self,
        id: &str,
        editor: &Editor,
        path: Vec<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Subscription> {
        let mut subs = Vec::new();
        match editor {
            Editor::Line(s) | Editor::Number(s) => {
                subs.push(cx.subscribe_in(s, window, {
                    let id = id.to_string();
                    let path = path.clone();
                    move |this, s, ev: &InputEvent, _, cx| {
                        if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                            let text = s.read(cx).value();
                            this.commit(&id, &path, &text, cx);
                        }
                    }
                }));
                if let Editor::Number(_) = editor {
                    subs.push(cx.subscribe_in(s, window, {
                        let id = id.to_string();
                        let path = path.clone();
                        move |this, s, ev: &NumberInputEvent, window, cx| {
                            let NumberInputEvent::Step(action) = ev;
                            this.step(&id, &path, *action, s, window, cx);
                        }
                    }));
                }
            }
            Editor::Area(s) => subs.push(cx.subscribe_in(s, window, {
                let id = id.to_string();
                move |this, s, ev: &InputEvent, _, cx| {
                    if matches!(ev, InputEvent::Blur) {
                        let text = s.read(cx).value();
                        this.commit(&id, &path, &text, cx);
                    }
                }
            })),
        }
        subs
    }

    /// Parse the editor text by its kind and write it to the document.
    fn commit(&mut self, id: &str, path: &[usize], text: &str, cx: &mut Context<Self>) {
        let Some(f) = self.fields.get_mut(id) else {
            return;
        };
        let parsed = match (&f.target, &f.editor) {
            (Target::Text, _) | (_, Editor::Line(_)) => Some(Value::String(text.to_string())),
            (_, Editor::Number(_)) => parse_number(text, &f.last),
            (_, Editor::Area(_)) => serde_json::from_str(text).ok(),
        };
        let Some(value) = parsed else {
            f.invalid = true;
            cx.notify();
            return;
        };
        if f.invalid {
            f.invalid = false;
            cx.notify();
        }
        if value == f.last {
            return;
        }
        f.last = value.clone();
        let target = f.target.clone();
        let path = path.to_vec();
        self.state.update(cx, |s, cx| match target {
            Target::Attr(k) => s.set_attr(path, &k, value, cx),
            Target::Text => s.set_text(path, text.to_string(), cx),
            Target::MarkAttr(m, k) => s.set_mark_attr(path, &m, &k, value, cx),
        });
    }

    /// − / + on a number field: integers stay integers, floats stay floats.
    fn step(
        &mut self,
        id: &str,
        path: &[usize],
        action: StepAction,
        s: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(f) = self.fields.get_mut(id) else {
            return;
        };
        let base = parse_number(&s.read(cx).value(), &f.last).unwrap_or_else(|| f.last.clone());
        let d: i64 = match action {
            StepAction::Increment => 1,
            StepAction::Decrement => -1,
        };
        let next = match base.as_i64() {
            Some(i) => Value::from(i + d),
            None => match base.as_f64() {
                Some(x) => serde_json::Number::from_f64(x + d as f64)
                    .map(Value::Number)
                    .unwrap_or(base),
                None => Value::from(d),
            },
        };
        s.update(cx, |s, cx| s.set_value(text_of(&next), window, cx));
        self.commit(id, path, &text_of(&next), cx);
    }

    fn add_attr(&mut self, path: Vec<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.new_attr_key.clone() else {
            return;
        };
        let key = input.read(cx).value().trim().to_string();
        if key.is_empty() {
            return;
        }
        let value = self.new_attr_kind.default_value();
        self.state
            .update(cx, |s, cx| s.set_attr(path, &key, value, cx));
        input.update(cx, |s, cx| s.set_value("", window, cx));
    }

    fn add_mark(&mut self, path: Vec<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = &self.add_mark else {
            return;
        };
        let ty = row.ty.read(cx).value().trim().to_string();
        if ty.is_empty() {
            return;
        }
        let key = row.key.read(cx).value().trim().to_string();
        let val = row.val.read(cx).value().to_string();
        let mut mark = Mark::new(ty);
        if !key.is_empty() {
            mark = mark.attr(key, val);
        }
        for s in [&row.ty, &row.key, &row.val] {
            s.update(cx, |s, cx| s.set_value("", window, cx));
        }
        self.add_mark_open = false;
        self.state.update(cx, |s, cx| s.add_mark(path, mark, cx));
    }

    fn add_mark_row(&mut self, window: &mut Window, cx: &mut Context<Self>) -> &AddMark {
        if self.add_mark.is_none() {
            let ty = cx.new(|cx| InputState::new(window, cx).placeholder("type"));
            let key = cx.new(|cx| InputState::new(window, cx).placeholder("key"));
            let val = cx.new(|cx| InputState::new(window, cx).placeholder("value"));
            // `link` prefills `href`
            let sub = cx.subscribe_in(&ty, window, {
                let key = key.clone();
                move |_, ty, ev: &InputEvent, window, cx| {
                    if matches!(ev, InputEvent::Change)
                        && ty.read(cx).value().trim() == "link"
                        && key.read(cx).value().is_empty()
                    {
                        key.update(cx, |s, cx| s.set_value("href", window, cx));
                    }
                }
            });
            self.add_mark = Some(AddMark {
                ty,
                key,
                val,
                _sub: sub,
            });
        }
        self.add_mark.as_ref().unwrap()
    }
}

impl Render for InspectorPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        // Built before borrowing the state: it needs `&mut self` for the input entity.
        let wrap_chooser = self.wrap_chooser(window, cx).into_any_element();
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
            self.fields.clear();
            self.fields_path = None;
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
        // snapshot of the selected node; the editors below need `&mut self`
        let text = node.text.clone();
        let attrs: Vec<(String, Value)> = node
            .attrs
            .as_ref()
            .map(|a| a.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();
        let marks: Vec<Mark> = node.marks.clone().unwrap_or_default();
        let content = if is_text {
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
        };

        if self.fields_path.as_deref() != Some(path.as_slice()) {
            self.fields.clear();
            self.fields_path = Some(path.clone());
            self.add_mark_open = false;
        }
        self.seen.clear();

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
                .child(field("content", value_text(content, &mono), None)),
        );

        // text (commit on blur)
        if let Some(text) = text {
            let editor = self.editor(
                "t".into(),
                Target::Text,
                &Value::String(text),
                &path,
                window,
                cx,
            );
            body = body.child(section("Text", None).child(editor));
        }

        // attributes: typed editors
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
            let kind = Kind::of(&v);
            let editor = match &v {
                Value::Bool(b) => {
                    let state = state.clone();
                    let path = path.clone();
                    let key = k.clone();
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .h(px(28.))
                        .child(
                            Switch::new(ElementId::Name(format!("sw-{k}").into()))
                                .small()
                                .checked(*b)
                                .on_change(move |on: &bool, _, cx| {
                                    state.update(cx, |s, cx| {
                                        s.set_attr(path.clone(), &key, Value::Bool(*on), cx)
                                    })
                                }),
                        )
                        .child(
                            div()
                                .font_family(mono.clone())
                                .text_size(px(12.))
                                .text_color(Tokens::secondary())
                                .child(if *b { "true" } else { "false" }),
                        )
                        .into_any_element()
                }
                _ => self.editor(
                    format!("a:{k}"),
                    Target::Attr(k.clone()),
                    &v,
                    &path,
                    window,
                    cx,
                ),
            };
            attrs_section = attrs_section.child(field(
                k.clone(),
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(div().flex_1().min_w_0().child(editor))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(10.))
                            .text_color(Tokens::disabled())
                            .child(kind.label()),
                    ),
                Some(Box::new(remove)),
            ));
        }
        // add attribute row
        let new_key = self
            .new_attr_key
            .get_or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("key")))
            .clone();
        let mut kinds = div().flex().gap(px(2.));
        for kind in NEW_KINDS {
            let on = kind == self.new_attr_kind;
            kinds = kinds.child(
                div()
                    .id(ElementId::Name(format!("nk-{}", kind.label()).into()))
                    .h(px(22.))
                    .px(px(6.))
                    .rounded(px(4.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(10.))
                    .bg(if on {
                        Tokens::accent_fill()
                    } else {
                        Tokens::button()
                    })
                    .text_color(if on {
                        Tokens::foreground()
                    } else {
                        Tokens::muted()
                    })
                    .hover(|s| s.bg(Tokens::raised()))
                    .child(kind.label())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.new_attr_kind = kind;
                        cx.notify();
                    })),
            );
        }
        attrs_section = attrs_section.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .pt(px(4.))
                .child(div().flex_1().min_w_0().child(Input::new(&new_key).small()))
                .child(kinds.flex_shrink_0())
                .child(plus_button("add-attr", {
                    let path = path.clone();
                    cx.listener(move |this, _, window, cx| this.add_attr(path.clone(), window, cx))
                })),
        );
        body = body.child(attrs_section);

        // marks
        if is_text {
            let present: Vec<String> = marks.iter().map(|m| m.mark_type.clone()).collect();
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
            // `+ mark…` reveals the add-mark row
            let open = self.add_mark_open;
            chips = chips.child(
                div()
                    .id("add-mark-chip")
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .h(px(28.))
                    .px(px(10.))
                    .rounded(px(14.))
                    .border_1()
                    .border_dashed()
                    .cursor_pointer()
                    .text_size(px(12.))
                    .border_color(if open {
                        Tokens::accent()
                    } else {
                        Tokens::input_border()
                    })
                    .text_color(Tokens::muted())
                    .hover(|s| s.bg(Tokens::raised()))
                    .child(Icon::new(IconName::Plus).xsmall())
                    .child("mark…")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.add_mark_open = !this.add_mark_open;
                        cx.notify();
                    })),
            );
            let mut marks_section = section(
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
            .child(chips);
            if open {
                let row = self.add_mark_row(window, cx);
                let (ty, key, val) = (row.ty.clone(), row.key.clone(), row.val.clone());
                marks_section = marks_section.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(Input::new(&ty).small())
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(div().flex_1().min_w_0().child(Input::new(&key).small()))
                                .child(div().flex_1().min_w_0().child(Input::new(&val).small()))
                                .child(
                                    Button::new("add-mark")
                                        .outline()
                                        .small()
                                        .label("Add")
                                        .on_click({
                                            let path = path.clone();
                                            cx.listener(move |this, _, window, cx| {
                                                this.add_mark(path.clone(), window, cx)
                                            })
                                        }),
                                ),
                        ),
                );
            }
            body = body.child(marks_section);
            // per-mark attrs: editable, plus `+ attr`
            for m in &marks {
                let mt = m.mark_type.clone();
                let mut sec = section(format!("{mt} attrs"), None);
                for (k, v) in m.attrs.iter().flatten() {
                    let editor = self.editor(
                        format!("m:{mt}:{k}"),
                        Target::MarkAttr(mt.clone(), k.clone()),
                        v,
                        &path,
                        window,
                        cx,
                    );
                    sec = sec.child(field(k.clone(), editor, None));
                }
                let key_id = format!("m+:{mt}");
                let key_input = match self.fields.get(&key_id) {
                    Some(Field {
                        editor: Editor::Line(s),
                        ..
                    }) => s.clone(),
                    _ => {
                        let s = cx.new(|cx| InputState::new(window, cx).placeholder("new key"));
                        self.fields.insert(
                            key_id.clone(),
                            Field {
                                editor: Editor::Line(s.clone()),
                                target: Target::Attr(String::new()),
                                last: Value::Null,
                                invalid: false,
                                _subs: Vec::new(),
                            },
                        );
                        s
                    }
                };
                self.seen.push(key_id);
                let add = {
                    let state = state.clone();
                    let path = path.clone();
                    let mt = mt.clone();
                    let key_input = key_input.clone();
                    move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        let key = key_input.read(cx).value().trim().to_string();
                        if key.is_empty() {
                            return;
                        }
                        state.update(cx, |s, cx| {
                            s.set_mark_attr(
                                path.clone(),
                                &mt,
                                &key,
                                Value::String(String::new()),
                                cx,
                            )
                        });
                        key_input.update(cx, |s, cx| s.set_value("", window, cx));
                    }
                };
                sec = sec.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .pt(px(4.))
                        .child(
                            div()
                                .w(px(96.))
                                .flex_shrink_0()
                                .text_size(px(12.))
                                .text_color(Tokens::muted())
                                .child("+ attr"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(Input::new(&key_input).small()),
                        )
                        .child(plus_button(
                            ElementId::Name(format!("add-mark-attr-{mt}").into()),
                            add,
                        )),
                );
                body = body.child(sec);
            }
        }
        // drop editors whose field disappeared (removed attr, removed mark)
        let seen = std::mem::take(&mut self.seen);
        self.fields.retain(|k, _| seen.contains(k));
        self.seen = seen;

        let s = self.state.read(cx);
        let Some(node) = s.selected() else {
            return pane;
        };

        // structure
        let can_wrap = s.can_wrap();
        let can_lift = s.can_lift();
        let can_split = s.split_target().is_some();
        let can_join = s.join_target().is_some();
        let can_up = s.can_move(-1);
        let can_down = s.can_move(1);
        let can_delete = !path.is_empty();
        let act = |label: &'static str, id: &'static str, enabled: bool, f: StateFn| {
            let state = state.clone();
            Button::new(id)
                .outline()
                .small()
                .label(label)
                .disabled(!enabled)
                .on_click(move |_, _, cx| state.update(cx, |s, cx| f(s, cx)))
        };
        let wrap: AnyElement = if can_wrap {
            wrap_chooser
        } else {
            Button::new("wrap")
                .outline()
                .small()
                .label("Wrap in…")
                .icon(Icon::new(IconName::ChevronDown).xsmall())
                .disabled(true)
                .into_any_element()
        };
        body = body.child(
            section("Structure", None)
                .child(
                    div()
                        .grid()
                        .grid_cols(2)
                        .gap(px(6.))
                        .child(wrap)
                        .child(act(
                            "Lift",
                            "lift",
                            can_lift,
                            Box::new(|s, cx| s.lift_selected(cx)),
                        ))
                        .child(act(
                            "Split",
                            "split",
                            can_split,
                            Box::new(|s, cx| s.split_selected(cx)),
                        ))
                        .child(act(
                            "Join next",
                            "join-next",
                            can_join,
                            Box::new(|s, cx| s.join_selected(cx)),
                        ))
                        .child(act(
                            "Move up",
                            "move-up",
                            can_up,
                            Box::new(|s, cx| s.move_selected(-1, cx)),
                        ))
                        .child(act(
                            "Move down",
                            "move-down",
                            can_down,
                            Box::new(|s, cx| s.move_selected(1, cx)),
                        )),
                )
                .child({
                    let state = state.clone();
                    Button::new("delete-node")
                        .danger()
                        .outline()
                        .small()
                        .icon(Icon::new(IconName::Trash).xsmall())
                        .label("Delete node")
                        .disabled(!can_delete)
                        .on_click(move |_, _, cx| state.update(cx, |s, cx| s.delete_selected(cx)))
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

/// Editor text for a document value: raw string, number literal, pretty JSON.
fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

/// Parse number text; an integer literal stays an integer unless `last` was a float.
fn parse_number(text: &str, last: &Value) -> Option<Value> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let was_float = matches!(last, Value::Number(n) if n.is_f64());
    if !was_float && !t.contains(['.', 'e', 'E']) {
        if let Ok(i) = t.parse::<i64>() {
            return Some(Value::from(i));
        }
    }
    serde_json::Number::from_f64(t.parse::<f64>().ok()?).map(Value::Number)
}

fn plus_button(
    id: impl Into<ElementId>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(24.))
        .flex_shrink_0()
        .rounded(px(4.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(Tokens::button())
        .text_color(Tokens::secondary())
        .hover(|s| s.bg(Tokens::raised()).text_color(Tokens::foreground()))
        .on_click(on_click)
        .child(Icon::new(IconName::Plus).xsmall())
}
