//! Selector helpers — convenience queries by type, mark, or attribute.
//!
//! Thin wrappers over the closure-based [`Node::find`] / [`Node::find_all`]
//! primitives in [`crate::query`]. Handy when you don't want to write a closure,
//! and a friendlier surface for future CLI / FFI layers (which can't pass Rust
//! closures across the boundary).

use crate::node::Node;
use serde_json::Value;

impl Node {
    /// All descendants (incl. `self`) whose type equals `node_type`.
    ///
    /// ```
    /// use tiptap_rusty_parser::Document;
    /// let doc = Document::from_json_str(
    ///     r#"{"type":"doc","content":[{"type":"paragraph"},{"type":"paragraph"}]}"#,
    /// ).unwrap();
    /// assert_eq!(doc.by_type("paragraph").len(), 2);
    /// ```
    pub fn by_type(&self, node_type: &str) -> Vec<&Node> {
        self.find_all(|n| n.node_type.as_deref() == Some(node_type))
    }

    /// First descendant (incl. `self`) whose type equals `node_type`, pre-order.
    pub fn first_by_type(&self, node_type: &str) -> Option<&Node> {
        self.find(|n| n.node_type.as_deref() == Some(node_type))
    }

    /// Mutable variant of [`Node::by_type`].
    pub fn by_type_mut(&mut self, node_type: &str) -> Vec<&mut Node> {
        let mut pred = |n: &Node| n.node_type.as_deref() == Some(node_type);
        self.find_all_mut(&mut pred)
    }

    /// All descendants (incl. `self`) carrying a mark of `mark_type`.
    ///
    /// ```
    /// use tiptap_rusty_parser::Document;
    /// let doc = Document::from_json_str(
    ///     r#"{"type":"doc","content":[{"type":"text","text":"a","marks":[{"type":"bold"}]}]}"#,
    /// ).unwrap();
    /// assert_eq!(doc.by_mark("bold").len(), 1);
    /// ```
    pub fn by_mark(&self, mark_type: &str) -> Vec<&Node> {
        self.find_all(|n| n.has_mark(mark_type))
    }

    /// All descendants (incl. `self`) whose attribute `key` equals `value`.
    ///
    /// ```
    /// use tiptap_rusty_parser::Document;
    /// let doc = Document::from_json_str(
    ///     r#"{"type":"doc","content":[{"type":"heading","attrs":{"level":1}}]}"#,
    /// ).unwrap();
    /// assert_eq!(doc.by_attr("level", 1).len(), 1);
    /// ```
    pub fn by_attr(&self, key: &str, value: impl Into<Value>) -> Vec<&Node> {
        let value = value.into();
        self.find_all(|n| n.attr(key) == Some(&value))
    }
}
