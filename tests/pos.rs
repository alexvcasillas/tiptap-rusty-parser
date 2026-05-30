//! Flat ProseMirror position model tests.

use tiptap_rusty_parser::{LeafPolicy, Node, PosError, Position};

fn p(text: &str) -> Node {
    Node::element("paragraph").with_child(Node::text(text))
}

/// `doc > [ p("hi"), horizontalRule, p("ok") ]`
/// Sizes: p("hi") = 2 + 2 = 4; hr (leaf) = 1; p("ok") = 4. content_size = 9.
fn sample() -> Node {
    Node::element("doc").with_children([p("hi"), Node::element("horizontalRule"), p("ok")])
}

// ---- sizes --------------------------------------------------------------

#[test]
fn node_sizes() {
    let d = sample();
    assert_eq!(d.content_size(), 9);
    assert_eq!(d.child(0).unwrap().node_size(), 4); // paragraph "hi"
    assert_eq!(d.child(1).unwrap().node_size(), 1); // horizontalRule leaf
    assert_eq!(d.child(0).unwrap().child(0).unwrap().node_size(), 2); // text "hi"
}

#[test]
fn empty_paragraph_is_a_container_not_a_leaf() {
    // content: Some([]) -> size 2 (open+close), not a leaf.
    let empty_para = Node::element("paragraph").with_children(Vec::<Node>::new());
    assert!(!empty_para.is_leaf());
    assert_eq!(empty_para.node_size(), 2);
    // content: None on a non-leaf type -> still a container (size 2) by default.
    let bare = Node::element("paragraph");
    assert!(!bare.is_leaf());
    assert_eq!(bare.node_size(), 2);
    // a known leaf type -> size 1.
    assert!(Node::element("image").is_leaf());
    assert_eq!(Node::element("image").node_size(), 1);
}

#[test]
fn leaf_policy_override() {
    let custom = LeafPolicy::from_types(["mediaEmbed"]);
    let n = Node::element("mediaEmbed");
    assert!(n.is_leaf_with(&custom));
    assert_eq!(n.node_size_with(&custom), 1);
    // not a leaf under the default policy.
    assert_eq!(n.node_size(), 2);
}

// ---- pos_before / after / in_text --------------------------------------

#[test]
fn hand_computed_positions() {
    let d = sample();
    assert_eq!(d.pos_before(&[0]).unwrap(), 0); // first paragraph
    assert_eq!(d.pos_after(&[0]).unwrap(), 4);
    assert_eq!(d.pos_before(&[1]).unwrap(), 4); // horizontalRule
    assert_eq!(d.pos_after(&[1]).unwrap(), 5);
    assert_eq!(d.pos_before(&[2]).unwrap(), 5); // third paragraph
    assert_eq!(d.pos_after(&[2]).unwrap(), 9);

    // Inside the first paragraph's text "hi": content starts at pos 1.
    assert_eq!(d.pos_before(&[0, 0]).unwrap(), 1);
    assert_eq!(d.pos_in_text(&[0, 0], 0).unwrap(), 1);
    assert_eq!(d.pos_in_text(&[0, 0], 1).unwrap(), 2);
    assert_eq!(d.pos_in_text(&[0, 0], 2).unwrap(), 3);
}

// ---- resolve ------------------------------------------------------------

#[test]
fn resolve_boundaries_and_text() {
    let d = sample();

    // pos 0: boundary before first child of doc.
    let r = d.resolve(0).unwrap();
    assert_eq!(r.path, Vec::<usize>::new());
    assert_eq!(r.index, 0);
    assert!(!r.is_in_text());

    // pos 2: inside "hi" at scalar offset 1.
    let r = d.resolve(2).unwrap();
    assert_eq!(r.path, vec![0]);
    let tp = r.text_offset.unwrap();
    assert_eq!(tp.path, vec![0, 0]);
    assert_eq!(tp.offset, 1);

    // pos 4: between the first paragraph and the rule (in doc).
    let r = d.resolve(4).unwrap();
    assert_eq!(r.path, Vec::<usize>::new());
    assert_eq!(r.index, 1);
    assert!(!r.is_in_text());

    // pos 9: very end of doc.
    let r = d.resolve(9).unwrap();
    assert_eq!(r.path, Vec::<usize>::new());
    assert_eq!(r.index, 3);
}

