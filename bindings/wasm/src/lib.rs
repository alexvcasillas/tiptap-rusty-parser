//! WebAssembly bindings for `tiptap-rusty-parser`.
//!
//! Exposes an opaque [`TiptapDoc`] handle to JavaScript: the document tree
//! stays owned by Rust/WASM and is only serialized to JS objects at the
//! boundary. Queries return cloned nodes (read) or index paths (`number[][]`,
//! for mutation targeting); mutation is path-addressed. Values cross the
//! boundary via `serde-wasm-bindgen`, so `path` arguments are plain `number[]`
//! and node/attr/schema arguments are plain JS objects.

use serde::Serialize;
use serde_json::Value;
use serde_wasm_bindgen::{from_value, Serializer};
use tiptap_rusty_parser::{Change, Document, HtmlOptions, Mark, Node, Schema};
use wasm_bindgen::prelude::*;

/// Map a `Display` error into a JS exception.
fn err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

/// Serialize to a *plain* JS object/array (maps become objects, not `Map`).
fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsError> {
    value.serialize(&Serializer::json_compatible()).map_err(err)
}

/// Parse a JS `number[]` into an index path.
fn parse_path(path: JsValue) -> Result<Vec<usize>, JsError> {
    from_value(path).map_err(err)
}

/// A Tiptap/ProseMirror document handle. Construct with [`TiptapDoc::from_json`]
/// or [`TiptapDoc::from_json_string`].
#[wasm_bindgen]
pub struct TiptapDoc {
    inner: Document,
}

#[wasm_bindgen]
impl TiptapDoc {
    // ---- lifecycle / serialize ----

    /// Build from a JS `JSONContent` object.
    #[wasm_bindgen(js_name = fromJSON)]
    pub fn from_json(value: JsValue) -> Result<TiptapDoc, JsError> {
        let node: Node = from_value(value).map_err(err)?;
        Ok(Self {
            inner: Document::new(node),
        })
    }

    /// Build from a JSON string.
    #[wasm_bindgen(js_name = fromJSONString)]
    pub fn from_json_string(s: &str) -> Result<TiptapDoc, JsError> {
        Ok(Self {
            inner: Document::from_json_str(s).map_err(err)?,
        })
    }

    /// Serialize the document to a JS `JSONContent` object.
    #[wasm_bindgen(js_name = toJSON)]
    pub fn to_json(&self) -> Result<JsValue, JsError> {
        to_js(self.inner.root())
    }

    /// Serialize the document to a JSON string.
    #[wasm_bindgen(js_name = toJSONString)]
    pub fn to_json_string(&self) -> Result<String, JsError> {
        self.inner.to_json_str().map_err(err)
    }

    // ---- read queries (return cloned node objects) ----

    /// All nodes of `node_type`, as an array of JS objects.
    #[wasm_bindgen(js_name = byType)]
    pub fn by_type(&self, node_type: &str) -> Result<JsValue, JsError> {
        to_js(&self.inner.by_type(node_type))
    }

