//! Ergonomic constructors / builders for nodes.

use crate::node::{Mark, Node};
use serde_json::{Map, Value};

impl Node {
    /// Start an element node of `node_type` (no content/marks/attrs yet).
    ///
    /// ```
    /// use tiptap_rusty_parser::Node;
    /// let p = Node::element("paragraph")
    ///     .with_attr("textAlign", "center")
    ///     .with_text("hello");
    /// assert_eq!(p.child_count(), 1);
    /// ```
    #[inline]
    pub fn element(node_type: impl Into<String>) -> Node {
        Node {
            node_type: Some(node_type.into()),
            ..Default::default()
        }
    }

    /// A `text` node with the given string.
    #[inline]
    pub fn text(text: impl Into<String>) -> Node {
        Node {
            node_type: Some("text".into()),
            text: Some(text.into()),
            ..Default::default()
        }
    }

    /// A `text` node with `text` and the given marks.
    pub fn text_with_marks(text: impl Into<String>, marks: impl IntoIterator<Item = Mark>) -> Node {
        Node {
            node_type: Some("text".into()),
            text: Some(text.into()),
            marks: Some(marks.into_iter().collect()),
            ..Default::default()
        }
    }

    // ---- builder-style chaining ----------------------------------------

    /// Builder: set one attr (consuming).
    pub fn with_attr(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.attrs
            .get_or_insert_with(Map::new)
            .insert(key.into(), value.into());
        self
    }

    /// Builder: add a child (consuming).
    pub fn with_child(mut self, node: Node) -> Self {
        self.push_child(node);
        self
    }

    /// Builder: add several children (consuming).
    pub fn with_children(mut self, nodes: impl IntoIterator<Item = Node>) -> Self {
        self.children_mut().extend(nodes);
        self
    }

    /// Builder: add a `text` child (consuming).
    pub fn with_text(self, text: impl Into<String>) -> Self {
        self.with_child(Node::text(text))
    }

    /// Builder: add a mark (consuming).
    pub fn with_mark(mut self, mark: Mark) -> Self {
        self.marks.get_or_insert_with(Vec::new).push(mark);
        self
    }
}

/// Build a `doc` root node from its children.
///
/// ```
/// use tiptap_rusty_parser::{doc, Node};
/// let d = doc([Node::element("paragraph").with_text("hi")]);
/// assert_eq!(d.node_type.as_deref(), Some("doc"));
/// ```
pub fn doc(children: impl IntoIterator<Item = Node>) -> Node {
    Node {
        node_type: Some("doc".into()),
        content: Some(children.into_iter().collect()),
        ..Default::default()
    }
}
