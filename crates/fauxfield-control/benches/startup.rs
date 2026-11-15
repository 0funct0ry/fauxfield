//! SPEC §15: headless part of cold start (open Store, build Engine, apply 10 Servers / 500 Routes,
//! bind every listener) must leave room in the 400 ms budget; asserted at < 150 ms in release.
//! Also hot reload: file save -> first response with the new behaviour, < 50 ms.
//! The workspace is generated into a temp dir with the same shape as `cargo xtask perf-workspace`.
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::{Duration, Instant};

use fauxfield_control::{Host, HostOptions};

fn server(s: usize, port: u16, body: &str) -> String {
    let mut t = format!(
        "schema = 1\nid = \"srv_perf_{s:02}\"\nname = \"Perf {s:02}\"\nenabled = true\nhost = \"127.0.0.1\"\nport = {port}\n"
    );
    for r in 0..50 {
        t += &format!(
            "\n[[routes]]\nid = \"rt_{s:02}_{r:02}\"\nmethod = \"GET\"\npath = \"/api/r{r:02}/items\"\n\n  [[routes.responses]]\n  id = \"rsp_{s:02}_{r:02}\"\n  default = true\n  status = 200\n  content_type = \"application/json\"\n  body = '''{body}'''\n"
        );
    }
    t
}

fn write_ws(dir: &Path, base: u16) {
    std::fs::create_dir_all(dir.join("servers")).unwrap();
    std::fs::write(dir.join("fauxfield.toml"), "schema = 1\nname = \"Perf\"\n").unwrap();
    for s in 0..10 {
        std::fs::write(
            dir.join(format!("servers/{s:02}-perf.toml")),
            server(s, base + s as u16, "{\"v\":1}"),
        )
        .unwrap();
    }
}

fn get(port: u16, path: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut o = String::new();
    s.read_to_string(&mut o).unwrap();
    o
}

fn free_base() -> u16 {
    // 10 consecutive free ports: probe from a random-ish start.
    let mut p = 20000 + (std::process::id() % 20000) as u16;
    loop {
        if (0..10).all(|i| std::net::TcpListener::bind(("127.0.0.1", p + i)).is_ok()) {
            return p;
        }
        p += 11;
    }
}

fn main() {
    let release = !cfg!(debug_assertions);
    let base = free_base();

    // Startup (best of 5 to shed disk-cache noise).
    let mut best = u128::MAX;
    for _ in 0..5 {
        let dir = tempfile::tempdir().unwrap();
        write_ws(dir.path(), base);
        let t = Instant::now();
        let host = Host::start(dir.path(), HostOptions::default()).unwrap();
        assert!(host.engine().start_all().is_empty());
        best = best.min(t.elapsed().as_micros());
        drop(host);
    }
    println!("startup_headless: {:.1} ms", best as f64 / 1000.0);
    if release {
        assert!(best < 150_000, "headless startup {best} µs >= 150 ms");
    }

    // Hot reload.
    let dir = tempfile::tempdir().unwrap();
    write_ws(dir.path(), base);
    let host = Host::start(
        dir.path(),
        HostOptions {
            watch: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(host.engine().start_all().is_empty());
    std::thread::sleep(Duration::from_millis(500));
    let file = dir.path().join("servers/00-perf.toml");
    let mut times = vec![];
    for i in 2..8 {
        let body = format!("{{\"v\":{i}}}");
        let t = Instant::now();
        std::fs::write(&file, server(0, base, &body)).unwrap();
        loop {
            if get(base, "/api/r00/items").contains(&body) {
                break;
            }
            assert!(t.elapsed() < Duration::from_secs(5), "reload never served");
            std::thread::sleep(Duration::from_millis(1));
        }
        times.push(t.elapsed().as_micros());
        std::thread::sleep(Duration::from_millis(400));
    }
    times.sort_unstable();
    let med = times[times.len() / 2];
    println!(
        "hot_reload_median: {:.1} ms (max {:.1} ms)",
        med as f64 / 1000.0,
        *times.last().unwrap() as f64 / 1000.0
    );
    if release {
        assert!(
            *times.last().unwrap() < 50_000,
            "hot reload max {} µs >= 50 ms",
            times.last().unwrap()
        );
    }
}
