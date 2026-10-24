//! Compiles the Rust code blocks of the docs site as doctests, so every
//! example on the site builds against the crates in this workspace.
//!
//! Register each page that has ```` ```rust ```` fences in [`pages!`]; the
//! `coverage` test fails when a page with Rust code is missing here.

macro_rules! pages {
    ($($name:ident => $path:literal),* $(,)?) => {
        $(
            #[cfg(doctest)]
            #[doc = include_str!(concat!("../../docs/content/docs/", $path))]
            pub struct $name;
        )*

        /// Pages (relative to `docs/content/docs/`) whose Rust fences compile.
        pub const PAGES: &[&str] = &[$($path),*];
    };
}

pages! {}
