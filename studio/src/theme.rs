//! Visual tokens from the v0 design canvas, applied over the gpui-component dark theme.

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

#[inline]
fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

pub struct Tokens;

impl Tokens {
    pub fn background() -> Hsla {
        c(0x1B1C1F)
    }
    pub fn panel() -> Hsla {
        c(0x1F2024)
    }
    pub fn title_bar() -> Hsla {
        c(0x161719)
    }
    pub fn raised() -> Hsla {
        c(0x2A2B30)
    }
    pub fn button() -> Hsla {
        c(0x26272B)
    }
    pub fn border() -> Hsla {
        c(0x2C2E33)
    }
    pub fn rule() -> Hsla {
        c(0x33353B)
    }
    pub fn input_border() -> Hsla {
        c(0x3A3D44)
    }
    pub fn code_bg() -> Hsla {
        c(0x141518)
    }
    pub fn foreground() -> Hsla {
        c(0xE8E9EC)
    }
    pub fn secondary() -> Hsla {
        c(0xC9CCD1)
    }
    pub fn muted() -> Hsla {
        c(0x9DA1A8)
    }
    pub fn disabled() -> Hsla {
        c(0x6B6F77)
    }
    pub fn ghost() -> Hsla {
        c(0x4A4D55)
    }
    pub fn accent() -> Hsla {
        c(0x5B9CF6)
    }
    pub fn accent_fill() -> Hsla {
        let mut h = Self::accent();
        h.a = 0.18;
        h
    }
    pub fn unknown() -> Hsla {
        c(0xE6A23C)
    }
    pub fn unknown_fill() -> Hsla {
        let mut h = Self::unknown();
        h.a = 0.06;
        h
    }
    pub fn removed() -> Hsla {
        c(0xF26D6D)
    }
    pub fn moved() -> Hsla {
        c(0x7FB2FF)
    }
}

impl Tokens {
    pub fn added() -> Hsla {
        c(0x4CC38A)
    }
}

/// Switch gpui-component to dark and overlay the canvas palette.
pub fn apply(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    let t = Theme::global_mut(cx);
    t.colors.background = Tokens::background();
    t.colors.foreground = Tokens::foreground();
    t.colors.border = Tokens::border();
    t.colors.sidebar = Tokens::panel();
    t.colors.sidebar_border = Tokens::border();
    t.colors.sidebar_foreground = Tokens::foreground();
    t.colors.title_bar = Tokens::title_bar();
    t.colors.title_bar_border = Tokens::border();
    t.colors.muted = Tokens::raised();
    t.colors.muted_foreground = Tokens::muted();
    t.colors.accent = Tokens::raised();
    t.colors.accent_foreground = Tokens::foreground();
    t.colors.primary = Tokens::accent();
    t.colors.primary_foreground = c(0xFFFFFF);
    t.colors.ring = Tokens::accent();
    t.colors.input = Tokens::input_border();
    t.colors.secondary = Tokens::button();
    t.colors.secondary_foreground = Tokens::secondary();
    t.colors.danger = Tokens::removed();
    t.colors.link = Tokens::moved();
    t.colors.list_active = Tokens::accent_fill();
    t.colors.list_active_border = Tokens::accent();
    t.colors.list_hover = Tokens::raised();
    t.colors.popover = Tokens::panel();
    t.colors.popover_foreground = Tokens::foreground();
}
