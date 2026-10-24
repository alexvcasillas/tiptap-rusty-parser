//! Compiles the Rust code blocks of the docs site as doctests, so every
//! example on the site builds against the crates in this workspace.
//!
//! Each section registers its pages in its own module with [`pages!`]; the
//! `coverage` test fails when a page with Rust code is missing.

/// `pages! { Name => "dir/page.mdx", … }`: one doctest holder per page, plus
/// `PAGES` listing their paths (relative to `docs/content/docs/`).
macro_rules! pages {
    ($($name:ident => $path:literal),* $(,)?) => {
        $(
            #[cfg(doctest)]
            #[doc = include_str!(concat!("../../docs/content/docs/", $path))]
            pub struct $name;
        )*

        /// Pages of this section whose Rust fences compile.
        pub const PAGES: &[&str] = &[$($path),*];
    };
}

pub mod guides;
pub mod javascript;
pub mod markdown;
pub mod parser_core;
pub mod parser_editing;

/// Every registered page, all sections.
pub fn pages() -> Vec<&'static str> {
    [
        parser_core::PAGES,
        parser_editing::PAGES,
        markdown::PAGES,
        javascript::PAGES,
        guides::PAGES,
    ]
    .concat()
}
