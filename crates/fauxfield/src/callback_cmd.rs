//! `callback refire` (M14): fire the Callbacks of a recorded request again on a running host.
//! Every command has a stable `--json`.

use clap::Subcommand;
use fauxfield_control::ControlApi;
use serde_json::json;

use crate::cli::EXIT_OK;
use crate::host_cmd::{attach, control_failed, root_of, rt};
use crate::workspace_cmd::{WsArgs, print_json};

#[derive(Subcommand, Debug)]
pub enum CallbackAction {
    /// Fire the Callbacks of a recorded request again, now (the delay is skipped).
    Refire {
        /// Server ID, name or slug.
        server: String,
        /// Recording id of the request (or of one of its callback entries); see `logs --json`.
        recording: u64,
        /// Only this Callback id; omit to fire all Callbacks of that Response.
        #[arg(long)]
        callback: Option<String>,
        #[command(flatten)]
        ws: WsArgs,
    },
}

pub fn callback(action: CallbackAction) -> i32 {
    match action {
        CallbackAction::Refire {
            server,
            recording,
            callback,
            ws,
        } => {
            let c = match attach(&root_of(&ws)) {
                Ok((_, c)) => c,
                Err(code) => return code,
            };
            match rt().block_on(c.callback_refire(server, recording, callback)) {
                Ok(list) => {
                    if ws.json {
                        let items: Vec<_> = list
                            .iter()
                            .map(|r| {
                                json!({
                                    "id": r.id,
                                    "parent_id": r.parent_id,
                                    "callback_id": r.callback_id,
                                    "method": r.method,
                                    "url": r.url,
                                    "status": r.status,
                                    "error": r.error,
                                })
                            })
                            .collect();
                        print_json(&json!({ "callbacks": items }));
                    } else {
                        for r in &list {
                            println!(
                                "{} {} {} -> {}{}",
                                r.callback_id.as_deref().unwrap_or("-"),
                                r.method,
                                r.url,
                                r.status,
                                r.error
                                    .as_deref()
                                    .map(|e| format!(" ({e})"))
                                    .unwrap_or_default()
                            );
                        }
                    }
                    EXIT_OK
                }
                Err(e) => control_failed(e),
            }
        }
    }
}
