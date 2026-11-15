//! Codegen and release helpers. `schema` (M2); docs generators arrive in M18.

use std::path::Path;

const TEMPLATE_DOC: &str = "site/src/content/docs/reference/template-functions.md";
const SERVER_SCHEMA: &str = "site/public/schema/v1/server.json";

/// One generated file: the xtask command, its path and its current text.
struct Gen {
    cmd: &'static str,
    path: &'static str,
    text: fn() -> String,
}

const GENERATED: &[Gen] = &[
    Gen {
        cmd: "schema",
        path: SERVER_SCHEMA,
        text: || serde_json::to_string_pretty(&fauxfield_model::server_schema()).unwrap() + "\n",
    },
    Gen {
        cmd: "template-docs",
        path: TEMPLATE_DOC,
        text: template_docs,
    },
    Gen {
        cmd: "generator-docs",
        path: GENERATOR_DOC,
        text: generator_docs,
    },
    Gen {
        cmd: "mcp-tools-docs",
        path: MCP_DOC,
        text: mcp_tools_docs,
    },
    Gen {
        cmd: "settings-schema",
        path: SETTINGS_SCHEMA,
        text: settings_schema,
    },
    Gen {
        cmd: "settings-docs",
        path: SETTINGS_DOC,
        text: settings_docs,
    },
    Gen {
        cmd: "keybindings-docs",
        path: KEYBINDINGS_DOC,
        text: keybindings_docs,
    },
    Gen {
        cmd: "cli-docs",
        path: CLI_DOC,
        text: cli_docs,
    },
];

fn write(g: &Gen) {
    let path = Path::new(g.path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, (g.text)()).unwrap();
    println!("wrote {}", g.path);
}

/// Whether the file on disk matches what the generator produces now.
fn fresh(g: &Gen) -> bool {
    std::fs::read_to_string(g.path).ok().as_deref() == Some((g.text)().as_str())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    match args.first().map(String::as_str) {
        // `cargo xtask check` fails on any drift (CI).
        Some("check") => {
            let stale: Vec<&Gen> = GENERATED.iter().filter(|g| !fresh(g)).collect();
            for g in &stale {
                eprintln!("{} is out of date; run `cargo xtask {}`", g.path, g.cmd);
            }
            if !stale.is_empty() {
                std::process::exit(1);
            }
            println!("generated files are up to date");
        }
        // `cargo xtask perf-workspace [DIR]`: the SPEC §15 start-up workspace (10 Servers, 500 Routes).
        Some("perf-workspace") => {
            let dir = args
                .get(1)
                .map(String::as_str)
                .unwrap_or("fixtures/perf-workspace");
            perf_workspace(Path::new(dir));
            println!("wrote {dir}");
        }
        Some("all") => GENERATED.iter().for_each(write),
        Some(cmd) if GENERATED.iter().any(|g| g.cmd == cmd) => {
            let g = GENERATED.iter().find(|g| g.cmd == cmd).unwrap();
            if check {
                if !fresh(g) {
                    eprintln!("{} is out of date; run `cargo xtask {}`", g.path, g.cmd);
                    std::process::exit(1);
                }
            } else {
                write(g);
            }
        }
        _ => {
            eprintln!(
                "usage: cargo xtask schema|settings-schema|settings-docs|keybindings-docs|template-docs|generator-docs|mcp-tools-docs|cli-docs [--check] | check | all | perf-workspace [DIR]"
            );
            std::process::exit(2);
        }
    }
}

const MCP_DOC: &str = "site/src/content/docs/reference/mcp-tools.md";
const CLI_DOC: &str = "site/src/content/docs/reference/cli.md";

/// The CLI reference comes from the binary's own clap tree (`fauxfield docs-cli`).
fn cli_docs() -> String {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = std::process::Command::new(cargo)
        .args([
            "run",
            "-q",
            "-p",
            "fauxfield",
            "--no-default-features",
            "--features",
            "headless",
            "--",
            "docs-cli",
        ])
        .output()
        .expect("run cargo");
    assert!(
        out.status.success(),
        "docs-cli failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8")
}

