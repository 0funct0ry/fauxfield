//! The clap CLI skeleton (M1). Only `open`, `version` and `completions` exist; every later
//! command is added by its own milestone. Exit codes follow SPEC §10.

use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

pub const EXIT_OK: i32 = 0;
pub const EXIT_RUNTIME: i32 = 1;
pub const EXIT_USAGE: i32 = 2;

#[derive(Parser, Debug)]
#[command(
    name = "fauxfield",
    version,
    about = "Local-first mock API tool: desktop app, headless runner, CLI and MCP server.",
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    /// Workspace folder to open (default: the nearest ancestor containing fauxfield.toml).
    pub path: Option<PathBuf>,

    /// Host the MCP server inside the app on a unix socket (dev-mcp builds only). Without a PATH
    /// the default socket location is used.
    #[cfg(feature = "dev-mcp")]
    #[arg(long, value_name = "PATH", num_args = 0..=1)]
    pub mcp_socket: Option<Option<PathBuf>>,

    /// Host the MCP server inside the app on stdio; stdout is protocol-only (dev-mcp builds only).
    #[cfg(feature = "dev-mcp")]
    #[arg(long)]
    pub mcp_stdio: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Open the desktop app, optionally on a workspace folder.
    Open {
        /// Workspace folder to open.
        path: Option<PathBuf>,
    },
    /// Run the MCP server for agents (SPEC §11.1). Logs go to stderr; stdout is protocol-only.
    Mcp(McpArgs),
    /// Print version information.
    Version {
        /// Machine-readable output with a stable schema.
        #[arg(long)]
        json: bool,
    },
    /// Print a shell completion script to stdout.
    Completions {
        /// Target shell.
        shell: Shell,
    },
}

#[derive(clap::Args, Debug)]
pub struct McpArgs {
    /// Workspace folder (used from M2 on; validated now).
    pub path: Option<PathBuf>,
    /// Transport: stdio, unix, both, or http (http arrives in M16).
    #[arg(long, value_parser = parse_transport)]
    pub transport: Option<fauxfield_mcp::TransportKind>,
    /// Unix socket path (default: per-user runtime dir, mcp-<pid>.sock).
    #[arg(long, value_name = "PATH")]
    pub socket: Option<PathBuf>,
    /// Tool classes to expose: read, write, execute (default: all).
    #[arg(
        long,
        value_delimiter = ',',
        value_name = "CLASSES",
        conflicts_with = "read_only"
    )]
    pub allow: Option<Vec<String>>,
    /// Shorthand for --allow read.
    #[arg(long)]
    pub read_only: bool,
}

fn parse_transport(s: &str) -> Result<fauxfield_mcp::TransportKind, String> {
    s.parse()
}

/// Stable `version --json` schema.
pub fn version_json() -> serde_json::Value {
    serde_json::json!({
        "name": "fauxfield",
        "version": env!("CARGO_PKG_VERSION"),
        "gui": cfg!(feature = "gui"),
    })
}

pub fn run() -> i32 {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let code = if e.use_stderr() { EXIT_USAGE } else { EXIT_OK };
            e.print().ok();
            return code;
        }
    };
    #[cfg(feature = "dev-mcp")]
    if cli.command.is_none() && (cli.mcp_stdio || cli.mcp_socket.is_some()) {
        let unix = cli.mcp_socket.is_some();
        return open_dev(cli.path, cli.mcp_socket.flatten(), cli.mcp_stdio, unix);
    }
    match cli.command {
        None => open(cli.path),
        Some(Command::Open { path }) => open(path),
        Some(Command::Mcp(args)) => mcp(args),
        Some(Command::Version { json }) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&version_json()).unwrap());
            } else {
                println!("fauxfield {}", env!("CARGO_PKG_VERSION"));
            }
            EXIT_OK
        }
        Some(Command::Completions { shell }) => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "fauxfield", &mut std::io::stdout());
            EXIT_OK
        }
    }
}

fn mcp(args: McpArgs) -> i32 {
    use fauxfield_mcp::{AllowedClasses, McpSettings};
    if let Err(code) = check_path(&args.path) {
        return code;
    }
    let allow = if args.read_only {
        Ok(Some(AllowedClasses::read_only()))
    } else {
        args.allow
            .as_ref()
            .map(|l| AllowedClasses::parse_list(&l.join(",")).map(Some))
            .unwrap_or(Ok(None))
    };
    let config = allow
        .and_then(|allow| McpSettings::load_default()?.resolve(args.transport, args.socket, allow));
    let config = match config {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return EXIT_USAGE;
        }
    };
    // stdout carries only the protocol: all logs to stderr.
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: cannot start runtime: {e}");
            return EXIT_RUNTIME;
        }
    };
    match rt.block_on(fauxfield_mcp::serve(config)) {
        Ok(()) => EXIT_OK,
        Err(e) => {
            eprintln!("error: {e}");
            EXIT_RUNTIME
        }
    }
}

/// `fauxfield <word>` parses as a workspace PATH, so a mistyped subcommand lands here.
fn check_path(path: &Option<PathBuf>) -> Result<(), i32> {
    match path {
        Some(p) if !p.is_dir() => {
            eprintln!(
                "error: {} is not a folder. `fauxfield [PATH]` opens a workspace folder; \
                 see `fauxfield --help` for the available commands.",
                p.display()
            );
            Err(EXIT_RUNTIME)
        }
        _ => Ok(()),
    }
}

#[cfg(feature = "dev-mcp")]
fn open_dev(path: Option<PathBuf>, socket: Option<PathBuf>, stdio: bool, unix: bool) -> i32 {
    if let Err(code) = check_path(&path) {
        return code;
    }
    fauxfield_gui::run_dev(
        path.clone(),
        fauxfield_gui::dev_mcp::DevMcpOptions {
            socket,
            stdio,
            unix,
            workspace: path,
        },
    );
    EXIT_OK
}

#[cfg(feature = "gui")]
fn open(path: Option<PathBuf>) -> i32 {
    if let Err(code) = check_path(&path) {
        return code;
    }
    fauxfield_gui::run(path);
    EXIT_OK
}

#[cfg(not(feature = "gui"))]
fn open(path: Option<PathBuf>) -> i32 {
    if let Err(code) = check_path(&path) {
        return code;
    }
    eprintln!(
        "error: this build has no desktop app (built with --features headless). \
         Install the full build to open the GUI."
    );
    EXIT_RUNTIME
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_documented_forms() {
        assert!(
            Cli::try_parse_from(["fauxfield"])
                .unwrap()
                .command
                .is_none()
        );
        let c = Cli::try_parse_from(["fauxfield", "./ws"]).unwrap();
        assert_eq!(c.path.as_deref(), Some(std::path::Path::new("./ws")));
        assert!(matches!(
            Cli::try_parse_from(["fauxfield", "open", "x"])
                .unwrap()
                .command,
            Some(Command::Open { .. })
        ));
        // Unimplemented subcommands are not exposed: `run` is read as a PATH and rejected.
        let c = Cli::try_parse_from(["fauxfield", "run"]).unwrap();
        assert!(c.command.is_none());
        assert_eq!(check_path(&c.path), Err(EXIT_RUNTIME));
        assert_eq!(check_path(&Some(std::env::temp_dir())), Ok(()));
        assert_eq!(check_path(&None), Ok(()));
    }

    #[test]
    fn version_json_schema_is_stable() {
        let v = version_json();
        assert_eq!(v["name"], "fauxfield");
        assert!(v["version"].is_string() && v["gui"].is_boolean());
    }
}
