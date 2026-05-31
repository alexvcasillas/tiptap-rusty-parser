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
//! - **Address** by index path: [`Node::node_at`], [`Node::path_to`]; or by flat
//!   ProseMirror integer position: [`Node::resolve`], [`Node::pos_before`], [`ResolvedPos`].
//! - **Mutate** in place: marks, attrs, children, text, and bulk
//!   [`Node::replace_all`].
//! - **Normalize** to a canonical form (merge adjacent text, drop empties):
//!   [`Node::normalize`], [`NormalizeOptions`].
//! - **Range-edit** a block's inline content (insert/delete/replace text, mark
//!   ranges): [`Node::insert_text`], [`Node::add_mark_range`], [`Position`], [`Range`].
//! - **Restructure** the block tree (split/join/wrap/lift/retype):
//!   [`Node::split_block`], [`Node::join_blocks`], [`Node::wrap`], [`Node::lift`], [`BlockRange`].
//! - **Diff / apply / invert** structural change lists between two trees
//!   (undo-capable): [`Node::diff`], [`apply`], [`invert`].
//! - **Edit by flat position**: apply a batch of [`PosEdit`]s addressed by
//!   ProseMirror integer positions, recovering an invertible patch:
//!   [`Node::apply_pos_edits`], [`PosContent`].
//! - **Transact**: mutate in place while recording a replayable/invertible
//!   change log: [`Node::transform`], [`Transform`].
//! - **Operate on change lists**: [`compose`], [`compact`], and carry a path
//!   through edits with [`map_path`].
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

mod block;
mod builder;
mod change_ops;
mod content;
mod diff;
mod document;
mod error;
mod html;
mod mutate;
mod node;
mod normalize;
mod path;
mod pos;
mod pos_edit;
mod query;
mod range;
mod schema;
mod select;
mod text;
mod text_diff;
mod transform;

pub use block::{BlockError, BlockRange};
pub use builder::doc;
pub use change_ops::{compact, compose, map_path};
pub use content::{ContentExpr, ContentRule, ParseExprError};
pub use diff::{apply, diff, invert, ApplyError, Change};
pub use document::Document;
pub use error::{ParseError, Result};
pub use html::{to_html, HtmlOptions, SelfClosingStyle, UnknownMarkPolicy, UnknownNodePolicy};
pub use node::{Mark, Node};
pub use normalize::NormalizeOptions;
pub use pos::{LeafPolicy, PosError, PosRange, ResolvedPos, TextPoint};
pub use pos_edit::{PosContent, PosEdit, PosEditError};
pub use query::Descendants;
pub use range::{Position, Range, RangeError};
pub use schema::{MarkSpec, NodeSpec, Schema, Violation, ViolationKind};
pub use text_diff::{diff_text, DiffGranularity, DiffOptions, SegKind, TextSegment};
pub use transform::Transform;
