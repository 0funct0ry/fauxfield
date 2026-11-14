//! `fauxfield`: the desktop app and CLI in one binary (SPEC §3.1, §10).

mod callback_cmd;
mod certs_cmd;
mod cli;
mod data_cmd;
mod doctor_cmd;
mod env_cmd;
mod host_cmd;
mod scenario_cmd;
mod workspace_cmd;
mod ws_cmd;

fn main() {
    std::process::exit(cli::run());
}
