use sailry_link::Admission;
use sailry_protocol::{Command, ErrorCode, Fault, Receipt, Request, VERSION};
use tokio::sync::{mpsc, oneshot};

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
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => {
                    Fault::new(ErrorCode::Busy, "Node request queue is full")
                }
                mpsc::error::TrySendError::Closed(_) => unavailable(),
            })?;
        response.await.map_err(|_| unavailable())?
    }

    pub(super) async fn inspect_worktree(
        &self,
        request: Request,
        internal: bool,
    ) -> Result<Admission, Fault> {
        let access = crate::files::Access::request(&request, internal);
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
                | Command::ReadFileLines { worktree, .. }
                | Command::FindFiles { worktree, .. }
                | Command::ReadOffice { worktree, .. }
                | Command::ReadNotebook { worktree, .. }
                | Command::SearchFiles { worktree, .. }
                | Command::InspectGit { worktree }
                | Command::ReadGitStash { worktree, .. }
                | Command::ReadGitOutput { worktree }
                | Command::ListGitRemoteTags { worktree, .. }
                | Command::ListGitBranches { worktree }
                | Command::ResolveGitRevision { worktree, .. }
                | Command::ListWorktrees { worktree }
                | Command::ReadGitLog { worktree, .. }
                | Command::ReadGitShow { worktree, .. }
                | Command::ReadGitBlame { worktree, .. }
                | Command::ListGitChanges { worktree, .. }
                | Command::ReadGitChange { worktree, .. }
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
        let output = if let Command::ReadOffice {
            worktree,
            options:
                sailry_protocol::office::Read {
                    source: sailry_protocol::office::Source::Attachment(id),
                    offset,
                },
        } = &request.command
        {
            let turn = request
                .plugin
                .as_ref()
                .and_then(|context| context.turn)
                .ok_or_else(|| {
                    Fault::new(
                        ErrorCode::PermissionDenied,
                        "attachment inspection requires a captured turn",
                    )
                })?;
            let attachment = self.attachment(turn, *id).await?;
            if attachment.spec.worktree != *worktree {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "attachment belongs to another worktree",
                ));
            }
            if attachment.spec.size > crate::office::LIMIT as u64 {
                return Err(Fault::invalid("Office: file exceeds 32 MiB"));
            }
            let stop = self
                .agents
                .python_stop(turn)
                .ok_or_else(|| Fault::new(ErrorCode::Conflict, "turn is not running"))?;
            let name = attachment.spec.name.clone();
            let bytes = self.attachment_bytes(attachment, stop.clone()).await?;
            sailry_protocol::Output::OfficeContent(
                self.files
                    .inspect_document(name, bytes, *offset, stop)
                    .await?,
            )
        } else if matches!(request.command, Command::ListTerminalTools { .. }) {
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
            let environment = self.environment.clone();
            let tools = tokio::task::spawn_blocking(move || {
                crate::terminal::tools::list(&root, &settings, &environment)
            })
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
                | Command::ReadGitShow { .. }
                | Command::ReadGitBlame { .. }
                | Command::ListGitChanges { .. }
                | Command::ReadGitChange { .. }
                | Command::ReadGitCommit { .. }
        ) {
            self.git
                .inspect(root, request.command, self.closed.clone())
                .await?
        } else {
            self.files
                .inspect_with(root, request.command, self.closed.clone(), access)
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
