use crate::models::{TransferDirection, TransferStatus, TransferTask};
use crate::services::runtime::tokio_runtime;
use crate::storage::StorageBackend;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

struct ManagedTransfer {
    task: TransferTask,
    source_backend: Arc<dyn StorageBackend>,
    dest_backend: Arc<dyn StorageBackend>,
    cancel_token: CancellationToken,
}

#[derive(Clone)]
pub struct TransferManager {
    inner: Arc<RwLock<Vec<ManagedTransfer>>>,
    semaphore: Arc<Semaphore>,
}

impl TransferManager {
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Vec::new())),
            semaphore: Arc::new(Semaphore::new(max_concurrent.max(1))),
        }
    }

    pub fn enqueue(
        &self,
        direction: TransferDirection,
        source_backend: Arc<dyn StorageBackend>,
        source_path: String,
        dest_backend: Arc<dyn StorageBackend>,
        dest_path: String,
        file_name: String,
        total_bytes: u64,
    ) -> Uuid {
        let id = Uuid::new_v4();
        let cancel_token = CancellationToken::new();
        let task = TransferTask {
            id,
            file_name,
            source_path: source_path.clone(),
            dest_path: dest_path.clone(),
            direction,
            total_bytes,
            transferred_bytes: 0,
            speed_bytes_per_sec: 0,
            status: TransferStatus::Queued,
        };

        {
            let mut guard = self.inner.write().expect("transfer lock poisoned");
            guard.push(ManagedTransfer {
                task,
                source_backend: source_backend.clone(),
                dest_backend: dest_backend.clone(),
                cancel_token: cancel_token.clone(),
            });
        }

        self.spawn_worker(
            id,
            direction,
            source_backend,
            source_path,
            dest_backend,
            dest_path,
            total_bytes,
            cancel_token,
        );
        id
    }

    fn spawn_worker(
        &self,
        id: Uuid,
        direction: TransferDirection,
        source_backend: Arc<dyn StorageBackend>,
        source_path: String,
        dest_backend: Arc<dyn StorageBackend>,
        dest_path: String,
        total_bytes: u64,
        cancel_token: CancellationToken,
    ) {
        let inner = self.inner.clone();
        let sem = self.semaphore.clone();
        let handle = tokio::runtime::Handle::try_current()
            .unwrap_or_else(|_| tokio_runtime().handle().clone());

        handle.spawn(async move {
            let _permit = match sem.acquire_owned().await {
                Ok(p) => p,
                Err(_) => return,
            };

            if cancel_token.is_cancelled() {
                return;
            }

            {
                let mut guard = inner.write().expect("transfer lock poisoned");
                if let Some(item) = guard.iter_mut().find(|m| m.task.id == id) {
                    if matches!(item.task.status, TransferStatus::Paused | TransferStatus::Cancelled)
                    {
                        return;
                    }
                    item.task.status = TransferStatus::InProgress;
                    item.task.transferred_bytes = 0;
                    item.task.speed_bytes_per_sec = 0;
                }
            }

            let started = Instant::now();
            let progress_inner = inner.clone();
            let progress_cb: Arc<dyn Fn(u64) + Send + Sync> = Arc::new(move |bytes| {
                let elapsed = started.elapsed().as_secs_f64().max(0.001);
                let speed = (bytes as f64 / elapsed) as u64;
                if let Ok(mut guard) = progress_inner.write() {
                    if let Some(item) = guard.iter_mut().find(|m| m.task.id == id) {
                        item.task.transferred_bytes = bytes;
                        item.task.speed_bytes_per_sec = speed;
                    }
                }
            });

            let (mut reader, mut writer) = tokio::io::duplex(64 * 1024);
            let dl_cancel = cancel_token.clone();
            let ul_cancel = cancel_token.clone();
            let noop_progress: Arc<dyn Fn(u64) + Send + Sync> = Arc::new(|_| {});
            let (dl_progress, ul_progress) = match direction {
                TransferDirection::Download => (progress_cb.clone(), noop_progress),
                TransferDirection::Upload => (noop_progress, progress_cb.clone()),
            };

            let src_fut = async {
                let res = source_backend
                    .download_stream(&source_path, &mut writer, dl_progress, dl_cancel)
                    .await;
                drop(writer);
                res
            };

            let dst_fut = dest_backend.upload_stream(
                &mut reader,
                Some(total_bytes),
                &dest_path,
                ul_progress,
                ul_cancel,
            );

            let res = tokio::select! {
                biased;
                _ = cancel_token.cancelled() => Err(anyhow::anyhow!("Transfer cancelled")),
                r = async { tokio::try_join!(src_fut, dst_fut) } => r,
            };

            let mut guard = inner.write().expect("transfer lock poisoned");
            if let Some(item) = guard.iter_mut().find(|m| m.task.id == id) {
                if matches!(
                    item.task.status,
                    TransferStatus::Paused | TransferStatus::Cancelled
                ) {
                    item.task.speed_bytes_per_sec = 0;
                } else if cancel_token.is_cancelled() {
                    item.task.status = TransferStatus::Cancelled;
                    item.task.speed_bytes_per_sec = 0;
                } else {
                    match res {
                        Ok(_) => {
                            item.task.status = TransferStatus::Completed;
                            item.task.speed_bytes_per_sec = 0;
                            if item.task.total_bytes > 0 {
                                item.task.transferred_bytes = item.task.total_bytes;
                            }
                        }
                        Err(e) => {
                            item.task.status = TransferStatus::Failed(e.to_string());
                            item.task.speed_bytes_per_sec = 0;
                        }
                    }
                }
            }
        });
    }

    pub fn pause(&self, task_id: Uuid) {
        let mut guard = self.inner.write().expect("transfer lock poisoned");
        if let Some(item) = guard.iter_mut().find(|m| m.task.id == task_id) {
            if matches!(
                item.task.status,
                TransferStatus::InProgress | TransferStatus::Queued
            ) {
                item.task.status = TransferStatus::Paused;
                item.task.speed_bytes_per_sec = 0;
                item.cancel_token.cancel();
            }
        }
    }

    pub fn resume(&self, task_id: Uuid) {
        let params = {
            let mut guard = self.inner.write().expect("transfer lock poisoned");
            if let Some(item) = guard.iter_mut().find(|m| m.task.id == task_id) {
                if matches!(
                    item.task.status,
                    TransferStatus::Paused | TransferStatus::Failed(_)
                ) {
                    let new_token = CancellationToken::new();
                    item.cancel_token = new_token.clone();
                    item.task.status = TransferStatus::Queued;
                    item.task.transferred_bytes = 0;
                    Some((
                        item.task.direction,
                        item.source_backend.clone(),
                        item.task.source_path.clone(),
                        item.dest_backend.clone(),
                        item.task.dest_path.clone(),
                        item.task.total_bytes,
                        new_token,
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some((dir, src, src_p, dst, dst_p, total, token)) = params {
            self.spawn_worker(task_id, dir, src, src_p, dst, dst_p, total, token);
        }
    }

    pub fn cancel(&self, task_id: Uuid) {
        let mut guard = self.inner.write().expect("transfer lock poisoned");
        if let Some(item) = guard.iter_mut().find(|m| m.task.id == task_id) {
            item.task.status = TransferStatus::Cancelled;
            item.task.speed_bytes_per_sec = 0;
            item.cancel_token.cancel();
        }
    }

    pub fn clear_completed(&self) {
        let mut guard = self.inner.write().expect("transfer lock poisoned");
        guard.retain(|m| {
            !matches!(
                m.task.status,
                TransferStatus::Completed | TransferStatus::Cancelled
            )
        });
    }

    pub fn snapshot(&self) -> Vec<TransferTask> {
        let guard = self.inner.read().expect("transfer lock poisoned");
        guard.iter().map(|m| m.task.clone()).collect()
    }

    pub async fn enqueue_directory_recursive(
        &self,
        direction: TransferDirection,
        source_backend: Arc<dyn StorageBackend>,
        source_dir: String,
        source_is_local: bool,
        dest_backend: Arc<dyn StorageBackend>,
        dest_dir: String,
        dest_is_local: bool,
    ) -> anyhow::Result<usize> {
        let mut stack = vec![(source_dir, dest_dir)];
        let mut enqueued_files = 0usize;

        while let Some((cur_src, cur_dst)) = stack.pop() {
            dest_backend.create_dir(&cur_dst).await?;
            let entries = source_backend.list_dir(&cur_src).await?;
            for entry in entries {
                let child_src = entry.path;
                let child_dst = crate::ui::file_pane::join_local_or_remote(
                    &cur_dst,
                    &entry.name,
                    dest_is_local,
                );
                let _ = source_is_local;
                if entry.kind.is_dir_like() {
                    stack.push((child_src, child_dst));
                } else {
                    self.enqueue(
                        direction,
                        source_backend.clone(),
                        child_src,
                        dest_backend.clone(),
                        child_dst,
                        entry.name,
                        entry.size,
                    );
                    enqueued_files += 1;
                }
            }
        }
        Ok(enqueued_files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{TransferDirection, TransferStatus};
    use crate::storage::local::LocalFileSystem;

    #[tokio::test]
    async fn test_transfer_manager_completes_file_copy_and_tracks_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("source.bin");
        let dst_path = dir.path().join("dest.bin");
        let payload = vec![7u8; 256 * 1024];
        std::fs::write(&src_path, &payload).unwrap();

        let fs: Arc<dyn StorageBackend> = Arc::new(LocalFileSystem::new());
        let mgr = TransferManager::new(2);
        let id = mgr.enqueue(
            TransferDirection::Upload,
            fs.clone(),
            src_path.to_string_lossy().to_string(),
            fs.clone(),
            dst_path.to_string_lossy().to_string(),
            "source.bin".to_string(),
            payload.len() as u64,
        );

        for _ in 0..50 {
            let tasks = mgr.snapshot();
            if tasks[0].status == TransferStatus::Completed {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        let tasks = mgr.snapshot();
        assert_eq!(tasks[0].id, id);
        assert_eq!(tasks[0].status, TransferStatus::Completed);
        assert_eq!(tasks[0].transferred_bytes, payload.len() as u64);
        assert_eq!(std::fs::read(&dst_path).unwrap(), payload);
    }

    #[tokio::test]
    async fn test_transfer_manager_pause_and_resume_completes() {
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("resume_src.bin");
        let dst_path = dir.path().join("resume_dst.bin");
        let payload = vec![9u8; 64 * 1024];
        std::fs::write(&src_path, &payload).unwrap();

        let fs: Arc<dyn StorageBackend> = Arc::new(LocalFileSystem::new());
        let mgr = TransferManager::new(1);
        let id = mgr.enqueue(
            TransferDirection::Download,
            fs.clone(),
            src_path.to_string_lossy().to_string(),
            fs.clone(),
            dst_path.to_string_lossy().to_string(),
            "resume_src.bin".to_string(),
            payload.len() as u64,
        );

        mgr.pause(id);
        assert_eq!(mgr.snapshot()[0].status, TransferStatus::Paused);

        mgr.resume(id);
        for _ in 0..50 {
            if mgr.snapshot()[0].status == TransferStatus::Completed {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(mgr.snapshot()[0].status, TransferStatus::Completed);
    }

    struct SlowCompleteBackend;

    #[async_trait::async_trait]
    impl StorageBackend for SlowCompleteBackend {
        fn protocol_name(&self) -> &'static str {
            "SlowMock"
        }
        async fn connect_and_verify(&self) -> anyhow::Result<String> {
            Ok("/".to_string())
        }
        async fn list_dir(&self, _path: &str) -> anyhow::Result<Vec<crate::models::FileEntry>> {
            Ok(vec![])
        }
        async fn create_dir(&self, _path: &str) -> anyhow::Result<()> {
            Ok(())
        }
        async fn rename(&self, _from: &str, _to: &str) -> anyhow::Result<()> {
            Ok(())
        }
        async fn remove(&self, _path: &str, _is_dir: bool) -> anyhow::Result<()> {
            Ok(())
        }
        async fn download_stream(
            &self,
            _remote_path: &str,
            writer: &mut (dyn tokio::io::AsyncWrite + Unpin + Send),
            _progress: crate::storage::ProgressCallback,
            _cancel: CancellationToken,
        ) -> anyhow::Result<()> {
            use tokio::io::AsyncWriteExt;
            writer.write_all(&vec![1u8; 1024]).await?;
            writer.flush().await?;
            Ok(())
        }
        async fn upload_stream(
            &self,
            reader: &mut (dyn tokio::io::AsyncRead + Unpin + Send),
            _total_size: Option<u64>,
            _remote_path: &str,
            progress: crate::storage::ProgressCallback,
            _cancel: CancellationToken,
        ) -> anyhow::Result<()> {
            use tokio::io::AsyncReadExt;
            let mut buf = vec![0u8; 2048];
            let n = reader.read(&mut buf).await?;
            progress(n as u64);
            // Simulate an in-flight network flush where pause/cancel arrives before completion
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_transfer_manager_in_flight_pause_and_cancel_never_complete() {
        let mock: Arc<dyn StorageBackend> = Arc::new(SlowCompleteBackend);
        let mgr = TransferManager::new(2);

        let pause_id = mgr.enqueue(
            TransferDirection::Upload,
            mock.clone(),
            "/src_a".to_string(),
            mock.clone(),
            "/dst_a".to_string(),
            "a.bin".to_string(),
            1024,
        );
        let cancel_id = mgr.enqueue(
            TransferDirection::Upload,
            mock.clone(),
            "/src_b".to_string(),
            mock.clone(),
            "/dst_b".to_string(),
            "b.bin".to_string(),
            1024,
        );

        // Wait until both enter InProgress and report upload progress
        for _ in 0..30 {
            let snap = mgr.snapshot();
            if snap.iter().all(|t| t.status == TransferStatus::InProgress && t.transferred_bytes == 1024) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let mid_snap = mgr.snapshot();
        assert_eq!(mid_snap[0].transferred_bytes, 1024);

        mgr.pause(pause_id);
        mgr.cancel(cancel_id);

        // Wait past the 120ms mock completion window
        tokio::time::sleep(std::time::Duration::from_millis(180)).await;

        let final_snap = mgr.snapshot();
        let paused_task = final_snap.iter().find(|t| t.id == pause_id).unwrap();
        let cancelled_task = final_snap.iter().find(|t| t.id == cancel_id).unwrap();
        assert_eq!(paused_task.status, TransferStatus::Paused);
        assert_eq!(cancelled_task.status, TransferStatus::Cancelled);
    }

    #[tokio::test]
    async fn test_transfer_manager_recursive_directory_transfer() {
        let src_dir = tempfile::tempdir().unwrap();
        let dst_dir = tempfile::tempdir().unwrap();

        let nested_src = src_dir.path().join("project").join("sub");
        std::fs::create_dir_all(&nested_src).unwrap();
        std::fs::write(src_dir.path().join("project").join("readme.txt"), b"hello").unwrap();
        std::fs::write(nested_src.join("deep.bin"), b"world123").unwrap();

        let fs: Arc<dyn StorageBackend> = Arc::new(LocalFileSystem::new());
        let mgr = TransferManager::new(2);

        let src_root = src_dir.path().join("project").to_string_lossy().to_string();
        let dst_root = dst_dir.path().join("project").to_string_lossy().to_string();

        let count = mgr
            .enqueue_directory_recursive(
                TransferDirection::Upload,
                fs.clone(),
                src_root,
                true,
                fs.clone(),
                dst_root.clone(),
                true,
            )
            .await
            .unwrap();
        assert_eq!(count, 2);

        for _ in 0..50 {
            let snap = mgr.snapshot();
            if snap.len() == 2 && snap.iter().all(|t| t.status == TransferStatus::Completed) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        assert_eq!(
            std::fs::read(dst_dir.path().join("project").join("readme.txt")).unwrap(),
            b"hello"
        );
        assert_eq!(
            std::fs::read(dst_dir.path().join("project").join("sub").join("deep.bin")).unwrap(),
            b"world123"
        );
    }
}
