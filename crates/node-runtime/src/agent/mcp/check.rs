//! Explicit checks refresh shared connections or confirm private transport cleanup.
use super::*;
use sailry_protocol::plugin::{Context, mcp};

pub(crate) async fn run(
    ingress: &Arc<Ingress>,
    package: &Reference,
    server: &str,
    stop: &CancellationToken,
) -> Result<mcp::Check, Fault> {
    let (info, settings) = ingress
        .mcp_preparation(package.clone(), server.into())
        .await?;
    ingress.agents.mcp.observe(&info.summary);
    let allowed = settings.mcp_allowed.get(server).copied().unwrap_or(true);
    let resources = crate::plugins::resources::Resources::new(
        ingress.plugins.clone(),
        vec![info],
        std::collections::BTreeMap::from([(package.name.clone(), Ok(settings))]),
        stop.clone(),
    );
    let mut config = resources
        .mcp_server(&ingress.environment, server)
        .await
        .into_iter()
        .next()
        .ok_or_else(unavailable)?;
    if config.options.reuse && allowed {
        ingress.agents.mcp.reconnect(&package.name, server);
        let launch = std::mem::replace(&mut config.launch, Err(unavailable()));
        let attempt = Target {
            resources: &resources,
            ingress,
            invocation: None,
            stop,
        }
        .connect(0, &config, launch)
        .await;
        let (connection, tools, _) = attempt.result?;
        connection.release().await?;
        return Ok(checked(package, server, tools));
    }
    let attempt = allowed
        .then(|| ingress.agents.mcp.begin(package, server))
        .flatten();
    if allowed {
        ingress.agents.mcp.started(package, server);
    }
    let result = async {
    let mut launch = config.launch?;
    let context = Context {
        package: package.clone(),
        worktree: None,
        session: None,
        turn: None,
        invocation: None,
        surface: Default::default(),
    };
    let connector = tokio::select! {
        biased;
        _ = stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "MCP check was cancelled")),
        result = tokio::time::timeout(Duration::from_secs(10), resources.mcp_authorization(ingress, context, server, &mut launch)) => result.map_err(|_| unavailable())??,
    };
    let handler = Arc::new(input::Handler::new());
    let (mut transport, service) =
        transport::Connection::open(launch, connector, stop, handler, true, &ingress.environment)
            .await?;
    let result = tokio::select! {
        biased;
        _ = stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "MCP check was cancelled")),
        result = tokio::time::timeout(Duration::from_secs(10), list(service.peer(), config.tools.as_deref())) => result.unwrap_or_else(|_| Err(unavailable())),
    };
    drop(service);
    // A successful list is not a successful check until its diagnostic owner closes.
    transport.close().await?;
    result
    }.await;
    ingress.agents.mcp.finish(
        attempt,
        if stop.is_cancelled() {
            Err(cancelled())
        } else {
            result.as_ref().map(Vec::len).map_err(Clone::clone)
        },
    );
    result.map(|tools| checked(package, server, tools))
}

fn checked(package: &Reference, server: &str, tools: Vec<Descriptor>) -> mcp::Check {
    mcp::Check {
        package: package.clone(),
        server: server.into(),
        tools: tools
            .into_iter()
            .map(|tool| mcp::Tool {
                name: tool.name.to_string(),
                description: tool.description.as_ref().map(ToString::to_string),
            })
            .collect(),
    }
}
