//! In-place mutation: marks, attrs, children, text, bulk transforms.

use crate::node::{Mark, Node};
use serde_json::{Map, Value};

impl Node {
    // ---- marks ----------------------------------------------------------

    /// True if a mark of `mark_type` is present.
    pub fn has_mark(&self, mark_type: &str) -> bool {
        self.marks
            .as_ref()
            .is_some_and(|m| m.iter().any(|x| x.mark_type == mark_type))
    }

    /// Reference to the first mark of `mark_type`.
    pub fn get_mark(&self, mark_type: &str) -> Option<&Mark> {
        self.marks
            .as_ref()?
            .iter()
            .find(|x| x.mark_type == mark_type)
    }

    /// Add `mark` if no mark of that type exists yet. Returns `true` if added.
    pub fn add_mark(&mut self, mark: Mark) -> bool {
        if self.has_mark(&mark.mark_type) {
            return false;
        }
        self.marks.get_or_insert_with(Vec::new).push(mark);
        true
    }

    /// Remove every mark of `mark_type`. Returns count removed.
    pub fn remove_mark(&mut self, mark_type: &str) -> usize {
        let Some(marks) = self.marks.as_mut() else {
            return 0;
        };
        let before = marks.len();
        marks.retain(|m| m.mark_type != mark_type);
        let removed = before - marks.len();
        if marks.is_empty() {
            self.marks = None;
        }
        removed
    }

    /// Toggle a mark: remove if present, else add. Returns `true` if now present.
    pub fn toggle_mark(&mut self, mark: Mark) -> bool {
        if self.has_mark(&mark.mark_type) {
            self.remove_mark(&mark.mark_type);
            false
        } else {
            self.add_mark(mark);
            true
        }
    }

    /// Set an attr on the (first) mark of `mark_type`. Returns `false` if absent.
    pub fn set_mark_attr(
        &mut self,
        mark_type: &str,
        key: impl Into<String>,
        value: impl Into<Value>,
    ) -> bool {
        let Some(marks) = self.marks.as_mut() else {
            return false;
        };
        let Some(mark) = marks.iter_mut().find(|m| m.mark_type == mark_type) else {
            return false;
        };
        mark.attrs
            .get_or_insert_with(Map::new)
            .insert(key.into(), value.into());
        true
    }

    /// Drop all marks.
    pub fn clear_marks(&mut self) {
        self.marks = None;
    }

    // ---- attrs ----------------------------------------------------------

    /// Reference to attr `key`.
    pub fn attr(&self, key: &str) -> Option<&Value> {
        self.attrs.as_ref()?.get(key)
    }

    /// Mutable access to the attrs map, creating it if absent.
    pub fn attrs_mut(&mut self) -> &mut Map<String, Value> {
        self.attrs.get_or_insert_with(Map::new)
    }

    /// Set attr `key`, returning the previous value if any.
    pub fn set_attr(&mut self, key: impl Into<String>, value: impl Into<Value>) -> Option<Value> {
        self.attrs_mut().insert(key.into(), value.into())
    }

    /// Remove attr `key`, returning its value if present.
    pub fn remove_attr(&mut self, key: &str) -> Option<Value> {
        let attrs = self.attrs.as_mut()?;
        let prev = attrs.remove(key);
        if attrs.is_empty() {
            self.attrs = None;
        }
        prev
    }

    // ---- children -------------------------------------------------------

    /// Number of direct children.
    #[inline]
    pub fn child_count(&self) -> usize {
        self.content.as_ref().map_or(0, Vec::len)
    }

    /// Direct children slice (empty if none).
    #[inline]
    pub fn children(&self) -> &[Node] {
        self.content.as_deref().unwrap_or(&[])
    }

    /// Mutable access to children, creating the vec if absent.
    #[inline]
    pub fn children_mut(&mut self) -> &mut Vec<Node> {
        self.content.get_or_insert_with(Vec::new)
    }

    /// Direct child at `i`.
    #[inline]
    pub fn child(&self, i: usize) -> Option<&Node> {
        self.content.as_ref()?.get(i)
    }

    /// Mutable direct child at `i`.
    #[inline]
    pub fn child_mut(&mut self, i: usize) -> Option<&mut Node> {
        self.content.as_mut()?.get_mut(i)
    }

    /// Append a child.
    #[inline]
    pub fn push_child(&mut self, node: Node) {
        self.children_mut().push(node);
    }

    /// Insert a child at `i` (clamped to len).
    pub fn insert_child(&mut self, i: usize, node: Node) {
        let children = self.children_mut();
        let i = i.min(children.len());
        children.insert(i, node);
    }

    /// Remove and return the child at `i`.
    pub fn remove_child(&mut self, i: usize) -> Option<Node> {
        let children = self.content.as_mut()?;
        if i >= children.len() {
            return None;
        }
        let removed = children.remove(i);
        if children.is_empty() {
            self.content = None;
        }
        Some(removed)
    }

    /// Replace the child at `i`, returning the old node.
    pub fn replace_child(&mut self, i: usize, node: Node) -> Option<Node> {
        let child = self.content.as_mut()?.get_mut(i)?;
        Some(std::mem::replace(child, node))
    }

    /// Remove all children.
    pub fn clear_children(&mut self) {
        self.content = None;
    }

    /// Retain only children matching `pred`.
    pub fn retain_children(&mut self, mut pred: impl FnMut(&Node) -> bool) {
        if let Some(children) = self.content.as_mut() {
            children.retain(|c| pred(c));
            if children.is_empty() {
                self.content = None;
            }
        }
    }

    // ---- text -----------------------------------------------------------

    /// Text payload, if any.
    #[inline]
    pub fn get_text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// Set text payload.
    #[inline]
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = Some(text.into());
    }

    // ---- bulk transform -------------------------------------------------

    /// Apply `f` to every node (incl. `self`) matching `pred`. Returns count.
    pub fn replace_all(
        &mut self,
        mut pred: impl FnMut(&Node) -> bool,
        mut f: impl FnMut(&mut Node),
    ) -> usize {
        let mut n = 0;
        self.walk_mut(&mut |node| {
            if pred(node) {
                f(node);
                n += 1;
            }
        });
        n
    }
}
