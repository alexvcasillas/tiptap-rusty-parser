//! Files and safety: open A (dialog, `cmd-o`, drag and drop, recent files),
//! the unsaved-changes guard (open, close, quit), parse errors as UI state
//! instead of an exit, the window title, and reload on external change.
//!
//! UI only: parsing is `Document::from_json_str`; this module only reads
//! files, words errors and decides when a document may be replaced.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants, DropdownButton};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::{ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use tiptap_rusty_parser::{Document, ParseError};

use crate::state::DocumentState;
use crate::theme::Tokens;
use crate::workspace::{Mode, Workspace};

gpui_kit::actions!(studio_files, [OpenFile, Quit]);

const RECENT_MAX: usize = 10;
const POLL: Duration = Duration::from_secs(2);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-o", OpenFile, Some("Studio")),
        KeyBinding::new("ctrl-o", OpenFile, Some("Studio")),
        KeyBinding::new("cmd-q", Quit, Some("Studio")),
        KeyBinding::new("ctrl-q", Quit, Some("Studio")),
    ]);
}

// ---- loading -------------------------------------------------------------------

/// A file that could not be opened, worded for the UI.
#[derive(Clone, Debug)]
pub struct LoadError {
    pub path: PathBuf,
    pub message: SharedString,
    /// 1-based (line, column) when serde reports one.
    pub location: Option<(usize, usize)>,
}

impl LoadError {
    pub fn name(&self) -> SharedString {
        display_name(&self.path)
    }

    /// One line: `name: line 3, column 14: message`.
    pub fn summary(&self) -> SharedString {
        match self.location {
            Some((l, c)) => format!("{}: line {l}, column {c}: {}", self.name(), self.message),
            None => format!("{}: {}", self.name(), self.message),
        }
        .into()
    }
}

fn display_name(p: &Path) -> SharedString {
    p.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| p.display().to_string())
        .into()
}

/// Read and parse `path`. Empty files and non-object JSON get their own
/// message; syntax errors keep serde's line/column.
pub fn read_doc(path: &Path) -> Result<Document, LoadError> {
    let err = |message: String, location| LoadError {
        path: path.to_path_buf(),
        message: message.into(),
        location,
    };
    let text = std::fs::read_to_string(path).map_err(|e| err(e.to_string(), None))?;
    let body = text.trim_start_matches('\u{feff}');
    let Some(first) = body.bytes().find(|b| !b.is_ascii_whitespace()) else {
        return Err(err("the file is empty".into(), None));
    };
    let found = match first {
        b'{' => None,
        b'[' => Some("an array"),
        b'"' => Some("a string"),
        b't' | b'f' => Some("a boolean"),
        b'n' => Some("null"),
        b'-' | b'0'..=b'9' => Some("a number"),
        _ => None, // not JSON at all: let serde say where
    };
    if let Some(found) = found {
        return Err(err(
            format!("expected a JSON object at the top level, found {found}"),
            None,
        ));
    }
    Document::from_json_str(body).map_err(|e| match e {
        ParseError::Json(j) => {
            let full = j.to_string();
            // serde appends " at line L column C"; we show it separately.
            let msg = full
                .rfind(" at line ")
                .map_or(full.as_str(), |i| &full[..i])
                .to_string();
            let loc = (j.line() > 0).then(|| (j.line(), j.column()));
            err(msg, loc)
        }
        other => err(other.to_string(), None),
    })
}

/// Launch state: the sample without a path, the file when it parses, else
/// an empty document plus the error the panes show. The failed path is not
/// kept as `file`, so Save can never overwrite the broken file with nothing.
pub fn initial_state(path: Option<&Path>) -> (DocumentState, Option<LoadError>) {
    let Some(path) = path else {
        match DocumentState::load(None) {
            Ok(s) => return (s, None),
            Err(e) => {
                let err = LoadError {
                    path: "assets/sample.json".into(),
                    message: e.to_string().into(),
                    location: None,
                };
                return (empty_state(), Some(err));
            }
        }
    };
    match read_doc(path) {
        Ok(doc) => (DocumentState::from_doc(doc, Some(path.to_path_buf())), None),
        Err(e) => {
            eprintln!("tiptap-studio: {}", e.summary());
            (empty_state(), Some(e))
        }
    }
}

