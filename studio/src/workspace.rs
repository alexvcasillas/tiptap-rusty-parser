//! Title bar, toolbar and the three-pane layout. Owns the panel entities.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;

use std::path::PathBuf;

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::Selectable;

use crate::diff::{DiffPanel, DiffState};
use crate::inspector::InspectorPanel;
use crate::preview::PreviewPanel;
use crate::schema_panel::{NextProblem, PrevProblem, ProblemsPanel, SchemaState};
use crate::state::DocumentState;
use crate::theme::Tokens;
use crate::tree::TreePanel;

gpui_kit::actions!(
    studio,
    [Undo, Redo, DeleteNode, MoveUp, MoveDown, PrintJson, Save, SaveAs]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-z", Undo, Some("Studio")),
        KeyBinding::new("ctrl-z", Undo, Some("Studio")),
        KeyBinding::new("cmd-shift-z", Redo, Some("Studio")),
        KeyBinding::new("ctrl-shift-z", Redo, Some("Studio")),
        KeyBinding::new("ctrl-y", Redo, Some("Studio")),
        KeyBinding::new("cmd-s", Save, Some("Studio")),
        KeyBinding::new("ctrl-s", Save, Some("Studio")),
        KeyBinding::new("cmd-shift-s", SaveAs, Some("Studio")),
        KeyBinding::new("ctrl-shift-s", SaveAs, Some("Studio")),
        KeyBinding::new("backspace", DeleteNode, Some("Studio")),
        KeyBinding::new("delete", DeleteNode, Some("Studio")),
        KeyBinding::new("alt-up", MoveUp, Some(crate::nav::CONTEXT)),
        KeyBinding::new("alt-down", MoveDown, Some(crate::nav::CONTEXT)),
    ]);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Inspect,
    Diff,
}

pub struct Workspace {
    pub(crate) state: Entity<DocumentState>,
    pub(crate) mode: Mode,
    pub(crate) diff: Entity<DiffState>,
    pub(crate) files: crate::files::Files,
    diff_panel: Entity<DiffPanel>,
    tree: Entity<TreePanel>,
    preview: Entity<PreviewPanel>,
    inspector: Entity<InspectorPanel>,
    schema: Entity<SchemaState>,
    problems: Entity<ProblemsPanel>,
    source: Entity<crate::source::SourcePanel>,
    focus: FocusHandle,
    _subs: Vec<Subscription>,
}

impl Workspace {
    pub fn new(
        state: Entity<DocumentState>,
        file_b: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        let tree = cx.new(|cx| TreePanel::new(state.clone(), cx));
        let preview = cx.new(|cx| PreviewPanel::new(state.clone(), cx));
        let inspector = cx.new(|cx| InspectorPanel::new(state.clone(), cx));
        let diff = cx.new(|cx| DiffState::new(state.clone(), file_b, cx));
        let diff_panel = cx.new(|cx| DiffPanel::new(diff.clone(), cx));
        let sub = cx.observe(&state, |_, _, cx| cx.notify());
        let diff_sub = cx.observe(&diff, |_, _, cx| cx.notify());
        let schema = cx.new(|cx| {
            let mut s = SchemaState::new(state.clone(), cx);
            s.watch_diff(diff.clone(), cx);
            s
        });
        let problems = cx.new(|cx| ProblemsPanel::new(schema.clone(), state.clone(), cx));
        tree.update(cx, |t, cx| t.set_schema(schema.clone(), cx));
        inspector.update(cx, |i, cx| i.set_schema(schema.clone(), cx));
        let schema_sub = cx.observe(&schema, |_, _, cx| cx.notify());
        let source = cx.new(|cx| crate::source::SourcePanel::new(state.clone(), cx));
        Self {
            state,
            mode: Mode::Inspect,
            diff,
            files: crate::files::Files::default(),
            diff_panel,
            tree,
            preview,
            inspector,
            schema,
            problems,
            source,
            focus: cx.focus_handle(),
            _subs: vec![sub, diff_sub, schema_sub],
        }
    }

    /// Schema validation state (`--schema` in `main.rs`).
    pub fn schema(&self) -> Entity<SchemaState> {
        self.schema.clone()
    }

    fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        let focus = match mode {
            Mode::Inspect => self.tree.read(cx).focus_handle(),
            Mode::Diff => self.diff_panel.read(cx).focus_handle(),
        };
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Preview ↔ Source (⌘3). The hidden pane may hold focus: hand it to the tree.
    fn toggle_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::layout::toggle_source(cx);
        window.focus(&self.tree.read(cx).focus_handle(), cx);
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
        if self.mode == Mode::Inspect {
            self.state.update(cx, |s, cx| s.delete_selected(cx));
        }
    }

    fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Inspect {
            self.state.update(cx, |s, cx| s.move_selected(-1, cx));
        }
    }

    fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Inspect {
            self.state.update(cx, |s, cx| s.move_selected(1, cx));
        }
    }

    fn print_json(&mut self, _: &PrintJson, _: &mut Window, cx: &mut Context<Self>) {
        println!("{}", self.state.read(cx).to_json_pretty());
    }

    /// Save A in place; the bundled sample has no file, so it asks for one.
    fn save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        match self.state.read(cx).file.clone() {
            Some(p) => {
                self.state.update(cx, |s, cx| s.save_to(&p, cx));
                self.after_save(cx);
            }
            None => self.save_as(&SaveAs, window, cx),
        }
    }

    /// Save A under a new name. With a B loaded the suggestion is
    /// `<a>-merged.json`, so the merge never overwrites the original by accident.
    fn save_as(&mut self, _: &SaveAs, _: &mut Window, cx: &mut Context<Self>) {
        let s = self.state.read(cx);
        let dir = s
            .file
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let stem = s
            .file
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "document".into());
        let name = if self.diff.read(cx).has_b() {
            format!("{stem}-merged.json")
        } else {
            format!("{stem}.json")
        };
        let rx = cx.prompt_for_new_path(&dir, Some(&name));
        let state = self.state.clone();
        cx.spawn(async move |this, cx| match rx.await {
            Ok(Ok(Some(path))) => {
                state.update(cx, |s, cx| s.save_to(&path, cx));
                this.update(cx, |ws, cx| ws.after_save(cx)).ok();
            }
            Ok(Ok(None)) => {} // cancelled
            Ok(Err(e)) => eprintln!("tiptap-studio: save prompt failed: {e}"),
            Err(_) => {}
        })
        .detach();
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::profile::frame(window);
        self.sync_title(window, cx);
        let files_error = self.error_view(cx);
        let drop_overlay = self.drop_overlay(cx);
        let mono = cx.theme().mono_font_family.clone();
        let s = self.state.read(cx);
        let (nodes, texts, unknown) = s.stats();
        let file = match &self.files.start_error {
            Some(e) => e.name(),
            None => s.file_name(),
        };
        let dirty = s.dirty;
        let has_file = s.file.is_some();
        let can_undo = s.can_undo();
        let can_redo = s.can_redo();
        let mode = self.mode;
        let d = self.diff.read(cx);
        let (added, removed, changed) = d.counts;
        let total = d.total();
        let kept = d.kept_count();
        let has_b = d.has_b();
        let b_file = d.b_name();
        let only_changed = d.only_changed;
        let sch = self.schema.read(cx);
        let schema_issues = sch
            .b_issues
            .map(|b| format!("· schema A {} · B {b}", sch.issue_count()));

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
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::save_as))
            .on_action(cx.listener(Self::open_file))
            .on_action(cx.listener(Self::quit))
            .on_action(move |_: &crate::layout::ToggleLeftPane, _, cx| {
                crate::layout::toggle(mode, false, cx)
            })
            .on_action(move |_: &crate::layout::ToggleRightPane, _, cx| {
                crate::layout::toggle(mode, true, cx)
            })
            .on_action(
                cx.listener(|this, _: &crate::layout::ToggleSource, w, cx| {
                    this.toggle_source(w, cx)
                }),
            )
            .on_drop(cx.listener(|this, paths: &ExternalPaths, w, cx| {
                this.drop_paths(paths, false, w, cx)
            }))
            .on_action(cx.listener(|this, _: &NextProblem, _, cx| {
                this.schema.update(cx, |s, cx| s.step(1, cx))
            }))
            .on_action(cx.listener(|this, _: &PrevProblem, _, cx| {
                this.schema.update(cx, |s, cx| s.step(-1, cx))
            }))
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
                            .child(mode_tab("Inspect", mode == Mode::Inspect).on_click(
                                cx.listener(|this, _, w, cx| this.set_mode(Mode::Inspect, w, cx)),
                            ))
                            .child(mode_tab("Diff", mode == Mode::Diff).on_click(
                                cx.listener(|this, _, w, cx| this.set_mode(Mode::Diff, w, cx)),
                            )),
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
                            })
                            .when(mode == Mode::Diff && has_b, |d| {
                                d.child(div().text_color(Tokens::disabled()).child("→"))
                                    .child(b_file.clone())
                            }),
                    )
                    .child(match mode {
                        Mode::Inspect => div()
                            .font_family(mono.clone())
                            .text_size(px(11.))
                            .text_color(Tokens::muted())
                            .child(format!(
                                "{nodes} nodes · {texts} text · {unknown} unknown type{}",
                                if unknown == 1 { "" } else { "s" }
                            )),
                        Mode::Diff if !has_b => div(),
                        Mode::Diff => div()
                            .flex()
                            .gap(px(8.))
                            .font_family(mono.clone())
                            .text_size(px(11.))
                            .text_color(Tokens::muted())
                            .child(div().text_color(Tokens::added()).child(format!("+{added}")))
                            .child(
                                div()
                                    .text_color(Tokens::removed())
                                    .child(format!("−{removed}")),
                            )
                            .child(
                                div()
                                    .text_color(Tokens::unknown())
                                    .child(format!("~{changed}")),
                            )
                            .child(format!(
                                "· {total} change{}{}",
                                if total == 1 { "" } else { "s" },
                                if kept > 0 {
                                    format!(" · {kept} dismissed")
                                } else {
                                    String::new()
                                }
                            ))
                            .when_some(schema_issues, |d, s| d.child(s)),
                    }),
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
                    .overflow_hidden()
                    .bg(Tokens::panel())
                    .border_b_1()
                    .border_color(Tokens::border())
                    .child(self.open_button(cx))
                    .child(
                        tool("save", IconName::Save, "Save")
                            .disabled(!dirty && has_file)
                            .tooltip("Save A (⌘S)")
                            .on_click(cx.listener(|this, _, w, cx| this.save(&Save, w, cx))),
                    )
                    .child(
                        tool("save-as", IconName::FilePlus, "Save as…")
                            .tooltip("Save A as a new file (⇧⌘S)")
                            .on_click(cx.listener(|this, _, w, cx| this.save_as(&SaveAs, w, cx))),
                    )
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
                        tool("server", IconName::Cloud, "Server…")
                            .tooltip("Browse a Tiptap Document Server (read-only)")
                            .on_click(cx.listener(|this, _, w, cx| {
                                let ws = cx.entity().downgrade();
                                let on_diff: crate::cloud::OnDiff =
                                    std::rc::Rc::new(move |w, cx| {
                                        ws.update(cx, |ws, cx| ws.set_mode(Mode::Diff, w, cx)).ok();
                                    });
                                crate::cloud::open(
                                    this.state.clone(),
                                    this.diff.clone(),
                                    cx.entity().downgrade(),
                                    on_diff,
                                    w,
                                    cx,
                                );
                            })),
                    )
                    .map(|bar| match mode {
                        Mode::Inspect => bar
                            .child(tool("json", IconName::Braces, "JSON").on_click(
                                cx.listener(|this, _, w, cx| this.print_json(&PrintJson, w, cx)),
                            ))
                            .child({
                                let on = crate::layout::show_source(cx);
                                let icon = Icon::new(IconName::FileCode).small();
                                Button::new("source")
                                    .ghost()
                                    .small()
                                    .selected(on)
                                    .icon(if on {
                                        icon.text_color(Tokens::accent())
                                    } else {
                                        icon
                                    })
                                    .label("Source")
                                    .tooltip("Show the JSON source instead of the preview (⌘3)")
                                    .on_click(
                                        cx.listener(|this, _, w, cx| this.toggle_source(w, cx)),
                                    )
                            })
                            .child(crate::schema_panel::toolbar_item(&self.schema, cx))
                            .child(div().flex_1()),
                        Mode::Diff => {
                            let (open, apply, only) =
                                (self.diff.clone(), self.diff.clone(), self.diff.clone());
                            bar.child(
                                tool("open-b", IconName::FolderOpen, "Open B…").on_click(
                                    move |_, _, cx| crate::diff::open_b(open.clone(), cx),
                                ),
                            )
                            .child(
                                tool("swap", IconName::ArrowLeftRight, "Swap")
                                    .disabled(!has_b)
                                    .tooltip(crate::diff::SWAP_TOOLTIP)
                                    .on_click(crate::diff::swap_handler(&self.diff)),
                            )
                            .child(
                                tool("apply-all", IconName::CheckCheck, "Apply all to A")
                                    .disabled(total == 0)
                                    .on_click(move |_, _, cx| {
                                        apply.update(cx, |d, cx| d.apply_all(cx))
                                    }),
                            )
                            .child(sep())
                            .child(
                                Checkbox::new("only-changed")
                                    .small()
                                    .label("Only changed")
                                    .checked(only_changed)
                                    .on_click(move |on, _, cx| {
                                        only.update(cx, |d, cx| d.set_only_changed(*on, cx))
                                    }),
                            )
                            .child(div().w(px(12.)))
                            .child(
                                Checkbox::new("sync-scroll")
                                    .small()
                                    .label("Sync scroll")
                                    .checked(self.diff.read(cx).sync_scroll)
                                    .on_click(crate::diff::sync_scroll_handler(&self.diff)),
                            )
                            .child(div().flex_1())
                        }
                    })
                    .child(crate::find::bar(&self.state, cx)),
            )
            .children(self.banner_view(cx))
            // panes
            .child(div().relative().flex().flex_1().min_h_0().map(|panes| {
                match mode {
                    _ if files_error.is_some() => panes.children(files_error),
                    Mode::Inspect => panes.child(crate::layout::inspect_row(
                        self.tree.clone(),
                        // preview + Problems drawer (schema_panel.rs)
                        div()
                            .flex()
                            .flex_col()
                            .size_full()
                            .min_w_0()
                            .min_h_0()
                            .map(|d| match crate::layout::show_source(cx) {
                                true => d.child(self.source.clone()),
                                false => d.child(self.preview.clone()),
                            })
                            .child(self.problems.clone()),
                        self.inspector.clone(),
                        cx,
                    )),
                    Mode::Diff => panes.child(self.diff_panel.clone()),
                }
                .children(drop_overlay)
            }))
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

fn mode_tab(label: &'static str, active: bool) -> Stateful<Div> {
    div()
        .id(label)
        .cursor_pointer()
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
