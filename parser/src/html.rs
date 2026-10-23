//! Render [`Node`] trees to an HTML string (`to_html`).
//!
//! Schema-agnostic with Tiptap-sensible defaults: `paragraph`→`<p>`,
//! `heading`→`<h1>`..`<h6>`, marks like `bold`→`<strong>`, etc. Output is
//! compact; **text content and attribute values are HTML-escaped** (the element
//! tags themselves are emitted as markup). Behavior is tuned with [`HtmlOptions`]
//! — a plain data struct (no closures), so the same surface works over WASM/FFI.
//!
//! Escaping is not sanitization: URLs (e.g. a `link` `href`) are emitted as-is,
//! so a `javascript:` href survives. Sanitize output from untrusted documents.
//!
//! ```
//! use tiptap_rusty_parser::Document;
//! let doc = Document::from_json_str(
//!     r#"{"type":"doc","content":[{"type":"paragraph","content":[
//!         {"type":"text","text":"hi","marks":[{"type":"bold"}]}]}]}"#,
//! ).unwrap();
//! assert_eq!(doc.to_html(), "<p><strong>hi</strong></p>");
//! ```

use crate::node::{Mark, Node};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::HashMap;

/// How to render a node whose type has no built-in or configured mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnknownNodePolicy {
    /// Render the children with no wrapping element (default).
    #[default]
    Transparent,
    /// Wrap the children in `<div data-type="…">`.
    DataTypeDiv,
    /// Emit nothing (drop the subtree).
    Skip,
}

/// How to render a mark whose type has no built-in or configured mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnknownMarkPolicy {
    /// Render the text without the mark's element (default).
    #[default]
    Transparent,
    /// Wrap the text in `<span data-mark="…">`.
    DataMarkSpan,
    /// Drop the marked text entirely.
    Skip,
}

/// Whether void elements self-close (`<br/>`) or use the HTML5 form (`<br>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SelfClosingStyle {
    /// `<br>`, `<hr>`, `<img …>` (default).
    #[default]
    Html5,
    /// `<br/>`, `<hr/>`, `<img …/>`.
    Xhtml,
}

/// Options controlling HTML rendering. [`Default`] matches Tiptap conventions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HtmlOptions {
    /// Override/extend the node→tag map with simple wrappers (e.g. `callout`→`aside`).
    pub node_tags: HashMap<String, String>,
    /// Override/extend the mark→tag map with simple wrappers (e.g. `highlight`→`mark`).
    pub mark_tags: HashMap<String, String>,
    /// How to render unknown node types.
    pub unknown_node: UnknownNodePolicy,
    /// How to render unknown mark types.
    pub unknown_mark: UnknownMarkPolicy,
    /// Void-element style.
    pub self_closing: SelfClosingStyle,
    /// Emit a node's remaining (non-structural) `attrs` as HTML attributes.
    /// Off by default — arbitrary attribute names are an injection footgun.
    pub spread_attrs: bool,
    /// Emit `style="text-align:…"` for `paragraph`/`heading` `textAlign` attrs.
    pub text_align: bool,
}

impl Default for HtmlOptions {
    fn default() -> Self {
        Self {
            node_tags: HashMap::new(),
            mark_tags: HashMap::new(),
            unknown_node: UnknownNodePolicy::default(),
            unknown_mark: UnknownMarkPolicy::default(),
            self_closing: SelfClosingStyle::default(),
            spread_attrs: false,
            text_align: true,
        }
    }
}

impl Node {
    /// Render this node (and its subtree) to an HTML string with default options.
    pub fn to_html(&self) -> String {
        self.to_html_with(&HtmlOptions::default())
    }

    /// Render to HTML with custom [`HtmlOptions`].
    pub fn to_html_with(&self, opts: &HtmlOptions) -> String {
        let mut out = String::with_capacity(256);
        render_node(self, opts, &mut out);
        out
    }
}

/// Render a node to an HTML string. Free-function form of [`Node::to_html`].
pub fn to_html(node: &Node) -> String {
    node.to_html()
}

// ---- rendering ----------------------------------------------------------

