//! Golden tests for `to_markdown`.
//!
//! Fixtures: `tests/fixtures/export/<name>.json` (a `doc`), optional
//! `<name>.opts.json` ([`RenderOptions`] as camelCase JSON), expected output in
//! `<name>.md`. Run with `UPDATE_FIXTURES=1` to (re)write the `.md` files.

use std::fs;
use std::path::PathBuf;
use tiptap_rusty_markdown::{to_markdown, to_markdown_with, Document, RenderOptions};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/export")
}

fn diff(expected: &str, actual: &str) -> String {
    let (e, a): (Vec<_>, Vec<_>) = (expected.lines().collect(), actual.lines().collect());
    let mut s = String::new();
    for i in 0..e.len().max(a.len()) {
        let (l, r) = (e.get(i), a.get(i));
        if l == r {
            s.push_str(&format!("  {:>3}  {}\n", i + 1, l.unwrap_or(&"")));
        } else {
            if let Some(l) = l {
                s.push_str(&format!("- {:>3}  {l}\n", i + 1));
            }
            if let Some(r) = r {
                s.push_str(&format!("+ {:>3}  {r}\n", i + 1));
            }
        }
    }
    if expected.ends_with('\n') != actual.ends_with('\n') {
        s.push_str("(trailing newline differs)\n");
    }
    s
}

fn render_fixture(name: &str) -> String {
    let d = dir();
    let json = fs::read_to_string(d.join(format!("{name}.json"))).unwrap();
    let doc = Document::from_json_str(&json).unwrap_or_else(|e| panic!("{name}: {e}"));
    let opts_path = d.join(format!("{name}.opts.json"));
    match fs::read_to_string(opts_path) {
        Ok(o) => {
            let opts: RenderOptions = serde_json::from_str(&o).unwrap();
            to_markdown_with(&doc, &opts)
        }
        Err(_) => to_markdown(&doc),
    }
}

#[test]
fn golden_fixtures() {
    let update = std::env::var_os("UPDATE_FIXTURES").is_some();
    let mut names: Vec<String> = fs::read_dir(dir())
        .unwrap()
        .filter_map(|e| {
            let f = e.ok()?.file_name().into_string().ok()?;
            let stem = f.strip_suffix(".json")?;
            (!stem.ends_with(".opts")).then(|| stem.to_string())
        })
        .collect();
    names.sort();
    assert!(names.len() > 20, "fixtures missing");
    let mut failures = Vec::new();
    for name in &names {
        let actual = render_fixture(name);
        let md = dir().join(format!("{name}.md"));
        if update {
            fs::write(&md, &actual).unwrap();
            continue;
        }
        match fs::read_to_string(&md) {
            Ok(expected) if expected == actual => {}
            Ok(expected) => failures.push(format!("{name}:\n{}", diff(&expected, &actual))),
            Err(_) => failures.push(format!("{name}: missing .md (run with UPDATE_FIXTURES=1)")),
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

fn md(json: &str) -> String {
    to_markdown(&Document::from_json_str(json).unwrap())
}

#[test]
fn empty_doc_is_empty_string() {
    assert_eq!(md(r#"{"type":"doc"}"#), "");
    assert_eq!(md(r#"{"type":"doc","content":[]}"#), "");
    assert_eq!(md(r#"{}"#), "");
}

#[test]
fn exactly_one_trailing_newline() {
    let out = md(r#"{"type":"doc","content":[{"type":"paragraph","content":[
        {"type":"text","text":"a"},{"type":"hardBreak"}]}]}"#);
    assert_eq!(out, "a\n");
}

#[test]
fn no_double_delimiters_for_adjacent_marks() {
    let out = md(r#"{"type":"doc","content":[{"type":"paragraph","content":[
        {"type":"text","text":"a","marks":[{"type":"bold"}]},
        {"type":"text","text":"b","marks":[{"type":"bold"}]}]}]}"#);
    assert_eq!(out, "**ab**\n");
}

#[test]
fn bundled_sample_renders() {
    let doc = Document::from_json_str(include_str!("../../studio/assets/sample.json")).unwrap();
    let out = to_markdown(&doc);
    assert!(out.starts_with("# tiptap-rusty-parser\n"));
    assert!(out.ends_with('\n') && !out.ends_with("\n\n"));
    for policy in ["transparent", "html", "skip"] {
        let opts: RenderOptions =
            serde_json::from_str(&format!(r#"{{"unknownNode":"{policy}"}}"#)).unwrap();
        let _ = to_markdown_with(&doc, &opts);
    }
}

#[test]
fn sample_v2_renders_when_present() {
    let doc = Document::from_json_str(include_str!("../../studio/assets/sample-v2.json")).unwrap();
    assert!(!to_markdown(&doc).is_empty());
}

#[test]
fn options_defaults_from_empty_json() {
    let o: RenderOptions = serde_json::from_str("{}").unwrap();
    assert_eq!(o, RenderOptions::default());
    assert_eq!(o.bullet, '-');
    assert_eq!(o.emphasis, '*');
}

#[test]
fn invalid_option_chars_fall_back() {
    let doc = Document::from_json_str(
        r#"{"type":"doc","content":[{"type":"bulletList","content":[{"type":"listItem","content":[
        {"type":"paragraph","content":[{"type":"text","text":"a","marks":[{"type":"italic"}]}]}]}]}]}"#,
    )
    .unwrap();
    let opts = RenderOptions {
        bullet: 'x',
        emphasis: '?',
        ..RenderOptions::default()
    };
    assert_eq!(to_markdown_with(&doc, &opts), "- *a*\n");
}

