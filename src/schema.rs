//! Opt-in schema validation.
//!
//! The crate is schema-agnostic by default; nothing here runs unless you call
//! [`Node::validate`]. A [`Schema`] is an allow-list of node types, marks,
//! attributes, and child types. Validation collects *all* problems as
//! [`Violation`]s (each carrying the offending node's index path), so a single
//! pass reports everything wrong.
//!
//! A schema can be built in Rust or loaded from JSON:
//!
//! ```
//! use tiptap_rusty_parser::{Document, Schema, NodeSpec, MarkSpec};
//!
//! let schema = Schema::new()
//!     .node("doc", NodeSpec::new().content(["paragraph"]))
//!     .node("paragraph", NodeSpec::new().content(["text"]).marks(["bold"]))
//!     .node("text", NodeSpec::new())
//!     .mark("bold", MarkSpec::new());
//!
//! let doc = Document::from_json_str(
//!     r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"hi"}]}]}"#,
//! ).unwrap();
//! assert!(doc.is_valid(&schema));
//! ```

use crate::content::{ContentExpr, ContentRule, ParseExprError};
use crate::node::Node;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;

/// An allow-list schema: which node/mark types, attributes, and children are
/// permitted. Build with [`Schema::new`] or load with [`Schema::from_json_str`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Schema {
    /// Node type -> its spec. Types absent here are reported as unknown.
    #[serde(default)]
    pub nodes: HashMap<String, NodeSpec>,
    /// Mark type -> its spec. Marks absent here are reported as unknown.
    #[serde(default)]
    pub marks: HashMap<String, MarkSpec>,
}

/// Rules for one node type.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NodeSpec {
    /// Allowed content: a set of child types (array form) or a content
    /// expression (string form). `None` = any child allowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<ContentRule>,
    /// Groups this node belongs to (space-separated, ProseMirror-style), so
    /// content expressions can reference it by group name (e.g. `block`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Allowed mark types on this node. `None` = any mark allowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marks: Option<HashSet<String>>,
    /// Allowed attribute keys. `None` = any attrs allowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attrs: Option<HashSet<String>>,
    /// Attribute keys that must be present.
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub required_attrs: HashSet<String>,
}

/// Rules for one mark type.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MarkSpec {
    /// Allowed attribute keys. `None` = any attrs allowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attrs: Option<HashSet<String>>,
    /// Attribute keys that must be present.
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub required_attrs: HashSet<String>,
}

fn into_set<I, S>(items: I) -> HashSet<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    items.into_iter().map(Into::into).collect()
}

impl Schema {
    /// An empty schema.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) a node type's spec.
    pub fn node(mut self, node_type: impl Into<String>, spec: NodeSpec) -> Self {
        self.nodes.insert(node_type.into(), spec);
        self
    }

    /// Register (or replace) a mark type's spec.
    pub fn mark(mut self, mark_type: impl Into<String>, spec: MarkSpec) -> Self {
        self.marks.insert(mark_type.into(), spec);
        self
    }

    /// Load a schema from its JSON definition.
    ///
    /// ```
    /// use tiptap_rusty_parser::Schema;
    /// let schema = Schema::from_json_str(r#"{
    ///   "nodes": { "doc": { "content": ["paragraph"] }, "paragraph": { "content": ["text"] }, "text": {} },
    ///   "marks": { "link": { "attrs": ["href"], "required_attrs": ["href"] } }
    /// }"#).unwrap();
    /// assert!(schema.nodes.contains_key("doc"));
    /// ```
    pub fn from_json_str(s: &str) -> crate::Result<Self> {
        Ok(serde_json::from_str(s)?)
    }
}

impl NodeSpec {
    /// An unrestricted node spec (any attrs/marks/children allowed).
    pub fn new() -> Self {
        Self::default()
    }

    /// Restrict allowed child node types (any count/order). For ordering and
    /// cardinality use [`content_match`](Self::content_match).
    pub fn content<I, S>(mut self, types: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.content = Some(ContentRule::Types(into_set(types)));
        self
    }

    /// Restrict content with a ProseMirror content expression (e.g.
    /// `"heading paragraph+"`). Panics on an invalid expression — use
    /// [`try_content_match`](Self::try_content_match) to handle the error.
    pub fn content_match(self, expr: &str) -> Self {
        self.try_content_match(expr)
            .expect("invalid content expression")
    }

