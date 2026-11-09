//! Static routes and CORS at the socket level (SPEC §5.7, §5.9).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use fauxfield_engine::{Engine, EngineConfig};
use fauxfield_model::parse_server;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Fx {
    _rt: tokio::runtime::Runtime,
    e: Engine,
    port: u16,
    dir: tempfile::TempDir,
}

fn fx(top: &str) -> Fx {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let pubdir = dir.path().join("public");
    std::fs::create_dir_all(pubdir.join("sub")).unwrap();
    std::fs::write(pubdir.join("app.js"), "0123456789abcdefghij").unwrap();
    std::fs::write(pubdir.join("sub/a.css"), "body{}").unwrap();
    std::fs::write(pubdir.join("index.html.tera"), "<h1>env={{ env }}</h1>").unwrap();
    std::fs::write(dir.path().join("secret.txt"), "top secret").unwrap();
    let e = Engine::new(
        rt.handle().clone(),
        EngineConfig {
            root: dir.path().to_path_buf(),
            ..Default::default()
        },
    );
    let port = free_port();
    e.apply(vec![
        parse_server(&format!(
            r#"
id = "srv_a"
name = "S"
port = {port}
{top}
[[routes]]
id = "rte_static"
kind = "static"
path = "/assets/*"
dir = "public"
index = "index.html"
template_extensions = [".tera"]
[[routes]]
id = "rte_api"
method = "GET"
path = "/api"
[[routes.responses]]
id = "rsp_1"
body = "ok"
"#
        ))
        .unwrap(),
    ]);
    e.start("srv_a").unwrap();
    Fx {
        _rt: rt,
        e,
        port,
        dir,
    }
}

impl Fx {
    /// Raw request; returns (head, body).
    fn req(&self, method: &str, path: &str, headers: &[(&str, &str)]) -> (String, Vec<u8>) {
        let mut s = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut r = format!("{method} {path} HTTP/1.1\r\nhost: x\r\nconnection: close\r\n");
        for (k, v) in headers {
            r += &format!("{k}: {v}\r\n");
        }
        s.write_all((r + "\r\n").as_bytes()).unwrap();
        let mut buf = Vec::new();
        let _ = s.read_to_end(&mut buf);
        let i = buf
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .unwrap_or(buf.len());
        let head = String::from_utf8_lossy(&buf[..i]).to_string();
        (head, buf.get(i + 4..).unwrap_or_default().to_vec())
    }
}

fn status(head: &str) -> u16 {
    head.split(' ').nth(1).unwrap().parse().unwrap()
}
fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

#[test]
fn serves_files_with_mime_etag_and_conditionals() {
    let f = fx("");
    let (h, b) = f.req("GET", "/assets/app.js", &[]);
    assert_eq!(status(&h), 200);
    assert_eq!(b, b"0123456789abcdefghij");
    assert!(header(&h, "content-type").unwrap().contains("javascript"));
    assert!(header(&h, "last-modified").is_some());
    let etag = header(&h, "etag").unwrap().to_string();
    let (h, b) = f.req("GET", "/assets/app.js", &[("if-none-match", &etag)]);
    assert_eq!(status(&h), 304);
    assert!(b.is_empty());
    let (h, _) = f.req("GET", "/assets/sub/a.css", &[]);
    assert!(header(&h, "content-type").unwrap().starts_with("text/css"));
    let (h, _) = f.req("GET", "/assets/nope.txt", &[]);
    assert_eq!(status(&h), 404);
    let (h, b) = f.req("HEAD", "/assets/app.js", &[]);
    assert_eq!(status(&h), 200);
    assert_eq!(header(&h, "content-length"), Some("20"));
    assert!(b.is_empty());
    let (h, _) = f.req("POST", "/assets/app.js", &[]);
    assert_eq!(status(&h), 404); // static routes only match GET/HEAD
}

#[test]
fn ranges() {
    let f = fx("");
    let (h, b) = f.req("GET", "/assets/app.js", &[("range", "bytes=2-5")]);
    assert_eq!(status(&h), 206);
    assert_eq!(b, b"2345");
    assert_eq!(header(&h, "content-range"), Some("bytes 2-5/20"));
    let (h, b) = f.req("GET", "/assets/app.js", &[("range", "bytes=-3")]);
    assert_eq!((status(&h), b.as_slice()), (206, b"hij".as_slice()));
    let (h, _) = f.req("GET", "/assets/app.js", &[("range", "bytes=50-")]);
    assert_eq!(status(&h), 416);
    assert_eq!(header(&h, "content-range"), Some("bytes */20"));
}

