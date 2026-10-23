//! Roundtrip properties.
//!
//! - Import fixtures: `from(to(from(md))) == from(md)` (doc equality), using
//!   the same `_nogfm` / `_htmldrop` suffix options as `import.rs`.
//! - Export fixtures + bundled sample: `to(from(to(doc))) == to(doc)`
//!   (export idempotence), except fixtures that are lossy by design.

use std::fs;
use std::path::PathBuf;

use tiptap_rusty_markdown::{
    from_markdown_with, to_markdown_with, Document, HtmlPolicy, ParseOptions, RenderOptions,
};

/// Export fixtures excluded from the idempotence check: the first export is
/// intentionally lossy or not schema-sane, so reimporting it cannot reproduce
/// the same Markdown.
const LOSSY_EXPORT: &[&str] = &[
    // Unknown nodes: rendered as raw HTML / skipped / unwrapped, which the
    // importer reads back as text or drops by design.
    "unknown_html",
    "unknown_skip",
    "unknown_transparent",
    // Underline / sub / sup have no Markdown syntax: exported as inline HTML,
    // which the default HTML policy (Text) reimports as literal text.
    "marks_basic",
    // A hard break inside a pipe-table cell is exported as `<br>`; same as above.
    "table_pipe",
    // Table with cells that cannot be pipe-encoded falls back to an HTML
    // block; HTML policy `Text` reimports it as paragraph text.
    "table_html_fallback",
    // Deliberately malformed input (bad nesting / missing fields).
    "odd_input",
    // Root is not a `doc`; no document structure to reimport.
    "non_doc_root",
];

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn parse_opts(name: &str) -> ParseOptions {
    ParseOptions {
        gfm: !name.contains("_nogfm"),
        html: if name.contains("_htmldrop") {
            HtmlPolicy::Drop
        } else {
            HtmlPolicy::Text
        },
    }
}

fn stems(dir: &str, ext: &str) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(fixtures().join(dir))
        .unwrap()
        .filter_map(|e| {
            let f = e.ok()?.file_name().into_string().ok()?;
            let s = f.strip_suffix(ext)?;
            (!s.ends_with(".opts")).then(|| s.to_owned())
        })
        .collect();
    v.sort();
    v
}

fn pretty(d: &Document) -> String {
    serde_json::to_string_pretty(&d.to_value().unwrap()).unwrap()
}

/// Shows the first differing line of two texts with context.
fn line_diff(a: &str, b: &str) -> String {
    let (al, bl): (Vec<_>, Vec<_>) = (a.lines().collect(), b.lines().collect());
    let first = al
        .iter()
        .zip(&bl)
        .position(|(x, y)| x != y)
        .unwrap_or(al.len().min(bl.len()));
    let from = first.saturating_sub(3);
    let mut out = format!("first difference at line {}\n", first + 1);
    for (tag, ls) in [("-", &al), ("+", &bl)] {
        for (i, l) in ls.iter().enumerate().skip(from).take(8) {
            let m = if i == first { '>' } else { ' ' };
            out.push_str(&format!("{tag}{m}{:4} {l}\n", i + 1));
        }
    }
    out
}

fn render_opts(name: &str) -> RenderOptions {
    match fs::read_to_string(fixtures().join(format!("export/{name}.opts.json"))) {
        Ok(o) => serde_json::from_str(&o).unwrap(),
        Err(_) => RenderOptions::default(),
    }
}

