//! Title bar, toolbar and the three-pane layout. Owns the panel entities.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::inspector::InspectorPanel;
use crate::preview::PreviewPanel;
use crate::state::DocumentState;
use crate::theme::Tokens;
use crate::tree::TreePanel;

gpui_kit::actions!(
    studio,
    [Undo, Redo, DeleteNode, MoveUp, MoveDown, PrintJson]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-z", Undo, Some("Studio")),
        KeyBinding::new("ctrl-z", Undo, Some("Studio")),
        KeyBinding::new("cmd-shift-z", Redo, Some("Studio")),
        KeyBinding::new("ctrl-shift-z", Redo, Some("Studio")),
        KeyBinding::new("ctrl-y", Redo, Some("Studio")),
        KeyBinding::new("backspace", DeleteNode, Some("Studio")),
        KeyBinding::new("delete", DeleteNode, Some("Studio")),
        KeyBinding::new("alt-up", MoveUp, Some(crate::nav::CONTEXT)),
        KeyBinding::new("alt-down", MoveDown, Some(crate::nav::CONTEXT)),
    ]);
}

pub struct Workspace {
    state: Entity<DocumentState>,
    tree: Entity<TreePanel>,
    preview: Entity<PreviewPanel>,
    inspector: Entity<InspectorPanel>,
    focus: FocusHandle,
    _subs: Vec<Subscription>,
}

impl Workspace {
    pub fn new(state: Entity<DocumentState>, cx: &mut Context<Self>) -> Self {
        let tree = cx.new(|cx| TreePanel::new(state.clone(), cx));
        let preview = cx.new(|cx| PreviewPanel::new(state.clone(), cx));
        let inspector = cx.new(|cx| InspectorPanel::new(state.clone(), cx));
        let sub = cx.observe(&state, |_, _, cx| cx.notify());
        Self {
            state,
            tree,
            preview,
            inspector,
            focus: cx.focus_handle(),
            _subs: vec![sub],
        }
    }

    /// Focus target on launch, so arrow keys navigate immediately.
    pub fn tree_focus(&self, cx: &App) -> FocusHandle {
        self.tree.read(cx).focus_handle()
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |s, cx| s.undo(cx));
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |s, cx| s.redo(cx));
    }

    fn delete_node(&mut self, _: &DeleteNode, _: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |s, cx| s.delete_selected(cx));
    }

    fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |s, cx| s.move_selected(-1, cx));
    }

    fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |s, cx| s.move_selected(1, cx));
    }

    fn print_json(&mut self, _: &PrintJson, _: &mut Window, cx: &mut Context<Self>) {
        println!("{}", self.state.read(cx).to_json_pretty());
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        let s = self.state.read(cx);
        let (nodes, texts, unknown) = s.stats();
        let file = s.file_name();
        let dirty = s.dirty;
        let can_undo = s.can_undo();
        let can_redo = s.can_redo();

        div()
            .id("studio-root")
            .key_context("Studio")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::delete_node))
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::print_json))
            .flex()
            .flex_col()
            .size_full()
            .bg(Tokens::background())
            .text_color(Tokens::foreground())
            .text_size(px(13.))
            // title bar
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .h(px(38.))
                    .flex_shrink_0()
                    .px(px(14.))
                    .bg(Tokens::title_bar())
                    .border_b_1()
                    .border_color(Tokens::border())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(Tokens::secondary())
                            .child(
                                div()
                                    .text_color(Tokens::accent())
                                    .child(Icon::new(IconName::ListTree).small()),
                            )
                            .child("tiptap-studio"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .ml(px(8.))
                            .p(px(2.))
                            .rounded(px(7.))
                            .bg(Tokens::panel())
                            .border_1()
                            .border_color(Tokens::border())
                            .child(mode_tab("Inspect", true))
                            .child(mode_tab("Diff", false)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .justify_center()
                            .gap(px(6.))
                            .font_family(mono.clone())
                            .text_size(px(12.))
                            .text_color(Tokens::muted())
                            .child(file)
                            .when(dirty, |d| {
                                d.child(div().text_color(Tokens::unknown()).child("●"))
                            }),
                    )
                    .child(
                        div()
                            .font_family(mono.clone())
                            .text_size(px(11.))
                            .text_color(Tokens::muted())
                            .child(format!(
                                "{nodes} nodes · {texts} text · {unknown} unknown type{}",
                                if unknown == 1 { "" } else { "s" }
                            )),
                    ),
            )
            // toolbar
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .h(px(40.))
                    .flex_shrink_0()
                    .px(px(10.))
                    .bg(Tokens::panel())
                    .border_b_1()
                    .border_color(Tokens::border())
                    .child(tool("open", IconName::FolderOpen, "Open").disabled(true))
                    .child(tool("save", IconName::Save, "Save").disabled(true))
                    .child(sep())
                    .child(
                        tool("undo", IconName::Undo, "Undo")
                            .disabled(!can_undo)
                            .on_click(cx.listener(|this, _, w, cx| this.undo(&Undo, w, cx))),
                    )
                    .child(
                        tool("redo", IconName::Redo, "Redo")
                            .disabled(!can_redo)
                            .on_click(cx.listener(|this, _, w, cx| this.redo(&Redo, w, cx))),
                    )
                    .child(sep())
                    .child(
                        tool("json", IconName::Braces, "JSON").on_click(
                            cx.listener(|this, _, w, cx| this.print_json(&PrintJson, w, cx)),
                        ),
                    )
                    .child(tool("schema", IconName::ShieldCheck, "Schema · none").disabled(true))
                    .child(div().flex_1()),
            )
            // panes
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.tree.clone())
                    .child(self.preview.clone())
                    .child(self.inspector.clone()),
            )
    }
}

fn tool(id: &'static str, icon: IconName, label: &'static str) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .icon(Icon::new(icon).small())
        .label(label)
}

fn sep() -> Div {
    div().w(px(1.)).h(px(18.)).mx(px(6.)).bg(Tokens::rule())
}

fn mode_tab(label: &'static str, active: bool) -> Div {
    div()
        .h(px(24.))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .text_size(px(12.))
        .when(active, |d| {
            d.bg(Tokens::border())
                .text_color(Tokens::foreground())
                .font_weight(FontWeight::MEDIUM)
        })
        .when(!active, |d| d.text_color(Tokens::muted()))
        .child(label)
}
