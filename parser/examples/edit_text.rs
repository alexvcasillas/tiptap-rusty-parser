//! Inline range editing + a recorded transaction.
//!
//! Run with: `cargo run --example edit_text`

use tiptap_rusty_parser::{Document, Mark, Position, Range};

fn main() {
    let mut doc = Document::from_json_str(
        r#"{"type":"doc","content":[
            {"type":"paragraph","content":[{"type":"text","text":"Hello world"}]}
        ]}"#,
    )
    .unwrap();

    // Reach into the paragraph at path [0] and range-edit its inline content.
    let para = doc.root_mut().node_at_mut(&[0]).unwrap();

    // Bold "world".
    para.add_mark_range(
        Range::new(Position::new(0, 6), Position::new(0, 11)),
        Mark::new("bold"),
    )
    .unwrap();

    // Insert a prefix, then delete it again.
    para.insert_text(Position::new(0, 0), "» ", None).unwrap();
    para.delete_range(Range::new(Position::new(0, 0), Position::new(0, 2)))
        .unwrap();

    println!("text:   {:?}", doc.text_content());
    println!(
        "spans:  {}",
        doc.root().node_at(&[0]).unwrap().child_count()
    );

    // Record a transaction: edits apply in place AND yield a replayable patch.
    let before = doc.clone();
    let changes = {
        let mut tx = doc.root_mut().transform();
        tx.set_attr(vec![0], "textAlign", "center").unwrap();
        tx.set_text(vec![0, 1], Some("WORLD".into())).unwrap();
        tx.finish()
    };
    println!("transaction recorded {} change(s)", changes.len());

    // The recorded log replays from the pre-image and inverts to an undo.
    let undo = before.invert(&changes).unwrap();
    let mut back = doc.clone();
    tiptap_rusty_parser::apply(back.root_mut(), &undo).unwrap();
    assert_eq!(back, before);
    println!("undo restored the original: {:?}", back.text_content());
}
