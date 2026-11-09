use std::future::Future;
use std::sync::OnceLock;
use tokio::runtime::{Builder, Runtime};
use tokio::task::JoinHandle;

static TOKIO_RUNTIME: OnceLock<Runtime> = OnceLock::new();

pub fn tokio_runtime() -> &'static Runtime {
    TOKIO_RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .thread_name("rustduck-tokio")
            .build()
            .expect("Failed to initialize background Tokio runtime")
    })
}

pub fn spawn_tokio<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    tokio_runtime().spawn(future)
}
