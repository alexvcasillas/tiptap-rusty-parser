//! Rendering of [`CloudPanel`]: profile picker + form with token warnings,
//! document list, versions and the open actions.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::rc::Rc;

use super::secret::{self, Expiry, TokenInfo};
use super::{api, CloudPanel, Form, Target, TokenState};
use crate::theme::Tokens;

// ---- rendering -------------------------------------------------------------------

fn section(title: &'static str) -> Div {
    div()
        .pt(px(14.))
        .pb(px(6.))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(Tokens::muted())
        .child(title.to_uppercase())
}

fn note(color: Hsla, text: impl Into<SharedString>) -> Div {
    div()
        .w_full()
        .text_size(px(12.))
        .text_color(color)
        .child(text.into())
}

fn field(label: &'static str, input: &Entity<InputState>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .pb(px(8.))
        .child(
            div()
                .text_size(px(11.))
                .text_color(Tokens::secondary())
                .child(label),
        )
        .child(Input::new(input).small())
}

impl CloudPanel {
    fn render_token_info(&self, form: &Form, cx: &mut Context<Self>) -> Div {
        let now = secret::now_secs();
        let mut out = div().flex().flex_col().gap(px(4.)).pb(px(8.));
        match &form.info {
            TokenInfo::Empty => {}
            TokenInfo::Jwt(j) => {
                let aud = if j.aud.is_empty() {
                    "—".to_string()
                } else {
                    j.aud.join(", ")
                };
                let perms = if j.permissions.is_empty() {
                    "none".to_string()
                } else {
                    j.permissions.join(", ")
                };
                let (exp_text, exp_color) = match j.expiry(now) {
                    Expiry::Expired => ("expired".to_string(), Tokens::removed()),
                    Expiry::Missing => ("no exp claim".to_string(), Tokens::unknown()),
                    Expiry::In(s) if s > secret::LONG_LIVED_SECS => (
                        format!("expires in {}", secret::fmt_duration(s)),
                        Tokens::unknown(),
                    ),
                    Expiry::In(s) => (
                        format!("expires in {}", secret::fmt_duration(s)),
                        Tokens::added(),
                    ),
                };
                let mono = cx.theme().mono_font_family.clone();
                out = out.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(10.))
                        .font_family(mono)
                        .text_size(px(11.))
                        .text_color(Tokens::secondary())
                        .child("JWT (decoded, not verified)")
                        .child(format!("aud {aud}"))
                        .child(format!("permissions {perms}"))
                        .child(div().text_color(exp_color).child(exp_text)),
                );
                match j.expiry(now) {
                    Expiry::Missing => {
                        out = out.child(note(
                            Tokens::unknown(),
                            "No expiry: a leaked token works forever. Prefer a short-lived JWT.",
                        ))
                    }
                    Expiry::In(s) if s > secret::LONG_LIVED_SECS => {
                        out = out.child(note(
                            Tokens::unknown(),
                            "Expires in more than 24 h. Prefer a short-lived JWT.",
                        ))
                    }
                    _ => {}
                }
                if !j.has_api_permission() {
                    out = out.child(note(
                        Tokens::unknown(),
                        "permissions lack Documents:Api:All; the server will likely reject it.",
                    ));
                }
                if !j.aud.iter().any(|a| a == "Documents") {
                    out = out.child(note(Tokens::unknown(), "aud is not \"Documents\"."));
                }
            }
            TokenInfo::Opaque => {
                out = out
                    .child(note(
                        Tokens::removed(),
                        "Long-lived admin secret. Prefer a short-lived JWT signed on your server.",
                    ))
                    .child(
                        Checkbox::new("save-anyway")
                            .small()
                            .label("Save to keychain anyway")
                            .checked(form.save_anyway)
                            .on_click(cx.listener(|this, on: &bool, _, cx| {
                                if let Some(f) = this.form.as_mut() {
                                    f.save_anyway = *on;
                                }
                                cx.notify();
                            })),
                    )
                    .when(!form.save_anyway, |d| {
                        d.child(note(
                            Tokens::muted(),
                            "Unticked: the secret is used for this session only, never stored.",
                        ))
                    });
            }
        }
        out
    }

    fn render_form(&self, form: &Form, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .p(px(12.))
            .mt(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(Tokens::border())
            .bg(Tokens::panel())
            .child(div().pb(px(8.)).font_weight(FontWeight::SEMIBOLD).child(
                if form.editing.is_some() {
                    "Edit server profile"
                } else {
                    "Add server profile"
                },
            ))
            .child(field("Label", &self.in_label))
            .child(field("App id", &self.in_app))
            .child(field("Base URL override (testing)", &self.in_base))
            .child(field(
                "Token (stored only in the OS keychain)",
                &self.in_token,
            ))
            .child(self.render_token_info(form, cx))
            .when_some(form.error.clone(), |d, e| {
                d.child(note(Tokens::removed(), e))
            })
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .pt(px(4.))
                    .child(
                        Button::new("form-save")
                            .primary()
                            .small()
                            .label("Save")
                            .on_click(cx.listener(|this, _, w, cx| this.save_form(w, cx))),
                    )
                    .when(!self.profiles.is_empty(), |d| {
                        d.child(
                            Button::new("form-cancel")
                                .ghost()
                                .small()
                                .label("Cancel")
                                .on_click(cx.listener(|this, _, w, cx| {
                                    this.form = None;
                                    this.in_token.update(cx, |s, cx| s.set_value("", w, cx));
                                    cx.notify();
                                })),
                        )
                    }),
            )
    }

    fn render_profiles(&self, cx: &mut Context<Self>) -> Div {
        let mut chips = div().flex().flex_wrap().gap(px(4.));
        for (i, p) in self.profiles.iter().enumerate() {
            let active = self.current == Some(i);
            let b = Button::new(ElementId::NamedInteger("profile".into(), i as u64))
                .small()
                .label(p.display().to_string())
                .on_click(cx.listener(move |this, _, w, cx| {
                    if this.current != Some(i) {
                        this.current = Some(i);
                        this.form = None;
                        this.fetch_docs(false, w, cx);
                    }
                }));
            chips = chips.child(if active { b.primary() } else { b.outline() });
        }
        chips = chips.child(
            Button::new("profile-add")
                .ghost()
                .small()
                .icon(Icon::new(IconName::Plus).small())
                .label("Add")
                .on_click(cx.listener(|this, _, w, cx| this.start_form(None, w, cx))),
        );
        let mut out = div().flex().flex_col().gap(px(6.)).child(chips);
        if let Some(p) = self.profile() {
            let mono = cx.theme().mono_font_family.clone();
            let base = api::base_url(&p.app_id, &p.base_url).unwrap_or_else(|e| e);
            let (txt, color): (SharedString, Hsla) = match self.token_state(&p.id) {
                TokenState::Unknown => (
                    "Token: OS keychain (read on connect)".into(),
                    Tokens::muted(),
                ),
                TokenState::Keychain => ("Token: in OS keychain".into(), Tokens::added()),
                TokenState::SessionOnly(r) => (r, Tokens::unknown()),
                TokenState::Missing => ("No token stored".into(), Tokens::removed()),
            };
            let idx = self.current.unwrap_or(0);
            out = out
                .child(
                    div()
                        .font_family(mono)
                        .text_size(px(11.))
                        .text_color(Tokens::secondary())
                        .child(base),
                )
                .child(note(color, txt))
                .child(
                    div()
                        .flex()
                        .gap(px(4.))
                        .child(
                            Button::new("connect")
                                .outline()
                                .small()
                                .icon(Icon::new(IconName::RefreshCw).small())
                                .label("Connect")
                                .disabled(self.busy.is_some())
                                .on_click(
                                    cx.listener(|this, _, w, cx| this.fetch_docs(false, w, cx)),
                                ),
                        )
                        .child(
                            Button::new("edit")
                                .ghost()
                                .small()
                                .icon(Icon::new(IconName::Pencil).small())
                                .label("Edit")
                                .on_click(cx.listener(move |this, _, w, cx| {
                                    this.start_form(Some(idx), w, cx)
                                })),
                        )
                        .child(
                            Button::new("forget")
                                .ghost()
                                .small()
                                .icon(Icon::new(IconName::KeyRound).small())
                                .label("Forget token")
                                .on_click(cx.listener(|this, _, _, cx| this.forget_token(cx))),
                        )
                        .child(
                            Button::new("remove")
                                .ghost()
                                .small()
                                .icon(Icon::new(IconName::Trash).small())
                                .label("Remove")
                                .on_click(cx.listener(|this, _, _, cx| this.remove_profile(cx))),
                        ),
                );
        }
        out
    }

    fn render_docs(&self, cx: &mut Context<Self>) -> Div {
        let mono = cx.theme().mono_font_family.clone();
        let rows = self.filtered.clone();
        let docs: Rc<Vec<(SharedString, SharedString)>> = Rc::new(
            rows.iter()
                .map(|&i| {
                    let d = &self.docs[i];
                    (
                        SharedString::from(d.name.clone()),
                        SharedString::from(d.updated.clone().unwrap_or_default()),
                    )
                })
                .collect(),
        );
        let selected = self.selected_doc.clone();
        let entity = cx.entity().downgrade();
        let count = docs.len();
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(Input::new(&self.in_filter).small())
            .child(
                div()
                    .h(px(200.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(Tokens::border())
                    .bg(Tokens::panel())
                    .when(count == 0, |d| {
                        d.child(
                            div()
                                .p(px(10.))
                                .text_size(px(12.))
                                .text_color(Tokens::muted())
                                .child(if self.busy == Some("docs") {
                                    "Loading…"
                                } else if self.docs.is_empty() {
                                    "No documents loaded. Connect to list them."
                                } else {
                                    "No document matches the filter."
                                }),
                        )
                    })
                    .when(count > 0, |d| {
                        d.child(
                            uniform_list("cloud-docs", count, move |range, _, _| {
                                range
                                    .map(|i| {
                                        let (name, updated) = docs[i].clone();
                                        let active = selected.as_deref() == Some(name.as_ref());
                                        let entity = entity.clone();
                                        let n = name.to_string();
                                        div()
                                            .id(ElementId::NamedInteger("doc".into(), i as u64))
                                            .w_full()
                                            .h(px(26.))
                                            .px(px(10.))
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .cursor_pointer()
                                            .when(active, |d| d.bg(Tokens::accent_fill()))
                                            .hover(|d| d.bg(Tokens::raised()))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .font_family(mono.clone())
                                                    .text_size(px(12.))
                                                    .when(active, |d| {
                                                        d.text_color(Tokens::accent())
                                                    })
                                                    .child(name),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(11.))
                                                    .text_color(Tokens::muted())
                                                    .child(updated),
                                            )
                                            .on_click(move |_, window, cx| {
                                                entity
                                                    .update(cx, |this, cx| {
                                                        this.select_doc(n.clone(), window, cx)
                                                    })
                                                    .ok();
                                            })
                                    })
                                    .collect()
                            })
                            .size_full(),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(11.))
                    .text_color(Tokens::muted())
                    .child(format!(
                        "{} of {} loaded{}",
                        count,
                        self.docs.len(),
                        if self.more {
                            " · more on the server"
                        } else {
                            ""
                        }
                    ))
                    .child(div().flex_1())
                    .child(
                        Button::new("more")
                            .ghost()
                            .small()
                            .label("Load more")
                            .disabled(!self.more || self.busy.is_some())
                            .on_click(cx.listener(|this, _, w, cx| this.fetch_docs(true, w, cx))),
                    ),
            )
    }

    fn render_versions(&self, cx: &mut Context<Self>) -> Div {
        let Some(doc) = self.selected_doc.clone() else {
            return div();
        };
        let mono = cx.theme().mono_font_family.clone();
        let rows = self.versions.clone();
        let picked = self.picked;
        let pick = self.diff_pick.clone();
        let entity = cx.entity().downgrade();
        let count = rows.len();
        let busy = self.busy.is_some();
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .font_family(mono.clone())
                    .text_size(px(12.))
                    .text_color(Tokens::secondary())
                    .child(format!(
                        "{doc} · {} version{}",
                        count - 1,
                        if count == 2 { "" } else { "s" }
                    )),
            )
            .child(
                div()
                    .h(px(200.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(Tokens::border())
                    .bg(Tokens::panel())
                    .child(
                        uniform_list("cloud-versions", count, move |range, _, _| {
                            range
                                .map(|i| {
                                    let r = &rows[i];
                                    let badge = match &pick {
                                        Some(p) => p.iter().position(|&j| j == i).map(|k| {
                                            if k == 0 {
                                                "A"
                                            } else {
                                                "B"
                                            }
                                        }),
                                        None => None,
                                    };
                                    let active = match &pick {
                                        Some(_) => badge.is_some(),
                                        None => i == picked,
                                    };
                                    let entity = entity.clone();
                                    div()
                                        .id(ElementId::NamedInteger("ver".into(), i as u64))
                                        .w_full()
                                        .h(px(34.))
                                        .px(px(10.))
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .cursor_pointer()
                                        .when(active, |d| d.bg(Tokens::accent_fill()))
                                        .hover(|d| d.bg(Tokens::raised()))
                                        .child(
                                            div()
                                                .w(px(18.))
                                                .text_size(px(11.))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(Tokens::accent())
                                                .children(badge),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .when(active, |d| {
                                                            d.text_color(Tokens::accent())
                                                        })
                                                        .child(r.title.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .font_family(mono.clone())
                                                        .text_size(px(10.))
                                                        .text_color(Tokens::muted())
                                                        .child(r.sub.clone()),
                                                ),
                                        )
                                        .on_click(move |_, window, cx| {
                                            entity
                                                .update(cx, |this, cx| {
                                                    this.pick_version(i, window, cx)
                                                })
                                                .ok();
                                        })
                                })
                                .collect()
                        })
                        .size_full(),
                    ),
            )
            .child(match &self.diff_pick {
                Some(p) => div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(note(
                        Tokens::accent(),
                        if p.is_empty() {
                            "Pick the version for A (first), then B."
                        } else {
                            "Now pick the version for B."
                        },
                    ))
                    .child(div().flex_1())
                    .child(
                        Button::new("diff-cancel")
                            .ghost()
                            .small()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.diff_pick = None;
                                cx.notify();
                            })),
                    ),
                None => div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        Button::new("open-a")
                            .primary()
                            .small()
                            .label("Open as A")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, w, cx| this.open_as(Target::A, w, cx))),
                    )
                    .child(
                        Button::new("open-b")
                            .outline()
                            .small()
                            .label("Open as B")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, w, cx| this.open_as(Target::B, w, cx))),
                    )
                    .child(
                        Button::new("diff-versions")
                            .outline()
                            .small()
                            .icon(Icon::new(IconName::GitCompare).small())
                            .label("Diff versions…")
                            .disabled(busy || count < 2)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.diff_pick = Some(Vec::new());
                                cx.notify();
                            })),
                    ),
            })
    }
}

