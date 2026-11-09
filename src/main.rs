use gpui::{px, size, App, AppContext as _, Bounds};
use rustduck::ui::{
    workspace::{workspace_window_options, RustduckWorkspace},
    RustduckAssets,
};

pub fn configure_native_child_hosting_env() {
    #[cfg(target_os = "windows")]
    {
        // Disable DirectComposition topmost visual so embedded Win32 child HWNDs
        // (SysListView32) render cleanly inside the GPUI SwapChain window.
        std::env::set_var("GPUI_DISABLE_DIRECT_COMPOSITION", "1");
    }
}

fn main() {
    configure_native_child_hosting_env();
    let app = gpui_kit::application().with_assets(RustduckAssets);

    app.run(move |cx: &mut App| {
        gpui_kit::init(cx);

        let bounds = Bounds::centered(None, size(px(1360.0), px(860.0)), cx);
        let window_options = workspace_window_options(bounds);

        gpui_kit::open_window(window_options, cx, |window, cx| {
            cx.new(|cx| RustduckWorkspace::new(window, cx))
        })
        .expect("Failed to open Rustduck window");
    });
}
