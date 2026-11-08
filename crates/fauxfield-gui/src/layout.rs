//! Dock sizes, visibility, open tabs and the chosen theme, persisted as JSON in the platform
//! data dir (M1 only; per-workspace `.fauxfield/ui.json` arrives in M2). Loading never fails: a
//! missing, corrupt, or future-version file yields defaults.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::tabs::Tab;

pub const VERSION: u32 = 1;
pub const LEFT: (f32, f32, f32) = (160.0, 260.0, 520.0);
pub const RIGHT: (f32, f32, f32) = (200.0, 300.0, 560.0);
pub const BOTTOM: (f32, f32, f32) = (120.0, 260.0, 700.0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BottomTab {
    #[default]
    Traffic,
    Problems,
    Logs,
}

impl BottomTab {
    pub const ALL: [BottomTab; 3] = [BottomTab::Traffic, BottomTab::Problems, BottomTab::Logs];

    pub fn label(self) -> &'static str {
        match self {
            BottomTab::Traffic => "Traffic",
            BottomTab::Problems => "Problems",
            BottomTab::Logs => "Logs",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub version: u32,
    pub left_open: bool,
    pub right_open: bool,
    pub bottom_open: bool,
    pub left_width: f32,
    pub right_width: f32,
    pub bottom_height: f32,
    pub bottom_tab: BottomTab,
    /// Selected sidebar tab id (`servers`|`environments`|`datasets`); unknown loads as servers.
    pub sidebar_tab: String,
    pub theme: Option<String>,
    pub tabs: Vec<Tab>,
    pub active_tab: Option<String>,
}

impl Default for Layout {
    fn default() -> Layout {
        Layout {
            version: VERSION,
            left_open: true,
            right_open: false,
            bottom_open: true,
            left_width: LEFT.1,
            right_width: RIGHT.1,
            bottom_height: BOTTOM.1,
            bottom_tab: BottomTab::Traffic,
            sidebar_tab: "servers".into(),
            theme: None,
            tabs: Vec::new(),
            active_tab: None,
        }
    }
}

fn clamp(v: f32, (min, def, max): (f32, f32, f32)) -> f32 {
    if v.is_finite() {
        v.clamp(min, max)
    } else {
        def
    }
}

impl Layout {
    pub fn clamped(mut self) -> Layout {
        self.left_width = clamp(self.left_width, LEFT);
        self.right_width = clamp(self.right_width, RIGHT);
        self.bottom_height = clamp(self.bottom_height, BOTTOM);
        self.sidebar_tab = crate::tree::SidebarTab::from_id(&self.sidebar_tab)
            .id()
            .into();
        self
    }

    pub fn set_left_width(&mut self, w: f32) {
        self.left_width = clamp(w, LEFT);
    }

    pub fn set_right_width(&mut self, w: f32) {
        self.right_width = clamp(w, RIGHT);
    }

    pub fn set_bottom_height(&mut self, h: f32) {
        self.bottom_height = clamp(h, BOTTOM);
    }

    pub fn load(path: &Path) -> Layout {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Layout>(&s).ok())
            .filter(|l| l.version == VERSION)
            .map(Layout::clamped)
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)
            .with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tabs::TabKind;

    #[test]
    fn round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub/layout.json");
        let mut l = Layout {
            left_open: false,
            bottom_tab: BottomTab::Logs,
            theme: Some("One Light".into()),
            ..Layout::default()
        };
        l.tabs.push(Tab {
            id: "rt_1".into(),
            kind: TabKind::Route,
            title: "GET /x".into(),
            preview: true,
        });
        l.active_tab = Some("rt_1".into());
        l.set_left_width(333.0);
        l.save(&p).unwrap();
        assert_eq!(Layout::load(&p), l);
    }

    #[test]
    fn corrupt_missing_and_future_files_give_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("l.json");
        assert_eq!(Layout::load(&p), Layout::default());
        std::fs::write(&p, "{ nope").unwrap();
        assert_eq!(Layout::load(&p), Layout::default());
        std::fs::write(&p, r#"{"version": 99, "left_open": false}"#).unwrap();
        assert_eq!(Layout::load(&p), Layout::default());
    }

    #[test]
    fn partial_files_fill_defaults_and_sizes_clamp() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("l.json");
        std::fs::write(&p, r#"{"version":1,"left_width":9999,"bottom_open":false}"#).unwrap();
        let l = Layout::load(&p);
        assert_eq!(l.left_width, LEFT.2);
        assert!(!l.bottom_open && l.left_open);
        let mut l = Layout::default();
        l.set_bottom_height(1.0);
        assert_eq!(l.bottom_height, BOTTOM.0);
        l.set_right_width(f32::NAN);
        assert_eq!(l.right_width, RIGHT.1);
    }

    #[test]
    fn sidebar_tab_falls_back_to_servers() {
        let dir = std::env::temp_dir().join(format!("ff-layout-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("layout.json");
        for (json, want) in [
            (r#"{"version":1}"#, "servers"),
            (r#"{"version":1,"sidebar_tab":""}"#, "servers"),
            (r#"{"version":1,"sidebar_tab":"history"}"#, "servers"),
            (r#"{"version":1,"sidebar_tab":"datasets"}"#, "datasets"),
        ] {
            std::fs::write(&p, json).unwrap();
            assert_eq!(Layout::load(&p).sidebar_tab, want);
        }
        let l = Layout {
            sidebar_tab: "environments".into(),
            ..Layout::default()
        };
        l.save(&p).unwrap();
        assert_eq!(Layout::load(&p), l);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
