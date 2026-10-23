//! Tiptap Document Server browser (toolbar `Server…`, a right-hand sheet).
//!
//! Lists documents and versions from a Document Server and opens them as A or
//! B, or two versions straight into Diff mode. Bring-your-own token: tokens
//! live only in the OS keychain (or in memory for the session when there is
//! none), and the HTTP client is GET-only (see [`api`]). All network and
//! keychain calls run on `background_spawn`; results are applied on the
//! foreground. A and B are loaded through `DocumentState::load_document` and
//! `DiffState::load_b_document`, never through temp files.

mod api;
mod secret;
mod store;
mod view;

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::button::Button;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use tiptap_rusty_parser::Document;

use self::api::{ApiError, DocEntry, VersionEntry};
use self::secret::{Secret, TokenInfo};
use self::store::Profile;
use crate::diff::DiffState;
use crate::state::DocumentState;
use crate::workspace::Workspace;

/// Switches the workspace to Diff mode (handed in by the toolbar item).
pub type OnDiff = Rc<dyn Fn(&mut Window, &mut App)>;

struct CloudGlobal(Entity<CloudPanel>);
impl Global for CloudGlobal {}

/// Open the server sheet. The panel entity is kept for the app's lifetime, so
/// lists and session tokens survive closing the sheet.
/// `workspace` runs the unsaved-changes guard (`Workspace::guard`) before A
/// is replaced.
pub fn open(
    state: Entity<DocumentState>,
    diff: Entity<DiffState>,
    workspace: WeakEntity<Workspace>,
    on_diff: OnDiff,
    window: &mut Window,
    cx: &mut App,
) {
    let panel = match cx.try_global::<CloudGlobal>() {
        Some(g) => g.0.clone(),
        None => {
            let p = cx.new(|cx| CloudPanel::new(state, diff, workspace, on_diff, window, cx));
            cx.set_global(CloudGlobal(p.clone()));
            p
        }
    };
    #[cfg(debug_assertions)]
    simulate_open(&panel, window, cx);
    window.open_sheet(cx, move |sheet, _, _| {
        sheet
            .title("Tiptap Document Server")
            .size(px(580.))
            .overlay(false)
            .child(panel.clone())
            // The header × dispatches `Cancel`, which a focused input inside
            // the sheet swallows; this one always closes.
            .footer(
                Button::new("cloud-close")
                    .outline()
                    .small()
                    .label("Close")
                    .on_click(|_, window, cx| window.close_sheet(cx)),
            )
    });
}

/// Where the current token of a profile comes from.
#[derive(Clone, PartialEq)]
enum TokenState {
    /// Not looked up yet (we only touch the keychain when needed).
    Unknown,
    Keychain,
    /// Kept in memory only; the reason is shown.
    SessionOnly(SharedString),
    Missing,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    A,
    B,
}

/// Version row: `None` id = latest.
#[derive(Clone)]
struct VersionRow {
    id: Option<String>,
    title: SharedString,
    sub: SharedString,
}

impl VersionRow {
    fn label(&self) -> String {
        match &self.id {
            None => "latest".into(),
            Some(id) if id.bytes().all(|b| b.is_ascii_digit()) => format!("v{id}"),
            Some(id) => id.clone(),
        }
    }
}

enum Fail {
    NoToken,
    Keychain(String),
    Api(ApiError),
}

impl Fail {
    fn message(&self) -> SharedString {
        match self {
            Fail::NoToken => "No token for this profile. Edit it and paste a token.".into(),
            Fail::Keychain(e) => format!(
                "Keychain unavailable ({e}). Edit the profile and paste the token; it will be kept for this session only."
            )
            .into(),
            Fail::Api(e) => e.to_string().into(),
        }
    }
}

struct Form {
    /// `Some(id)` = editing that profile.
    editing: Option<String>,
    info: TokenInfo,
    save_anyway: bool,
    error: Option<SharedString>,
}

