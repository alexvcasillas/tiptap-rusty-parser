//! Markdown <-> Tiptap/ProseMirror JSONContent on top of [`tiptap_rusty_parser`].
//!
//! - [`from_markdown`]: CommonMark (+ GFM by default) to a [`Document`] using
//!   Tiptap StarterKit names (`paragraph`, `heading`, `bulletList`, ...).
//! - [`to_markdown`]: a [`Node`] tree back to Markdown.
//!
//! Options are plain data structs (no closures) so the same surface can be
//! exposed over WASM/FFI later.

mod from_md;
mod to_md;

pub use from_md::{from_markdown, from_markdown_with, HtmlPolicy, ParseOptions};
pub use tiptap_rusty_parser::{Document, Node};
pub use to_md::{to_markdown, to_markdown_with, RenderOptions, UnknownNodePolicy};
