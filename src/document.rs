//! [`Document`] — owning wrapper around a root [`Node`] with (de)serialization.

use crate::diff::{ApplyError, Change};
use crate::error::Result;
use crate::node::Node;
use serde_json::Value;
use std::io::Read;
use std::ops::{Deref, DerefMut};

/// An owned Tiptap document (its root node, usually `type: "doc"`).
///
/// Derefs to the root [`Node`], so all query/mutation methods are available
/// directly on a `Document`.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    root: Node,
}

impl Document {
    /// Wrap an existing root node.
    #[inline]
    pub fn new(root: Node) -> Self {
        Document { root }
    }

    /// Parse from a JSON string.
    ///
    /// ```
    /// use tiptap_rusty_parser::Document;
    /// let d = Document::from_json_str(r#"{"type":"doc","content":[]}"#).unwrap();
    /// assert_eq!(d.node_type.as_deref(), Some("doc"));
    /// ```
    pub fn from_json_str(s: &str) -> Result<Self> {
        Ok(Document {
            root: serde_json::from_str(s)?,
        })
    }

    /// Parse from a `serde_json::Value`.
    pub fn from_value(value: Value) -> Result<Self> {
        Ok(Document {
            root: serde_json::from_value(value)?,
        })
    }

    /// Parse from any reader.
    pub fn from_reader(reader: impl Read) -> Result<Self> {
        Ok(Document {
            root: serde_json::from_reader(reader)?,
        })
    }

    /// Serialize to a compact JSON string.
    pub fn to_json_str(&self) -> Result<String> {
        Ok(serde_json::to_string(&self.root)?)
    }

    /// Serialize to a pretty JSON string.
    pub fn to_string_pretty(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(&self.root)?)
    }

    /// Serialize to a `serde_json::Value`.
    pub fn to_value(&self) -> Result<Value> {
        Ok(serde_json::to_value(&self.root)?)
    }

    /// Borrow the root node.
    #[inline]
    pub fn root(&self) -> &Node {
        &self.root
    }

    /// Mutably borrow the root node.
    #[inline]
    pub fn root_mut(&mut self) -> &mut Node {
        &mut self.root
    }

    /// Consume into the root node.
    #[inline]
    pub fn into_root(self) -> Node {
        self.root
    }

    /// Structural diff from this document to `other`. See [`Node::diff`].
    pub fn diff(&self, other: &Document) -> Vec<Change> {
        self.root.diff(&other.root)
    }

    /// Apply a [`Change`] list in place. See [`crate::apply`].
    pub fn apply(&mut self, changes: &[Change]) -> std::result::Result<(), ApplyError> {
        crate::diff::apply(&mut self.root, changes)
    }
}

impl Deref for Document {
    type Target = Node;
    #[inline]
    fn deref(&self) -> &Node {
        &self.root
    }
}

impl DerefMut for Document {
    #[inline]
    fn deref_mut(&mut self) -> &mut Node {
        &mut self.root
    }
}

impl From<Node> for Document {
    #[inline]
    fn from(root: Node) -> Self {
        Document { root }
    }
}
