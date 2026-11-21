//! M19.1: every example starts and every Route answers without a 5xx.

use std::{
    io::{Read, Write},
    net::TcpStream,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use fauxfield_store::examples;

fn request(port: u16, method: &str, path: &str) -> Option<u16> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let body = if method == "GET" { "" } else { "{}" };
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    )
    .ok()?;
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out.split_whitespace().nth(1)?.parse().ok()
}

struct Host(Child);
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn every_route_of_every_example_answers_without_5xx() {
    let bin = assert_cmd::cargo::cargo_bin("fauxfield");
    for (n, m) in examples::EXAMPLES.iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("ws");
        std::fs::create_dir(&root).unwrap();
        examples::init_example(&root, m.name, false).unwrap();
        let offset = 20000 + (n as u16) * 50;
        let child = Command::new(&bin)
            .args([
                "run",
                root.to_str().unwrap(),
                "--port-offset",
                &offset.to_string(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let _host = Host(child);
        let servers = examples::example_servers(m.name);
        let deadline = Instant::now() + Duration::from_secs(20);
        for s in &servers {
            while TcpStream::connect(("127.0.0.1", s.port + offset)).is_err() {
                assert!(
                    Instant::now() < deadline,
                    "{}/{} did not start",
                    m.name,
                    s.slug
                );
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        for (p, bytes) in examples::example_files(m.name) {
            if !p.starts_with("servers/") {
                continue;
            }
            let v: toml::Value = toml::from_str(std::str::from_utf8(bytes).unwrap()).unwrap();
            let port = v["port"].as_integer().unwrap() as u16 + offset;
            for r in v["routes"].as_array().unwrap() {
                let kind = r.get("kind").and_then(|k| k.as_str()).unwrap_or("http");
                if kind == "websocket" {
                    continue;
                }
                let method = r.get("method").and_then(|k| k.as_str()).unwrap_or("GET");
                let mut path = r["path"].as_str().unwrap().replace("/*", "/index.html");
                while let Some(i) = path.find("/:") {
                    let end = path[i + 1..].find('/').map_or(path.len(), |e| e + i + 1);
                    path.replace_range(i + 1..end, "1");
                }
                let status = request(port, method, &path)
                    .unwrap_or_else(|| panic!("{}: {method} {path} got no response", m.name));
                assert!(status < 500, "{}: {method} {path} -> {status}", m.name);
            }
        }
    }
}

/// The template preview (what the app shows under a Response body) renders every Response of
/// every example without a template error or invalid JSON. The preview uses the Route path as
/// the request, so a path param renders as `:id`: bodies must quote it.
#[test]
fn every_response_previews_without_errors() {
    use fauxfield_engine::{SampleRequest, preview};
    let headers = vec![
        ("accept".to_string(), "application/json".to_string()),
        ("content-type".to_string(), "application/json".to_string()),
        (
            "authorization".to_string(),
            "Bearer sample-token".to_string(),
        ),
    ];
    let mut bad = Vec::new();
    for m in examples::EXAMPLES {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("ws");
        std::fs::create_dir(&root).unwrap();
        examples::init_example(&root, m.name, false).unwrap();
        let snap = fauxfield_store::Store::open(&root).unwrap().snapshot();
        for (file, server) in &snap.servers {
            for route in server.routes.iter().filter(|r| r.kind == "http") {
                for resp in &route.responses {
                    let sample = SampleRequest {
                        method: route.method.clone(),
                        url: route.path.clone(),
                        headers: headers.clone(),
                        body: String::new(),
                        datasets: None,
                    };
                    let p = preview(&root, server, route, resp, &sample, None);
                    if let Some(e) = p.error.or(p.json_error) {
                        bad.push(format!(
                            "{}/{file} {} {} [{}]: {e}",
                            m.name,
                            route.method,
                            route.path,
                            resp.name.clone().unwrap_or_default()
                        ));
                    }
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{} preview problems:\n{}",
        bad.len(),
        bad.join("\n")
    );
}
