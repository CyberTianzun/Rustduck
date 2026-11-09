use gpui::{px, size, App, AppContext as _, Bounds};
use rustduck::ui::{
    workspace::{workspace_window_options, RustduckWorkspace},
    RustduckAssets,
};

fn main() {
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
