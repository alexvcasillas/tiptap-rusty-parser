use serde_json::json;
use tiptap_rusty_parser::Document;

const SAMPLE: &str = r#"{
  "type": "doc",
  "content": [
    {
      "type": "heading",
      "attrs": { "level": 1 },
      "content": [ { "type": "text", "text": "Title" } ]
    },
    {
      "type": "paragraph",
      "attrs": { "textAlign": "left" },
      "content": [
        { "type": "text", "text": "Hello " },
        {
          "type": "text",
          "text": "world",
          "marks": [
            { "type": "bold" },
            { "type": "link", "attrs": { "href": "https://tiptap.dev" } }
          ]
        }
      ]
    },
    {
      "type": "customWidget",
      "attrs": { "id": 7 },
      "data-foo": "bar",
      "content": []
    }
  ]
}"#;

#[test]
fn value_roundtrip_is_lossless() {
    let original: serde_json::Value = serde_json::from_str(SAMPLE).unwrap();
    let doc = Document::from_value(original.clone()).unwrap();
    let out = doc.to_value().unwrap();
    assert_eq!(
        original, out,
        "roundtrip must preserve structure & unknown fields"
    );
}

#[test]
fn preserves_unknown_node_type_and_extra_fields() {
    let doc = Document::from_json_str(SAMPLE).unwrap();
    let widget = doc
        .find(|n| n.node_type.as_deref() == Some("customWidget"))
        .unwrap();
    assert_eq!(widget.extra.get("data-foo"), Some(&json!("bar")));
    assert_eq!(widget.attr("id"), Some(&json!(7)));
}

#[test]
fn empty_content_distinct_from_missing() {
    // customWidget has empty content -> stays as []
    let doc = Document::from_json_str(SAMPLE).unwrap();
    let widget = doc
        .find(|n| n.node_type.as_deref() == Some("customWidget"))
        .unwrap();
    assert_eq!(widget.child_count(), 0);
    assert!(widget.content.is_some());
}
