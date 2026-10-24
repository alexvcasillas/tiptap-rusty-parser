//! Core data model: [`Node`] and [`Mark`].
//!
//! Mirrors Tiptap's `JSONContent` interface. Schema-agnostic: any node/mark
//! `type` is accepted and unknown JSON fields are preserved in `extra` for
//! faithful roundtrip.

use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fmt;

/// A single Tiptap/ProseMirror node.
///
/// All structural fields are optional to faithfully distinguish "missing" from
/// "empty" on roundtrip. Unknown top-level keys land in [`Node::extra`].
///
/// Serde: serialized as a JSON object with keys in the order `type`, `attrs`,
/// `content`, `marks`, `text` (each only when `Some`), then `extra` in its own
/// order. On deserialize, a `null` known field reads as `None`, a repeated
/// known field is a `duplicate field` error, and a repeated unknown key keeps
/// its first position with the last value.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Node {
    /// Node type, e.g. `"doc"`, `"paragraph"`, `"text"`.
    pub node_type: Option<String>,

    /// Node attributes.
    pub attrs: Option<Map<String, Value>>,

    /// Child nodes.
    pub content: Option<Vec<Node>>,

    /// Marks applied to this node (typically text nodes).
    pub marks: Option<Vec<Mark>>,

    /// Text payload (text nodes only).
    pub text: Option<String>,

    /// Any unknown/extra top-level fields, preserved verbatim.
    pub extra: Map<String, Value>,
}

/// A mark applied to a node (e.g. `bold`, `italic`, `link`).
///
/// Serde: `type` (required, string), then `attrs` (when `Some`), then `extra`;
/// same `null`/duplicate rules as [`Node`].
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    /// Mark type, e.g. `"bold"`.
    pub mark_type: String,

    /// Mark attributes (e.g. `href` for a link).
    pub attrs: Option<Map<String, Value>>,

    /// Any unknown/extra fields, preserved verbatim.
    pub extra: Map<String, Value>,
}

impl Mark {
    /// Create a mark of `mark_type` with no attrs.
    ///
    /// ```
    /// use tiptap_rusty_parser::Mark;
    /// let m = Mark::new("bold");
    /// assert_eq!(m.mark_type, "bold");
    /// ```
    #[inline]
    pub fn new(mark_type: impl Into<String>) -> Self {
        Mark {
            mark_type: mark_type.into(),
            attrs: None,
            extra: Map::new(),
        }
    }

    /// Builder: set one attr.
    #[inline]
    pub fn attr(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.attrs
            .get_or_insert_with(Map::new)
            .insert(key.into(), value.into());
        self
    }
}

// Hand-written serde: one pass over the map, known keys straight into typed
// fields, the rest into `extra`. Avoids `#[serde(flatten)]`, which buffers
// every unknown entry through serde's private `Content` tree.

/// Object key: a known field (matched without allocating) or an unknown key.
enum Key {
    Type,
    Attrs,
    Content,
    Marks,
    Text,
    Other(String),
}

impl Key {
    #[inline]
    fn known(s: &str) -> Option<Key> {
        Some(match s {
            "type" => Key::Type,
            "attrs" => Key::Attrs,
            "content" => Key::Content,
            "marks" => Key::Marks,
            "text" => Key::Text,
            _ => return None,
        })
    }
}

struct KeyVisitor;

impl<'de> Visitor<'de> for KeyVisitor {
    type Value = Key;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("field identifier")
    }

    #[inline]
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Key, E> {
        Ok(Key::known(v).unwrap_or_else(|| Key::Other(v.to_owned())))
    }

    #[inline]
    fn visit_string<E: de::Error>(self, v: String) -> Result<Key, E> {
        Ok(Key::known(&v).unwrap_or(Key::Other(v)))
    }

    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Key, E> {
        match std::str::from_utf8(v) {
            Ok(s) => self.visit_str(s),
            Err(_) => Err(E::invalid_value(de::Unexpected::Bytes(v), &self)),
        }
    }
}

impl<'de> Deserialize<'de> for Key {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Key, D::Error> {
        d.deserialize_identifier(KeyVisitor)
    }
}

