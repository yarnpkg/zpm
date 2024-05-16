use criterion::{criterion_group, criterion_main, Criterion};
use zpm::project::tree;

fn install_benchmark(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("tree", |b| {
        b.to_async(&runtime).iter(|| async {
            println!("Running tree");
            tree().await.unwrap();
            panic!("tree");
        });
    });
}

criterion_group!(benches, install_benchmark);
criterion_main!(benches);
