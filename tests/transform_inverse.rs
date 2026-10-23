//! Incremental inverses: `Transform::finish_with_inverse` and
//! `Node::apply_inverting` record the undo while applying. For every `Change`
//! variant (and the container-shape edge cases), applying the inverse must
//! restore the original exactly, and must agree with the diff-based `invert`.

use serde_json::{json, Map};
use tiptap_rusty_parser::{apply, BlockRange, Change, Mark, Node, Position};

fn p(text: &str) -> Node {
    Node::element("paragraph").with_child(Node::text(text))
}

fn doc() -> Node {
    Node::element("doc").with_children([
        Node::element("heading")
            .with_attr("level", 1)
            .with_child(Node::text("Title")),
        p("alpha"),
        Node::element("bulletList").with_children([
            Node::element("listItem").with_child(p("one")),
            Node::element("listItem").with_child(p("two")),
        ]),
        Node::text_with_marks("loose", [Mark::new("bold")]),
        p("omega"),
    ])
}

/// Apply `changes` through a transaction and check the recorded inverse (and
/// `apply_inverting`'s) restores `base`, matches `invert`, and that the
/// inverse's own inverse redoes the edit.
fn check(base: &Node, changes: Vec<Change>) {
    let mut tx_doc = base.clone();
    let (fwd, undo) = {
        let mut tx = tx_doc.transform();
        for c in changes.clone() {
            tx.apply_change(c).unwrap();
        }
        let peek = tx.inverse();
        let out = tx.finish_with_inverse();
        assert_eq!(peek, out.1);
        out
    };
    assert_eq!(fwd, changes);
    let edited = tx_doc.clone();

    // Same result as plain apply.
    let mut plain = base.clone();
    apply(&mut plain, &changes).unwrap();
    assert_eq!(plain, edited, "transform result differs from apply");

    // Undo restores, structurally and in serialized form.
    let mut back = edited.clone();
    apply(&mut back, &undo).unwrap();
    assert_eq!(&back, base, "recorded inverse did not restore: {undo:?}");
    assert_eq!(
        serde_json::to_value(&back).unwrap(),
        serde_json::to_value(base).unwrap()
    );

    // The diff-based invert agrees on the effect.
    let reference = base.invert(&changes).unwrap();
    let mut back_ref = edited.clone();
    apply(&mut back_ref, &reference).unwrap();
    assert_eq!(back_ref, back);

    // apply_inverting: consume the undo, get the redo; both directions exact.
    let mut tree = edited.clone();
    let redo = tree.apply_inverting(undo).unwrap();
    assert_eq!(&tree, base);
    let again = tree.apply_inverting(redo).unwrap();
    assert_eq!(tree, edited);
    tree.apply_inverting(again).unwrap();
    assert_eq!(&tree, base);
}

fn one(base: &Node, change: Change) {
    check(base, vec![change]);
}

#[test]
fn set_attr_overwrite_and_new() {
    let d = doc();
    one(
        &d,
        Change::SetAttr {
            path: vec![0],
            key: "level".into(),
            value: json!(2),
        },
    );
    one(
        &d,
        Change::SetAttr {
            path: vec![1],
            key: "textAlign".into(),
            value: json!("center"),
        },
    );
}

#[test]
fn set_attr_on_present_empty_attrs() {
    let mut d = doc();
    d.node_at_mut(&[1]).unwrap().attrs = Some(Map::new());
    one(
        &d,
        Change::SetAttr {
            path: vec![1],
            key: "x".into(),
            value: json!(1),
        },
    );
}

#[test]
fn remove_attr_present_absent_last() {
    let d = doc()
        .with_attr("a", 1)
        .with_attr("b", 2)
        .with_attr("c", json!({"nested": [1, 2]}));
    for key in ["a", "b", "c", "missing"] {
        one(
            &d,
            Change::RemoveAttr {
                path: vec![],
                key: key.into(),
            },
        );
    }
    // Removing the only attr normalizes attrs to absent; undo restores it.
    one(
        &doc(),
        Change::RemoveAttr {
            path: vec![0],
            key: "level".into(),
        },
    );
    // No attrs at all: no-op.
    one(
        &doc(),
        Change::RemoveAttr {
            path: vec![1],
            key: "level".into(),
        },
    );
    let mut empty = doc();
    empty.node_at_mut(&[1]).unwrap().attrs = Some(Map::new());
    one(
        &empty,
        Change::RemoveAttr {
            path: vec![1],
            key: "x".into(),
        },
    );
}

#[test]
fn set_text_and_clear() {
    let d = doc();
    one(
        &d,
        Change::SetText {
            path: vec![1, 0],
            text: Some("ALPHA".into()),
        },
    );
    one(
        &d,
        Change::SetText {
            path: vec![1, 0],
            text: None,
        },
    );
    one(
        &d,
        Change::SetText {
            path: vec![1],
            text: Some("on an element".into()),
        },
    );
}

