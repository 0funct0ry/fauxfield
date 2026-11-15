//! SPEC §15: template render, 1 KB output with 10 fake calls, < 100 µs (compile cache warm).

use criterion::Criterion;
use fauxfield_engine::template::{Input, render};
use std::path::Path;
use std::time::Instant;

fn main() {
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
    let mut c = Criterion::default().configure_from_args();
    c.bench_function("render_1kb_10_fake", |b| {
        b.iter(|| render(src, &input).unwrap())
    });
    c.final_summary();
    // Hard budget (release only): mean over 20k renders < 100 µs.
    let t = Instant::now();
    for _ in 0..20_000 {
        render(src, &input).unwrap();
    }
    let mean = t.elapsed().as_nanos() / 20_000;
    println!("template_render_mean: {mean} ns");
    if !cfg!(debug_assertions) {
        assert!(mean < 100_000, "template render {mean} ns >= 100 µs");
    }
}
