//! Published API documentation at the socket level (SPEC §12.4, M17.06).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use fauxfield_engine::{Engine, EngineConfig, TrafficFilter};
use fauxfield_model::parse_server;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn server_toml(port: u16, top: &str) -> String {
    format!(
        r#"
id = "srv_a"
name = "S"
port = {port}
{top}
[[routes]]
id = "rte_api"
method = "GET"
path = "/api"
[[routes.responses]]
id = "rsp_1"
body = '{{"ok":true}}'
[[routes]]
id = "rte_shadow"
method = "GET"
path = "/__docs/x"
[[routes.responses]]
id = "rsp_2"
body = "shadowed"
"#
    )
}

struct Fx {
    _rt: tokio::runtime::Runtime,
    e: Engine,
    port: u16,
    _dir: tempfile::TempDir,
}

fn fx(top: &str) -> Fx {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::new(
        rt.handle().clone(),
        EngineConfig {
            root: dir.path().to_path_buf(),
            ..Default::default()
        },
    );
    let port = free_port();
    e.apply(vec![parse_server(&server_toml(port, top)).unwrap()]);
    e.start("srv_a").unwrap();
    Fx {
        _rt: rt,
        e,
        port,
        _dir: dir,
    }
}

fn req(port: u16, method: &str, path: &str, headers: &[(&str, &str)]) -> (String, Vec<u8>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
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
    (
        String::from_utf8_lossy(&buf[..i]).to_string(),
        buf.get(i + 4..).unwrap_or_default().to_vec(),
    )
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

const ON: &str = "[docs_site]\nenabled = true\n";

#[test]
fn table_rows() {
    let f = fx(ON);
    let p = f.port;
    let (h, _) = req(p, "GET", "/__docs", &[]);
    assert_eq!(status(&h), 308);
    assert_eq!(header(&h, "location"), Some("/__docs/"));

    let (h, b) = req(p, "GET", "/__docs/", &[]);
    assert_eq!(status(&h), 200);
    assert!(header(&h, "content-type").unwrap().starts_with("text/html"));
    assert!(
        header(&h, "content-security-policy")
            .unwrap()
            .contains("default-src 'self'")
    );
    assert_eq!(header(&h, "x-content-type-options"), Some("nosniff"));
    assert!(!String::from_utf8_lossy(&b).contains("<script>"));

    let (h, b) = req(p, "GET", "/__docs/openapi.json", &[]);
    assert_eq!(status(&h), 200);
    let etag = header(&h, "etag").unwrap().to_string();
    assert_eq!(header(&h, "cache-control"), Some("no-cache"));
    let doc: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(doc["openapi"], "3.1.0");
    assert!(doc["paths"]["/api"]["get"].is_object());
    let (h, _) = req(
        p,
        "GET",
        "/__docs/openapi.json",
        &[("if-none-match", &etag)],
    );
    assert_eq!(status(&h), 304);

    // assets: gzip negotiation and immutable caching
    let (h, b) = req(
        p,
        "GET",
        "/__docs/assets/swagger-ui-bundle.js",
        &[("accept-encoding", "gzip")],
    );
    assert_eq!(status(&h), 200);
    assert_eq!(header(&h, "content-encoding"), Some("gzip"));
    assert!(header(&h, "cache-control").unwrap().contains("immutable"));
    assert_eq!(&b[..2], &[0x1f, 0x8b]);
    let (h, b) = req(p, "GET", "/__docs/assets/swagger-ui-bundle.js", &[]);
    assert_eq!(header(&h, "content-encoding"), None);
    assert!(b.len() > 1_000_000);
    assert_eq!(status(&req(p, "GET", "/__docs/assets/init.js", &[]).0), 200);

    // HEAD has headers but no body; other methods 405; unknown 404.
    let (h, b) = req(p, "HEAD", "/__docs/", &[]);
    assert_eq!(status(&h), 200);
    assert!(b.is_empty());
    let (h, _) = req(p, "POST", "/__docs/", &[]);
    assert_eq!(status(&h), 405);
    assert_eq!(header(&h, "allow"), Some("GET, HEAD"));
    let (h, _) = req(p, "GET", "/__docs/assets/nope.js", &[]);
    assert_eq!(status(&h), 404);
    assert!(header(&h, "content-type").unwrap().contains("problem+json"));
    // The docs path wins over a Route under it (W096).
    assert_eq!(status(&req(p, "GET", "/__docs/x", &[]).0), 404);
    // Ordinary Routes are untouched.
    assert_eq!(status(&req(p, "GET", "/api", &[]).0), 200);
}

#[test]
fn not_recorded_and_off_by_default() {
    let f = fx(ON);
    req(f.port, "GET", "/__docs/", &[]);
    req(f.port, "GET", "/__docs/openapi.json", &[]);
    assert!(f.e.traffic(&TrafficFilter::default()).is_empty());
    req(f.port, "GET", "/api", &[]);
    assert_eq!(f.e.traffic(&TrafficFilter::default()).len(), 1);

    let off = fx("");
    assert_eq!(status(&req(off.port, "GET", "/__docs/", &[]).0), 404);
}

#[test]
fn hot_reload_toggles_and_repaths() {
    let f = fx(ON);
    assert_eq!(status(&req(f.port, "GET", "/__docs/", &[]).0), 200);
    let apply = |top: &str| {
        f.e.apply(vec![parse_server(&server_toml(f.port, top)).unwrap()])
    };
    apply("[docs_site]\nenabled = true\npath = \"/spec\"\n");
    assert_eq!(status(&req(f.port, "GET", "/spec/", &[]).0), 200);
    assert_eq!(status(&req(f.port, "GET", "/__docs/", &[]).0), 404);
    apply("");
    assert_eq!(status(&req(f.port, "GET", "/spec/", &[]).0), 404);
    apply(ON);
    assert_eq!(status(&req(f.port, "GET", "/__docs/", &[]).0), 200);
}

#[test]
fn prefix_independent_and_servers_url() {
    let f = fx(&format!("prefix = \"/api/v1\"\n{ON}"));
    assert_eq!(status(&req(f.port, "GET", "/__docs/", &[]).0), 200);
    let (_, b) = req(f.port, "GET", "/__docs/openapi.json", &[]);
    let doc: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(doc["servers"][0]["url"], "/api/v1");
}

/// M17.1: a Dataset-reading template on an operation Response renders against sample rows, and
/// the Response `schema` replaces the built-in array shape for that status.
#[test]
fn operation_examples_render_dataset_templates_and_use_the_response_schema() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    std::fs::create_dir_all(dir.path().join("schemas")).unwrap();
    std::fs::write(
        dir.path().join("data/products.json"),
        r#"[{"id":1,"name":"A"},{"id":2,"name":"B"}]"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("schemas/list.json"),
        r#"{"type":"object","properties":{"items":{"type":"array"},"meta":{"type":"object"}}}"#,
    )
    .unwrap();
    let s = parse_server(
        r#"
id = "srv_p"
name = "P"
port = 1
[[datasets]]
name = "products"
seed = "data/products.json"
[[routes]]
id = "rte_p"
kind = "crud"
path = "/products"
dataset = "products"
[[routes.operations.list.responses]]
id = "rsp_env"
name = "Envelope"
status = 200
schema = "schemas/list.json"
body = '{"items": {{ data.products | json_encode() | safe }}, "meta": {"total": {{ dataset_count(data=data.products) }}}}'
"#,
    )
    .unwrap();
    let doc = fauxfield_engine::openapi_document(dir.path(), "srv_p", &s);
    let ok =
        &doc.value["paths"]["/products"]["get"]["responses"]["200"]["content"]["application/json"];
    assert_eq!(
        ok["examples"]["Envelope"]["value"]["meta"]["total"], 2,
        "{ok}"
    );
    assert_eq!(ok["examples"]["Envelope"]["value"]["items"][0]["name"], "A");
    assert_eq!(ok["schema"]["type"], "object", "{ok}");
}
