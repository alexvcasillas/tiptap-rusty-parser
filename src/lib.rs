//! # tiptap-rusty-parser
//!
//! Fast, schema-agnostic parser and manipulator for Tiptap / ProseMirror
//! `JSONContent` documents.
//!
//! - **Parse / serialize** via [`Document`] (faithful roundtrip, unknown fields
//!   preserved).
//! - **Query** with predicate closures: [`Node::find`], [`Node::find_all`],
//!   [`Node::walk`], [`Node::descendants`].
//! - **Select** by type/mark/attr: [`Node::by_type`], [`Node::by_mark`],
//!   [`Node::by_attr`].
//! - **Address** by index path: [`Node::node_at`], [`Node::path_to`].
//! - **Mutate** in place: marks, attrs, children, text, and bulk
//!   [`Node::replace_all`].
//! - **Diff / apply / invert** structural change lists between two trees
//!   (undo-capable): [`Node::diff`], [`apply`], [`invert`].
//! - **Extract** text: [`Node::text_content`], [`Node::word_count`].
//! - **Validate** (opt-in) against a schema, incl. ProseMirror content
//!   expressions: [`Node::validate`], [`Schema`], [`ContentExpr`].
//! - **Render** to HTML: [`Node::to_html`], [`HtmlOptions`].
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
mod content;
mod diff;
mod document;
mod error;
mod html;
mod mutate;
mod node;
mod path;
mod query;
mod schema;
mod select;
mod text;

pub use builder::doc;
pub use content::{ContentExpr, ContentRule, ParseExprError};
pub use diff::{apply, diff, invert, ApplyError, Change};
pub use document::Document;
pub use error::{ParseError, Result};
pub use html::{to_html, HtmlOptions, SelfClosingStyle, UnknownMarkPolicy, UnknownNodePolicy};
pub use node::{Mark, Node};
pub use query::Descendants;
pub use schema::{MarkSpec, NodeSpec, Schema, Violation, ViolationKind};
