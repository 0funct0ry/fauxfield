//! Example workspaces (SPEC §20.37, M19.1): ten ready-to-run multi-Server Workspaces compiled
//! into the binary from `examples/<name>/`. One registry feeds the CLI, GUI, MCP and docs.

use std::path::{Component, Path, PathBuf};

use include_dir::{Dir, include_dir};
use serde::Serialize;

use crate::store::StoreError;

static TREES: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../../examples");

/// Static facts about an Example; counts are computed from the embedded tree.
#[derive(Debug, Clone, Copy)]
pub struct ExampleMeta {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
}

pub const EXAMPLES: &[ExampleMeta] = &[
    ExampleMeta {
        name: "ecommerce",
        title: "E-commerce platform",
        description: "Catalog, cart, orders, payments with a webhook Callback, shipping and users.",
    },
    ExampleMeta {
        name: "banking",
        title: "Fintech and banking",
        description: "Accounts, paginated transactions, transfers, cards, KYC workflow and FX rates.",
    },
    ExampleMeta {
        name: "social",
        title: "Social network",
        description: "Profiles, posts with likes, comments, cursor feed, WebSocket notifications, media.",
    },
    ExampleMeta {
        name: "food-delivery",
        title: "Food delivery",
        description: "Restaurants, menus, orders, couriers, WebSocket tracking and promotions.",
    },
    ExampleMeta {
        name: "ride-hailing",
        title: "Ride hailing",
        description: "Riders, drivers, trip state machine, surge pricing, payments and ratings.",
    },
    ExampleMeta {
        name: "saas-billing",
        title: "SaaS multi-tenant billing",
        description: "OAuth2 token flow, tenants, subscriptions, invoices, metered usage, webhooks.",
    },
    ExampleMeta {
        name: "healthcare",
        title: "Healthcare records (synthetic)",
        description: "Patients, appointments, prescriptions, labs, insurance eligibility, providers.",
    },
    ExampleMeta {
        name: "iot-fleet",
        title: "IoT and fleet",
        description: "Devices, telemetry series, firmware updates, alerts, geofences, WebSocket stream.",
    },
    ExampleMeta {
        name: "streaming-media",
        title: "Video and music streaming",
        description: "Catalog, playback manifests, playlists, recommendations, subscriptions, analytics.",
    },
    ExampleMeta {
        name: "travel-booking",
        title: "Travel booking",
        description: "Flights, hotels, cars, bookings (hold, confirm, cancel), payments and loyalty.",
    },
];

/// One Server of an Example.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ExampleServer {
    pub slug: String,
    pub port: u16,
    pub routes: usize,
}

/// A registry entry with computed counts.
#[derive(Debug, Clone, Serialize)]
pub struct ExampleInfo {
    pub name: String,
    pub title: String,
    pub servers: usize,
    pub routes: usize,
    pub description: String,
}

/// Result of [`init_example`].
#[derive(Debug, Clone, Serialize)]
pub struct ExampleReport {
    pub path: PathBuf,
    pub example: String,
    pub created: Vec<String>,
    pub skipped: Vec<String>,
    pub servers: Vec<ExampleServer>,
}

fn tree(name: &str) -> Option<&'static Dir<'static>> {
    TREES.get_dir(name)
}