fn empty_state() -> DocumentState {
    let doc =
        Document::from_json_str(r#"{"type":"doc","content":[]}"#).expect("static empty doc parses");
    DocumentState::from_doc(doc, None)
}

// ---- recent files ------------------------------------------------------------------

pub(crate) fn config_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = home.map(|h| h.join("Library/Application Support"));
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from).or(home);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home.map(|h| h.join(".config")));
    base.map(|b| b.join("tiptap-studio"))
}

fn recent_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("recent.json"))
}

/// Recent A paths, newest first; files that no longer exist are dropped.
fn read_recent() -> Vec<PathBuf> {
    let Some(text) = recent_file().and_then(|f| std::fs::read_to_string(f).ok()) else {
        return Vec::new();
    };
    let list: Vec<PathBuf> = serde_json::from_str(&text).unwrap_or_default();
    let n = list.len();
    let kept: Vec<PathBuf> = list.into_iter().filter(|p| p.is_file()).collect();
    if kept.len() != n {
        write_recent(&kept);
    }
    kept
}

fn write_recent(list: &[PathBuf]) {
    let Some(file) = recent_file() else {
        return;
    };
    let result = file
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|_| {
            let json = serde_json::to_string_pretty(list).map_err(std::io::Error::other)?;
            std::fs::write(&file, json)
        });
    if let Err(e) = result {
        eprintln!("tiptap-studio: recent files {}: {e}", file.display());
    }
}

// ---- workspace-side state -------------------------------------------------------------

pub enum Banner {
    Error(SharedString),
    /// The open file changed on disk while A has unsaved edits.
    Changed(SharedString),
}

#[derive(Default)]
pub struct Files {
    /// A failed to load at launch (or while in this state): the panes show it.
    pub start_error: Option<LoadError>,
    pub banner: Option<Banner>,
    recent: Rc<Vec<PathBuf>>,
    /// The file A came from and its mtime when we last read or wrote it.
    watched: Option<(PathBuf, Option<SystemTime>)>,
    /// An unsaved-changes prompt is up; a second one would be re-entrant.
    prompting: bool,
    title: String,
    _tasks: Vec<Task<()>>,
    _subs: Vec<Subscription>,
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

type Then = Box<dyn FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

/// Wire the workspace to its window: start error, recent list, close guard,
/// disk polling.
pub fn install(
    ws: &Entity<Workspace>,
    start_error: Option<LoadError>,
    window: &mut Window,
    cx: &mut App,
) {
    let weak = ws.downgrade();
    window.on_window_should_close(cx, move |window, cx| {
        weak.update(cx, |ws, cx| ws.request_close(window, cx))
            .unwrap_or(true)
    });
    ws.update(cx, |ws, cx| {
        ws.files.start_error = start_error;
        ws.files.recent = Rc::new(read_recent());
        if let Some(p) = ws.state.read(cx).file.clone() {
            ws.remember(&p);
            ws.files.watched = Some((p.clone(), mtime(&p)));
        }
        let poll = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(POLL).await;
            if this.update(cx, |ws, cx| ws.poll_disk(cx)).is_err() {
                break;
            }
        });
        ws.files._tasks.push(poll);
        #[cfg(debug_assertions)]
        if let Ok(spec) = std::env::var("TIPTAP_STUDIO_SIM_DROP") {
            ws.files._tasks.push(simulate_drops(spec, window, cx));
        }
    });
}

impl Workspace {
    // ---- guard ------------------------------------------------------------------------

