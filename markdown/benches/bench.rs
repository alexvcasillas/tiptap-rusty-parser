use criterion::{criterion_group, criterion_main, Criterion};

fn benches(_c: &mut Criterion) {}

criterion_group!(g, benches);
criterion_main!(g);
