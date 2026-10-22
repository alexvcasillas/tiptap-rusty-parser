//! Resizable, collapsible pane layout for both modes, persisted to
//! `layout.json` in the config dir (with the window bounds).
//!
//! Widths live in one `Layout` entity behind a global, so `Workspace` and
//! `DiffPanel` read the same numbers without new constructor plumbing. A drag
//! only rewrites a few `f32`s and notifies; the file is written 500 ms after
//! the last change (and on quit).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::component::resizable::{resize_handle, HandleEdge};
use gpui_kit::component::InteractiveElementExt;
use gpui_kit::prelude::*;
use gpui_kit::*;
use serde_json::{json, Value};

use crate::workspace::Mode;

gpui_kit::actions!(studio, [ToggleLeftPane, ToggleRightPane]);

const SAVE_DELAY: Duration = Duration::from_millis(500);

/// Minimum widths. Inspect: tree / preview / inspector. Diff: changes / each
/// tree / inspector.
const INSPECT_MIN: (f32, f32, f32) = (200., 360., 280.);
const DIFF_MIN: (f32, f32, f32) = (200., 260., 240.);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-1", ToggleLeftPane, Some("Studio")),
        KeyBinding::new("ctrl-1", ToggleLeftPane, Some("Studio")),
        KeyBinding::new("cmd-2", ToggleRightPane, Some("Studio")),
        KeyBinding::new("ctrl-2", ToggleRightPane, Some("Studio")),
    ]);
}

/// One mode's panes: fixed side widths, collapsed flags and, in Diff mode,
/// A's share of the space between the side panes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Panes {
    pub left: f32,
    pub right: f32,
    pub split: f32,
    pub left_open: bool,
    pub right_open: bool,
}

impl Panes {
    const INSPECT: Panes = Panes {
        left: 280.,
        right: 320.,
        split: 0.5,
        left_open: true,
        right_open: true,
    };
    const DIFF: Panes = Panes {
        left: 300.,
        right: 300.,
        split: 0.5,
        left_open: true,
        right_open: true,
    };

    fn left_w(&self) -> f32 {
        if self.left_open {
            self.left
        } else {
            0.
        }
    }

    fn right_w(&self) -> f32 {
        if self.right_open {
            self.right
        } else {
            0.
        }
    }

    fn to_json(self) -> Value {
        json!({
            "left": self.left,
            "right": self.right,
            "split": self.split,
            "leftOpen": self.left_open,
            "rightOpen": self.right_open,
        })
    }

    /// Missing or out-of-range fields fall back to `d`.
    fn from_json(v: &Value, d: Panes) -> Panes {
        let num = |k: &str, lo: f32, hi: f32, def: f32| {
            v.get(k)
                .and_then(Value::as_f64)
                .map(|n| n as f32)
                .filter(|n| n.is_finite() && (lo..=hi).contains(n))
                .unwrap_or(def)
        };
        let flag = |k: &str, def: bool| v.get(k).and_then(Value::as_bool).unwrap_or(def);
        Panes {
            left: num("left", 100., 4000., d.left),
            right: num("right", 100., 4000., d.right),
            split: num("split", 0.05, 0.95, d.split),
            left_open: flag("leftOpen", d.left_open),
            right_open: flag("rightOpen", d.right_open),
        }
    }
}

/// Which divider a drag or double-click addresses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Split {
    Left,
    Middle,
    Right,
}

/// Drag payload for a pane handle.
#[derive(Clone, Copy)]
pub struct PaneDrag(Mode, Split);

/// Drag preview: nothing follows the cursor, the divider itself moves.
struct Ghost;

impl Render for Ghost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

