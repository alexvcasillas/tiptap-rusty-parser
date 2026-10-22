//! Keyboard navigation for the focusable document panes (tree, preview).
//!
//! Semantics follow tree views everywhere: ↑/↓ step through visible rows,
//! ← collapses or jumps to the parent, → expands or steps into the first
//! child, Home/End jump to the ends, Esc clears the selection.
//! Editing keys in the same context: ⏎ new paragraph after, ⌘D duplicate,
//! ⌘C/⌘X/⌘V copy/cut/paste nodes as JSON. Inspector inputs are outside this
//! context, so none of these fire while typing.

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
        ClearSelection,
        InsertParagraph,
        Duplicate,
        CopyNode,
        CutNode,
        PasteNode,
        CancelJsonEdit
    ]
);

pub const CONTEXT: &str = "Document";
/// Inspector `Edit JSON` textarea wrapper: Esc cancels.
pub const JSON_EDIT: &str = "JsonEdit";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectPrev, Some(CONTEXT)),
        KeyBinding::new("down", SelectNext, Some(CONTEXT)),
        KeyBinding::new("left", SelectOut, Some(CONTEXT)),
        KeyBinding::new("right", SelectIn, Some(CONTEXT)),
        KeyBinding::new("home", SelectFirst, Some(CONTEXT)),
        KeyBinding::new("end", SelectLast, Some(CONTEXT)),
        KeyBinding::new("escape", ClearSelection, Some(CONTEXT)),
        KeyBinding::new("enter", InsertParagraph, Some(CONTEXT)),
        KeyBinding::new("cmd-d", Duplicate, Some(CONTEXT)),
        KeyBinding::new("ctrl-d", Duplicate, Some(CONTEXT)),
        KeyBinding::new("cmd-c", CopyNode, Some(CONTEXT)),
        KeyBinding::new("ctrl-c", CopyNode, Some(CONTEXT)),
        KeyBinding::new("cmd-x", CutNode, Some(CONTEXT)),
        KeyBinding::new("ctrl-x", CutNode, Some(CONTEXT)),
        KeyBinding::new("cmd-v", PasteNode, Some(CONTEXT)),
        KeyBinding::new("ctrl-v", PasteNode, Some(CONTEXT)),
        KeyBinding::new("escape", CancelJsonEdit, Some(JSON_EDIT)),
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
    let el = el
        .key_context(CONTEXT)
        .on_action(move |_: &SelectPrev, _, cx| s1.update(cx, |s, cx| s.nav_step(-1, cx)))
        .on_action(move |_: &SelectNext, _, cx| s2.update(cx, |s, cx| s.nav_step(1, cx)))
        .on_action(move |_: &SelectOut, _, cx| s3.update(cx, |s, cx| s.nav_out(cx)))
        .on_action(move |_: &SelectIn, _, cx| s4.update(cx, |s, cx| s.nav_in(cx)))
        .on_action(move |_: &SelectFirst, _, cx| s5.update(cx, |s, cx| s.nav_edge(false, cx)))
        .on_action(move |_: &SelectLast, _, cx| s6.update(cx, |s, cx| s.nav_edge(true, cx)))
        .on_action(move |_: &ClearSelection, _, cx| s7.update(cx, |s, cx| s.select(None, cx)));
    attach_edit(el, state)
}

/// Node creation and clipboard handlers (same context as navigation).
fn attach_edit<E: InteractiveElement>(el: E, state: &Entity<DocumentState>) -> E {
    let (s1, s2, s3, s4, s5) = (
        state.clone(),
        state.clone(),
        state.clone(),
        state.clone(),
        state.clone(),
    );
    el.on_action(move |_: &InsertParagraph, _, cx| {
        s1.update(cx, |s, cx| s.insert_paragraph_after(cx))
    })
    .on_action(move |_: &Duplicate, _, cx| s2.update(cx, |s, cx| s.duplicate_selected(cx)))
    .on_action(move |_: &CopyNode, _, cx| {
        if let Some(json) = s3.update(cx, |s, cx| s.copy_selected(cx)) {
            cx.write_to_clipboard(ClipboardItem::new_string(json));
        }
    })
    .on_action(move |_: &CutNode, _, cx| {
        if let Some(json) = s4.update(cx, |s, cx| s.cut_selected(cx)) {
            cx.write_to_clipboard(ClipboardItem::new_string(json));
        }
    })
    .on_action(move |_: &PasteNode, _, cx| {
        let text = cx.read_from_clipboard().and_then(|c| c.text());
        s5.update(cx, |s, cx| s.paste(text.as_deref(), cx))
    })
}
