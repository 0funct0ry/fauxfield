//! `run`, `status`, `stop`, `logs` (M3): host a workspace headless, or attach to the live host
//! over the control socket (SPEC §3.1, §10). Banners go to stderr; request lines to stdout.

use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Args, ValueEnum};
use fauxfield_control::{
    ControlApi, ControlError, Host, HostError, HostOptions, RemoteControl, read_host,
};
use fauxfield_engine::{EngineError, EngineEvent, Recording, ServerStatus, TrafficFilter};
use fauxfield_model::Severity;
use serde_json::json;

use crate::cli::{
    EXIT_NO_HOST, EXIT_OK, EXIT_PORT_IN_USE, EXIT_RUNTIME, EXIT_USAGE, EXIT_VALIDATION,
};

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq)]
pub enum LogFormat {
    Pretty,
    Json,
}

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Workspace folder (default: the nearest ancestor containing fauxfield.toml).
    pub path: Option<PathBuf>,
    /// Only run these Servers (ID, name, or slug); repeatable.
    #[arg(long, value_name = "SERVER")]
    pub server: Vec<String>,
    /// Add this to every Server port.
    #[arg(long, value_name = "N", default_value_t = 0)]
    pub port_offset: u16,
    /// Bind every Server to this host address.
    #[arg(long, value_name = "ADDR")]
    pub host: Option<String>,
    /// Active Environment for this run (default: `default_environment`).
    #[arg(long, value_name = "NAME")]
    pub env: Option<String>,
    /// Network profile for this run: none, fast-3g, slow-3g, flaky-wifi or offline.
    #[arg(long, value_name = "PROFILE", value_parser = parse_profile)]
    pub network: Option<fauxfield_engine::NetworkProfile>,
    /// Chaos switch: inject random faults into this fraction (0 to 1) of requests on every Server.
    #[arg(long, value_name = "RATE", value_parser = parse_rate)]
    pub chaos: Option<f64>,
    /// Seed for fake and generated data on every Server, so a run repeats exactly.
    #[arg(long, value_name = "N")]
    pub seed: Option<u64>,
    /// Hot-reload external edits (default when attached to a terminal).
    #[arg(long, overrides_with = "no_watch")]
    pub watch: bool,
    /// Do not watch the workspace folder.
    #[arg(long = "no-watch")]
    pub no_watch: bool,
    /// Request log format on stdout.
    #[arg(long, value_enum, default_value = "pretty")]
    pub log_format: LogFormat,
}

#[derive(Args, Debug)]
pub struct StatusArgs {
    #[command(flatten)]
    pub ws: crate::workspace_cmd::WsArgs,
}

#[derive(Args, Debug)]
pub struct StopArgs {
    /// Only stop these Servers; without it every Server stops and a headless host exits.
    #[arg(long, value_name = "SERVER")]
    pub server: Vec<String>,
    #[command(flatten)]
    pub ws: crate::workspace_cmd::WsArgs,
}

#[derive(Args, Debug)]
pub struct LogsArgs {
    /// Keep printing new requests until interrupted.
    #[arg(long)]
    pub follow: bool,
    /// Only these Servers (ID or name); repeatable.
    #[arg(long, value_name = "SERVER")]
    pub server: Vec<String>,
    /// Status filter: `404` or a class like `5xx`.
    #[arg(long, value_name = "STATUS")]
    pub status: Option<String>,
    /// Only requests newer than this, e.g. `30s`, `10m`, `2h`.
    #[arg(long, value_name = "AGE")]
    pub since: Option<String>,
    /// Write the matching requests as a HAR 1.2 file instead of printing them.
    #[arg(long, value_name = "FILE", conflicts_with = "follow")]
    pub har: Option<PathBuf>,
    #[command(flatten)]
    pub ws: crate::workspace_cmd::WsArgs,
}

pub(crate) fn root_of(ws: &crate::workspace_cmd::WsArgs) -> PathBuf {
    match &ws.workspace {
        Some(p) => p.canonicalize().unwrap_or_else(|_| p.clone()),
        None => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let root = fauxfield_store::discover(&cwd).unwrap_or(cwd);
            root.canonicalize().unwrap_or(root)
        }
    }
}

pub(crate) fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

/// Log line for one recording. JSON: `{ts, server, method, path, status, route, duration_ms}`.
pub fn log_line(r: &Recording, format: LogFormat) -> String {
    match format {
        LogFormat::Json => json!({
            "ts": r.ts,
            "server": r.server,
            "method": r.method,
            "path": r.url,
            "status": r.status,
            "route": r.route,
            "duration_ms": r.timing.total_ms,
        })
        .to_string(),
        LogFormat::Pretty => format!(
            "{} {} {} -> {} ({}ms) [{}]",
            r.ts, r.method, r.url, r.status, r.timing.total_ms, r.route
        ),
    }
}

fn parse_profile(s: &str) -> Result<fauxfield_engine::NetworkProfile, String> {
    fauxfield_engine::NetworkProfile::parse(s).ok_or_else(|| {
        format!("`{s}` is not a profile; use none, fast-3g, slow-3g, flaky-wifi or offline")
    })
}

