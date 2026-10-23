//! Core data model: [`Node`] and [`Mark`].
//!
//! Mirrors Tiptap's `JSONContent` interface. Schema-agnostic: any node/mark
//! `type` is accepted and unknown JSON fields are preserved in `extra` for
//! faithful roundtrip.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A single Tiptap/ProseMirror node.
///
/// All structural fields are optional to faithfully distinguish "missing" from
/// "empty" on roundtrip. Unknown top-level keys land in [`Node::extra`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Node {
    /// Node type, e.g. `"doc"`, `"paragraph"`, `"text"`.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub node_type: Option<String>,

    /// Node attributes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attrs: Option<Map<String, Value>>,

    /// Child nodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<Node>>,

    /// Marks applied to this node (typically text nodes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marks: Option<Vec<Mark>>,

    /// Text payload (text nodes only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Any unknown/extra top-level fields, preserved verbatim.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A mark applied to a node (e.g. `bold`, `italic`, `link`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mark {
    /// Mark type, e.g. `"bold"`.
    #[serde(rename = "type")]
    pub mark_type: String,

    /// Mark attributes (e.g. `href` for a link).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attrs: Option<Map<String, Value>>,

    /// Any unknown/extra fields, preserved verbatim.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Mark {
    /// Create a mark of `mark_type` with no attrs.
    ///
    /// ```
    /// use tiptap_rusty_parser::Mark;
    /// let m = Mark::new("bold");
    /// assert_eq!(m.mark_type, "bold");
    /// ```
    #[inline]
    pub fn new(mark_type: impl Into<String>) -> Self {
        Mark {
            mark_type: mark_type.into(),
            attrs: None,
            extra: Map::new(),
        }
    }

    /// Builder: set one attr.
    #[inline]
    pub fn attr(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.attrs
            .get_or_insert_with(Map::new)
            .insert(key.into(), value.into());
        self
    }
}
