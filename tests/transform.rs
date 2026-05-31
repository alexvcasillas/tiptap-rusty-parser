//! Transaction tests: edits apply in place, and the recorded change log
//! reproduces the result from the original and inverts to an undo.

use tiptap_rusty_parser::{apply, Change, Mark, Node};

fn p(text: &str) -> Node {
    Node::element("paragraph").with_child(Node::text(text))
}

/// Assert that the recorded log, applied to a clone of `original`, reproduces
/// `result`, and that its inverse restores `original`.
fn assert_replayable(original: &Node, result: &Node, changes: &[Change]) {
    let mut replay = original.clone();
    apply(&mut replay, changes).unwrap();
    assert_eq!(
        &replay, result,
        "replay did not reproduce the transformed tree"
    );

    let undo = original.invert(changes).unwrap();
    let mut back = result.clone();
    apply(&mut back, &undo).unwrap();
    assert_eq!(&back, original, "invert did not restore the original");
}

#[test]
fn records_and_applies_in_place() {
    let mut doc = Node::element("doc").with_children([p("a"), p("b")]);
    let original = doc.clone();

    let changes = {
        let mut tx = doc.transform();
        tx.set_attr(vec![0], "level", 1).unwrap();
        tx.set_text(vec![1, 0], Some("B".into())).unwrap();
        tx.insert(vec![], 2, p("c")).unwrap();
        tx.finish()
    };

    // Mutated in place.
    assert_eq!(doc.child(0).unwrap().attr("level").unwrap(), 1);
    assert_eq!(doc.child(1).unwrap().text_content(), "B");
    assert_eq!(doc.child_count(), 3);

    assert_replayable(&original, &doc, &changes);
}

#[test]
fn chaining_with_question_mark() {
    let mut doc = Node::element("doc").with_child(p("x"));
    let original = doc.clone();
    let changes = {
        let mut tx = doc.transform();
        tx.set_text(vec![0, 0], Some("y".into()))
            .unwrap()
            .set_attr(vec![0], "align", "center")
            .unwrap();
        tx.finish()
    };
    assert_eq!(changes.len(), 2);
    assert_replayable(&original, &doc, &changes);
}

#[test]
fn move_child_records_move() {
    let mut doc = Node::element("doc").with_children([p("a"), p("b"), p("c")]);
    let original = doc.clone();
    let changes = {
        let mut tx = doc.transform();
        tx.move_child(vec![], 0, 2).unwrap(); // a -> end
        tx.finish()
    };
    assert_eq!(doc.child(2).unwrap().text_content(), "a");
    assert!(matches!(changes.as_slice(), [Change::Move { .. }]));
    assert_replayable(&original, &doc, &changes);
}

#[test]
fn marks_and_extra() {
    let mut doc = Node::element("doc").with_child(Node::text("hi"));
    let original = doc.clone();
    let changes = {
        let mut tx = doc.transform();
        tx.set_marks(vec![0], Some(vec![Mark::new("bold")]))
            .unwrap();
        tx.set_extra(vec![], "docId", "abc").unwrap();
        tx.finish()
    };
    assert!(doc.child(0).unwrap().has_mark("bold"));
    assert_eq!(doc.extra.get("docId").unwrap(), "abc");
    assert_replayable(&original, &doc, &changes);
}

#[test]
fn remove_and_replace() {
    let mut doc = Node::element("doc").with_children([p("a"), p("b")]);
    let original = doc.clone();
    let changes = {
        let mut tx = doc.transform();
        tx.replace(
            vec![0],
            Node::element("heading").with_child(Node::text("H")),
        )
        .unwrap();
        tx.remove(vec![], 1).unwrap();
        tx.finish()
    };
    assert_eq!(doc.child_count(), 1);
    assert_eq!(doc.child(0).unwrap().node_type.as_deref(), Some("heading"));
    assert_replayable(&original, &doc, &changes);
}

#[test]
fn inline_range_builders_record_and_invert() {
    use tiptap_rusty_parser::{Position, Range};
    // doc > paragraph("hello world")
    let mut doc = Node::element("doc").with_child(p("hello world"));
    let original = doc.clone();
    let changes = {
        let mut tx = doc.transform();
        // Bold "hello", then replace "world" with "there", then append "!".
        tx.add_mark_range_in(
            vec![0],
            Range::new(Position::new(0, 0), Position::new(0, 5)),
            Mark::new("bold"),
        )
        .unwrap();
        tx.replace_range_in(
            vec![0],
            Range::new(Position::new(1, 1), Position::new(1, 6)),
            "there",
            None,
        )
        .unwrap();
        tx.insert_text_at(vec![0], Position::new(2, 0), "!", None)
            .unwrap();
        tx.finish()
    };
    assert_eq!(doc.child(0).unwrap().text_content(), "hello there!");
    assert!(doc.child(0).unwrap().child(0).unwrap().has_mark("bold"));
    assert_replayable(&original, &doc, &changes);
}

#[test]
fn delete_range_builder_records() {
    use tiptap_rusty_parser::{Position, Range};
    let mut doc = Node::element("doc").with_child(p("hello world"));
    let original = doc.clone();
    let changes = {
        let mut tx = doc.transform();
        tx.delete_range_in(
            vec![0],
            Range::new(Position::new(0, 5), Position::new(0, 11)),
        )
        .unwrap();
        tx.finish()
    };
    assert_eq!(doc.child(0).unwrap().text_content(), "hello");
    assert_replayable(&original, &doc, &changes);
}

#[test]
fn empty_transaction_is_empty_log() {
    let mut doc = p("x");
    let changes = doc.transform().finish();
    assert!(changes.is_empty());
}

#[test]
fn bad_path_errors_and_stops() {
    let mut doc = Node::element("doc").with_child(p("a"));
    let mut tx = doc.transform();
    tx.set_text(vec![0, 0], Some("ok".into())).unwrap();
    // No child at index 5 -> error.
    let err = tx.set_text(vec![5, 0], Some("no".into()));
    assert!(err.is_err());
    // The earlier successful edit is still recorded.
    assert_eq!(tx.changes().len(), 1);
}

#[test]
fn matches_equivalent_diff() {
    // A transaction's log applied to the original yields the same tree a diff
    // would round-trip to.
    let mut doc = Node::element("doc").with_children([p("one"), p("two")]);
    let original = doc.clone();
    let changes = {
        let mut tx = doc.transform();
        tx.set_text(vec![0, 0], Some("ONE".into())).unwrap();
        tx.insert(vec![], 2, p("three")).unwrap();
        tx.finish()
    };
    // diff(original, doc) is a valid alternative patch to the same target.
    let mut via_diff = original.clone();
    apply(&mut via_diff, &original.diff(&doc)).unwrap();
    assert_eq!(via_diff, doc);
    assert_replayable(&original, &doc, &changes);
}
