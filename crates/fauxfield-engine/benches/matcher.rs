//! Route matcher benchmark: criterion mean plus a p99 harness (< 20 µs at 1,000 routes).
use std::time::Instant;

use criterion::{Criterion, black_box};
use fauxfield_engine::RouteTable;

fn table() -> RouteTable {
    let mut t = String::from("id=\"srv_b\"\nname=\"B\"\nport=1\n");
    for i in 0..700 {
        t += &format!("[[routes]]\nid=\"e{i}\"\npath=\"/api/r{i}/items\"\n");
    }
    for i in 0..230 {
        t += &format!("[[routes]]\nid=\"p{i}\"\npath=\"/svc{i}/:id/sub/:sid\"\n");
    }
    for i in 0..20 {
        t += &format!("[[routes]]\nid=\"w{i}\"\npath=\"/files{i}/*rest\"\n");
    }
    for i in 0..50 {
        t += &format!(
            "[[routes]]\nid=\"x{i}\"\nmatch=\"regex\"\npath='^/v{i}/orders/(?P<id>[0-9]+)$'\n"
        );
    }
    RouteTable::compile(&fauxfield_model::parse_server(&t).unwrap())
}

const REQS: &[&str] = &[
    "/api/r350/items",
    "/svc120/42/sub/7",
    "/files5/a/b/c.txt",
    "/v25/orders/1234",
    "/nope/missing",
];

fn p99(t: &RouteTable) {
    let n = 200_000;
    let mut s: Vec<u128> = Vec::with_capacity(n);
    for i in 0..n {
        let st = Instant::now();
        black_box(t.find("GET", REQS[i % REQS.len()]));
        s.push(st.elapsed().as_nanos());
    }
    s.sort_unstable();
    let p = s[n * 99 / 100];
    println!("matcher_p99: {} ns", p);
    if !cfg!(debug_assertions) {
        assert!(p < 20_000, "p99 {p} ns >= 20 µs");
    }
}

fn main() {
    let t = table();
    assert_eq!(t.len(), 1000);
    let mut c = Criterion::default().configure_from_args();
    c.bench_function("match_1000_routes", |b| {
        let mut i = 0;
        b.iter(|| {
            i += 1;
            black_box(t.find("GET", REQS[i % REQS.len()]))
        })
    });
    c.final_summary();
    p99(&t);
}
