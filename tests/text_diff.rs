//! Character-level (inline) text diff + `SpliceText` apply/invert tests.

use tiptap_rusty_parser::{
    diff_text, Change, DiffGranularity, DiffOptions, Node, SegKind, TextSegment,
};

fn seg(kind: SegKind, text: &str) -> TextSegment {
    TextSegment {
        kind,
        text: text.to_owned(),
    }
}

fn doc(text: &str) -> Node {
    Node::element("doc").with_child(Node::element("paragraph").with_child(Node::text(text)))
}

// ---- diff_text minimality ----------------------------------------------

#[test]
fn pure_insert_keeps_both_sides() {
    // "ac" -> "abc": keep "a", insert "b", keep "c".
    assert_eq!(
        diff_text("ac", "abc"),
        vec![
            seg(SegKind::Keep, "a"),
            seg(SegKind::Insert, "b"),
            seg(SegKind::Keep, "c"),
        ]
    );
}

#[test]
fn pure_delete() {
    assert_eq!(
        diff_text("abc", "ac"),
        vec![
            seg(SegKind::Keep, "a"),
            seg(SegKind::Delete, "b"),
            seg(SegKind::Keep, "c"),
        ]
    );
}

#[test]
fn replace_middle() {
    // common prefix "a", suffix "z"; middle "b" -> "X".
    assert_eq!(
        diff_text("abz", "aXz"),
        vec![
            seg(SegKind::Keep, "a"),
            seg(SegKind::Delete, "b"),
            seg(SegKind::Insert, "X"),
            seg(SegKind::Keep, "z"),
        ]
    );
}

#[test]
fn empty_sides() {
    assert_eq!(diff_text("", ""), vec![]);
    assert_eq!(diff_text("", "hi"), vec![seg(SegKind::Insert, "hi")]);
    assert_eq!(diff_text("hi", ""), vec![seg(SegKind::Delete, "hi")]);
}

#[test]
fn identical_is_one_keep() {
    assert_eq!(diff_text("same", "same"), vec![seg(SegKind::Keep, "same")]);
}

#[test]
fn multibyte_scalars() {
    // Emoji + accented char: edits land on scalar boundaries, never mid-byte.
    let segs = diff_text("café☕", "cafe☕!");
    let rebuilt: String = segs
        .iter()
        .filter(|s| s.kind != SegKind::Delete)
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(rebuilt, "cafe☕!");
}

// ---- diff_with(Inline) + apply / invert ---------------------------------

fn inline() -> DiffOptions {
    DiffOptions {
        text: DiffGranularity::Inline,
    }
}

#[test]
fn inline_emits_splicetext_not_settext() {
    let a = doc("The quick brown fox");
    let b = doc("The quick red fox");
    let changes = a.diff_with(&b, &inline());
    assert!(changes
        .iter()
        .all(|c| matches!(c, Change::SpliceText { .. })));
    assert!(!changes.is_empty());
}

#[test]
fn block_emits_settext() {
    let a = doc("hello");
    let b = doc("hxllo");
    let changes = a.diff(&b); // default Block
    assert!(matches!(changes.as_slice(), [Change::SetText { .. }]));
}

#[test]
fn inline_apply_reproduces_target() {
    let a = doc("the lazy dog sleeps soundly here");
    let b = doc("the LAZY cat sleeps here now");
    let changes = a.diff_with(&b, &inline());
    let mut c = a.clone();
    c.apply(&changes).unwrap();
    assert_eq!(c, b);
}

#[test]
fn inline_round_trip_inverts() {
    let a = doc("alpha beta gamma");
    let b = doc("alpha BETA delta");
    let forward = a.diff_with(&b, &inline());
    let undo = a.invert(&forward).unwrap();
    let mut c = b.clone();
    c.apply(&undo).unwrap();
    assert_eq!(c, a);
}

#[test]
fn inline_multibyte_apply() {
    let a = doc("naïve café résumé");
    let b = doc("naive café resume!");
    let changes = a.diff_with(&b, &inline());
    let mut c = a.clone();
    c.apply(&changes).unwrap();
    assert_eq!(c, b);
}

// ---- smart granularity ---------------------------------------------------

fn smart(t: f32) -> DiffOptions {
    DiffOptions {
        text: DiffGranularity::Smart {
            replace_threshold: t,
        },
    }
}

#[test]
fn smart_small_edit_splices() {
    // tiny change under threshold -> SpliceText islands.
    let a = doc("a long stable sentence that barely changes here");
    let b = doc("a long stable sentence that barely changed here");
    let changes = a.diff_with(&b, &smart(0.5));
    assert!(changes
        .iter()
        .all(|c| matches!(c, Change::SpliceText { .. })));
}

#[test]
fn smart_large_edit_falls_back_to_settext() {
    // wholesale rewrite over threshold -> single SetText.
    let a = doc("aaaaaa");
    let b = doc("zzzzzzzzz");
    let changes = a.diff_with(&b, &smart(0.5));
    assert!(matches!(changes.as_slice(), [Change::SetText { .. }]));
}

#[test]
fn smart_round_trips() {
    for (x, y) in [("hello world", "hello there"), ("abc", "xyzdef")] {
        let (a, b) = (doc(x), doc(y));
        let fwd = a.diff_with(&b, &smart(0.5));
        let mut c = a.clone();
        c.apply(&fwd).unwrap();
        assert_eq!(c, b);
        let undo = a.invert(&fwd).unwrap();
        let mut d = b.clone();
        d.apply(&undo).unwrap();
        assert_eq!(d, a);
    }
}

// ---- splice apply bounds -------------------------------------------------

#[test]
fn out_of_range_splice_errors() {
    let mut d = doc("hi");
    let bad = vec![Change::SpliceText {
        path: vec![0, 0],
        from: 5,
        len_del: 1,
        insert: "x".into(),
    }];
    assert!(d.apply(&bad).is_err());
}

#[test]
fn end_splice_appends() {
    let mut d = doc("hi");
    let ok = vec![Change::SpliceText {
        path: vec![0, 0],
        from: 2,
        len_del: 0,
        insert: "!".into(),
    }];
    d.apply(&ok).unwrap();
    assert_eq!(d, doc("hi!"));
}
