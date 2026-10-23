//! Block-level structural editing tests. Each op is exercised directly and,
//! where applicable, through a `Transform` — asserting the recorded patch
//! replays from the original and inverts to restore it.

use tiptap_rusty_parser::{apply, BlockError, BlockRange, Node, Position};

fn para(text: &str) -> Node {
    Node::element("paragraph").with_child(Node::text(text))
}

fn doc(children: impl IntoIterator<Item = Node>) -> Node {
    Node::element("doc").with_children(children)
}

/// Run `op` directly on a clone AND via a `Transform` on another clone; assert
/// both yield the same tree, and that the recorded patch replays + inverts.
fn check<F, G>(before: &Node, direct: F, recorded: G)
where
    F: FnOnce(&mut Node) -> Result<(), BlockError>,
    G: FnOnce(&mut tiptap_rusty_parser::Transform) -> Result<(), BlockError>,
{
    let mut a = before.clone();
    direct(&mut a).unwrap();

    let mut b = before.clone();
    let changes = {
        let mut tx = b.transform();
        recorded(&mut tx).unwrap();
        tx.finish()
    };
    assert_eq!(a, b, "direct op and transform-recorded op diverged");

    // Recorded patch replays from the original.
    let mut replay = before.clone();
    apply(&mut replay, &changes).unwrap();
    assert_eq!(replay, b, "recorded patch did not reproduce the result");

    // …and inverts to an undo.
    let undo = before.invert(&changes).unwrap();
    let mut back = b.clone();
    apply(&mut back, &undo).unwrap();
    assert_eq!(back, *before, "invert did not restore the original");
}

// ---- set_block_type -----------------------------------------------------

#[test]
fn set_block_type_keeps_content() {
    let before = doc([para("hi")]);
    check(
        &before,
        |n| n.set_block_type(&[0], "heading", None),
        |tx| tx.set_block_type(vec![0], "heading", None).map(|_| ()),
    );
    let mut n = before.clone();
    n.set_block_type(&[0], "heading", None).unwrap();
    assert_eq!(n.child(0).unwrap().node_type.as_deref(), Some("heading"));
    assert_eq!(n.child(0).unwrap().text_content(), "hi"); // content kept
}

#[test]
fn set_block_type_bad_path() {
    let mut n = doc([para("a")]);
    assert_eq!(
        n.set_block_type(&[5], "heading", None),
        Err(BlockError::PathNotFound { path: vec![5] })
    );
}

// ---- split_block --------------------------------------------------------

#[test]
fn split_block_at_boundary() {
    // paragraph with two inline spans, split between them.
    let before =
        doc([Node::element("paragraph")
            .with_children([Node::text("foo"), Node::element("hardBreak")])]);
    check(
        &before,
        |n| n.split_block(&[0], 1, 0),
        |tx| tx.split_block(vec![0], 1, 0).map(|_| ()),
    );
    let mut n = before.clone();
    n.split_block(&[0], 1, 0).unwrap();
    assert_eq!(n.child_count(), 2);
    assert_eq!(n.child(0).unwrap().text_content(), "foo");
    assert_eq!(
        n.child(1).unwrap().child(0).unwrap().node_type.as_deref(),
        Some("hardBreak")
    );
}

#[test]
fn split_block_mid_text() {
    let before = doc([para("abcd")]);
    let mut n = before.clone();
    n.split_block_at(&[0], Position::new(0, 2), 0).unwrap(); // "ab" | "cd"
    assert_eq!(n.child_count(), 2);
    assert_eq!(n.child(0).unwrap().text_content(), "ab");
    assert_eq!(n.child(1).unwrap().text_content(), "cd");

    // recorded form round-trips
    check(
        &before,
        |x| x.split_block_at(&[0], Position::new(0, 2), 0),
        |tx| {
            tx.split_block_at(vec![0], Position::new(0, 2), 0)
                .map(|_| ())
        },
    );
}

