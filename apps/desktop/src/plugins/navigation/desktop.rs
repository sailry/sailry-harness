//! The local catalog outlives execution-host selection.
use super::*;
use sailry_link::CancellationToken;

pub(super) struct Desktop {
    pub client: Arc<Client>,
    pub metadata: Entity<Metadata>,
    pub connected: bool,
    stop: CancellationToken,
    _watch: Task<()>,
}

impl Desktop {
    pub fn new(services: crate::backend::Services, cx: &mut Context<Self>) -> Self {
        let client = Arc::new(Client::new(services.local));
        let metadata = cx.new(|_| Metadata::new(client.clone(), services.runtime.clone()));
        cx.observe(&metadata, |_, _, cx| cx.notify()).detach();
        let stop = CancellationToken::new();
        let cancellation = stop.clone();
        let watched = client.clone();
        let (sender, mut receiver) = tokio::sync::watch::channel(sailry_client::View::default());
        services.runtime.spawn(async move {
            let _ = watched.watch(sender, cancellation).await;
        });
        let watch = cx.spawn(async move |desktop, cx| {
            loop {
                let view = receiver.borrow_and_update().clone();
                if desktop
                    .update(cx, |desktop, cx| {
                        desktop.connected = view.connected;
                        if let Some(snapshot) = view.snapshot {
                            desktop
                                .metadata
                                .update(cx, |metadata, cx| metadata.accept(&snapshot.plugins, cx));
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
                if receiver.changed().await.is_err() {
                    break;
                }
            }
        });
        Self {
            client,
            metadata,
            connected: false,
            stop,
            _watch: watch,
        }
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
