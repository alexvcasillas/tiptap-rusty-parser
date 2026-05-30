//! `wasm-pack test --node bindings/wasm` runs these in a Node WASM runtime.

use serde_wasm_bindgen::to_value;
use tiptap_rusty_parser_wasm::TiptapDoc;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

const DOC: &str = r#"{"type":"doc","content":[
  {"type":"heading","content":[{"type":"text","text":"Title"}]},
  {"type":"paragraph","content":[{"type":"text","text":"Hello world"}]}
]}"#;

fn path(p: &[usize]) -> JsValue {
    to_value(&p.to_vec()).unwrap()
}

#[wasm_bindgen_test]
fn diff_then_apply() {
    let a = TiptapDoc::from_json_string(DOC).unwrap();
    let b = TiptapDoc::from_json_string(
        r#"{"type":"doc","content":[
          {"type":"heading","attrs":{"level":1},"content":[{"type":"text","text":"Title"}]},
          {"type":"paragraph","content":[{"type":"text","text":"Changed"}]},
          {"type":"paragraph","content":[{"type":"text","text":"Appended"}]}
        ]}"#,
    )
    .unwrap();

    let changes = a.diff(&b).unwrap();
    // Apply the changes onto a fresh copy of A and expect B's JSON.
    let mut got = TiptapDoc::from_json_string(DOC).unwrap();
    got.apply_changes(changes).unwrap();
    assert_eq!(got.to_json_string().unwrap(), b.to_json_string().unwrap());
}

#[wasm_bindgen_test]
fn diff_shape_has_op() {
    let a = TiptapDoc::from_json_string(DOC).unwrap();
    let b = TiptapDoc::from_json_string(
        r#"{"type":"doc","content":[
          {"type":"heading","content":[{"type":"text","text":"Title"}]},
          {"type":"paragraph","content":[{"type":"text","text":"Hello there"}]}
        ]}"#,
    )
    .unwrap();
    let changes = a.diff(&b).unwrap();
    // Deserialize the JS array back into tagged Rust enums to confirm the shape.
    let parsed: Vec<serde_json::Value> = serde_wasm_bindgen::from_value(changes).unwrap();
    assert!(!parsed.is_empty());
    assert!(parsed.iter().all(|c| c.get("op").is_some()));
    assert!(parsed
        .iter()
        .any(|c| c.get("op") == Some(&serde_json::json!("setText"))));
}

#[wasm_bindgen_test]
fn roundtrip_and_text() {
    let doc = TiptapDoc::from_json_string(DOC).unwrap();
    assert_eq!(doc.text_content(), "TitleHello world");
    assert_eq!(doc.word_count(), 3);
    let s = doc.to_json_string().unwrap();
    assert!(s.contains("\"type\":\"heading\""));
}

#[wasm_bindgen_test]
fn paths_and_node_at() {
    let doc = TiptapDoc::from_json_string(DOC).unwrap();
    let paths = doc.paths_by_type("text").unwrap();
    let paths: Vec<Vec<usize>> = serde_wasm_bindgen::from_value(paths).unwrap();
    assert_eq!(paths.len(), 2);
    assert!(doc.node_at(path(&[0, 0])).unwrap().is_object());
    assert_eq!(doc.child_count(path(&[1])).unwrap(), Some(1));
}

#[wasm_bindgen_test]
fn mutate_via_path() {
    let mut doc = TiptapDoc::from_json_string(DOC).unwrap();
    // set heading level
    doc.set_attr(path(&[0]), "level".into(), JsValue::from_f64(1.0))
        .unwrap();
    // bold the heading's text node
    assert!(doc
        .add_mark(path(&[0, 0]), "bold".into(), JsValue::UNDEFINED)
        .unwrap());
    let s = doc.to_json_string().unwrap();
    assert!(s.contains("\"level\":1"));
    assert!(s.contains("\"type\":\"bold\""));
}

#[wasm_bindgen_test]
fn validate_via_schema() {
    let doc =
        TiptapDoc::from_json_string(r#"{"type":"doc","content":[{"type":"widget"}]}"#).unwrap();
    let schema = to_value(&serde_json::json!({
        "nodes": { "doc": { "content": ["paragraph"] } }
    }))
    .unwrap();
    assert!(!doc.is_valid(schema.clone()).unwrap());
    let violations = doc.validate(schema).unwrap();
    let violations: Vec<serde_json::Value> = serde_wasm_bindgen::from_value(violations).unwrap();
    assert!(!violations.is_empty());
}
