//! The Tiptap logo, embedded from `assets/tiptap-logo.svg` and tinted like text.

use gpui_kit::*;

const LOGO_SVG: &[u8] = include_bytes!("../assets/tiptap-logo.svg");

/// Square logo of `size` px in `color` (an svg does not inherit text colour).
pub fn logo(size: f32, color: Hsla) -> Svg {
    svg()
        .data(LOGO_SVG)
        .size(px(size))
        .flex_shrink_0()
        .text_color(color)
}
