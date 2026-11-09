//! SPEC §15: template render, 1 KB output with 10 fake calls, < 100 µs (compile cache warm).

use criterion::{Criterion, criterion_group, criterion_main};
use fauxfield_engine::template::{Input, render};
use std::path::Path;

fn bench(c: &mut Criterion) {
    let src = "{% for i in range(end=10) %}{\"id\":{{ i }},\"name\":\"{{ fake(kind='name') }}\",\"email\":\"{{ fake(kind='internet.email') }}\",\"city\":\"{{ fake(kind='address.city') }}\"},{% endfor %}";
    let input = Input {
        context: serde_json::json!({"request": {"method": "GET"}}),
        root: Path::new("."),
        seed: Some(1),
        route_id: "r",
        counter: 1,
        seq: None,
    };
    render(src, &input).unwrap();
    c.bench_function("render_1kb_10_fake", |b| {
        b.iter(|| render(src, &input).unwrap())
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