#[test]
fn index_and_template_with_environment_name() {
    let f = fx("");
    let vc = fauxfield_engine::VarContext {
        active_env: "staging".into(),
        ..Default::default()
    };
    f.e.set_var_context(vc);
    for p in ["/assets", "/assets/", "/assets/index.html"] {
        let (h, b) = f.req("GET", p, &[]);
        assert_eq!(status(&h), 200, "{p}");
        assert_eq!(String::from_utf8_lossy(&b), "<h1>env=staging</h1>", "{p}");
        assert!(header(&h, "content-type").unwrap().starts_with("text/html"));
    }
    let (h, _) = f.req("GET", "/assets/index.html.tera", &[]);
    assert!(header(&h, "content-type").unwrap().starts_with("text/html"));
}

#[test]
fn traversal_is_rejected() {
    let f = fx("");
    for p in [
        "/assets/../secret.txt",
        "/assets/%2e%2e/secret.txt",
        "/assets/sub/../../secret.txt",
        "/assets/%2e%2e%2fsecret.txt",
    ] {
        let (h, b) = f.req("GET", p, &[]);
        assert!(matches!(status(&h), 403 | 404), "{p} -> {h}");
        assert!(!String::from_utf8_lossy(&b).contains("top secret"), "{p}");
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            f.dir.path().join("secret.txt"),
            f.dir.path().join("public/link.txt"),
        )
        .unwrap();
        let (h, b) = f.req("GET", "/assets/link.txt", &[]);
        assert_ne!(status(&h), 200);
        assert!(!String::from_utf8_lossy(&b).contains("top secret"));
    }
}

const CORS: &str = r#"
[cors]
enabled = true
origins = ["http://localhost:*"]
methods = ["GET", "POST"]
credentials = true
max_age = 600
"#;

#[test]
fn cors_preflight_and_headers() {
    let f = fx(CORS);
    let (h, _) = f.req(
        "OPTIONS",
        "/api",
        &[
            ("origin", "http://localhost:5173"),
            ("access-control-request-method", "POST"),
            ("access-control-request-headers", "content-type"),
        ],
    );
    assert_eq!(status(&h), 204);
    assert_eq!(
        header(&h, "access-control-allow-origin"),
        Some("http://localhost:5173")
    );
    assert_eq!(
        header(&h, "access-control-allow-methods"),
        Some("GET, POST")
    );
    assert_eq!(
        header(&h, "access-control-allow-headers"),
        Some("content-type")
    );
    assert_eq!(header(&h, "access-control-allow-credentials"), Some("true"));
    assert_eq!(header(&h, "access-control-max-age"), Some("600"));
    // Simple request, 404 and static responses all carry the headers.
    for p in ["/api", "/missing", "/assets/app.js"] {
        let (h, _) = f.req("GET", p, &[("origin", "http://localhost:3000")]);
        assert_eq!(
            header(&h, "access-control-allow-origin"),
            Some("http://localhost:3000"),
            "{p}"
        );
        assert_eq!(header(&h, "vary"), Some("Origin"), "{p}");
    }
    // Disallowed origin: no headers.
    let (h, _) = f.req("GET", "/api", &[("origin", "http://evil.test")]);
    assert!(header(&h, "access-control-allow-origin").is_none());
}

#[test]
fn explicit_options_route_wins_over_automatic_preflight() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let e = Engine::new(rt.handle().clone(), EngineConfig::default());
    let port = free_port();
    e.apply(vec![parse_server(&format!(
        "id=\"srv_a\"\nname=\"S\"\nport={port}\n[cors]\nenabled=true\norigins=[\"*\"]\n\
         [[routes]]\nid=\"r\"\nmethod=\"OPTIONS\"\npath=\"/x\"\n[[routes.responses]]\nid=\"p\"\nstatus=202\nbody=\"mine\"\n"
    ))
    .unwrap()]);
    e.start("srv_a").unwrap();
    let f = Fx {
        _rt: rt,
        e,
        port,
        dir: tempfile::tempdir().unwrap(),
    };
    let (h, b) = f.req(
        "OPTIONS",
        "/x",
        &[
            ("origin", "http://a"),
            ("access-control-request-method", "GET"),
        ],
    );
    assert_eq!(status(&h), 202);
    assert_eq!(b, b"mine");
    assert_eq!(header(&h, "access-control-allow-origin"), Some("*"));
}
