//! Markdown -> JSONContent. Owner: import agent.

use serde::{Deserialize, Serialize};
use tiptap_rusty_parser::Document;

/// What to do with raw HTML found in Markdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HtmlPolicy {
    /// Keep it as plain text (visible, schema-valid).
    #[default]
    Text,
    /// Drop it.
    Drop,
}

/// Import options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ParseOptions {
    /// GFM extensions: tables, task lists, strikethrough, autolinks.
    pub gfm: bool,
    pub html: HtmlPolicy,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            gfm: true,
            html: HtmlPolicy::Text,
        }
    }
}

/// Parse Markdown with default options.
pub fn from_markdown(src: &str) -> Document {
    from_markdown_with(src, &ParseOptions::default())
}

/// Parse Markdown with `opts`.
pub fn from_markdown_with(src: &str, opts: &ParseOptions) -> Document {
    let _ = (src, opts);
    todo!()
}
