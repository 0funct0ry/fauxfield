//! `certs status|trust|untrust` (M10, SPEC §5.8). Trust commands are printed; they run only
//! after an explicit `--yes` or an interactive `y`. Nothing edits a trust store silently.

use std::io::{BufRead, IsTerminal, Write};

use clap::Subcommand;
use fauxfield_engine::tls;
use serde_json::json;

use crate::cli::{EXIT_OK, EXIT_RUNTIME};
use crate::workspace_cmd::print_json;

#[derive(Subcommand, Debug)]
pub enum CertsAction {
    /// Show the development CA: path, fingerprint, expiry and whether this machine trusts it.
    Status {
        /// Machine-readable output with a stable schema.
        #[arg(long)]
        json: bool,
    },
    /// Print the command that trusts the dev CA (creating the CA if needed); run it with --yes.
    Trust(TrustArgs),
    /// Print the command that removes trust in the dev CA; run it with --yes.
    Untrust(TrustArgs),
}

#[derive(clap::Args, Debug)]
pub struct TrustArgs {
    /// Run the command without asking. Without it a terminal session is asked to confirm.
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// Machine-readable output with a stable schema (never runs the command).
    #[arg(long)]
    pub json: bool,
}

fn data_dir() -> Result<std::path::PathBuf, i32> {
    tls::default_data_dir().ok_or_else(|| {
        eprintln!("error: no platform data directory");
        EXIT_RUNTIME
    })
}

pub fn certs(action: CertsAction) -> i32 {
    let dd = match data_dir() {
        Ok(d) => d,
        Err(c) => return c,
    };
    match action {
        CertsAction::Status { json } => {
            let st = tls::ca_status(&dd);
            if json {
                print_json(&serde_json::to_value(&st).unwrap());
            } else if !st.exists {
                println!("Dev CA: not created yet ({}).", st.path);
                println!(
                    "It is created the first time an HTTPS Server starts, or by `fauxfield certs trust`."
                );
            } else {
                println!("Dev CA:      {}", st.path);
                println!("Fingerprint: {}", st.fingerprint.as_deref().unwrap_or("-"));
                println!("Expires in:  {} days", st.days_left.unwrap_or(0));
                println!("Trusted:     {:?}", st.trust);
            }
            EXIT_OK
        }
        CertsAction::Trust(a) => trust(&dd, a, true),
        CertsAction::Untrust(a) => trust(&dd, a, false),
    }
}

fn trust(dd: &std::path::Path, a: TrustArgs, add: bool) -> i32 {
    if add && let Err(e) = tls::load_or_create_ca(dd) {
        eprintln!("error: {e}");
        return EXIT_RUNTIME;
    }
    let st = tls::ca_status(dd);
    let cmd = if add {
        &st.trust_command
    } else {
        &st.untrust_command
    };
    if a.json {
        print_json(&json!({ "command": cmd, "trust": st.trust, "ran": false }));
        return EXIT_OK;
    }
    println!("{cmd}");
    let run = a.yes || {
        if std::io::stdin().is_terminal() {
            eprint!("Run this command now? [y/N] ");
            let _ = std::io::stderr().flush();
            let mut l = String::new();
            let _ = std::io::stdin().lock().read_line(&mut l);
            matches!(l.trim(), "y" | "Y" | "yes")
        } else {
            false
        }
    };
    if !run {
        eprintln!("Not run. Copy the command above, or pass --yes.");
        return EXIT_OK;
    }
    match tls::run_command(cmd) {
        Ok(()) => {
            eprintln!("Done. Restart the browser if it was open.");
            EXIT_OK
        }
        Err(e) => {
            eprintln!("error: {e}");
            EXIT_RUNTIME
        }
    }
}
