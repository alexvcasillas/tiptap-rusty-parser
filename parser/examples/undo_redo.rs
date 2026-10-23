//! Undo/redo via `diff` + `invert`.
//!
//! Run with: `cargo run --example undo_redo`

use tiptap_rusty_parser::{apply, Document};

fn main() {
    let original = Document::from_json_str(
        r#"{"type":"doc","content":[
            {"type":"paragraph","content":[{"type":"text","text":"Hello"}]}
        ]}"#,
    )
    .unwrap();

    // An edited version of the document.
    let mut edited = original.clone();
    edited
        .root_mut()
        .node_at_mut(&[0, 0])
        .unwrap()
        .set_text("Hello, world");
    edited.push_child(tiptap_rusty_parser::Node::element("paragraph").with_text("A new line."));

    // The forward patch and its inverse form an undo/redo pair.
    let redo = original.diff(&edited);
    let undo = original.invert(&redo).unwrap();

    println!("forward patch has {} change(s)", redo.len());

    // Apply forward, then undo, then redo.
    let mut doc = original.clone();
    apply(doc.root_mut(), &redo).unwrap();
    assert_eq!(doc, edited);
    println!("after redo:  {:?}", doc.text_content());

    apply(doc.root_mut(), &undo).unwrap();
    assert_eq!(doc, original);
    println!("after undo:  {:?}", doc.text_content());

    apply(doc.root_mut(), &redo).unwrap();
    assert_eq!(doc, edited);
    println!("after redo:  {:?}", doc.text_content());
}
