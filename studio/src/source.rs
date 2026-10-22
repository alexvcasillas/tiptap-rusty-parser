//! Center pane alternative: read-only pretty JSON of A with line numbers,
//! synced with the selection through the crate's `SourceMap`.
//!
//! The text, its line index and the map are rebuilt only when the document
//! revision changes, and lazily (only while the pane is shown). Rows are a
//! `uniform_list` by line; each frame styles only the visible lines.

use std::ops::Range;
use std::rc::Rc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::*;
use gpui_kit::*;
use tiptap_rusty_parser::SourceMap;

use crate::files::LoadError;
use crate::state::{fmt_path, DocumentState};
use crate::theme::Tokens;
use crate::tree::pane_header;

const ROW_H: f32 = 20.;
/// Lines kept above a revealed node when the view scrolls to it.
const CONTEXT_LINES: usize = 3;

#[derive(Default)]
struct Source {
    text: String,
    /// Byte offset of each line start.
    lines: Vec<usize>,
    map: SourceMap,
}

impl Source {
    fn build(state: &DocumentState) -> Self {
        let _p = crate::profile::span("source text+map");
        let (text, map) = SourceMap::pretty(state.root());
        let mut lines = Vec::with_capacity(text.len() / 24 + 1);
        lines.push(0);
        lines.extend(
            text.bytes()
                .enumerate()
                .filter(|&(_, b)| b == b'\n')
                .map(|(i, _)| i + 1),
        );
        Self { text, lines, map }
    }

    fn line_of(&self, offset: usize) -> usize {
        self.lines
            .partition_point(|&s| s <= offset)
            .saturating_sub(1)
    }

    fn line(&self, i: usize) -> &str {
        let start = self.lines[i];
        let end = self.lines.get(i + 1).map_or(self.text.len(), |&e| e - 1);
        &self.text[start..end]
    }

    /// First and last line of the node at `path`.
    fn line_span(&self, path: &[usize]) -> Option<(usize, usize)> {
        let r = self.map.span(path)?;
        Some((self.line_of(r.start), self.line_of(r.end.saturating_sub(1))))
    }

    /// Deepest node containing line `i` (its first non-blank byte).
    fn path_on_line(&self, i: usize) -> Option<Vec<usize>> {
        let line = self.line(i);
        let indent = line.len() - line.trim_start().len();
        self.map.path_at(self.lines[i] + indent)
    }
}

pub struct SourcePanel {
    state: Entity<DocumentState>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    src: Rc<Source>,
    seen_revision: Option<u64>,
    seen_selection: Option<Vec<usize>>,
    /// Selected node's line span.
    hl: Option<(usize, usize)>,
    /// Set by a click in this pane: that selection change must not scroll.
    clicked: Option<Vec<usize>>,
    _subs: Vec<Subscription>,
}

impl SourcePanel {
    pub fn new(state: Entity<DocumentState>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&state, |_, _, cx| cx.notify());
        Self {
            state,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            src: Rc::default(),
            seen_revision: None,
            seen_selection: None,
            hl: None,
            clicked: None,
            _subs: vec![sub],
        }
    }

    /// Rebuild on a new revision; re-highlight / reveal on a new selection.
    /// Runs from `render`, so a hidden pane does no work.
    fn sync(&mut self, cx: &App) {
        let s = self.state.read(cx);
        let rebuilt = self.seen_revision != Some(s.revision);
        if rebuilt {
            self.seen_revision = Some(s.revision);
            self.src = Rc::new(Source::build(s));
        }
        if rebuilt || self.seen_selection != s.selection {
            let moved = self.seen_selection != s.selection;
            self.seen_selection = s.selection.clone();
            self.hl = s.selection.as_deref().and_then(|p| self.src.line_span(p));
            let from_click = self.clicked.take().is_some_and(|c| Some(c) == s.selection);
            if let (true, false, Some((lo, _))) = (moved, from_click, self.hl) {
                self.scroll
                    .scroll_to_item_with_offset(lo, ScrollStrategy::Top, CONTEXT_LINES);
            }
        }
    }
}

