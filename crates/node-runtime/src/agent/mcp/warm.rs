use super::*;

pub(crate) async fn run(ingress: &Arc<Ingress>, package: Option<String>, stop: &CancellationToken) {
    let complete = package.is_none();
    let resources = match ingress.mcp_resources(package.clone()).await {
        Ok(resources) => resources,
        Err(_) => return,
    };
    let mut selected = resources
        .warm(&ingress.environment, package.as_deref())
        .await;
    if complete && let Err(error) = ingress.agents.mcp.reconcile().await {
        let mut entry =
            crate::logging::Entry::new(crate::logging::Area::Run, crate::logging::Phase::Failed);
        entry.context.node = Some(ingress.node);
        entry.stage = Some("mcp_server");
        entry.code = Some(error.code);
        entry.reason = Some("connection_cleanup");
        entry.emit_warn();
    }
    let launches: Vec<_> = selected
        .iter_mut()
        .map(|config| std::mem::replace(&mut config.launch, Err(unavailable())))
        .collect();
    let attempts = futures::future::join_all(selected.iter().zip(launches).enumerate().map(
        |(index, (config, launch))| {
            Target {
                resources: &resources,
                ingress,
                invocation: None,
                stop,
            }
            .connect(index, config, launch)
        },
    ))
    .await;
    for (config, attempt) in selected.into_iter().zip(attempts) {
        let result = match attempt.result {
            Ok((connection, _, _)) => connection.release().await,
            Err(error) => Err(error),
        };
        let mut entry = crate::logging::Entry::new(
            crate::logging::Area::Run,
            if result.is_ok() {
                crate::logging::Phase::Completed
            } else if stop.is_cancelled() {
                crate::logging::Phase::Cancelled
            } else {
                crate::logging::Phase::Failed
            },
        );
        entry.context.node = Some(ingress.node);
        entry.context.package = Some(config.reference);
        entry.context.handler = Some(config.name);
        entry.stage = Some("mcp_server");
        entry.connection_reused = Some(attempt.reused);
        entry.duration_us = Some(u64::try_from(attempt.elapsed.as_micros()).unwrap_or(u64::MAX));
        entry.code = result.err().map(|error| error.code);
        if entry.code.is_some() {
            entry.emit_warn();
        } else {
            entry.emit();
        }
    }
}
