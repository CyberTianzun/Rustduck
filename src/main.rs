#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use gpui::{App, AppContext as _, Bounds, px, size};
use rustduck::ui::{
    RustduckAssets,
    workspace::{
        RustduckWorkspace, configure_app_lifecycle, preferred_app_quit_mode,
        register_main_window_close_quit, workspace_window_options,
    },
};

pub fn configure_native_child_hosting_env() {
    #[cfg(target_os = "windows")]
    {
        // Disable DirectComposition topmost visual so embedded Win32 child HWNDs
        // (SysListView32) render cleanly inside the GPUI SwapChain window.
        // SAFETY: Called once at process startup on the main thread before spawning worker threads.
        unsafe {
            std::env::set_var("GPUI_DISABLE_DIRECT_COMPOSITION", "1");
        }
    }
}

fn main() {
    configure_native_child_hosting_env();
    let app = gpui_kit::application()
        .with_assets(RustduckAssets)
        .with_quit_mode(preferred_app_quit_mode());

    app.run(move |cx: &mut App| {
        gpui_kit::init(cx);
        configure_app_lifecycle(cx);
        cx.activate(true);

        let bounds = Bounds::centered(None, size(px(1360.0), px(860.0)), cx);
        let window_options = workspace_window_options(bounds);

        let (window_handle, _) = gpui_kit::open_window(window_options, cx, |window, cx| {
            window.activate_window();
            cx.new(|cx| RustduckWorkspace::new(window, cx))
        })
        .expect("Failed to open Rustduck window");

        register_main_window_close_quit(window_handle, cx);
    });
}
