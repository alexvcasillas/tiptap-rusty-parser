//! # tiptap-rusty-parser
//!
//! Fast, schema-agnostic parser and manipulator for Tiptap / ProseMirror
//! `JSONContent` documents.
//!
//! - **Parse / serialize** via [`Document`] (faithful roundtrip, unknown fields
//!   preserved).
//! - **Query** with predicate closures: [`Node::find`], [`Node::find_all`],
//!   [`Node::walk`], [`Node::descendants`].
//! - **Mutate** in place: marks, attrs, children, text, and bulk
//!   [`Node::replace_all`].
//! - **Build** nodes ergonomically: [`Node::element`], [`Node::text`], [`doc`].
//!
//! ```
//! use tiptap_rusty_parser::{Document, Mark, Node};
//!
//! let mut doc = Document::from_json_str(
//!     r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"hi"}]}]}"#,
//! )
//! .unwrap();
//!
//! // Bold every text node.
//! doc.replace_all(
//!     |n| n.node_type.as_deref() == Some("text"),
//!     |n| { n.add_mark(Mark::new("bold")); },
//! );
//!
//! // Append a new paragraph.
//! doc.push_child(Node::element("paragraph").with_text("bye"));
//!
//! assert_eq!(doc.find_all(|n| n.node_type.as_deref() == Some("paragraph")).len(), 2);
//! ```

mod builder;
mod document;
mod error;
mod mutate;
mod node;
mod query;

pub use builder::doc;
pub use document::Document;
pub use error::{ParseError, Result};
pub use node::{Mark, Node};
pub use query::Descendants;
