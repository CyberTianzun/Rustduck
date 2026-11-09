use super::{NativeEventSink, NativePaneEvent, NativePaneSnapshot};
use futures::channel::mpsc::UnboundedReceiver;
use std::sync::Mutex;

#[derive(Default)]
pub struct PlatformHost {
    events: Mutex<NativeEventSink>,
}

impl PlatformHost {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sync_pane(&self, _window: &gpui::Window, _snapshot: &NativePaneSnapshot) {}

    pub fn hide_all(&self) {}

    pub fn drain_events(&self) -> Vec<NativePaneEvent> {
        self.events.lock().unwrap().drain()
    }

    pub fn subscribe_wakeup(&self) -> UnboundedReceiver<()> {
        self.events.lock().unwrap().subscribe()
    }

    pub fn push_event(&self, event: NativePaneEvent) {
        self.events.lock().unwrap().push(event);
    }
}
