//! tiptap-studio: GPUI desktop inspector, structural editor and differ for
//! Tiptap/ProseMirror JSON. UI only; every document operation is delegated to
//! `tiptap-rusty-parser`.

// Release builds on Windows are GUI apps: no console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod clipboard;
mod cloud;
mod diff;
mod files;
mod find;
mod inspector;
mod layout;
mod nav;
mod preview;
mod schema_panel;
mod state;
mod theme;
mod tree;
mod workspace;

use std::path::PathBuf;

use gpui_kit::assets::AllAssets;
use gpui_kit::*;

use crate::workspace::Workspace;

fn main() {
    // `--schema path.json` (or `--schema=path.json`) anywhere; positional A, B.
    let mut schema_file: Option<PathBuf> = None;
    let mut positional = Vec::new();
    let mut raw = std::env::args().skip(1);
    while let Some(a) = raw.next() {
        if a == "--schema" {
            schema_file = raw.next().map(PathBuf::from);
        } else if let Some(p) = a.strip_prefix("--schema=") {
            schema_file = Some(PathBuf::from(p));
        } else {
            positional.push(a);
        }
    }
    let mut args = positional.into_iter().map(PathBuf::from);
    let file = args.next();
    let file_b = args.next();
    // Finder / `open -a` / Dock drops (macOS only; see `files::OsOpen`).
    let os_open = files::OsOpen::default();
    os_open
        .hook(gpui_kit::application())
        .with_assets(AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::apply(cx);
            workspace::bind_keys(cx);
            nav::bind_keys(cx);
            diff::bind_keys(cx);
            files::bind_keys(cx);
            layout::bind_keys(cx);
            layout::init(cx);
            schema_panel::bind_keys(cx);

            #[cfg(debug_assertions)]
            os_open.simulate(cx);
            // A CLI arg wins; else a file the OS sent at launch replaces the sample.
            let file = file.or_else(|| os_open.take_launch());
            // A bad A never quits: the window opens in an error state instead.
            let (state, start_error) = files::initial_state(file.as_deref());
            let state = cx.new(|_| state);

            let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
            let options = WindowOptions {
                window_bounds: Some(
                    layout::saved_window(cx).unwrap_or(WindowBounds::Windowed(bounds)),
                ),
                titlebar: Some(TitlebarOptions {
                    title: Some("tiptap-studio".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let (window, ws) = gpui_kit::open_window(options, cx, |window, cx| {
                let ws = cx.new(|cx| Workspace::new(state, file_b, cx));
                files::install(&ws, start_error, window, cx);
                layout::track_window(window, cx);
                if let Some(p) = schema_file.as_deref() {
                    let schema = ws.read(cx).schema();
                    schema.update(cx, |s, cx| {
                        if p.as_os_str() == "starter-kit" {
                            s.use_starter_kit(cx) // bundled StarterKit schema
                        } else {
                            s.load_file(p, cx)
                        }
                    });
                }
                let focus = ws.read(cx).tree_focus(cx);
                window.focus(&focus, cx);
                ws
            })
            .expect("failed to open window");
            os_open.attach(window, &ws, cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.activate(true);
        });
}