    /// Fallible [`content_match`](Self::content_match).
    pub fn try_content_match(mut self, expr: &str) -> Result<Self, ParseExprError> {
        self.content = Some(ContentRule::Expr(ContentExpr::parse(expr)?));
        Ok(self)
    }

    /// Set the groups this node belongs to (space-separated, e.g. `"block"`).
    pub fn group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    /// The allowed child-type set, if `content` is the array form (not an
    /// expression). Convenience accessor for the `content` field.
    pub fn content_types(&self) -> Option<&HashSet<String>> {
        match &self.content {
            Some(ContentRule::Types(set)) => Some(set),
            _ => None,
        }
    }

    /// Restrict allowed mark types on this node.
    pub fn marks<I, S>(mut self, types: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.marks = Some(into_set(types));
        self
    }

    /// Restrict allowed attribute keys.
    pub fn attrs<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.attrs = Some(into_set(keys));
        self
    }

    /// Set attribute keys that must be present.
    pub fn required_attrs<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.required_attrs = into_set(keys);
        self
    }
}

impl MarkSpec {
    /// An unrestricted mark spec (any attrs allowed).
    pub fn new() -> Self {
        Self::default()
    }

    /// Restrict allowed attribute keys.
    pub fn attrs<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.attrs = Some(into_set(keys));
        self
    }

    /// Set attribute keys that must be present.
    pub fn required_attrs<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.required_attrs = into_set(keys);
        self
    }
}

/// A single schema violation, located by the offending node's index path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Violation {
    /// Index path to the node (root = `[]`), as used by [`Node::node_at`].
    pub path: Vec<usize>,
    /// What's wrong.
    pub kind: ViolationKind,
}

/// The kinds of schema violation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViolationKind {
    /// A node had no `type`.
    MissingNodeType,
    /// A node's type is not in the schema.
    UnknownNodeType(String),
    /// A child type is not allowed under its parent.
    DisallowedChild { parent: String, child: String },
    /// A node's children don't satisfy its content expression.
    InvalidContent { parent: String, expr: String },
    /// A mark type is not in the schema.
    UnknownMark(String),
    /// A mark is registered but not allowed on this node type.
    DisallowedMark { node: String, mark: String },
    /// A required attribute is missing (on a node, or a mark of the node).
    MissingAttr { key: String },
    /// An attribute key is not in the allowed set.
    UnknownAttr { key: String },
}

impl fmt::Display for ViolationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ViolationKind::MissingNodeType => write!(f, "node has no type"),
            ViolationKind::UnknownNodeType(t) => write!(f, "unknown node type `{t}`"),
            ViolationKind::DisallowedChild { parent, child } => {
                write!(f, "node type `{child}` not allowed inside `{parent}`")
            }
            ViolationKind::InvalidContent { parent, expr } => {
                write!(
                    f,
                    "children of `{parent}` do not match content expression `{expr}`"
                )
            }
            ViolationKind::UnknownMark(m) => write!(f, "unknown mark type `{m}`"),
            ViolationKind::DisallowedMark { node, mark } => {
                write!(f, "mark `{mark}` not allowed on `{node}`")
            }
            ViolationKind::MissingAttr { key } => write!(f, "missing required attribute `{key}`"),
            ViolationKind::UnknownAttr { key } => write!(f, "unknown attribute `{key}`"),
        }
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "at {:?}: {}", self.path, self.kind)
    }
}

impl Node {
    /// Validate against `schema`, collecting every [`Violation`]. An empty
    /// result means the document is valid.
    ///
    /// ```
    /// use tiptap_rusty_parser::{Document, Schema, NodeSpec};
    /// let schema = Schema::new()
    ///     .node("doc", NodeSpec::new().content(["paragraph"]))
    ///     .node("paragraph", NodeSpec::new())
    ///     .node("heading", NodeSpec::new());
    /// let doc = Document::from_json_str(
    ///     r#"{"type":"doc","content":[{"type":"heading"}]}"#,
    /// ).unwrap();
    /// let v = doc.validate(&schema);
    /// assert_eq!(v.len(), 1); // heading is a known type, but not allowed as a child of doc
    /// ```
    pub fn validate(&self, schema: &Schema) -> Vec<Violation> {
        let mut out = Vec::new();
        let mut path = Vec::new();
        validate_node(self, schema, &mut path, &mut out);
        out
    }