/// Every tool of the full registry (no UI tools), with class, hints and arguments, plus the
/// resources and prompts.
fn mcp_tools_docs() -> String {
    use std::sync::Arc;

    use fauxfield_mcp::{
        control_tools::register_control_tools, resources, workspace_tools::register_workspace_tools,
    };
    let mut r = fauxfield_mcp::build_registry(&Default::default());
    register_workspace_tools(&mut r, Arc::new(|| None));
    register_control_tools(&mut r, Arc::new(|_| Ok(None)));
    let mut o = String::from(
        "---\ntitle: MCP tools\ndescription: Every tool, resource and prompt of the Fauxfield MCP server, with its class and arguments.\n---\n\n<!-- Generated by `cargo xtask mcp-tools-docs`; do not edit. -->\n\n",
    );
    o.push_str("`fauxfield mcp` selects classes with `--allow read,write,execute` (default all; `--read-only` is `--allow read`). A tool outside the allowed classes is not listed and is refused with a protocol error. Results never contain secret values.\n\n");
    o.push_str("## Tools\n\n| Tool | Class | Read-only | Destructive | Idempotent | Purpose |\n|---|---|---|---|---|---|\n");
    let mut tools: Vec<_> = r.all().iter().collect();
    tools.sort_by_key(|t| (t.class.to_string(), t.name));
    for t in &tools {
        let h = t.hints();
        let yn = |b: bool| if b { "yes" } else { "no" };
        o.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} |\n",
            t.name,
            t.class,
            yn(h.read_only),
            yn(h.destructive),
            yn(h.idempotent),
            t.description.replace('|', "\\|").replace('\n', " ")
        ));
    }
    o.push_str("\n## Arguments\n\n");
    for t in &tools {
        o.push_str(&format!("### `{}`\n\n", t.name));
        let props = t.input_schema.get("properties").and_then(|p| p.as_object());
        let required: Vec<&str> = t
            .input_schema
            .get("required")
            .and_then(|r| r.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
            .unwrap_or_default();
        match props {
            Some(p) if !p.is_empty() => {
                o.push_str("| Argument | Required | Description |\n|---|---|---|\n");
                let mut names: Vec<_> = p.keys().collect();
                names.sort();
                for n in names {
                    let d = p[n]
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("")
                        .replace('|', "\\|")
                        .replace('\n', " ");
                    o.push_str(&format!(
                        "| `{n}` | {} | {d} |\n",
                        if required.contains(&n.as_str()) {
                            "yes"
                        } else {
                            "no"
                        }
                    ));
                }
                o.push('\n');
            }
            _ => o.push_str("No arguments.\n\n"),
        }
    }
    o.push_str("## Resources\n\n| URI | Description |\n|---|---|\n");
    for res in resources::resources() {
        o.push_str(&format!(
            "| `{}` | {} |\n",
            res.uri,
            res.description.clone().unwrap_or_default()
        ));
    }
    for t in resources::resource_templates() {
        o.push_str(&format!(
            "| `{}` | {} |\n",
            t.uri_template,
            t.description.clone().unwrap_or_default()
        ));
    }
    o.push_str("\n## Prompts\n\n| Prompt | Argument | Description |\n|---|---|---|\n");
    for p in resources::prompts() {
        let arg = p
            .arguments
            .as_ref()
            .and_then(|a| a.first())
            .map(|a| a.name.clone())
            .unwrap_or_default();
        o.push_str(&format!(
            "| `{}` | `{arg}` | {} |\n",
            p.name,
            p.description.clone().unwrap_or_default()
        ));
    }
    o
}

const GENERATOR_DOC: &str = "site/src/content/docs/reference/seed-generators.md";

fn generator_docs() -> String {
    let mut o = String::from(
        "---\ntitle: Seed generators\ndescription: Every generator a Dataset can declare to fill itself with realistic rows.\n---\n\n<!-- Generated by `cargo xtask generator-docs`; do not edit. -->\n\nA Dataset may declare `[datasets.generator]` with a row count and a field list instead of `seed` or `seed_template` (at most one source). Each field names a generator with `kind` and its options. With the Server's `fake_seed` set, the same rows are produced on every start.\n\n## Generators\n\n| Kind | Description |\n|---|---|\n",
    );
    for (n, d) in fauxfield_model::GENERATORS {
        o.push_str(&format!("| `{n}` | {} |\n", d.replace('|', "\\|")));
    }
    o
}

