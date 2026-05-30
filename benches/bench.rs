use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tiptap_rusty_parser::{
    doc, BlockRange, DiffGranularity, DiffOptions, Document, Mark, MarkSpec, Node, NodeSpec,
    Position, Range, Schema,
};

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

    c.bench_function("to_html", |b| {
        b.iter(|| black_box(document.to_html().len()))
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

    // Reorder: reverse the 500 paragraphs. Every child relocates, exercising
    // move detection — the diff is a list of clone-free `Move` ops.
    let mut reordered = document.clone();
    if let Some(kids) = reordered.root_mut().content.as_mut() {
        kids.reverse();
    }
    c.bench_function("diff_reordered_children", |b| {
        b.iter(|| black_box(document.root().diff(reordered.root()).len()))
    });

    let reorder_changes = document.root().diff(reordered.root());
    c.bench_function("apply_reordered_children", |b| {
        b.iter_batched(
            || document.clone(),
            |mut d| tiptap_rusty_parser::apply(d.root_mut(), black_box(&reorder_changes)).unwrap(),
            criterion::BatchSize::SmallInput,
        )
    });

    // `big_doc`'s 20 same-mark spans per paragraph all merge into one — the
    // merge-heavy path.
    c.bench_function("normalize_merge_heavy", |b| {
        b.iter_batched(
            || document.clone(),
            |mut d| d.normalize(),
            criterion::BatchSize::SmallInput,
        )
    });

    // Already-canonical doc (one text node per paragraph): nothing to merge.
    let canonical = Document::new(doc((0..500).map(|p| {
        Node::element("paragraph").with_children([Node::text(format!("paragraph {p}"))])
    })));
    c.bench_function("normalize_noop", |b| {
        b.iter_batched(
            || canonical.clone(),
            |mut d| d.normalize(),
            criterion::BatchSize::SmallInput,
        )
    });

    // Range editing over a large single block: one paragraph, 5000 inline spans.
    let big_block =
        Node::element("paragraph").with_children((0..5000).map(|i| Node::text(format!("w{i} "))));
    let spans = big_block.child_count();
    c.bench_function("add_mark_range_large_block", |b| {
        b.iter_batched(
            || big_block.clone(),
            |mut p| {
                let r = Range::new(Position::new(0, 0), Position::new(spans, 0));
                p.add_mark_range(black_box(r), Mark::new("bold")).unwrap()
            },
            criterion::BatchSize::SmallInput,
        )
    });
    c.bench_function("delete_range_large_block", |b| {
        b.iter_batched(
            || big_block.clone(),
            |mut p| {
                let r = Range::new(Position::new(1000, 0), Position::new(4000, 0));
                p.delete_range(black_box(r)).unwrap()
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Transaction: record a handful of edits on `big_doc`. Returns the doc (and
    // log) so their deallocation falls outside the timed section.
    c.bench_function("transform_small_txn", |b| {
        b.iter_batched(
            || document.clone(),
            |mut d| {
                let mut tx = d.root_mut().transform();
                tx.set_attr(vec![0], "textAlign", "center").unwrap();
                tx.set_text(vec![0, 0], Some("edited".into())).unwrap();
                tx.insert(vec![], 0, Node::element("paragraph")).unwrap();
                let changes = tx.finish();
                (d, changes)
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Block-structural ops on `big_doc` (500 paragraphs). Return the doc so its
    // deallocation falls outside the timed section.
    c.bench_function("wrap_range_500_blocks", |b| {
        b.iter_batched(
            || document.clone(),
            |mut d| {
                let r = BlockRange::new(vec![], 0, 500);
                d.root_mut()
                    .wrap_range(black_box(&r), "section", None)
                    .unwrap();
                d
            },
            criterion::BatchSize::SmallInput,
        )
    });
    c.bench_function("split_block", |b| {
        b.iter_batched(
            || document.clone(),
            |mut d| {
                d.root_mut().split_block(black_box(&[0]), 10, 0).unwrap();
                d
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Change-list algebra. `compact` over a redundant list (many writes to a few
    // paths); `map_path` carrying a path through the 500-move reorder patch.
    let redundant: Vec<tiptap_rusty_parser::Change> = (0..2000)
        .map(|i| tiptap_rusty_parser::Change::SetText {
            path: vec![i % 100, 0],
            text: Some(format!("v{i}")),
        })
        .collect();
    c.bench_function("compact_redundant", |b| {
        b.iter(|| black_box(tiptap_rusty_parser::compact(black_box(&redundant)).len()))
    });
    c.bench_function("map_path_through_reorder", |b| {
        b.iter(|| {
            black_box(tiptap_rusty_parser::map_path(
                black_box(&[250, 0]),
                &reorder_changes,
            ))
        })
    });

    // Flat ProseMirror positions over `big_doc`: total size, and resolving a
    // mid-document position to a ResolvedPos.
    c.bench_function("content_size", |b| {
        b.iter(|| black_box(document.root().content_size()))
    });
    let mid = document.root().content_size() / 2;
    c.bench_function("resolve_pos", |b| {
        b.iter(|| black_box(document.root().resolve(black_box(mid)).unwrap()))
    });

    // Inline (character-level) diff: a single-word edit in the middle of a
    // ~10k-char paragraph — prefix/suffix trim should keep this near-linear.
    let long: String = (0..2000).map(|i| format!("w{i} ")).collect();
    let big_para =
        Node::element("doc").with_child(Node::element("paragraph").with_child(Node::text(&long)));
    let mut edited = long.clone();
    let cut = edited.len() / 2;
    edited.insert_str(cut, "EDIT ");
    let big_para_edited =
        Node::element("doc").with_child(Node::element("paragraph").with_child(Node::text(&edited)));
    let inline = DiffOptions {
        text: DiffGranularity::Inline,
    };
    c.bench_function("diff_inline_10k_para_small_edit", |b| {
        b.iter(|| black_box(big_para.diff_with(black_box(&big_para_edited), &inline)))
    });
}

criterion_group!(g, benches);
criterion_main!(g);
