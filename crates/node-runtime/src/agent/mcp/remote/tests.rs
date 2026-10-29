use super::*;
use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    model::{DetailedTask, GetTaskResult, ServerCapabilities, ServerConfig, TaskStatus},
};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;

#[derive(Clone, Default)]
struct Server {
    cancelled: Arc<AtomicBool>,
    notified: Arc<Notify>,
}

fn task() -> Task {
    Task::new(
        "abandoned",
        TaskStatus::Working,
        "2026-09-11T00:00:00Z",
        "2026-09-11T00:00:00Z",
    )
    .with_poll_interval_ms(60_000)
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tasks().build())
    }

    async fn cancel_task(
        &self,
        request: CancelTaskParams,
        _: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        assert_eq!(request.task_id, "abandoned");
        self.cancelled.store(true, Ordering::SeqCst);
        self.notified.notify_one();
        Ok(())
    }

    async fn get_task(
        &self,
        request: GetTaskParams,
        _: RequestContext<RoleServer>,
    ) -> Result<GetTaskResult, ErrorData> {
        assert_eq!(request.task_id, "abandoned");
        Ok(GetTaskResult::new(DetailedTask::new(
            task(),
            if self.cancelled.load(Ordering::SeqCst) {
                TaskPayload::Cancelled
            } else {
                TaskPayload::Working
            },
        )))
    }
}

#[tokio::test]
async fn cancels_abandoned_task_before_cleanup() {
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server = Server::default();
    let state = server.clone();
    let running = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap();
    });
    let service = Arc::new(input::Handler::new())
        .serve(client_transport)
        .await
        .unwrap();
    let remote = Remote::new(service);
    let mut waiting = Box::pin(remote.task(task()));
    assert!(futures::poll!(waiting.as_mut()).is_pending());
    assert!(remote.tasks.lock().await.contains("abandoned"));
    assert!(!state.cancelled.load(Ordering::SeqCst));

    drop(waiting);
    tokio::time::timeout(Duration::from_secs(2), state.notified.notified())
        .await
        .expect("abandoning a call must cancel its task while the connection is open");
    assert!(!remote.is_closed());
    assert!(remote.tasks.lock().await.contains("abandoned"));

    remote.cancel_pending().await.unwrap();
    assert!(remote.tasks.lock().await.is_empty());
    assert!(!remote.is_closed());
    drop(remote);
    running.abort();
}
