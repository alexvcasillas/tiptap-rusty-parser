//! Flat-position mapping (`PosMap`) tests.

use tiptap_rusty_parser::{Assoc, Mark, Node, PosContent, PosEdit, PosMap, PosRange};

fn p(text: &str) -> Node {
    Node::element("paragraph").with_child(Node::text(text))
}

fn text(s: &str) -> PosContent {
    PosContent::Text {
        text: s.into(),
        marks: None,
    }
}

// ---- raw map mechanics --------------------------------------------------

#[test]
fn empty_map_is_identity() {
    let m = PosMap::new();
    assert!(m.is_empty());
    for pos in 0..10 {
        assert_eq!(m.map(pos, Assoc::Left), pos);
        assert_eq!(m.map(pos, Assoc::Right), pos);
    }
}

#[test]
fn insertion_shifts_and_respects_assoc() {
    // Insert 4 units at position 7.
    let mut m = PosMap::new();
    m.push(7, 0, 4);
    assert_eq!(m.map(0, Assoc::Left), 0); // before
    assert_eq!(m.map(6, Assoc::Right), 6);
    assert_eq!(m.map(7, Assoc::Left), 7); // at insert, stay before
    assert_eq!(m.map(7, Assoc::Right), 11); // at insert, move after
    assert_eq!(m.map(8, Assoc::Left), 12); // after: +4
}

#[test]
fn deletion_collapses_interior() {
    // Delete [5, 10).
    let mut m = PosMap::new();
    m.push(5, 5, 0);
    assert_eq!(m.map(4, Assoc::Left), 4);
    assert_eq!(m.map(5, Assoc::Left), 5);
    assert_eq!(m.map(7, Assoc::Left), 5); // interior collapses to start
    assert_eq!(m.map(7, Assoc::Right), 5);
    assert_eq!(m.map(10, Assoc::Left), 5); // end boundary -> start
    assert_eq!(m.map(11, Assoc::Left), 6); // after: -5
}

#[test]
fn replacement_maps_edges() {
    // Replace [5, 10) (5 wide) with 2 wide.
    let mut m = PosMap::new();
    m.push(5, 5, 2);
    assert_eq!(m.map(5, Assoc::Left), 5);
    assert_eq!(m.map(7, Assoc::Left), 5); // interior -> left edge
    assert_eq!(m.map(7, Assoc::Right), 7); // interior -> right edge (5 + 2)
    assert_eq!(m.map(10, Assoc::Right), 7); // end -> right edge
    assert_eq!(m.map(12, Assoc::Left), 9); // after: -3
}

#[test]
fn multiple_disjoint_steps_accumulate() {
    // Delete [2,4) (-2), insert 3 at 10 (+3), order-insensitive on push.
    let mut m = PosMap::new();
    m.push(10, 0, 3);
    m.push(2, 2, 0);
    assert_eq!(m.map(1, Assoc::Left), 1);
    assert_eq!(m.map(5, Assoc::Left), 3); // after first step: -2
    assert_eq!(m.map(10, Assoc::Right), 11); // -2 then +3 at the insert
    assert_eq!(m.map(12, Assoc::Left), 13); // -2 + 3
}

#[test]
fn map_range_keeps_collapsed_collapsed() {
    let mut m = PosMap::new();
    m.push(3, 0, 5); // insert 5 at pos 3
    let r = m.map_range(PosRange::new(3, 3), Assoc::Right);
    assert_eq!((r.from, r.to), (8, 8)); // both endpoints move together
}

// ---- built from edits ---------------------------------------------------

#[test]
fn from_pos_edits_matches_manual() {
    let edits = vec![
        PosEdit::Delete { from: 2, to: 4 },
        PosEdit::Insert {
            pos: 10,
            content: text("xyz"),
        },
        // marks/attrs contribute no step
        PosEdit::AddMark {
            from: 6,
            to: 7,
            mark: Mark::new("bold"),
        },
    ];
    let m = PosMap::from_pos_edits(&edits);
    let mut manual = PosMap::new();
    manual.push(2, 2, 0);
    manual.push(10, 0, 3);
    assert_eq!(m, manual);
}

#[test]
fn from_pos_edits_counts_node_sizes() {
    // Inserting a paragraph("ab") = node_size 4 (2 tokens + 2 scalars).
    let edits = vec![PosEdit::Insert {
        pos: 0,
        content: PosContent::Nodes {
            nodes: vec![p("ab")],
        },
    }];
    let m = PosMap::from_pos_edits(&edits);
    assert_eq!(m.map(0, Assoc::Right), 4);
}

// ---- end-to-end: map a position through an applied batch ----------------

#[test]
fn mapped_edit_anchors_position_in_new_doc() {
    // doc > [ p("hello world"), p("second line") ]
    let mut doc = Node::element("doc").with_children([p("hello world"), p("second line")]);

    // A cursor sitting at the start of "line" in p1: flat pos 21.
    let cursor = 21;

    // Insert "big " before "world" (pos 7) — entirely before the cursor.
    let (_patch, map) = doc
        .apply_pos_edits_mapped(&[PosEdit::Insert {
            pos: 7,
            content: text("big "),
        }])
        .unwrap();

    // The cursor shifts right by the 4 inserted units.
    let moved = map.map(cursor, Assoc::Left);
    assert_eq!(moved, 25);

    // And `moved` still resolves to the same spot ("line") in the edited doc.
    let (block, inline) = doc.pos_to_inline(moved).unwrap();
    assert_eq!(block, vec![1]);
    // p1 == "second line" still; offset 7 == before "line".
    assert_eq!(inline.offset, 7);
}

#[test]
fn mapped_delete_pulls_trailing_position_back() {
    let mut doc = Node::element("doc").with_children([p("hello world"), p("second line")]);
    let end = doc.content_size(); // 26
    let (_patch, map) = doc
        .apply_pos_edits_mapped(&[PosEdit::Delete { from: 6, to: 12 }]) // drop " world"
        .unwrap();
    // 6 units removed -> end shifts back by 6.
    assert_eq!(map.map(end, Assoc::Left), end - 6);
    assert_eq!(map.map(end, Assoc::Left), doc.content_size());
}