fn parse_rate(s: &str) -> Result<f64, String> {
    s.parse::<f64>()
        .ok()
        .filter(|r| (0.0..=1.0).contains(r))
        .ok_or_else(|| format!("`{s}` is not a rate between 0 and 1"))
}

pub fn run(a: RunArgs) -> i32 {
    if let Some(p) = &a.path
        && !p.is_dir()
    {
        crate::err!("{} is not a folder", p.display());
        return EXIT_RUNTIME;
    }
    let root = root_of(&crate::workspace_cmd::WsArgs {
        workspace: a.path.clone(),
        json: false,
    });
    let watch = !a.no_watch && (a.watch || std::io::IsTerminal::is_terminal(&std::io::stdin()));
    let host = match Host::start(
        &root,
        HostOptions {
            port_offset: a.port_offset,
            host_override: a.host.clone(),
            env: a.env.clone(),
            watch,
            publish: true,
            network: a.network,
            chaos: a.chaos,
            seed: a.seed,
        },
    ) {
        Ok(h) => h,
        Err(HostError::Already(info)) => {
            crate::err!(
                "this workspace is already hosted by pid {} (a running app or `fauxfield run`). \
                 Use `fauxfield status`, or `fauxfield stop` first.",
                info.pid
            );
            return EXIT_RUNTIME;
        }
        Err(HostError::Other(e)) => {
            crate::err!("{e}");
            return EXIT_RUNTIME;
        }
    };
    let snap = host.store().snapshot();
    let errors: Vec<_> = snap
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    if !errors.is_empty() {
        for d in &errors {
            crate::err!("{}", d.message);
        }
        eprintln!(
            "{} validation error(s); fix them (see `fauxfield validate`).",
            errors.len()
        );
        return EXIT_VALIDATION;
    }
    let engine = host.engine().clone();
    let known: Vec<_> = engine.list();
    for want in &a.server {
        if !known
            .iter()
            .any(|s| s.id == *want || s.name == *want || slug(&s.name) == *want)
        {
            crate::err!("no server `{want}`; see `fauxfield server list`");
            return EXIT_USAGE;
        }
    }
    // Print request lines as they arrive.
    let mut events = engine.subscribe();
    let format = a.log_format;
    host.handle().spawn(async move {
        while let Ok(ev) = events.recv().await {
            if let EngineEvent::Traffic(r) = ev {
                println!("{}", log_line(&r, format));
            }
        }
    });
    let mut code = EXIT_OK;
    if a.server.is_empty() {
        for (_, e) in engine.start_all() {
            crate::err!("{e}");
            code = if matches!(e, EngineError::PortInUse { .. }) {
                EXIT_PORT_IN_USE
            } else {
                EXIT_RUNTIME
            };
        }
    } else {
        for s in &a.server {
            if let Err(e) = engine.start(s) {
                crate::err!("{e}");
                code = if matches!(e, EngineError::PortInUse { .. }) {
                    EXIT_PORT_IN_USE
                } else {
                    EXIT_RUNTIME
                };
            }
        }
    }
    if code != EXIT_OK {
        engine.stop_all();
        return code;
    }
    for s in engine.list() {
        if let ServerStatus::Running { addr } = &s.status {
            let scheme = if s.https { "https" } else { "http" };
            eprintln!("serving {} on {scheme}://{addr}", s.name);
        }
    }
    eprintln!("press Ctrl+C to stop");
    let control = host.control();
    host.block_on(wait_for_stop(&control));
    eprintln!("stopping (draining for up to 5s)...");
    engine.stop_all();
    EXIT_OK
}

async fn wait_for_stop(control: &fauxfield_control::LocalControl) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
            _ = control.shutdown_requested() => {}
        }
    }
    #[cfg(not(unix))]
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = control.shutdown_requested() => {}
    }
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

pub(crate) fn attach(root: &Path) -> Result<(fauxfield_control::HostInfo, RemoteControl), i32> {
    match read_host(root) {
        Some(info) => {
            let c = RemoteControl::new(&info.socket);
            Ok((info, c))
        }
        None => {
            crate::err!(
                "no host is running for {}. Start one with `fauxfield run` or the app.",
                root.display()
            );
            Err(EXIT_NO_HOST)
        }
    }
}

pub(crate) fn control_failed(e: ControlError) -> i32 {
    crate::err!("{e}");
    if e.kind == "no_host" {
        EXIT_NO_HOST
    } else {
        EXIT_RUNTIME
    }
}

