//! WebSocket routes at the socket level (SPEC §7.4, M13).

use std::net::TcpListener;
use std::time::Duration;

use fauxfield_engine::{Engine, EngineConfig};
use fauxfield_model::parse_server;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Fx {
    _srv_rt: tokio::runtime::Runtime,
    rt: tokio::runtime::Runtime,
    e: Engine,
    port: u16,
    _dir: tempfile::TempDir,
}

fn fx(routes: &str) -> Fx {
    let srv_rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::new(
        srv_rt.handle().clone(),
        EngineConfig {
            root: dir.path().to_path_buf(),
            ..Default::default()
        },
    );
    let port = free_port();
    let toml = format!("id = \"srv_a\"\nname = \"S\"\nport = {port}\n{routes}");
    e.apply(vec![parse_server(&toml).unwrap()]);
    e.start("srv_a").unwrap();
    Fx {
        _srv_rt: srv_rt,
        rt: tokio::runtime::Runtime::new().unwrap(),
        e,
        port,
        _dir: dir,
    }
}

const SCRIPT: &str = r#"
[[routes]]
id = "rte_ws"
kind = "websocket"
path = "/ws/events"
mode = "script"
  [[routes.messages]]
  trigger = "connect"
  body = '{"type":"hello","conn":"{{ connection.id }}"}'
  [[routes.messages]]
  trigger = "interval"
  every = "50ms"
  body = '{"type":"tick","n":{{ random_int(min=0, max=100) }}}'
  [[routes.messages]]
  trigger = "message"
  when = { target = "body", key = "$.type", op = "eq", value = "ping" }
  body = '{"type":"pong","echo":"{{ message.json.type }}"}'
"#;

async fn next_text<S>(ws: &mut S) -> String
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        let m = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("timed out")
            .expect("closed")
            .expect("error");
        if let Message::Text(t) = m {
            return t.to_string();
        }
    }
}