/// All files of an Example as `(relative path, bytes)`, sorted.
pub fn example_files(name: &str) -> Vec<(String, &'static [u8])> {
    fn walk(d: &'static Dir<'static>, base: &str, out: &mut Vec<(String, &'static [u8])>) {
        for f in d.files() {
            let p = f.path().to_string_lossy().replace('\\', "/");
            out.push((p[base.len()..].to_string(), f.contents()));
        }
        for s in d.dirs() {
            walk(s, base, out);
        }
    }
    let mut out = Vec::new();
    if let Some(d) = tree(name) {
        walk(d, &format!("{name}/"), &mut out);
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Servers (slug = file stem, port, Route count) of an Example, in file order.
pub fn example_servers(name: &str) -> Vec<ExampleServer> {
    example_files(name)
        .into_iter()
        .filter(|(p, _)| p.starts_with("servers/") && p.ends_with(".toml"))
        .filter_map(|(p, bytes)| {
            let v: toml::Value = toml::from_str(std::str::from_utf8(bytes).ok()?).ok()?;
            Some(ExampleServer {
                slug: Path::new(&p).file_stem()?.to_string_lossy().into_owned(),
                port: v.get("port")?.as_integer()? as u16,
                routes: v
                    .get("routes")
                    .and_then(|r| r.as_array())
                    .map_or(0, Vec::len),
            })
        })
        .collect()
}

/// Registry with counts, in registry order.
pub fn list_examples() -> Vec<ExampleInfo> {
    EXAMPLES
        .iter()
        .map(|m| {
            let servers = example_servers(m.name);
            ExampleInfo {
                name: m.name.into(),
                title: m.title.into(),
                servers: servers.len(),
                routes: servers.iter().map(|s| s.routes).sum(),
                description: m.description.into(),
            }
        })
        .collect()
}

/// Closest registry name to `name` (edit distance <= 3 or a prefix/substring match).
pub fn suggest_example(name: &str) -> Option<&'static str> {
    let n = name.to_ascii_lowercase();
    EXAMPLES
        .iter()
        .map(|m| (lev(&n, m.name), m.name))
        .filter(|(d, m)| *d <= 3 || m.contains(&n) || n.contains(m))
        .min_by_key(|(d, _)| *d)
        .map(|(_, m)| m)
}

fn lev(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Write Example `name` into `root`. Existing files are never changed. With `force` false a
/// target that already holds any of the Example's files fails and nothing is written; with
/// `force` true those files are skipped and the rest is written.
pub fn init_example(root: &Path, name: &str, force: bool) -> Result<ExampleReport, StoreError> {
    if tree(name).is_none() {
        return Err(StoreError::NotFound(format!("example {name}")));
    }
    let ws_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace");
    let mut plan: Vec<(String, Vec<u8>)> = Vec::new();
    for (rel, bytes) in example_files(name) {
        if Path::new(&rel)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(StoreError::Invalid(format!("bad example path {rel}")));
        }
        let mut data = bytes.to_vec();
        if rel == "fauxfield.toml" {
            let text = String::from_utf8_lossy(bytes)
                .replace("__WORKSPACE_NAME__", &ws_name.replace(['"', '\\'], ""));
            data = text.into_bytes();
        }
        plan.push((rel, data));
    }
    plan.push((".gitignore".into(), b".fauxfield/\n".to_vec()));
    let conflicts: Vec<String> = plan
        .iter()
        .filter(|(rel, _)| root.join(rel).exists())
        .map(|(rel, _)| rel.clone())
        .collect();
    if !conflicts.is_empty() && !force {
        return Err(StoreError::Invalid(format!(
            "{} already contains: {} (use --force-example to skip existing files)",
            root.display(),
            conflicts.join(", ")
        )));
    }
    let (mut created, mut skipped) = (Vec::new(), Vec::new());
    for (rel, data) in plan {
        let p = root.join(&rel);
        if p.exists() {
            skipped.push(rel);
            continue;
        }
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&p, data)?;
        created.push(rel);
    }
    for d in ["servers", "environments", "data", "files", "docs"] {
        std::fs::create_dir_all(root.join(d))?;
    }
    Ok(ExampleReport {
        path: root.to_path_buf(),
        example: name.into(),
        created,
        skipped,
        servers: example_servers(name),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_close_names() {
        assert_eq!(suggest_example("ecomerce"), Some("ecommerce"));
        assert_eq!(suggest_example("travel"), Some("travel-booking"));
        assert_eq!(suggest_example("zzzzzzzzzzzz"), None);
    }
}
