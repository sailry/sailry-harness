use super::*;

impl Details {
    pub(super) fn reconnect(
        &mut self,
        server: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(info) = self.info.as_ref().filter(|info| info.summary.enabled) else {
            return;
        };
        if !matches!(self.target, Target::Installed(_))
            || self.stop.is_cancelled()
            || !info.mcp.iter().any(|current| {
                current.name == server
                    && current.allowed
                    && current.availability == sailry_protocol::plugin::mcp::Availability::Available
            })
            || !self.reconnecting.insert(server.clone())
        {
            return;
        }
        let package = info.summary.reference();
        let binding = self.binding.clone();
        let expected = package.clone();
        let name = server.clone();
        let job = binding.runtime.spawn(async move {
            binding
                .client
                .execute(
                    binding
                        .client
                        .prepare(Command::CheckPluginMcp { package, server }),
                )
                .await
        });
        cx.spawn_in(window, async move |details, cx| {
            let result = job.await;
            _ = details.update_in(cx, |details, window, cx| {
                details.reconnecting.remove(&name);
                if details.stop.is_cancelled()
                    || details
                        .info
                        .as_ref()
                        .is_none_or(|info| info.summary.reference() != expected)
                {
                    return;
                }
                match result {
                    Ok(Ok(Output::PluginMcpChecked(check)))
                        if check.package == expected && check.server == name =>
                    {
                        crate::feedback::toast(
                            window,
                            tr("connector_mcp_reconnect"),
                            gpui_kit::component::notification::Notification::success(
                                rust_i18n::t!("connector_mcp_reconnected", n = check.tools.len())
                                    .to_string(),
                            ),
                            cx,
                        );
                    }
                    Ok(Err(error)) => crate::settings::connectors::failure(error, window, cx),
                    _ => crate::feedback::error(
                        "connector_check_failed",
                        "",
                        &tr("connector_check_failed"),
                        window,
                        cx,
                    ),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
