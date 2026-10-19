//! Headless callbacks compose existing scoped commands on a separate worker.
use super::super::Ingress;
use sailry_link::{CancellationToken, Handler};
use sailry_protocol::*;
use std::{sync::Arc, time::Instant};

impl Ingress {
    pub(crate) async fn script_dispatch(
        &self,
        caller: NodeId,
        request: Request,
        deadline: Instant,
        stop: &CancellationToken,
    ) -> sailry_link::Response {
        let durable = request.command.durable();
        let id = request.id;
        #[cfg(any(test, feature = "test-support"))]
        let original = request.clone();
        let completion = async {
            let admitted = self.dispatch(caller, request).await?;
            #[cfg(any(test, feature = "test-support"))]
            if durable {
                let wait = self.plugins.script_wait.lock().unwrap().take();
                if let Some(wait) = wait {
                    let _ = wait.entered.send(original);
                    wait.release.cancelled().await;
                }
            }
            admitted.completion.await.map_err(|_| {
                Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "plugin SDK completion is unknown",
                )
            })?
        };
        // Admission can itself wait for I/O. Bound the entire dispatch, not only
        // the completion receiver. Dropping this observer never replays a write.
        tokio::select! {
            biased;
            result = completion => result,
            _ = stop.cancelled() => Err(interrupted(durable, id)),
            _ = tokio::time::sleep_until(deadline.into()) => Err(interrupted(durable, id)),
        }
    }
}

fn interrupted(durable: bool, id: RequestId) -> Fault {
    if durable {
        Fault::new(
            ErrorCode::OutcomeUnknown,
            format!("plugin SDK result is unknown for request {id}"),
        )
    } else {
        Fault::new(ErrorCode::Cancelled, "plugin SDK wait stopped")
    }
}

pub(super) async fn run(
    ingress: Arc<Ingress>,
    caller: NodeId,
    request: Request,
    info: plugin::Info,
    stop: CancellationToken,
) -> Result<Output, Fault> {
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let read_only = matches!(request.command, Command::ReadPluginCallback { .. });
        let (Command::CallPlugin { handler, input }
        | Command::ReadPluginCallback { handler, input }) = request.command
        else {
            unreachable!()
        };
        let mut context = request.plugin.expect("admitted plugin callback");
        if !read_only {
            context.invocation = Some(request.id);
        }
        let bundle = ingress.plugins.read_host(&info, stop.clone())?;
        let target = ingress.node;
        let stopped = stop.clone();
        let deadline = ingress.plugins.script_deadline();
        crate::plugins::script::run_callback(
            bundle,
            &handler,
            input,
            crate::plugins::script::Environment {
                context,
                target,
                stop,
                deadline,
            },
            read_only,
            move |request| {
                runtime.block_on(ingress.script_dispatch(caller, request, deadline, &stopped))
            },
        )
        .map(Output::PluginResult)
    })
    .await
    .map_err(|_| Fault::new(ErrorCode::OutcomeUnknown, "plugin callback worker failed"))?
}
