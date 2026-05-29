use tiptap_rusty_parser::{Document, Node};

fn sample() -> Document {
    Document::from_json_str(
        r#"{
          "type": "doc",
          "content": [
            { "type": "heading", "attrs": { "level": 1 }, "content": [
              { "type": "text", "text": "Title" }
            ]},
            { "type": "paragraph", "attrs": { "level": 1 }, "content": [
              { "type": "text", "text": "a", "marks": [{ "type": "bold" }] },
              { "type": "text", "text": "b" }
            ]},
            { "type": "paragraph", "content": [
              { "type": "text", "text": "c", "marks": [{ "type": "italic" }] }
            ]}
          ]
        }"#,
    )
    .unwrap()
}

#[test]
fn by_type_and_first() {
    let doc = sample();
    assert_eq!(doc.by_type("paragraph").len(), 2);
    assert_eq!(doc.by_type("text").len(), 4);
    assert_eq!(doc.by_type("nope").len(), 0);
    assert_eq!(
        doc.first_by_type("text").and_then(|n| n.get_text()),
        Some("Title")
    );
}

#[test]
fn by_mark() {
    let doc = sample();
    let bold = doc.by_mark("bold");
    assert_eq!(bold.len(), 1);
    assert_eq!(bold[0].get_text(), Some("a"));
    assert_eq!(doc.by_mark("italic").len(), 1);
}

#[test]
fn by_attr() {
    let doc = sample();
    // heading + first paragraph both have level: 1
    assert_eq!(doc.by_attr("level", 1).len(), 2);
    assert_eq!(doc.by_attr("level", 2).len(), 0);
}

#[test]
fn by_type_mut_persists() {
    let mut doc = sample();
    for p in doc.root_mut().by_type_mut("paragraph") {
        p.set_attr("touched", true);
    }
    let touched = doc.by_attr("touched", true);
    assert_eq!(touched.len(), 2);
    assert!(touched
        .iter()
        .all(|n| n.node_type.as_deref() == Some("paragraph")));
}

#[test]
fn includes_self() {
    let n = Node::element("paragraph");
    assert_eq!(n.by_type("paragraph").len(), 1);
}
