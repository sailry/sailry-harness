use super::Ingress;
use sailry_link::Admission;
use sailry_protocol::{Command, ErrorCode, Fault, NodeId, Output, Request};

struct Operation<'a> {
    controls: &'a crate::agent::Controls,
    turn: sailry_protocol::TurnId,
}

impl Drop for Operation<'_> {
    fn drop(&mut self) {
        self.controls.finish_operation(self.turn);
    }
}

impl Ingress {
    pub(super) async fn fetch_web(
        &self,
        caller: NodeId,
        request: Request,
    ) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != sailry_protocol::VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let output = match &request.command {
            Command::FetchWeb(input) => {
                let turn = request.plugin.as_ref().and_then(|context| context.turn);
                let (_operation, stop) = if let Some(turn) = turn {
                    let stop = self.agents.begin_operation(turn).ok_or_else(|| {
                        Fault::new(ErrorCode::Cancelled, "web fetch turn is no longer running")
                    })?;
                    (
                        Some(Operation {
                            controls: &self.agents,
                            turn,
                        }),
                        stop,
                    )
                } else {
                    (None, self.closed.clone())
                };
                Output::WebPage(self.web.fetch(caller, request.id, input, &stop).await?)
            }
            Command::CancelWebFetch { request } => {
                self.web.cancel(caller, *request);
                Output::WebFetchCancelled { request: *request }
            }
            _ => unreachable!("web command expected"),
        };
        let (reply, completion) = tokio::sync::oneshot::channel();
        let _ = reply.send(Ok(output));
        Ok(Admission {
            receipt: sailry_protocol::Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }
}
