//! `fauxfield agents init` (SPEC §11.3, M16). The planning and writing live in
//! `fauxfield_store::agents`, shared with the GUI dialog.

use std::{
    io::{BufRead, IsTerminal, Write},
    path::{Path, PathBuf},
};

use clap::Subcommand;
use fauxfield_store::agents::{self, Client, Options};
use serde_json::json;

use crate::{
    cli::{EXIT_OK, EXIT_RUNTIME, EXIT_USAGE},
    workspace_cmd::{WsArgs, print_json},
};

#[derive(Subcommand, Debug)]
pub enum AgentsAction {
    /// Write MCP client config and an AGENTS.md section for this Workspace.
    Init(InitArgs),
}

#[derive(clap::Args, Debug)]
pub struct InitArgs {
    /// Configure Claude Code (.mcp.json).
    #[arg(long)]
    pub claude: bool,
    /// Configure Codex (.codex/config.toml).
    #[arg(long)]
    pub codex: bool,
    /// Configure Cursor (.cursor/mcp.json).
    #[arg(long)]
    pub cursor: bool,
    /// Write without asking.
    #[arg(short = 'y', long)]
    pub yes: bool,
    /// Print the diff and exit; nothing is written.
    #[arg(long)]
    pub dry_run: bool,
    /// Tool classes the agent may use: read, write, execute.
    #[arg(long, value_delimiter = ',', default_values_t = ["read".to_string(), "write".to_string(), "execute".to_string()])]
    pub allow: Vec<String>,
    /// Command written into the client config (default: `fauxfield`; use an absolute path when the binary is not on PATH).
    #[arg(long, value_name = "PATH")]
    pub command: Option<String>,
    #[command(flatten)]
    pub ws: WsArgs,
}

pub fn agents(action: AgentsAction) -> i32 {
    let AgentsAction::Init(a) = action;
    init(a)
}

fn on_path(bin: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|d| {
        let p = d.join(bin);
        p.is_file() || p.with_extension("exe").is_file()
    })
}

fn parse_clients(s: &str) -> Result<Vec<Client>, String> {
    let mut v = Vec::new();
    for part in s.split([',', ' ']).filter(|p| !p.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "claude" => v.push(Client::Claude),
            "codex" => v.push(Client::Codex),
            "cursor" => v.push(Client::Cursor),
            other => return Err(format!("unknown client `{other}` (claude, codex, cursor)")),
        }
    }
    Ok(v)
}

fn root_of(ws: &WsArgs) -> PathBuf {
    match &ws.workspace {
        Some(p) => p.clone(),
        None => {
            let cwd = std::env::current_dir().unwrap_or_default();
            fauxfield_store::discover(&cwd).unwrap_or(cwd)
        }
    }
}

fn confirm(prompt: &str) -> bool {
    eprint!("{prompt} ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).is_ok()
        && matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

pub fn init(a: InitArgs) -> i32 {
    let root = root_of(&a.ws);
    let mut clients: Vec<Client> = Vec::new();
    for (on, c) in [
        (a.claude, Client::Claude),
        (a.codex, Client::Codex),
        (a.cursor, Client::Cursor),
    ] {
        if on {
            clients.push(c);
        }
    }
    let tty = std::io::stdin().is_terminal();
    if clients.is_empty() {
        if !tty {
            crate::err!(
                "no client chosen: pass --claude, --codex and/or --cursor (no TTY for a prompt)"
            );
            return EXIT_USAGE;
        }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let mut detected = agents::detect_clients(&root, home.as_deref());
        if detected.is_empty() {
            detected.push(Client::Claude);
        }
        let default = detected
            .iter()
            .map(|c| c.id())
            .collect::<Vec<_>>()
            .join(",");
        eprint!("Clients to configure (claude, codex, cursor) [{default}]: ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
        clients = if line.trim().is_empty() {
            detected
        } else {
            match parse_clients(&line) {
                Ok(c) => c,
                Err(m) => {
                    crate::err!("{m}");
                    return EXIT_USAGE;
                }
            }
        };
    }
    for c in &a.allow {
        if !["read", "write", "execute"].contains(&c.as_str()) {
            crate::err!("unknown tool class `{c}` (read, write, execute)");
            return EXIT_USAGE;
        }
    }
    run_plan(
        &root, clients, a.allow, a.command, a.yes, a.dry_run, a.ws.json, tty,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_plan(
    root: &Path,
    clients: Vec<Client>,
    allow: Vec<String>,
    command: Option<String>,
    yes: bool,
    dry_run: bool,
    json: bool,
    tty: bool,
) -> i32 {
    let command = command.unwrap_or_else(|| {
        if !on_path("fauxfield") {
            eprintln!(
                "warning: `fauxfield` is not on PATH; clients may not find it. Re-run with --command <absolute path to the binary>."
            );
        }
        "fauxfield".into()
    });
    let opts = Options {
        clients,
        allow,
        command,
    };
    let plan = match agents::plan(root, &opts) {
        Ok(p) => p,
        Err(e) => {
            crate::err!("{e}");
            return EXIT_RUNTIME;
        }
    };
    let n = plan.changes().count();
    if !json {
        for f in plan.changes() {
            println!("{}", f.diff.trim_end());
            println!();
        }
        if n == 0 {
            println!("Already configured");
        }
    }
    let emit = |written: bool| {
        if json {
            print_json(&json!({
                "files": plan.files.iter().map(|f| json!({
                    "path": f.path, "action": f.action, "diff": f.diff,
                })).collect::<Vec<_>>(),
                "written": written,
            }));
        }
    };
    if n == 0 || dry_run {
        emit(false);
        return EXIT_OK;
    }
    if !yes {
        if json && !tty {
            crate::err!("pass --yes to write, or --dry-run to preview");
            return EXIT_USAGE;
        }
        if !confirm(&format!(
            "Write {n} file{}? [y/N]",
            if n == 1 { "" } else { "s" }
        )) {
            emit(false);
            eprintln!("Nothing written.");
            return EXIT_OK;
        }
    }
    match agents::apply(root, &plan) {
        Ok(written) => {
            emit(true);
            if !json {
                for w in &written {
                    println!("Wrote {w}");
                }
                println!("Restart your agent, then run /mcp to see Fauxfield.");
            }
            EXIT_OK
        }
        Err(e) => {
            crate::err!("{e}");
            EXIT_RUNTIME
        }
    }
}

/// `fauxfield init --agents`: the same plan, written without asking (the flag is the consent).
/// Returns the files written.
pub fn apply_defaults(root: &Path) -> Result<Vec<String>, String> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut clients = agents::detect_clients(root, home.as_deref());
    if clients.is_empty() {
        clients.push(Client::Claude);
    }
    let opts = Options {
        clients,
        ..Options::default()
    };
    let plan = agents::plan(root, &opts).map_err(|e| e.to_string())?;
    agents::apply(root, &plan).map_err(|e| e.to_string())
}
