use tiptap_rusty_parser::{Document, MarkSpec, NodeSpec, Schema, ViolationKind};

fn schema() -> Schema {
    Schema::new()
        .node("doc", NodeSpec::new().content(["paragraph", "heading"]))
        .node("paragraph", NodeSpec::new().content(["text"]))
        .node(
            "heading",
            NodeSpec::new()
                .content(["text"])
                .attrs(["level"])
                .required_attrs(["level"]),
        )
        // marks live on text nodes, so the allow-list belongs here
        .node("text", NodeSpec::new().marks(["bold", "italic"]))
        .mark("bold", MarkSpec::new())
        .mark("italic", MarkSpec::new())
        .mark(
            "link",
            MarkSpec::new().attrs(["href"]).required_attrs(["href"]),
        )
}

fn doc(json: &str) -> Document {
    Document::from_json_str(json).unwrap()
}

#[test]
fn valid_document() {
    let d = doc(r#"{
      "type":"doc","content":[
        {"type":"heading","attrs":{"level":1},"content":[{"type":"text","text":"T"}]},
        {"type":"paragraph","content":[{"type":"text","text":"a","marks":[{"type":"bold"}]}]}
      ]}"#);
    assert!(d.is_valid(&schema()));
    assert_eq!(d.validate(&schema()), vec![]);
}

#[test]
fn unknown_node_type() {
    let d = doc(r#"{"type":"doc","content":[{"type":"widget"}]}"#);
    let v = d.validate(&schema());
    // widget: not allowed child of doc + unknown type
    assert!(v
        .iter()
        .any(|x| x.kind == ViolationKind::UnknownNodeType("widget".into()) && x.path == vec![0]));
}

#[test]
fn missing_node_type() {
    let d = doc(r#"{"type":"doc","content":[{"attrs":{}}]}"#);
    let v = d.validate(&schema());
    assert!(v
        .iter()
        .any(|x| x.kind == ViolationKind::MissingNodeType && x.path == vec![0]));
}

#[test]
fn disallowed_child() {
    // heading nested inside paragraph (paragraph only allows text)
    let d = doc(r#"{
      "type":"doc","content":[
        {"type":"paragraph","content":[{"type":"heading","attrs":{"level":1}}]}
      ]}"#);
    let v = d.validate(&schema());
    assert!(v.iter().any(|x| x.path == vec![0]
        && x.kind
            == ViolationKind::DisallowedChild {
                parent: "paragraph".into(),
                child: "heading".into()
            }));
}

#[test]
fn unknown_and_disallowed_marks() {
    // "strike" not registered -> UnknownMark; "italic" registered but allowed on paragraph,
    // so put italic somewhere disallowed: heading allows no marks override? heading marks=None=any.
    let d = doc(r#"{
      "type":"doc","content":[
        {"type":"paragraph","content":[
          {"type":"text","text":"x","marks":[{"type":"strike"}]}
        ]}
      ]}"#);
    let v = d.validate(&schema());
    assert!(v
        .iter()
        .any(|x| x.kind == ViolationKind::UnknownMark("strike".into())));
}

#[test]
fn disallowed_mark_on_node() {
    // paragraph allows only [bold, italic]; link is registered but not allowed there
    let d = doc(r#"{
      "type":"doc","content":[
        {"type":"paragraph","content":[
          {"type":"text","text":"x","marks":[{"type":"link","attrs":{"href":"u"}}]}
        ]}
      ]}"#);
    let v = d.validate(&schema());
    assert!(v.iter().any(|x| x.kind
        == ViolationKind::DisallowedMark {
            node: "text".into(),
            mark: "link".into()
        }));
}

#[test]
fn missing_required_attr() {
    let d = doc(r#"{"type":"doc","content":[{"type":"heading","content":[]}]}"#);
    let v = d.validate(&schema());
    assert!(v.iter().any(|x| x.path == vec![0]
        && x.kind
            == ViolationKind::MissingAttr {
                key: "level".into()
            }));
}

#[test]
fn unknown_attr() {
    let d =
        doc(r#"{"type":"doc","content":[{"type":"heading","attrs":{"level":1,"bogus":true}}]}"#);
    let v = d.validate(&schema());
    assert!(v.iter().any(|x| x.kind
        == ViolationKind::UnknownAttr {
            key: "bogus".into()
        }));
}

#[test]
fn missing_mark_attr() {
    // allow link on paragraph children via a schema where text allows link
    let schema = Schema::new()
        .node("doc", NodeSpec::new().content(["paragraph"]))
        .node("paragraph", NodeSpec::new().content(["text"]))
        .node("text", NodeSpec::new().marks(["link"]))
        .mark(
            "link",
            MarkSpec::new().attrs(["href"]).required_attrs(["href"]),
        );
    let d = doc(r#"{
      "type":"doc","content":[
        {"type":"paragraph","content":[
          {"type":"text","text":"x","marks":[{"type":"link"}]}
        ]}
      ]}"#);
    let v = d.validate(&schema);
    assert!(v
        .iter()
        .any(|x| x.kind == ViolationKind::MissingAttr { key: "href".into() }));
}

#[test]
fn json_schema_matches_builder() {
    let json = r#"{
      "nodes": {
        "doc": { "content": ["paragraph","heading"] },
        "paragraph": { "content": ["text"] },
        "heading": { "content": ["text"], "attrs": ["level"], "required_attrs": ["level"] },
        "text": { "marks": ["bold","italic"] }
      },
      "marks": {
        "bold": {}, "italic": {},
        "link": { "attrs": ["href"], "required_attrs": ["href"] }
      }
    }"#;
    let from_json = Schema::from_json_str(json).unwrap();
    let bad = doc(r#"{"type":"doc","content":[{"type":"heading"}]}"#);
    // same verdict from JSON-loaded schema as the builder schema
    assert_eq!(bad.validate(&from_json), bad.validate(&schema()));
    assert!(!bad.is_valid(&from_json)); // heading missing required level
}
