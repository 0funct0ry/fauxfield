//! `fauxfield`: the desktop app and CLI in one binary (SPEC §3.1, §10).

mod cli;
mod scenario_cmd;
mod workspace_cmd;

fn main() {
    std::process::exit(cli::run());
}
