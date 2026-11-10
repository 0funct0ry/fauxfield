//! Fauxfield GPUI application: window shell, docks, tabs, Quick Search, Command Palette and
//! themes, and the workspace views. Only this crate and the `fauxfield` binary may depend on GPUI.

pub mod actions;
pub mod assets;
pub mod bottom_dock;
pub mod center;
pub mod chord;
pub mod code_editor;
pub mod combo_box;
pub mod dataset_ui;
#[cfg(feature = "dev-mcp")]
pub mod dev_mcp;
pub mod editor;
pub mod header_catalog;
pub mod header_table;
pub mod https_ui;
pub mod keymap;
pub mod layout;
pub mod left_dock;
pub mod lifecycle;
pub mod menu;
pub mod menu_shell;
pub mod menubar;
pub mod network_ui;
pub mod overlay;
pub mod preview;
pub mod response_pane;
pub mod route_headers;
pub mod rule_curl;
pub mod rules_ui;
pub mod run;
pub mod sample;
pub mod search;
pub mod server_dialog;
pub mod shell;
pub mod status_bar;
pub mod tabs;
pub mod text_input;
pub mod theme;
pub mod theme_model;
pub mod title_bar;
pub mod traffic;
pub mod tree;
pub mod ui;
pub mod view;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use gpui::{
    App, AppContext as _, Application, Bounds, SharedString, TitlebarOptions, WindowBounds,
    WindowOptions, point, px, size,
};

pub use shell::{Shell, ShellOptions};

/// Which workspace `fauxfield [PATH]` opens: the explicit path, else the nearest ancestor of
/// `cwd` containing `fauxfield.toml`, else `None` (the Welcome view).
pub fn resolve_workspace(explicit: Option<PathBuf>, cwd: &std::path::Path) -> Option<PathBuf> {
    explicit.or_else(|| fauxfield_store::discover(cwd))
}

/// Starts the application and blocks until the last window closes.
pub fn run(workspace: Option<PathBuf>) {
    launch(workspace, |_, _| {});
}

/// Like [`run`], and hosts the MCP server inside the app (SPEC §11.4).
#[cfg(feature = "dev-mcp")]
pub fn run_dev(workspace: Option<PathBuf>, opts: dev_mcp::DevMcpOptions) {
    launch(workspace, move |window, cx| {
        dev_mcp::host::start(opts, window, cx)
    });
}

/// Where Recent lives. dev-mcp builds use a scratch file so scenarios and fixtures that clear or
/// fill Recent never touch the real one (`FAUXFIELD_RECENT` overrides the location).
fn recent_file(dirs: Option<&directories::ProjectDirs>) -> Option<PathBuf> {
    #[cfg(feature = "dev-mcp")]
    {
        let _ = dirs;
        Some(
            std::env::var_os("FAUXFIELD_RECENT")
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::temp_dir().join("fauxfield-dev-recent.json")),
        )
    }
    #[cfg(not(feature = "dev-mcp"))]
    {
        dirs.map(|d| d.data_dir().join("recent.json"))
    }
}

fn launch(
    workspace: Option<PathBuf>,
    after_open: impl FnOnce(gpui::WindowHandle<Shell>, &mut App) + 'static,
) {
    let workspace = resolve_workspace(workspace, &std::env::current_dir().unwrap_or_default());
    Application::new()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            assets::load_fonts(cx).expect("bundled fonts load");
            let dirs = directories::ProjectDirs::from("dev", "fauxfield", "fauxfield");
            let opts = ShellOptions {
                layout_path: dirs.as_ref().map(|d| d.data_dir().join("layout.json")),
                config_dir: dirs.as_ref().map(|d| d.config_dir().to_path_buf()),
                workspace: workspace.clone(),
                recent_path: recent_file(dirs.as_ref()),
                port_offset: 0,
            };
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        titlebar: Some(TitlebarOptions {
                            title: Some(SharedString::from("Fauxfield")),
                            appears_transparent: cfg!(target_os = "macos"),
                            traffic_light_position: Some(point(px(9.), px(7.))),
                        }),
                        window_min_size: Some(size(px(900.), px(600.))),
                        ..Default::default()
                    },
                    move |window, cx| cx.new(|cx| Shell::new(opts, window, cx)),
                )
                .expect("open main window");
            after_open(window, cx);
            cx.activate(true);
        });
}
