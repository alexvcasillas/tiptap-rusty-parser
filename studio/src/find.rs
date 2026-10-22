//! Find box: live search with the parser's query language
//! (`tiptap_rusty_parser::Query`). UI only; parsing and matching live in the crate.
//!
//! `FindState` (one per `DocumentState`) holds the last good query and its
//! matches, recomputed on document revision changes, never per frame. The tree
//! and preview observe it and decorate matching rows/blocks with a set lookup.
//! `FindBar` is the toolbar widget: input, error popover, counter, prev/next,
//! "Only matches".
//!
//! Both are created lazily through a per-app registry keyed by the document
//! entity, so panels and the toolbar can reach them without new plumbing.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use tiptap_rusty_parser::Query;

use crate::state::DocumentState;
use crate::theme::Tokens;

gpui_kit::actions!(find, [FocusFind, FindNext, FindPrev, FindClear]);

const CONTEXT: &str = "FindBar";
const DEBOUNCE: Duration = Duration::from_millis(120);
const PLACEHOLDER: &str = "find: type == heading && attrs.level > 1";

fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-f", FocusFind, Some("Studio")),
        KeyBinding::new("ctrl-f", FocusFind, Some("Studio")),
        KeyBinding::new("cmd-g", FindNext, Some("Studio")),
        KeyBinding::new("ctrl-g", FindNext, Some("Studio")),
        KeyBinding::new("cmd-shift-g", FindPrev, Some("Studio")),
        KeyBinding::new("ctrl-shift-g", FindPrev, Some("Studio")),
        KeyBinding::new("escape", FindClear, Some(CONTEXT)),
    ]);
    // App-level handlers: they fire wherever focus is inside the Studio window,
    // without a handler on the workspace root.
    cx.on_action(|_: &FocusFind, cx| {
        if let Some(bar) = active_bar(cx) {
            let (input, window) = {
                let b = bar.read(cx);
                (b.input.clone(), b.window)
            };
            if let (Some(input), Some(window)) = (input, window) {
                // The window is leased while the action dispatches.
                cx.defer(move |cx| {
                    let _ = window.update(cx, |_, window, cx| {
                        input.update(cx, |s, cx| {
                            s.focus(window, cx);
                            s.select_all(window, cx);
                        })
                    });
                });
            }
        }
    });
    cx.on_action(|_: &FindNext, cx| step_active(1, cx));
    cx.on_action(|_: &FindPrev, cx| step_active(-1, cx));
}

#[derive(Default)]
struct Registry {
    states: HashMap<EntityId, Entity<FindState>>,
    bars: HashMap<EntityId, Entity<FindBar>>,
}

impl Global for Registry {}

fn registry(cx: &mut App) -> &mut Registry {
    if !cx.has_global::<Registry>() {
        bind_keys(cx);
        cx.set_global(Registry::default());
    }
    cx.global_mut::<Registry>()
}

/// The find state for `doc`, created on first use.
pub fn find_state(doc: &Entity<DocumentState>, cx: &mut App) -> Entity<FindState> {
    let id = doc.entity_id();
    if let Some(f) = registry(cx).states.get(&id) {
        return f.clone();
    }
    let f = cx.new(|cx| FindState::new(doc.clone(), cx));
    registry(cx).states.insert(id, f.clone());
    f
}

/// The toolbar find box for `doc`, created on first use.
pub fn bar(doc: &Entity<DocumentState>, cx: &mut App) -> Entity<FindBar> {
    let id = doc.entity_id();
    if let Some(b) = registry(cx).bars.get(&id) {
        return b.clone();
    }
    let find = find_state(doc, cx);
    let b = cx.new(|_| FindBar {
        find,
        input: None,
        window: None,
        pending: None,
        _subs: Vec::new(),
    });
    registry(cx).bars.insert(id, b.clone());
    b
}

fn active_bar(cx: &mut App) -> Option<Entity<FindBar>> {
    let active = cx.active_window();
    let bars: Vec<Entity<FindBar>> = registry(cx).bars.values().cloned().collect();
    bars.iter()
        .find(|b| active.is_some() && b.read(cx).window == active)
        .or(bars.first())
        .cloned()
}

fn step_active(delta: isize, cx: &mut App) {
    if let Some(bar) = active_bar(cx) {
        let find = bar.read(cx).find.clone();
        find.update(cx, |f, cx| f.step(delta, cx));
    }
}

// ---- state ------------------------------------------------------------------------

/// How a tree row relates to the current matches.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RowMatch {
    None,
    Hit,
    /// Collapsed row with a match somewhere below it.
    Inside,
}

