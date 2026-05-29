//! Traversal and querying via predicate closures.
//!
//! All traversal is depth-first pre-order (a node is visited before its
//! children). Iterators are stack-based (no recursion) to stay cheap and avoid
//! blowing the call stack on deep documents.

use crate::node::Node;

impl Node {
    /// Visit `self` and every descendant, pre-order.
    ///
    /// ```
    /// use tiptap_rusty_parser::Document;
    /// let doc = Document::from_json_str(r#"{"type":"doc","content":[{"type":"paragraph"}]}"#).unwrap();
    /// let mut n = 0;
    /// doc.root().walk(&mut |_| n += 1);
    /// assert_eq!(n, 2);
    /// ```
    pub fn walk(&self, f: &mut impl FnMut(&Node)) {
        f(self);
        if let Some(children) = &self.content {
            for child in children {
                child.walk(f);
            }
        }
    }

    /// Mutable pre-order visit of `self` and every descendant.
    pub fn walk_mut(&mut self, f: &mut impl FnMut(&mut Node)) {
        f(self);
        if let Some(children) = &mut self.content {
            for child in children {
                child.walk_mut(f);
            }
        }
    }

    /// First node (incl. `self`) matching `pred`, pre-order.
    pub fn find(&self, mut pred: impl FnMut(&Node) -> bool) -> Option<&Node> {
        self.descendants().find(|n| pred(n))
    }

    /// Mutable variant of [`Node::find`].
    pub fn find_mut(&mut self, mut pred: impl FnMut(&Node) -> bool) -> Option<&mut Node> {
        if pred(self) {
            return Some(self);
        }
        let children = self.content.as_mut()?;
        for child in children {
            if let Some(found) = child.find_mut(&mut pred) {
                return Some(found);
            }
        }
        None
    }

    /// All nodes (incl. `self`) matching `pred`, pre-order.
    pub fn find_all(&self, mut pred: impl FnMut(&Node) -> bool) -> Vec<&Node> {
        self.descendants().filter(|n| pred(n)).collect()
    }

    /// Mutable variant of [`Node::find_all`]. Collects matching `&mut Node`.
    pub fn find_all_mut(&mut self, pred: &mut impl FnMut(&Node) -> bool) -> Vec<&mut Node> {
        let mut out = Vec::new();
        self.collect_mut(pred, &mut out);
        out
    }

    fn collect_mut<'a>(
        &'a mut self,
        pred: &mut impl FnMut(&Node) -> bool,
        out: &mut Vec<&'a mut Node>,
    ) {
        let matched = pred(self);
        // Split borrow: push self, then recurse into children.
        let children_ptr = self.content.as_mut().map(|c| c as *mut Vec<Node>);
        if matched {
            out.push(self);
        }
        // SAFETY: `content` is disjoint from the `&mut Node` pushed above
        // (different field), and we never alias the same node twice.
        if let Some(ptr) = children_ptr {
            let children = unsafe { &mut *ptr };
            for child in children {
                child.collect_mut(pred, out);
            }
        }
    }

    /// Lazy pre-order iterator over `self` and all descendants.
    #[inline]
    pub fn descendants(&self) -> Descendants<'_> {
        Descendants { stack: vec![self] }
    }
}

/// Pre-order descendant iterator. See [`Node::descendants`].
pub struct Descendants<'a> {
    stack: Vec<&'a Node>,
}

impl<'a> Iterator for Descendants<'a> {
    type Item = &'a Node;

    fn next(&mut self) -> Option<&'a Node> {
        let node = self.stack.pop()?;
        if let Some(children) = &node.content {
            // Push in reverse so children pop in document order.
            self.stack.extend(children.iter().rev());
        }
        Some(node)
    }
}