    /// Run `then` once the current A may be discarded: right away when clean,
    /// else after Save (saved) or Don't Save; Cancel drops it.
    fn guard(&mut self, window: &mut Window, cx: &mut Context<Self>, then: Then) {
        if !self.state.read(cx).dirty {
            return then(self, window, cx);
        }
        if self.files.prompting {
            return;
        }
        self.files.prompting = true;
        let name = self.state.read(cx).file_name();
        let rx = window.prompt(
            PromptLevel::Warning,
            &format!("Save changes to {name}?"),
            Some("Unsaved edits are lost otherwise."),
            &[
                PromptButton::ok("Save"),
                PromptButton::new("Don't Save"),
                PromptButton::cancel("Cancel"),
            ],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = rx.await.ok();
            let file = this
                .update(cx, |ws, cx| {
                    ws.files.prompting = false;
                    ws.state.read(cx).file.clone()
                })
                .ok()
                .flatten();
            let target = match answer {
                Some(0) => match file {
                    Some(p) => Some(p),
                    None => {
                        let dir = std::env::current_dir().unwrap_or_else(|_| ".".into());
                        let Ok(rx) = this.update(cx, |_, cx| {
                            cx.prompt_for_new_path(&dir, Some("document.json"))
                        }) else {
                            return;
                        };
                        match rx.await {
                            Ok(Ok(Some(p))) => Some(p),
                            Ok(Err(e)) => {
                                let msg = format!("Save dialog unavailable: {e}");
                                this.update(cx, |ws, cx| ws.show_error(msg, cx)).ok();
                                return;
                            }
                            _ => return, // cancelled
                        }
                    }
                },
                Some(1) => None,
                _ => return, // Cancel, or the prompt went away
            };
            this.update_in(cx, |ws, window, cx| {
                if let Some(p) = target {
                    ws.state.update(cx, |s, cx| s.save_to(&p, cx));
                    ws.after_save(cx);
                    if ws.state.read(cx).dirty {
                        return; // save failed; keep the window and the doc
                    }
                }
                then(ws, window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Window close button. Clean: close. Dirty: ask, then close ourselves.
    fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.state.read(cx).dirty {
            return true;
        }
        self.guard(window, cx, Box::new(|_, window, _| window.remove_window()));
        false
    }

    pub(crate) fn quit(&mut self, _: &Quit, window: &mut Window, cx: &mut Context<Self>) {
        self.guard(window, cx, Box::new(|_, _, cx| cx.quit()));
    }

    // ---- open -------------------------------------------------------------------------

    pub(crate) fn open_file(&mut self, _: &OpenFile, window: &mut Window, cx: &mut Context<Self>) {
        self.guard(window, cx, Box::new(|ws, w, cx| ws.prompt_open(w, cx)));
    }

    fn prompt_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let picked = match rx.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(e)) => {
                    let msg = format!("File dialog unavailable: {e}");
                    this.update(cx, |ws, cx| ws.show_error(msg, cx)).ok();
                    None
                }
            };
            if let Some(p) = picked {
                this.update(cx, |ws, cx| ws.open_a(&p, cx)).ok();
            }
        })
        .detach();
    }

    fn open_recent(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.guard(window, cx, Box::new(move |ws, _, cx| ws.open_a(&path, cx)));
    }

    /// Replace A with `path`. On failure A stays as it is and the error shows
    /// in the banner (or in the panes while there is no document yet).
    pub(crate) fn open_a(&mut self, path: &Path, cx: &mut Context<Self>) {
        match read_doc(path) {
            Ok(doc) => {
                let p = path.to_path_buf();
                self.state
                    .update(cx, |s, cx| s.replace_doc(doc, Some(p.clone()), false, cx));
                self.files.start_error = None;
                self.files.banner = None;
                self.files.watched = Some((p.clone(), mtime(&p)));
                self.remember(&p);
            }
            Err(e) => {
                eprintln!("tiptap-studio: open {}", e.summary());
                if self.files.start_error.is_some() {
                    self.files.start_error = Some(e);
                } else {
                    self.files.banner = Some(Banner::Error(e.summary()));
                }
            }
        }
        cx.notify();
    }

    fn show_error(&mut self, msg: String, cx: &mut Context<Self>) {
        eprintln!("tiptap-studio: {msg}");
        self.files.banner = Some(Banner::Error(msg.into()));
        cx.notify();
    }

    /// Dropped files: the first `.json` (else the first path) opens as A, or
    /// loads as B when `as_b`.
    pub(crate) fn drop_paths(
        &mut self,
        paths: &ExternalPaths,
        as_b: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = paths.paths();
        let Some(p) = paths
            .iter()
            .find(|p| {
                p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            })
            .or_else(|| paths.first())
            .cloned()
        else {
            return;
        };
        if as_b {
            let failed = self.diff.update(cx, |d, cx| {
                d.error = None;
                d.load_b(&p, cx);
                d.error.is_some()
            });
            // Re-read only on failure, for the same wording as A errors.
            self.files.banner = failed.then(|| {
                let msg = read_doc(&p).err().map(|e| e.summary());
                Banner::Error(
                    msg.unwrap_or_else(|| format!("could not load {}", p.display()).into()),
                )
            });
            cx.notify();
        } else {
            self.guard(window, cx, Box::new(move |ws, _, cx| ws.open_a(&p, cx)));
        }
    }

    // ---- save / recent / disk ---------------------------------------------------------

    /// After any save: the saved path joins the recent list and becomes the
    /// watched file, so our own write is not reported as an external change.
    pub(crate) fn after_save(&mut self, cx: &mut Context<Self>) {
        let s = self.state.read(cx);
        if s.dirty {
            return;
        }
        if let Some(p) = s.file.clone() {
            self.files.watched = Some((p.clone(), mtime(&p)));
            self.remember(&p);
        }
        if matches!(self.files.banner, Some(Banner::Changed(_))) {
            self.files.banner = None;
        }
        cx.notify();
    }

    fn remember(&mut self, path: &Path) {
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let mut list: Vec<PathBuf> = self
            .files
            .recent
            .iter()
            .filter(|p| **p != abs)
            .cloned()
            .collect();
        list.insert(0, abs);
        list.truncate(RECENT_MAX);
        write_recent(&list);
        self.files.recent = Rc::new(list);
    }

    fn clear_recent(&mut self, cx: &mut Context<Self>) {
        self.files.recent = Rc::new(Vec::new());
        write_recent(&[]);
        cx.notify();
    }

    /// Every `POLL`: one `stat` of the open file. Changed and clean: reload in
    /// place. Changed and dirty: ask via the banner.
    fn poll_disk(&mut self, cx: &mut Context<Self>) {
        let Some((path, seen)) = self.files.watched.as_mut() else {
            return;
        };
        let now = mtime(path);
        if now.is_none() || now == *seen {
            return; // gone (maybe mid-rename) or unchanged
        }
        *seen = now;
        let path = path.clone();
        if self.state.read(cx).dirty {
            self.files.banner = Some(Banner::Changed(display_name(&path)));
            cx.notify();
        } else {
            self.reload(&path, cx);
        }
    }

    fn reload(&mut self, path: &Path, cx: &mut Context<Self>) {
        match read_doc(path) {
            Ok(doc) => {
                let p = path.to_path_buf();
                self.state
                    .update(cx, |s, cx| s.replace_doc(doc, Some(p), true, cx));
                self.files.banner = None;
                self.files.watched = Some((path.to_path_buf(), mtime(path)));
            }
            Err(e) => self.files.banner = Some(Banner::Error(e.summary())),
        }
        cx.notify();
    }

    // ---- title ------------------------------------------------------------------------

    /// `<file name>` plus ` — edited` when dirty; touches the platform only
    /// when it changes.
    pub(crate) fn sync_title(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let s = self.state.read(cx);
        let dirty = s.dirty;
        let title = match &self.files.start_error {
            Some(e) => format!("{} — error", e.name()),
            None if dirty => format!("{} — edited", s.file_name()),
            None => s.file_name().to_string(),
        };
        if title != self.files.title {
            window.set_window_title(&title);
            window.set_window_edited(dirty);
            self.files.title = title;
        }
    }

    // ---- views ------------------------------------------------------------------------

    /// Toolbar `Open` with the recent files as its dropdown.
    pub(crate) fn open_button(&self, cx: &mut Context<Self>) -> DropdownButton {
        let weak = cx.entity().downgrade();
        let recent = self.files.recent.clone();
        DropdownButton::new("open")
            .ghost()
            .small()
            .button(
                Button::new("open-a")
                    .icon(Icon::new(IconName::FolderOpen).small())
                    .label("Open")
                    .tooltip("Open A (⌘O)")
                    .on_click(cx.listener(|this, _, w, cx| this.open_file(&OpenFile, w, cx))),
            )
            .menu_accessibility_label("Recent files")
            .dropdown_menu(move |menu, _, _| recent_menu(menu, &recent, &weak))
    }

    pub(crate) fn banner_view(&self, cx: &mut Context<Self>) -> Option<Div> {
        let banner = self.files.banner.as_ref()?;
        let (color, icon, text, actions): (Hsla, IconName, SharedString, Vec<Button>) = match banner
        {
            Banner::Error(msg) => (
                Tokens::removed(),
                IconName::CircleAlert,
                msg.clone(),
                vec![],
            ),
            Banner::Changed(name) => (
                Tokens::unknown(),
                IconName::RefreshCw,
                format!("{name} changed on disk").into(),
                vec![
                    Button::new("banner-reload")
                        .outline()
                        .xsmall()
                        .label("Reload")
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some((p, _)) = this.files.watched.clone() {
                                this.reload(&p, cx);
                            }
                        })),
                    Button::new("banner-keep")
                        .ghost()
                        .xsmall()
                        .label("Keep mine")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.files.banner = None;
                            cx.notify();
                        })),
                ],
            ),
        };
        let mut fill = color;
        fill.a = 0.10;
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(32.))
                .flex_shrink_0()
                .px(px(14.))
                .bg(fill)
                .border_b_1()
                .border_color(Tokens::border())
                .text_size(px(12.))
                .child(div().text_color(color).child(Icon::new(icon).small()))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_color(Tokens::foreground())
                        .child(text),
                )
                .children(actions)
                .child(
                    Button::new("banner-dismiss")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::X).xsmall())
                        .tooltip("Dismiss")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.files.banner = None;
                            cx.notify();
                        })),
                ),
        )
    }

    /// Centred parse error with `Open…` and the recent files; replaces the panes.
    pub(crate) fn error_view(&self, cx: &mut Context<Self>) -> Option<Div> {
        let e = self.files.start_error.as_ref()?;
        let mono = cx.theme().mono_font_family.clone();
        let loc = e
            .location
            .map(|(l, c)| format!("line {l}, column {c}"))
            .unwrap_or_default();
        let recent = self.files.recent.clone();
        Some(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .bg(Tokens::background())
                .child(
                    div()
                        .text_color(Tokens::removed())
                        .child(Icon::new(IconName::FileExclamationPoint).large()),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("Could not open {}", e.name())),
                )
                .child(
                    div()
                        .max_w(px(640.))
                        .px(px(14.))
                        .py(px(10.))
                        .rounded(px(6.))
                        .bg(Tokens::panel())
                        .border_1()
                        .border_color(Tokens::border())
                        .font_family(mono.clone())
                        .text_size(px(12.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .when(!loc.is_empty(), |d| {
                            d.child(div().text_color(Tokens::unknown()).child(loc))
                        })
                        .child(
                            div()
                                .text_color(Tokens::secondary())
                                .child(e.message.clone()),
                        )
                        .child(
                            div()
                                .text_color(Tokens::disabled())
                                .child(e.path.display().to_string()),
                        ),
                )
                .child(
                    Button::new("error-open")
                        .outline()
                        .small()
                        .icon(Icon::new(IconName::FolderOpen).small())
                        .label("Open…")
                        .on_click(cx.listener(|this, _, w, cx| this.prompt_open(w, cx))),
                )
                .when(!recent.is_empty(), |d| {
                    d.child(
                        div()
                            .mt(px(8.))
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(Tokens::muted())
                            .child("RECENT"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .children(recent.iter().enumerate().map(|(i, p)| {
                                let path = p.clone();
                                div()
                                    .id(ElementId::NamedInteger("recent-row".into(), i as u64))
                                    .flex()
                                    .gap(px(8.))
                                    .w(px(420.))
                                    .px(px(10.))
                                    .py(px(4.))
                                    .rounded(px(5.))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(Tokens::raised()))
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.open_a(&path, cx)),
                                    )
                                    .child(
                                        div()
                                            .text_color(Tokens::foreground())
                                            .child(display_name(p)),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .font_family(mono.clone())
                                            .text_size(px(11.))
                                            .text_color(Tokens::disabled())
                                            .child(parent_display(p)),
                                    )
                            })),
                    )
                }),
        )
    }

    /// While files are dragged over the window: drop zones over the panes.
    /// Inspect mode is one A zone; Diff mode targets B on the changes pane and
    /// the B side, A on the A tree.
    pub(crate) fn drop_overlay(&self, cx: &mut Context<Self>) -> Option<Div> {
        if !cx.has_active_drag() {
            return None;
        }
        let has_b = self.diff.read(cx).has_b();
        let zone = |id: &'static str, as_b: bool, label: &'static str| {
            div()
                .id(id)
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .border_2()
                .border_color(gpui_kit::transparent_black())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(14.))
                        .py(px(8.))
                        .rounded(px(8.))
                        .bg(Tokens::raised())
                        .border_1()
                        .border_color(Tokens::border())
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(Tokens::secondary())
                        .child(Icon::new(IconName::FileDown).small())
                        .child(label),
                )
                .drag_over::<ExternalPaths>(|s, _, _, _| {
                    s.bg(Tokens::accent_fill())
                        .border_color(Tokens::accent())
                        .text_color(Tokens::foreground())
                })
                .on_drop(cx.listener(move |this, paths: &ExternalPaths, w, cx| {
                    this.drop_paths(paths, as_b, w, cx)
                }))
        };
        let zones = div().absolute().inset_0().flex().bg(dim());
        Some(match self.mode {
            Mode::Diff if has_b => zones
                .child(zone("drop-changes", true, "Drop as B").w(px(300.)))
                .child(zone("drop-a", false, "Drop to open as A").flex_1())
                .child(zone("drop-b", true, "Drop as B").flex_1())
                .child(zone("drop-inspector", true, "Drop as B").w(px(300.))),
            Mode::Diff => zones.child(zone("drop-b", true, "Drop to diff as B").flex_1()),
            Mode::Inspect => zones.child(zone("drop-a", false, "Drop to open as A").flex_1()),
        })
    }
}