fn reimport(json: &str, opts: &RenderOptions) -> (String, serde_json::Value) {
    let out = to_markdown_with(&Document::from_json_str(json).unwrap(), opts);
    let back = tiptap_rusty_markdown::from_markdown(&out);
    (out, back.to_value().unwrap())
}

#[test]
fn rule_first_in_bullet_item_stays_in_list() {
    for (bullet, nested) in [
        ('-', false),
        ('*', false),
        ('+', false),
        ('-', true),
        ('*', true),
    ] {
        let item = r#"{"type":"listItem","content":[{"type":"horizontalRule"}]}"#;
        let inner = format!(r#"{{"type":"bulletList","content":[{item}]}}"#);
        let outer_item = if nested {
            format!(r#"{{"type":"listItem","content":[{inner}]}}"#)
        } else {
            item.to_owned()
        };
        let json = format!(
            r#"{{"type":"doc","content":[{{"type":"bulletList","content":[{outer_item}]}}]}}"#
        );
        let opts = RenderOptions {
            bullet,
            ..RenderOptions::default()
        };
        let (out, v) = reimport(&json, &opts);
        assert_eq!(
            v["content"][0]["type"], "bulletList",
            "{bullet} {nested}: {out:?}"
        );
        let item = &v["content"][0]["content"][0];
        let item = if nested {
            &item["content"][1]["content"][0]
        } else {
            item
        };
        assert_eq!(item["content"][1]["type"], "horizontalRule", "{out:?}");
    }
}

#[test]
fn leading_whitespace_is_kept_and_never_indents_code() {
    let p =
        |t: &str| format!(r#"{{"type":"paragraph","content":[{{"type":"text","text":"{t}"}}]}}"#);
    let cases = [
        (p("    code"), "/content/0/content/0/text", "    code"),
        (p("\\tx"), "/content/0/content/0/text", "\tx"),
        (
            r#"{"type":"paragraph","content":[{"type":"text","text":"a"},{"type":"hardBreak"},{"type":"text","text":"  b"}]}"#.to_owned(),
            "/content/0/content/2/text",
            "  b",
        ),
        (
            r#"{"type":"heading","attrs":{"level":2},"content":[{"type":"text","text":" h"}]}"#.to_owned(),
            "/content/0/content/0/text",
            " h",
        ),
        (
            format!(r#"{{"type":"bulletList","content":[{{"type":"listItem","content":[{}]}}]}}"#, p(" x")),
            "/content/0/content/0/content/0/content/0/text",
            " x",
        ),
        (
            format!(
                r#"{{"type":"table","content":[{{"type":"tableRow","content":[{{"type":"tableHeader","content":[{}]}}]}}]}}"#,
                p(" c")
            ),
            "/content/0/content/0/content/0/content/0/content/0/text",
            " c",
        ),
    ];
    for (block, ptr, want) in cases {
        let json = format!(r#"{{"type":"doc","content":[{block}]}}"#);
        let (out, v) = reimport(&json, &RenderOptions::default());
        assert_eq!(
            v.pointer(ptr).and_then(|x| x.as_str()),
            Some(want),
            "{out:?} -> {v}"
        );
    }
}

#[test]
fn abutting_emphasis_runs_reparse() {
    let t =
        |text: &str, marks: &str| format!(r#"{{"type":"text","text":"{text}","marks":[{marks}]}}"#);
    let (it, bo, st) = (
        r#"{"type":"italic"}"#,
        r#"{"type":"bold"}"#,
        r#"{"type":"strike"}"#,
    );
    let cases = [
        vec![t("a", it), t("b", bo)],
        vec![t("{", it), t("b", bo)],
        vec![t("a", bo), t("b", it)],
        vec![t("a", it), t("b", "")],
        vec![t("a", ""), t("b", it), t("c", "")],
        vec![t("a", bo), t("b", "")],
        vec![t("a", ""), t("b", bo), t("c", "")],
        vec![t("a", bo), t("b", it), t("c", "")],
        vec![t("a", ""), t("{b", it)],
        vec![t("b{", it), t("c", "")],
        vec![t("a", ""), t("{b", bo)],
        vec![t("b{", bo), t("c", "")],
        vec![t("é", ""), t("{b", it), t("é", "")],
        vec![t("a", ""), t("{b", st), t("c", "")],
        vec![t("a", ""), t("b}", st), t("c", "")],
        vec![t("a", ""), t("b", &format!("{bo},{it}")), t("c", "")],
        vec![t("a", ""), t("{b}", &format!("{bo},{it}")), t("c", "")],
        vec![t("a", it), t("{", &format!("{bo},{it}")), t("c", it)],
    ];
    let mut bad = Vec::new();
    for emphasis in ['*', '_'] {
        let opts = RenderOptions {
            emphasis,
            ..RenderOptions::default()
        };
        for nodes in &cases {
            let json = format!(
                r#"{{"type":"doc","content":[{{"type":"paragraph","content":[{}]}}]}}"#,
                nodes.join(",")
            );
            let (out, v) = reimport(&json, &opts);
            let want = Document::from_json_str(&json).unwrap().to_value().unwrap();
            let norm = |v: &serde_json::Value| {
                // `"marks":[]` and an absent `marks` mean the same
                serde_json::to_string(v)
                    .unwrap()
                    .replace(r#","marks":[]"#, "")
            };
            if norm(&v) != norm(&want) {
                bad.push(format!("{emphasis} {out:?}"));
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn code_block_info_string_roundtrips() {
    for (lang, code) in [
        ("`c`", "x"),
        ("```", "~~~\ny"),
        ("a\\\\*", "x"),
        ("&amp;", "x"),
        ("c++", "```"),
    ] {
        let json = format!(
            r#"{{"type":"doc","content":[{{"type":"codeBlock","attrs":{{"language":"{lang}"}},"content":[{{"type":"text","text":"{}"}}]}}]}}"#,
            code.replace('\n', "\\n")
        );
        let want = Document::from_json_str(&json).unwrap().to_value().unwrap();
        let (out, v) = reimport(&json, &RenderOptions::default());
        assert_eq!(v, want, "{out:?}");
    }
}

#[test]
fn nested_bullet_markers_on_one_line_are_not_a_rule() {
    let mut item = r#"{"type":"listItem"}"#.to_owned();
    for _ in 0..4 {
        item = format!(
            r#"{{"type":"listItem","content":[{{"type":"bulletList","content":[{item}]}}]}}"#
        );
    }
    let json =
        format!(r#"{{"type":"doc","content":[{{"type":"bulletList","content":[{item}]}}]}}"#);
    for bullet in ['-', '*', '+'] {
        let opts = RenderOptions {
            bullet,
            ..RenderOptions::default()
        };
        let (out, v) = reimport(&json, &opts);
        let mut n = &v["content"][0];
        for depth in 0..5 {
            assert_eq!(n["type"], "bulletList", "{bullet} depth {depth}: {out:?}");
            n = &n["content"][0]["content"][1];
        }
    }
}

#[test]
fn unicode_whitespace_at_mark_edges() {
    let json = r#"{"type":"doc","content":[{"type":"paragraph","content":[
        {"type":"text","text":"a ","marks":[{"type":"italic"}]},
        {"type":"text","text":" b","marks":[{"type":"bold"}]},
        {"type":"text","text":"c "}]}]}"#;
    let (out, v) = reimport(json, &RenderOptions::default());
    let p = &v["content"][0]["content"];
    assert_eq!(p[0]["marks"][0]["type"], "italic", "{out:?}");
    assert_eq!(p[0]["text"], "a", "{out:?}");
    assert_eq!(p[2]["marks"][0]["type"], "bold", "{out:?}");
    // significant at the end of a block (only ASCII whitespace is stripped)
    assert_eq!(
        p[p.as_array().unwrap().len() - 1]["text"],
        "c\u{a0}",
        "{out:?}"
    );
}

#[test]
fn carriage_returns_in_text_are_line_breaks() {
    for t in ["a\\rb", "a\\r\\nb", "a\\nb"] {
        let json = format!(
            r#"{{"type":"doc","content":[{{"type":"paragraph","content":[{{"type":"text","text":"{t}"}}]}}]}}"#
        );
        let (out, v) = reimport(&json, &RenderOptions::default());
        let want: serde_json::Value = serde_json::from_str(
            r#"[{"type":"text","text":"a"},{"type":"hardBreak"},{"type":"text","text":"b"}]"#,
        )
        .unwrap();
        assert_eq!(v["content"][0]["content"], want, "{t} {out:?}");
    }
}

#[test]
fn whitespace_after_hard_break_is_kept() {
    for tail in [
        r#"{"type":"text","text":" "},{"type":"text","text":"c","marks":[{"type":"code"}]}"#,
        r#"{"type":"text","text":" "}"#,
        r#"{"type":"text","text":"  "},{"type":"hardBreak"},{"type":"text","text":"b"}"#,
    ] {
        let json = format!(
            r#"{{"type":"doc","content":[{{"type":"paragraph","content":[{{"type":"text","text":"a"}},{{"type":"hardBreak"}},{tail}]}}]}}"#
        );
        let want = Document::from_json_str(&json).unwrap().to_value().unwrap();
        let (out, v) = reimport(&json, &RenderOptions::default());
        assert_eq!(v, want, "{out:?}");
    }
}

#[test]
fn marks_added_inside_code_span_reparse() {
    for (second, ty) in [
        (r#"[{"type":"bold"},{"type":"code"}]"#, "bold"),
        (r#"[{"type":"strike"},{"type":"code"}]"#, "strike"),
        (
            r#"[{"type":"code"},{"type":"link","attrs":{"href":"u"}}]"#,
            "link",
        ),
    ] {
        let json = format!(
            r#"{{"type":"doc","content":[{{"type":"paragraph","content":[
            {{"type":"text","text":"a","marks":[{{"type":"code"}}]}},
            {{"type":"text","text":"b","marks":{second}}}]}}]}}"#
        );
        let (out, v) = reimport(&json, &RenderOptions::default());
        let p = &v["content"][0]["content"];
        assert_eq!(p[0]["text"], "a", "{out:?}");
        assert_eq!(p[1]["text"], "b", "{out:?}");
        let types: Vec<_> = p[1]["marks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["type"].clone())
            .collect();
        assert!(
            types.contains(&ty.into()) && types.contains(&"code".into()),
            "{out:?}"
        );
    }
}

fn assert_idempotent(json: &str) {
    let m1 = md(json);
    let m2 = to_markdown(&tiptap_rusty_markdown::from_markdown(&m1));
    assert_eq!(m1, m2, "{json}");
}

#[test]
fn whitespace_in_link_text_at_block_end() {
    let link = r#""marks":[{"type":"link","attrs":{"href":"u"}}]"#;
    for t in [" ", "a ", "\\n", "  \\n"] {
        let text = format!(r#"{{"type":"text","text":"{t}",{link}}}"#);
        for wrap in [
            format!(r#"{{"type":"paragraph","content":[{text}]}}"#),
            format!(r#"{{"type":"heading","attrs":{{"level":3}},"content":[{text}]}}"#),
            format!(
                r#"{{"type":"bulletList","content":[{{"type":"listItem","content":[{{"type":"paragraph","content":[{text}]}}]}}]}}"#
            ),
        ] {
            assert_idempotent(&format!(r#"{{"type":"doc","content":[{wrap}]}}"#));
        }
    }
}

#[test]
fn adjacent_text_nodes_with_equal_marks_escape_as_one() {
    let t = |s: &str| format!(r#"{{"type":"text","text":"{s}"}}"#);
    for pair in [
        ["-", "9"],
        ["+", "} "],
        ["(<", "!"],
        ["<", "x-"],
        ["1", ". a"],
        ["#", " h"],
        ["&", "amp;"],
        ["a", "_b"],
    ] {
        let json = format!(
            r#"{{"type":"doc","content":[{{"type":"paragraph","content":[{},{}]}}]}}"#,
            t(pair[0]),
            t(pair[1])
        );
        assert_idempotent(&json);
        let (out, v) = reimport(&json, &RenderOptions::default());
        assert_eq!(
            v["content"][0]["content"][0]["text"],
            format!("{}{}", pair[0], pair[1]).trim_end(),
            "{out:?}"
        );
    }
}

#[test]
fn lookahead_escapes_hold_across_unknown_marks() {
    let json = r#"{"type":"doc","content":[{"type":"paragraph","content":[
        {"type":"text","text":"&"},
        {"type":"text","text":"amp; <","marks":[{"type":"highlight"}]},
        {"type":"text","text":"b>"}]}]}"#;
    let (out, v) = reimport(json, &RenderOptions::default());
    assert_eq!(
        v["content"][0]["content"][0]["text"], "&amp; <b>",
        "{out:?}"
    );
    let back = tiptap_rusty_markdown::from_markdown_with(
        &out,
        &tiptap_rusty_markdown::ParseOptions {
            html: tiptap_rusty_markdown::HtmlPolicy::Drop,
            ..Default::default()
        },
    );
    assert_eq!(
        back.to_value().unwrap()["content"][0]["content"][0]["text"],
        "&amp; <b>",
        "{out:?}"
    );
}

#[test]
fn destinations_starting_with_angle_bracket() {
    for href in ["<y>", "<", "<a b", "a<b>"] {
        let json = format!(
            r#"{{"type":"doc","content":[{{"type":"paragraph","content":[
            {{"type":"text","text":"l","marks":[{{"type":"link","attrs":{{"href":"{href}"}}}}]}},
            {{"type":"image","attrs":{{"src":"{href}","alt":"i"}}}}]}}]}}"#
        );
        let (out, v) = reimport(&json, &RenderOptions::default());
        let p = &v["content"][0]["content"];
        assert_eq!(p[0]["marks"][0]["attrs"]["href"], href, "{out:?}");
        assert_eq!(p[1]["attrs"]["src"], href, "{out:?}");
    }
}

#[test]
fn code_language_with_spaces_exports_first_word() {
    let json = r#"{"type":"doc","content":[{"type":"codeBlock","attrs":{"language":"rust ignore"},"content":[{"type":"text","text":"x"}]}]}"#;
    assert_eq!(md(json), "```rust\nx\n```\n");
    assert_idempotent(json);
}

#[test]
fn trailing_and_mark_closing_hard_break_runs() {
    let hb = |m: &str| format!(r#"{{"type":"hardBreak","marks":[{m}]}}"#);
    let bold = r#"{"type":"bold"}"#;
    let cases = [
        (
            format!(r#"{{"type":"text","text":"a"}},{},{}"#, hb(""), hb("")),
            "a\n",
        ),
        (format!("{},{}", hb(""), hb("")), ""),
        (
            format!(
                r#"{{"type":"text","text":"a","marks":[{bold}]}},{},{}"#,
                hb(bold),
                hb(bold)
            ),
            "**a**\n",
        ),
        (
            format!(
                r#"{{"type":"text","text":"a","marks":[{bold}]}},{},{},{{"type":"text","text":"b"}}"#,
                hb(bold),
                hb(bold)
            ),
            "**a**\\\n\\\nb\n",
        ),
    ];
    for (content, want) in cases {
        let json =
            format!(r#"{{"type":"doc","content":[{{"type":"paragraph","content":[{content}]}}]}}"#);
        assert_eq!(md(&json), want, "{json}");
        assert_idempotent(&json);
    }
}

#[test]
fn leading_newline_in_heading() {
    let h = r#"{"type":"doc","content":[{"type":"heading","attrs":{"level":2},"content":[{"type":"text","text":"\n<","marks":[{"type":"bold"}]}]}]}"#;
    assert_eq!(md(h), "## **<**\n");
    assert_idempotent(h);
}

#[test]
fn image_only_paragraphs_export_like_they_import() {
    let img = |s: &str| format!(r#"{{"type":"image","attrs":{{"src":"{s}","alt":""}}}}"#);
    for content in [
        format!(r#"{},{{"type":"hardBreak"}},{}"#, img("a"), img("b")),
        format!(r#"{{"type":"hardBreak"}},{}"#, img("a")),
        format!(r#"{},{{"type":"text","text":" "}},{}"#, img("a"), img("b")),
    ] {
        let p = format!(r#"{{"type":"paragraph","content":[{content}]}}"#);
        assert_idempotent(&format!(r#"{{"type":"doc","content":[{p}]}}"#));
        assert_idempotent(&format!(
            r#"{{"type":"doc","content":[{{"type":"bulletList","content":[{{"type":"listItem","content":[{p},{p}]}}]}}]}}"#
        ));
    }
}

#[test]
fn hard_break_edges_with_code_headings_and_links() {
    let link = r#"{"type":"link","attrs":{"href":"u"}}"#;
    let docs = [
        r#"[{"type":"paragraph","content":[{"type":"text","text":"a"},{"type":"hardBreak"},{"type":"text","text":"c","marks":[{"type":"code"}]}]}]"#.to_owned(),
        r#"[{"type":"paragraph","content":[{"type":"text","text":"c","marks":[{"type":"code"}]},{"type":"hardBreak"},{"type":"text","text":"c","marks":[{"type":"code"}]}]}]"#.to_owned(),
        r#"[{"type":"heading","attrs":{"level":2},"content":[{"type":"text","text":"<"},{"type":"hardBreak"},{"type":"text","text":"'"}]}]"#.to_owned(),
        format!(r#"[{{"type":"paragraph","content":[{{"type":"text","text":"a","marks":[{link},{{"type":"italic"}}]}},{{"type":"hardBreak","marks":[{link}]}}]}}]"#),
        format!(r#"[{{"type":"paragraph","content":[{{"type":"text","text":"]\n","marks":[{{"type":"italic"}},{link}]}}]}}]"#),
        format!(r#"[{{"type":"paragraph","content":[{{"type":"text","text":"a","marks":[{link}]}},{{"type":"hardBreak","marks":[{link}]}},{{"type":"hardBreak","marks":[{link}]}}]}}]"#),
    ];
    for content in docs {
        let json = format!(r#"{{"type":"doc","content":{content}}}"#);
        assert_idempotent(&json);
        let (out, v) = reimport(&json, &RenderOptions::default());
        let want = Document::from_json_str(&json).unwrap().to_value().unwrap();
        let breaks = |v: &serde_json::Value| v.to_string().matches("hardBreak").count();
        if !content.contains("heading") && !content.contains("]\\n") {
            assert_eq!(breaks(&v), breaks(&want), "{out:?}");
        }
    }
}

#[test]
fn lookahead_escapes_before_newlines_match_split_nodes() {
    for t in [
        "2)\\n&", "&\\n_", "<\\nb", "-\\n:", "+\\nx", "=\\nx", "#\\nx", "1.\\nx",
    ] {
        for wrap in [
            format!(r#"{{"type":"paragraph","content":[{{"type":"text","text":"{t}"}}]}}"#),
            format!(
                r#"{{"type":"orderedList","content":[{{"type":"listItem","content":[{{"type":"paragraph","content":[{{"type":"text","text":"{t}","marks":[{{"type":"bold"}}]}}]}}]}}]}}"#
            ),
        ] {
            assert_idempotent(&format!(r#"{{"type":"doc","content":[{wrap}]}}"#));
        }
    }
}

#[test]
fn render_nothing_edge_cases_are_stable() {
    for content in [
        // image whose only mark is not rendered, after a break
        r#"[{"type":"paragraph","content":[{"type":"hardBreak"},{"type":"image","attrs":{"src":"s","alt":"a"},"marks":[{"type":"code"}]}]}]"#,
        // empty paragraphs do not make a list loose
        r#"[{"type":"orderedList","content":[{"type":"listItem","content":[]},{"type":"listItem","content":[{"type":"paragraph"},{"type":"paragraph"}]}]}]"#,
        // whitespace before trailing breaks
        r#"[{"type":"paragraph","content":[{"type":"text","text":"a  ","marks":[{"type":"italic"}]},{"type":"hardBreak"},{"type":"hardBreak"}]}]"#,
    ] {
        assert_idempotent(&format!(r#"{{"type":"doc","content":{content}}}"#));
    }
}

#[test]
fn adjacent_bullet_lists_never_share_a_marker() {
    let empty = r#"{"type":"bulletList","content":[{"type":"listItem"}]}"#;
    let json = format!(
        r#"{{"type":"doc","content":[{{"type":"bulletList","content":[{{"type":"listItem","content":[
        {{"type":"bulletList","content":[{{"type":"listItem","content":[{empty},{empty}]}}]}}]}}]}}]}}"#
    );
    assert_idempotent(&json);
    let (out, v) = reimport(&json, &RenderOptions::default());
    let inner = &v["content"][0]["content"][0]["content"][1]["content"][0]["content"];
    assert_eq!(inner.as_array().map(|a| a.len()), Some(3), "{out:?} {v}");
}

#[test]
fn newline_breaks_and_empty_rendering_blocks_are_stable() {
    for content in [
        // a newline in a single-line context is a space, not a node end
        r#"[{"type":"heading","attrs":{"level":5},"content":[{"type":"text","text":"&\n&"}]}]"#,
        r#"[{"type":"paragraph","content":[{"type":"image","attrs":{"src":"s","alt":"<\n"}}]}]"#,
        // a newline written as a break is dropped at the end like a break
        r#"[{"type":"paragraph","content":[{"type":"text","text":"\n","marks":[{"type":"bold"}]},{"type":"hardBreak"},{"type":"hardBreak"}]}]"#,
        r#"[{"type":"paragraph","content":[{"type":"text","text":"a\n","marks":[{"type":"bold"}]},{"type":"hardBreak","marks":[{"type":"strike"}]},{"type":"hardBreak"}]}]"#,
        // paragraphs of breaks only render nothing
        r#"[{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"a"}]},{"type":"paragraph","content":[{"type":"hardBreak"}]}]},{"type":"listItem"}]}]"#,
        r#"[{"type":"orderedList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"a"}]},{"type":"orderedList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"hardBreak"}]}]}]}]}]}]"#,
    ] {
        assert_idempotent(&format!(r#"{{"type":"doc","content":{content}}}"#));
    }
}

#[test]
fn leading_marked_break_runs_are_stable() {
    let b = r#"{"type":"hardBreak","marks":[{"type":"bold"}]}"#;
    let bt = |t: &str| format!(r#"{{"type":"text","text":"{t}","marks":[{{"type":"bold"}}]}}"#);
    for content in [
        format!("[{b},{}]", bt("\\n9")),
        format!("[{b},{b},{}]", bt("x")),
        format!("[{b},{b}]"),
        format!(r#"[{{"type":"text","text":"a"}},{b},{b}]"#),
        // whitespace before a dropped break run
        r#"[{"type":"text","text":" "},{"type":"hardBreak"},{"type":"hardBreak","marks":[{"type":"bold"}]}]"#.to_owned(),
    ] {
        let json = format!(r#"{{"type":"doc","content":[{{"type":"paragraph","content":{content}}}]}}"#);
        assert_idempotent(&json);
        assert!(!md(&json).contains("****"), "{}", md(&json));
    }
    // a whitespace-only code span is content
    assert_idempotent(
        r#"{"type":"doc","content":[{"type":"orderedList","content":[{"type":"listItem","content":[
        {"type":"paragraph","content":[{"type":"text","text":"\t","marks":[{"type":"code"}]}]},
        {"type":"paragraph","content":[{"type":"text","text":"x"}]}]}]}]}"#,
    );
}

#[test]
fn whitespace_neighbours_are_stable() {
    let link = r#"{"type":"link","attrs":{"href":"u"}}"#;
    for content in [
        // leading whitespace of link text stays inside the link
        format!(r#"[{{"type":"text","text":" ","marks":[{{"type":"strike"}},{link}]}},{{"type":"text","text":"x","marks":[{link}]}}]"#),
        format!(r#"[{{"type":"text","text":"a"}},{{"type":"text","text":" b","marks":[{link}]}}]"#),
        // a break that carries code renders nothing
        r#"[{"type":"hardBreak","marks":[{"type":"code"}]}]"#.to_owned(),
        // whitespace between dropped breaks
        r#"[{"type":"hardBreak","marks":[{"type":"italic"}]},{"type":"text","text":"  "},{"type":"hardBreak","marks":[{"type":"bold"}]}]"#.to_owned(),
        // `<` / `&` before whitespace from a differently marked node
        r#"[{"type":"text","text":"'<"},{"type":"text","text":" ","marks":[{"type":"bold"}]},{"type":"text","text":"x"}]"#.to_owned(),
    ] {
        for wrap in ["paragraph", "heading"] {
            let attrs = if wrap == "heading" { r#","attrs":{"level":2}"# } else { "" };
            let block = format!(r#"{{"type":"{wrap}"{attrs},"content":{content}}}"#);
            assert_idempotent(&format!(
                r#"{{"type":"doc","content":[{{"type":"orderedList","content":[{{"type":"listItem","content":[{{"type":"paragraph","content":[{{"type":"text","text":"a"}}]}},{block}]}},{{"type":"listItem"}}]}}]}}"#
            ));
        }
    }
}

#[test]
fn angle_and_ampersand_at_block_end_need_no_escape() {
    for t in ["< ", "a&\\t", "x<\\n"] {
        let json = format!(
            r#"{{"type":"doc","content":[{{"type":"paragraph","content":[{{"type":"text","text":"{t}","marks":[{{"type":"bold"}}]}}]}}]}}"#
        );
        assert_idempotent(&json);
        assert!(!md(&json).contains('\\'), "{}", md(&json));
    }
}
