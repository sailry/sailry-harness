use crate::store::Ingress;
use sailry_link::Admission;
use sailry_protocol::{Command, ErrorCode, Fault, Output, Receipt, Request, VERSION};
use tokio::sync::oneshot;

impl Ingress {
    pub(in crate::store) async fn discover_skills(
        &self,
        request: Request,
    ) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let output = match request.command {
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
