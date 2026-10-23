use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::fmt::Write;
use tiptap_rusty_markdown::{from_markdown, to_markdown};

/// One realistic "section": heading, marked-up paragraphs, nested/task
/// lists, code, table, quote.
fn section(i: usize, out: &mut String) {
    let _ = write!(
        out,
        "## Section {i}: parsing *fast* and **safe**\n\n\
         Plain intro with **bold {i}**, *italic*, ~~strike~~, `inline_code({i})`, a \
         [link](https://example.com/{i} \"title\"), ***both***, and an ![image](img/{i}.png \"t\"). \
         Escapes: 1\\. \\*star\\* snake_case a < b &amp; c.\n\n\
         Second paragraph that goes on for a while so text nodes have some length to them, \
         mixing [**bold link**](https://example.org/a/b?c=d) and *emphasis with `code` inside*.\\\n\
         Hard break above.\n\n\
         - item one with **bold**\n\
         - item two\n  \
           - nested a\n  \
           - nested b with `code`\n    \
             - deep nested\n\
         - item three\n\n\
         1. first\n2. second\n3. third with [link](https://x.test)\n\n\
         - [x] done task\n- [ ] open task with *style*\n- [ ] another\n\n\
         ```rust\nfn main() {{\n    println!(\"hello {i}\");\n}}\n```\n\n\
         | Name | Qty | Notes |\n| --- | :-: | ---: |\n\
         | a | {i} | **bold** cell |\n| b | 2 | `code` |\n| c | 3 | [l](https://x.test) |\n\n\
         > quoted *text* with **marks**\n>\n> - quoted list\n> - second\n\n---\n\n"
    );
}

fn build(target: usize) -> String {
    let mut s = String::with_capacity(target + 2048);
    let mut i = 0;
    while s.len() < target {
        section(i, &mut s);
        i += 1;
    }
    s
}

fn benches(c: &mut Criterion) {
    let small = build(4 * 1024); // README-sized
    let large = build(1024 * 1024);
    let small_doc = from_markdown(&small);
    let large_doc = from_markdown(&large);
    eprintln!("inputs: small {} B, large {} B", small.len(), large.len());

    c.bench_function("md_import_small", |b| {
        b.iter(|| from_markdown(black_box(&small)))
    });
    c.bench_function("md_import_large", |b| {
        b.iter(|| from_markdown(black_box(&large)))
    });
    c.bench_function("md_export_small", |b| {
        b.iter(|| to_markdown(black_box(&small_doc)))
    });
    c.bench_function("md_export_large", |b| {
        b.iter(|| to_markdown(black_box(&large_doc)))
    });
    c.bench_function("md_roundtrip_large", |b| {
        b.iter(|| to_markdown(&from_markdown(black_box(&large))))
    });
}

criterion_group!(g, benches);
criterion_main!(g);