#[test]
fn invalid_construction_errors() {
    let d = sample(); // doc with 3 children; [1] is a leaf horizontalRule
                      // pos_before to a non-existent node index.
    assert!(d.pos_before(&[3]).is_err());
    // pos_in_text on a non-text node, and offset past text length.
    assert!(d.pos_in_text(&[1], 0).is_err()); // the rule is not text
    assert!(d.pos_in_text(&[0, 0], 2).is_ok()); // "hi" has len 2; 0..=2 valid
    assert!(d.pos_in_text(&[0, 0], 3).is_err()); // past the text
                                                 // inline_to_pos offset validation.
    assert!(d.inline_to_pos(&[0], Position::new(0, 99)).is_err()); // past "hi"
    assert!(d.inline_to_pos(&[0], Position::new(1, 1)).is_err()); // end boundary, offset != 0
    assert!(d.inline_to_pos(&[0], Position::new(0, 2)).is_ok()); // end of "hi"
}

#[test]
fn resolve_out_of_range() {
    let d = sample();
    assert_eq!(
        d.resolve(10),
        Err(PosError::OutOfRange { pos: 10, size: 9 })
    );
}

#[test]
fn resolve_roundtrips_pos_before() {
    let d = sample();
    // For each node boundary, resolve(pos_before(path)) lands in the parent.
    for path in [vec![0], vec![1], vec![2]] {
        let pos = d.pos_before(&path).unwrap();
        let r = d.resolve(pos).unwrap();
        assert_eq!(r.path, Vec::<usize>::new());
        assert_eq!(r.index, *path.last().unwrap());
    }
}

#[test]
fn resolve_every_position_is_ok() {
    let d = sample();
    for pos in 0..=d.content_size() {
        let r = d.resolve(pos).unwrap();
        assert_eq!(r.pos, pos);
    }
}

// ---- nested + bridges ---------------------------------------------------

#[test]
fn nested_blocks_positions() {
    // doc > bulletList > listItem > paragraph("xy")
    // sizes: text "xy"=2; paragraph=2+2=4; listItem=2+4=6; bulletList=2+6=8.
    let d = Node::element("doc").with_child(
        Node::element("bulletList").with_child(Node::element("listItem").with_child(p("xy"))),
    );
    assert_eq!(d.content_size(), 8);
    assert_eq!(d.pos_before(&[0, 0, 0, 0]).unwrap(), 3); // text "xy": 3 open tokens in
    assert_eq!(d.pos_in_text(&[0, 0, 0, 0], 1).unwrap(), 4);
    let r = d.resolve(4).unwrap(); // strictly inside "xy"
    assert_eq!(r.path, vec![0, 0, 0]);
    assert_eq!(r.text_offset.unwrap().offset, 1);
}

#[test]
fn inline_bridges_roundtrip() {
    // paragraph with two text spans: "ab" | "cd"
    let d = Node::element("doc")
        .with_child(Node::element("paragraph").with_children([Node::text("ab"), Node::text("cd")]));
    // flat pos 4 = inside "cd" at scalar 1 (1 open + 2 "ab" + 1 into "cd").
    let (block, inline) = d.pos_to_inline(4).unwrap();
    assert_eq!(block, vec![0]);
    assert_eq!(inline, Position::new(1, 1));
    // inverse
    assert_eq!(d.inline_to_pos(&[0], Position::new(1, 1)).unwrap(), 4);
    // boundary before the paragraph's first span: pos 1 -> (child 0, offset 0)
    let (b2, i2) = d.pos_to_inline(1).unwrap();
    assert_eq!((b2, i2), (vec![0], Position::new(0, 0)));
    assert_eq!(d.inline_to_pos(&[0], Position::new(0, 0)).unwrap(), 1);
}