#[test]
fn splice_text_variants() {
    let d = doc();
    for (from, len_del, insert) in [
        (0, 0, "pre-"),
        (5, 0, "-post"),
        (1, 3, ""),
        (1, 3, "ŁÓŻ"),
        (0, 5, "replaced"),
        (2, 0, ""),
    ] {
        one(
            &d,
            Change::SpliceText {
                path: vec![1, 0],
                from,
                len_del,
                insert: insert.into(),
            },
        );
    }
    // Absent text: a real splice materializes it, undo clears it again; a
    // no-op splice keeps it absent.
    for insert in ["new", ""] {
        one(
            &d,
            Change::SpliceText {
                path: vec![1],
                from: 0,
                len_del: 0,
                insert: insert.into(),
            },
        );
    }
}

#[test]
fn set_marks_variants() {
    let d = doc();
    one(
        &d,
        Change::SetMarks {
            path: vec![3],
            marks: Some(vec![
                Mark::new("italic"),
                Mark::new("link").attr("href", "x"),
            ]),
        },
    );
    one(
        &d,
        Change::SetMarks {
            path: vec![3],
            marks: None,
        },
    );
    one(
        &d,
        Change::SetMarks {
            path: vec![1, 0],
            marks: Some(vec![Mark::new("code")]),
        },
    );
}

#[test]
fn extra_fields() {
    let mut d = doc();
    d.node_at_mut(&[1])
        .unwrap()
        .extra
        .insert("x-id".into(), json!("abc"));
    one(
        &d,
        Change::SetExtra {
            path: vec![1],
            key: "x-id".into(),
            value: json!("def"),
        },
    );
    one(
        &d,
        Change::SetExtra {
            path: vec![1],
            key: "fresh".into(),
            value: json!([1]),
        },
    );
    one(
        &d,
        Change::RemoveExtra {
            path: vec![1],
            key: "x-id".into(),
        },
    );
    one(
        &d,
        Change::RemoveExtra {
            path: vec![1],
            key: "missing".into(),
        },
    );
}

#[test]
fn insert_variants() {
    let d = doc();
    for index in [0, 2, 5] {
        one(
            &d,
            Change::Insert {
                path: vec![],
                index,
                node: p("new"),
            },
        );
    }
    // Into a leaf without content (content becomes Some, undo makes it absent).
    one(
        &d,
        Change::Insert {
            path: vec![3],
            index: 0,
            node: p("child of text"),
        },
    );
    // Into a present-but-empty list.
    let mut e = doc();
    e.node_at_mut(&[1]).unwrap().content = Some(Vec::new());
    one(
        &e,
        Change::Insert {
            path: vec![1],
            index: 0,
            node: Node::text("x"),
        },
    );
}

#[test]
fn remove_variants() {
    let d = doc();
    for index in [0, 2, 4] {
        one(
            &d,
            Change::Remove {
                path: vec![],
                index,
            },
        );
    }
    // Last child: content normalizes to absent, undo brings it back.
    one(
        &d,
        Change::Remove {
            path: vec![1],
            index: 0,
        },
    );
}

#[test]
fn replace_variants() {
    let d = doc();
    one(
        &d,
        Change::Replace {
            path: vec![2],
            node: p("was a list"),
        },
    );
    one(
        &d,
        Change::Replace {
            path: vec![],
            node: Node::element("doc"),
        },
    );
    one(
        &d,
        Change::Replace {
            path: vec![2, 1, 0, 0],
            node: Node::text("deep"),
        },
    );
}

#[test]
fn move_variants() {
    let d = doc();
    for (from, to) in [(0, 4), (4, 0), (1, 3), (3, 1), (2, 2)] {
        one(
            &d,
            Change::Move {
                path: vec![],
                from,
                to,
            },
        );
    }
}

#[test]
fn mixed_sequences() {
    let d = doc();
    check(
        &d,
        vec![
            Change::Insert {
                path: vec![],
                index: 1,
                node: p("x"),
            },
            Change::SetAttr {
                path: vec![1],
                key: "k".into(),
                value: json!(true),
            },
            Change::Move {
                path: vec![],
                from: 1,
                to: 4,
            },
            Change::Remove {
                path: vec![],
                index: 0,
            },
            Change::SpliceText {
                path: vec![0, 0],
                from: 0,
                len_del: 1,
                insert: "A".into(),
            },
            Change::Replace {
                path: vec![1, 0],
                node: Node::element("listItem").with_child(p("r")),
            },
            Change::SetMarks {
                path: vec![2],
                marks: None,
            },
        ],
    );
    // Diff-produced lists (moves, splices) round-trip through apply_inverting.
    let mut b = d.clone();
    b.node_at_mut(&[])
        .unwrap()
        .content
        .as_mut()
        .unwrap()
        .swap(0, 4);
    b.node_at_mut(&[2, 0, 0, 0]).unwrap().text = Some("uno".into());
    check(&d, d.diff(&b));
}

