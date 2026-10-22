//! Long child lists: the diff aligns them with Myers' O((m+n)·D) LCS over
//! interned equality classes (above 256×256 cells), so a few scattered edits in
//! a 10k-block document stay cheap and the patch stays minimal.

use serde_json::json;
use tiptap_rusty_parser::{apply, Change, Node};

fn long_doc(n: usize) -> Node {
    Node::element("doc").with_children((0..n).map(|i| {
        Node::element("paragraph")
            .with_attr("i", i as u64)
            .with_child(Node::text(format!("p{i}")))
    }))
}

fn roundtrip(a: &Node, b: &Node) -> Vec<Change> {
    let changes = a.diff(b);
    let mut got = a.clone();
    apply(&mut got, &changes).unwrap();
    assert_eq!(&got, b);
    let undo = a.invert(&changes).unwrap();
    let mut back = b.clone();
    apply(&mut back, &undo).unwrap();
    assert_eq!(&back, a);
    changes
}

#[test]
fn scattered_edits_are_minimal_and_roundtrip() {
    let a = long_doc(2000);
    let mut b = a.clone();
    {
        let kids = b.content.as_mut().unwrap();
        kids.remove(1500);
        kids.insert(
            1000,
            Node::element("paragraph").with_child(Node::text("new")),
        );
        kids[500].content.as_mut().unwrap()[0].text = Some("edited".into());
        kids.remove(10);
        let moved = kids.remove(1800);
        kids.insert(3, moved);
    }
    let changes = roundtrip(&a, &b);
    assert!(changes.len() <= 8, "{} changes: {changes:?}", changes.len());
    assert!(changes.iter().any(|c| matches!(c, Change::Move { .. })));
}

#[test]
fn attr_order_and_signed_zero_count_as_equal() {
    // Map and float equality are order/sign-insensitive; interning must agree.
    let mut a = long_doc(600);
    let mut b = a.clone();
    for k in a.content.as_mut().unwrap().iter_mut() {
        k.attrs.as_mut().unwrap().insert("z".into(), json!(-0.0));
    }
    for k in b.content.as_mut().unwrap().iter_mut() {
        let attrs = k.attrs.as_mut().unwrap();
        let i = attrs.remove("i").unwrap();
        attrs.insert("z".into(), json!(0.0));
        attrs.insert("i".into(), i);
    }
    assert_eq!(a, b);
    assert!(a.diff(&b).is_empty());
    // One real change among equal-but-reordered siblings is still found alone.
    b.content.as_mut().unwrap()[300].content.as_mut().unwrap()[0].text = Some("x".into());
    assert_eq!(roundtrip(&a, &b).len(), 1);
}

#[test]
fn duplicates_and_many_edits_roundtrip() {
    let dup = |n: usize| {
        Node::element("doc").with_children(
            (0..n).map(|i| Node::element("paragraph").with_child(Node::text(format!("{}", i % 7)))),
        )
    };
    // Many equal siblings: alignment and FIFO move pairing must stay exact.
    let a = dup(900);
    let mut b = a.clone();
    let kids = b.content.as_mut().unwrap();
    for k in 0..12 {
        let n = kids.remove(k * 70 + 5);
        kids.insert(k * 61 + 30, n);
        kids.insert(k * 50, Node::element("heading").with_child(Node::text("x")));
    }
    kids.truncate(850);
    roundtrip(&a, &b);
}

#[test]
fn completely_different_lists_roundtrip() {
    let a = long_doc(700);
    let b = Node::element("doc").with_children(
        (0..650).map(|i| Node::element("heading").with_child(Node::text(format!("h{i}")))),
    );
    roundtrip(&a, &b);
}

#[test]
fn reordered_equal_siblings_stay_linear() {
    // Many value-equal siblings reversed: the move settle used to cascade,
    // re-moving placed nodes (n=100 -> 1.3M Moves, n=200 never finished).
    let n = 900;
    let a = Node::element("doc").with_children(
        (0..n).map(|i| Node::element("paragraph").with_child(Node::text(format!("{}", i % 7)))),
    );
    let mut b = a.clone();
    b.content.as_mut().unwrap()[..n * 8 / 9].reverse();
    let t = std::time::Instant::now();
    let changes = roundtrip(&a, &b);
    let moves = changes
        .iter()
        .filter(|c| matches!(c, Change::Move { .. }))
        .count();
    assert!(moves <= n, "{moves} moves for {n} children");
    assert!(t.elapsed().as_secs() < 10, "took {:?}", t.elapsed());
}
