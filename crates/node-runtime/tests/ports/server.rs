use sailry_link::CancellationToken;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    task::{JoinHandle, JoinSet},
};

pub struct Server {
    pub port: u16,
    active: Arc<AtomicUsize>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}
struct Active(Arc<AtomicUsize>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
impl Server {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let active = Arc::new(AtomicUsize::new(0));
        let count = active.clone();
        let stop = CancellationToken::new();
        let cancellation = stop.clone();
        let task = tokio::spawn(async move {
            let mut tasks = JoinSet::new();
            loop {
                tokio::select! {
                    _ = cancellation.cancelled() => break,
                    _ = tasks.join_next(), if !tasks.is_empty() => {},
                    accepted = listener.accept() => {
                        let (mut socket, _) = accepted.unwrap();
                        count.fetch_add(1, Ordering::SeqCst);
                        let active = Active(count.clone());
                        tasks.spawn(async move {
                            let _active = active;
                            let mut bytes = Vec::new();
                            if socket.read_to_end(&mut bytes).await.is_ok() {
                                let _ = socket.write_all(&bytes).await;
                                let _ = socket.shutdown().await;
                            }
                        });
                    }
                }
            }
            tasks.shutdown().await;
        });
        Self {
            port,
            active,
            stop,
            task,
        }
    }
    pub async fn wait_active(&self) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while self.active.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }
    pub async fn wait_idle(&self) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while self.active.load(Ordering::SeqCst) != 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }
    pub async fn close(self) {
        self.stop.cancel();
        self.task.await.unwrap();
    }
}
