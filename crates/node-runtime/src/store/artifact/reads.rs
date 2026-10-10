//! Package verification runs on the resource worker, outside storage admission.
use super::*;
use crate::store::{Ingress, Job, unavailable};
use sailry_link::Admission;
use tokio::sync::oneshot;

impl Ingress {
    pub(in crate::store) async fn read_artifact(
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
        let (reply, response) = oneshot::channel();
        let job = Job::Request {
            caller,
            request: Box::new(request.clone()),
            reply,
        };
        if internal {
            self.send_internal(job).await?;
        } else {
            self.sender
                .try_send(job)
                .map_err(|_| Fault::new(ErrorCode::Busy, "Node request queue is unavailable"))?;
        }
        let admission = response.await.map_err(|_| unavailable())??;
        let Output::Artifact(mut content) =
            admission.completion.await.map_err(|_| unavailable())??
        else {
            return Err(Fault::new(ErrorCode::Internal, "artifact content expected"));
        };
        let session = request
            .plugin
            .as_ref()
            .and_then(|context| context.session)
            .or({
                if let Command::ReadArtifact { session, .. } = &request.command {
                    *session
                } else {
                    None
                }
            })
            .unwrap_or(content.artifact.session);
        let reference = content.artifact.package.clone();
        let package = self
            .dispatch_request(
                caller,
                Request::new(self.node, Command::ReadPluginVersion { package: reference }),
                internal,
            )
            .await?;
        let Output::Plugin(info) = package.completion.await.map_err(|_| unavailable())?? else {
            return Err(Fault::new(ErrorCode::Internal, "artifact package expected"));
        };
        content.view = self
            .plugins
            .read_artifact_view(info, self.closed.clone())
            .await?;
        // Explicit removal while package resources load invalidates the read.
        let owner = self
            .dispatch_request(
                caller,
                Request::new(self.node, Command::ReadSession { session }),
                internal,
            )
            .await?;
        owner.completion.await.map_err(|_| unavailable())??;
        if self.closed.is_cancelled() {
            return Err(unavailable());
        }
        let (send, completion) = oneshot::channel();
        let _ = send.send(Ok(Output::Artifact(content)));
        Ok(Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }
}
