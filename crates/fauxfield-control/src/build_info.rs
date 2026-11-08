//! Build metadata (SPEC §20.14): the single source for the About box, the clipboard text and
//! `fauxfield version`.

/// The pinned `gpui` version (a unit test checks it against the workspace `Cargo.toml`).
pub const GPUI_VERSION: &str = "0.2.2";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildInfo {
    pub version: &'static str,
    pub commit: &'static str,
    pub commit_date: &'static str,
    pub dirty: bool,
    pub target: &'static str,
    pub profile: &'static str,
    pub gpui: &'static str,
    /// Cargo features worth reporting (currently only `dev-mcp`).
    pub features: Vec<&'static str>,
}

impl BuildInfo {
    /// `features` is supplied by the caller crate, which knows its own cfg.
    pub fn current(features: Vec<&'static str>) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            commit: env!("FAUXFIELD_GIT_SHA"),
            commit_date: env!("FAUXFIELD_GIT_DATE"),
            dirty: env!("FAUXFIELD_GIT_DIRTY") == "1",
            target: env!("FAUXFIELD_TARGET"),
            profile: env!("FAUXFIELD_PROFILE"),
            gpui: GPUI_VERSION,
            features,
        }
    }

    /// Short sha, with `-dirty` when the work tree was dirty at build time.
    pub fn commit_label(&self) -> String {
        if self.dirty && self.commit != "unknown" {
            format!("{}-dirty", self.commit)
        } else {
            self.commit.to_string()
        }
    }

    pub fn is_debug(&self) -> bool {
        self.profile == "debug"
    }

    /// The exact text "Copy Version Info" puts on the clipboard; also printed by the CLI.
    pub fn copy_text(&self) -> String {
        let mut s = format!(
            "Fauxfield {} ({}, {})\nBuild: {}, {}\nOS: {} {}\nGPUI: {}",
            self.version,
            self.commit_label(),
            self.commit_date,
            self.profile,
            self.target,
            std::env::consts::OS,
            std::env::consts::ARCH,
            self.gpui,
        );
        if !self.features.is_empty() {
            s.push_str(&format!("\nFeatures: {}", self.features.join(", ")));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> BuildInfo {
        BuildInfo {
            version: "0.1.0",
            commit: "a7f3116",
            commit_date: "2026-11-08",
            dirty: false,
            target: "aarch64-apple-darwin",
            profile: "debug",
            gpui: "0.2.2",
            features: vec!["dev-mcp"],
        }
    }

    #[test]
    fn copy_text_golden() {
        let t = sample().copy_text();
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "Fauxfield 0.1.0 (a7f3116, 2026-11-08)");
        assert_eq!(lines[1], "Build: debug, aarch64-apple-darwin");
        assert!(lines[2].starts_with("OS: "));
        assert_eq!(lines[3], "GPUI: 0.2.2");
        assert_eq!(lines[4], "Features: dev-mcp");
    }

    #[test]
    fn features_line_omitted_when_none_and_dirty_suffix() {
        let mut b = sample();
        b.features.clear();
        b.dirty = true;
        let t = b.copy_text();
        assert_eq!(t.lines().count(), 4);
        assert!(t.starts_with("Fauxfield 0.1.0 (a7f3116-dirty, "));
    }

    #[test]
    fn gpui_version_matches_workspace_pin() {
        let toml = include_str!("../../../Cargo.toml");
        let pin = format!("gpui = \"={GPUI_VERSION}\"");
        assert!(toml.contains(&pin), "workspace Cargo.toml must pin {pin}");
    }
}
