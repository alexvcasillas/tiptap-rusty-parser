//! Structural diff/apply tests. The central invariant is the round-trip
//! property: `apply(&mut a.clone(), &a.diff(b)) == b`.

use serde_json::json;
use tiptap_rusty_parser::{apply, Change, Mark, Node};

/// Assert the round-trip property and return the change list for inspection.
fn roundtrip(a: &Node, b: &Node) -> Vec<Change> {
    let changes = a.diff(b);
    let mut got = a.clone();
    apply(&mut got, &changes).unwrap();
    assert_eq!(&got, b, "diff+apply did not reproduce target");
    changes
}

fn p(text: &str) -> Node {
    Node::element("paragraph").with_child(Node::text(text))
}

// ---- identity / fields --------------------------------------------------

#[test]
fn identical_is_empty() {
    let a = Node::element("doc").with_children([p("one"), p("two")]);
    let changes = roundtrip(&a, &a.clone());
    assert!(changes.is_empty());
}

#[test]
fn attr_add_change_remove() {
    let base = Node::element("heading");
    let with = base.clone().with_attr("level", 1);
    let changed = base.clone().with_attr("level", 2);

    let c = roundtrip(&base, &with);
    assert_eq!(
        c,
        vec![Change::SetAttr {
            path: vec![],
            key: "level".into(),
            value: json!(1)
        }]
    );
    roundtrip(&with, &changed); // change value
    let c = roundtrip(&with, &base); // remove last attr -> attrs back to None
    assert_eq!(
        c,
        vec![Change::RemoveAttr {
            path: vec![],
            key: "level".into()
        }]
    );
}

#[test]
fn text_some_none_transitions() {
    let mut a = Node::text("hello");
    let mut b = Node::text("world");
    roundtrip(&a, &b); // Some -> Some

    b.text = None;
    let c = roundtrip(&a, &b); // Some -> None (clear)
    assert_eq!(
        c,
        vec![Change::SetText {
            path: vec![],
            text: None
        }]
    );

    a.text = None;
    let b2 = Node::text("added");
    roundtrip(&a, &b2); // None -> Some
}

#[test]
fn marks_via_setmarks() {
    let plain = Node::text("x");
    let bold = Node::text("x").with_mark(Mark::new("bold"));
    let link_a = Node::text("x").with_mark(Mark::new("link").attr("href", "a"));
    let link_b = Node::text("x").with_mark(Mark::new("link").attr("href", "b"));

    // add
    let c = roundtrip(&plain, &bold);
    assert!(matches!(c.as_slice(), [Change::SetMarks { .. }]));
    // remove (back to None)
    let c = roundtrip(&bold, &plain);
    assert_eq!(
        c,
        vec![Change::SetMarks {
            path: vec![],
            marks: None
        }]
    );
    // change a mark's attr
    roundtrip(&link_a, &link_b);

    // reorder marks
    let ab = Node::text("x")
        .with_mark(Mark::new("bold"))
        .with_mark(Mark::new("italic"));
    let ba = Node::text("x")
        .with_mark(Mark::new("italic"))
        .with_mark(Mark::new("bold"));
    roundtrip(&ab, &ba);
}

// ---- children -----------------------------------------------------------

#[test]
fn child_insert_positions() {
    let two = Node::element("doc").with_children([p("a"), p("b")]);
    // start
    roundtrip(
        &two,
        &Node::element("doc").with_children([p("x"), p("a"), p("b")]),
    );
    // middle
    roundtrip(
        &two,
        &Node::element("doc").with_children([p("a"), p("x"), p("b")]),
    );
    // end
    roundtrip(
        &two,
        &Node::element("doc").with_children([p("a"), p("b"), p("x")]),
    );
    // into empty (content None -> Some)
    roundtrip(
        &Node::element("doc"),
        &Node::element("doc").with_child(p("a")),
    );
}

#[test]
fn child_remove_positions() {
    let three = Node::element("doc").with_children([p("a"), p("b"), p("c")]);
    roundtrip(
        &three,
        &Node::element("doc").with_children([p("b"), p("c")]),
    ); // start
    roundtrip(
        &three,
        &Node::element("doc").with_children([p("a"), p("c")]),
    ); // middle
    roundtrip(
        &three,
        &Node::element("doc").with_children([p("a"), p("b")]),
    ); // end
       // remove last child -> content back to None
    let one = Node::element("doc").with_child(p("a"));
    roundtrip(&one, &Node::element("doc"));
}

#[test]
fn interleaved_inserts_and_removes() {
    // a: [a b c d]   b: [x b d e]
    // keep b, d; remove a, c; insert x (front), e (end). Stresses the
    // live-list index contract.
    let a = Node::element("doc").with_children([p("a"), p("b"), p("c"), p("d")]);
    let b = Node::element("doc").with_children([p("x"), p("b"), p("d"), p("e")]);
    roundtrip(&a, &b);
}

