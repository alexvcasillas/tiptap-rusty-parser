//! Property-based tests (proptest) that generalize the hand-rolled fuzz: they
//! generate arbitrary `Node` trees and assert the core invariants hold.

use proptest::prelude::*;
use tiptap_rusty_parser::{apply, compact, compose, map_path, Mark, Node, Position, Range};

/// A text node, sometimes carrying a single mark (so merge/normalize and mark
/// diffing get exercised). Empty strings are allowed on purpose.
fn arb_text() -> impl Strategy<Value = Node> {
    (
        "[a-z ]{0,5}",
        prop::option::of(prop::sample::select(vec!["bold", "italic", "code"])),
    )
        .prop_map(|(s, mark)| match mark {
            Some(m) => Node::text_with_marks(s, [Mark::new(m)]),
            None => Node::text(s),
        })
}

/// An arbitrary (bounded) node tree: text/leaf nodes at the bottom, element
/// nodes with 0..4 children above, up to depth 4.
fn arb_tree() -> impl Strategy<Value = Node> {
    let leaf = prop_oneof![
        arb_text(),
        prop::sample::select(vec!["hardBreak", "image", "horizontalRule"]).prop_map(Node::element),
    ];
    leaf.prop_recursive(4, 40, 4, |inner| {
        (
            prop::sample::select(vec![
                "doc",
                "paragraph",
                "blockquote",
                "listItem",
                "bulletList",
                "heading",
            ]),
            prop::collection::vec(inner, 0..4),
        )
            .prop_map(|(t, kids)| Node::element(t).with_children(kids))
    })
}

proptest! {
    /// `apply(a, diff(a, b)) == b`, and the inverse restores `a`.
    #[test]
    fn diff_roundtrip_and_undo(a in arb_tree(), b in arb_tree()) {
        let changes = a.diff(&b);
        let mut got = a.clone();
        apply(&mut got, &changes).unwrap();
        prop_assert_eq!(&got, &b);

        let undo = a.invert(&changes).unwrap();
        let mut back = b.clone();
        apply(&mut back, &undo).unwrap();
        prop_assert_eq!(&back, &a);
    }

    /// `normalize` is idempotent and never changes the concatenated text.
    #[test]
    fn normalize_idempotent_and_text_preserving(a in arb_tree()) {
        let text = a.text_content();
        let mut once = a.clone();
        once.normalize();
        prop_assert_eq!(once.text_content(), text);

        let mut twice = once.clone();
        twice.normalize();
        prop_assert_eq!(once, twice);
    }

    /// `compose`/`compact` are apply-equivalent to sequential application, and
    /// every `map_path` Some result resolves in the applied tree.
    #[test]
    fn change_ops_properties(a in arb_tree(), b in arb_tree(), c in arb_tree()) {
        let d1 = a.diff(&b);
        let mut mid = a.clone();
        apply(&mut mid, &d1).unwrap();
        prop_assert_eq!(&mid, &b); // diff(a, b) reproduces b
        let d2 = mid.diff(&c);

        let mut via_compose = a.clone();
        apply(&mut via_compose, &compose(&d1, &d2)).unwrap();
        prop_assert_eq!(&via_compose, &c);

        let cat: Vec<_> = d1.iter().chain(&d2).cloned().collect();
        let compacted = compact(&cat);
        let mut via_compact = a.clone();
        apply(&mut via_compact, &compacted).unwrap();
        prop_assert_eq!(&via_compact, &c);
        prop_assert!(compacted.len() <= cat.len());

        for path in a.paths_to(|_| true) {
            if let Some(mapped) = map_path(&path, &d1) {
                prop_assert!(b.node_at(&mapped).is_some());
            }
        }
    }

    /// Wrapping a child then lifting it back out restores the original tree.
    #[test]
    fn wrap_then_lift_roundtrips(a in arb_tree()) {
        prop_assume!(a.child_count() > 0);
        let mut t = a.clone();
        t.wrap(&[0], "wrapper", None).unwrap();
        t.lift(&[0, 0]).unwrap();
        prop_assert_eq!(&t, &a);
    }

    /// Inserting text into a paragraph then deleting exactly that range restores
    /// the original text content.
    #[test]
    fn range_insert_then_delete_restores(
        text in "[a-z]{0,8}",
        ins in "[A-Z]{1,3}",
        at in 0usize..10,
    ) {
        let len = text.chars().count();
        let at = at.min(len);
        let mut p = Node::element("paragraph").with_child(Node::text(text.clone()));

        p.insert_text(Position::new(0, at), &ins, None).unwrap();
        let ins_len = ins.chars().count();
        p.delete_range(Range::new(Position::new(0, at), Position::new(0, at + ins_len)))
            .unwrap();

        prop_assert_eq!(p.text_content(), text);
    }
}