#[test]
fn split_block_depth_one_splits_parent() {
    // bulletList > listItem > paragraph("ab|cd"); depth 1 splits the listItem too.
    let before =
        doc([Node::element("bulletList")
            .with_child(Node::element("listItem").with_child(para("abcd")))]);
    let mut n = before.clone();
    n.split_block_at(&[0, 0, 0], Position::new(0, 2), 1)
        .unwrap();
    let list = n.child(0).unwrap();
    assert_eq!(list.child_count(), 2, "listItem should have split");
    assert_eq!(
        list.child(0).unwrap().node_type.as_deref(),
        Some("listItem")
    );
    assert_eq!(list.child(0).unwrap().text_content(), "ab");
    assert_eq!(list.child(1).unwrap().text_content(), "cd");
}

#[test]
fn split_block_edges() {
    let before = doc([para("ab")]);
    let mut at0 = before.clone();
    at0.split_block(&[0], 0, 0).unwrap(); // left empty
    assert_eq!(at0.child_count(), 2);
    assert_eq!(at0.child(0).unwrap().child_count(), 0);
    assert_eq!(at0.child(1).unwrap().text_content(), "ab");

    let mut at_len = before.clone();
    at_len.split_block(&[0], 1, 0).unwrap(); // right empty
    assert_eq!(at_len.child(1).unwrap().child_count(), 0);
}

#[test]
fn split_block_root_errors() {
    let mut n = doc([para("a")]);
    assert_eq!(n.split_block(&[], 0, 0), Err(BlockError::NoParent));
}

#[test]
fn split_block_at_root_errors_without_partial_mutation() {
    // split_block_at materializes a boundary (mutating) before delegating; a
    // failed precondition must not leave the tree half-split.
    let mut n = doc([para("abcd")]);
    let before = n.clone();
    assert_eq!(
        n.split_block_at(&[], Position::new(0, 2), 0),
        Err(BlockError::NoParent)
    );
    assert_eq!(n, before, "tree must be unchanged on error");
}

// ---- join_blocks --------------------------------------------------------

#[test]
fn join_merges_into_previous() {
    let before = doc([para("Hello "), para("world")]);
    check(
        &before,
        |n| n.join_blocks(&[], 1),
        |tx| tx.join_blocks(vec![], 1).map(|_| ()),
    );
    let mut n = before.clone();
    n.join_blocks(&[], 1).unwrap();
    assert_eq!(n.child_count(), 1);
    // seam text merged into a single node
    assert_eq!(n.child(0).unwrap().child_count(), 1);
    assert_eq!(n.child(0).unwrap().text_content(), "Hello world");
}

#[test]
fn join_keeps_left_type_on_mismatch() {
    let before = doc([
        Node::element("heading").with_child(Node::text("H")),
        para("p"),
    ]);
    let mut n = before.clone();
    n.join_blocks(&[], 1).unwrap();
    assert_eq!(n.child(0).unwrap().node_type.as_deref(), Some("heading"));
    assert_eq!(n.child(0).unwrap().text_content(), "Hp");
}

#[test]
fn join_index_zero_errors() {
    let mut n = doc([para("a")]);
    assert_eq!(
        n.join_blocks(&[], 0),
        Err(BlockError::NoPreviousSibling { path: vec![0] })
    );
}

// ---- wrap / wrap_range --------------------------------------------------

#[test]
fn wrap_single_block() {
    let before = doc([para("a")]);
    check(
        &before,
        |n| n.wrap(&[0], "blockquote", None),
        |tx| tx.wrap(vec![0], "blockquote", None).map(|_| ()),
    );
    let mut n = before.clone();
    n.wrap(&[0], "blockquote", None).unwrap();
    assert_eq!(n.child(0).unwrap().node_type.as_deref(), Some("blockquote"));
    assert_eq!(n.child(0).unwrap().child(0).unwrap().text_content(), "a");
}

#[test]
fn wrap_range_preserves_surrounding_siblings() {
    let before = doc([para("a"), para("b"), para("c"), para("d")]);
    let range = BlockRange::new(vec![], 1, 3); // wrap b, c
    check(
        &before,
        |n| n.wrap_range(&range, "blockquote", None),
        |tx| {
            tx.wrap_range(BlockRange::new(vec![], 1, 3), "blockquote", None)
                .map(|_| ())
        },
    );
    let mut n = before.clone();
    n.wrap_range(&range, "blockquote", None).unwrap();
    assert_eq!(n.child_count(), 3); // a, blockquote(b,c), d
    assert_eq!(n.child(0).unwrap().text_content(), "a");
    assert_eq!(n.child(1).unwrap().node_type.as_deref(), Some("blockquote"));
    assert_eq!(n.child(1).unwrap().child_count(), 2);
    assert_eq!(n.child(2).unwrap().text_content(), "d");
}

