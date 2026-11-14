//! Built-in templates (SPEC §12.2). The gallery UI arrives in M15; M13 registers the
//! *Realtime feed* template so the list already has an entry to show.

/// A built-in template: a Server fragment the gallery adds to the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinTemplate {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// A complete Server file (`id`/`port` are replaced when it is added to a workspace).
    pub server_toml: &'static str,
}

const REALTIME_FEED: &str = r#"id = "srv_realtime"
name = "Realtime feed"
port = 3100

[[routes]]
id = "rte_feed"
name = "Feed"
kind = "websocket"
path = "/ws/feed"
mode = "script"

  [[routes.messages]]
  trigger = "connect"
  body = '{"type":"hello","connection":"{{ connection.id }}","at":"{{ now() }}"}'

  [[routes.messages]]
  trigger = "interval"
  every = "5s"
  body = '{"type":"tick","value":{{ random_int(min=0, max=100) }}}'

  [[routes.messages]]
  trigger = "message"
  when = { target = "body", key = "$.type", op = "eq", value = "ping" }
  body = '{"type":"pong"}'
"#;

/// Every built-in template, in gallery order.
pub fn builtin_templates() -> &'static [BuiltinTemplate] {
    &[BuiltinTemplate {
        id: "realtime-feed",
        name: "Realtime feed (WebSocket)",
        description: "A WebSocket feed: hello on connect, a tick every 5 seconds, pong for ping.",
        server_toml: REALTIME_FEED,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_parse_and_validate() {
        for t in builtin_templates() {
            let s = fauxfield_model::parse_server(t.server_toml).unwrap();
            let d = fauxfield_model::validate_servers(&[("servers/t.toml".into(), s)]);
            assert!(d.is_empty(), "{}: {d:?}", t.id);
        }
    }
}