fn template_docs() -> String {
    use fauxfield_engine::template::{FAKE_KINDS, FILTERS, FUNCTIONS};
    let mut o = String::from(
        "---\ntitle: Template functions and fake kinds\ndescription: Every Tera function, filter and fake kind available in Response bodies and headers.\n---\n\n<!-- Generated by `cargo xtask template-docs`; do not edit. -->\n\n",
    );
    for (title, rows) in [
        ("Functions", FUNCTIONS),
        ("Filters", FILTERS),
        ("Fake kinds", FAKE_KINDS),
    ] {
        o.push_str(&format!(
            "## {title}\n\n| Name | Description |\n|---|---|\n"
        ));
        for (n, d) in rows {
            o.push_str(&format!("| `{n}` | {} |\n", d.replace('|', "\\|")));
        }
        o.push('\n');
        if title == "Functions" {
            o.push_str("## Callback context\n\nInside a Callback (`url`, `headers`, `body`) the usual context is joined by `response` (`status`, `headers`, `body`, `json` of the Response that was sent) and, for `url` and `headers`, `callback` (`id`, `method` and `body`, the rendered body, so `hmac_sha256(key=vars.secret, data=callback.body)` signs what is sent). See the Callbacks guide.\n\n");
        }
    }
    o
}

const SETTINGS_SCHEMA: &str = "site/public/schema/v1/settings.json";
const SETTINGS_DOC: &str = "site/src/content/docs/reference/settings.md";
const KEYBINDINGS_DOC: &str = "site/src/content/docs/reference/keybindings.md";

fn kind_json(k: fauxfield_store::settings::SettingKind) -> serde_json::Value {
    use fauxfield_store::settings::SettingKind as K;
    use serde_json::json;
    match k {
        K::Bool => json!({"type": "boolean"}),
        K::Int { min, max } => json!({"type": "integer", "minimum": min, "maximum": max}),
        K::Float { min, max, .. } => json!({"type": "number", "minimum": min, "maximum": max}),
        K::Text | K::Font | K::Path => json!({"type": "string"}),
        K::Enum(o) => json!({"type": "string", "enum": o}),
        K::Multi(o) => json!({"type": "array", "items": {"type": "string", "enum": o}}),
    }
}

/// JSON Schema of settings.toml, from `fauxfield_store::settings::SETTING_DEFS`.
fn settings_schema() -> String {
    use serde_json::{Map, Value, json};
    let mut root = Map::new();
    for d in fauxfield_store::settings::SETTING_DEFS {
        let mut prop = kind_json(d.kind);
        let o = prop.as_object_mut().unwrap();
        o.insert("description".into(), json!(d.description));
        if let Some(v) = fauxfield_store::settings::default_value(d) {
            o.insert("default".into(), serde_json::to_value(v).unwrap());
        }
        let mut cur = &mut root;
        let parts: Vec<&str> = d.key.split('.').collect();
        for (i, p) in parts.iter().enumerate() {
            if i + 1 == parts.len() {
                cur.insert((*p).into(), prop.clone());
            } else {
                let e = cur
                    .entry(*p)
                    .or_insert_with(|| json!({"type": "object", "properties": {}}));
                cur = e
                    .as_object_mut()
                    .unwrap()
                    .get_mut("properties")
                    .and_then(Value::as_object_mut)
                    .unwrap();
            }
        }
    }
    let schema = json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "$id": "https://fauxfield.dev/schema/v1/settings.json",
        "title": "Fauxfield settings.toml",
        "type": "object",
        "properties": root,
    });
    serde_json::to_string_pretty(&schema).unwrap() + "\n"
}