pub struct CloudPanel {
    state: Entity<DocumentState>,
    diff: Entity<DiffState>,
    workspace: WeakEntity<Workspace>,
    on_diff: OnDiff,
    profiles: Vec<Profile>,
    current: Option<usize>,
    /// Session tokens by profile id (read from the keychain once, or
    /// session-only when there is no keychain). Never written anywhere.
    tokens: HashMap<String, Arc<Secret>>,
    token_state: HashMap<String, TokenState>,
    form: Option<Form>,
    in_label: Entity<InputState>,
    in_app: Entity<InputState>,
    in_base: Entity<InputState>,
    in_token: Entity<InputState>,
    in_filter: Entity<InputState>,
    docs: Vec<DocEntry>,
    filtered: Rc<Vec<usize>>,
    more: bool,
    selected_doc: Option<String>,
    versions: Rc<Vec<VersionRow>>,
    picked: usize,
    /// `Some` while picking two versions for Diff (indices into `versions`).
    diff_pick: Option<Vec<usize>>,
    busy: Option<&'static str>,
    error: Option<SharedString>,
    /// Latest request id per kind; stale results are dropped.
    seq: HashMap<&'static str, u64>,
    _subs: Vec<Subscription>,
}

impl CloudPanel {
    fn new(
        state: Entity<DocumentState>,
        diff: Entity<DiffState>,
        workspace: WeakEntity<Workspace>,
        on_diff: OnDiff,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = |ph: &str, window: &mut Window, cx: &mut Context<Self>| {
            let ph = SharedString::from(ph.to_string());
            cx.new(|cx| InputState::new(window, cx).placeholder(ph))
        };
        let in_label = input("Label, e.g. Production", window, cx);
        let in_app = input("Document server ID (app id)", window, cx);
        let in_base = input("Base URL override (optional, https://)", window, cx);
        let in_token = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("Paste a JWT with Documents:Api:All")
        });
        let in_filter = input("Filter documents", window, cx);
        let subs = vec![
            cx.subscribe(&in_token, |this, s, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    if let Some(f) = this.form.as_mut() {
                        f.info = secret::analyze(&s.read(cx).value());
                        cx.notify();
                    }
                }
            }),
            cx.subscribe(&in_filter, |this, _, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    this.refilter(cx);
                }
            }),
        ];
        let profiles = store::load_profiles();
        let current = (!profiles.is_empty()).then_some(0);
        let mut this = Self {
            state,
            diff,
            workspace,
            on_diff,
            profiles,
            current,
            tokens: HashMap::new(),
            token_state: HashMap::new(),
            form: None,
            in_label,
            in_app,
            in_base,
            in_token,
            in_filter,
            docs: Vec::new(),
            filtered: Rc::new(Vec::new()),
            more: false,
            selected_doc: None,
            versions: Rc::new(Vec::new()),
            picked: 0,
            diff_pick: None,
            busy: None,
            error: None,
            seq: HashMap::new(),
            _subs: subs,
        };
        if this.profiles.is_empty() {
            this.start_form(None, window, cx);
        }
        this
    }

    fn profile(&self) -> Option<&Profile> {
        self.current.and_then(|i| self.profiles.get(i))
    }

    fn token_state(&self, id: &str) -> TokenState {
        self.token_state
            .get(id)
            .cloned()
            .unwrap_or(TokenState::Unknown)
    }

    // ---- profiles -------------------------------------------------------------

    fn start_form(&mut self, editing: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let p = editing.and_then(|i| self.profiles.get(i)).cloned();
        let set =
            |s: &Entity<InputState>, v: String, window: &mut Window, cx: &mut Context<Self>| {
                s.update(cx, |s, cx| s.set_value(v, window, cx));
            };
        set(
            &self.in_label,
            p.as_ref().map(|p| p.label.clone()).unwrap_or_default(),
            window,
            cx,
        );
        set(
            &self.in_app,
            p.as_ref().map(|p| p.app_id.clone()).unwrap_or_default(),
            window,
            cx,
        );
        set(
            &self.in_base,
            p.as_ref().map(|p| p.base_url.clone()).unwrap_or_default(),
            window,
            cx,
        );
        // Never pre-fill the token, not even masked.
        set(&self.in_token, String::new(), window, cx);
        let ph = if p.is_some() {
            "Leave empty to keep the stored token"
        } else {
            "Paste a JWT with Documents:Api:All"
        };
        self.in_token
            .update(cx, |s, cx| s.set_placeholder(ph, window, cx));
        self.form = Some(Form {
            editing: p.map(|p| p.id),
            info: TokenInfo::Empty,
            save_anyway: false,
            error: None,
        });
        cx.notify();
    }

    fn save_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let label = self.in_label.read(cx).value().trim().to_string();
        let app_id = self.in_app.read(cx).value().trim().to_string();
        let base_url = self.in_base.read(cx).value().trim().to_string();
        if let Err(e) = api::base_url(&app_id, &base_url) {
            form.error = Some(e.into());
            cx.notify();
            return;
        }
        let raw = self.in_token.read(cx).value().trim().to_string();
        let token = (!raw.is_empty()).then(|| Arc::new(Secret::new(raw)));
        let opaque = matches!(form.info, TokenInfo::Opaque);
        let to_keychain = !opaque || form.save_anyway;
        let id = form.editing.clone().unwrap_or_else(Profile::new_id);
        if token.is_none() && form.editing.is_none() {
            form.error = Some("Paste a token.".into());
            cx.notify();
            return;
        }
        let profile = Profile {
            id: id.clone(),
            label,
            app_id,
            base_url,
        };
        match self.profiles.iter().position(|p| p.id == id) {
            Some(i) => {
                self.profiles[i] = profile;
                self.current = Some(i);
            }
            None => {
                self.profiles.push(profile);
                self.current = Some(self.profiles.len() - 1);
            }
        }
        if let Err(e) = store::save_profiles(&self.profiles) {
            self.error = Some(format!("Could not save servers.json: {e}").into());
        }
        self.in_token
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.form = None;
        self.reset_lists();
        if let Some(token) = token {
            self.tokens.insert(id.clone(), token.clone());
            if to_keychain {
                self.token_state.insert(id.clone(), TokenState::Unknown);
                let task = cx.background_spawn(async move {
                    let r = store::keychain_set(&id, &token);
                    (id, r)
                });
                cx.spawn(async move |this, cx| {
                    let (id, r) = task.await;
                    this.update(cx, |this, cx| {
                        let st = match r {
                            Ok(()) => TokenState::Keychain,
                            Err(e) => {
                                eprintln!("tiptap-studio: keychain unavailable: {e}");
                                TokenState::SessionOnly("Not saved: no keychain available".into())
                            }
                        };
                        this.token_state.insert(id, st);
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            } else {
                self.token_state.insert(
                    id,
                    TokenState::SessionOnly(
                        "Not saved: legacy secret kept in memory for this session".into(),
                    ),
                );
            }
        }
        self.fetch_docs(false, window, cx);
        cx.notify();
    }

    fn forget_token(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.profile().map(|p| p.id.clone()) else {
            return;
        };
        self.tokens.remove(&id);
        self.token_state.insert(id.clone(), TokenState::Missing);
        let task = cx.background_spawn(async move { store::keychain_delete(&id) });
        cx.spawn(async move |this, cx| {
            if let Err(e) = task.await {
                this.update(cx, |this, cx| {
                    this.error = Some(
                        format!("Token cleared from memory; keychain not reachable ({e})").into(),
                    );
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        cx.notify();
    }

    fn remove_profile(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.current else {
            return;
        };
        self.forget_token(cx);
        self.profiles.remove(i);
        self.current = (!self.profiles.is_empty()).then_some(0);
        if let Err(e) = store::save_profiles(&self.profiles) {
            self.error = Some(format!("Could not save servers.json: {e}").into());
        }
        self.reset_lists();
        cx.notify();
    }

    fn reset_lists(&mut self) {
        self.docs.clear();
        self.filtered = Rc::new(Vec::new());
        self.more = false;
        self.selected_doc = None;
        self.versions = Rc::new(Vec::new());
        self.diff_pick = None;
        self.error = None;
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let q = self.in_filter.read(cx).value().to_lowercase();
        self.filtered = Rc::new(
            self.docs
                .iter()
                .enumerate()
                .filter(|(_, d)| q.is_empty() || d.name.to_lowercase().contains(&q))
                .map(|(i, _)| i)
                .collect(),
        );
        cx.notify();
    }

    // ---- requests ---------------------------------------------------------------

    /// Run `job` against the current profile on a background thread, then
    /// `done` on the foreground (dropped if a newer `kind` request started).
    fn run<T: Send + 'static>(
        &mut self,
        kind: &'static str,
        job: impl FnOnce(&api::Client) -> Result<T, ApiError> + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(p) = self.profile().cloned() else {
            return;
        };
        let base = match api::base_url(&p.app_id, &p.base_url) {
            Ok(b) => b,
            Err(e) => {
                self.error = Some(e.into());
                cx.notify();
                return;
            }
        };
        let cached = self.tokens.get(&p.id).cloned();
        let seq = self.seq.entry(kind).or_default();
        *seq += 1;
        let seq = *seq;
        self.busy = Some(kind);
        self.error = None;
        cx.notify();
        let id = p.id.clone();
        let bg_id = p.id;
        let task = cx.background_spawn(async move {
            let (token, read) = match cached {
                Some(t) => (t, false),
                None => match store::keychain_get(&bg_id) {
                    Ok(Some(t)) => (Arc::new(t), true),
                    Ok(None) => return Err(Fail::NoToken),
                    Err(e) => return Err(Fail::Keychain(e)),
                },
            };
            let client = api::Client::new(base, &token);
            job(&client)
                .map(|v| (v, read.then_some(token)))
                .map_err(Fail::Api)
        });
        cx.spawn_in(window, async move |this, cx| {
            let r = task.await;
            cx.update(|window, cx| {
                this.update(cx, |this, cx| {
                    if this.seq.get(kind) != Some(&seq) {
                        return;
                    }
                    if this.busy == Some(kind) {
                        this.busy = None;
                    }
                    match r {
                        Ok((v, read)) => {
                            if let Some(t) = read {
                                this.tokens.insert(id.clone(), t);
                                this.token_state.insert(id, TokenState::Keychain);
                            }
                            done(this, v, window, cx);
                        }
                        Err(f) => {
                            match f {
                                Fail::NoToken => {
                                    this.token_state.insert(id, TokenState::Missing);
                                }
                                Fail::Keychain(_) => {
                                    this.token_state.insert(
                                        id,
                                        TokenState::SessionOnly(
                                            "No keychain available: tokens are kept for this session only".into(),
                                        ),
                                    );
                                }
                                Fail::Api(_) => {}
                            }
                            this.error = Some(f.message());
                        }
                    }
                    cx.notify();
                })
            })
            .ok();
        })
        .detach();
    }

    fn fetch_docs(&mut self, more: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !more {
            self.reset_lists();
        }
        let skip = self.docs.len();
        self.run(
            "docs",
            move |c| c.list_documents(skip),
            |this, page, _, cx| {
                this.more = page.len() >= api::PAGE;
                this.docs.extend(page);
                this.refilter(cx);
            },
            window,
            cx,
        );
    }

    fn select_doc(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_doc = Some(name.clone());
        self.versions = Rc::new(vec![latest_row()]);
        self.picked = 0;
        self.diff_pick = None;
        self.run(
            "versions",
            move |c| c.versions(&name),
            |this, list, _, _| {
                let mut rows = vec![latest_row()];
                rows.extend(list.into_iter().map(version_row));
                this.versions = Rc::new(rows);
            },
            window,
            cx,
        );
        cx.notify();
    }

    fn origin(&self, doc: &str, v: &VersionRow) -> SharedString {
        let app = self
            .profile()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        format!("☁ {app} / {doc} @ {}", v.label()).into()
    }

    /// Run `then` now when A is clean, else through the file guard
    /// (`Workspace::guard`): Save (to A's file, or a picked path when A has
    /// none, e.g. it came from this server) / Don't Save / Cancel.
    fn guard_a(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if !self.state.read(cx).dirty {
            // Synchronous: the guard would call back into this (leased) panel.
            then(self, window, cx);
            return;
        }
        let panel = cx.entity().downgrade();
        let then: crate::files::Then = Box::new(move |_, window, cx| {
            panel.update(cx, |this, cx| then(this, window, cx)).ok();
        });
        self.workspace
            .update(cx, |ws, cx| ws.guard(window, cx, then))
            .ok();
    }

    fn open_as(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(doc), Some(v)) = (
            self.selected_doc.clone(),
            self.versions.get(self.picked).cloned(),
        ) else {
            return;
        };
        let origin = self.origin(&doc, &v);
        let go = move |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            let version = v.id.clone();
            this.run(
                "open",
                move |c| fetch_doc(c, &doc, version.as_deref()),
                move |this, d, _, cx| match target {
                    Target::A => this
                        .state
                        .update(cx, |s, cx| s.load_document(d, origin, cx)),
                    Target::B => this
                        .diff
                        .update(cx, |s, cx| s.load_b_document(d, origin, cx)),
                },
                window,
                cx,
            );
        };
        match target {
            Target::A => self.guard_a(window, cx, go),
            Target::B => go(self, window, cx),
        }
    }

    fn pick_version(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pick) = self.diff_pick.as_mut() else {
            self.picked = i;
            cx.notify();
            return;
        };
        if pick.contains(&i) {
            pick.retain(|&j| j != i);
        } else {
            pick.push(i);
        }
        if pick.len() == 2 {
            let (a, b) = (pick[0], pick[1]);
            self.open_diff(a, b, window, cx);
        }
        cx.notify();
    }

    fn open_diff(&mut self, a: usize, b: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(doc), Some(va), Some(vb)) = (
            self.selected_doc.clone(),
            self.versions.get(a).cloned(),
            self.versions.get(b).cloned(),
        ) else {
            return;
        };
        let (oa, ob) = (self.origin(&doc, &va), self.origin(&doc, &vb));
        self.guard_a(window, cx, move |this, window, cx| {
            this.run(
                "open",
                move |c| {
                    Ok((
                        fetch_doc(c, &doc, va.id.as_deref())?,
                        fetch_doc(c, &doc, vb.id.as_deref())?,
                    ))
                },
                move |this, (da, db), window, cx| {
                    this.diff_pick = None;
                    this.state.update(cx, |s, cx| s.load_document(da, oa, cx));
                    this.diff.update(cx, |s, cx| s.load_b_document(db, ob, cx));
                    (this.on_diff)(window, cx);
                    window.close_sheet(cx);
                },
                window,
                cx,
            );
        });
    }
}

/// Debug-only test hook: `TIPTAP_STUDIO_SIM_SERVER_OPEN=doc.json` makes every
/// `Server…` click open that file as A after 1s, as if fetched from a server,
/// through the same guard as a real open (headless runs have no server).
#[cfg(debug_assertions)]
fn simulate_open(panel: &Entity<CloudPanel>, window: &mut Window, cx: &mut App) {
    let Ok(path) = std::env::var("TIPTAP_STUDIO_SIM_SERVER_OPEN") else {
        return;
    };
    let panel = panel.downgrade();
    window
        .spawn(cx, async move |cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(1))
                .await;
            cx.update(|window, cx| {
                panel
                    .update(cx, |this, cx| {
                        this.guard_a(window, cx, move |this, _, cx| match crate::files::read_doc(
                            std::path::Path::new(&path),
                        ) {
                            Ok(d) => this.state.update(cx, |s, cx| {
                                s.load_document(d, "☁ sim / doc @ latest".into(), cx)
                            }),
                            Err(e) => eprintln!("tiptap-studio: sim open {}", e.summary()),
                        })
                    })
                    .ok();
            })
            .ok();
        })
        .detach();
}

fn fetch_doc(c: &api::Client, id: &str, version: Option<&str>) -> Result<Document, ApiError> {
    let v = c.document(id, version)?;
    Document::from_value(v).map_err(|e| ApiError::Parse(e.to_string()))
}

fn latest_row() -> VersionRow {
    VersionRow {
        id: None,
        title: "Latest".into(),
        sub: "current state of the document".into(),
    }
}

fn version_row(v: VersionEntry) -> VersionRow {
    let label = if v.id.bytes().all(|b| b.is_ascii_digit()) {
        format!("v{}", v.id)
    } else {
        v.id.clone()
    };
    VersionRow {
        title: match &v.name {
            Some(n) => format!("{label} · {n}").into(),
            None => label.into(),
        },
        sub: format!("{} · id {}", v.date.as_deref().unwrap_or("no date"), v.id).into(),
        id: Some(v.id),
    }
}