pub fn status(a: StatusArgs) -> i32 {
    let root = root_of(&a.ws);
    let (info, c) = match attach(&root) {
        Ok(x) => x,
        Err(code) => return code,
    };
    let servers = match rt().block_on(c.status()) {
        Ok(s) => s,
        Err(e) => return control_failed(e),
    };
    if a.ws.json {
        crate::output::print_json(&json!({
            "host": {
                "pid": info.pid,
                "version": info.version,
                "workspace": info.workspace,
                "started_at": info.started_at,
            },
            "servers": servers,
        }));
    } else {
        println!(
            "host pid {} (fauxfield {}), started {}",
            info.pid, info.version, info.started_at
        );
        for s in &servers {
            let state = match &s.status {
                ServerStatus::Running { addr } => {
                    format!(
                        "running on {}://{addr}",
                        if s.https { "https" } else { "http" }
                    )
                }
                ServerStatus::Stopped => "stopped".into(),
                ServerStatus::Failed { error, .. } => format!("failed: {error}"),
            };
            println!("{:<24} {state} ({} routes)", s.name, s.routes);
        }
    }
    EXIT_OK
}

pub fn stop(a: StopArgs) -> i32 {
    let root = root_of(&a.ws);
    let (_, c) = match attach(&root) {
        Ok(x) => x,
        Err(code) => return code,
    };
    let rt = rt();
    let mut servers = vec![];
    if a.server.is_empty() {
        match rt.block_on(c.stop(None)) {
            Ok(s) => servers = s,
            Err(e) => return control_failed(e),
        }
        // A headless host exits; the app ignores this and keeps running with servers stopped.
        let _ = rt.block_on(c.shutdown());
    } else {
        for s in &a.server {
            match rt.block_on(c.stop(Some(s.clone()))) {
                Ok(list) => servers = list,
                Err(e) => return control_failed(e),
            }
        }
    }
    if a.ws.json {
        crate::output::print_json(&json!({"servers": servers}));
    } else {
        println!("stopped");
    }
    EXIT_OK
}

fn parse_status(s: &str) -> Result<(u16, u16), String> {
    fauxfield_engine::parse_status_filter(s)
}

fn parse_age(s: &str) -> Result<Duration, String> {
    let (num, unit) = s.split_at(s.len().saturating_sub(1));
    let n: u64 = num
        .parse()
        .map_err(|_| format!("`{s}` is not an age like 30s, 10m or 2h"))?;
    match unit {
        "s" => Ok(Duration::from_secs(n)),
        "m" => Ok(Duration::from_secs(n * 60)),
        "h" => Ok(Duration::from_secs(n * 3600)),
        _ => Err(format!("`{s}` is not an age like 30s, 10m or 2h")),
    }
}

pub fn logs(a: LogsArgs) -> i32 {
    let status = match a.status.as_deref().map(parse_status).transpose() {
        Ok(s) => s,
        Err(e) => {
            crate::err!("{e}");
            return EXIT_USAGE;
        }
    };
    let since_ms = match a.since.as_deref().map(parse_age).transpose() {
        Ok(d) => d.map(|d| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|n| n.as_millis() as u64)
                .unwrap_or(0)
                .saturating_sub(d.as_millis() as u64)
        }),
        Err(e) => {
            crate::err!("{e}");
            return EXIT_USAGE;
        }
    };
    let root = root_of(&a.ws);
    let (_, c) = match attach(&root) {
        Ok(x) => x,
        Err(code) => return code,
    };
    let format = if a.ws.json {
        LogFormat::Json
    } else {
        LogFormat::Pretty
    };
    let rt = rt();
    let mut filter = TrafficFilter {
        // One Server filters in the engine (ID, name or slug); several filter below by ID or name.
        server: match a.server.as_slice() {
            [one] => Some(one.clone()),
            _ => None,
        },
        status,
        since_ms,
        ..Default::default()
    };
    let several = a.server.len() > 1;
    let wanted = |r: &fauxfield_engine::Recording| {
        !several || a.server.iter().any(|s| *s == r.server_id || *s == r.server)
    };
    if let Some(file) = &a.har {
        let list = match rt.block_on(c.traffic(filter.clone())) {
            Ok(l) => l.into_iter().filter(&wanted).collect::<Vec<_>>(),
            Err(e) => return control_failed(e),
        };
        let har = fauxfield_engine::har::to_har(&list);
        if let Err(e) = std::fs::write(
            file,
            serde_json::to_string_pretty(&har).unwrap_or_default() + "\n",
        ) {
            crate::err!("cannot write {}: {e}", file.display());
            return EXIT_RUNTIME;
        }
        if a.ws.json {
            crate::output::print_json(
                &serde_json::json!({ "file": file.to_string_lossy(), "entries": list.len() }),
            );
        } else {
            println!("Wrote {} entries to {}", list.len(), file.display());
        }
        return EXIT_OK;
    }
    loop {
        match rt.block_on(c.traffic(filter.clone())) {
            Ok(list) => {
                for r in &list {
                    filter.after_id = Some(r.id);
                    if wanted(r) {
                        println!("{}", log_line(r, format));
                    }
                }
            }
            Err(e) => return control_failed(e),
        }
        if !a.follow {
            return EXIT_OK;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_and_age() {
        assert_eq!(parse_status("5xx"), Ok((500, 599)));
        assert_eq!(parse_status("404"), Ok((404, 404)));
        assert!(parse_status("9xx").is_err());
        assert_eq!(parse_age("10m"), Ok(Duration::from_secs(600)));
        assert!(parse_age("10").is_err());
    }
}
