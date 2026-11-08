//! `fauxfield`: the desktop app and CLI in one binary (SPEC §3.1, §10).

mod cli;

fn main() {
    std::process::exit(cli::run());
}
