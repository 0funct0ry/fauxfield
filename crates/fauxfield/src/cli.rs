//! The clap CLI skeleton (M1). Only `open`, `version` and `completions` exist; every later
//! command is added by its own milestone. Exit codes follow SPEC §10.

use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

pub const EXIT_OK: i32 = 0;
pub const EXIT_RUNTIME: i32 = 1;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_VALIDATION: i32 = 3;
pub const EXIT_NO_HOST: i32 = 4;
pub const EXIT_PORT_IN_USE: i32 = 5;

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
    /// Create a workspace (SPEC §4.1) with one example server.
    Init {
        /// Folder to create the workspace in (default: the current directory).
        path: Option<PathBuf>,
        /// Machine-readable output with a stable schema.
        #[arg(long)]
        json: bool,
    },
    /// Validate the workspace files; exit code 3 when there are errors.
    Validate(crate::workspace_cmd::WsArgs),
    /// Show or rename the workspace.
    Workspace {
        #[command(subcommand)]
        action: crate::workspace_cmd::WorkspaceAction,
    },
    /// List and inspect Servers.
    Server {
        #[command(subcommand)]
        action: crate::workspace_cmd::ServerAction,
    },
    /// List and inspect Routes.
    Route {
        #[command(subcommand)]
        action: crate::workspace_cmd::RouteAction,
    },
    /// Add and configure Responses of a Route.
    Response {
        #[command(subcommand)]
        action: crate::workspace_cmd::ResponseAction,
    },
    /// Serve a workspace headless: become the host and run its servers until stopped.
    Run(crate::host_cmd::RunArgs),
    /// Show the running host and its servers (exit code 4 when no host is running).
    Status(crate::host_cmd::StatusArgs),
    /// Stop servers on the running host; without --server also ends a headless host.
    Stop(crate::host_cmd::StopArgs),
    /// Print request logs from the running host.
    Logs(crate::host_cmd::LogsArgs),
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
#[command(args_conflicts_with_subcommands = true)]
pub struct McpArgs {
    #[command(subcommand)]
    pub action: Option<McpAction>,
    /// Workspace folder to serve tools over.
    pub path: Option<PathBuf>,
    /// Transport: stdio, unix, pipe (Windows), both, or http (http arrives in M16).
    #[arg(long, value_parser = parse_transport)]
    pub transport: Option<fauxfield_mcp::TransportKind>,
    /// Unix socket path, or a pipe name on Windows (default: per-user runtime dir, mcp-<pid>.sock; \\\\.\\pipe\\fauxfield-mcp-<pid>).
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

#[derive(clap::Subcommand, Debug)]
pub enum McpAction {
    /// Run a scenario file against a running dev-mcp app (exit 0 pass, 1 failed step, 2 unreachable).
    Scenario(crate::scenario_cmd::ScenarioArgs),
}

fn parse_transport(s: &str) -> Result<fauxfield_mcp::TransportKind, String> {
    s.parse()
}

/// Stable `version --json` schema.
pub fn version_json() -> serde_json::Value {
    let b = build_info();
    serde_json::json!({
        "name": "fauxfield",
        "version": b.version,
        "gui": cfg!(feature = "gui"),
        "commit": b.commit_label(),
        "commit_date": b.commit_date,
        "dirty": b.dirty,
        "target": b.target,
        "profile": b.profile,
        "gpui": b.gpui,
    })
}

pub fn build_info() -> fauxfield_control::build_info::BuildInfo {
    let features = if cfg!(feature = "dev-mcp") {
        vec!["dev-mcp"]
    } else {
        vec![]
    };
    fauxfield_control::build_info::BuildInfo::current(features)
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
        Some(Command::Init { path, json }) => crate::workspace_cmd::init(path, json),
        Some(Command::Validate(a)) => crate::workspace_cmd::validate(&a),
        Some(Command::Workspace { action }) => crate::workspace_cmd::workspace(action),
        Some(Command::Server { action }) => crate::workspace_cmd::server(action),
        Some(Command::Route { action }) => crate::workspace_cmd::route(action),
        Some(Command::Response { action }) => crate::workspace_cmd::response(action),
        Some(Command::Run(a)) => crate::host_cmd::run(a),
        Some(Command::Status(a)) => crate::host_cmd::status(a),
        Some(Command::Stop(a)) => crate::host_cmd::stop(a),
        Some(Command::Logs(a)) => crate::host_cmd::logs(a),
        Some(Command::Mcp(args)) => mcp(args),
        Some(Command::Version { json }) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&version_json()).unwrap());
            } else {
                let b = build_info();
                println!("fauxfield {}", b.version);
                // Then the commit, target and gpui lines (`copy_text()`, SPEC §20.14).
                for line in b.copy_text().lines() {
                    println!("{line}");
                }
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
    if let Some(McpAction::Scenario(a)) = args.action {
        return crate::scenario_cmd::run(a);
    }
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
        .and_then(|allow| McpSettings::load_default()?.resolve(args.transport, args.socket, allow))
        .and_then(|c| c.transport.check_platform().map(|()| c));
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
    // A host started by a tool call lives here so it is dropped outside the runtime.
    let hosted: HostSlot = Default::default();
    let registry = workspace_registry(&config, &args.path, &hosted);
    let code = match rt.block_on(fauxfield_mcp::serve_with(config, registry)) {
        Ok(()) => EXIT_OK,
        Err(e) => {
            eprintln!("error: {e}");
            EXIT_RUNTIME
        }
    };
    drop(rt);
    hosted.lock().unwrap().take();
    code
}