impl Render for SourcePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync(cx);
        let mono = cx.theme().mono_font_family.clone();
        let focused = self.focus.contains_focused(window, cx);
        let src = self.src.clone();
        let count = src.lines.len();
        let digits = count.to_string().len();
        let gutter = px(digits as f32 * 7.5 + 20.);
        let hl = self.hl;
        let header_info: SharedString = match (&self.seen_selection, hl) {
            (Some(p), Some((lo, hi))) => {
                format!("{} · lines {}–{} of {count}", fmt_path(p), lo + 1, hi + 1).into()
            }
            _ => format!("{count} lines · click a line to select its node").into(),
        };
        let state = self.state.clone();
        let focus = self.focus.clone();
        let this = cx.entity().downgrade();
        let rows = uniform_list("source-lines", count, move |range: Range<usize>, _, _| {
            range
                .map(|i| {
                    let (state, focus, this, src2) =
                        (state.clone(), focus.clone(), this.clone(), src.clone());
                    let on = hl.is_some_and(|(lo, hi)| (lo..=hi).contains(&i));
                    let edge = hl.is_some_and(|(lo, hi)| i == lo || i == hi);
                    let text = src.line(i);
                    div()
                        .id(ElementId::NamedInteger("source-line".into(), i as u64))
                        .flex()
                        .h(px(ROW_H))
                        .w_full()
                        .cursor_pointer()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .when(on, |d| d.bg(Tokens::accent_fill()))
                        .when(!on, |d| d.hover(|s| s.bg(Tokens::raised())))
                        .on_click(move |_, window, cx| {
                            window.focus(&focus, cx);
                            let path = src2.path_on_line(i);
                            this.update(cx, |p, _| p.clicked = path.clone()).ok();
                            state.update(cx, |s, cx| s.select(path, cx));
                        })
                        .child(
                            div()
                                .flex_shrink_0()
                                .w(gutter)
                                .pr(px(12.))
                                .flex()
                                .justify_end()
                                .border_r_2()
                                .border_color(if on {
                                    Tokens::accent()
                                } else {
                                    Tokens::background()
                                })
                                .text_color(if edge {
                                    Tokens::accent()
                                } else {
                                    Tokens::ghost()
                                })
                                .child(SharedString::from((i + 1).to_string())),
                        )
                        .child(
                            div()
                                .pl(px(12.))
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(highlight(text)),
                        )
                })
                .collect::<Vec<_>>()
        })
        .track_scroll(&self.scroll)
        .flex_1()
        .font_family(mono)
        .text_size(px(12.))
        .line_height(px(ROW_H));

        let focus = self.focus.clone();
        crate::nav::attach(div().id("source-pane"), &self.state)
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
                pane_header("Source")
                    .when(focused, |h| h.text_color(Tokens::accent()))
                    .child(
                        div()
                            .font_weight(FontWeight::NORMAL)
                            .text_color(Tokens::disabled())
                            .child(header_info),
                    ),
            )
            .child(rows.py(px(6.)))
    }
}

fn color(c: Hsla) -> HighlightStyle {
    HighlightStyle {
        color: Some(c),
        ..Default::default()
    }
}

/// JSON token colors for one pretty-printed line: keys, strings, scalars.
/// Strings never span lines in `serde_json` pretty output.
fn highlight(line: &str) -> StyledText {
    let b = line.as_bytes();
    let mut hl = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                let start = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i = (i + 1).min(b.len());
                let key = b[i..].iter().find(|c| !c.is_ascii_whitespace()) == Some(&b':');
                let c = if key {
                    Tokens::moved()
                } else {
                    Tokens::added()
                };
                hl.push((start..i, color(c)));
            }
            b'-' | b'0'..=b'9' | b't' | b'f' | b'n' => {
                let start = i;
                while i < b.len() && !matches!(b[i], b',' | b'}' | b']' | b' ') {
                    i += 1;
                }
                hl.push((start..i, color(Tokens::unknown())));
            }
            _ => i += 1,
        }
    }
    StyledText::new(SharedString::from(line.to_string())).with_highlights(hl)
}

/// Lines around a parse error, with the error column marked; shown in the
/// "Could not open" view (`files.rs`).
pub fn error_excerpt(e: &LoadError, cx: &App) -> Option<Div> {
    let (line, col) = e.location?;
    if e.excerpt.is_empty() {
        return None;
    }
    let digits = e.excerpt.last().map_or(1, |(n, _)| n.to_string().len());
    let gutter = px(digits as f32 * 7.5 + 20.);
    Some(
        div()
            .w(px(640.))
            .py(px(6.))
            .rounded(px(6.))
            .bg(Tokens::code_bg())
            .border_1()
            .border_color(Tokens::border())
            .font_family(cx.theme().mono_font_family.clone())
            .text_size(px(12.))
            .line_height(px(ROW_H))
            .children(e.excerpt.iter().map(|(n, text)| {
                let at = *n == line;
                let caret = at.then(|| {
                    // Column is 1-based in chars; 0 means "end of input".
                    let pad: String = text
                        .chars()
                        .take(col.saturating_sub(1))
                        .map(|c| if c == '\t' { '\t' } else { ' ' })
                        .collect();
                    format!("{pad}^")
                });
                div()
                    .flex()
                    .flex_col()
                    .when(at, |d| d.bg(Tokens::unknown_fill()))
                    .child(
                        div()
                            .flex()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .w(gutter)
                                    .pr(px(12.))
                                    .flex()
                                    .justify_end()
                                    .text_color(if at {
                                        Tokens::removed()
                                    } else {
                                        Tokens::ghost()
                                    })
                                    .child(SharedString::from(n.to_string())),
                            )
                            .child(
                                div()
                                    .pl(px(12.))
                                    .min_w_0()
                                    .text_ellipsis()
                                    .text_color(Tokens::secondary())
                                    .child(text.clone()),
                            ),
                    )
                    .when_some(caret, |d, c| {
                        d.child(
                            div()
                                .flex()
                                .whitespace_nowrap()
                                .child(div().flex_shrink_0().w(gutter))
                                .child(div().pl(px(12.)).text_color(Tokens::removed()).child(c)),
                        )
                    })
            })),
    )
}

/// Up to `around` lines either side of 1-based `line`, for [`error_excerpt`].
pub fn excerpt(text: &str, line: usize, around: usize) -> Vec<(usize, SharedString)> {
    let lo = line.saturating_sub(around).max(1);
    text.lines()
        .enumerate()
        .skip(lo - 1)
        .take(line + around + 1 - lo)
        .map(|(i, l)| {
            let l: String = l.chars().take(400).collect();
            (i + 1, l.into())
        })
        .collect()
}
