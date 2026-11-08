//! Codegen and release helpers. `schema` (M2); docs generators arrive in M18.

use std::path::Path;

const SERVER_SCHEMA: &str = "site/public/schema/v1/server.json";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("schema") => {
            let check = args.iter().any(|a| a == "--check");
            let text =
                serde_json::to_string_pretty(&fauxfield_model::server_schema()).unwrap() + "\n";
            let path = Path::new(SERVER_SCHEMA);
            if check {
                if std::fs::read_to_string(path).ok().as_deref() != Some(text.as_str()) {
                    eprintln!("{SERVER_SCHEMA} is out of date; run `cargo xtask schema`");
                    std::process::exit(1);
                }
                return;
            }
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
            println!("wrote {SERVER_SCHEMA}");
        }
        _ => {
            eprintln!("usage: cargo xtask schema [--check]   (docs reference generators: M18)");
            std::process::exit(2);
        }
    }
}