    /// True if the document has no schema violations.
    ///
    /// ```
    /// use tiptap_rusty_parser::{Document, Schema, NodeSpec};
    /// let schema = Schema::new().node("doc", NodeSpec::new());
    /// let doc = Document::from_json_str(r#"{"type":"doc"}"#).unwrap();
    /// assert!(doc.is_valid(&schema));
    /// ```
    pub fn is_valid(&self, schema: &Schema) -> bool {
        self.validate(schema).is_empty()
    }
}

fn validate_node(node: &Node, schema: &Schema, path: &mut Vec<usize>, out: &mut Vec<Violation>) {
    let push = |out: &mut Vec<Violation>, path: &[usize], kind: ViolationKind| {
        out.push(Violation {
            path: path.to_vec(),
            kind,
        });
    };

    let spec = match &node.node_type {
        None => {
            push(out, path, ViolationKind::MissingNodeType);
            None
        }
        Some(t) => match schema.nodes.get(t) {
            Some(spec) => Some(spec),
            None => {
                push(out, path, ViolationKind::UnknownNodeType(t.clone()));
                None
            }
        },
    };

    if let Some(spec) = spec {
        // node attrs
        check_attrs(
            node.attrs.as_ref(),
            spec.attrs.as_ref(),
            &spec.required_attrs,
            path,
            out,
        );

        // marks
        if let Some(marks) = &node.marks {
            let node_type = node.node_type.as_deref().unwrap_or_default();
            for mark in marks {
                match schema.marks.get(&mark.mark_type) {
                    None => push(
                        out,
                        path,
                        ViolationKind::UnknownMark(mark.mark_type.clone()),
                    ),
                    Some(mark_spec) => {
                        if let Some(allowed) = &spec.marks {
                            if !allowed.contains(&mark.mark_type) {
                                push(
                                    out,
                                    path,
                                    ViolationKind::DisallowedMark {
                                        node: node_type.to_string(),
                                        mark: mark.mark_type.clone(),
                                    },
                                );
                            }
                        }
                        check_attrs(
                            mark.attrs.as_ref(),
                            mark_spec.attrs.as_ref(),
                            &mark_spec.required_attrs,
                            path,
                            out,
                        );
                    }
                }
            }
        }

        // content rules
        if let Some(rule) = &spec.content {
            let parent = node.node_type.as_deref().unwrap_or_default();
            match rule {
                // array form: per-child type membership (unchanged behavior)
                ContentRule::Types(allowed) => {
                    if let Some(children) = &node.content {
                        for child in children {
                            if let Some(ct) = &child.node_type {
                                if !allowed.contains(ct) {
                                    push(
                                        out,
                                        path,
                                        ViolationKind::DisallowedChild {
                                            parent: parent.to_string(),
                                            child: ct.clone(),
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
                // expression form: cardinality + ordering over the child sequence
                ContentRule::Expr(expr) => {
                    let children = node.content.as_deref().unwrap_or(&[]);
                    if !expr.matches(children, schema) {
                        push(
                            out,
                            path,
                            ViolationKind::InvalidContent {
                                parent: parent.to_string(),
                                expr: expr.as_str().to_string(),
                            },
                        );
                    }
                }
            }
        }
    }

    // recurse
    if let Some(children) = &node.content {
        for (i, child) in children.iter().enumerate() {
            path.push(i);
            validate_node(child, schema, path, out);
            path.pop();
        }
    }
}

fn check_attrs(
    attrs: Option<&serde_json::Map<String, serde_json::Value>>,
    allowed: Option<&HashSet<String>>,
    required: &HashSet<String>,
    path: &[usize],
    out: &mut Vec<Violation>,
) {
    for key in required {
        let present = attrs.is_some_and(|m| m.contains_key(key));
        if !present {
            out.push(Violation {
                path: path.to_vec(),
                kind: ViolationKind::MissingAttr { key: key.clone() },
            });
        }
    }
    if let (Some(allowed), Some(attrs)) = (allowed, attrs) {
        for key in attrs.keys() {
            if !allowed.contains(key) {
                out.push(Violation {
                    path: path.to_vec(),
                    kind: ViolationKind::UnknownAttr { key: key.clone() },
                });
            }
        }
    }
}