fn render_node(n: &Node, opts: &HtmlOptions, out: &mut String) {
    let ty = match n.node_type.as_deref() {
        None => return render_children(n, opts, out),
        Some(t) => t,
    };
    match ty {
        "text" => return render_text(n, opts, out),
        "doc" => return render_children(n, opts, out),
        _ => {}
    }
    // configured simple-wrapper override takes precedence over built-ins
    if let Some(tag) = opts.node_tags.get(ty) {
        out.push('<');
        out.push_str(tag);
        if opts.spread_attrs {
            spread(n, &[], out);
        }
        out.push('>');
        render_children(n, opts, out);
        out.push_str("</");
        out.push_str(tag);
        out.push('>');
        return;
    }
    match ty {
        "paragraph" => wrap(n, opts, out, "p", true, &["textAlign"]),
        "heading" => {
            let level = n
                .attr("level")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 6) as u8;
            let digit = (b'0' + level) as char;
            out.push_str("<h");
            out.push(digit);
            write_text_align(n, opts, out);
            if opts.spread_attrs {
                spread(n, &["level", "textAlign"], out);
            }
            out.push('>');
            render_children(n, opts, out);
            out.push_str("</h");
            out.push(digit);
            out.push('>');
        }
        "blockquote" => wrap(n, opts, out, "blockquote", false, &[]),
        "bulletList" => wrap(n, opts, out, "ul", false, &[]),
        "listItem" => wrap(n, opts, out, "li", false, &[]),
        "orderedList" => {
            out.push_str("<ol");
            if let Some(start) = n.attr("start").and_then(Value::as_u64) {
                if start != 1 {
                    out.push_str(" start=\"");
                    out.push_str(&start.to_string());
                    out.push('"');
                }
            }
            if opts.spread_attrs {
                spread(n, &["start"], out);
            }
            out.push('>');
            render_children(n, opts, out);
            out.push_str("</ol>");
        }
        "codeBlock" => {
            out.push_str("<pre><code");
            if let Some(Value::String(lang)) = n.attr("language") {
                if !lang.is_empty() {
                    out.push_str(" class=\"language-");
                    escape_attr(lang, out);
                    out.push('"');
                }
            }
            if opts.spread_attrs {
                spread(n, &["language"], out);
            }
            out.push('>');
            render_code_text(n, out); // raw text, marks ignored
            out.push_str("</code></pre>");
        }
        "hardBreak" => void(out, "br", opts),
        "horizontalRule" => void(out, "hr", opts),
        "image" => {
            out.push_str("<img");
            for key in ["src", "alt", "title"] {
                if let Some(v) = n.attr(key).and_then(attr_string) {
                    write_attr(key, &v, out);
                }
            }
            if opts.spread_attrs {
                spread(n, &["src", "alt", "title"], out);
            }
            void_close(out, opts);
        }
        other => render_unknown(n, opts, out, other),
    }
}

fn render_children(n: &Node, opts: &HtmlOptions, out: &mut String) {
    if let Some(children) = &n.content {
        for c in children {
            render_node(c, opts, out);
        }
    }
}

fn wrap(n: &Node, opts: &HtmlOptions, out: &mut String, tag: &str, align: bool, consumed: &[&str]) {
    out.push('<');
    out.push_str(tag);
    if align {
        write_text_align(n, opts, out);
    }
    if opts.spread_attrs {
        spread(n, consumed, out);
    }
    out.push('>');
    render_children(n, opts, out);
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

fn render_unknown(n: &Node, opts: &HtmlOptions, out: &mut String, ty: &str) {
    match opts.unknown_node {
        UnknownNodePolicy::Transparent => render_children(n, opts, out),
        UnknownNodePolicy::Skip => {}
        UnknownNodePolicy::DataTypeDiv => {
            out.push_str("<div data-type=\"");
            escape_attr(ty, out);
            out.push('"');
            if opts.spread_attrs {
                spread(n, &[], out);
            }
            out.push('>');
            render_children(n, opts, out);
            out.push_str("</div>");
        }
    }
}

fn render_text(n: &Node, opts: &HtmlOptions, out: &mut String) {
    let text = n.text.as_deref().unwrap_or("");
    let marks = n.marks.as_deref().unwrap_or(&[]);
    // `Skip` policy drops text carrying an unknown mark entirely.
    if opts.unknown_mark == UnknownMarkPolicy::Skip
        && marks.iter().any(|m| is_unknown_mark(m, opts))
    {
        return;
    }
    for m in marks {
        open_mark(m, opts, out);
    }
    escape_text(text, out);
    for m in marks.iter().rev() {
        close_mark(m, opts, out);
    }
}

/// Concatenate descendant text (escaped), ignoring marks — for `codeBlock`.
fn render_code_text(n: &Node, out: &mut String) {
    if let Some(t) = &n.text {
        escape_text(t, out);
    }
    if let Some(children) = &n.content {
        for c in children {
            render_code_text(c, out);
        }
    }
}

fn builtin_mark_tag(mark_type: &str) -> Option<&'static str> {
    Some(match mark_type {
        "bold" => "strong",
        "italic" => "em",
        "strike" => "s",
        "code" => "code",
        "underline" => "u",
        "subscript" => "sub",
        "superscript" => "sup",
        "link" => "a",
        _ => return None,
    })
}

