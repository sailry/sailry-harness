//! Headless callbacks compose existing scoped commands on a separate worker.
use super::super::Ingress;
use sailry_link::{CancellationToken, Handler};
use sailry_protocol::*;
use std::sync::Arc;

pub(super) async fn run(
    ingress: Arc<Ingress>,
    caller: NodeId,
    request: Request,
    info: plugin::Info,
    stop: CancellationToken,
) -> Result<Output, Fault> {
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let Command::CallPlugin { handler, input } = request.command else {
            unreachable!()
        };
        let mut context = request.plugin.expect("admitted plugin callback");
        context.invocation = Some(request.id);
        let bundle = ingress.plugins.read_host(&info, stop.clone())?;
        let target = ingress.node;
        crate::plugins::script::run(
            bundle,
            &handler,
            input,
            context,
            target,
            stop,
            move |request| {
                // Await the original child completion before releasing the parent. Closing a
                // controller never drops admitted writes or causes automatic replay.
                runtime.block_on(async {
                    let admitted = ingress.dispatch(caller, request).await?;
                    admitted.completion.await.map_err(|_| {
                        Fault::new(
                            ErrorCode::OutcomeUnknown,
                            "plugin SDK completion is unknown",
                        )
                    })?
                })
            },
        )
        .map(Output::PluginResult)
    })
    .await
    .map_err(|_| Fault::new(ErrorCode::OutcomeUnknown, "plugin callback worker failed"))?
}
