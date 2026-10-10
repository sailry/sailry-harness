//! Artifact reads use the same captured Node boundary as conversations.
use crate::Client;
use sailry_protocol::{ArtifactId, Command, ErrorCode, Fault, Output, SessionId, artifact};

impl Client {
    pub async fn read_artifact(
        &self,
        id: ArtifactId,
        revision: Option<u32>,
    ) -> Result<artifact::Content, Fault> {
        self.artifact_content(None, id, revision).await
    }

    pub async fn read_artifact_in(
        &self,
        session: SessionId,
        id: ArtifactId,
        revision: Option<u32>,
    ) -> Result<artifact::Content, Fault> {
        self.artifact_content(Some(session), id, revision).await
    }

    async fn artifact_content(
        &self,
        session: Option<SessionId>,
        id: ArtifactId,
        revision: Option<u32>,
    ) -> Result<artifact::Content, Fault> {
        match self
            .execute(self.prepare(Command::ReadArtifact {
                id,
                revision,
                session,
            }))
            .await?
        {
            Output::Artifact(value)
                if value.artifact.id == id
                    && revision.is_none_or(|revision| value.artifact.revision == revision)
                    && value.artifact.valid()
                    && value.latest_revision >= value.artifact.revision
                    && value.html.len() as u64 == value.artifact.size
                    && value.view.as_ref().is_none_or(artifact::ViewContent::valid) =>
            {
                Ok(value)
            }
            _ => Err(invalid()),
        }
    }

    pub async fn list_artifacts(
        &self,
        session: SessionId,
        after: Option<ArtifactId>,
        limit: u16,
    ) -> Result<artifact::Page, Fault> {
        match self
            .execute(self.prepare(Command::ListArtifacts {
                session,
                after,
                limit,
            }))
            .await?
        {
            Output::Artifacts(value)
                if value.items.len() <= usize::from(limit)
                    && value
                        .items
                        .iter()
                        .all(|artifact| artifact.session == session && artifact.valid())
                    && value.next_after.is_none_or(|id| {
                        value.items.last().is_some_and(|artifact| artifact.id == id)
                    }) =>
            {
                Ok(value)
            }
            _ => Err(invalid()),
        }
    }

    pub async fn list_artifact_revisions(
        &self,
        id: ArtifactId,
        before: Option<u32>,
        limit: u16,
    ) -> Result<artifact::RevisionPage, Fault> {
        self.artifact_revisions(None, id, before, limit).await
    }

    pub async fn list_artifact_revisions_in(
        &self,
        session: SessionId,
        id: ArtifactId,
        before: Option<u32>,
        limit: u16,
    ) -> Result<artifact::RevisionPage, Fault> {
        self.artifact_revisions(Some(session), id, before, limit)
            .await
    }

    async fn artifact_revisions(
        &self,
        session: Option<SessionId>,
        id: ArtifactId,
        before: Option<u32>,
        limit: u16,
    ) -> Result<artifact::RevisionPage, Fault> {
        match self
            .execute(self.prepare(Command::ListArtifactRevisions {
                id,
                before,
                limit,
                session,
            }))
            .await?
        {
            Output::ArtifactRevisions(value)
                if value.items.len() <= usize::from(limit)
                    && value.items.iter().all(|revision| {
                        revision.id == id
                            && revision.revision > 0
                            && before.is_none_or(|before| revision.revision < before)
                    })
                    && value.next_before.is_none_or(|number| {
                        value
                            .items
                            .last()
                            .is_some_and(|revision| revision.revision == number)
                    }) =>
            {
                Ok(value)
            }
            _ => Err(invalid()),
        }
    }
}

fn invalid() -> Fault {
    Fault::new(ErrorCode::Internal, "invalid artifact response")
}
