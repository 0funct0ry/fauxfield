//! `doctor` (M10, SPEC §10): environment checks: versions, ports, CA trust, certificate expiry.

use fauxfield_engine::tls;
use serde_json::json;

use crate::cli::{EXIT_OK, EXIT_RUNTIME};
use crate::workspace_cmd::{WsArgs, open_store, print_json};

pub fn doctor(a: WsArgs) -> i32 {
    let b = crate::cli::build_info();
    let dd = tls::default_data_dir();
    let ca = dd.as_deref().map(tls::ca_status);
    let mut ports = Vec::new();
    let mut certs = Vec::new();
    let mut workspace = serde_json::Value::Null;
    let mut problems = 0;
    if let Ok(store) = open_store(&a) {
        let root = store.root();
        workspace = json!(root.display().to_string());
        for (_, s) in store.snapshot().servers.iter().filter(|(_, s)| s.enabled) {
            let free = std::net::TcpListener::bind((s.host.as_str(), s.port)).is_ok();
            ports.push(json!({ "server": s.name, "port": s.port, "available": free }));
            if let Some(t) = s.tls.as_ref().filter(|t| t.enabled) {
                if t.mode_name() == "files" {
                    match tls::build(s, &root, dd.as_deref(), &s.host) {
                        Ok(r) => {
                            let st = r.status;
                            if st.as_ref().is_some_and(|c| c.warning.is_some()) {
                                problems += 1;
                            }
                            certs.push(json!({ "server": s.name, "mode": "files", "cert": st, "error": null }));
                        }
                        Err(e) => {
                            problems += 1;
                            certs.push(json!({ "server": s.name, "mode": "files", "cert": null, "error": e }));
                        }
                    }
                } else {
                    certs.push(
                        json!({ "server": s.name, "mode": "dev-ca", "cert": null, "error": null }),
                    );
                }
            }
        }
    }
    let ca_json = ca.as_ref().map(|c| serde_json::to_value(c).unwrap());
    if let Some(c) = &ca
        && c.exists
        && c.days_left.is_some_and(|d| d <= tls::WARN_DAYS)
    {
        problems += 1;
    }
    if a.json {
        print_json(&json!({
            "version": b.version,
            "gui": cfg!(feature = "gui"),
            "workspace": workspace,
            "ports": ports,
            "ca": ca_json,
            "certs": certs,
            "ok": problems == 0,
        }));
    } else {
        println!("Fauxfield {}  (gui: {})", b.version, cfg!(feature = "gui"));
        for p in &ports {
            println!(
                "port {}  {}  {}",
                p["port"],
                p["server"].as_str().unwrap_or(""),
                if p["available"] == true {
                    "available"
                } else {
                    "in use"
                }
            );
        }
        match &ca {
            Some(c) if c.exists => println!(
                "dev CA: {:?}, expires in {} days ({})",
                c.trust,
                c.days_left.unwrap_or(0),
                c.path
            ),
            _ => println!("dev CA: not created yet (`fauxfield certs trust` creates it)"),
        }
        for c in &certs {
            let d = &c["cert"];
            match (d["days_left"].as_i64(), c["error"].as_str()) {
                (_, Some(e)) => println!("cert {}: error: {e}", c["server"].as_str().unwrap_or("")),
                (Some(n), _) => println!(
                    "cert {}: expires in {n} days",
                    c["server"].as_str().unwrap_or("")
                ),
                _ => println!("cert {}: dev CA leaf", c["server"].as_str().unwrap_or("")),
            }
        }
    }
    if problems == 0 { EXIT_OK } else { EXIT_RUNTIME }
}