    /// The first node of `node_type`, or `undefined`.
    #[wasm_bindgen(js_name = firstByType)]
    pub fn first_by_type(&self, node_type: &str) -> Result<JsValue, JsError> {
        match self.inner.first_by_type(node_type) {
            Some(n) => to_js(n),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    /// All nodes carrying a mark of `mark_type`.
    #[wasm_bindgen(js_name = byMark)]
    pub fn by_mark(&self, mark_type: &str) -> Result<JsValue, JsError> {
        to_js(&self.inner.by_mark(mark_type))
    }

    /// All nodes whose attribute `key` equals `value`.
    #[wasm_bindgen(js_name = byAttr)]
    pub fn by_attr(&self, key: &str, value: JsValue) -> Result<JsValue, JsError> {
        let v: Value = from_value(value).map_err(err)?;
        to_js(&self.inner.by_attr(key, v))
    }

    // ---- path-returning selectors (for mutation targeting) ----

    /// Index paths (`number[][]`) of every node of `node_type`.
    #[wasm_bindgen(js_name = pathsByType)]
    pub fn paths_by_type(&self, node_type: &str) -> Result<JsValue, JsError> {
        to_js(
            &self
                .inner
                .paths_to(|n| n.node_type.as_deref() == Some(node_type)),
        )
    }

    /// Index paths of every node carrying a mark of `mark_type`.
    #[wasm_bindgen(js_name = pathsByMark)]
    pub fn paths_by_mark(&self, mark_type: &str) -> Result<JsValue, JsError> {
        to_js(&self.inner.paths_to(|n| n.has_mark(mark_type)))
    }

    /// Index paths of every node whose attribute `key` equals `value`.
    #[wasm_bindgen(js_name = pathsByAttr)]
    pub fn paths_by_attr(&self, key: &str, value: JsValue) -> Result<JsValue, JsError> {
        let v: Value = from_value(value).map_err(err)?;
        to_js(&self.inner.paths_to(|n| n.attr(key) == Some(&v)))
    }

    // ---- read a single node ----

    /// The node at `path`, or `undefined`.
    #[wasm_bindgen(js_name = nodeAt)]
    pub fn node_at(&self, path: JsValue) -> Result<JsValue, JsError> {
        let path = parse_path(path)?;
        match self.inner.node_at(&path) {
            Some(n) => to_js(n),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    /// Child count of the node at `path`, or `undefined` if no such node.
    #[wasm_bindgen(js_name = childCount)]
    pub fn child_count(&self, path: JsValue) -> Result<Option<usize>, JsError> {
        let path = parse_path(path)?;
        Ok(self.inner.node_at(&path).map(Node::child_count))
    }

    // ---- path-addressed mutation ----

    /// Set attribute `key` to `value` on the node at `path`.
    #[wasm_bindgen(js_name = setAttr)]
    pub fn set_attr(&mut self, path: JsValue, key: String, value: JsValue) -> Result<(), JsError> {
        let path = parse_path(path)?;
        let v: Value = from_value(value).map_err(err)?;
        self.at_mut(&path)?.set_attr(key, v);
        Ok(())
    }

    /// Remove attribute `key` from the node at `path`; returns whether it existed.
    #[wasm_bindgen(js_name = removeAttr)]
    pub fn remove_attr(&mut self, path: JsValue, key: &str) -> Result<bool, JsError> {
        let path = parse_path(path)?;
        Ok(self.at_mut(&path)?.remove_attr(key).is_some())
    }

    /// Set the `text` of the node at `path`.
    #[wasm_bindgen(js_name = setText)]
    pub fn set_text(&mut self, path: JsValue, text: String) -> Result<(), JsError> {
        let path = parse_path(path)?;
        self.at_mut(&path)?.set_text(text);
        Ok(())
    }

    /// Add a mark of `mark_type` (optional `attrs` object) to the node at `path`;
    /// returns whether it was newly added.
    #[wasm_bindgen(js_name = addMark)]
    pub fn add_mark(
        &mut self,
        path: JsValue,
        mark_type: String,
        attrs: JsValue,
    ) -> Result<bool, JsError> {
        let path = parse_path(path)?;
        let mut mark = Mark::new(mark_type);
        if !attrs.is_null() && !attrs.is_undefined() {
            let m: serde_json::Map<String, Value> = from_value(attrs).map_err(err)?;
            if !m.is_empty() {
                mark.attrs = Some(m);
            }
        }
        Ok(self.at_mut(&path)?.add_mark(mark))
    }

    /// Remove all marks of `mark_type` from the node at `path`; returns the count removed.
    #[wasm_bindgen(js_name = removeMark)]
    pub fn remove_mark(&mut self, path: JsValue, mark_type: &str) -> Result<usize, JsError> {
        let path = parse_path(path)?;
        Ok(self.at_mut(&path)?.remove_mark(mark_type))
    }

    /// Append `child` to the node at `path`.
    #[wasm_bindgen(js_name = pushChild)]
    pub fn push_child(&mut self, path: JsValue, child: JsValue) -> Result<(), JsError> {
        let path = parse_path(path)?;
        let child: Node = from_value(child).map_err(err)?;
        self.at_mut(&path)?.push_child(child);
        Ok(())
    }

    /// Insert `child` at `index` under the node at `path` (index clamped).
    #[wasm_bindgen(js_name = insertChild)]
    pub fn insert_child(
        &mut self,
        path: JsValue,
        index: usize,
        child: JsValue,
    ) -> Result<(), JsError> {
        let path = parse_path(path)?;
        let child: Node = from_value(child).map_err(err)?;
        self.at_mut(&path)?.insert_child(index, child);
        Ok(())
    }

    /// Remove the child at `index` under the node at `path`; returns it, or `undefined`.
    #[wasm_bindgen(js_name = removeChild)]
    pub fn remove_child(&mut self, path: JsValue, index: usize) -> Result<JsValue, JsError> {
        let path = parse_path(path)?;
        match self.at_mut(&path)?.remove_child(index) {
            Some(removed) => to_js(&removed),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    // ---- text ----

    /// Concatenated text of all descendant text nodes.
    #[wasm_bindgen(js_name = textContent)]
    pub fn text_content(&self) -> String {
        self.inner.text_content()
    }

    /// Unicode scalar count of the extracted text.
    #[wasm_bindgen(js_name = charCount)]
    pub fn char_count(&self) -> usize {
        self.inner.char_count()
    }

    /// Word count of the extracted text.
    #[wasm_bindgen(js_name = wordCount)]
    pub fn word_count(&self) -> usize {
        self.inner.word_count()
    }

    // ---- validation ----

    /// Validate against a `schema` object; returns an array of violations
    /// (empty = valid).
    pub fn validate(&self, schema: JsValue) -> Result<JsValue, JsError> {
        let schema: Schema = from_value(schema).map_err(err)?;
        to_js(&self.inner.validate(&schema))
    }

    /// True if the document has no schema violations.
    #[wasm_bindgen(js_name = isValid)]
    pub fn is_valid(&self, schema: JsValue) -> Result<bool, JsError> {
        let schema: Schema = from_value(schema).map_err(err)?;
        Ok(self.inner.is_valid(&schema))
    }

    // ---- diff / apply ----

    /// Structural diff from this document to `other`; returns an array of
    /// change objects (each tagged with an `op`).
    pub fn diff(&self, other: &TiptapDoc) -> Result<JsValue, JsError> {
        to_js(&self.inner.root().diff(other.inner.root()))
    }

    /// Apply a change array (as produced by [`diff`](Self::diff)) in place.
    #[wasm_bindgen(js_name = applyChanges)]
    pub fn apply_changes(&mut self, changes: JsValue) -> Result<(), JsError> {
        let changes: Vec<Change> = from_value(changes).map_err(err)?;
        tiptap_rusty_parser::apply(self.inner.root_mut(), &changes).map_err(err)
    }

    /// Invert a change array relative to this document (the pre-image); returns
    /// the reverse change array for undo. See `applyChanges`.
    pub fn invert(&self, changes: JsValue) -> Result<JsValue, JsError> {
        let changes: Vec<Change> = from_value(changes).map_err(err)?;
        let inverse = tiptap_rusty_parser::invert(self.inner.root(), &changes).map_err(err)?;
        to_js(&inverse)
    }

    // ---- HTML rendering ----

    /// Render the document to an HTML string (Tiptap-default mapping).
    #[wasm_bindgen(js_name = toHTML)]
    pub fn to_html(&self) -> String {
        self.inner.to_html()
    }

    /// Render to HTML with an options object (see `HtmlOptions`).
    #[wasm_bindgen(js_name = toHTMLWith)]
    pub fn to_html_with(&self, options: JsValue) -> Result<String, JsError> {
        let opts: HtmlOptions = from_value(options).map_err(err)?;
        Ok(self.inner.to_html_with(&opts))
    }
}

impl TiptapDoc {
    /// Resolve a mutable node at `path` or raise a JS error.
    fn at_mut(&mut self, path: &[usize]) -> Result<&mut Node, JsError> {
        self.inner
            .node_at_mut(path)
            .ok_or_else(|| JsError::new("no node at path"))
    }
}
