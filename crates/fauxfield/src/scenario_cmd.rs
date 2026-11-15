//! `fauxfield mcp scenario FILE --socket PATH`: runs a scenario file against a running dev-mcp
//! app over its MCP socket (SPEC §11.4). Exit 0 pass, 1 failed step, 2 app unreachable.

use std::path::PathBuf;

use clap::Args;

#[cfg_attr(not(unix), allow(dead_code))]
pub const EXIT_PASS: i32 = 0;
#[cfg_attr(not(unix), allow(dead_code))]
pub const EXIT_FAILED: i32 = 1;
pub const EXIT_UNREACHABLE: i32 = 2;

#[derive(Args, Debug)]
pub struct ScenarioArgs {
    /// Scenario YAML file (fixtures/scenarios/<name>.yaml).
    pub file: PathBuf,
    /// MCP unix socket of a running `fauxfield --mcp-socket` app.
    #[arg(long, value_name = "PATH")]
    pub socket: PathBuf,
    /// Write checkpoint and failure screenshots (PNG) into this folder.
    #[arg(long, value_name = "DIR")]
    pub out: Option<PathBuf>,
    /// Skip screenshots (no Screen Recording permission, no display tools).
    #[arg(long)]
    pub no_capture: bool,
    /// Print the report as JSON.
    #[arg(long)]
    pub json: bool,
}

#[cfg(not(unix))]
pub fn run(_args: ScenarioArgs) -> i32 {
    eprintln!(
        "error: `fauxfield mcp scenario` needs a unix socket; not available on this platform"
    );
    EXIT_UNREACHABLE
}

#[cfg(unix)]
pub fn run(args: ScenarioArgs) -> i32 {
    let text = match std::fs::read_to_string(&args.file) {
        Ok(t) => t,
        Err(e) => {
            crate::err!("cannot read {}: {e}", args.file.display());
            return EXIT_UNREACHABLE;
        }
    };
    let scenario = match fauxfield_mcp::scenario::Scenario::from_yaml(&text) {
        Ok(s) => s,
        Err(e) => {
            crate::err!("{}: {e}", args.file.display());
            return EXIT_UNREACHABLE;
        }
    };
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            crate::err!("cannot start runtime: {e}");
            return EXIT_UNREACHABLE;
        }
    };
    rt.block_on(async move {
        let caller = match client::connect(&args.socket).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "error: cannot reach the app at {}: {e}\nstart it with `make dev-mcp` or \
                     `fauxfield --mcp-socket {}`",
                    args.socket.display(),
                    args.socket.display()
                );
                return EXIT_UNREACHABLE;
            }
        };
        let opts = fauxfield_mcp::scenario::RunOptions {
            capture: !args.no_capture,
        };
        let report = fauxfield_mcp::scenario::run(&scenario, &caller, opts).await;
        let mut written = Vec::new();
        if let Some(dir) = &args.out {
            use base64::Engine as _;
            let _ = std::fs::create_dir_all(dir);
            for img in &report.images {
                let Ok(png) = base64::engine::general_purpose::STANDARD.decode(&img.data) else {
                    continue;
                };
                let path = dir.join(format!("{}-{}.png", report.name, img.label));
                if std::fs::write(&path, png).is_ok() {
                    written.push(path);
                }
            }
        }
        if args.json {
            let mut v = serde_json::to_value(&report).unwrap_or_default();
            v["screenshots"] = serde_json::json!(
                written
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
            );
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        } else {
            print_report(&report, &written);
        }
        if report.passed {
            EXIT_PASS
        } else {
            EXIT_FAILED
        }
    })
}

#[cfg(unix)]
fn print_report(r: &fauxfield_mcp::scenario::Report, written: &[PathBuf]) {
    for s in &r.steps {
        println!(
            "  {} {:>2}. {} ({} ms)",
            if s.ok { "ok  " } else { "FAIL" },
            s.index,
            s.step,
            s.ms
        );
    }
    match &r.failure {
        None => println!(
            "PASS {} ({} steps, {} ms)",
            r.name,
            r.steps.len(),
            r.elapsed_ms
        ),
        Some(f) => {
            println!("FAIL {}: step {} ({})", r.name, f.index, f.step);
            println!("  expected: {}", f.expected);
            println!("  actual:   {}", f.actual);
            if let Some(state) = &f.state {
                println!("  generation: {}", state["generation"]);
            }
        }
    }
    for p in written {
        println!("  screenshot: {}", p.display());
    }
}

#[cfg(unix)]
mod client {
    use std::path::Path;

    use fauxfield_mcp::scenario::{CallFuture, ToolCaller};
    use rmcp::{RoleClient, ServiceExt, model::CallToolRequestParams, service::RunningService};
    use serde_json::{Map, Value, json};

    pub struct Client {
        service: RunningService<RoleClient, ()>,
    }

    pub async fn connect(path: &Path) -> Result<Client, String> {
        let stream = tokio::net::UnixStream::connect(path)
            .await
            .map_err(|e| e.to_string())?;
        let service = ().serve(stream).await.map_err(|e| e.to_string())?;
        Ok(Client { service })
    }

    impl ToolCaller for Client {
        fn call<'a>(&'a self, name: &'a str, args: Map<String, Value>) -> CallFuture<'a> {
            Box::pin(async move {
                let params = CallToolRequestParams::new(name.to_owned()).with_arguments(args);
                let r = self
                    .service
                    .call_tool(params)
                    .await
                    .map_err(|e| e.to_string())?;
                if r.is_error == Some(true) {
                    let msg = r
                        .content
                        .iter()
                        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
                        .collect::<Vec<_>>()
                        .join("\n");
                    return Err(msg);
                }
                let mut v = r.structured_content.unwrap_or_else(|| json!({}));
                let images: Vec<Value> = r
                    .content
                    .iter()
                    .filter_map(|c| c.as_image())
                    .map(|i| json!({ "data": i.data, "mime": i.mime_type }))
                    .collect();
                if !images.is_empty()
                    && let Some(o) = v.as_object_mut()
                {
                    o.insert("_images".into(), Value::Array(images));
                }
                Ok(v)
            })
        }
    }
}