fn settings_docs() -> String {
    use fauxfield_store::settings::{Apply, CATEGORIES, SETTING_DEFS};
    let mut o = String::from(
        "---\ntitle: settings.toml reference\ndescription: Every key of settings.toml. Generated from the app's settings table.\n---\n\n\
<!-- Generated by `cargo xtask settings-docs`; do not edit. -->\n\n\
Settings live in `settings.toml` in the config directory (`~/.config/fauxfield` on Linux and macOS, `%APPDATA%\\fauxfield` on Windows; `$FAUXFIELD_SETTINGS` overrides the path). Edit them in the Settings tab (`Cmd+,` / `Ctrl+,`) or by hand: the app watches the file, and writes keep your comments, key order and unknown keys. An invalid value becomes a Problem and the default stays in effect. A [JSON Schema](/schema/v1/settings.json) is published for editors.\n\n",
    );
    for c in CATEGORIES {
        let defs: Vec<_> = SETTING_DEFS
            .iter()
            .filter(|d| d.category == *c && !d.hidden)
            .collect();
        if defs.is_empty() {
            continue;
        }
        o.push_str(&format!(
            "## {c}\n\n| Key | Default | Description |\n|---|---|---|\n"
        ));
        for d in defs {
            let default = if d.default.is_empty() {
                "unset"
            } else {
                d.default
            };
            let note = match d.apply {
                Apply::NewSessions => " Applies to new sessions.",
                Apply::Instant => "",
            };
            o.push_str(&format!(
                "| `{}` | `{}` | {}{} |\n",
                d.key,
                default.replace('|', "\\|"),
                d.description,
                note
            ));
        }
        o.push('\n');
    }
    o
}

fn binding_rows(src: &str) -> Vec<(String, String, String)> {
    let t: toml::Table = src.parse().expect("keymap parses");
    t.get("binding")
        .and_then(|b| b.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|b| {
                    let b = b.as_table()?;
                    let s = |k: &str| b.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    Some((s("action"), s("keys"), s("context")))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn keybindings_docs() -> String {
    let mut o = String::from(
        "---\ntitle: Default keybindings\ndescription: Every default binding and the vim list-navigation preset. Generated from the bundled keymap.\n---\n\n\
<!-- Generated by `cargo xtask keybindings-docs`; do not edit. -->\n\n\
`secondary` is Cmd on macOS and Ctrl elsewhere. Override any binding in the Keymap category of the Settings tab; overrides are the only thing written to `keymap.toml`.\n\n",
    );
    for (title, src) in [
        (
            "Defaults",
            include_str!("../../crates/fauxfield-gui/assets/keymap/default.toml"),
        ),
        (
            "Vim list navigation preset",
            include_str!("../../crates/fauxfield-gui/assets/keymap/vim.toml"),
        ),
    ] {
        o.push_str(&format!(
            "## {title}\n\n| Action | Keys | Context |\n|---|---|---|\n"
        ));
        for (a, k, c) in binding_rows(src) {
            let c = if c.is_empty() { "any".to_string() } else { c };
            o.push_str(&format!("| `{a}` | `{k}` | {c} |\n"));
        }
        o.push('\n');
    }
    o
}

/// Deterministic perf workspace: 10 Servers on ports 4800.., 50 Routes each (SPEC §15).
fn perf_workspace(dir: &Path) {
    let servers = dir.join("servers");
    std::fs::create_dir_all(&servers).unwrap();
    std::fs::write(
        dir.join("fauxfield.toml"),
        "schema = 1\nname = \"Perf workspace\"\n",
    )
    .unwrap();
    for s in 0..10 {
        let mut t = format!(
            "schema = 1\nid = \"srv_perf_{s:02}\"\nname = \"Perf {s:02}\"\nenabled = true\nhost = \"127.0.0.1\"\nport = {}\n",
            4800 + s
        );
        for r in 0..50 {
            t += &format!(
                "\n[[routes]]\nid = \"rt_perf_{s:02}_{r:02}\"\nname = \"Route {r:02}\"\nmethod = \"GET\"\npath = \"/api/r{r:02}/items\"\n\n  [[routes.responses]]\n  id = \"rsp_perf_{s:02}_{r:02}\"\n  name = \"200 OK\"\n  default = true\n  status = 200\n  content_type = \"application/json\"\n  body = '''{{ \"server\": {s}, \"route\": {r} }}'''\n"
            );
        }
        std::fs::write(servers.join(format!("{s:02}-perf-{s:02}.toml")), t).unwrap();
    }
}
