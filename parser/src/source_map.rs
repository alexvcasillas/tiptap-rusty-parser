//! [`SourceMap`]: byte spans of every node object in a document's JSON text.
//!
//! Built by a single-pass, allocation-light scanner over the text (no
//! `Value` tree), so it is exact for whatever produced the text (pretty,
//! compact, hand-written) and for unknown node types and fields. A node is an
//! object at the root or an object element of a node's `"content"` array;
//! paths are child indices into `content`, the same addressing as
//! [`Node::node_at`](crate::Node::node_at).

use std::fmt;
use std::ops::Range;

use crate::node::Node;

/// Nesting deeper than this (objects/arrays) is refused instead of recursing.
const MAX_DEPTH: usize = 1024;

#[derive(Debug, Clone, Copy)]
struct Entry {
    start: usize,
    end: usize,
    /// Index in `entries` of the parent; `u32::MAX` for the root.
    parent: u32,
    /// Child index within the parent's `content`.
    index: u32,
    /// This node's children: `kids[kid_start..kid_start + kid_len]`.
    kid_start: u32,
    kid_len: u32,
}

/// Why a [`SourceMap`] could not be built: the text is not a JSON node tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMapError {
    /// Byte offset where scanning stopped.
    pub offset: usize,
    /// What was expected there.
    pub message: &'static str,
}

impl fmt::Display for SourceMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for SourceMapError {}

/// Byte spans of every node object in a JSON text, addressable by index path
/// and by offset.
///
/// Build cost is O(text); [`span`](Self::span) is O(depth) and
/// [`path_at`](Self::path_at) is O(log nodes + depth).
///
/// ```
/// use tiptap_rusty_parser::{Document, SourceMap};
///
/// let doc = Document::from_json_str(
///     r#"{"type":"doc","content":[{"type":"callout","x":1,"content":[{"type":"text","text":"hi"}]}]}"#,
/// ).unwrap();
/// let (text, map) = SourceMap::pretty(doc.root());
/// let span = map.span(&[0, 0]).unwrap();
/// assert!(text[span.clone()].contains("\"hi\""));
/// // Any byte inside the text node resolves back to it.
/// assert_eq!(map.path_at(span.start + 3), Some(vec![0, 0]));
/// ```
#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    /// Pre-order, so `start` is strictly increasing.
    entries: Vec<Entry>,
    kids: Vec<u32>,
}

impl SourceMap {
    /// Scan `text` (a JSON node object, any formatting).
    pub fn build(text: &str) -> Result<SourceMap, SourceMapError> {
        let mut s = Scanner {
            b: text.as_bytes(),
            i: 0,
            map: SourceMap::default(),
            stack: Vec::new(),
        };
        s.ws();
        if s.peek() != Some(b'{') {
            return Err(s.err("expected a JSON object"));
        }
        s.node(u32::MAX, 0, 0)?;
        s.ws();
        if s.i != s.b.len() {
            return Err(s.err("trailing characters"));
        }
        Ok(s.map)
    }

    /// Serialize `node` with `serde_json` pretty formatting (the same text as
    /// [`Document::to_string_pretty`](crate::Document::to_string_pretty)) and
    /// map it.
    pub fn pretty(node: &Node) -> (String, SourceMap) {
        let text = serde_json::to_string_pretty(node).expect("Node serializes");
        let map = SourceMap::build(&text).expect("serde_json output is a node tree");
        (text, map)
    }

    /// Number of node objects.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no node was mapped (only for `SourceMap::default()`).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn entry_at(&self, path: &[usize]) -> Option<usize> {
        let mut e = 0usize;
        self.entries.first()?;
        for &i in path {
            let n = &self.entries[e];
            if i >= n.kid_len as usize {
                return None;
            }
            e = self.kids[n.kid_start as usize + i] as usize;
        }
        Some(e)
    }

    /// Byte range of the node object at `path` (`{` through `}`), or `None`
    /// when the path does not resolve.
    pub fn span(&self, path: &[usize]) -> Option<Range<usize>> {
        self.entry_at(path).map(|e| {
            let n = &self.entries[e];
            n.start..n.end
        })
    }

    /// Number of children of the node at `path`.
    pub fn child_count(&self, path: &[usize]) -> Option<usize> {
        self.entry_at(path)
            .map(|e| self.entries[e].kid_len as usize)
    }

    /// Path of the deepest node whose span contains byte `offset`, or `None`
    /// when `offset` lies outside the root object.
    pub fn path_at(&self, offset: usize) -> Option<Vec<usize>> {
        // Last node starting at or before `offset`; then climb to the first
        // ancestor-or-self that still contains it.
        let k = self.entries.partition_point(|e| e.start <= offset);
        let mut e = k.checked_sub(1)?;
        loop {
            let n = &self.entries[e];
            if offset < n.end {
                break;
            }
            if n.parent == u32::MAX {
                return None;
            }
            e = n.parent as usize;
        }
        let mut path = Vec::new();
        while self.entries[e].parent != u32::MAX {
            path.push(self.entries[e].index as usize);
            e = self.entries[e].parent as usize;
        }
        path.reverse();
        Some(path)
    }
}

