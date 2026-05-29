use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tiptap_rusty_parser::{doc, Document, Mark, Node};

/// Build a sizeable doc: `paras` paragraphs, each with `spans` text spans.
fn big_doc(paras: usize, spans: usize) -> Document {
    let paragraphs = (0..paras).map(|p| {
        let texts =
            (0..spans).map(move |s| Node::text_with_marks(format!("w{p}-{s} "), [Mark::new("bold")]));
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

    c.bench_function("serialize", |b| {
        b.iter(|| document.to_json_str().unwrap())
    });

    c.bench_function("walk_count", |b| {
        b.iter(|| {
            let mut n = 0usize;
            document.walk(&mut |_| n += 1);
            black_box(n)
        })
    });

    c.bench_function("find_all_text", |b| {
        b.iter(|| {
            black_box(document.find_all(|n| n.node_type.as_deref() == Some("text")).len())
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
}

criterion_group!(g, benches);
criterion_main!(g);
