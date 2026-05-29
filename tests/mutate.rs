use serde_json::json;
use tiptap_rusty_parser::{doc, Mark, Node};

#[test]
fn mark_add_remove_toggle() {
    let mut t = Node::text("hi");
    assert!(t.add_mark(Mark::new("bold")));
    assert!(!t.add_mark(Mark::new("bold"))); // dedup
    assert!(t.has_mark("bold"));
    assert_eq!(t.remove_mark("bold"), 1);
    assert!(!t.has_mark("bold"));
    assert!(t.marks.is_none()); // cleared when empty

    assert!(t.toggle_mark(Mark::new("italic")));
    assert!(t.has_mark("italic"));
    assert!(!t.toggle_mark(Mark::new("italic")));
    assert!(!t.has_mark("italic"));
}

#[test]
fn mark_attr() {
    let mut t = Node::text("link");
    t.add_mark(Mark::new("link"));
    assert!(t.set_mark_attr("link", "href", "https://x.dev"));
    assert_eq!(
        t.get_mark("link")
            .unwrap()
            .attrs
            .as_ref()
            .unwrap()
            .get("href"),
        Some(&json!("https://x.dev"))
    );
    assert!(!t.set_mark_attr("missing", "k", "v"));
}

#[test]
fn attrs() {
    let mut n = Node::element("heading");
    assert_eq!(n.set_attr("level", 2), None);
    assert_eq!(n.attr("level"), Some(&json!(2)));
    assert_eq!(n.set_attr("level", 3), Some(json!(2)));
    assert_eq!(n.remove_attr("level"), Some(json!(3)));
    assert!(n.attrs.is_none());
}

#[test]
fn children_ops() {
    let mut p = Node::element("paragraph");
    p.push_child(Node::text("a"));
    p.push_child(Node::text("c"));
    p.insert_child(1, Node::text("b"));
    assert_eq!(p.child_count(), 3);
    assert_eq!(p.child(1).unwrap().get_text(), Some("b"));

    let old = p.replace_child(0, Node::text("A")).unwrap();
    assert_eq!(old.get_text(), Some("a"));

    let removed = p.remove_child(2).unwrap();
    assert_eq!(removed.get_text(), Some("c"));
    assert_eq!(p.child_count(), 2);

    p.retain_children(|c| c.get_text() == Some("b"));
    assert_eq!(p.child_count(), 1);

    p.clear_children();
    assert!(p.content.is_none());
}

#[test]
fn replace_all_bulk() {
    let mut d = doc([
        Node::element("paragraph").with_text("x"),
        Node::element("paragraph").with_text("y"),
    ]);
    let n = d.replace_all(
        |node| node.node_type.as_deref() == Some("text"),
        |node| {
            node.add_mark(Mark::new("bold"));
        },
    );
    assert_eq!(n, 2);
    assert!(d
        .find_all(|node| node.node_type.as_deref() == Some("text"))
        .iter()
        .all(|t| t.has_mark("bold")));
}

#[test]
fn builder_chains() {
    let p = Node::element("paragraph")
        .with_attr("textAlign", "center")
        .with_mark(Mark::new("bold"))
        .with_text("hello")
        .with_child(Node::text("world"));
    assert_eq!(p.attr("textAlign"), Some(&json!("center")));
    assert_eq!(p.child_count(), 2);
    assert!(p.has_mark("bold"));
}
