//! JSONContent -> Markdown. Owner: export agent.

use serde::{Deserialize, Serialize};
use tiptap_rusty_parser::Node;

/// How to render a node with no Markdown equivalent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnknownNodePolicy {
    /// Render its children only.
    #[default]
    Transparent,
    /// Emit the node as HTML via the parser's `to_html`.
    Html,
    /// Drop it and its children.
    Skip,
}

/// Export options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RenderOptions {
    pub unknown_node: UnknownNodePolicy,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            unknown_node: UnknownNodePolicy::Transparent,
        }
    }
}

/// Render `node` to Markdown with default options.
pub fn to_markdown(node: &Node) -> String {
    to_markdown_with(node, &RenderOptions::default())
}

/// Render `node` to Markdown with `opts`.
pub fn to_markdown_with(node: &Node, opts: &RenderOptions) -> String {
    let _ = (node, opts);
    todo!()
}
