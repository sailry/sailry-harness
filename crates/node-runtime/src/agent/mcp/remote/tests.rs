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
    delayed: bool,
    terminal: Arc<AtomicBool>,
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
            if self.cancelled.load(Ordering::SeqCst)
                && (!self.delayed || self.terminal.load(Ordering::SeqCst))
            {
                TaskPayload::Cancelled
            } else {
                TaskPayload::Working
            },
        )))
    }
}

#[tokio::test]
async fn retains_cancelled_task_until_terminal() {
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server = Server {
        delayed: true,
        ..Server::default()
    };
    let state = server.clone();
    let opening = tokio::spawn(async move { server.serve(server_transport).await });
    let service = Arc::new(input::Handler::new())
        .serve(client_transport)
        .await
        .unwrap();
    let remote = Remote::new(service);
    let server = opening.await.unwrap().unwrap();
    let stop = CancellationToken::new();
    let cancelled = stop.clone();
    let active = remote.clone();
    let serial = Arc::new(Mutex::new(()));
    let held = serial.clone().lock_owned().await;
    let running = tokio::spawn(async move {
        let _held = held;
        active
            .task(task().with_poll_interval_ms(1), &cancelled)
            .await
    });
    tokio::task::yield_now().await;
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(2), state.notified.notified())
        .await
        .unwrap();
    assert!(serial.try_lock().is_err());
    assert!(!running.is_finished());
    assert!(remote.tasks.lock().await.contains("abandoned"));
    state.terminal.store(true, Ordering::SeqCst);
    let result = tokio::time::timeout(Duration::from_secs(2), running)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
    assert!(serial.try_lock().is_ok());
    assert!(remote.tasks.lock().await.is_empty());
    assert!(!remote.is_closed());
    drop(server);
}

#[tokio::test]
async fn detects_eof_with_retained_handles() {
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let opening = tokio::spawn(async move { Server::default().serve(server_transport).await });
    let service = Arc::new(input::Handler::new())
        .serve(client_transport)
        .await
        .unwrap();
    let peer = service.peer().clone();
    let remote = Remote::new(service);
    let server = opening.await.unwrap().unwrap();
    assert!(!remote.is_closed());
    drop(server);
    tokio::time::timeout(Duration::from_secs(2), async {
        while !peer.is_transport_closed() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("transport EOF deadline");
    assert!(remote.is_closed());
}

#[tokio::test]
async fn separates_call_failures_from_transport_failure() {
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let opening = tokio::spawn(async move { Server::default().serve(server_transport).await });
    let service = Arc::new(input::Handler::new())
        .serve(client_transport)
        .await
        .unwrap();
    let remote = Remote::new(service);
    let server = opening.await.unwrap().unwrap();
    for error in [
        rmcp::ServiceError::McpError(ErrorData::internal_error("Fixture call failed", None)),
        rmcp::ServiceError::Timeout {
            timeout: Duration::from_secs(120),
        },
        rmcp::ServiceError::Cancelled { reason: None },
    ] {
        let _ = remote.failed(error);
        assert!(!remote.is_closed());
    }
    let _ = remote.failed(rmcp::ServiceError::UnexpectedResponse);
    assert!(remote.is_closed());
    drop(server);
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
    let stop = CancellationToken::new();
    let mut waiting = Box::pin(remote.task(task(), &stop));
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
