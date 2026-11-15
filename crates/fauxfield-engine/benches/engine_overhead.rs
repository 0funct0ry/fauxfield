//! SPEC §15: engine overhead, static response, no delay, p99 < 0.5 ms (loopback, keep-alive,
//! one connection; the number includes the client's own socket work, so it is an upper bound).
//! Also the throughput budget: > 30k req/s on 8 cores (8 keep-alive connections, loopback).
//! The p99 and throughput budgets assert only in release; throughput asserts only when
//! `FAUXFIELD_BENCH_DEDICATED=1` (shared runners report only).
use std::time::{Duration, Instant};

use fauxfield_engine::{Engine, EngineConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const REQ: &[u8] = b"GET /api/ping HTTP/1.1\r\nHost: x\r\n\r\n";

fn engine(rt: &tokio::runtime::Runtime) -> (Engine, std::net::SocketAddr) {
    let src = "id=\"srv_o\"\nname=\"O\"\nhost=\"127.0.0.1\"\nport=0\n[[routes]]\nid=\"ping\"\nmethod=\"GET\"\npath=\"/api/ping\"\n[[routes.responses]]\nid=\"r1\"\ndefault=true\nstatus=200\ncontent_type=\"application/json\"\nbody='{\"ok\":true}'\n";
    let e = Engine::new(rt.handle().clone(), EngineConfig::default());
    e.apply(vec![fauxfield_model::parse_server(src).unwrap()]);
    let addr = e.start("srv_o").unwrap();
    (e, addr)
}

/// Send one request and read until the full response (Content-Length framed) arrived.
async fn one(s: &mut TcpStream, buf: &mut Vec<u8>) {
    s.write_all(REQ).await.unwrap();
    buf.clear();
    let mut tmp = [0u8; 2048];
    loop {
        let n = s.read(&mut tmp).await.unwrap();
        assert!(n > 0, "connection closed");
        buf.extend_from_slice(&tmp[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
            let len: usize = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .map(|v| v.trim().parse().unwrap())
                .unwrap_or(0);
            if buf.len() >= i + 4 + len {
                return;
            }
        }
    }
}

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(8)
        .enable_all()
        .build()
        .unwrap();
    let (_e, addr) = engine(&rt);
    let release = !cfg!(debug_assertions);

    // p99 latency.
    let mut lat = rt.block_on(async {
        let mut s = TcpStream::connect(addr).await.unwrap();
        s.set_nodelay(true).unwrap();
        let mut buf = vec![];
        for _ in 0..2_000 {
            one(&mut s, &mut buf).await;
        }
        let mut v = Vec::with_capacity(50_000);
        for _ in 0..50_000 {
            let t = Instant::now();
            one(&mut s, &mut buf).await;
            v.push(t.elapsed().as_nanos());
        }
        v
    });
    lat.sort_unstable();
    let p99 = lat[lat.len() * 99 / 100];
    println!("engine_overhead_p50: {} ns", lat[lat.len() / 2]);
    println!("engine_overhead_p99: {p99} ns");
    if release {
        assert!(p99 < 500_000, "engine overhead p99 {p99} ns >= 0.5 ms");
    }

    // Throughput: 8 connections for 3 s.
    let total = rt.block_on(async {
        let end = Instant::now() + Duration::from_secs(3);
        let tasks: Vec<_> = (0..8)
            .map(|_| {
                tokio::spawn(async move {
                    let mut s = TcpStream::connect(addr).await.unwrap();
                    s.set_nodelay(true).unwrap();
                    let (mut buf, mut n) = (vec![], 0u64);
                    while Instant::now() < end {
                        one(&mut s, &mut buf).await;
                        n += 1;
                    }
                    n
                })
            })
            .collect();
        let mut t = 0;
        for h in tasks {
            t += h.await.unwrap();
        }
        t
    });
    let rps = total as f64 / 3.0;
    println!("throughput_rps: {rps:.0}");
    if release && std::env::var("FAUXFIELD_BENCH_DEDICATED").is_ok_and(|v| v == "1") {
        assert!(rps > 30_000.0, "throughput {rps:.0} req/s <= 30k");
    }
}