struct Scanner<'a> {
    b: &'a [u8],
    i: usize,
    map: SourceMap,
    /// Child entry ids of the nodes being scanned, flattened.
    stack: Vec<u32>,
}

impl Scanner<'_> {
    #[inline]
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn err(&self, message: &'static str) -> SourceMapError {
        SourceMapError {
            offset: self.i,
            message,
        }
    }

    #[inline]
    fn ws(&mut self) {
        while let Some(b' ' | b'\n' | b'\r' | b'\t') = self.peek() {
            self.i += 1;
        }
    }

    fn expect(&mut self, c: u8, message: &'static str) -> Result<(), SourceMapError> {
        self.ws();
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(self.err(message))
        }
    }

    /// At `"`: skip the string; returns its raw body and whether it has escapes.
    fn string(&mut self) -> Result<(usize, usize, bool), SourceMapError> {
        self.i += 1;
        let start = self.i;
        let mut escaped = false;
        loop {
            // Fast path: jump to the next quote or backslash.
            match self.b[self.i..]
                .iter()
                .position(|&c| c == b'"' || c == b'\\')
            {
                None => {
                    self.i = self.b.len();
                    return Err(self.err("unterminated string"));
                }
                Some(k) => {
                    self.i += k;
                    if self.b[self.i] == b'"' {
                        let end = self.i;
                        self.i += 1;
                        return Ok((start, end, escaped));
                    }
                    escaped = true;
                    self.i += 2;
                    if self.i > self.b.len() {
                        self.i = self.b.len();
                        return Err(self.err("unterminated string"));
                    }
                }
            }
        }
    }

    fn is_content_key(&self, (s, e, escaped): (usize, usize, bool)) -> bool {
        let raw = &self.b[s..e];
        if !escaped {
            return raw == b"content";
        }
        // Rare: an escaped key; decode it exactly as serde would.
        let quoted = &self.b[s - 1..e + 1];
        serde_json::from_slice::<String>(quoted).is_ok_and(|k| k == "content")
    }

    /// Any JSON value (not a node).
    fn value(&mut self, depth: usize) -> Result<(), SourceMapError> {
        if depth > MAX_DEPTH {
            return Err(self.err("nesting too deep"));
        }
        self.ws();
        match self.peek() {
            Some(b'"') => self.string().map(|_| ()),
            Some(b'{') => {
                self.i += 1;
                self.ws();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                    return Ok(());
                }
                loop {
                    self.ws();
                    if self.peek() != Some(b'"') {
                        return Err(self.err("expected a key"));
                    }
                    self.string()?;
                    self.expect(b':', "expected `:`")?;
                    self.value(depth + 1)?;
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(());
                        }
                        _ => return Err(self.err("expected `,` or `}`")),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                self.ws();
                if self.peek() == Some(b']') {
                    self.i += 1;
                    return Ok(());
                }
                loop {
                    self.value(depth + 1)?;
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(());
                        }
                        _ => return Err(self.err("expected `,` or `]`")),
                    }
                }
            }
            Some(b'-' | b'0'..=b'9' | b't' | b'f' | b'n') => {
                let start = self.i;
                while let Some(c) = self.peek() {
                    if matches!(c, b',' | b'}' | b']' | b' ' | b'\n' | b'\r' | b'\t') {
                        break;
                    }
                    self.i += 1;
                }
                (self.i > start)
                    .then_some(())
                    .ok_or_else(|| self.err("expected a value"))
            }
            _ => Err(self.err("expected a value")),
        }
    }

    /// At `{` of a node: record it and its `content` children.
    fn node(&mut self, parent: u32, index: u32, depth: usize) -> Result<(), SourceMapError> {
        if depth > MAX_DEPTH {
            return Err(self.err("nesting too deep"));
        }
        let id = self.map.entries.len() as u32;
        self.map.entries.push(Entry {
            start: self.i,
            end: 0,
            parent,
            index,
            kid_start: 0,
            kid_len: 0,
        });
        let kids_from = self.stack.len();
        self.i += 1;
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
        } else {
            loop {
                self.ws();
                if self.peek() != Some(b'"') {
                    return Err(self.err("expected a key"));
                }
                let key = self.string()?;
                self.expect(b':', "expected `:`")?;
                self.ws();
                if self.peek() == Some(b'[') && self.is_content_key(key) {
                    self.content(id, depth)?;
                } else {
                    self.value(depth + 1)?;
                }
                self.ws();
                match self.peek() {
                    Some(b',') => self.i += 1,
                    Some(b'}') => {
                        self.i += 1;
                        break;
                    }
                    _ => return Err(self.err("expected `,` or `}`")),
                }
            }
        }
        let kid_start = self.map.kids.len() as u32;
        let kid_len = (self.stack.len() - kids_from) as u32;
        self.map.kids.extend(self.stack.drain(kids_from..));
        let e = &mut self.map.entries[id as usize];
        e.end = self.i;
        e.kid_start = kid_start;
        e.kid_len = kid_len;
        Ok(())
    }

    /// At `[` of a node's `content`: every element must be a node object.
    fn content(&mut self, parent: u32, depth: usize) -> Result<(), SourceMapError> {
        self.i += 1;
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(());
        }
        let mut index = 0u32;
        loop {
            self.ws();
            if self.peek() != Some(b'{') {
                return Err(self.err("expected a node object in `content`"));
            }
            self.stack.push(self.map.entries.len() as u32);
            self.node(parent, index, depth + 1)?;
            index += 1;
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(());
                }
                _ => return Err(self.err("expected `,` or `]`")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;

    const SRC: &str = r#"{"type":"doc","content":[
        {"type":"heading","attrs":{"level":1,"content":[1,2]},"content":[{"type":"text","text":"T \"q\" }"}]},
        {"type":"callout","weird":{"content":[{"a":1}]},"content":[]},
        {"type":"paragraph","content":[{"type":"text","marks":[{"type":"bold"}],"text":"a"},{"type":"hardBreak"}]}
    ],"extra":[true,null,-1.5e3]}"#;

    #[test]
    fn spans_match_every_node_exactly() {
        let doc = Document::from_json_str(SRC).unwrap();
        let map = SourceMap::build(SRC).unwrap();
        let mut count = 0;
        fn go(n: &Node, path: &mut Vec<usize>, map: &SourceMap, src: &str, count: &mut usize) {
            *count += 1;
            let span = map.span(path).unwrap();
            let parsed: Node = serde_json::from_str(&src[span]).unwrap();
            assert_eq!(&parsed, n, "path {path:?}");
            assert_eq!(map.child_count(path), Some(n.child_count()));
            for (i, c) in n.children().iter().enumerate() {
                path.push(i);
                go(c, path, map, src, count);
                path.pop();
            }
        }
        go(doc.root(), &mut Vec::new(), &map, SRC, &mut count);
        assert_eq!(count, map.len());
        assert_eq!(map.span(&[0, 1]), None);
        assert_eq!(map.span(&[9]), None);
    }

    #[test]
    fn content_inside_attrs_or_unknown_fields_is_not_a_node() {
        let map = SourceMap::build(SRC).unwrap();
        assert_eq!(map.child_count(&[0]), Some(1));
        assert_eq!(map.child_count(&[1]), Some(0));
        assert_eq!(map.len(), 7);
    }

    #[test]
    fn path_at_finds_deepest() {
        let map = SourceMap::build(SRC).unwrap();
        let text = SRC.find("\"a\"}").unwrap();
        assert_eq!(map.path_at(text), Some(vec![2, 0]));
        let hb = SRC.find("hardBreak").unwrap();
        assert_eq!(map.path_at(hb), Some(vec![2, 1]));
        // Between children: the parent.
        let comma = map.span(&[2, 0]).unwrap().end;
        assert_eq!(map.path_at(comma), Some(vec![2]));
        assert_eq!(map.path_at(SRC.find("extra").unwrap()), Some(vec![]));
        assert_eq!(map.path_at(0), Some(vec![]));
        assert_eq!(map.path_at(SRC.len()), None);
        let s = map.span(&[1]).unwrap();
        assert_eq!(map.path_at(s.start), Some(vec![1]));
        assert_eq!(map.path_at(s.end - 1), Some(vec![1]));
    }

    #[test]
    fn pretty_matches_document_and_roundtrips_paths() {
        let doc = Document::from_json_str(SRC).unwrap();
        let (text, map) = SourceMap::pretty(doc.root());
        assert_eq!(text, doc.to_string_pretty().unwrap());
        for path in [vec![], vec![0], vec![0, 0], vec![1], vec![2, 1]] {
            let s = map.span(&path).unwrap();
            assert_eq!(map.path_at(s.start), Some(path.clone()));
            assert!(text[s].starts_with('{'));
        }
    }

    #[test]
    fn escaped_content_key() {
        let src = r#"{"type":"doc","content":[{"type":"p"}]}"#;
        assert_eq!(
            Document::from_json_str(src).unwrap().child_count(),
            SourceMap::build(src).unwrap().child_count(&[]).unwrap()
        );
    }

    #[test]
    fn errors() {
        for bad in [
            "",
            "[]",
            "{",
            r#"{"a":}"#,
            r#"{"a":"x}"#,
            r#"{"content":[1]}"#,
            r#"{"a":1} x"#,
        ] {
            assert!(SourceMap::build(bad).is_err(), "{bad}");
        }
        let deep = format!("{}{}", "[".repeat(5000), "]".repeat(5000));
        assert!(SourceMap::build(&format!(r#"{{"a":{deep}}}"#)).is_err());
    }
}
