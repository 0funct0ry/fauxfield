//! `fauxfield theme list|set` (SPEC §10, §13): reads the bundled Zed-format themes and any
//! `*.json` family in `<config dir>/fauxfield/themes`, writes `theme` in `settings.toml`
//! (comments and other keys survive).

use std::path::PathBuf;

use clap::Subcommand;
use fauxfield_store::settings::{ThemeMode, write_theme};
use serde::Deserialize;
use serde_json::json;
use toml_edit::DocumentMut;

use crate::{
    cli::{EXIT_OK, EXIT_RUNTIME, EXIT_USAGE},
    output::print_json,
};

#[derive(Subcommand, Debug)]
pub enum ThemeAction {
    /// List the bundled and user themes.
    List {
        /// Machine-readable output with a stable schema.
        #[arg(long)]
        json: bool,
    },
    /// Use a theme (writes `theme` in settings.toml).
    Set {
        /// Theme name (case-insensitive), as shown by `theme list`.
        name: String,
        /// Machine-readable output with a stable schema.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Deserialize)]
struct Family {
    #[serde(default)]
    themes: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    name: String,
    appearance: String,
}

#[derive(Clone)]
struct Theme {
    name: String,
    appearance: String,
    source: &'static str,
}

fn parse(text: &str, source: &'static str) -> Vec<Theme> {
    serde_json::from_str::<Family>(text)
        .map(|f| {
            f.themes
                .into_iter()
                .map(|t| Theme {
                    name: t.name,
                    appearance: t.appearance,
                    source,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn user_dir() -> Option<PathBuf> {
    fauxfield_store::settings::user_themes_dir()
}

fn all_themes() -> Vec<Theme> {
    let mut v: Vec<Theme> = fauxfield_store::settings::BUNDLED_THEMES
        .iter()
        .map(|(name, appearance)| Theme {
            name: (*name).to_owned(),
            appearance: (*appearance).to_owned(),
            source: "bundled",
        })
        .collect();
    if let Some(dir) = user_dir()
        && let Ok(rd) = std::fs::read_dir(dir)
    {
        let mut files: Vec<_> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        for p in files {
            if let Ok(t) = std::fs::read_to_string(&p) {
                v.extend(parse(&t, "user"));
            }
        }
    }
    v
}

fn current(doc: &DocumentMut) -> (String, String, String) {
    let t = doc.get("theme");
    let get = |k: &str, d: &str| {
        t.and_then(|t| t.get(k))
            .and_then(|v| v.as_str())
            .unwrap_or(d)
            .to_owned()
    };
    (
        get("mode", "system"),
        get("light", "One Light"),
        get("dark", "One Dark"),
    )
}

pub fn theme(action: ThemeAction) -> i32 {
    let path = fauxfield_store::settings::settings_path();
    let text = path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let doc: DocumentMut = match text.parse() {
        Ok(d) => d,
        Err(e) => {
            crate::err!("settings.toml is not valid TOML: {e}");
            return EXIT_RUNTIME;
        }
    };
    let themes = all_themes();
    match action {
        ThemeAction::List { json } => {
            let (mode, light, dark) = current(&doc);
            let active = |t: &Theme| {
                (t.appearance == "dark" && t.name == dark && mode != "light")
                    || (t.appearance == "light" && t.name == light && mode != "dark")
            };
            if json {
                print_json(&json!({
                    "mode": mode,
                    "themes": themes.iter().map(|t| json!({
                        "name": t.name, "appearance": t.appearance,
                        "source": t.source, "active": active(t),
                    })).collect::<Vec<_>>(),
                }));
            } else {
                for t in &themes {
                    println!(
                        "{}{}  ({}, {})",
                        t.name,
                        if active(t) { " *" } else { "" },
                        t.appearance,
                        t.source
                    );
                }
            }
            EXIT_OK
        }
        ThemeAction::Set { name, json } => {
            let Some(t) = themes.iter().find(|t| t.name.eq_ignore_ascii_case(&name)) else {
                crate::err!(
                    "unknown theme `{name}`; run `fauxfield theme list` ({} available)",
                    themes.len()
                );
                return EXIT_USAGE;
            };
            let Some(path) = path else {
                crate::err!("no config directory to write settings.toml to");
                return EXIT_RUNTIME;
            };
            let slot = if t.appearance == "dark" {
                "dark"
            } else {
                "light"
            };
            let which = if slot == "dark" {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            };
            if let Err(e) = write_theme(&path, Some(which), Some((which, t.name.as_str()))) {
                crate::err!("{e}");
                return EXIT_RUNTIME;
            }
            if json {
                print_json(&json!({
                    "theme": t.name, "appearance": t.appearance,
                    "settings": path.to_string_lossy(),
                }));
            } else {
                println!("Theme set to {} ({})", t.name, path.display());
            }
            EXIT_OK
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn bundled_index_lists_every_family() {
        let t = super::all_themes();
        let names: Vec<_> = t.iter().map(|t| t.name.as_str()).collect();
        assert!(names.starts_with(&["One Dark", "One Light"]));
        assert!(names.contains(&"Catppuccin Mocha") && names.contains(&"Nord Dark"));
    }
}