fn dim() -> Hsla {
    let mut c = Tokens::background();
    c.a = 0.86;
    c
}

fn parent_display(p: &Path) -> String {
    let parent = p.parent().map(Path::to_path_buf).unwrap_or_default();
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(h) if parent.starts_with(&h) => {
            format!("~/{}", parent.strip_prefix(&h).unwrap_or(&parent).display())
        }
        _ => parent.display().to_string(),
    }
}

/// `parent_display` cut to its last `max` chars, so menu rows stay narrow.
fn short_parent(p: &Path, max: usize) -> String {
    let full = parent_display(p);
    let n = full.chars().count();
    if n <= max {
        return full;
    }
    let tail: String = full.chars().skip(n - max).collect();
    format!("…{tail}")
}

fn recent_menu(
    menu: gpui_kit::component::menu::PopupMenu,
    recent: &Rc<Vec<PathBuf>>,
    weak: &WeakEntity<Workspace>,
) -> gpui_kit::component::menu::PopupMenu {
    let mut menu = menu.min_w(px(320.)).label("Recent files");
    if recent.is_empty() {
        return menu.item(PopupMenuItem::new("No recent files").disabled(true));
    }
    for p in recent.iter() {
        let (path, weak) = (p.clone(), weak.clone());
        menu = menu.item(
            PopupMenuItem::new(format!("{}  ·  {}", display_name(p), short_parent(p, 36)))
                .icon(Icon::new(IconName::FileBraces))
                .on_click(move |_, window, cx| {
                    let path = path.clone();
                    weak.update(cx, |ws, cx| ws.open_recent(path, window, cx))
                        .ok();
                }),
        );
    }
    let weak = weak.clone();
    menu.separator().item(
        PopupMenuItem::new("Clear recent files").on_click(move |_, _, cx| {
            weak.update(cx, |ws, cx| ws.clear_recent(cx)).ok();
        }),
    )
}

