//! `fauxfield`: the desktop app and CLI in one binary (SPEC §3.1, §10).

mod agents_cmd;
mod callback_cmd;
mod certs_cmd;
mod cli;
mod data_cmd;
mod docs_cmd;
mod doctor_cmd;
mod env_cmd;
mod host_cmd;
mod output;
mod scenario_cmd;
mod theme_cmd;
mod workspace_cmd;
mod ws_cmd;

fn main() {
    fauxfield_control::trace::init();
    std::process::exit(cli::run());
}
