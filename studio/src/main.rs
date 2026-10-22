//! tiptap-studio: GPUI desktop inspector, structural editor and differ for
//! Tiptap/ProseMirror JSON. UI only; every document operation is delegated to
//! `tiptap-rusty-parser`.

mod inspector;
mod nav;
mod preview;
mod state;
mod theme;
mod tree;
mod workspace;

use std::path::PathBuf;

use gpui_kit::assets::AllAssets;
use gpui_kit::*;

use crate::state::DocumentState;
use crate::workspace::Workspace;

fn main() {
    let file = std::env::args().nth(1).map(PathBuf::from);
    gpui_kit::application()
        .with_assets(AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::apply(cx);
            workspace::bind_keys(cx);
            nav::bind_keys(cx);

            let state = match DocumentState::load(file.as_deref()) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("tiptap-studio: {e}");
                    cx.quit();
                    return;
                }
            };
            let state = cx.new(|_| state);

            let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("tiptap-studio".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                let ws = cx.new(|cx| Workspace::new(state, cx));
                let focus = ws.read(cx).tree_focus(cx);
                window.focus(&focus, cx);
                ws
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