#[test]
fn structural_builders_record_scoped_inverse() {
    let base = doc();
    type Op = fn(&mut tiptap_rusty_parser::Transform<'_>);
    let ops: Vec<Op> = vec![
        |t| {
            t.wrap(vec![1], "blockquote", None).unwrap();
        },
        |t| {
            t.wrap_range(BlockRange::new(vec![], 0, 2), "section", None)
                .unwrap();
        },
        |t| {
            t.lift(vec![2, 1]).unwrap();
        },
        |t| {
            t.lift(vec![2, 0, 0]).unwrap();
        },
        |t| {
            t.join_blocks(vec![], 2).unwrap();
        },
        |t| {
            t.split_block(vec![2], 1, 0).unwrap();
        },
        |t| {
            t.split_block(vec![2, 1, 0], 0, 2).unwrap();
        },
        |t| {
            t.split_block_at(vec![1], Position::new(0, 2), 0).unwrap();
        },
        |t| {
            t.set_block_type(vec![1], "heading", None).unwrap();
        },
        |t| {
            t.insert_text_at(vec![1], Position::new(0, 1), "ins", None)
                .unwrap();
        },
        |t| {
            t.add_mark_range_in(
                vec![1],
                tiptap_rusty_parser::Range::new(Position::new(0, 1), Position::new(0, 3)),
                Mark::new("bold"),
            )
            .unwrap();
        },
    ];
    for (i, op) in ops.into_iter().enumerate() {
        let mut d = base.clone();
        let (fwd, undo) = {
            let mut t = d.transform();
            op(&mut t);
            // Chain a second op on top so scopes compose.
            t.set_attr(vec![0], "after", i).unwrap();
            t.finish_with_inverse()
        };
        let mut replay = base.clone();
        apply(&mut replay, &fwd).unwrap();
        assert_eq!(replay, d, "op {i}: forward log does not replay");
        let mut back = d.clone();
        apply(&mut back, &undo).unwrap();
        assert_eq!(back, base, "op {i}: inverse does not restore");
    }
}

#[test]
fn failed_structural_op_leaves_tree_untouched() {
    let base = doc();
    let mut d = base.clone();
    let mut t = d.transform();
    assert!(t.lift(vec![1]).is_err());
    assert!(t.join_blocks(vec![], 0).is_err());
    assert!(t.split_block(vec![9], 0, 0).is_err());
    assert!(t.wrap(vec![7, 7], "x", None).is_err());
    let (fwd, undo) = t.finish_with_inverse();
    assert!(fwd.is_empty() && undo.is_empty());
    assert_eq!(d, base);
}

#[test]
fn rollback_restores_after_error() {
    let base = doc();
    let mut d = base.clone();
    let mut t = d.transform();
    t.set_attr(vec![0], "level", 3).unwrap();
    t.remove(vec![], 1).unwrap();
    t.wrap(vec![0], "blockquote", None).unwrap();
    assert!(t.remove(vec![42], 0).is_err());
    t.rollback();
    assert_eq!(d, base);
}

#[test]
fn apply_inverting_is_atomic_on_error() {
    let base = doc();
    let mut d = base.clone();
    let err = d
        .apply_inverting(vec![
            Change::Remove {
                path: vec![],
                index: 0,
            },
            Change::SetText {
                path: vec![0, 0],
                text: Some("x".into()),
            },
            Change::Remove {
                path: vec![99],
                index: 0,
            },
        ])
        .unwrap_err();
    assert_eq!(err.path, vec![99]);
    assert_eq!(d, base);
}

#[test]
fn errors_match_apply() {
    let base = doc();
    let bad = [
        Change::Insert {
            path: vec![],
            index: 9,
            node: p("x"),
        },
        Change::Remove {
            path: vec![],
            index: 9,
        },
        Change::Replace {
            path: vec![9],
            node: p("x"),
        },
        Change::Move {
            path: vec![],
            from: 9,
            to: 0,
        },
        Change::SpliceText {
            path: vec![1, 0],
            from: 3,
            len_del: 9,
            insert: String::new(),
        },
        Change::SetAttr {
            path: vec![9, 9],
            key: "k".into(),
            value: json!(1),
        },
    ];
    for c in bad {
        let want = apply(&mut base.clone(), std::slice::from_ref(&c)).unwrap_err();
        let mut d = base.clone();
        let got = d.apply_inverting(vec![c]).unwrap_err();
        assert_eq!(got, want);
        assert_eq!(d, base);
    }
}
