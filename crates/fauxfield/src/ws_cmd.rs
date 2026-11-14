//! `ws list|log|send|disconnect` (M13): live WebSocket connections of a running host.
//! Every command has a stable `--json`.

use clap::Subcommand;
use fauxfield_control::ControlApi;
use serde_json::json;

use crate::cli::EXIT_OK;
use crate::host_cmd::{attach, control_failed, root_of, rt};
use crate::workspace_cmd::{WsArgs, print_json};

#[derive(Subcommand, Debug)]
pub enum WsAction {
    /// List live connections of a Server.
    List {
        /// Server ID, name or slug.
        server: String,
        /// Only this WebSocket Route ID.
        #[arg(long)]
        route: Option<String>,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Print the message timeline (in/out frames).
    Log {
        server: String,
        #[arg(long)]
        route: Option<String>,
        /// Only this connection ID.
        #[arg(long)]
        connection: Option<String>,
        /// Only frames with an id greater than this.
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Send a message to one connection or to all of them.
    Send {
        server: String,
        /// Message body (text, or base64 with --binary).
        body: String,
        #[arg(long)]
        route: Option<String>,
        /// One connection ID; omit to send to every connection.
        #[arg(long)]
        connection: Option<String>,
        /// Send a binary frame (body is base64).
        #[arg(long)]
        binary: bool,
        #[command(flatten)]
        ws: WsArgs,
    },
    /// Close a connection.
    Disconnect {
        server: String,
        connection: String,
        /// Close code (default 1000).
        #[arg(long)]
        code: Option<u16>,
        #[command(flatten)]
        ws: WsArgs,
    },
}

fn api(ws: &WsArgs) -> Result<fauxfield_control::RemoteControl, i32> {
    attach(&root_of(ws)).map(|(_, c)| c)
}

pub fn ws(action: WsAction) -> i32 {
    match action {
        WsAction::List { server, route, ws } => {
            let c = match api(&ws) {
                Ok(c) => c,
                Err(code) => return code,
            };
            match rt().block_on(c.ws_connections(server, route)) {
                Ok(list) => {
                    if ws.json {
                        print_json(&json!({ "connections": list }));
                    } else if list.is_empty() {
                        println!("No open connections.");
                    } else {
                        for i in list {
                            println!(
                                "{}  {}  {}  in {} out {}",
                                i.id, i.route_id, i.peer, i.messages_in, i.messages_out
                            );
                        }
                    }
                    EXIT_OK
                }
                Err(e) => control_failed(e),
            }
        }
        WsAction::Log {
            server,
            route,
            connection,
            after,
            ws,
        } => {
            let c = match api(&ws) {
                Ok(c) => c,
                Err(code) => return code,
            };
            match rt().block_on(c.ws_messages(server, route, connection, after, 0)) {
                Ok(frames) => {
                    if ws.json {
                        print_json(&json!({ "frames": frames }));
                    } else {
                        for f in frames {
                            println!(
                                "{} {} {} {} {}B {}",
                                f.id,
                                f.ts,
                                f.connection,
                                if f.dir == "in" { "<-" } else { "->" },
                                f.size,
                                f.body
                            );
                        }
                    }
                    EXIT_OK
                }
                Err(e) => control_failed(e),
            }
        }
        WsAction::Send {
            server,
            body,
            route,
            connection,
            binary,
            ws,
        } => {
            let c = match api(&ws) {
                Ok(c) => c,
                Err(code) => return code,
            };
            match rt().block_on(c.ws_send(server, route, connection, body, binary)) {
                Ok(n) => {
                    if ws.json {
                        print_json(&json!({ "sent": n }));
                    } else {
                        println!("Sent to {n} connection(s).");
                    }
                    EXIT_OK
                }
                Err(e) => control_failed(e),
            }
        }
        WsAction::Disconnect {
            server,
            connection,
            code,
            ws,
        } => {
            let c = match api(&ws) {
                Ok(c) => c,
                Err(code) => return code,
            };
            match rt().block_on(c.ws_disconnect(server, connection.clone(), code)) {
                Ok(()) => {
                    if ws.json {
                        print_json(&json!({ "disconnected": connection }));
                    } else {
                        println!("Closed {connection}.");
                    }
                    EXIT_OK
                }
                Err(e) => control_failed(e),
            }
        }
    }
}
