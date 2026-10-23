//! Markdown import: golden fixtures + adversarial inputs.
//!
//! Fixtures live in `tests/fixtures/import/<name>.md` with the expected
//! JSONContent in `<name>.json`. Name suffixes select options: `_nogfm`
//! (gfm = false), `_htmldrop` (HtmlPolicy::Drop). Run with
//! `UPDATE_FIXTURES=1` to (re)write the `.json` files.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tiptap_rusty_markdown::{
    from_markdown, from_markdown_with, Document, HtmlPolicy, ParseOptions, DEFAULT_MAX_DEPTH,
};
use tiptap_rusty_parser::Schema;

/// Fixtures whose output uses nodes/attrs outside StarterKit (validated only
/// against `gfm.schema.json`).
const GFM_ONLY: &[&str] = &["tables", "tasks", "images", "links"];

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn schema(name: &str) -> Schema {
    let s = fs::read_to_string(fixtures_dir().join(name)).expect("read schema");
    Schema::from_json_str(&s).expect("parse schema")
}

fn opts_for(name: &str) -> ParseOptions {
    ParseOptions {
        gfm: !name.contains("_nogfm"),
        html: if name.contains("_htmldrop") {
            HtmlPolicy::Drop
        } else {
            HtmlPolicy::Text
        },
        ..Default::default()
    }
}

/// Line diff of two pretty-printed values: shows the first mismatch with context.
fn diff(expected: &Value, actual: &Value) -> String {
    let e = serde_json::to_string_pretty(expected).unwrap();
    let a = serde_json::to_string_pretty(actual).unwrap();
    let el: Vec<&str> = e.lines().collect();
    let al: Vec<&str> = a.lines().collect();
    let first = el
        .iter()
        .zip(&al)
        .position(|(x, y)| x != y)
        .unwrap_or(el.len().min(al.len()));
    let from = first.saturating_sub(4);
    let mut out = format!("first difference at line {}\n", first + 1);
    for (tag, lines) in [("-", &el), ("+", &al)] {
        for (i, l) in lines.iter().enumerate().skip(from).take(12) {
            let m = if i == first { '>' } else { ' ' };
            out.push_str(&format!("{tag}{m}{:4} {l}\n", i + 1));
        }
        out.push('\n');
    }
    out.push_str("--- actual (full) ---\n");
    out.push_str(&a);
    out
}

