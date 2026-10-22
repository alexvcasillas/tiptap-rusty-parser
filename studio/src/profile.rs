//! Opt-in timing logs: `TIPTAP_STUDIO_PROFILE=1` prints durations of the hot
//! paths (load/parse, tree flatten, preview build, find/diff recompute,
//! edit+invert) and of interaction → next frame to stderr. Off by default; when
//! off every call is one cached bool check.

use std::cell::Cell;
use std::sync::OnceLock;
use std::time::Instant;

use gpui_kit::Window;

static START: OnceLock<Instant> = OnceLock::new();

thread_local! {
    /// Pending interaction (label, start) waiting for its frame.
    static MARK: Cell<Option<(&'static str, Instant)>> = const { Cell::new(None) };
    static FIRST_FRAME: Cell<bool> = const { Cell::new(true) };
}

pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("TIPTAP_STUDIO_PROFILE").is_ok_and(|v| !v.is_empty() && v != "0")
    })
}

/// Process start reference for "first frame"; call first thing in `main`.
pub fn start() {
    START.get_or_init(Instant::now);
}

fn log(label: &str, since: Instant) {
    let ms = since.elapsed().as_secs_f64() * 1e3;
    eprintln!("[profile] {label}: {ms:.2} ms");
}

/// Logs its lifetime on drop.
pub struct Span(Option<(&'static str, Instant)>);

impl Drop for Span {
    fn drop(&mut self) {
        if let Some((label, t)) = self.0 {
            log(label, t);
        }
    }
}

pub fn span(label: &'static str) -> Span {
    Span(enabled().then(|| (label, Instant::now())))
}

pub fn time<T>(label: &'static str, f: impl FnOnce() -> T) -> T {
    let _s = span(label);
    f()
}

/// Start of a user interaction (selection step, edit); [`frame`] reports
/// the time until the frame that shows it. The first mark of a burst wins.
pub fn mark(label: &'static str) {
    if enabled() {
        MARK.with(|m| {
            if m.get().is_none() {
                m.set(Some((label, Instant::now())));
            }
        });
    }
}

thread_local! {
    /// Per-frame accumulator: (label, total, count, flush scheduled).
    static ACC: Cell<(&'static str, f64, usize, bool)> = const { Cell::new(("", 0.0, 0, false)) };
}

/// Add the time since `t0` to this frame's `label` total (one label per app:
/// the preview's item builds); logged once after the frame.
pub fn accumulate(window: &Window, label: &'static str, t0: Option<Instant>) {
    let Some(t0) = t0 else { return };
    let ms = t0.elapsed().as_secs_f64() * 1e3;
    let (_, total, n, scheduled) = ACC.with(|a| a.get());
    ACC.with(|a| a.set((label, total + ms, n + 1, true)));
    if !scheduled {
        window.on_next_frame(|_, _| {
            let (label, total, n, _) = ACC.with(|a| a.replace(("", 0.0, 0, false)));
            eprintln!("[profile] {label}: {total:.2} ms ({n} built)");
        });
    }
}

/// Start time for [`accumulate`], `None` when profiling is off.
pub fn now() -> Option<Instant> {
    enabled().then(Instant::now)
}

/// Call from the root view's `render`: logs the first frame since process
/// start, and interaction → frame for a pending [`mark`].
pub fn frame(window: &Window) {
    if !enabled() {
        return;
    }
    if FIRST_FRAME.with(|f| f.replace(false)) {
        if let Some(&t) = START.get() {
            window.on_next_frame(move |_, _| log("first frame (since start)", t));
        }
    }
    if let Some((label, t)) = MARK.with(|m| m.take()) {
        window.on_next_frame(move |_, _| {
            let ms = t.elapsed().as_secs_f64() * 1e3;
            eprintln!("[profile] {label} → frame: {ms:.2} ms");
        });
    }
}