pub struct FindState {
    doc: Entity<DocumentState>,
    /// Raw text of the box.
    text: SharedString,
    /// Last query that parsed; kept while the text has an error.
    query: Option<Query>,
    pub error: Option<SharedString>,
    /// Matches in document order.
    pub matches: Rc<Vec<Vec<usize>>>,
    pub set: Rc<HashSet<Vec<usize>>>,
    /// Strict prefixes of every match (for "Only matches" and collapsed pips).
    pub ancestors: Rc<HashSet<Vec<usize>>>,
    /// Index of the selected node in `matches`, if it is one.
    pub current: Option<usize>,
    pub only_matches: bool,
    /// Where `esc` sends focus (the tree registers itself).
    pub return_focus: Option<FocusHandle>,
    revision: u64,
    _subs: Vec<Subscription>,
}

impl FindState {
    fn new(doc: Entity<DocumentState>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&doc, |this, doc, cx| {
            let rev = doc.read(cx).revision;
            if rev != this.revision {
                this.revision = rev;
                this.recompute(cx);
            } else if this.sync_current(cx) {
                cx.notify();
            }
        });
        let revision = doc.read(cx).revision;
        Self {
            doc,
            text: SharedString::default(),
            query: None,
            error: None,
            matches: Rc::default(),
            set: Rc::default(),
            ancestors: Rc::default(),
            current: None,
            only_matches: false,
            return_focus: None,
            revision,
            _subs: vec![sub],
        }
    }

    /// Query active (text parsed at least once and not cleared).
    pub fn active(&self) -> bool {
        self.query.is_some()
    }

    pub fn row_match(&self, path: &[usize], expanded: bool) -> RowMatch {
        if self.set.contains(path) {
            RowMatch::Hit
        } else if !expanded && self.ancestors.contains(path) {
            RowMatch::Inside
        } else {
            RowMatch::None
        }
    }

    pub fn filtering(&self) -> bool {
        self.only_matches && self.query.is_some()
    }

    fn set_text(&mut self, text: SharedString, cx: &mut Context<Self>) {
        if text == self.text {
            return;
        }
        self.text = text;
        if self.text.trim().is_empty() {
            self.clear(cx);
            return;
        }
        match Query::parse(&self.text) {
            Ok(q) => {
                self.error = None;
                self.query = Some(q);
                self.recompute(cx);
            }
            Err(e) => {
                self.error = Some(e.to_string().into());
                cx.notify();
            }
        }
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.text = SharedString::default();
        self.query = None;
        self.error = None;
        self.matches = Rc::default();
        self.set = Rc::default();
        self.ancestors = Rc::default();
        self.current = None;
        cx.notify();
    }

    pub fn set_only_matches(&mut self, on: bool, cx: &mut Context<Self>) {
        self.only_matches = on;
        cx.notify();
    }

    fn recompute(&mut self, cx: &mut Context<Self>) {
        let Some(q) = &self.query else {
            return;
        };
        let matches = q.find_paths(self.doc.read(cx).root());
        let mut ancestors = HashSet::new();
        for m in &matches {
            // Longest prefix first: once one is present, all shorter ones are.
            for d in (0..m.len()).rev() {
                if ancestors.contains(&m[..d]) {
                    break;
                }
                ancestors.insert(m[..d].to_vec());
            }
        }
        self.set = Rc::new(matches.iter().cloned().collect());
        self.ancestors = Rc::new(ancestors);
        self.matches = Rc::new(matches);
        self.sync_current(cx);
        cx.notify();
    }

    /// Point `current` at the selection if it is a match. Returns true if it changed.
    fn sync_current(&mut self, cx: &App) -> bool {
        let sel = self.doc.read(cx).selection.as_deref();
        let cur = sel.and_then(|p| self.matches.binary_search_by(|m| m.as_slice().cmp(p)).ok());
        let changed = cur != self.current;
        self.current = cur;
        changed
    }

    /// Select the next (`delta > 0`) or previous match relative to the
    /// selection, wrapping. Paths compare lexicographically in document order.
    pub fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.matches.len();
        if n == 0 {
            return;
        }
        let sel = self.doc.read(cx).selection.clone();
        let ix = match (sel, delta > 0) {
            (None, true) => 0,
            (None, false) => n - 1,
            (Some(p), fwd) => {
                // First match > p, or last match < p.
                let at = self
                    .matches
                    .partition_point(|m| m.as_slice() <= p.as_slice());
                if fwd {
                    at % n
                } else {
                    let below = self
                        .matches
                        .partition_point(|m| m.as_slice() < p.as_slice());
                    (below + n - 1) % n
                }
            }
        };
        let path = self.matches[ix].clone();
        self.doc.update(cx, |s, cx| s.select(Some(path), cx));
    }

    pub fn counter(&self) -> String {
        match self.current {
            Some(i) => format!("{} / {}", i + 1, self.matches.len()),
            None => format!("– / {}", self.matches.len()),
        }
    }
}

// ---- toolbar widget -----------------------------------------------------------

