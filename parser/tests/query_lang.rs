use tiptap_rusty_parser::{Document, Query, QueryErrorKind};

const SAMPLE: &str = r#"{"type":"doc","content":[
  {"type":"heading","attrs":{"level":1},"content":[{"type":"text","text":"tiptap-rusty-parser"}]},
  {"type":"paragraph","content":[
    {"type":"text","text":"A fast, "},
    {"type":"text","marks":[{"type":"bold"}],"text":"schema-agnostic"},
    {"type":"text","marks":[{"type":"link","attrs":{"href":"https://github.com/x/y"}}],"text":"roundtrip it"}
  ]},
  {"type":"heading","attrs":{"level":2},"content":[{"type":"text","text":"What it does"}]},
  {"type":"callout","attrs":{"variant":"info","priority":2,"collapsed":false,"note":null,
    "meta":{"source":"import","tags":["docs","beta"]}},
   "content":[{"type":"paragraph","content":[{"type":"text","text":"Unknown"}]}]},
  {"type":"bulletList","content":[{"type":"listItem"},{"type":"listItem"},{"type":"listItem"}]}
]}"#;

fn paths(q: &str) -> Vec<Vec<usize>> {
    let doc = Document::from_json_str(SAMPLE).unwrap();
    Query::parse(q)
        .unwrap_or_else(|e| panic!("{q}: {e}"))
        .find_paths(doc.root())
}

fn err(q: &str) -> (usize, QueryErrorKind) {
    let e = Query::parse(q).unwrap_err();
    (e.offset, e.kind)
}

#[test]
fn bare_words_and_strings() {
    assert_eq!(paths("roundtrip"), vec![vec![1, 2]]);
    assert_eq!(paths("ROUNDTRIP"), vec![vec![1, 2]]);
    assert_eq!(paths("\"what it\""), vec![vec![2, 0]]);
    // type substring too
    assert_eq!(paths("callout"), vec![vec![3]]);
    // adjacent terms are ANDed
    assert_eq!(paths("what does"), vec![vec![2, 0]]);
    assert!(paths("what nope").is_empty());
}

#[test]
fn type_ops() {
    assert_eq!(paths("type == heading"), vec![vec![0], vec![2]]);
    assert_eq!(
        paths("type ~ LIST"),
        vec![vec![4], vec![4, 0], vec![4, 1], vec![4, 2]]
    );
    let all = paths("type != text");
    assert!(all.iter().all(|p| p != &vec![0, 0]));
    assert!(all.contains(&Vec::<usize>::new()));
}

#[test]
fn attrs() {
    assert_eq!(paths("attrs.level > 1"), vec![vec![2]]);
    assert_eq!(paths("attrs.level >= 1"), vec![vec![0], vec![2]]);
    assert_eq!(paths("attrs.level <= 1"), vec![vec![0]]);
    assert_eq!(paths("attrs.level == 2"), vec![vec![2]]);
    assert_eq!(paths("attrs.level"), vec![vec![0], vec![2]]);
    assert_eq!(paths("attrs.variant == info"), vec![vec![3]]);
    assert_eq!(paths("attrs.variant ~ NF"), vec![vec![3]]);
    assert_eq!(paths("attrs.collapsed == false"), vec![vec![3]]);
    assert_eq!(paths("attrs.note == null"), vec![vec![3]]);
    assert_eq!(paths("attrs.meta.source == import"), vec![vec![3]]);
    assert_eq!(paths("attrs.meta.source == 'import'"), vec![vec![3]]);
    assert_eq!(paths("attrs.meta.tags ~ bet"), vec![vec![3]]);
    assert_eq!(paths("attrs.meta.tags.0 == docs"), vec![vec![3]]);
    assert!(paths("attrs.meta.missing").is_empty());
    // string literal vs number value never equal
    assert!(paths("attrs.level == \"1\"").is_empty());
    // != negates ==, so it holds where the attr is missing
    assert!(paths("attrs.level != 1").contains(&Vec::<usize>::new()));
    assert_eq!(paths("attrs && type ~ head"), vec![vec![0], vec![2]]);
}

