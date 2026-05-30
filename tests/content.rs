//! Content-expression validation tests (cardinality + ordering + groups).

use tiptap_rusty_parser::{ContentExpr, Document, NodeSpec, Schema, ViolationKind};

/// Validate a `doc` whose content rule is `expr` against children of the given
/// types. `paragraph`/`heading` are in group `block`; `text`/`image` in `inline`.
fn is_valid(expr: &str, children: &[&str]) -> bool {
    let schema = Schema::new()
        .node("doc", NodeSpec::new().content_match(expr))
        .node("paragraph", NodeSpec::new().group("block"))
        .node("heading", NodeSpec::new().group("block"))
        .node("image", NodeSpec::new().group("inline"))
        .node("text", NodeSpec::new().group("inline"));
    let content: Vec<_> = children
        .iter()
        .map(|t| serde_json::json!({ "type": t }))
        .collect();
    let doc =
        Document::from_value(serde_json::json!({ "type": "doc", "content": content })).unwrap();
    doc.is_valid(&schema)
}

#[test]
fn plus_star_opt() {
    assert!(!is_valid("paragraph+", &[]));
    assert!(is_valid("paragraph+", &["paragraph"]));
    assert!(is_valid("paragraph+", &["paragraph", "paragraph"]));
    assert!(!is_valid("paragraph+", &["paragraph", "heading"]));

    assert!(is_valid("paragraph*", &[]));
    assert!(is_valid("paragraph*", &["paragraph", "paragraph"]));

    assert!(is_valid("paragraph?", &[]));
    assert!(is_valid("paragraph?", &["paragraph"]));
    assert!(!is_valid("paragraph?", &["paragraph", "paragraph"]));
}

#[test]
fn ranges() {
    assert!(!is_valid("heading{1,3}", &[]));
    assert!(is_valid("heading{1,3}", &["heading"]));
    assert!(is_valid("heading{1,3}", &["heading", "heading", "heading"]));
    assert!(!is_valid(
        "heading{1,3}",
        &["heading", "heading", "heading", "heading"]
    ));

    assert!(!is_valid("heading{2,}", &["heading"]));
    assert!(is_valid("heading{2,}", &["heading", "heading"]));
    assert!(is_valid("heading{2,}", &["heading", "heading", "heading"]));
}

#[test]
fn alternation_sequence_groups() {
    assert!(is_valid(
        "(paragraph | heading)*",
        &["heading", "paragraph", "heading"]
    ));
    assert!(!is_valid("(paragraph | heading)*", &["text"]));

    // ordering: heading then one-or-more paragraphs
    assert!(is_valid("heading paragraph+", &["heading", "paragraph"]));
    assert!(is_valid(
        "heading paragraph+",
        &["heading", "paragraph", "paragraph"]
    ));
    assert!(!is_valid("heading paragraph+", &["heading"]));
    assert!(!is_valid("heading paragraph+", &["paragraph", "heading"]));

    // groups: `block` matches paragraph + heading (both in group), not text
    assert!(is_valid("block+", &["paragraph", "heading"]));
    assert!(!is_valid("block+", &["text"]));
    assert!(!is_valid("block+", &[]));

    // unknown name matches nothing
    assert!(!is_valid("widget+", &["paragraph"]));
}

#[test]
fn invalid_content_violation_shape_and_path() {
    let schema = Schema::new()
        .node("doc", NodeSpec::new().content_match("paragraph+"))
        .node("paragraph", NodeSpec::new().content_match("text+"))
        .node("text", NodeSpec::new());

    // doc has a paragraph, but that paragraph has no text -> InvalidContent at [0]
    let doc =
        Document::from_json_str(r#"{"type":"doc","content":[{"type":"paragraph"}]}"#).unwrap();
    let v = doc.validate(&schema);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].path, vec![0]);
    assert!(matches!(
        &v[0].kind,
        ViolationKind::InvalidContent { parent, expr } if parent == "paragraph" && expr == "text+"
    ));

    // empty doc -> InvalidContent at root
    let empty = Document::from_json_str(r#"{"type":"doc"}"#).unwrap();
    let v = empty.validate(&schema);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].path, Vec::<usize>::new());
}

#[test]
fn parse_errors_and_ok() {
    for ok in [
        "paragraph+",
        "heading{1,3}",
        "(a | b) c*",
        "",
        "  ",
        "block+",
    ] {
        assert!(ContentExpr::parse(ok).is_ok(), "should parse: {ok:?}");
    }
    // Note: `a |` is valid (a choice with an empty branch, like `a?`).
    for bad in ["(a", "a)", "{2,1}", "{}", "a**", "heading{2000}"] {
        assert!(ContentExpr::parse(bad).is_err(), "should fail: {bad:?}");
    }
}

#[test]
fn serde_roundtrip_and_invalid_expr() {
    let json = r#"{"nodes":{
        "doc":{"content":"block+"},
        "paragraph":{"group":"block","content":["text"]},
        "heading":{"group":"block"},
        "text":{}
    }}"#;
    let schema = Schema::from_json_str(json).unwrap();
    let good =
        Document::from_json_str(r#"{"type":"doc","content":[{"type":"paragraph"}]}"#).unwrap();
    let bad = Document::from_json_str(r#"{"type":"doc","content":[{"type":"text"}]}"#).unwrap();
    assert!(good.is_valid(&schema));
    assert!(!bad.is_valid(&schema));

    // re-serialize, re-parse, identical verdicts
    let reser = serde_json::to_string(&schema).unwrap();
    let schema2 = Schema::from_json_str(&reser).unwrap();
    assert_eq!(good.validate(&schema2), good.validate(&schema));
    assert_eq!(bad.validate(&schema2), bad.validate(&schema));

    // an invalid expression in JSON is a load-time error
    assert!(Schema::from_json_str(r#"{"nodes":{"doc":{"content":"(a"}}}"#).is_err());
}

#[test]
fn builder_matches_json() {
    let built = Schema::new()
        .node("doc", NodeSpec::new().content_match("paragraph+"))
        .node("paragraph", NodeSpec::new());
    let loaded =
        Schema::from_json_str(r#"{"nodes":{"doc":{"content":"paragraph+"},"paragraph":{}}}"#)
            .unwrap();
    let doc = Document::from_json_str(r#"{"type":"doc"}"#).unwrap();
    assert_eq!(doc.validate(&built), doc.validate(&loaded));
}

#[test]
fn array_form_still_disallowed_child() {
    // Backward-compat: array content emits DisallowedChild, not InvalidContent.
    let schema = Schema::from_json_str(
        r#"{"nodes":{"doc":{"content":["paragraph"]},"paragraph":{},"heading":{}}}"#,
    )
    .unwrap();
    let doc = Document::from_json_str(r#"{"type":"doc","content":[{"type":"heading"}]}"#).unwrap();
    let v = doc.validate(&schema);
    assert_eq!(v.len(), 1);
    assert!(matches!(&v[0].kind, ViolationKind::DisallowedChild { .. }));
}
