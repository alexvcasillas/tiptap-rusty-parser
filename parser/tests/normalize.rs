use serde_json::json;
use tiptap_rusty_parser::{doc, Mark, Node, NormalizeOptions};

/// A paragraph wrapping the given inline children.
fn para(children: impl IntoIterator<Item = Node>) -> Node {
    Node::element("paragraph").with_children(children)
}

#[test]
fn merges_adjacent_same_mark_text() {
    let mut p = para([Node::text("foo"), Node::text("bar"), Node::text("baz")]);
    p.normalize();
    assert_eq!(p.child_count(), 1);
    assert_eq!(p.child(0).unwrap().get_text(), Some("foobarbaz"));
}

#[test]
fn does_not_merge_differing_marks() {
    let mut p = para([
        Node::text_with_marks("a", [Mark::new("bold")]),
        Node::text_with_marks("b", [Mark::new("italic")]),
    ]);
    p.normalize();
    assert_eq!(p.child_count(), 2);
}

#[test]
fn merges_equal_marks_keeps_them() {
    let mut p = para([
        Node::text_with_marks("a", [Mark::new("bold")]),
        Node::text_with_marks("b", [Mark::new("bold")]),
    ]);
    p.normalize();
    assert_eq!(p.child_count(), 1);
    assert_eq!(p.child(0).unwrap().get_text(), Some("ab"));
    assert!(p.child(0).unwrap().has_mark("bold"));
}

#[test]
fn does_not_merge_differing_mark_attrs() {
    let mut p = para([
        Node::text_with_marks("x", [Mark::new("link").attr("href", "a")]),
        Node::text_with_marks("y", [Mark::new("link").attr("href", "b")]),
    ]);
    p.normalize();
    assert_eq!(p.child_count(), 2);
}

#[test]
fn drops_empty_text_by_default() {
    let mut p = para([Node::text(""), Node::text("hi"), Node::text("")]);
    p.normalize();
    assert_eq!(p.child_count(), 1);
    assert_eq!(p.child(0).unwrap().get_text(), Some("hi"));
}

#[test]
fn empty_text_between_breaks_no_merge_after_removal() {
    // Empty middle node removed first, then the two reals merge.
    let mut p = para([Node::text("a"), Node::text(""), Node::text("b")]);
    p.normalize();
    assert_eq!(p.child_count(), 1);
    assert_eq!(p.child(0).unwrap().get_text(), Some("ab"));
}

#[test]
fn keeps_empty_paragraph_by_default() {
    let mut d = doc([para([])]);
    d.normalize();
    assert_eq!(d.child_count(), 1); // empty paragraph preserved
    assert_eq!(d.child(0).unwrap().child_count(), 0);
}

#[test]
fn remove_empty_nodes_opt_in_drops_empty_paragraph() {
    let mut d = doc([para([Node::text("keep")]), para([]), para([Node::text("")])]);
    let opts = NormalizeOptions {
        remove_empty_nodes: true,
        ..Default::default()
    };
    d.normalize_with(&opts);
    // Second (empty) and third (only-empty-text -> empty) paragraphs dropped.
    assert_eq!(d.child_count(), 1);
    assert_eq!(d.child(0).unwrap().text_content(), "keep");
}

#[test]
fn nested_propagation() {
    let mut d =
        doc(
            [
                Node::element("blockquote")
                    .with_children([para([Node::text("a"), Node::text("b")])]),
            ],
        );
    d.normalize();
    let inner = d.child(0).unwrap().child(0).unwrap();
    assert_eq!(inner.child_count(), 1);
    assert_eq!(inner.child(0).unwrap().get_text(), Some("ab"));
}

#[test]
fn idempotent() {
    let mut once = para([
        Node::text(""),
        Node::text("a"),
        Node::text("b"),
        Node::text_with_marks("c", [Mark::new("bold")]),
    ]);
    once.normalize();
    let mut twice = once.clone();
    twice.normalize();
    assert_eq!(once, twice);
}

#[test]
fn preserves_mixed_sibling_order() {
    let mut p = para([
        Node::text("a"),
        Node::element("hardBreak"),
        Node::text("b"),
        Node::text("c"),
    ]);
    p.normalize();
    // text a | hardBreak | "bc"
    assert_eq!(p.child_count(), 3);
    assert_eq!(p.child(0).unwrap().get_text(), Some("a"));
    assert_eq!(p.child(1).unwrap().node_type.as_deref(), Some("hardBreak"));
    assert_eq!(p.child(2).unwrap().get_text(), Some("bc"));
}

#[test]
fn merge_only_off() {
    let mut p = para([Node::text("a"), Node::text("b")]);
    let opts = NormalizeOptions {
        merge_adjacent_text: false,
        ..Default::default()
    };
    p.normalize_with(&opts);
    assert_eq!(p.child_count(), 2);
}

#[test]
fn keep_empty_text_when_disabled() {
    let mut p = para([Node::text(""), Node::text("hi")]);
    let opts = NormalizeOptions {
        remove_empty_text: false,
        merge_adjacent_text: false,
        ..Default::default()
    };
    p.normalize_with(&opts);
    assert_eq!(p.child_count(), 2);
}

#[test]
fn does_not_touch_absent_content() {
    // A leaf node with no content stays content: None (faithful), even with
    // remove_empty_nodes on.
    let mut d = doc([Node::element("horizontalRule")]);
    let before = d.clone();
    let opts = NormalizeOptions {
        remove_empty_nodes: true,
        ..Default::default()
    };
    d.normalize_with(&opts);
    assert_eq!(d, before);
    assert!(d.child(0).unwrap().content.is_none());
}

#[test]
fn merge_preserves_extra_fields() {
    // Two text nodes differing in an unknown `extra` field must NOT merge.
    let a: Node = serde_json::from_value(json!({"type":"text","text":"a","foo":1})).unwrap();
    let b: Node = serde_json::from_value(json!({"type":"text","text":"b","foo":2})).unwrap();
    let mut p = para([a, b]);
    p.normalize();
    assert_eq!(p.child_count(), 2);
}
