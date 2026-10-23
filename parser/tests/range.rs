use tiptap_rusty_parser::{Mark, Node, Position, Range, RangeError};

/// A paragraph wrapping the given inline children.
fn para(children: impl IntoIterator<Item = Node>) -> Node {
    Node::element("paragraph").with_children(children)
}

fn pos(child: usize, offset: usize) -> Position {
    Position::new(child, offset)
}

fn range(s: (usize, usize), e: (usize, usize)) -> Range {
    Range::new(pos(s.0, s.1), pos(e.0, e.1))
}

// ---- insert -------------------------------------------------------------

#[test]
fn insert_text_splits_and_merges_same_marks() {
    let mut p = para([Node::text("ab")]);
    p.insert_text(pos(0, 1), "X", None).unwrap();
    // "a" | "X" | "b" all unmarked -> merged into one "aXb".
    assert_eq!(p.child_count(), 1);
    assert_eq!(p.child(0).unwrap().get_text(), Some("aXb"));
}

#[test]
fn insert_text_with_marks_stays_separate() {
    let mut p = para([Node::text("ab")]);
    p.insert_text(pos(0, 1), "X", Some(&[Mark::new("bold")]))
        .unwrap();
    assert_eq!(p.text_content(), "aXb");
    // distinct marks -> three nodes: "a" | "X"(bold) | "b"
    assert_eq!(p.child_count(), 3);
    assert!(p.child(1).unwrap().has_mark("bold"));
}

#[test]
fn insert_text_at_end_position() {
    let mut p = para([Node::text("hi")]);
    p.insert_text(pos(1, 0), "!", None).unwrap();
    assert_eq!(p.text_content(), "hi!");
    assert_eq!(p.child_count(), 1);
}

// ---- delete -------------------------------------------------------------

#[test]
fn delete_within_single_text_node() {
    let mut p = para([Node::text("hello")]);
    p.delete_range(range((0, 1), (0, 3))).unwrap(); // remove "el"
    assert_eq!(p.text_content(), "hlo");
    assert_eq!(p.child_count(), 1);
}

#[test]
fn delete_across_nodes_merges_remainder() {
    let mut p = para([Node::text("ab"), Node::text("cd")]);
    p.delete_range(range((0, 1), (1, 1))).unwrap(); // remove "b","c"
    assert_eq!(p.text_content(), "ad");
    assert_eq!(p.child_count(), 1); // "a" + "d" merged
}

#[test]
fn delete_whole_inline_leaf() {
    let mut p = para([Node::text("a"), Node::element("hardBreak"), Node::text("b")]);
    // remove the hardBreak (child 1) via a range covering just it
    p.delete_range(range((1, 0), (2, 0))).unwrap();
    assert_eq!(p.child_count(), 1);
    assert_eq!(p.child(0).unwrap().get_text(), Some("ab"));
}

#[test]
fn delete_collapsed_is_noop() {
    let mut p = para([Node::text("hello")]);
    let before = p.clone();
    p.delete_range(Range::collapsed(pos(0, 2))).unwrap();
    assert_eq!(p, before);
}

// ---- replace ------------------------------------------------------------

#[test]
fn replace_range_basic() {
    let mut p = para([Node::text("hello")]);
    p.replace_range(range((0, 1), (0, 4)), "EY", None).unwrap(); // "ell" -> "EY"
    assert_eq!(p.text_content(), "hEYo");
}

#[test]
fn replace_with_empty_deletes() {
    let mut p = para([Node::text("hello")]);
    p.replace_range(range((0, 0), (0, 2)), "", None).unwrap();
    assert_eq!(p.text_content(), "llo");
}

// ---- marks --------------------------------------------------------------

#[test]
fn add_mark_range_marks_only_covered() {
    let mut p = para([Node::text("Hello world")]);
    p.add_mark_range(range((0, 6), (0, 11)), Mark::new("bold"))
        .unwrap();
    // "Hello " | "world"(bold)
    assert_eq!(p.child_count(), 2);
    assert_eq!(p.child(0).unwrap().get_text(), Some("Hello "));
    assert!(!p.child(0).unwrap().has_mark("bold"));
    assert_eq!(p.child(1).unwrap().get_text(), Some("world"));
    assert!(p.child(1).unwrap().has_mark("bold"));
}

#[test]
fn add_mark_full_node_then_remove_remerges() {
    let mut p = para([Node::text("ab"), Node::text("cd")]);
    // mark everything bold -> both nodes bold, then merge into one bold "abcd"
    p.add_mark_range(range((0, 0), (1, 2)), Mark::new("bold"))
        .unwrap();
    assert_eq!(p.child_count(), 1);
    assert_eq!(p.child(0).unwrap().get_text(), Some("abcd"));
    assert!(p.child(0).unwrap().has_mark("bold"));

    // remove it -> back to a single plain "abcd"
    p.remove_mark_range(range((0, 0), (0, 4)), "bold").unwrap();
    assert_eq!(p.child_count(), 1);
    assert!(!p.child(0).unwrap().has_mark("bold"));
}

#[test]
fn toggle_mark_range_adds_then_removes() {
    let mut p = para([Node::text("abc")]);
    let r = range((0, 0), (0, 3));
    p.toggle_mark_range(r, Mark::new("italic")).unwrap();
    assert!(p.child(0).unwrap().has_mark("italic"));
    // all covered already have it -> toggle removes
    p.toggle_mark_range(r, Mark::new("italic")).unwrap();
    assert!(!p.child(0).unwrap().has_mark("italic"));
}

#[test]
fn toggle_partial_coverage_adds_to_all() {
    // one half already bold; toggling the whole range adds to the rest.
    let mut p = para([
        Node::text_with_marks("ab", [Mark::new("bold")]),
        Node::text("cd"),
    ]);
    p.toggle_mark_range(range((0, 0), (1, 2)), Mark::new("bold"))
        .unwrap();
    // not all had it -> add to all -> merge into one bold "abcd"
    assert_eq!(p.text_content(), "abcd");
    assert!(p.child(0).unwrap().has_mark("bold"));
}

// ---- unicode / errors ---------------------------------------------------

#[test]
fn unicode_scalar_split_does_not_panic() {
    let mut p = para([Node::text("héllo😀x")]);
    // split between the emoji and 'x' (offset 6 in scalars: h é l l o 😀 | x)
    p.insert_text(pos(0, 6), "-", None).unwrap();
    assert_eq!(p.text_content(), "héllo😀-x");
}

#[test]
fn offset_out_of_range_errors() {
    let mut p = para([Node::text("hi")]);
    assert_eq!(
        p.insert_text(pos(0, 5), "x", None),
        Err(RangeError::OffsetOutOfRange {
            child: 0,
            offset: 5
        })
    );
}

#[test]
fn child_out_of_range_errors() {
    let mut p = para([Node::text("hi")]);
    assert_eq!(
        p.delete_range(range((3, 0), (3, 0))),
        Err(RangeError::ChildOutOfRange { child: 3 })
    );
}

#[test]
fn inverted_range_errors() {
    let mut p = para([Node::text("hello")]);
    assert_eq!(
        p.delete_range(range((0, 3), (0, 1))),
        Err(RangeError::InvertedRange)
    );
}

#[test]
fn non_text_offset_errors() {
    let mut p = para([Node::element("hardBreak"), Node::text("x")]);
    assert_eq!(
        p.insert_text(pos(0, 1), "y", None),
        Err(RangeError::NotTextNode { child: 0 })
    );
}
