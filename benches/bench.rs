use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tiptap_rusty_parser::{doc, Document, Mark, MarkSpec, Node, NodeSpec, Schema};

/// Build a sizeable doc: `paras` paragraphs, each with `spans` text spans.
fn big_doc(paras: usize, spans: usize) -> Document {
    let paragraphs = (0..paras).map(|p| {
        let texts = (0..spans)
            .map(move |s| Node::text_with_marks(format!("w{p}-{s} "), [Mark::new("bold")]));
        Node::element("paragraph")
            .with_attr("textAlign", "left")
            .with_children(texts)
    });
    Document::new(doc(paragraphs))
}

fn benches(c: &mut Criterion) {
    let document = big_doc(500, 20);
    let json = document.to_json_str().unwrap();

    c.bench_function("parse", |b| {
        b.iter(|| Document::from_json_str(black_box(&json)).unwrap())
    });

    c.bench_function("serialize", |b| b.iter(|| document.to_json_str().unwrap()));

    c.bench_function("walk_count", |b| {
        b.iter(|| {
            let mut n = 0usize;
            document.walk(&mut |_| n += 1);
            black_box(n)
        })
    });

    c.bench_function("find_all_text", |b| {
        b.iter(|| {
            black_box(
                document
                    .find_all(|n| n.node_type.as_deref() == Some("text"))
                    .len(),
            )
        })
    });

    c.bench_function("replace_all_addmark", |b| {
        b.iter_batched(
            || document.clone(),
            |mut d| {
                d.replace_all(
                    |n| n.node_type.as_deref() == Some("text"),
                    |n| {
                        n.add_mark(Mark::new("italic"));
                    },
                )
            },
            criterion::BatchSize::SmallInput,
        )
    });

    c.bench_function("by_type", |b| {
        b.iter(|| black_box(document.by_type("text").len()))
    });

    c.bench_function("text_content", |b| {
        b.iter(|| black_box(document.text_content().len()))
    });

    let schema = Schema::new()
        .node("doc", NodeSpec::new().content(["paragraph"]))
        .node("paragraph", NodeSpec::new().content(["text"]))
        .node("text", NodeSpec::new().marks(["bold"]))
        .mark("bold", MarkSpec::new());
    c.bench_function("validate", |b| {
        b.iter(|| black_box(document.validate(&schema).len()))
    });

    // Near-identical large docs: flip one attr on every 50th paragraph and
    // append one paragraph. Exercises the `==` early-out + prefix/suffix trim.
    let mut modified = document.clone();
    let mut i = 0usize;
    modified.replace_all(
        |n| n.node_type.as_deref() == Some("paragraph"),
        |n| {
            if i.is_multiple_of(50) {
                n.set_attr("textAlign", "right");
            }
            i += 1;
        },
    );
    modified.push_child(Node::element("paragraph").with_text("appended"));

    c.bench_function("diff_large_small_change", |b| {
        b.iter(|| black_box(document.root().diff(modified.root()).len()))
    });

    let changes = document.root().diff(modified.root());
    c.bench_function("apply_large_small_change", |b| {
        b.iter_batched(
            || document.clone(),
            |mut d| tiptap_rusty_parser::apply(d.root_mut(), black_box(&changes)).unwrap(),
            criterion::BatchSize::SmallInput,
        )
    });
}

criterion_group!(g, benches);
criterion_main!(g);