pub struct Layout {
    pub inspect: Panes,
    pub diff: Panes,
    window: Option<WindowBounds>,
    changed_at: Instant,
    pending: bool,
    _save: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

struct LayoutGlobal(Entity<Layout>);

impl Global for LayoutGlobal {}

fn layout_file() -> Option<PathBuf> {
    crate::files::config_dir().map(|d| d.join("layout.json"))
}

/// Load `layout.json` (a corrupt file is replaced by defaults right away) and
/// install the global. Call once before opening the window.
pub fn init(cx: &mut App) {
    let file = layout_file();
    let text = file.as_ref().and_then(|f| std::fs::read_to_string(f).ok());
    let parsed = text.as_deref().map(serde_json::from_str::<Value>);
    let v = match &parsed {
        Some(Ok(v)) if v.is_object() => Some(v),
        _ => None,
    };
    let corrupt = text.is_some() && v.is_none();
    let null = Value::Null;
    let get = |k: &str| v.and_then(|v| v.get(k)).unwrap_or(&null);
    let layout = Layout {
        inspect: Panes::from_json(get("inspect"), Panes::INSPECT),
        diff: Panes::from_json(get("diff"), Panes::DIFF),
        window: window_from_json(get("window")),
        changed_at: Instant::now(),
        pending: false,
        _save: None,
        _subs: Vec::new(),
    };
    if corrupt {
        eprintln!("tiptap-studio: layout.json unreadable, using defaults");
        layout.write();
    }
    let entity = cx.new(|cx| {
        let quit = cx.on_app_quit(|l: &mut Layout, _| {
            if l.pending {
                l.pending = false;
                l.write();
            }
            async {}
        });
        Layout {
            _subs: vec![quit],
            ..layout
        }
    });
    cx.set_global(LayoutGlobal(entity));
}

/// The saved window bounds, if any.
pub fn saved_window(cx: &App) -> Option<WindowBounds> {
    cx.global::<LayoutGlobal>().0.read(cx).window
}

/// Track the window's bounds so the next launch reopens it there.
pub fn track_window(window: &mut Window, cx: &mut App) {
    let entity = cx.global::<LayoutGlobal>().0.clone();
    entity.update(cx, |layout, cx| {
        let sub = cx.observe_window_bounds(window, |l, window, cx| {
            let b = match window.window_bounds() {
                WindowBounds::Fullscreen(b) => WindowBounds::Windowed(b),
                b => b,
            };
            if l.window != Some(b) {
                l.window = Some(b);
                l.touch(cx);
            }
        });
        layout._subs.push(sub);
    });
}

fn window_to_json(b: WindowBounds) -> Value {
    let (maximized, b) = match b {
        WindowBounds::Maximized(b) => (true, b),
        WindowBounds::Windowed(b) | WindowBounds::Fullscreen(b) => (false, b),
    };
    json!({
        "x": f32::from(b.origin.x),
        "y": f32::from(b.origin.y),
        "w": f32::from(b.size.width),
        "h": f32::from(b.size.height),
        "maximized": maximized,
    })
}

fn window_from_json(v: &Value) -> Option<WindowBounds> {
    let n = |k: &str| {
        v.get(k)
            .and_then(Value::as_f64)
            .map(|n| n as f32)
            .filter(|n| n.is_finite())
    };
    let (x, y, w, h) = (n("x")?, n("y")?, n("w")?, n("h")?);
    if !(400. ..=20000.).contains(&w) || !(300. ..=20000.).contains(&h) {
        return None;
    }
    let b = Bounds::new(point(px(x), px(y)), size(px(w), px(h)));
    Some(
        if v.get("maximized").and_then(Value::as_bool) == Some(true) {
            WindowBounds::Maximized(b)
        } else {
            WindowBounds::Windowed(b)
        },
    )
}

impl Layout {
    fn panes_mut(&mut self, mode: Mode) -> &mut Panes {
        match mode {
            Mode::Inspect => &mut self.inspect,
            Mode::Diff => &mut self.diff,
        }
    }

