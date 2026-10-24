//! Guards the docs-site doctests: every page with Rust fences is registered in
//! `docs_check::PAGES`, and no fence lacks a language (rustdoc would compile an
//! unlabelled fence as Rust).

use std::fs;
use std::path::{Path, PathBuf};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/content/docs");

fn mdx_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("docs/content/docs exists") {
        let path = entry.unwrap().path();
        if path.is_dir() {
            mdx_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "mdx" || e == "md") {
            out.push(path);
        }
    }
}

/// Info strings of the opening fences in `src` (backtick or tilde fences).
fn fence_langs(src: &str) -> Vec<(usize, String)> {
    let mut open: Option<String> = None;
    let mut langs = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let t = line.trim_start();
        let marker = if t.starts_with("```") {
            "```"
        } else if t.starts_with("~~~") {
            "~~~"
        } else {
            continue;
        };
        let run: String = t
            .chars()
            .take_while(|&c| c == marker.as_bytes()[0] as char)
            .collect();
        match &open {
            Some(fence) if run.len() >= fence.len() && t[run.len()..].trim().is_empty() => {
                open = None
            }
            Some(_) => {}
            None => {
                langs.push((i + 1, t[run.len()..].trim().to_string()));
                open = Some(run);
            }
        }
    }
    langs
}

#[test]
fn every_rust_page_is_compiled_and_every_fence_is_labelled() {
    let root = Path::new(ROOT);
    let mut files = Vec::new();
    mdx_files(root, &mut files);
    files.sort();
    assert!(!files.is_empty(), "no docs pages found under {ROOT}");

    let mut problems = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let src = fs::read_to_string(file).unwrap();
        let langs = fence_langs(&src);
        for (line, info) in &langs {
            if info.is_empty() {
                problems.push(format!("{rel}:{line}: code fence without a language"));
            }
        }
        let has_rust = langs
            .iter()
            .any(|(_, info)| info.split([' ', ',']).next() == Some("rust"));
        if has_rust && !docs_check::pages().contains(&rel.as_str()) {
            problems.push(format!(
                "{rel}: has Rust fences but is not registered in a docs-check/src/<section>.rs module"
            ));
        }
    }
    for page in docs_check::pages() {
        if !root.join(page).exists() {
            problems.push(format!(
                "{page}: registered in docs-check but the file does not exist"
            ));
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}
