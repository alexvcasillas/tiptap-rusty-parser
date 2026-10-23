//! Schema validation against a small allow-list schema.
//!
//! Run with: `cargo run --example validate`

use tiptap_rusty_parser::{Document, MarkSpec, NodeSpec, Schema};

fn main() {
    // doc > paragraph+ ; paragraph > text* ; text allows the `bold` mark.
    let schema = Schema::new()
        .node("doc", NodeSpec::new().content(["paragraph"]))
        .node("paragraph", NodeSpec::new().content(["text"]))
        .node("text", NodeSpec::new().marks(["bold"]))
        .mark("bold", MarkSpec::new());

    let good = Document::from_json_str(
        r#"{"type":"doc","content":[
            {"type":"paragraph","content":[{"type":"text","text":"ok","marks":[{"type":"bold"}]}]}
        ]}"#,
    )
    .unwrap();

    let bad = Document::from_json_str(
        r#"{"type":"doc","content":[
            {"type":"heading","content":[{"type":"text","text":"nope","marks":[{"type":"italic"}]}]}
        ]}"#,
    )
    .unwrap();

    let good_violations = good.validate(&schema);
    println!("good doc: {} violation(s)", good_violations.len());
    assert!(good_violations.is_empty());

    let bad_violations = bad.validate(&schema);
    println!("bad doc:  {} violation(s)", bad_violations.len());
    for v in &bad_violations {
        println!("  at {:?}: {:?}", v.path, v.kind);
    }
    assert!(!bad_violations.is_empty());
}
