//! Change-list algebra: compose / compact / map_path.

use tiptap_rusty_parser::{apply, compact, compose, map_path, Change, Node};

fn p(text: &str) -> Node {
    Node::element("paragraph").with_child(Node::text(text))
}

fn doc(children: impl IntoIterator<Item = Node>) -> Node {
    Node::element("doc").with_children(children)
}

// ---- compose ------------------------------------------------------------

#[test]
fn compose_is_sequential_apply() {
    let base = doc([p("x")]);
    let a = base.diff(&doc([p("y")]));
    let mut mid = base.clone();
    apply(&mut mid, &a).unwrap();
    let b = mid.diff(&doc([p("z")]));

    let composed = compose(&a, &b);
    let mut viacompose = base.clone();
    apply(&mut viacompose, &composed).unwrap();

    let mut sequential = base.clone();
    apply(&mut sequential, &a).unwrap();
    apply(&mut sequential, &b).unwrap();

    assert_eq!(viacompose, sequential);
    assert_eq!(viacompose, doc([p("z")]));
}

// ---- compact ------------------------------------------------------------

#[test]
fn compact_keeps_last_set_text() {
    let c = vec![
        Change::SetText {
            path: vec![0],
            text: Some("a".into()),
        },
        Change::SetText {
            path: vec![0],
            text: Some("b".into()),
        },
    ];
    let out = compact(&c);
    assert_eq!(
        out,
        vec![Change::SetText {
            path: vec![0],
            text: Some("b".into())
        }]
    );
}

#[test]
fn compact_attr_set_then_remove_keeps_remove() {
    let c = vec![
        Change::SetAttr {
            path: vec![0],
            key: "k".into(),
            value: 1.into(),
        },
        Change::RemoveAttr {
            path: vec![0],
            key: "k".into(),
        },
    ];
    let out = compact(&c);
    assert_eq!(
        out,
        vec![Change::RemoveAttr {
            path: vec![0],
            key: "k".into()
        }]
    );
}

#[test]
fn compact_distinct_keys_preserved() {
    let c = vec![
        Change::SetText {
            path: vec![0],
            text: Some("t".into()),
        },
        Change::SetMarks {
            path: vec![0],
            marks: None,
        },
        Change::SetAttr {
            path: vec![1],
            key: "a".into(),
            value: 2.into(),
        },
    ];
    assert_eq!(compact(&c).len(), 3);
}

#[test]
fn compact_adjacent_insert_remove_cancels() {
    let c = vec![
        Change::Insert {
            path: vec![],
            index: 1,
            node: p("new"),
        },
        Change::Remove {
            path: vec![],
            index: 1,
        },
    ];
    assert!(compact(&c).is_empty());
}

#[test]
fn compact_structural_is_a_barrier() {
    // A Replace between two SetTexts on the same path must NOT coalesce them.
    let c = vec![
        Change::SetText {
            path: vec![0],
            text: Some("a".into()),
        },
        Change::Replace {
            path: vec![0],
            node: p("mid"),
        },
        Change::SetText {
            path: vec![0],
            text: Some("b".into()),
        },
    ];
    let out = compact(&c);
    assert_eq!(
        out.len(),
        3,
        "must not coalesce across a structural barrier"
    );
}

#[test]
fn compact_preserves_apply_result() {
    let base = doc([p("x"), p("y")]);
    let c = vec![
        Change::SetText {
            path: vec![0, 0],
            text: Some("1".into()),
        },
        Change::SetText {
            path: vec![0, 0],
            text: Some("2".into()),
        },
        Change::SetAttr {
            path: vec![1],
            key: "k".into(),
            value: 9.into(),
        },
    ];
    let mut a = base.clone();
    apply(&mut a, &c).unwrap();
    let mut b = base.clone();
    apply(&mut b, &compact(&c)).unwrap();
    assert_eq!(a, b);
    assert!(compact(&c).len() <= c.len());
}

// ---- map_path -----------------------------------------------------------

#[test]
fn map_path_insert_shifts() {
    let ch = vec![Change::Insert {
        path: vec![],
        index: 0,
        node: p("n"),
    }];
    assert_eq!(map_path(&[1, 0], &ch), Some(vec![2, 0]));
    // insert after the node: unchanged
    let ch2 = vec![Change::Insert {
        path: vec![],
        index: 2,
        node: p("n"),
    }];
    assert_eq!(map_path(&[1, 0], &ch2), Some(vec![1, 0]));
}

