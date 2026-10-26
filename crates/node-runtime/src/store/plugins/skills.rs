use crate::store::Ingress;
use sailry_link::Admission;
use sailry_protocol::{Command, ErrorCode, Fault, NodeId, Output, Receipt, Request};
use tokio::sync::oneshot;

impl Ingress {
    pub(in crate::store) async fn discover_skills(
        &self,
        caller: NodeId,
        request: Request,
        internal: bool,
    ) -> Result<Admission, Fault> {
        let output = match request.command {
            Command::ReadSkillDocument { source } => {
                let installed =
                    if let sailry_protocol::plugin::skills::DocumentSource::Installed {
                        package,
                        ..
                    } = &source
                    {
                        let admission = self
                            .dispatch_request(
                                caller,
                                Request::new(
                                    self.node,
                                    Command::ReadPluginVersion {
                                        package: package.clone(),
                                    },
                                ),
                                internal,
                            )
                            .await?;
                        let output = admission
                            .completion
                            .await
                            .map_err(|_| crate::store::unavailable())??;
                        let Output::Plugin(info) = output else {
                            return Err(Fault::new(ErrorCode::Internal, "skill package expected"));
                        };
                        Some(info)
                    } else {
                        None
                    };
                Output::SkillDocument(
                    self.plugins
                        .read_skill_document(&source, installed, self.closed.clone())
                        .await?,
                )
            }
            Command::DiscoverSkills { source } => Output::SkillDiscovery(
                self.plugins
                    .discover_skills(&source, self.closed.clone())
                    .await?,
            ),
            Command::DiscoverRegistrySkill { source } => Output::RegistrySkillDiscovery(
                self.plugins
                    .discover_registry_skill(&source, self.closed.clone())
                    .await?,
            ),
            _ => {
                return Err(Fault::new(
                    ErrorCode::Internal,
                    "skill discovery command expected",
                ));
            }
        };
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