#[test]
fn wrap_range_invalid() {
    let mut n = doc([para("a")]);
    assert_eq!(
        n.wrap_range(&BlockRange::new(vec![], 0, 5), "blockquote", None),
        Err(BlockError::InvalidRange {
            parent: vec![],
            start: 0,
            end: 5
        })
    );
}

// ---- lift ---------------------------------------------------------------

#[test]
fn lift_sole_child_collapses_parent() {
    // doc > blockquote > paragraph  =>  doc > paragraph
    let before = doc([Node::element("blockquote").with_child(para("a"))]);
    check(
        &before,
        |n| n.lift(&[0, 0]),
        |tx| tx.lift(vec![0, 0]).map(|_| ()),
    );
    let mut n = before.clone();
    n.lift(&[0, 0]).unwrap();
    assert_eq!(n.child_count(), 1);
    assert_eq!(n.child(0).unwrap().node_type.as_deref(), Some("paragraph"));
    assert_eq!(n.child(0).unwrap().text_content(), "a");
}

#[test]
fn lift_middle_child_splits_parent() {
    // doc > blockquote > [p"a", p"b", p"c"]; lift the middle.
    let before =
        doc([Node::element("blockquote").with_children([para("a"), para("b"), para("c")])]);
    check(
        &before,
        |n| n.lift(&[0, 1]),
        |tx| tx.lift(vec![0, 1]).map(|_| ()),
    );
    let mut n = before.clone();
    n.lift(&[0, 1]).unwrap();
    // blockquote(a) | paragraph(b) | blockquote(c)
    assert_eq!(n.child_count(), 3);
    assert_eq!(n.child(0).unwrap().node_type.as_deref(), Some("blockquote"));
    assert_eq!(n.child(0).unwrap().text_content(), "a");
    assert_eq!(n.child(1).unwrap().node_type.as_deref(), Some("paragraph"));
    assert_eq!(n.child(1).unwrap().text_content(), "b");
    assert_eq!(n.child(2).unwrap().node_type.as_deref(), Some("blockquote"));
    assert_eq!(n.child(2).unwrap().text_content(), "c");
}

#[test]
fn lift_too_shallow_errors() {
    let mut n = doc([para("a")]);
    assert_eq!(n.lift(&[0]), Err(BlockError::NoParent)); // paragraph's parent is root
}

// ---- fuzz: random block ops always round-trip via Transform -------------

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn fuzz_block_ops_roundtrip() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    for _ in 0..500 {
        // A small doc of blockquotes/paragraphs to give structure to edit.
        let n = 2 + rng.below(4);
        let before = doc((0..n).map(|i| {
            if rng.below(2) == 0 {
                Node::element("blockquote").with_child(para(&format!("b{i}")))
            } else {
                para(&format!("p{i}"))
            }
        }));

        let mut tree = before.clone();
        let changes = {
            let mut tx = tree.transform();
            // one random op
            let kids = before.child_count();
            let _ = match rng.below(5) {
                0 => tx.set_block_type(vec![rng.below(kids)], "heading", None),
                1 if kids >= 2 => tx.join_blocks(vec![], 1 + rng.below(kids - 1)),
                2 => tx.wrap(vec![rng.below(kids)], "section", None),
                3 => tx.split_block(vec![rng.below(kids)], 0, 0),
                _ => tx.wrap_range(BlockRange::new(vec![], 0, kids), "section", None),
            };
            tx.finish()
        };
        // The recorded patch must reproduce the mutated tree and invert cleanly.
        let mut replay = before.clone();
        apply(&mut replay, &changes).unwrap();
        assert_eq!(replay, tree, "patch replay mismatch");
        let undo = before.invert(&changes).unwrap();
        let mut back = tree.clone();
        apply(&mut back, &undo).unwrap();
        assert_eq!(back, before, "undo mismatch");
    }
}
