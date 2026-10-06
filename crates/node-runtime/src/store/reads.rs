use sailry_link::Admission;
use sailry_protocol::{Command, ErrorCode, Fault, Receipt, Request, VERSION};
use tokio::sync::oneshot;

use super::{Ingress, Job, unavailable};

impl Ingress {
    pub(crate) async fn check_plugin(
        &self,
        caller: sailry_protocol::NodeId,
        request: Request,
    ) -> Result<(), Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(Job::CheckPlugin {
                caller,
                request: Box::new(request),
                reply,
            })
            .map_err(|_| Fault::new(ErrorCode::Busy, "Node request queue is unavailable"))?;
        response.await.map_err(|_| unavailable())?
    }

    pub(super) async fn inspect_worktree(
        &self,
        request: Request,
        internal: bool,
    ) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let root = if matches!(request.command, Command::BrowseFiles { .. }) {
            std::path::PathBuf::new()
        } else {
            let worktree = match &request.command {
                Command::ListDirectory { worktree, .. }
                | Command::ListTerminalTools { worktree }
                | Command::ReadFile { worktree, .. }
                | Command::ReadOffice { worktree, .. }
                | Command::SearchFiles { worktree, .. }
                | Command::InspectGit { worktree }
                | Command::ReadGitStash { worktree, .. }
                | Command::ReadGitOutput { worktree }
                | Command::ListGitRemoteTags { worktree, .. }
                | Command::ListGitBranches { worktree }
                | Command::ResolveGitRevision { worktree, .. }
                | Command::ListWorktrees { worktree }
                | Command::ReadGitLog { worktree, .. }
                | Command::ReadGitCommit { worktree, .. }
                | Command::ReadGitDiff { worktree, .. } => *worktree,
                _ => {
                    return Err(Fault::new(
                        ErrorCode::Internal,
                        "expected worktree inspection command",
                    ));
                }
            };
            self.worktree_root_with(worktree, internal).await?
        };
        let output = if matches!(request.command, Command::ListTerminalTools { .. }) {
            let (reply, response) = oneshot::channel();
            if internal {
                self.send_internal(Job::TerminalSettings { reply }).await?;
            } else {
                self.sender
                    .try_send(Job::TerminalSettings { reply })
                    .map_err(|_| {
                        Fault::new(ErrorCode::Busy, "Node request queue is unavailable")
                    })?;
            }
            let settings = response.await.map_err(|_| unavailable())??;
            let tools =
                tokio::task::spawn_blocking(move || crate::terminal::tools::list(&root, &settings))
                    .await
                    .map_err(|_| unavailable())?;
            sailry_protocol::Output::TerminalTools(tools)
        } else if matches!(
            request.command,
            Command::InspectGit { .. }
                | Command::ReadGitStash { .. }
                | Command::ReadGitOutput { .. }
                | Command::ListGitRemoteTags { .. }
                | Command::ListGitBranches { .. }
                | Command::ResolveGitRevision { .. }
                | Command::ListWorktrees { .. }
                | Command::ReadGitDiff { .. }
                | Command::ReadGitLog { .. }
                | Command::ReadGitCommit { .. }
        ) {
            self.git
                .inspect(root, request.command, self.closed.clone())
                .await?
        } else {
            self.files
                .inspect(root, request.command, self.closed.clone())
                .await?
        };
        if self.closed.is_cancelled() {
            return Err(unavailable());
        }
        let (send, completion) = oneshot::channel();
        let _ = send.send(Ok(output));
        Ok(Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }

    pub(crate) async fn worktree_root(
        &self,
        id: sailry_protocol::WorktreeId,
    ) -> Result<std::path::PathBuf, Fault> {
        self.worktree_root_with(id, false).await
    }

    pub(in crate::store) async fn worktree_root_with(
        &self,
        id: sailry_protocol::WorktreeId,
        internal: bool,
    ) -> Result<std::path::PathBuf, Fault> {
        if self.closed.is_cancelled() {
            return Err(unavailable());
        }
        let (reply, response) = oneshot::channel();
        if internal {
            self.send_internal(Job::WorktreeRoot { id, reply }).await?;
        } else {
            self.sender
                .try_send(Job::WorktreeRoot { id, reply })
                .map_err(|_| Fault::new(ErrorCode::Busy, "Node request queue is unavailable"))?;
        }
        response.await.map_err(|_| unavailable())?
    }
}
