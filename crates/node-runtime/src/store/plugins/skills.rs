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
        let Command::DiscoverSkills { source } = request.command else {
            return Err(Fault::new(
                ErrorCode::Internal,
                "skill discovery command expected",
            ));
        };
        let discovery = self
            .plugins
            .discover_skills(&source, self.closed.clone())
            .await?;
        let (send, completion) = oneshot::channel();
        let _ = send.send(Ok(Output::SkillDiscovery(discovery)));
        Ok(Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }
}