fn is_unknown_mark(m: &Mark, opts: &HtmlOptions) -> bool {
    !opts.mark_tags.contains_key(&m.mark_type) && builtin_mark_tag(&m.mark_type).is_none()
}

fn open_mark(m: &Mark, opts: &HtmlOptions, out: &mut String) {
    if let Some(tag) = opts.mark_tags.get(&m.mark_type) {
        out.push('<');
        out.push_str(tag);
        out.push('>');
        return;
    }
    match m.mark_type.as_str() {
        "link" => {
            out.push_str("<a");
            for key in ["href", "target", "rel"] {
                if let Some(v) = m
                    .attrs
                    .as_ref()
                    .and_then(|a| a.get(key))
                    .and_then(attr_string)
                {
                    write_attr(key, &v, out);
                }
            }
            out.push('>');
        }
        other => match builtin_mark_tag(other) {
            Some(tag) => {
                out.push('<');
                out.push_str(tag);
                out.push('>');
            }
            None => {
                if opts.unknown_mark == UnknownMarkPolicy::DataMarkSpan {
                    out.push_str("<span data-mark=\"");
                    escape_attr(other, out);
                    out.push_str("\">");
                }
            }
        },
    }
}

fn close_mark(m: &Mark, opts: &HtmlOptions, out: &mut String) {
    if let Some(tag) = opts.mark_tags.get(&m.mark_type) {
        out.push_str("</");
        out.push_str(tag);
        out.push('>');
        return;
    }
    let tag = match builtin_mark_tag(&m.mark_type) {
        Some(tag) => tag,
        None => match opts.unknown_mark {
            UnknownMarkPolicy::DataMarkSpan => "span",
            _ => return,
        },
    };
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

// ---- attribute / escape helpers -----------------------------------------

fn write_text_align(n: &Node, opts: &HtmlOptions, out: &mut String) {
    if !opts.text_align {
        return;
    }
    if let Some(Value::String(a)) = n.attr("textAlign") {
        // Whitelist known keywords: emitting an arbitrary value into a `style`
        // attribute would allow CSS injection (`;color:red`) despite escaping.
        if matches!(
            a.as_str(),
            "left" | "right" | "center" | "justify" | "start" | "end"
        ) {
            out.push_str(" style=\"text-align:");
            out.push_str(a);
            out.push('"');
        }
    }
}

fn void(out: &mut String, tag: &str, opts: &HtmlOptions) {
    out.push('<');
    out.push_str(tag);
    void_close(out, opts);
}

fn void_close(out: &mut String, opts: &HtmlOptions) {
    match opts.self_closing {
        SelfClosingStyle::Html5 => out.push('>'),
        SelfClosingStyle::Xhtml => out.push_str("/>"),
    }
}

/// Render a JSON scalar as an attribute value; `None` for null/array/object.
fn attr_string(v: &Value) -> Option<Cow<'_, str>> {
    match v {
        Value::String(s) => Some(Cow::Borrowed(s)),
        Value::Bool(b) => Some(Cow::Owned(b.to_string())),
        Value::Number(n) => Some(Cow::Owned(n.to_string())),
        _ => None,
    }
}

fn valid_attr_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':'))
}

/// Emit a node's remaining attrs (skipping `consumed` keys and invalid names).
fn spread(n: &Node, consumed: &[&str], out: &mut String) {
    if let Some(attrs) = &n.attrs {
        for (k, v) in attrs {
            if consumed.contains(&k.as_str()) || !valid_attr_name(k) {
                continue;
            }
            if let Some(s) = attr_string(v) {
                write_attr(k, &s, out);
            }
        }
    }
}

fn write_attr(name: &str, value: &str, out: &mut String) {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    escape_attr(value, out);
    out.push('"');
}

fn escape_text(s: &str, out: &mut String) {
    escape_into(s, out, false);
}

fn escape_attr(s: &str, out: &mut String) {
    escape_into(s, out, true);
}

/// Escape `& < >` (and `"` when `quote`), pushing clean runs in bulk.
fn escape_into(s: &str, out: &mut String, quote: bool) {
    let mut last = 0;
    for (i, b) in s.bytes().enumerate() {
        let rep = match b {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' if quote => "&quot;",
            _ => continue,
        };
        out.push_str(&s[last..i]);
        out.push_str(rep);
        last = i + 1;
    }
    out.push_str(&s[last..]);
}
