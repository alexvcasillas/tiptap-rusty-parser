//! Text extraction utilities.

use crate::node::Node;
use unicode_segmentation::UnicodeSegmentation;

impl Node {
    /// Concatenate all descendant text, with no separators.
    ///
    /// Matches ProseMirror's `node.textContent`.
    ///
    /// ```
    /// use tiptap_rusty_parser::Document;
    /// let doc = Document::from_json_str(
    ///     r#"{"type":"doc","content":[
    ///         {"type":"paragraph","content":[{"type":"text","text":"Hello "},{"type":"text","text":"world"}]}
    ///     ]}"#,
    /// ).unwrap();
    /// assert_eq!(doc.text_content(), "Hello world");
    /// ```
    pub fn text_content(&self) -> String {
        let mut out = String::new();
        self.walk(&mut |n| {
            if let Some(t) = &n.text {
                out.push_str(t);
            }
        });
        out
    }

    /// Concatenate descendant text, inserting `sep` between adjacent block-level
    /// siblings (a node is "block-level" if it has `content` and is not a `text`
    /// node). Text within a block stays contiguous.
    ///
    /// ```
    /// use tiptap_rusty_parser::Document;
    /// let doc = Document::from_json_str(
    ///     r#"{"type":"doc","content":[
    ///         {"type":"paragraph","content":[{"type":"text","text":"Hello"}]},
    ///         {"type":"paragraph","content":[{"type":"text","text":"world"}]}
    ///     ]}"#,
    /// ).unwrap();
    /// assert_eq!(doc.text_content_with_separator("\n\n"), "Hello\n\nworld");
    /// ```
    pub fn text_content_with_separator(&self, sep: &str) -> String {
        let mut out = String::new();
        render_sep(self, sep, &mut out);
        out
    }

    /// Total number of Unicode scalar values across all descendant text.
    pub fn char_count(&self) -> usize {
        let mut n = 0;
        self.walk(&mut |node| {
            if let Some(t) = &node.text {
                n += t.chars().count();
            }
        });
        n
    }

    /// Number of words across all text, via Unicode word segmentation.
    ///
    /// Blocks are separated by a space first, so words don't merge across block
    /// boundaries. Correct for CJK and other complex scripts.
    ///
    /// ```
    /// use tiptap_rusty_parser::Document;
    /// let doc = Document::from_json_str(
    ///     r#"{"type":"doc","content":[
    ///         {"type":"paragraph","content":[{"type":"text","text":"Hello"}]},
    ///         {"type":"paragraph","content":[{"type":"text","text":"brave world"}]}
    ///     ]}"#,
    /// ).unwrap();
    /// assert_eq!(doc.word_count(), 3);
    /// ```
    pub fn word_count(&self) -> usize {
        self.text_content_with_separator(" ")
            .unicode_words()
            .count()
    }
}

fn is_block(node: &Node) -> bool {
    node.content.is_some() && node.node_type.as_deref() != Some("text")
}

fn render_sep(node: &Node, sep: &str, out: &mut String) {
    if let Some(t) = &node.text {
        out.push_str(t);
    }
    if let Some(children) = &node.content {
        let mut prev_block = false;
        for child in children {
            let block = is_block(child);
            if block && prev_block {
                out.push_str(sep);
            }
            render_sep(child, sep, out);
            prev_block = block;
        }
    }
}