/// Error on a repeated known field (matches the derive's behaviour).
#[inline]
fn once<E: de::Error>(seen: &mut u8, bit: u8, name: &'static str) -> Result<(), E> {
    if *seen & bit != 0 {
        return Err(E::duplicate_field(name));
    }
    *seen |= bit;
    Ok(())
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Node, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Node;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("struct Node")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
                let mut n = Node::default();
                let mut seen = 0u8;
                while let Some(key) = map.next_key::<Key>()? {
                    match key {
                        Key::Type => {
                            once(&mut seen, 1, "type")?;
                            n.node_type = map.next_value()?;
                        }
                        Key::Attrs => {
                            once(&mut seen, 2, "attrs")?;
                            n.attrs = map.next_value()?;
                        }
                        Key::Content => {
                            once(&mut seen, 4, "content")?;
                            n.content = map.next_value()?;
                        }
                        Key::Marks => {
                            once(&mut seen, 8, "marks")?;
                            n.marks = map.next_value()?;
                        }
                        Key::Text => {
                            once(&mut seen, 16, "text")?;
                            n.text = map.next_value()?;
                        }
                        Key::Other(k) => {
                            let v: Value = map.next_value()?;
                            n.extra.insert(k, v);
                        }
                    }
                }
                Ok(n)
            }
        }
        d.deserialize_map(V)
    }
}

impl Serialize for Node {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let len = self.node_type.is_some() as usize
            + self.attrs.is_some() as usize
            + self.content.is_some() as usize
            + self.marks.is_some() as usize
            + self.text.is_some() as usize
            + self.extra.len();
        let mut m = s.serialize_map(Some(len))?;
        if let Some(t) = &self.node_type {
            m.serialize_entry("type", t)?;
        }
        if let Some(a) = &self.attrs {
            m.serialize_entry("attrs", a)?;
        }
        if let Some(c) = &self.content {
            m.serialize_entry("content", c)?;
        }
        if let Some(mk) = &self.marks {
            m.serialize_entry("marks", mk)?;
        }
        if let Some(t) = &self.text {
            m.serialize_entry("text", t)?;
        }
        for (k, v) in &self.extra {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

impl<'de> Deserialize<'de> for Mark {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Mark, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Mark;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("struct Mark")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Mark, A::Error> {
                let mut mark_type: Option<String> = None;
                let mut attrs = None;
                let mut extra = Map::new();
                let mut seen = 0u8;
                while let Some(key) = map.next_key::<Key>()? {
                    match key {
                        Key::Type => {
                            once(&mut seen, 1, "type")?;
                            mark_type = Some(map.next_value()?);
                        }
                        Key::Attrs => {
                            once(&mut seen, 2, "attrs")?;
                            attrs = map.next_value()?;
                        }
                        Key::Content => extra_entry(&mut map, &mut extra, "content")?,
                        Key::Marks => extra_entry(&mut map, &mut extra, "marks")?,
                        Key::Text => extra_entry(&mut map, &mut extra, "text")?,
                        Key::Other(k) => {
                            let v: Value = map.next_value()?;
                            extra.insert(k, v);
                        }
                    }
                }
                let mark_type = mark_type.ok_or_else(|| de::Error::missing_field("type"))?;
                Ok(Mark {
                    mark_type,
                    attrs,
                    extra,
                })
            }
        }
        d.deserialize_map(V)
    }
}

/// `Mark` has no `content`/`marks`/`text` fields: those keys are unknown there.
#[inline]
fn extra_entry<'de, A: MapAccess<'de>>(
    map: &mut A,
    extra: &mut Map<String, Value>,
    key: &str,
) -> Result<(), A::Error> {
    let v: Value = map.next_value()?;
    extra.insert(key.to_owned(), v);
    Ok(())
}

impl Serialize for Mark {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let len = 1 + self.attrs.is_some() as usize + self.extra.len();
        let mut m = s.serialize_map(Some(len))?;
        m.serialize_entry("type", &self.mark_type)?;
        if let Some(a) = &self.attrs {
            m.serialize_entry("attrs", a)?;
        }
        for (k, v) in &self.extra {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}