#[test]
fn map_path_remove() {
    // remove before -> shift down
    assert_eq!(
        map_path(
            &[2, 0],
            &[Change::Remove {
                path: vec![],
                index: 0
            }]
        ),
        Some(vec![1, 0])
    );
    // remove the node itself -> None
    assert_eq!(
        map_path(
            &[2, 0],
            &[Change::Remove {
                path: vec![],
                index: 2
            }]
        ),
        None
    );
    // remove after -> unchanged
    assert_eq!(
        map_path(
            &[2, 0],
            &[Change::Remove {
                path: vec![],
                index: 5
            }]
        ),
        Some(vec![2, 0])
    );
}

#[test]
fn map_path_move_tracks_node() {
    // The node itself moves from 0 to 2.
    assert_eq!(
        map_path(
            &[0, 1],
            &[Change::Move {
                path: vec![],
                from: 0,
                to: 2
            }]
        ),
        Some(vec![2, 1])
    );
    // A move of another sibling shifts our index as apply would.
    assert_eq!(
        map_path(
            &[3, 0],
            &[Change::Move {
                path: vec![],
                from: 0,
                to: 5
            }]
        ),
        Some(vec![2, 0])
    );
}

#[test]
fn map_path_replace_ancestor_drops() {
    assert_eq!(
        map_path(
            &[0, 1],
            &[Change::Replace {
                path: vec![0],
                node: p("x")
            }]
        ),
        None
    );
    // replace elsewhere -> unchanged
    assert_eq!(
        map_path(
            &[0, 1],
            &[Change::Replace {
                path: vec![1],
                node: p("x")
            }]
        ),
        Some(vec![0, 1])
    );
}

#[test]
fn map_path_field_ops_dont_move() {
    let ch = vec![
        Change::SetText {
            path: vec![0, 0],
            text: Some("z".into()),
        },
        Change::SetAttr {
            path: vec![],
            key: "k".into(),
            value: 1.into(),
        },
    ];
    assert_eq!(map_path(&[0, 0], &ch), Some(vec![0, 0]));
}

#[test]
fn map_path_empty_changes_identity() {
    assert_eq!(map_path(&[1, 2, 3], &[]), Some(vec![1, 2, 3]));
}

// ---- property fuzz ------------------------------------------------------

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
    if depth == 0 || rng.below(3) == 0 {
        if rng.below(2) == 0 {
            let words = ["a", "bb", "ccc"];
            n = n.with_child(Node::text(words[rng.below(words.len())]));
        }
        return n;
    }
    for _ in 0..rng.below(4) {
        n = n.with_child(random_tree(rng, depth - 1));
    }
    n
}

#[test]
fn fuzz_compose_and_compact_equivalence() {
    let mut rng = Rng(0xC0FF_EE12_3456_7890);
    for _ in 0..1000 {
        let a = random_tree(&mut rng, 3);
        let b = random_tree(&mut rng, 3);
        let c = random_tree(&mut rng, 3);

        let d1 = a.diff(&b);
        let mut mid = a.clone();
        apply(&mut mid, &d1).unwrap();
        let d2 = mid.diff(&c);

        // compose == sequential apply
        let mut viacompose = a.clone();
        apply(&mut viacompose, &compose(&d1, &d2)).unwrap();
        assert_eq!(viacompose, c, "compose mismatch");

        // compact preserves the apply result and never grows the list
        let cat: Vec<Change> = d1.iter().chain(d2.iter()).cloned().collect();
        let mut viacompact = a.clone();
        apply(&mut viacompact, &compact(&cat)).unwrap();
        assert_eq!(viacompact, c, "compact changed apply result");
        assert!(compact(&cat).len() <= cat.len());
    }
}

#[test]
fn fuzz_map_path_matches_post_apply_node() {
    // For a random edit a->b: a path that maps to `Some(q)` must resolve to a
    // real node in `b` (the surviving node's new location); `None` means it was
    // removed/replaced. `b == apply(a, diff(a, b))`, so `b` is the applied tree.
    let mut rng = Rng(0xBEEF_9988_7766);
    for _ in 0..1000 {
        let a = random_tree(&mut rng, 3);
        let b = random_tree(&mut rng, 3);
        let changes = a.diff(&b);

        for path in a.paths_to(|_| true).iter() {
            if let Some(mapped) = map_path(path, &changes) {
                assert!(
                    b.node_at(&mapped).is_some(),
                    "map_path returned {mapped:?} for {path:?} but it doesn't resolve in b\n a={a:?}\n b={b:?}\n changes={changes:?}"
                );
            }
        }
    }
}
