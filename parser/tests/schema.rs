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

#[test]
fn violation_locates_child_and_mark() {
    let schema = Schema::new()
        .node("doc", NodeSpec::new().content_match("block+"))
        .node(
            "paragraph",
            NodeSpec::new().group("block").content_match("inline*"),
        )
        .node(
            "heading",
            NodeSpec::new()
                .group("block")
                .content_match("inline*")
                .attrs(["level"]),
        )
        .node("text", NodeSpec::new().group("inline"))
        .mark("bold", MarkSpec::new())
        .mark("link", MarkSpec::new().attrs(["href"]));
    let d = doc(r#"{"type":"doc","content":[
      {"type":"paragraph","content":[
        {"type":"text","text":"a"},
        {"type":"heading","attrs":{"level":1}}
      ]},
      {"type":"heading","attrs":{"level":1,"color":"red"},"content":[
        {"type":"text","text":"b","marks":[{"type":"bold"},{"type":"glow"},{"type":"link","attrs":{"href":"u","x":1}}]}
      ]}
    ]}"#);
    let v = d.validate(&schema);
    // heading inside paragraph: first unmatched child is index 1
    let ic = v
        .iter()
        .find(|x| matches!(x.kind, ViolationKind::InvalidContent { .. }))
        .unwrap();
    assert_eq!((ic.path.as_slice(), ic.child), (&[0][..], Some(1)));
    // node attr: no mark index
    let ua = v
        .iter()
        .find(|x| {
            x.kind
                == ViolationKind::UnknownAttr {
                    key: "color".into(),
                }
        })
        .unwrap();
    assert_eq!((ua.path.as_slice(), ua.mark), (&[1][..], None));
    // unknown mark and mark attr carry their mark index
    let um = v
        .iter()
        .find(|x| x.kind == ViolationKind::UnknownMark("glow".into()))
        .unwrap();
    assert_eq!((um.path.as_slice(), um.mark), (&[1, 0][..], Some(1)));
    let ma = v
        .iter()
        .find(|x| x.kind == ViolationKind::UnknownAttr { key: "x".into() })
        .unwrap();
    assert_eq!(ma.mark, Some(2));
    assert_eq!(v.len(), 4, "{v:?}");
}

#[test]
fn invalid_content_at_end_has_no_child() {
    let schema = Schema::new()
        .node("doc", NodeSpec::new().content_match("paragraph+"))
        .node("paragraph", NodeSpec::new());
    let v = doc(r#"{"type":"doc"}"#).validate(&schema);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].child, None);
}

#[test]
fn unknown_child_reported_once() {
    // expression form: the unknown child is a wildcard, parent stays valid
    let expr_schema = Schema::new()
        .node("doc", NodeSpec::new().content_match("block"))
        .node("paragraph", NodeSpec::new().group("block"));
    let v = doc(r#"{"type":"doc","content":[{"type":"widget"}]}"#).validate(&expr_schema);
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].kind, ViolationKind::UnknownNodeType("widget".into()));
    // cardinality still counts it: `block` allows one child, not two
    let v = doc(r#"{"type":"doc","content":[{"type":"widget"},{"type":"widget"}]}"#)
        .validate(&expr_schema);
    assert_eq!(v.len(), 3, "{v:?}");
    assert_eq!(v[0].child, Some(1));

    // array form: no DisallowedChild for an unknown child
    let v = doc(r#"{"type":"doc","content":[{"type":"widget"}]}"#).validate(&schema());
    assert_eq!(v.len(), 1, "{v:?}");
}

#[test]
fn violation_json_omits_empty_locators() {
    let v = doc(r#"{"type":"doc","content":[{"type":"widget"}]}"#).validate(&schema());
    let json = serde_json::to_string(&v[0]).unwrap();
    assert!(!json.contains("child") && !json.contains("mark"), "{json}");
}
