//! Keyboard navigation for the focusable document panes (tree, preview).
//!
//! Semantics follow tree views everywhere: ↑/↓ step through visible rows,
//! ← collapses or jumps to the parent, → expands or steps into the first
//! child, Home/End jump to the ends, Esc clears the selection.

use gpui_kit::*;

use crate::state::DocumentState;

gpui_kit::actions!(
    document,
    [
        SelectPrev,
        SelectNext,
        SelectOut,
        SelectIn,
        SelectFirst,
        SelectLast,
        ClearSelection
    ]
);

pub const CONTEXT: &str = "Document";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectPrev, Some(CONTEXT)),
        KeyBinding::new("down", SelectNext, Some(CONTEXT)),
        KeyBinding::new("left", SelectOut, Some(CONTEXT)),
        KeyBinding::new("right", SelectIn, Some(CONTEXT)),
        KeyBinding::new("home", SelectFirst, Some(CONTEXT)),
        KeyBinding::new("end", SelectLast, Some(CONTEXT)),
        KeyBinding::new("escape", ClearSelection, Some(CONTEXT)),
    ]);
}

/// Attach the navigation key context and action handlers to a pane root.
pub fn attach<E: InteractiveElement>(el: E, state: &Entity<DocumentState>) -> E {
    let (s1, s2, s3, s4, s5, s6, s7) = (
        state.clone(),
        state.clone(),
        state.clone(),
        state.clone(),
        state.clone(),
        state.clone(),
        state.clone(),
    );
    el.key_context(CONTEXT)
        .on_action(move |_: &SelectPrev, _, cx| s1.update(cx, |s, cx| s.nav_step(-1, cx)))
        .on_action(move |_: &SelectNext, _, cx| s2.update(cx, |s, cx| s.nav_step(1, cx)))
        .on_action(move |_: &SelectOut, _, cx| s3.update(cx, |s, cx| s.nav_out(cx)))
        .on_action(move |_: &SelectIn, _, cx| s4.update(cx, |s, cx| s.nav_in(cx)))
        .on_action(move |_: &SelectFirst, _, cx| s5.update(cx, |s, cx| s.nav_edge(false, cx)))
        .on_action(move |_: &SelectLast, _, cx| s6.update(cx, |s, cx| s.nav_edge(true, cx)))
        .on_action(move |_: &ClearSelection, _, cx| s7.update(cx, |s, cx| s.select(None, cx)))
}