#[test]
fn golden_fixtures() {
    let dir = fixtures_dir().join("import");
    let update = std::env::var_os("UPDATE_FIXTURES").is_some();
    let starter = schema("tiptap-starter-kit.schema.json");
    let gfm = schema("gfm.schema.json");

    let mut names: Vec<String> = fs::read_dir(&dir)
        .expect("fixtures dir")
        .filter_map(|e| {
            let p = e.ok()?.path();
            (p.extension()? == "md").then(|| p.file_stem()?.to_str().map(str::to_owned))?
        })
        .collect();
    names.sort();
    assert!(names.len() >= 20, "expected fixtures, found {names:?}");

    let mut failures = Vec::new();
    for name in &names {
        let md = fs::read_to_string(dir.join(format!("{name}.md"))).unwrap();
        let doc = from_markdown_with(&md, &opts_for(name));
        let actual = doc.to_value().unwrap();
        let json_path = dir.join(format!("{name}.json"));

        if update {
            let mut s = serde_json::to_string_pretty(&actual).unwrap();
            s.push('\n');
            fs::write(&json_path, s).unwrap();
        } else {
            match fs::read_to_string(&json_path) {
                Ok(s) => {
                    let expected: Value = serde_json::from_str(&s).unwrap();
                    if expected != actual {
                        failures.push(format!("[{name}] {}", diff(&expected, &actual)));
                    }
                }
                Err(_) => failures.push(format!("[{name}] missing {}", json_path.display())),
            }
        }

        let v = doc.validate(&gfm);
        if !v.is_empty() {
            failures.push(format!("[{name}] gfm schema violations: {v:?}"));
        }
        if !GFM_ONLY.contains(&name.as_str()) {
            let v = doc.validate(&starter);
            if !v.is_empty() {
                failures.push(format!("[{name}] starter-kit schema violations: {v:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}

#[test]
fn gfm_off_keeps_syntax_as_text() {
    let opts = ParseOptions {
        gfm: false,
        ..Default::default()
    };
    let v = from_markdown_with("| a | b |\n|---|---|\n\n~~s~~", &opts)
        .to_value()
        .unwrap();
    let s = v.to_string();
    assert!(!s.contains("\"table\""), "{s}");
    assert!(!s.contains("strike"), "{s}");
    assert!(s.contains("~~s~~"), "{s}");
}

#[test]
fn options_serde_defaults() {
    let o: ParseOptions = serde_json::from_str("{}").unwrap();
    assert_eq!(o, ParseOptions::default());
    let o: ParseOptions = serde_json::from_str(r#"{"html":"drop"}"#).unwrap();
    assert!(o.gfm);
    assert_eq!(o.html, HtmlPolicy::Drop);
}

/// Tiny deterministic PRNG (xorshift) so the test needs no extra deps.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn adversarial() -> Vec<String> {
    let mut v: Vec<String> = [
        "```",
        "```rust",
        "~~~\nunclosed",
        "`",
        "``a`",
        "**",
        "***a**",
        "*a **b* c**",
        "[",
        "[a](",
        "[a](b \"",
        "![",
        "![a](",
        "![![![x](a)](b)](c)",
        "[![a](b)](c)",
        "[[[[[a]]]]](b)",
        "<",
        "<div",
        "<!--",
        "<![CDATA[",
        "</div>\n\n<div>",
        "|",
        "||\n|-|",
        "|a|\n|-|\n|b|c|d|",
        "| a |\n|---|\n| <b>x</b> |",
        "- [ ]",
        "- [x]\n- [ ]",
        "- [ ] \n  - [x]",
        "1. [ ] a\n- [x] b",
        "-",
        "- \n-\n- -",
        "1.",
        "99999999999999999999. big",
        "999999999. big",
        "> ",
        ">",
        "\\",
        "\\\n",
        "  \n",
        "\0\0",
        "\r\n\r\n",
        "\u{feff}# bom",
        "#######",
        "# \n",
        "---\n---\n---",
        "***\n___",
        "a  \n",
        "&#0; &#xFFFFFF; &bogus;",
        "\t\t- a\n\t- b",
        "    \n    ",
        "- a\n\n\n\n- b",
        "[a]: <>\n[a]",
        "* * *\n- - -",
        "~~a~~~",
        "~~~~",
        "<span>**x</span>**",
        "> - [ ] a\n> | t |\n> |---|",
        "- ```\n  code\n- b",
        "- ![x](y)\n\n  ![z](w)",
        "![a](b)  \n![c](d)",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    // deep nesting
    v.push(">".repeat(2000) + " deep");
    v.push(
        (0..1000)
            .map(|i| format!("{}- l{i}\n", "  ".repeat(i)))
            .collect(),
    );
    v.push("*".repeat(500) + "x" + &"*".repeat(500));
    v.push("[".repeat(500) + "x" + &"](u)".repeat(500));
    v.push("![".repeat(200) + &"](u)".repeat(200));
    v.push("`".repeat(1000));
    v.push("> - ".repeat(1000) + "x");
    v.push("- [ ] ".repeat(1000));
    v.push(
        "> | a |\n> | - |\n".repeat(3)
            + &"> ".repeat(70)
            + "| a | b |\n"
            + &"> ".repeat(70)
            + "| - | - |\n"
            + &"> ".repeat(70)
            + "| c | d |",
    );
    v.push("|".repeat(300) + "\n" + &"|-".repeat(150) + "|");

    // random punctuation soup
    let alphabet: &[u8] = b"*_`~[]()!<>|-+#>\\\n \t:\"'&;x1.=/";
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..400 {
        let len = (rng.next() % 80) as usize;
        let s: String = (0..len)
            .map(|_| alphabet[(rng.next() % alphabet.len() as u64) as usize] as char)
            .collect();
        v.push(s);
    }
    v
}

#[test]
fn adversarial_inputs_never_panic_and_stay_valid() {
    let gfm = schema("gfm.schema.json");
    let starter = schema("tiptap-starter-kit.schema.json");
    let all = [
        ParseOptions::default(),
        ParseOptions {
            gfm: true,
            html: HtmlPolicy::Drop,
            ..Default::default()
        },
        ParseOptions {
            gfm: false,
            html: HtmlPolicy::Text,
            ..Default::default()
        },
        ParseOptions {
            gfm: false,
            html: HtmlPolicy::Drop,
            ..Default::default()
        },
    ];
    for input in adversarial() {
        for opts in &all {
            let doc = from_markdown_with(&input, opts);
            let v = doc.validate(&gfm);
            assert!(v.is_empty(), "input {input:?} {opts:?}: {v:?}");
            if !opts.gfm {
                // Without GFM only images (and link titles) leave StarterKit.
                let v: Vec<_> = doc
                    .validate(&starter)
                    .into_iter()
                    .filter(|v| {
                        !matches!(&v.kind,
                            tiptap_rusty_parser::ViolationKind::UnknownNodeType(t) if t == "image")
                            && !matches!(&v.kind,
                            tiptap_rusty_parser::ViolationKind::UnknownAttr { key } if key == "title")
                    })
                    .collect();
                assert!(v.is_empty(), "input {input:?} {opts:?}: {v:?}");
            }
        }
    }
}

#[test]
fn default_entry_point_matches_default_options() {
    let md = "# t\n\n- [x] a\n";
    assert_eq!(
        from_markdown(md),
        from_markdown_with(md, &ParseOptions::default())
    );
}

#[test]
fn trailing_whitespace_is_stripped_from_blocks() {
    for (md, want) in [
        ("# x\t", "x"),
        ("<b> ", "<b>"),
        ("<b>\t", "<b>"),
        ("> q\n[]()", "q"),
        ("[# ](2) ", "# "),
    ] {
        let v = from_markdown(md).to_value().unwrap();
        let mut node = &v["content"][0];
        while node["type"] != "text" {
            node = &node["content"][0];
        }
        assert_eq!(node["text"], want, "{md:?}: {v}");
    }
    // code spans keep theirs
    let v = from_markdown("`a `").to_value().unwrap();
    assert_eq!(v["content"][0]["content"][0]["text"], "a ");
}

#[test]
fn dropped_html_leaves_no_trailing_hard_break() {
    let opts = ParseOptions {
        gfm: true,
        html: HtmlPolicy::Drop,
        ..Default::default()
    };
    for (md, want) in [
        (
            "a\\\n<b>",
            r#"[{"type":"paragraph","content":[{"type":"text","text":"a"}]}]"#,
        ),
        (
            "a  \n</b>",
            r#"[{"type":"paragraph","content":[{"type":"text","text":"a"}]}]"#,
        ),
        ("\\\n<b>", r#"[{"type":"paragraph"}]"#),
        (
            "x\n\n\\\n<b>",
            r#"[{"type":"paragraph","content":[{"type":"text","text":"x"}]}]"#,
        ),
    ] {
        let v = from_markdown_with(md, &opts).to_value().unwrap();
        let want: Value = serde_json::from_str(want).unwrap();
        assert_eq!(v["content"], want, "{md:?}");
    }
}

#[test]
fn multiline_inline_html_text_uses_hard_breaks() {
    for md in ["a <!-- x\ny -->", "a <!-- x\r\ny -->"] {
        let v = from_markdown(md).to_value().unwrap();
        let want: Value = serde_json::from_str(
            r#"[{"type":"text","text":"a <!-- x"},{"type":"hardBreak"},{"type":"text","text":"y -->"}]"#,
        )
        .unwrap();
        assert_eq!(v["content"][0]["content"], want, "{md:?}");
    }
}

#[test]
fn whitespace_only_link_at_block_end_is_kept() {
    for md in ["[ ](u)", "a [\t](u)", "### [ ](u)", "[\\\n](u)"] {
        let v = from_markdown(md).to_value().unwrap();
        let p = v["content"][0]["content"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let last = p.last().cloned().unwrap_or_default();
        assert_eq!(last["marks"][0]["type"], "link", "{md:?}: {v}");
    }
}

/// JSON nesting depth of a value (objects and arrays).
fn json_depth(v: &Value) -> usize {
    match v {
        Value::Object(m) => 1 + m.values().map(json_depth).max().unwrap_or(0),
        Value::Array(a) => 1 + a.iter().map(json_depth).max().unwrap_or(0),
        _ => 0,
    }
}

fn all_text(v: &Value, out: &mut String) {
    if let Some(t) = v["text"].as_str() {
        out.push_str(t);
    }
    if let Some(c) = v["content"].as_array() {
        for x in c {
            all_text(x, out);
        }
    }
}

#[test]
fn deep_nesting_is_capped_and_flattened() {
    let gfm = schema("gfm.schema.json");
    let quotes = ">".repeat(5000) + " **deep** [l](u \"t\")";
    let list: String = (0..5000)
        .map(|i| format!("{}- l{i}\n", "  ".repeat(i)))
        .collect();
    let mixed = "> - [ ] ".repeat(2000) + "x\n" + &"> - ".repeat(2000) + "| a |\n| - |\n| `b` |";
    for (md, want) in [
        (quotes, "deep l".to_owned()),
        (list, (0..5000).map(|i| format!("l{i}")).collect::<String>()),
        (mixed, String::new()),
    ] {
        for max_depth in [DEFAULT_MAX_DEPTH, 40, 0] {
            let opts = ParseOptions {
                max_depth,
                ..Default::default()
            };
            let doc = from_markdown_with(&md, &opts);
            assert!(doc.validate(&gfm).is_empty(), "{:?}", doc.validate(&gfm));
            let v = doc.to_value().unwrap();
            assert!(
                json_depth(&v) <= max_depth.max(8),
                "{} > {max_depth}",
                json_depth(&v)
            );
            let json = doc.to_json_str().unwrap();
            let back = Document::from_json_str(&json).expect("reload");
            assert_eq!(back, doc);
            let mut text = String::new();
            all_text(&v, &mut text);
            if !want.is_empty() {
                assert_eq!(text, want);
            } else {
                assert!(
                    text.contains('x') && text.contains("[ ] ") && text.contains('b'),
                    "{text}"
                );
            }
            drop(back);
        }
    }
    // shallow docs are untouched by the cap
    let md = "> > - a\n>   > b";
    assert_eq!(
        from_markdown(md),
        from_markdown_with(
            md,
            &ParseOptions {
                max_depth: usize::MAX,
                ..Default::default()
            }
        )
    );
}