type HostSlot = std::sync::Arc<std::sync::Mutex<Option<fauxfield_control::Host>>>;

/// Attach to a live host for this workspace, else become the host on first use (SPEC §3.1, §11.1).
fn control_provider(
    root: PathBuf,
    slot: &HostSlot,
) -> fauxfield_mcp::control_tools::ControlProvider {
    use fauxfield_control::{ControlApi, Host, HostOptions, RemoteControl, read_host};
    let slot = slot.clone();
    std::sync::Arc::new(move |mode| {
        use fauxfield_mcp::control_tools::ControlMode;
        use std::sync::Arc;
        let mut g = slot.lock().unwrap();
        if let Some(h) = g.as_ref() {
            return Ok(Some(h.control() as Arc<dyn ControlApi>));
        }
        if let Some(info) = read_host(&root) {
            return Ok(Some(Arc::new(RemoteControl::new(&info.socket))));
        }
        if mode == ControlMode::AttachOnly {
            // Read-only tools never create a host.
            return Ok(None);
        }
        match Host::start(
            &root,
            HostOptions {
                watch: true,
                publish: true,
                ..Default::default()
            },
        ) {
            Ok(host) => {
                let control = host.control() as Arc<dyn ControlApi>;
                *g = Some(host);
                Ok(Some(control))
            }
            // Lost the race to another process: attach to the winner.
            Err(fauxfield_control::HostError::Already(info)) => {
                Ok(Some(Arc::new(RemoteControl::new(&info.socket))))
            }
            Err(e) => Err(e.to_string()),
        }
    })
}

/// Core tools plus the workspace tools when PATH (or the cwd's nearest ancestor) is a workspace.
fn workspace_registry(
    config: &fauxfield_mcp::McpConfig,
    path: &Option<PathBuf>,
    hosted: &HostSlot,
) -> Option<fauxfield_mcp::Registry> {
    let root = match path {
        Some(p) => Some(p.clone()),
        None => std::env::current_dir()
            .ok()
            .and_then(|c| fauxfield_store::discover(&c)),
    }?;
    match fauxfield_store::Store::open(&root) {
        Ok(store) => {
            let provider = control_provider(store.root(), hosted);
            let mut r = fauxfield_mcp::build_registry(config);
            fauxfield_mcp::control_tools::register_control_tools(&mut r, provider);
            fauxfield_mcp::workspace_tools::register_workspace_tools(
                &mut r,
                fauxfield_mcp::workspace_tools::fixed_store(store),
            );
            Some(r)
        }
        Err(e) => {
            eprintln!("warning: workspace tools disabled: {e}");
            None
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
        // Unimplemented subcommands are not exposed: `doctor` is read as a PATH and rejected.
        let c = Cli::try_parse_from(["fauxfield", "doctor"]).unwrap();
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
