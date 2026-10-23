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