#[test]
fn nested_change_leaves_siblings_untouched() {
    let a = Node::element("doc").with_children([p("keep"), p("old"), p("keep2")]);
    let b = Node::element("doc").with_children([p("keep"), p("new"), p("keep2")]);
    let changes = roundtrip(&a, &b);
    // Only the middle paragraph's text node changes -> one SetText at [1,0].
    assert_eq!(
        changes,
        vec![Change::SetText {
            path: vec![1, 0],
            text: Some("new".into())
        }]
    );
}

#[test]
fn aligned_modify_vs_type_replace() {
    // same type between anchors -> recurse (SetText), not remove+insert
    let a = Node::element("doc").with_children([p("a"), p("b")]);
    let b = Node::element("doc").with_children([p("a"), p("B")]);
    let c = roundtrip(&a, &b);
    assert!(matches!(c.as_slice(), [Change::SetText { .. }]));

    // different type -> Replace of the child
    let a2 =
        Node::element("doc").with_child(Node::element("paragraph").with_child(Node::text("t")));
    let b2 = Node::element("doc").with_child(Node::element("heading").with_child(Node::text("t")));
    let c2 = roundtrip(&a2, &b2);
    assert!(matches!(c2.as_slice(), [Change::Replace { path, .. }] if path == &vec![0]));
}

#[test]
fn node_type_replace_at_root_and_child() {
    // root replace (empty path)
    let a = Node::element("doc").with_child(p("x"));
    let b = Node::element("section").with_child(p("x"));
    let c = roundtrip(&a, &b);
    assert_eq!(
        c,
        vec![Change::Replace {
            path: vec![],
            node: b.clone()
        }]
    );
}

#[test]
fn long_list_single_middle_insert() {
    let kids_a: Vec<Node> = (0..200).map(|i| p(&format!("n{i}"))).collect();
    let mut kids_b = kids_a.clone();
    kids_b.insert(100, p("inserted"));
    let a = Node::element("doc").with_children(kids_a);
    let b = Node::element("doc").with_children(kids_b);
    let c = roundtrip(&a, &b);
    // Prefix/suffix trim + LCS should yield exactly one Insert.
    assert_eq!(
        c,
        vec![Change::Insert {
            path: vec![],
            index: 100,
            node: p("inserted")
        }]
    );
}

// ---- extra (unknown fields), lossless -----------------------------------

#[test]
fn extra_fields_roundtrip() {
    let mut a = Node::element("doc");
    a.extra.insert("docId".into(), json!("abc"));
    let mut b = Node::element("doc");
    b.extra.insert("docId".into(), json!("xyz")); // change
    b.extra.insert("version".into(), json!(3)); // add
    let c = roundtrip(&a, &b);
    assert!(c
        .iter()
        .any(|ch| matches!(ch, Change::SetExtra { key, .. } if key == "version")));

    // remove an extra
    roundtrip(&b, &a);
}

#[test]
fn extra_from_parsed_json() {
    use tiptap_rusty_parser::Document;
    let a = Document::from_json_str(
        r#"{"type":"doc","customField":1,"content":[{"type":"paragraph","textAlign":"left"}]}"#,
    )
    .unwrap();
    let b = Document::from_json_str(
        r#"{"type":"doc","customField":2,"content":[{"type":"paragraph","textAlign":"center"}]}"#,
    )
    .unwrap();
    let changes = a.diff(&b);
    let mut got = a.clone();
    got.apply(&changes).unwrap();
    assert_eq!(got, b);
}

// ---- hand-rolled fuzz (no extra deps) -----------------------------------

/// Tiny xorshift PRNG — deterministic, dependency-free.
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

fn random_tree(rng: &mut Rng, depth: usize) -> Node {
    let types = ["doc", "paragraph", "heading", "list", "item"];
    let mut n = Node::element(types[rng.below(types.len())]);
    if rng.below(2) == 0 {
        n = n.with_attr("k", rng.below(3) as i64);
    }
    if depth == 0 || rng.below(3) == 0 {
        // leaf-ish: maybe a text child
        if rng.below(2) == 0 {
            let words = ["a", "bb", "ccc", "dd"];
            n = n.with_child(Node::text(words[rng.below(words.len())]));
        }
        return n;
    }
    let count = rng.below(4);
    for _ in 0..count {
        n = n.with_child(random_tree(rng, depth - 1));
    }
    n
}

#[test]
fn fuzz_roundtrip() {
    let mut rng = Rng(0x9E3779B97F4A7C15);
    for _ in 0..2000 {
        let a = random_tree(&mut rng, 4);
        let b = random_tree(&mut rng, 4);
        let changes = a.diff(&b);
        let mut got = a.clone();
        apply(&mut got, &changes).unwrap();
        assert_eq!(got, b, "fuzz round-trip failed\n a={a:?}\n b={b:?}");
    }
}
