//! Empty-vs-absent container fidelity (`content`/`attrs`/`marks`: absent vs
//! `[]`/`{}`) through the studio's undo model: edit with a `Transform`, undo
//! with `invert(before, changes)` + `apply`, redo with `invert(after, undo)`.
//! Compared as serialized JSON so a stray `"content":[]` can't hide.

use serde_json::{json, Value};
use tiptap_rusty_parser::{apply, invert, Mark, Node, Transform};

fn node(v: &Value) -> Node {
    serde_json::from_value(v.clone()).unwrap()
}

fn json_of(n: &Node) -> Value {
    serde_json::to_value(n).unwrap()
}

/// Edit `base` with `f`, undo, redo; assert exact JSON at every step.
/// Returns the edited JSON for further assertions.
fn undo_redo(base: &Value, f: impl FnOnce(&mut Transform<'_>)) -> Value {
    let original = node(base);
    let mut doc = original.clone();
    let changes = {
        let mut tr = doc.transform();
        f(&mut tr);
        tr.finish()
    };
    let edited = json_of(&doc);
    let undo = invert(&original, &changes).unwrap();
    apply(&mut doc, &undo).unwrap();
    assert_eq!(&json_of(&doc), base, "undo lost shape; undo = {undo:?}");
    // redo = invert(edited, undo), as `DocumentState::undo` computes it.
    let redo = invert(&node(&edited), &undo).unwrap();
    apply(&mut doc, &redo).unwrap();
    assert_eq!(json_of(&doc), edited, "redo lost shape; redo = {redo:?}");
    edited
}

// ---- the reported repro ---------------------------------------------------

#[test]
fn insert_into_contentless_undoes_to_no_content_key() {
    let base = json!({"type":"paragraph"});
    let edited = undo_redo(&base, |tr| {
        tr.insert(vec![], 0, Node::text("x")).unwrap();
    });
    assert_eq!(
        edited,
        json!({"type":"paragraph","content":[{"type":"text","text":"x"}]})
    );
}

#[test]
fn insert_into_nested_contentless_undoes_to_no_content_key() {
    let base = json!({"type":"doc","content":[{"type":"paragraph"}]});
    undo_redo(&base, |tr| {
        tr.insert(vec![0], 0, Node::text("x")).unwrap();
    });
}

#[test]
fn insert_into_explicit_empty_undoes_to_empty_array() {
    let base = json!({"type":"doc","content":[{"type":"paragraph","content":[]}]});
    undo_redo(&base, |tr| {
        tr.insert(vec![0], 0, Node::text("x")).unwrap();
    });
}

#[test]
fn remove_last_child_of_originally_empty_node() {
    // `[]` -> insert -> remove (forward ends with no `content` key: removal
    // normalizes) -> undo must restore the explicit `[]`.
    let base = json!({"type":"doc","content":[{"type":"paragraph","content":[]}]});
    undo_redo(&base, |tr| {
        tr.insert(vec![0], 0, Node::text("x")).unwrap();
        tr.remove(vec![0], 0).unwrap();
    });
}

#[test]
fn remove_only_child_undoes_to_populated() {
    let base = json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"x"}]}]});
    let edited = undo_redo(&base, |tr| {
        tr.remove(vec![0], 0).unwrap();
    });
    assert_eq!(
        edited,
        json!({"type":"doc","content":[{"type":"paragraph"}]})
    );
}

// ---- marks / attrs --------------------------------------------------------

#[test]
fn set_marks_none_vs_empty() {
    for marks in [json!(null), json!([])] {
        let mut t = json!({"type":"text","text":"x"});
        if !marks.is_null() {
            t["marks"] = marks;
        }
        let base = json!({"type":"paragraph","content":[t]});
        undo_redo(&base, |tr| {
            tr.set_marks(vec![0], Some(vec![Mark::new("bold")]))
                .unwrap();
        });
        undo_redo(&base, |tr| {
            tr.set_marks(vec![0], None).unwrap();
        });
        undo_redo(&base, |tr| {
            tr.set_marks(vec![0], Some(Vec::new())).unwrap();
        });
    }
}

