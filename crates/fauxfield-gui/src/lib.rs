//! Fauxfield GPUI application: window shell, docks, tabs, Quick Search, Command Palette and
//! themes, on mock data until M2. Only this crate and the `fauxfield` binary may depend on GPUI.

pub mod actions;
pub mod assets;
pub mod bottom_dock;
pub mod center;
pub mod chord;
#[cfg(feature = "dev-mcp")]
pub mod dev_mcp;
pub mod keymap;
pub mod layout;
pub mod left_dock;
pub mod mock;
pub mod overlay;
pub mod search;
pub mod shell;
pub mod status_bar;
pub mod tabs;
pub mod theme;
pub mod theme_model;
pub mod title_bar;
pub mod tree;
pub mod ui;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use gpui::{
    App, AppContext as _, Application, Bounds, SharedString, TitlebarOptions, WindowBounds,
    WindowOptions, point, px, size,
};

pub use shell::{Shell, ShellOptions};

/// Starts the application and blocks until the last window closes. `workspace` is accepted for
/// `fauxfield [PATH]` and ignored until M2 loads real workspaces.
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

fn launch(
    workspace: Option<PathBuf>,
    after_open: impl FnOnce(gpui::WindowHandle<Shell>, &mut App) + 'static,
) {
    let _ = workspace;
    Application::new()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            assets::load_fonts(cx).expect("bundled fonts load");
            let dirs = directories::ProjectDirs::from("dev", "fauxfield", "fauxfield");
            let opts = ShellOptions {
                layout_path: dirs.as_ref().map(|d| d.data_dir().join("layout.json")),
                config_dir: dirs.as_ref().map(|d| d.config_dir().to_path_buf()),
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