#[test]
fn text_and_marks() {
    assert_eq!(paths("text ~ \"FAST\""), vec![vec![1, 0]]);
    assert_eq!(paths("text == Unknown"), vec![vec![3, 0, 0]]);
    assert_eq!(paths("has mark bold"), vec![vec![1, 1]]);
    assert_eq!(paths("mark == link"), vec![vec![1, 2]]);
    assert_eq!(paths("mark ~ LIN"), vec![vec![1, 2]]);
    assert_eq!(paths("marks.link"), vec![vec![1, 2]]);
    assert_eq!(paths("marks.link.href ~ github"), vec![vec![1, 2]]);
    assert!(paths("marks.link.href ~ gitlab").is_empty());
    assert_eq!(paths("marks"), vec![vec![1, 1], vec![1, 2]]);
    let no_link = paths("text && mark != link");
    assert!(!no_link.contains(&vec![1, 2]) && no_link.contains(&vec![1, 1]));
    assert_eq!(paths("has attr meta.source"), vec![vec![3]]);
}

#[test]
fn children_and_depth() {
    assert_eq!(paths("children > 2"), vec![vec![], vec![1], vec![4]]);
    assert_eq!(
        paths("type == listItem && !children"),
        vec![vec![4, 0], vec![4, 1], vec![4, 2]]
    );
    assert_eq!(paths("depth == 0"), vec![Vec::<usize>::new()]);
    assert_eq!(paths("depth >= 3"), vec![vec![3, 0, 0]]);
}

#[test]
fn boolean_logic() {
    assert_eq!(
        paths("type == heading || callout"),
        vec![vec![0], vec![2], vec![3]]
    );
    assert_eq!(paths("type == heading and attrs.level == 2"), vec![vec![2]]);
    assert_eq!(
        paths("type == heading AND NOT attrs.level == 2"),
        vec![vec![0]]
    );
    assert_eq!(
        paths("(type == heading || type == callout) && attrs.level"),
        vec![vec![0], vec![2]]
    );
    // && binds tighter than ||
    assert_eq!(
        paths("type == callout || type == heading && attrs.level == 2"),
        vec![vec![2], vec![3]]
    );
    assert_eq!(paths("!!callout"), vec![vec![3]]);
}

#[test]
fn errors_with_offsets() {
    assert_eq!(err(""), (0, QueryErrorKind::Empty));
    assert_eq!(err("   "), (0, QueryErrorKind::Empty));
    assert_eq!(
        err("attrs.level >"),
        (
            13,
            QueryErrorKind::UnexpectedEnd {
                expected: "a value"
            }
        )
    );
    assert_eq!(
        err("type heading"),
        (
            5,
            QueryErrorKind::UnexpectedToken {
                found: "heading".into(),
                expected: "an operator"
            }
        )
    );
    assert_eq!(
        err("type"),
        (
            4,
            QueryErrorKind::UnexpectedEnd {
                expected: "an operator"
            }
        )
    );
    assert_eq!(err("a & b"), (2, QueryErrorKind::LoneOperator('&')));
    assert_eq!(err("x | y"), (2, QueryErrorKind::LoneOperator('|')));
    assert_eq!(err("foo \"bar"), (4, QueryErrorKind::UnterminatedString));
    assert_eq!(
        err("(a || b"),
        (7, QueryErrorKind::UnexpectedEnd { expected: "`)`" })
    );
    assert_eq!(
        err("a)"),
        (
            1,
            QueryErrorKind::UnexpectedToken {
                found: ")".into(),
                expected: "an operator or end of query"
            }
        )
    );
    assert_eq!(
        err("level > 1"),
        (0, QueryErrorKind::UnknownField("level".into()))
    );
    assert_eq!(err("children > x"), (11, QueryErrorKind::ExpectedNumber));
    assert_eq!(err("children ~ 2"), (9, QueryErrorKind::InvalidOperator));
    assert_eq!(err("attrs..x"), (6, QueryErrorKind::EmptyPath));
    assert_eq!(
        err("marks.link == x"),
        (11, QueryErrorKind::InvalidOperator)
    );
    assert_eq!(
        err("has bold"),
        (
            4,
            QueryErrorKind::UnexpectedToken {
                found: "bold".into(),
                expected: "`mark` or `attr`"
            }
        )
    );
    assert_eq!(
        err("a &&"),
        (4, QueryErrorKind::UnexpectedEnd { expected: "a term" })
    );
    let msg = Query::parse("attrs.level >").unwrap_err().to_string();
    assert_eq!(msg, "expected a value at end of query (at byte 13)");
}

#[test]
fn unicode_case_folding() {
    let doc = Document::from_json_str(
        r#"{"type":"doc","content":[{"type":"text","text":"Ärger über Straße"}]}"#,
    )
    .unwrap();
    let q = Query::parse("ÜBER").unwrap();
    assert_eq!(q.find_paths(doc.root()), vec![vec![0]]);
    assert!(Query::parse("\"ä\"")
        .unwrap()
        .matches(&doc.root().children()[0]));
}