impl Render for CloudPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let form = self.form.take();
        let el = div()
            .flex()
            .flex_col()
            .pb(px(16.))
            .text_size(px(13.))
            .child(note(
                Tokens::muted(),
                "Read-only: studio only issues GET requests. Tokens stay in your OS keychain.",
            ))
            .child(section("Server"))
            .child(self.render_profiles(cx))
            .when_some(form.as_ref(), |d, f| d.child(self.render_form(f, cx)))
            .when_some(self.error.clone(), |d, e| {
                d.child(
                    div()
                        .mt(px(10.))
                        .p(px(10.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(Tokens::removed())
                        .child(note(Tokens::removed(), e)),
                )
            })
            .when(self.busy.is_some(), |d| {
                d.child(note(
                    Tokens::muted(),
                    format!(
                        "{}…",
                        match self.busy {
                            Some("docs") => "Loading documents",
                            Some("versions") => "Loading versions",
                            _ => "Fetching document",
                        }
                    ),
                ))
            })
            .when(self.profile().is_some(), |d| {
                d.child(section("Documents"))
                    .child(self.render_docs(cx))
                    .when(self.selected_doc.is_some(), |d| {
                        d.child(section("Versions")).child(self.render_versions(cx))
                    })
            });
        self.form = form;
        el
    }
}
