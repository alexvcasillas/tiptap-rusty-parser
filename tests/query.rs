use tiptap_rusty_parser::Document;

fn sample() -> Document {
    Document::from_json_str(
        r#"{
          "type": "doc",
          "content": [
            { "type": "paragraph", "content": [
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
fn find_by_type() {
    let doc = sample();
    let p = doc.find(|n| n.node_type.as_deref() == Some("paragraph"));
    assert!(p.is_some());
}

#[test]
fn find_all_by_type() {
    let doc = sample();
    let ps = doc.find_all(|n| n.node_type.as_deref() == Some("paragraph"));
    assert_eq!(ps.len(), 2);
    let texts = doc.find_all(|n| n.node_type.as_deref() == Some("text"));
    assert_eq!(texts.len(), 3);
}

#[test]
fn find_by_mark() {
    let doc = sample();
    let bold = doc.find(|n| n.has_mark("bold")).unwrap();
    assert_eq!(bold.get_text(), Some("a"));
}

#[test]
fn descendants_preorder() {
    let doc = sample();
    let types: Vec<_> = doc
        .descendants()
        .filter_map(|n| n.node_type.clone())
        .collect();
    assert_eq!(
        types,
        ["doc", "paragraph", "text", "text", "paragraph", "text"]
    );
}

#[test]
fn find_all_mut_edits() {
    let mut doc = sample();
    let mut pred = |n: &tiptap_rusty_parser::Node| n.node_type.as_deref() == Some("text");
    let texts = doc.root_mut().find_all_mut(&mut pred);
    assert_eq!(texts.len(), 3);
    for t in texts {
        t.set_text("X");
    }
    assert!(doc
        .find_all(|n| n.node_type.as_deref() == Some("text"))
        .iter()
        .all(|n| n.get_text() == Some("X")));
}
