//! HTTPS at the socket level (SPEC §5.8): dev CA, files mode, also_http_port, plain-HTTP refusal.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use fauxfield_engine::{Engine, EngineConfig, ServerStatus, TestRequest, tls};
use fauxfield_model::parse_server;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn server(port: u16, tls_toml: &str) -> fauxfield_model::Server {
    parse_server(&format!(
        "id=\"srv_a\"\nname=\"S\"\nport={port}\n{tls_toml}\n\
         [[routes]]\nid=\"r\"\npath=\"/hi\"\n[[routes.responses]]\nid=\"p\"\nbody=\"secure\"\n"
    ))
    .unwrap()
}

fn engine(root: &std::path::Path, data: &std::path::Path) -> (tokio::runtime::Runtime, Engine) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let e = Engine::new(
        rt.handle().clone(),
        EngineConfig {
            root: root.into(),
            data_dir: Some(data.into()),
            ..Default::default()
        },
    );
    (rt, e)
}

fn get(rt: &tokio::runtime::Runtime, e: &Engine) -> String {
    rt.block_on(e.send_test(
        "srv_a",
        &TestRequest {
            method: "GET".into(),
            path: "/hi".into(),
            ..Default::default()
        },
    ))
    .unwrap()
    .body
}

#[test]
fn dev_ca_https_and_also_http() {
    let (root, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (rt, e) = engine(root.path(), data.path());
    let (port, plain) = (free_port(), free_port());
    e.apply(vec![server(
        port,
        &format!("[tls]\nenabled=true\nalso_http_port={plain}"),
    )]);
    e.start("srv_a").unwrap();
    // Trusted through the dev CA (the client only trusts that CA).
    assert_eq!(get(&rt, &e), "secure");
    let info = &e.list()[0];
    assert!(info.https && info.cert.as_ref().unwrap().warning.is_none());
    assert!(e.base_url("srv_a").unwrap().starts_with("https://"));
    assert!(tls::ca_cert_path(data.path()).exists());
    // The also_http_port listener serves the same routes in plain HTTP.
    let mut s = TcpStream::connect(("127.0.0.1", plain)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(b"GET /hi HTTP/1.1\r\nhost: x\r\nconnection: close\r\n\r\n")
        .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    assert!(out.ends_with("secure"), "{out}");
    // Plain HTTP on the TLS port does not get an HTTP answer.
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    s.write_all(b"GET /hi HTTP/1.1\r\nhost: x\r\nconnection: close\r\n\r\n")
        .unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    assert!(!String::from_utf8_lossy(&out).contains("secure"));
    // Stopping frees both ports.
    e.stop("srv_a").unwrap();
    assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
    assert!(TcpListener::bind(("127.0.0.1", plain)).is_ok());
}

#[test]
fn files_mode_and_bad_files() {
    let (root, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let ca = tls::load_or_create_ca(data.path()).unwrap();
    let (cert, key) = tls::leaf_pems(data.path(), &ca, "127.0.0.1").unwrap();
    std::fs::create_dir_all(root.path().join("certs")).unwrap();
    std::fs::write(root.path().join("certs/s.pem"), cert).unwrap();
    std::fs::write(root.path().join("certs/s-key.pem"), key).unwrap();
    let (rt, e) = engine(root.path(), data.path());
    let port = free_port();
    e.apply(vec![server(
        port,
        "[tls]\nenabled=true\nmode=\"files\"\ncert=\"certs/s.pem\"\nkey=\"certs/s-key.pem\"",
    )]);
    e.start("srv_a").unwrap();
    assert_eq!(get(&rt, &e), "secure");
    assert_eq!(e.list()[0].cert.as_ref().unwrap().mode, "files");
    e.stop("srv_a").unwrap();
    // Missing files: Failed status, not a panic.
    e.apply(vec![server(
        port,
        "[tls]\nenabled=true\nmode=\"files\"\ncert=\"nope.pem\"\nkey=\"nope-key.pem\"",
    )]);
    assert!(e.start("srv_a").is_err());
    assert!(matches!(e.list()[0].status, ServerStatus::Failed { .. }));
}