pub struct FindBar {
    find: Entity<FindState>,
    input: Option<Entity<InputState>>,
    window: Option<AnyWindowHandle>,
    pending: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl FindBar {
    fn input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(i) = &self.input {
            return i.clone();
        }
        self.window = Some(window.window_handle());
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(PLACEHOLDER));
        let sub = cx.subscribe_in(
            &input,
            window,
            |this, input, ev: &InputEvent, _, cx| match ev {
                InputEvent::Change => {
                    let text = input.read(cx).value();
                    let find = this.find.clone();
                    // Debounce: dropping the previous task cancels it.
                    this.pending = Some(cx.spawn(async move |_, cx| {
                        cx.background_executor().timer(DEBOUNCE).await;
                        find.update(cx, |f, cx| f.set_text(text, cx));
                    }));
                }
                InputEvent::PressEnter { shift, .. } => {
                    // Flush a pending edit first so enter acts on what is typed.
                    this.pending = None;
                    let text = input.read(cx).value();
                    let delta = if *shift { -1 } else { 1 };
                    this.find.update(cx, |f, cx| {
                        f.set_text(text, cx);
                        f.step(delta, cx);
                    });
                }
                _ => {}
            },
        );
        let obs = cx.observe(&self.find, |_, _, cx| cx.notify());
        self._subs.extend([sub, obs]);
        self.input = Some(input.clone());
        input
    }

    fn clear(&mut self, _: &FindClear, window: &mut Window, cx: &mut Context<Self>) {
        self.pending = None;
        if let Some(i) = &self.input {
            i.update(cx, |s, cx| s.set_value("", window, cx));
        }
        let back = self.find.update(cx, |f, cx| {
            f.clear(cx);
            f.return_focus.clone()
        });
        if let Some(h) = back {
            window.focus(&h, cx);
        }
    }
}

impl Render for FindBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let input = self.input(window, cx);
        let mono = cx.theme().mono_font_family.clone();
        let f = self.find.read(cx);
        let error = f.error.clone();
        let has_matches = !f.matches.is_empty();
        let active = f.active();
        let counter = f.counter();
        let only = f.only_matches;
        let (prev, next, toggle) = (self.find.clone(), self.find.clone(), self.find.clone());

        div()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::clear))
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .relative()
                    .w(px(360.))
                    .font_family(mono.clone())
                    .child(Input::new(&input).small().when(error.is_some(), |i| {
                        i.focus_bordered(false).border_color(Tokens::removed())
                    }))
                    .when_some(error, |d, msg| {
                        d.child(deferred(
                            div()
                                .absolute()
                                .top(px(32.))
                                .left_0()
                                .max_w(px(420.))
                                .px(px(8.))
                                .py(px(5.))
                                .rounded(px(6.))
                                .bg(Tokens::raised())
                                .border_1()
                                .border_color(Tokens::removed())
                                .text_size(px(11.))
                                .line_height(px(15.))
                                .text_color(Tokens::removed())
                                .child(msg),
                        ))
                    }),
            )
            .child(
                div()
                    .min_w(px(52.))
                    .font_family(mono)
                    .text_size(px(11.))
                    .text_color(if active && has_matches {
                        Tokens::secondary()
                    } else {
                        Tokens::disabled()
                    })
                    .child(if active { counter } else { String::new() }),
            )
            .child(nav_button(
                "find-prev",
                IconName::ChevronUp,
                !has_matches,
                move |_, _, cx| prev.update(cx, |f, cx| f.step(-1, cx)),
            ))
            .child(nav_button(
                "find-next",
                IconName::ChevronDown,
                !has_matches,
                move |_, _, cx| next.update(cx, |f, cx| f.step(1, cx)),
            ))
            .child(
                Checkbox::new("only-matches")
                    .small()
                    .label("Only matches")
                    .checked(only)
                    .on_click(move |on, _, cx| {
                        toggle.update(cx, |f, cx| f.set_only_matches(*on, cx))
                    }),
            )
    }
}

fn nav_button(
    id: &'static str,
    icon: IconName,
    disabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    crate::tree::icon_button(id, icon, on_click).when(disabled, |d| d.opacity(0.4))
}

// ---- decoration helpers for tree and preview ------------------------------------

/// Light accent fill for matches, distinct from the selection fill.
pub fn match_fill() -> Hsla {
    let mut c = Tokens::accent();
    c.a = 0.10;
    c
}

/// Tree row decoration: 2px accent bar in the left gutter plus a light fill
/// (hit), or a faint bar (collapsed row hiding a match).
pub fn decorate_row(row: Stateful<Div>, m: RowMatch, selected: bool) -> Stateful<Div> {
    let bar = |alpha: f32| {
        let mut c = Tokens::accent();
        c.a = alpha;
        div()
            .absolute()
            .left_0()
            .top(px(4.))
            .bottom(px(4.))
            .w(px(2.))
            .rounded(px(1.))
            .bg(c)
    };
    match m {
        RowMatch::None => row,
        RowMatch::Hit => row
            .relative()
            .when(!selected, |r| r.bg(match_fill()))
            .child(bar(1.)),
        RowMatch::Inside => row.relative().child(bar(0.4)),
    }
}

/// Background for a matching text run inside a preview paragraph.
pub fn inline_match_fill() -> Hsla {
    let mut c = Tokens::accent();
    c.a = 0.30;
    c
}