/// Debug-only stand-in for a real OS drag (headless X has no drag source):
/// `TIPTAP_STUDIO_SIM_DROP="x,y,path;x,y,path"` replays each as Entered, a
/// 3 s hover (drop-zone highlight), then Submit, through gpui's own
/// file-drop translation. Starts 5 s after launch.
#[cfg(debug_assertions)]
fn simulate_drops(spec: String, window: &mut Window, cx: &mut Context<Workspace>) -> Task<()> {
    let drops: Vec<(Point<Pixels>, PathBuf)> = spec
        .split(';')
        .filter_map(|d| {
            let mut it = d.splitn(3, ',');
            let x: f32 = it.next()?.trim().parse().ok()?;
            let y: f32 = it.next()?.trim().parse().ok()?;
            Some((point(px(x), px(y)), PathBuf::from(it.next()?.trim())))
        })
        .collect();
    cx.spawn_in(window, async move |_, cx| {
        let ex = cx.background_executor().clone();
        let wait = |s| ex.timer(Duration::from_secs(s));
        wait(5).await;
        for (position, path) in drops {
            let paths = ExternalPaths([path].into_iter().collect());
            cx.update(|window, cx| {
                window.dispatch_event(
                    FileDropEvent::Entered { position, paths }.to_platform_input(),
                    cx,
                );
                window.dispatch_event(FileDropEvent::Pending { position }.to_platform_input(), cx);
            })
            .ok();
            wait(3).await;
            cx.update(|window, cx| {
                window.dispatch_event(FileDropEvent::Submit { position }.to_platform_input(), cx);
            })
            .ok();
            wait(3).await;
        }
    })
}
