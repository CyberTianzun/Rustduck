use super::{NativePaneEvent, NativePaneSnapshot};

#[derive(Default)]
pub struct PlatformHost;

impl PlatformHost {
    pub fn new() -> Self {
        Self
    }

    pub fn sync_pane(&self, _window: &gpui::Window, _snapshot: &NativePaneSnapshot) {}

    pub fn hide_all(&self) {}

    pub fn drain_events(&self) -> Vec<NativePaneEvent> {
        Vec::new()
    }
}