#[test]
fn import_fixtures_roundtrip() {
    let mut failures = Vec::new();
    let names = stems("import", ".md");
    assert!(names.len() >= 20, "fixtures missing");
    for name in &names {
        let md = fs::read_to_string(fixtures().join(format!("import/{name}.md"))).unwrap();
        let opts = parse_opts(name);
        let d1 = from_markdown_with(&md, &opts);
        let out = to_markdown_with(&d1, &RenderOptions::default());
        let d2 = from_markdown_with(&out, &opts);
        if d1 != d2 {
            failures.push(format!(
                "[{name}] reimport differs\n{}\n--- intermediate markdown ---\n{out}\n--- end ---",
                line_diff(&pretty(&d1), &pretty(&d2)),
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}

fn assert_idempotent(label: &str, doc: &Document, ro: &RenderOptions, failures: &mut Vec<String>) {
    let m1 = to_markdown_with(doc, ro);
    let d2 = from_markdown_with(&m1, &ParseOptions::default());
    let m2 = to_markdown_with(&d2, ro);
    if m1 != m2 {
        failures.push(format!(
            "[{label}] export not idempotent\n{}\n--- first export ---\n{m1}\n--- second export ---\n{m2}\n--- end ---",
            line_diff(&m1, &m2),
        ));
    }
}

#[test]
fn export_fixtures_idempotent() {
    let mut failures = Vec::new();
    let names = stems("export", ".json");
    assert!(names.len() > 20, "fixtures missing");
    for name in &names {
        if LOSSY_EXPORT.contains(&name.as_str()) {
            continue;
        }
        let json = fs::read_to_string(fixtures().join(format!("export/{name}.json"))).unwrap();
        let doc = Document::from_json_str(&json).unwrap();
        assert_idempotent(name, &doc, &render_opts(name), &mut failures);
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}

#[test]
fn excluded_fixtures_exist() {
    let names = stems("export", ".json");
    for n in LOSSY_EXPORT {
        assert!(names.iter().any(|x| x == n), "stale exclusion {n}");
    }
}

#[test]
fn sample_export_idempotent() {
    let mut failures = Vec::new();
    let doc = Document::from_json_str(include_str!("../../studio/assets/sample.json")).unwrap();
    assert_idempotent(
        "sample.json",
        &doc,
        &RenderOptions::default(),
        &mut failures,
    );
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}

/// Markdown snippets that once failed `from(to(from(md))) == from(md)`.
#[test]
fn regression_snippets_roundtrip() {
    let cases = [
        // consecutive hard breaks collapsed into one
        "a\\\n\\\nb",
        // abutting `*` closer/opener merged into one delimiter run
        "**a *b*** *c*",
        "*a* **b**",
        // marked hard break
        "**a\\\nb**",
        // lone image in a table cell was hoisted out of its paragraph
        "| a |\n| --- |\n| ![x](y) |",
        // mark nesting order
        "**~~both~~** ~~**both**~~",
        // delimiter runs that stopped flanking / merged on export
        "_{_**b**",
        "a*{b*",
        "*b{*c",
        "_a_b",
        // hard break opening a mark span keeps the mark
        "~\\\n]~",
        "*\\\n'* ",
        "**\\\n<* ",
        "a **\\\nb**",
        // whitespace before consecutive hard breaks moved past the first
        "a \\\n\\\nb",
    ];
    for md in cases {
        let d1 = from_markdown_with(md, &ParseOptions::default());
        let out = to_markdown_with(&d1, &RenderOptions::default());
        let d2 = from_markdown_with(&out, &ParseOptions::default());
        assert_eq!(d1, d2, "{md:?} -> {out:?}");
    }
}

#[test]
fn empty_link_destination_or_text_roundtrips() {
    for md in [
        "[a]()",
        "[> ]()",
        "x [a](<> \"t\")",
        "[ ](2) `",
        "[\t](0)`c`",
        "a [ ](u)",
    ] {
        let d1 = from_markdown_with(md, &ParseOptions::default());
        let out = to_markdown_with(&d1, &RenderOptions::default());
        let d2 = from_markdown_with(&out, &ParseOptions::default());
        assert_eq!(d1, d2, "{md:?} -> {out:?}");
    }
}

/// Markdown whose export put a block where it re-parses as something else.
#[test]
fn block_placement_roundtrips() {
    let cases = [
        // task item starting with a non-paragraph block
        "- [x]\n  - a",
        "- [ ]\n  # h",
        "- [x]\n  ```\n  c\n  ```",
        "- [ ]\n  > q",
        "- [x]\n  ---",
        "- [ ]\n  1. a\n2. b",
    ];
    for md in cases {
        let d1 = from_markdown_with(md, &ParseOptions::default());
        let out = to_markdown_with(&d1, &RenderOptions::default());
        let d2 = from_markdown_with(&out, &ParseOptions::default());
        assert_eq!(d1, d2, "{md:?} -> {out:?}");
    }
}
