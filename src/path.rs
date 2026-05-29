//! Positional addressing by index path.
//!
//! A *path* is a slice of child indices from a node, root = `&[]`. So in a
//! `doc -> paragraph -> text` tree, the text node is at `&[0, 0]`.
//!
//! There are no stored parent pointers; parent/sibling navigation is expressed
//! by slicing a path: the parent of `path` is `node.node_at(&path[..path.len()-1])`.

use crate::node::Node;

impl Node {
    /// Node at `path` (relative to `self`), or `None` if any index is missing.
    ///
    /// ```
    /// use tiptap_rusty_parser::Document;
    /// let doc = Document::from_json_str(
    ///     r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"hi"}]}]}"#,
    /// ).unwrap();
    /// assert_eq!(doc.node_at(&[0, 0]).unwrap().get_text(), Some("hi"));
    /// assert!(doc.node_at(&[5]).is_none());
    /// ```
    pub fn node_at(&self, path: &[usize]) -> Option<&Node> {
        let mut cur = self;
        for &i in path {
            cur = cur.content.as_ref()?.get(i)?;
        }
        Some(cur)
    }

    /// Mutable variant of [`Node::node_at`].
    pub fn node_at_mut(&mut self, path: &[usize]) -> Option<&mut Node> {
        let mut cur = self;
        for &i in path {
            cur = cur.content.as_mut()?.get_mut(i)?;
        }
        Some(cur)
    }

    /// Path to the first node (incl. `self`) matching `pred`, pre-order.
    ///
    /// Returns `Some(vec![])` when `self` matches.
    ///
    /// ```
    /// use tiptap_rusty_parser::Document;
    /// let doc = Document::from_json_str(
    ///     r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"hi"}]}]}"#,
    /// ).unwrap();
    /// let p = doc.path_to(|n| n.node_type.as_deref() == Some("text")).unwrap();
    /// assert_eq!(p, vec![0, 0]);
    /// assert_eq!(doc.node_at(&p).unwrap().get_text(), Some("hi"));
    /// ```
    pub fn path_to(&self, mut pred: impl FnMut(&Node) -> bool) -> Option<Vec<usize>> {
        let mut path = Vec::new();
        if path_to_rec(self, &mut pred, &mut path) {
            Some(path)
        } else {
            None
        }
    }

    /// Paths to every node (incl. `self`) matching `pred`, pre-order.
    pub fn paths_to(&self, mut pred: impl FnMut(&Node) -> bool) -> Vec<Vec<usize>> {
        let mut out = Vec::new();
        let mut path = Vec::new();
        paths_to_rec(self, &mut pred, &mut path, &mut out);
        out
    }
}

fn path_to_rec(node: &Node, pred: &mut impl FnMut(&Node) -> bool, path: &mut Vec<usize>) -> bool {
    if pred(node) {
        return true;
    }
    if let Some(children) = &node.content {
        for (i, child) in children.iter().enumerate() {
            path.push(i);
            if path_to_rec(child, pred, path) {
                return true;
            }
            path.pop();
        }
    }
    false
}

fn paths_to_rec(
    node: &Node,
    pred: &mut impl FnMut(&Node) -> bool,
    path: &mut Vec<usize>,
    out: &mut Vec<Vec<usize>>,
) {
    if pred(node) {
        out.push(path.clone());
    }
    if let Some(children) = &node.content {
        for (i, child) in children.iter().enumerate() {
            path.push(i);
            paths_to_rec(child, pred, path, out);
            path.pop();
        }
    }
}
