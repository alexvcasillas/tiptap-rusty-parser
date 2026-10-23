//! Position-addressed apply/invert tests.

use serde_json::json;
use tiptap_rusty_parser::{Mark, Node, PosContent, PosEdit, PosEditError};

fn p(text: &str) -> Node {
    Node::element("paragraph").with_child(Node::text(text))
}

/// `doc > [ p("hello world"), p("second line") ]`
fn two_paras() -> Node {
    Node::element("doc").with_children([p("hello world"), p("second line")])
}

fn text(s: &str) -> PosContent {
    PosContent::Text {
        text: s.into(),
        marks: None,
    }
}

/// apply_pos_edits returns a patch that reproduces the mutation and inverts it.
fn assert_invertible(before: &Node, after: &Node, patch: &[tiptap_rusty_parser::Change]) {
    let mut replay = before.clone();
    replay.apply(patch).unwrap();
    assert_eq!(&replay, after, "patch must reproduce the mutated tree");
    let undo = before.invert(patch).unwrap();
    let mut back = after.clone();
    back.apply(&undo).unwrap();
    assert_eq!(&back, before, "invert must restore the original");
}

// ---- same-block ---------------------------------------------------------

#[test]
fn same_block_replace_text() {
    let mut d = two_paras();
    let before = d.clone();
    // [7,12) in p0 == "world"
    let patch = d
        .apply_pos_edits(&[PosEdit::Replace {
            from: 7,
            to: 12,
            content: text("there"),
        }])
        .unwrap();
    assert_eq!(d.child(0).unwrap().text_content(), "hello there");
    assert_invertible(&before, &d, &patch);
}

#[test]
fn same_block_delete() {
    let mut d = two_paras();
    let before = d.clone();
    // delete " world" -> [6,12)
    let patch = d
        .apply_pos_edits(&[PosEdit::Delete { from: 6, to: 12 }])
        .unwrap();
    assert_eq!(d.child(0).unwrap().text_content(), "hello");
    assert_invertible(&before, &d, &patch);
}

#[test]
fn insert_text_mid_block() {
    let mut d = two_paras();
    let before = d.clone();
    // insert "big " before "world": pos 7
    let patch = d
        .apply_pos_edits(&[PosEdit::Insert {
            pos: 7,
            content: text("big "),
        }])
        .unwrap();
    assert_eq!(d.child(0).unwrap().text_content(), "hello big world");
    assert_invertible(&before, &d, &patch);
}

#[test]
fn add_and_remove_mark_same_block() {
    let mut d = two_paras();
    let before = d.clone();
    let patch = d
        .apply_pos_edits(&[PosEdit::AddMark {
            from: 7,
            to: 12,
            mark: Mark::new("bold"),
        }])
        .unwrap();
    // "world" split off and bolded.
    let p0 = d.child(0).unwrap();
    assert!(p0.children().iter().any(|c| c.has_mark("bold")));
    assert_invertible(&before, &d, &patch);

    // Now remove it again.
    let mut d2 = d.clone();
    let before2 = d2.clone();
    let patch2 = d2
        .apply_pos_edits(&[PosEdit::RemoveMark {
            from: 7,
            to: 12,
            mark_type: "bold".into(),
        }])
        .unwrap();
    assert!(!d2
        .child(0)
        .unwrap()
        .children()
        .iter()
        .any(|c| c.has_mark("bold")));
    assert_invertible(&before2, &d2, &patch2);
}

// ---- cross-block --------------------------------------------------------

#[test]
fn cross_block_delete_joins() {
    let mut d = two_paras();
    let before = d.clone();
    // From "hello |world" (pos 7) to "second |line" (in p1).
    // p0 size = 13; pos 7 = before "world". p1 starts at flat 13; its open token
    // is at 13, content at 14. "second line": delete up to before "line".
    // "second " = 7 scalars, so to = 14 + 7 = 21.
    let patch = d
        .apply_pos_edits(&[PosEdit::Delete { from: 7, to: 21 }])
        .unwrap();
    // The two paragraphs merge into one: "hello " + "line".
    assert_eq!(d.child_count(), 1);
    assert_eq!(d.child(0).unwrap().text_content(), "hello line");
    assert_invertible(&before, &d, &patch);
}

#[test]
fn cross_block_replace() {
    let mut d = two_paras();
    let before = d.clone();
    let patch = d
        .apply_pos_edits(&[PosEdit::Replace {
            from: 7,
            to: 21,
            content: text("X "),
        }])
        .unwrap();
    assert_eq!(d.child_count(), 1);
    assert_eq!(d.child(0).unwrap().text_content(), "hello X line");
    assert_invertible(&before, &d, &patch);
}

#[test]
fn cross_block_delete_with_middle_block() {
    // doc > [p("aa"), p("MID"), p("bb")]; delete from inside p0 to inside p2.
    let mut d = Node::element("doc").with_children([p("aa"), p("MID"), p("bb")]);
    let before = d.clone();
    // p0 size = 4 (2 + "aa"). pos 2 = after "a"(scalar1) inside p0? flat: open=0,
    // a@1, a@2(->scalar 1 after first a is pos 2). Use from=2 (after first "a").
    // p2 content starts: p0=4, p1 "MID" size=5, so p2 open at 9, content at 10,
    // "bb": to=11 (after first "b").
    let patch = d
        .apply_pos_edits(&[PosEdit::Delete { from: 2, to: 11 }])
        .unwrap();
    assert_eq!(d.child_count(), 1);
    assert_eq!(d.child(0).unwrap().text_content(), "ab");
    assert_invertible(&before, &d, &patch);
}

