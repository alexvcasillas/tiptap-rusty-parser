//! Edge cases of the hand-written `Node`/`Mark` serde impls. Behaviour pinned
//! to what the former `#[derive]` + `#[serde(flatten)]` produced.

use serde_json::{json, Value};
use tiptap_rusty_parser::{Change, Mark, Node};

fn node(s: &str) -> Node {
    serde_json::from_str(s).unwrap()
}

fn node_err(s: &str) -> String {
    serde_json::from_str::<Node>(s).unwrap_err().to_string()
}

fn mark_err(s: &str) -> String {
    serde_json::from_str::<Mark>(s).unwrap_err().to_string()
}

#[test]
fn unknown_keys_keep_order_around_known_ones() {
    let n = node(r#"{"b":1,"type":"p","a":2,"text":"x","c":{"z":1,"y":2}}"#);
    let keys: Vec<&str> = n.extra.keys().map(String::as_str).collect();
    assert_eq!(keys, ["b", "a", "c"]);
    // Known fields first, then extra in source order (nested order kept too).
    assert_eq!(
        serde_json::to_string(&n).unwrap(),
        r#"{"type":"p","text":"x","b":1,"a":2,"c":{"z":1,"y":2}}"#
    );
}

#[test]
fn null_known_fields_read_as_missing() {
    let n = node(r#"{"type":null,"attrs":null,"content":null,"marks":null,"text":null,"z":null}"#);
    assert_eq!(n.node_type, None);
    assert_eq!(n.attrs, None);
    assert_eq!(n.content, None);
    assert_eq!(n.marks, None);
    assert_eq!(n.text, None);
    // Unknown nulls are kept verbatim.
    assert_eq!(n.extra.get("z"), Some(&Value::Null));
    assert_eq!(serde_json::to_string(&n).unwrap(), r#"{"z":null}"#);
}

#[test]
fn missing_vs_empty_stays_distinct() {
    let n = node(r#"{"content":[],"marks":[],"attrs":{}}"#);
    assert_eq!(n.content.as_deref(), Some(&[][..]));
    assert_eq!(n.marks.as_deref(), Some(&[][..]));
    assert!(n.attrs.as_ref().is_some_and(|a| a.is_empty()));
    assert_eq!(
        serde_json::to_string(&n).unwrap(),
        r#"{"attrs":{},"content":[],"marks":[]}"#
    );
    assert_eq!(serde_json::to_string(&node("{}")).unwrap(), "{}");
}

#[test]
fn duplicate_known_field_is_an_error() {
    assert!(node_err(r#"{"type":"a","type":"b"}"#).contains("duplicate field `type`"));
    // A `null` occurrence still counts.
    assert!(node_err(r#"{"text":null,"text":"b"}"#).contains("duplicate field `text`"));
    assert!(
        node_err(r#"{"content":[{"marks":[{"type":"b","type":"c"}]}]}"#)
            .contains("duplicate field `type`")
    );
    assert!(mark_err(r#"{"type":"b","attrs":{},"attrs":{}}"#).contains("duplicate field `attrs`"));
}

#[test]
fn duplicate_unknown_key_keeps_first_slot_last_value() {
    let n = node(r#"{"x":1,"y":2,"x":3}"#);
    assert_eq!(serde_json::to_string(&n).unwrap(), r#"{"x":3,"y":2}"#);
    let m: Mark = serde_json::from_str(r#"{"q":1,"type":"b","p":2,"q":3}"#).unwrap();
    assert_eq!(
        serde_json::to_string(&m).unwrap(),
        r#"{"type":"b","q":3,"p":2}"#
    );
}

#[test]
fn type_errors_carry_position() {
    assert_eq!(
        node_err("{\"content\":[{\"type\":\"p\",\n  \"text\":7}]}"),
        "invalid type: integer `7`, expected a string at line 2 column 10"
    );
    assert!(node_err(r#"{"attrs":[1]}"#).starts_with("invalid type: sequence, expected a map"));
    assert!(node_err("[]").starts_with("invalid type: sequence, expected struct Node"));
    assert!(node_err("5").starts_with("invalid type: integer `5`, expected struct Node"));
}

#[test]
fn mark_type_is_required_and_non_null() {
    assert!(mark_err("{}").starts_with("missing field `type`"));
    assert!(mark_err(r#"{"type":null}"#).starts_with("invalid type: null, expected a string"));
    let m: Mark = serde_json::from_str(r#"{"attrs":null,"type":"b"}"#).unwrap();
    assert_eq!(m, Mark::new("b"));
}

#[test]
fn mark_node_only_keys_are_extra() {
    let m: Mark =
        serde_json::from_str(r#"{"text":"t","type":"b","content":[1],"marks":0}"#).unwrap();
    let keys: Vec<&str> = m.extra.keys().map(String::as_str).collect();
    assert_eq!(keys, ["text", "content", "marks"]);
    assert_eq!(
        serde_json::to_string(&m).unwrap(),
        r#"{"type":"b","text":"t","content":[1],"marks":0}"#
    );
}

#[test]
fn nested_roundtrip_via_value_and_tagged_enum() {
    let v = json!({
        "type": "doc", "zz": [1, {"b": 2, "a": 1}],
        "content": [{
            "data-x": true, "type": "p", "attrs": {"k": 1, "j": 2},
            "content": [{"text": "hi", "type": "text",
                "marks": [{"type": "link", "attrs": {"href": "x"}, "rel": "n"}]}]
        }]
    });
    let n: Node = serde_json::from_value(v.clone()).unwrap();
    let back = serde_json::to_value(&n).unwrap();
    assert_eq!(back, v);
    // Key order of the re-serialized text follows known-then-extra.
    assert_eq!(
        serde_json::to_string(&n.content.as_ref().unwrap()[0].content.as_ref().unwrap()[0])
            .unwrap(),
        r#"{"type":"text","marks":[{"type":"link","attrs":{"href":"x"},"rel":"n"}],"text":"hi"}"#
    );
    // Internally tagged enum: the node goes through serde's buffered path.
    let c = Change::Replace {
        path: vec![0],
        node: n.clone(),
    };
    let s = serde_json::to_string(&c).unwrap();
    let c2: Change = serde_json::from_str(&s).unwrap();
    assert_eq!(c2, c);
}
