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
    match cli.command {
        None => open(cli.path),
        Some(Command::Open { path }) => open(path),
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
