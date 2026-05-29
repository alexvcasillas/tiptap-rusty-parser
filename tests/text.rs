use tiptap_rusty_parser::Document;

fn sample() -> Document {
    Document::from_json_str(
        r#"{
          "type": "doc",
          "content": [
            { "type": "paragraph", "content": [
              { "type": "text", "text": "Hello " },
              { "type": "text", "text": "brave", "marks": [{ "type": "bold" }] },
              { "type": "text", "text": " world" }
            ]},
            { "type": "paragraph", "content": [
              { "type": "text", "text": "second line" }
            ]}
          ]
        }"#,
    )
    .unwrap()
}

#[test]
fn text_content_concats_in_order() {
    let doc = sample();
    assert_eq!(doc.text_content(), "Hello brave worldsecond line");
}

#[test]
fn text_content_with_separator_between_blocks() {
    let doc = sample();
    assert_eq!(
        doc.text_content_with_separator("\n\n"),
        "Hello brave world\n\nsecond line"
    );
    // within-block text stays contiguous (no sep between text spans)
    assert!(!doc
        .text_content_with_separator("|")
        .contains("Hello |brave"));
}

#[test]
fn char_count_counts_scalars() {
    let doc = sample();
    // "Hello brave world" (17) + "second line" (11) = 28
    assert_eq!(doc.char_count(), 28);
}

#[test]
fn char_count_multibyte() {
    let doc =
        Document::from_json_str(r#"{"type":"doc","content":[{"type":"text","text":"café 🦀"}]}"#)
            .unwrap();
    // c a f é space crab = 6 scalar values
    assert_eq!(doc.char_count(), 6);
}

#[test]
fn word_count_across_blocks_and_marks() {
    let doc = sample();
    // "Hello brave world" (3) + "second line" (2) = 5; marks don't split words
    assert_eq!(doc.word_count(), 5);
}

#[test]
fn word_count_cjk() {
    let doc =
        Document::from_json_str(r#"{"type":"doc","content":[{"type":"text","text":"你好世界"}]}"#)
            .unwrap();
    // unicode word segmentation counts each CJK ideograph as a word
    assert_eq!(doc.word_count(), 4);
}