    fn write(&self) {
        let Some(file) = layout_file() else {
            return;
        };
        let mut v = json!({
            "version": 1,
            "inspect": self.inspect.to_json(),
            "diff": self.diff.to_json(),
        });
        if let Some(w) = self.window {
            v["window"] = window_to_json(w);
        }
        let result = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| {
                let text = serde_json::to_string_pretty(&v).map_err(std::io::Error::other)?;
                std::fs::write(&file, text)
            });
        if let Err(e) = result {
            eprintln!("tiptap-studio: layout {}: {e}", file.display());
        }
    }

    /// Record a change; the file is written once nothing changed for
    /// `SAVE_DELAY`. One timer task per burst of changes, not one per event.
    fn touch(&mut self, cx: &mut Context<Self>) {
        self.changed_at = Instant::now();
        cx.notify();
        if self.pending {
            return;
        }
        self.pending = true;
        self._save = Some(cx.spawn(async move |this, cx| {
            let mut wait = SAVE_DELAY;
            loop {
                cx.background_executor().timer(wait).await;
                let left = this.update(cx, |l, _| {
                    let since = l.changed_at.elapsed();
                    if since >= SAVE_DELAY {
                        l.pending = false;
                        l.write();
                        None
                    } else {
                        Some(SAVE_DELAY - since)
                    }
                });
                match left {
                    Ok(Some(d)) => wait = d,
                    _ => break,
                }
            }
        }));
    }

    fn set(&mut self, mode: Mode, p: Panes, cx: &mut Context<Self>) {
        let cur = self.panes_mut(mode);
        if *cur != p {
            *cur = p;
            self.touch(cx);
        }
    }

    fn toggle(&mut self, mode: Mode, side: Split, cx: &mut Context<Self>) {
        let mut p = *self.panes_mut(mode);
        match side {
            Split::Left => p.left_open = !p.left_open,
            Split::Right => p.right_open = !p.right_open,
            Split::Middle => {}
        }
        self.set(mode, p, cx);
    }

    fn reset(&mut self, mode: Mode, split: Split, cx: &mut Context<Self>) {
        let d = match mode {
            Mode::Inspect => Panes::INSPECT,
            Mode::Diff => Panes::DIFF,
        };
        let mut p = *self.panes_mut(mode);
        match split {
            Split::Left => p.left = d.left,
            Split::Middle => p.split = d.split,
            Split::Right => p.right = d.right,
        }
        self.set(mode, p, cx);
    }

    /// Move divider `split` to window x `x` inside a row spanning `row`,
    /// clamped so every visible pane keeps its minimum width.
    fn drag(
        &mut self,
        mode: Mode,
        split: Split,
        x: Pixels,
        row: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let (min_l, min_m, min_r) = match mode {
            Mode::Inspect => INSPECT_MIN,
            Mode::Diff => DIFF_MIN,
        };
        // Diff has two middle panes.
        let mid = match mode {
            Mode::Inspect => min_m,
            Mode::Diff => min_m * 2.,
        };
        let x = f32::from(x);
        let (left, right) = (f32::from(row.left()), f32::from(row.right()));
        let total = right - left;
        let mut p = *self.panes_mut(mode);
        let clamp = |v: f32, lo: f32, hi: f32| v.min(hi).max(lo);
        match split {
            Split::Left => p.left = clamp(x - left, min_l, total - p.right_w() - mid).round(),
            Split::Right => p.right = clamp(right - x, min_r, total - p.left_w() - mid).round(),
            Split::Middle => {
                let area = (total - p.left_w() - p.right_w()).max(1.);
                let lo = (min_m / area).min(0.5);
                p.split = clamp((x - left - p.left_w()) / area, lo, 1. - lo);
            }
        }
        self.set(mode, p, cx);
    }
}

fn entity(cx: &App) -> Entity<Layout> {
    cx.global::<LayoutGlobal>().0.clone()
}

