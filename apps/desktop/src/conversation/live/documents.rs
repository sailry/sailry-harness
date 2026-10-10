//! Form answers enter the ordinary durable submit path without touching drafts.
use super::*;

impl View {
    pub(crate) fn document_visible(&self, artifact: &sailry_protocol::artifact::Summary) -> bool {
        let Some(snapshot) = &self.history.snapshot else {
            return false;
        };
        snapshot.page.runs.iter().any(|run| {
            run.turn == artifact.turn
                && Some(run.session) == self.session()
                && run.origin.unwrap_or(run.session) == artifact.session
        }) && self.history.calls.iter().any(|call| {
            call.turn == artifact.turn
                && call.content.as_ref().is_some_and(|content| {
                    content.blocks.iter().any(|block| {
                        matches!(block, sailry_protocol::tool::Block::Artifact(value) if value == artifact)
                    })
                })
        })
    }

    pub(crate) fn document_writable(&self, session: SessionId) -> bool {
        self.session() == Some(session)
            && !self.readonly()
            && self.connected()
            && !self.busy()
            && self.configured()
            && self.history.snapshot.is_some()
    }

    pub(crate) fn send_document(
        &mut self,
        session: SessionId,
        text: String,
        status: tokio::sync::watch::Sender<Option<bool>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.document_writable(session) || text.trim().is_empty() || text.len() > 32 * 1024 {
            return false;
        }
        let revision = self.session.as_ref().expect("captured session").revision;
        let input = sailry_protocol::conversation::Input {
            text,
            attachments: Vec::new(),
            references: Vec::new(),
        };
        self.show_outgoing(input.clone(), cx);
        if let Some(outgoing) = &mut self.outgoing {
            outgoing.attachments.clear();
        }
        self.execute(
            Command::SubmitTurn {
                session,
                expected_revision: revision,
                message: input.clone(),
            },
            actions::Action::Document(status),
            window,
            cx,
        );
        true
    }
}