#[test]
fn set_and_remove_attr_none_vs_empty() {
    for attrs in [json!(null), json!({})] {
        let mut p = json!({"type":"paragraph"});
        if !attrs.is_null() {
            p["attrs"] = attrs;
        }
        let base = json!({"type":"doc","content":[p]});
        undo_redo(&base, |tr| {
            tr.set_attr(vec![0], "level", 1).unwrap();
        });
        undo_redo(&base, |tr| {
            tr.set_attr(vec![0], "level", 1).unwrap();
            tr.remove_attr(vec![0], "level").unwrap();
        });
    }
    // removing the last attr: forward drops the key, undo restores it.
    let base = json!({"type":"doc","content":[{"type":"heading","attrs":{"level":1}}]});
    let edited = undo_redo(&base, |tr| {
        tr.remove_attr(vec![0], "level").unwrap();
    });
    assert_eq!(edited, json!({"type":"doc","content":[{"type":"heading"}]}));
}

// ---- exhaustive shape x op matrix -------------------------------------------

fn shapes() -> Vec<Value> {
    let mut out = Vec::new();
    for content in [
        None,
        Some(json!([])),
        Some(json!([{"type":"text","text":"a"}])),
    ] {
        for attrs in [None, Some(json!({})), Some(json!({"k":1}))] {
            for marks in [None, Some(json!([])), Some(json!([{"type":"bold"}]))] {
                let mut n = json!({"type":"x"});
                if let Some(c) = &content {
                    n["content"] = c.clone();
                }
                if let Some(a) = &attrs {
                    n["attrs"] = a.clone();
                }
                if let Some(m) = &marks {
                    n["marks"] = m.clone();
                }
                out.push(n);
            }
        }
    }
    out
}

type Op = fn(&mut Transform<'_>, Vec<usize>, &Node);

fn ops() -> Vec<(&'static str, Op)> {
    vec![
        ("insert", |tr, p, _| {
            tr.insert(p, 0, Node::text("n")).unwrap();
        }),
        ("insert+remove", |tr, p, _| {
            tr.insert(p.clone(), 0, Node::text("n")).unwrap();
            tr.remove(p, 0).unwrap();
        }),
        ("remove-all", |tr, p, n| {
            for _ in 0..n.child_count() {
                tr.remove(p.clone(), 0).unwrap();
            }
        }),
        ("set_attr", |tr, p, _| {
            tr.set_attr(p, "z", 2).unwrap();
        }),
        ("set+remove_attr", |tr, p, _| {
            tr.set_attr(p.clone(), "z", 2).unwrap();
            tr.remove_attr(p, "z").unwrap();
        }),
        ("remove_attr k", |tr, p, _| {
            tr.remove_attr(p, "k").unwrap();
        }),
        ("set_marks some", |tr, p, _| {
            tr.set_marks(p, Some(vec![Mark::new("italic")])).unwrap();
        }),
        ("set_marks empty", |tr, p, _| {
            tr.set_marks(p, Some(Vec::new())).unwrap();
        }),
        ("set_marks none", |tr, p, _| {
            tr.set_marks(p, None).unwrap();
        }),
    ]
}

#[test]
fn shape_matrix_roundtrips_at_root_and_nested() {
    for shape in shapes() {
        let target = node(&shape);
        for (name, op) in ops() {
            // root
            let r = std::panic::catch_unwind(|| {
                undo_redo(&shape, |tr| op(tr, vec![], &target));
            });
            assert!(r.is_ok(), "root {name} on {shape}");
            // nested between siblings
            let base = json!({"type":"doc","content":[{"type":"s"}, shape.clone(), {"type":"s"}]});
            let r = std::panic::catch_unwind(|| {
                undo_redo(&base, |tr| op(tr, vec![1], &target));
            });
            assert!(r.is_ok(), "nested {name} on {shape}");
        }
    }
}
