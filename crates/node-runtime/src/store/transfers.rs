use super::{Ingress, unavailable};
use sailry_link::Admission;
use sailry_protocol::{Command, ErrorCode, Fault, NodeId, Output, Receipt, Request, VERSION};
use tokio::sync::oneshot;

impl Ingress {
    pub(super) async fn transfer(
        &self,
        caller: NodeId,
        request: Request,
        internal: bool,
    ) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let output = match request.command {
            Command::StageSshUpload(spec) => {
                let admission = self
                    .dispatch_request(caller, Request::new(self.node, Command::ListSsh), internal)
                    .await?;
                let Output::SshProfiles(profiles) =
                    admission.completion.await.map_err(|_| unavailable())??
                else {
                    return Err(unavailable());
                };
                let profile = profiles
                    .iter()
                    .find(|profile| profile.id == spec.profile)
                    .ok_or_else(|| Fault::new(ErrorCode::NotFound, "SSH connection not found"))?;
                super::commands::check_revision(profile.revision, spec.expected_revision)?;
                Output::SshUpload(
                    self.transfers
                        .upload_ssh(caller, spec, self.closed.clone())?,
                )
            }
            Command::UploadPlugin(spec) => Output::PluginUpload(
                self.transfers
                    .upload_plugin(caller, spec, self.closed.clone())
                    .await?,
            ),
            Command::InspectPluginUpload { stream } => Output::Plugin(
                self.transfers
                    .inspect_plugin(caller, stream, self.plugins.clone(), self.closed.clone())
                    .await?,
            ),
            Command::UploadAttachment(spec) => {
                let root = self.worktree_root_with(spec.worktree, internal).await?;
                let prepared = self
                    .transfers
                    .upload_attachment(caller, root.clone(), spec, self.closed.clone())
                    .await?;
                if self
                    .worktree_root_with(prepared.spec.worktree, internal)
                    .await
                    .as_ref()
                    != Ok(&root)
                {
                    let _ = self.transfers.cancel(caller, prepared.stream);
                    return Err(unavailable());
                }
                Output::AttachmentUpload(prepared)
            }
            Command::DownloadAttachment {
                worktree,
                attachment,
            } => {
                let root = self.worktree_root_with(worktree, internal).await?;
                let read = Request::new(
                    self.node,
                    Command::ReadAttachment {
                        worktree,
                        attachment,
                    },
                );
                let admission = self
                    .dispatch_request(caller, read.clone(), internal)
                    .await?;
                let Output::Attachment(info) =
                    admission.completion.await.map_err(|_| unavailable())??
                else {
                    return Err(unavailable());
                };
                let prepared = self
                    .transfers
                    .download_attachment(caller, root.clone(), info.clone(), self.closed.clone())
                    .await?;
                let current = self.dispatch_request(caller, read, internal).await;
                let valid = match current {
                    Ok(admission) => {
                        matches!(admission.completion.await, Ok(Ok(Output::Attachment(current))) if current == info)
                    }
                    Err(_) => false,
                };
                if !valid || self.worktree_root_with(worktree, internal).await.as_ref() != Ok(&root)
                {
                    let _ = self.transfers.cancel(caller, prepared.stream);
                    return Err(unavailable());
                }
                Output::AttachmentDownload(prepared)
            }
            Command::DownloadImage { session, image } => {
                let bytes = self.tool_image(session, image.clone()).await?;
                let worktree = image.attachment.spec.worktree;
                let root = self.worktree_root_with(worktree, internal).await?;
                let prepared = self.transfers.download_bytes(
                    caller,
                    root.clone(),
                    image.attachment.clone(),
                    bytes,
                    self.closed.clone(),
                )?;
                // A rewind or removal may hide the source while preparing its stream.
                if self.tool_image(session, image).await.is_err()
                    || self.worktree_root_with(worktree, internal).await.as_ref() != Ok(&root)
                {
                    let _ = self.transfers.cancel(caller, prepared.stream);
                    return Err(unavailable());
                }
                Output::AttachmentDownload(prepared)
            }
            Command::PreviewOffice { worktree, path } => {
                let root = self.worktree_root_with(worktree, internal).await?;
                let preview = self
                    .transfers
                    .office(caller, worktree, root.clone(), path, self.closed.clone())
                    .await?;
                if self.worktree_root_with(worktree, internal).await.as_ref() != Ok(&root) {
                    let _ = self.transfers.cancel(caller, preview.download.stream);
                    return Err(unavailable());
                }
                Output::OfficePreview(preview)
            }
            Command::DownloadFile { worktree, path } => {
                let root = self.worktree_root_with(worktree, internal).await?;
                let prepared = self
                    .transfers
                    .download(caller, worktree, root.clone(), path, self.closed.clone())
                    .await?;
                // Removal may have started between lookup and registering the transfer.
                if self.worktree_root_with(worktree, internal).await.as_ref() != Ok(&root) {
                    let _ = self.transfers.cancel(caller, prepared.stream);
                    return Err(unavailable());
                }
                Output::FileDownload(prepared)
            }
            Command::CancelFileTransfer { stream } => {
                self.transfers.cancel(caller, stream)?;
                Output::FileTransferCancelled { stream }
            }
            Command::UploadFile(spec) => {
                let root = self.worktree_root_with(spec.worktree, internal).await?;
                let prepared = self
                    .transfers
                    .upload(caller, root.clone(), spec, self.closed.clone())
                    .await?;
                if self
                    .worktree_root_with(prepared.spec.worktree, internal)
                    .await
                    .as_ref()
                    != Ok(&root)
                {
                    let _ = self.transfers.cancel(caller, prepared.stream);
                    return Err(unavailable());
                }
                Output::FileUpload(prepared)
            }
            _ => {
                return Err(Fault::new(
                    ErrorCode::Internal,
                    "file transfer command expected",
                ));
            }
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
}
