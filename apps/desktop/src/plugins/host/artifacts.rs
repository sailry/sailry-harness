//! Artifact invalidation shares the Host's authoritative conversation projection.
use super::*;
use sailry_protocol::{ArtifactId, Output};
use tokio::sync::{mpsc, watch};

impl Host {
    pub(in crate::plugins) fn read_artifact(
        self: &Arc<Self>,
        id: ArtifactId,
        revision: Option<u32>,
        session: sailry_protocol::SessionId,
    ) -> Result<
        impl Future<Output = Result<sailry_protocol::artifact::Content, HostError>>
        + Send
        + 'static
        + use<>,
        HostError,
    > {
        self.check()?;
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| HostError::new("plugin request capacity exhausted"))?;
        let owner = self.clone();
        let request = self
            .client
            .prepare(Command::ReadArtifact {
                id,
                revision,
                session: self.context.session,
            })
            .with_plugin(self.context());
        Ok(async move {
            let runtime = owner.runtime.clone();
            runtime.spawn(async move {
                let _permit = permit;
                let output = tokio::select! {
                    biased;
                    _ = owner.stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                    result = owner.client.execute(request) => result.map_err(sdk::values::fault)?,
                };
                match output {
                    Output::Artifact(content) if content.artifact.id == id
                        && content.artifact.session == session
                        && revision.is_none_or(|revision| revision == content.artifact.revision) => Ok(content),
                    _ => Err(HostError::new("invalid artifact response")),
                }
            }).await.map_err(|_| HostError::new("artifact worker failed"))?
        })
    }

    pub(in crate::plugins) fn artifact_updates(
        self: &Arc<Self>,
    ) -> Result<watch::Receiver<sailry_client::conversation::View>, HostError> {
        self.check()?;
        let session = self
            .context
            .session
            .ok_or_else(|| HostError::new("plugin has no session scope"))?;
        Ok(self
            .conversation
            .receiver
            .get_or_init(|| {
                let (sender, receiver) = watch::channel(Default::default());
                let (_, requests) = mpsc::channel(1);
                let client = self.client.clone();
                let stop = self.stop.clone();
                self.runtime.spawn(async move {
                    let _ = client
                        .watch_conversation(session, sender, stop, requests)
                        .await;
                });
                receiver
            })
            .clone())
    }

    pub(in crate::plugins) fn artifact_revisions(
        self: &Arc<Self>,
        id: ArtifactId,
        before: Option<u32>,
        limit: u16,
    ) -> Result<
        impl Future<Output = Result<HostValue, HostError>> + Send + 'static + use<>,
        HostError,
    > {
        self.read_public(Command::ListArtifactRevisions {
            id,
            before,
            limit,
            session: self.context.session,
        })
    }
}