#[test]
fn cross_block_add_mark_fans_out() {
    let mut d = Node::element("doc").with_children([p("aa"), p("MID"), p("bb")]);
    let before = d.clone();
    let patch = d
        .apply_pos_edits(&[PosEdit::AddMark {
            from: 2,
            to: 11,
            mark: Mark::new("em"),
        }])
        .unwrap();
    // Every block now has at least one em-marked text node.
    for i in 0..d.child_count() {
        assert!(
            d.child(i)
                .unwrap()
                .children()
                .iter()
                .any(|c| c.has_mark("em")),
            "block {i} should carry the em mark"
        );
    }
    assert_invertible(&before, &d, &patch);
}

// ---- set block attrs ----------------------------------------------------

#[test]
fn set_block_attrs_replaces_map() {
    let mut d = two_paras();
    let before = d.clone();
    let mut attrs = serde_json::Map::new();
    attrs.insert("textAlign".into(), json!("center"));
    // pos 0 = boundary before p0.
    let patch = d
        .apply_pos_edits(&[PosEdit::SetBlockAttrs { pos: 0, attrs }])
        .unwrap();
    assert_eq!(
        d.child(0).unwrap().attrs.as_ref().unwrap().get("textAlign"),
        Some(&json!("center"))
    );
    assert_invertible(&before, &d, &patch);
}

#[test]
fn set_block_attrs_from_inline_position_targets_block() {
    // pos inside the paragraph's inline content (start of "hello") must set
    // attrs on the *paragraph*, not the text node.
    let mut d = two_paras();
    let before = d.clone();
    let inside = d.pos_before(&[0]).unwrap() + 1; // just inside p0
    let mut attrs = serde_json::Map::new();
    attrs.insert("textAlign".into(), json!("right"));
    let patch = d
        .apply_pos_edits(&[PosEdit::SetBlockAttrs { pos: inside, attrs }])
        .unwrap();
    // Paragraph carries the attr; its text child is untouched.
    assert_eq!(
        d.child(0).unwrap().attrs.as_ref().unwrap().get("textAlign"),
        Some(&json!("right"))
    );
    assert!(d.child(0).unwrap().child(0).unwrap().attrs.is_none());
    assert_invertible(&before, &d, &patch);
}

#[test]
fn inverted_span_errors_as_inverted_range() {
    use tiptap_rusty_parser::RangeError;
    let mut d = two_paras();
    let err = d
        .apply_pos_edits(&[PosEdit::Delete { from: 6, to: 2 }])
        .unwrap_err();
    assert_eq!(err, PosEditError::Range(RangeError::InvertedRange));
    assert_eq!(d, two_paras()); // untouched
}

// ---- insert nodes -------------------------------------------------------

#[test]
fn insert_block_nodes_at_boundary() {
    let mut d = two_paras();
    let before = d.clone();
    // pos 13 = boundary after p0 / before p1 (doc level).
    let patch = d
        .apply_pos_edits(&[PosEdit::Insert {
            pos: 13,
            content: PosContent::Nodes {
                nodes: vec![p("inserted")],
            },
        }])
        .unwrap();
    assert_eq!(d.child_count(), 3);
    assert_eq!(d.child(1).unwrap().text_content(), "inserted");
    assert_invertible(&before, &d, &patch);
}

// ---- batch + errors -----------------------------------------------------

#[test]
fn disjoint_batch_applies_descending() {
    let mut d = two_paras();
    let before = d.clone();
    // Two disjoint edits in document order; engine applies highest-first so the
    // low edit's positions stay valid.
    let patch = d
        .apply_pos_edits(&[
            PosEdit::Replace {
                from: 1,
                to: 6,
                content: text("HI"),
            }, // "hello" -> "HI" in p0
            PosEdit::Replace {
                from: 21,
                to: 25,
                content: text("LINE"),
            }, // "line" -> "LINE" in p1
        ])
        .unwrap();
    assert_eq!(d.child(0).unwrap().text_content(), "HI world");
    assert_eq!(d.child(1).unwrap().text_content(), "second LINE");
    assert_invertible(&before, &d, &patch);
}

#[test]
fn overlapping_edits_error() {
    let mut d = two_paras();
    let err = d
        .apply_pos_edits(&[
            PosEdit::Delete { from: 1, to: 6 },
            PosEdit::Delete { from: 4, to: 9 },
        ])
        .unwrap_err();
    assert!(matches!(err, PosEditError::OverlappingEdits { .. }));
    // self is untouched on error.
    assert_eq!(d, two_paras());
}

#[test]
fn cross_depth_span_unsupported() {
    // doc > [ p("aa"), blockquote > p("bb") ]: a span from p0 into the nested
    // paragraph crosses depths -> UnsupportedSpan.
    let mut d = Node::element("doc")
        .with_children([p("aa"), Node::element("blockquote").with_child(p("bb"))]);
    // p0 size 4; blockquote open at 4, its p open at 5, content at 6. to=7.
    let err = d
        .apply_pos_edits(&[PosEdit::Delete { from: 2, to: 7 }])
        .unwrap_err();
    assert!(matches!(err, PosEditError::UnsupportedSpan { .. }));
}

#[test]
fn pos_edits_serde_roundtrip() {
    let edits = vec![
        PosEdit::Insert {
            pos: 3,
            content: PosContent::Text {
                text: "x".into(),
                marks: Some(vec![Mark::new("bold")]),
            },
        },
        PosEdit::SetBlockAttrs {
            pos: 0,
            attrs: serde_json::Map::new(),
        },
    ];
    let s = serde_json::to_string(&edits).unwrap();
    // camelCase tags + fields.
    assert!(s.contains("\"type\":\"insert\""));
    assert!(s.contains("\"setBlockAttrs\""));
    let back: Vec<PosEdit> = serde_json::from_str(&s).unwrap();
    assert_eq!(back, edits);
}
