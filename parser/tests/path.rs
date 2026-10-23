use tiptap_rusty_parser::Document;

fn sample() -> Document {
    Document::from_json_str(
        r#"{
          "type": "doc",
          "content": [
            { "type": "paragraph", "content": [
              { "type": "text", "text": "a" },
              { "type": "text", "text": "b" }
            ]},
            { "type": "paragraph", "content": [
              { "type": "text", "text": "c" }
            ]}
          ]
        }"#,
    )
    .unwrap()
}

#[test]
fn node_at_valid_and_oob() {
    let doc = sample();
    assert_eq!(doc.node_at(&[]).unwrap().node_type.as_deref(), Some("doc"));
    assert_eq!(doc.node_at(&[0, 1]).unwrap().get_text(), Some("b"));
    assert_eq!(doc.node_at(&[1, 0]).unwrap().get_text(), Some("c"));
    assert!(doc.node_at(&[2]).is_none());
    assert!(doc.node_at(&[0, 9]).is_none());
    assert!(doc.node_at(&[0, 0, 0]).is_none()); // text node has no content
}

#[test]
fn node_at_mut_edits() {
    let mut doc = sample();
    doc.node_at_mut(&[0, 1]).unwrap().set_text("B");
    assert_eq!(doc.node_at(&[0, 1]).unwrap().get_text(), Some("B"));
}

#[test]
fn path_to_first() {
    let doc = sample();
    let p = doc
        .path_to(|n| n.get_text() == Some("c"))
        .expect("should find c");
    assert_eq!(p, vec![1, 0]);
    // round-trip
    assert_eq!(doc.node_at(&p).unwrap().get_text(), Some("c"));
    // self match -> empty path
    assert_eq!(
        doc.path_to(|n| n.node_type.as_deref() == Some("doc")),
        Some(vec![])
    );
    assert_eq!(doc.path_to(|n| n.get_text() == Some("zzz")), None);
}

#[test]
fn paths_to_all_in_order() {
    let doc = sample();
    let paths = doc.paths_to(|n| n.node_type.as_deref() == Some("text"));
    assert_eq!(paths, vec![vec![0, 0], vec![0, 1], vec![1, 0]]);
}

#[test]
fn parent_via_slice() {
    let doc = sample();
    let p = doc.path_to(|n| n.get_text() == Some("b")).unwrap();
    let parent = doc.node_at(&p[..p.len() - 1]).unwrap();
    assert_eq!(parent.node_type.as_deref(), Some("paragraph"));
    assert_eq!(parent.child_count(), 2);
}
