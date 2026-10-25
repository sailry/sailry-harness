//! Code structure reads use the same command boundary on every transport.
use sailry_protocol::{Command, Fault, Output, WorktreeId, code_index};

use crate::Client;
use sailry_link::Transport;
use sailry_protocol::Request;
use std::sync::Arc;

pub(crate) struct Pending {
    transport: Arc<dyn Transport>,
    cancellation: Option<Request>,
}

impl Pending {
    pub(crate) fn new(transport: Arc<dyn Transport>, request: &Request) -> Self {
        let cancellation = matches!(
            request.command,
            Command::SearchCode { .. } | Command::OutlineCode { .. }
        )
        .then(|| {
            let mut cancel = Request::new(
                request.target,
                Command::CancelCodeIndexQuery {
                    request: request.id,
                },
            );
            cancel.plugin = request.plugin.clone();
            cancel
        });
        Self {
            transport,
            cancellation,
        }
    }

    pub(crate) fn complete(&mut self) {
        self.cancellation = None;
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(request) = self.cancellation.take()
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            let transport = self.transport.clone();
            runtime.spawn(async move {
                let _ = crate::execute(&*transport, request).await;
            });
        }
    }
}

impl Client {
    pub async fn search_code(
        &self,
        worktree: WorktreeId,
        query: code_index::Search,
    ) -> Result<code_index::SearchPage, Fault> {
        match self
            .execute(self.prepare(Command::SearchCode { worktree, query }))
            .await?
        {
            Output::CodeSearch(page) => Ok(page),
            _ => Err(Fault::invalid("invalid code search response")),
        }
    }

    pub async fn outline_code(
        &self,
        worktree: WorktreeId,
        path: &str,
    ) -> Result<code_index::Outline, Fault> {
        match self
            .execute(self.prepare(Command::OutlineCode {
                worktree,
                path: path.into(),
            }))
            .await?
        {
            Output::CodeOutline(outline) if outline.path == path => Ok(outline),
            _ => Err(Fault::invalid("invalid code outline response")),
        }
    }
}

#[cfg(test)]
mod tests;