#[test]
fn hello_ticks_pong_and_manual_send() {
    let f = fx(SCRIPT);
    f.rt.block_on(async {
        let (mut ws, resp) = connect_async(format!("ws://127.0.0.1:{}/ws/events", f.port))
            .await
            .unwrap();
        assert_eq!(resp.status(), 101);
        let hello = next_text(&mut ws).await;
        assert!(
            hello.contains("\"hello\"") && hello.contains("wsc_"),
            "{hello}"
        );
        let t1 = next_text(&mut ws).await;
        let t2 = next_text(&mut ws).await;
        assert!(t1.contains("tick") && t2.contains("tick"));

        ws.send(Message::text(r#"{"type":"ping"}"#)).await.unwrap();
        loop {
            let m = next_text(&mut ws).await;
            if m.contains("pong") {
                assert!(m.contains("\"echo\":\"ping\""), "{m}");
                break;
            }
        }
        // A protocol-level ping gets a protocol pong.
        ws.send(Message::Ping(vec![1, 2].into())).await.unwrap();
        loop {
            let m = tokio::time::timeout(Duration::from_secs(5), ws.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            if matches!(m, Message::Pong(_)) {
                break;
            }
        }

        let conns = f.e.ws_connections("srv_a", None).unwrap();
        assert_eq!(conns.len(), 1);
        let n =
            f.e.ws_send("srv_a", Some("rte_ws"), None, "from the GUI", false)
                .unwrap();
        assert_eq!(n, 1);
        loop {
            if next_text(&mut ws).await == "from the GUI" {
                break;
            }
        }
        let frames = f.e.ws_messages("srv_a", None, None, 0, 0).unwrap();
        assert!(frames.iter().any(|x| x.dir == "in" && x.kind == "text"));
        assert!(
            frames
                .iter()
                .any(|x| x.dir == "out" && x.body == "from the GUI")
        );
        // The 101 upgrade is in Traffic.
        let t = f.e.traffic(&Default::default());
        assert!(
            t.iter()
                .any(|r| r.status == 101 && r.response_id == conns[0].id)
        );

        f.e.ws_disconnect("srv_a", &conns[0].id, Some(4001))
            .unwrap();
        loop {
            match tokio::time::timeout(Duration::from_secs(5), ws.next())
                .await
                .unwrap()
            {
                Some(Ok(Message::Close(Some(cf)))) => {
                    assert_eq!(u16::from(cf.code), 4001);
                    break;
                }
                Some(Ok(_)) => continue,
                other => panic!("{other:?}"),
            }
        }
    });
    std::thread::sleep(Duration::from_millis(100));
    assert!(f.e.ws_connections("srv_a", None).unwrap().is_empty());
}

#[test]
fn echo_binary_and_plain_get_is_426() {
    let f = fx("[[routes]]\nid=\"rte_e\"\nkind=\"websocket\"\npath=\"/echo\"\n");
    f.rt.block_on(async {
        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{}/echo", f.port))
            .await
            .unwrap();
        ws.send(Message::text("hi")).await.unwrap();
        assert_eq!(next_text(&mut ws).await, "hi");
        ws.send(Message::Binary(vec![9, 8, 7].into()))
            .await
            .unwrap();
        loop {
            if let Message::Binary(b) = ws.next().await.unwrap().unwrap() {
                assert_eq!(&b[..], &[9, 8, 7]);
                break;
            }
        }
    });
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", f.port)).unwrap();
    s.write_all(b"GET /echo HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    assert!(out.starts_with("HTTP/1.1 426"), "{out}");
}

#[test]
fn broadcast_reaches_every_client_and_stop_closes() {
    let f = fx(r#"
[[routes]]
id = "rte_b"
kind = "websocket"
path = "/feed"
mode = "broadcast"
  [[routes.messages]]
  trigger = "interval"
  every = "50ms"
  body = "tick"
"#);
    f.rt.block_on(async {
        let url = format!("ws://127.0.0.1:{}/feed", f.port);
        let (mut a, _) = connect_async(&url).await.unwrap();
        let (mut b, _) = connect_async(&url).await.unwrap();
        assert_eq!(next_text(&mut a).await, "tick");
        assert_eq!(next_text(&mut b).await, "tick");
        // Rebroadcast of a client message to the others.
        a.send(Message::text("hello all")).await.unwrap();
        loop {
            if next_text(&mut b).await == "hello all" {
                break;
            }
        }
    });
    f.e.stop("srv_a").unwrap();
}

#[test]
fn message_format_compacts_json_by_default() {
    let f = fx(r#"
[[routes]]
id = "rte_f"
kind = "websocket"
path = "/fmt"
mode = "script"
  [[routes.messages]]
  trigger = "message"
  body = '''
  {
  		"id": "{{ message.text }}",
      "n": 1
  }'''
  [[routes.messages]]
  trigger = "connect"
  format = "pretty"
  body = '{"a":1,"b":[2]}'
  [[routes.messages]]
  trigger = "connect"
  format = "raw"
  body = '{ "a" : 1 }'
  [[routes.messages]]
  trigger = "connect"
  body = 'not   json'
"#);
    f.rt.block_on(async {
        let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{}/fmt", f.port))
            .await
            .unwrap();
        assert_eq!(
            next_text(&mut ws).await,
            "{\n  \"a\": 1,\n  \"b\": [\n    2\n  ]\n}"
        );
        assert_eq!(next_text(&mut ws).await, "{ \"a\" : 1 }");
        assert_eq!(next_text(&mut ws).await, "not   json");
        ws.send(Message::text("abc")).await.unwrap();
        assert_eq!(next_text(&mut ws).await, r#"{"id":"abc","n":1}"#);
    });
}

#[test]
fn adhoc_send_renders_templates_per_connection() {
    let f = fx("[[routes]]\nid=\"rte_e\"\nkind=\"websocket\"\npath=\"/echo\"\n");
    f.rt.block_on(async {
        let url = format!("ws://127.0.0.1:{}/echo", f.port);
        let (mut a, _) = connect_async(&url).await.unwrap();
        let (mut b, _) = connect_async(&url).await.unwrap();
        // Wait until both are registered.
        while f.e.ws_connections("srv_a", None).unwrap().len() < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let n =
            f.e.ws_send_rendered(
                "srv_a",
                Some("rte_e"),
                None,
                "{\n  \"id\": \"{{ connection.id }}\"\n}",
                false,
                None,
            )
            .unwrap();
        assert_eq!(n, 2);
        let (ma, mb) = (next_text(&mut a).await, next_text(&mut b).await);
        assert!(ma.starts_with("{\"id\":\"wsc_") && ma != mb, "{ma} {mb}");
        let err =
            f.e.ws_send_rendered("srv_a", None, None, "{{ nope(", false, None)
                .unwrap_err();
        assert!(!err.to_string().is_empty());
    });
}