/// Toggle the left / right pane of `mode` (⌘1 / ⌘2).
pub fn toggle(mode: Mode, right: bool, cx: &mut App) {
    let side = if right { Split::Right } else { Split::Left };
    entity(cx).update(cx, |l, cx| l.toggle(mode, side, cx));
}

fn handle(mode: Mode, split: Split, edge: HandleEdge, l: &Entity<Layout>) -> impl IntoElement {
    let id = match (mode, split) {
        (Mode::Inspect, Split::Left) => "pane-handle-tree",
        (Mode::Inspect, _) => "pane-handle-inspector",
        (Mode::Diff, Split::Left) => "pane-handle-changes",
        (Mode::Diff, Split::Middle) => "pane-handle-ab",
        (Mode::Diff, Split::Right) => "pane-handle-diff-inspector",
    };
    let l = l.clone();
    resize_handle::<PaneDrag, Ghost>(id, Axis::Horizontal)
        .inside(edge)
        .on_drag(PaneDrag(mode, split), |_, _, _, cx| cx.new(|_| Ghost))
        .on_double_click(move |_, _, cx| l.update(cx, |l, cx| l.reset(mode, split, cx)))
}

fn row(id: &'static str, l: &Entity<Layout>, cx: &App) -> Stateful<Div> {
    let l = l.clone();
    div()
        .id(id)
        .flex()
        .flex_1()
        .min_w_0()
        .h_full()
        .when(cx.has_active_drag(), |d| d.cursor_col_resize())
        .on_drag_move(move |e: &DragMoveEvent<PaneDrag>, _, cx| {
            let PaneDrag(mode, split) = *e.drag(cx);
            let (x, bounds) = (e.event.position.x, e.bounds);
            l.update(cx, |l, cx| l.drag(mode, split, x, bounds, cx));
        })
}

fn side(w: f32) -> Div {
    div().relative().flex().flex_shrink_0().h_full().w(px(w))
}

fn fluid(grow: f32) -> Div {
    div()
        .relative()
        .flex()
        .flex_grow(grow)
        .flex_shrink_1()
        .flex_basis(px(0.))
        .min_w_0()
        .h_full()
}

/// Inspect mode: tree | preview | inspector.
pub fn inspect_row(
    tree: impl IntoElement,
    preview: impl IntoElement,
    inspector: impl IntoElement,
    cx: &App,
) -> Stateful<Div> {
    let l = entity(cx);
    let p = l.read(cx).inspect;
    let m = Mode::Inspect;
    row("inspect-row", &l, cx)
        .when(p.left_open, |r| {
            r.child(side(p.left).child(tree).child(handle(
                m,
                Split::Left,
                HandleEdge::Trailing,
                &l,
            )))
        })
        .child(fluid(1.).child(preview))
        .when(p.right_open, |r| {
            r.child(side(p.right).child(inspector).child(handle(
                m,
                Split::Right,
                HandleEdge::Leading,
                &l,
            )))
        })
}

/// Diff mode: changes | A | B | inspector. The side panes' own fixed widths
/// are overridden to fill their slot.
pub fn diff_row<S: Styled + IntoElement>(
    changes: S,
    a: impl IntoElement,
    b: impl IntoElement,
    inspector: S,
    cx: &App,
) -> Stateful<Div> {
    let l = entity(cx);
    let p = l.read(cx).diff;
    let m = Mode::Diff;
    row("diff-row", &l, cx)
        .when(p.left_open, |r| {
            r.child(side(p.left).child(changes.w_full()).child(handle(
                m,
                Split::Left,
                HandleEdge::Trailing,
                &l,
            )))
        })
        .child(
            fluid(p.split)
                .child(a)
                .child(handle(m, Split::Middle, HandleEdge::Trailing, &l)),
        )
        .child(fluid(1. - p.split).child(b))
        .when(p.right_open, |r| {
            r.child(side(p.right).child(inspector.w_full()).child(handle(
                m,
                Split::Right,
                HandleEdge::Leading,
                &l,
            )))
        })
}
