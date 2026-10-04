//! Discovered-service mappings share their process lifetime on every controller.
use super::*;
use sailry_protocol::{
    RequestId, SessionId,
    process::{Service, Status},
};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Source {
    pub session: SessionId,
    pub command: RequestId,
    pub service: Service,
}
impl Source {
    fn present(&self, items: &[sailry_protocol::process::Info]) -> bool {
        items.iter().any(|info| {
            info.id == self.command
                && info.session == self.session
                && matches!(info.status, Status::Running | Status::Stopping)
                && info.services.contains(&self.service)
        })
    }
}
impl Client {
    pub async fn forward_service(
        &self,
        source: Source,
        local_port: u16,
    ) -> Result<Forwarder, Fault> {
        let Output::Commands(items) = self
            .execute(self.prepare(Command::ListCommands {
                session: source.session,
            }))
            .await?
        else {
            return Err(Fault::new(ErrorCode::Internal, "command list expected"));
        };
        if !source.present(&items) {
            return Err(Fault::new(
                ErrorCode::NotFound,
                "service is no longer available",
            ));
        }
        let mut forwarder = self.forward_port(source.service.port, local_port).await?;
        let stop = forwarder.stop.clone();
        let client = Client::new(self.transport.clone());
        forwarder.service_watch = Some(tokio::spawn(async move {
            let (_selection, selected) = watch::channel(None);
            let (updates, mut receiver) = watch::channel(crate::commands::View::default());
            let observe = client.watch_commands(source.session, selected, updates, stop.clone());
            tokio::pin!(observe);
            loop {
                tokio::select! {
                    biased;
                    _ = stop.cancelled() => break,
                    _ = &mut observe => { stop.cancel(); break; },
                    changed = receiver.changed() => {
                        if changed.is_err() { stop.cancel(); break; }
                        let view = receiver.borrow_and_update();
                        if view.connected && !source.present(&view.items) { stop.cancel(); break; }
                    }
                }
            }
        }));
        Ok(forwarder)
    }
}
