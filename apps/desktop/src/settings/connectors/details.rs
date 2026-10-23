//! Service details compose existing account and MCP owners without probing on entry.
use super::*;
use sailry_protocol::{
    NodeId,
    plugin::{Info, Mcp, Reference, mcp::Check},
};

mod view;

pub(super) struct Details {
    owner: Entity<Workspace>,
    binding: Binding,
    key: Option<catalog::Key>,
    package: String,
    checks: BTreeMap<String, Check>,
    pending: BTreeSet<String>,
    requests: BTreeMap<String, Request>,
    drafts: BTreeMap<String, bool>,
    stop: CancellationToken,
    scroll: ScrollHandle,
}

impl Drop for Details {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub(super) fn open(
    owner: Entity<Workspace>,
    key: Option<catalog::Key>,
    package: String,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(binding) = owner.read(cx).connectors.binding.clone() else {
        return;
    };
    let details = cx.new(|cx| {
        cx.observe_in(&owner, window, |details: &mut Details, _, window, cx| {
            if !details.active(cx) {
                details.stop.cancel();
                window.dispatch_action(
                    Box::new(super::super::CloseOwnedDetails(cx.entity_id())),
                    cx,
                );
            }
            details.reconcile(cx);
            cx.notify();
        })
        .detach();
        Details {
            owner,
            binding,
            key,
            package,
            checks: BTreeMap::new(),
            pending: BTreeSet::new(),
            requests: BTreeMap::new(),
            drafts: BTreeMap::new(),
            stop: CancellationToken::new(),
            scroll: ScrollHandle::new(),
        }
    });
    window.dispatch_action(Box::new(super::super::OpenDetails(details.into())), cx);
}

fn current(
    expected: NodeId,
    actual: Option<NodeId>,
    package: &Reference,
    service: Option<&catalog::Service>,
) -> bool {
    actual == Some(expected) && service.is_some_and(|service| service.package == *package)
}

impl Details {
    fn loading(&self, cx: &App) -> bool {
        let owner = self.owner.read(cx);
        owner.connectors.loading
            || owner
                .plugin_catalog
                .metadata
                .as_ref()
                .is_some_and(|metadata| !metadata.read(cx).settled())
    }

    fn reconcile(&mut self, cx: &App) {
        if let Some(service) = self.service(cx) {
            self.requests.retain(|server, request| {
                self.pending.contains(server)
                    || matches!(&request.command, Command::SetPluginMcpAllowed { package, .. } if *package == service.package)
            });
            if service.info.is_some() {
                self.checks.retain(|server, checked| {
                    checked.package == service.package
                        && service
                            .servers
                            .iter()
                            .any(|current| current.name == *server)
                });
            }
            retain_drafts(&mut self.drafts, &self.pending, &self.requests, &service);
        }
    }

    fn active(&self, cx: &App) -> bool {
        !self.stop.is_cancelled()
            && self
                .owner
                .read(cx)
                .provider_link
                .as_ref()
                .is_some_and(|live| live.binding.client.target() == self.binding.client.target())
    }

    fn service(&self, cx: &App) -> Option<catalog::Service> {
        if !self.active(cx) {
            return None;
        }
        self.owner
            .read(cx)
            .service_catalog(cx)
            .into_iter()
            .find(|service| {
                self.key
                    .as_ref()
                    .map_or(service.package.name == self.package, |key| {
                        service.key == *key
                    })
            })
    }

    fn accepts(&self, package: &Reference, cx: &App) -> bool {
        self.active(cx)
            && current(
                self.binding.client.target(),
                self.owner
                    .read(cx)
                    .provider_link
                    .as_ref()
                    .map(|live| live.binding.client.target()),
                package,
                self.service(cx).as_ref(),
            )
    }

    fn check(&mut self, server: Mcp, window: &mut Window, cx: &mut Context<Self>) {
        let Some(service) = self.service(cx) else {
            return;
        };
        if !service.enabled() || !self.pending.insert(server.name.clone()) {
            return;
        }
        let package = service.package;
        let binding = self.binding.clone();
        let expected = package.clone();
        let name = server.name;
        let request_name = name.clone();
        let stop = self.stop.clone();
        let job = binding.runtime.spawn(async move {
            tokio::select! {
                biased;
                _ = stop.cancelled() => None,
                result = binding.client.execute(binding.client.prepare(Command::CheckPluginMcp {
                    package, server: request_name,
                })) => Some(result),
            }
        });
        cx.spawn_in(window, async move |details, cx| {
            let result = job.await;
            _ = details.update_in(cx, |details, window, cx| {
                if !details.active(cx) {
                    return;
                }
                details.pending.remove(&name);
                if !details.accepts(&expected, cx) {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(Some(Ok(Output::PluginMcpChecked(checked))))
                        if checked.package == expected && checked.server == name =>
                    {
                        details.checks.insert(name, checked);
                        crate::feedback::toast(
                            window,
                            tr("connector_checked"),
                            notification::Notification::success(tr("connector_checked")),
                            cx,
                        );
                    }
                    Ok(Some(Err(error))) => failure(error, window, cx),
                    Ok(None) => (),
                    _ => crate::feedback::error("", &tr("connector_check_failed"), window, cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn set_allowed(
        &mut self,
        server: Mcp,
        allowed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(service) = self.service(cx) else {
            return;
        };
        if !self.pending.insert(server.name.clone()) {
            return;
        }
        let name = server.name;
        let package = service.package;
        let binding = self.binding.clone();
        if self
            .requests
            .get(&name)
            .is_some_and(|request| !same_toggle(request, &package, &name, allowed))
        {
            self.pending.remove(&name);
            crate::feedback::error("", &tr("connector_unknown"), window, cx);
            cx.notify();
            return;
        }
        self.drafts.insert(name.clone(), allowed);
        let request = self
            .requests
            .entry(name.clone())
            .or_insert_with(|| {
                binding.client.prepare(Command::SetPluginMcpAllowed {
                    package: package.clone(),
                    server: name.clone(),
                    allowed,
                })
            })
            .clone();
        let expected = package;
        let job = binding
            .runtime
            .spawn(async move { binding.client.execute(request).await });
        cx.spawn_in(window, async move |details, cx| {
            let result = job.await;
            _ = details.update_in(cx, |details, window, cx| {
                if !details.active(cx) {
                    return;
                }
                details.pending.remove(&name);
                let uncertain = match &result {
                    Ok(Err(error)) => super::super::plugins::live::uncertain(error),
                    Err(_) => true,
                    _ => false,
                };
                if !uncertain {
                    details.requests.remove(&name);
                }
                match result {
                    Ok(Ok(Output::Plugin(info))) if info.summary.name == expected.name => {
                        details.checks.remove(&name);
                        details.drafts.remove(&name);
                        crate::feedback::toast(
                            window,
                            tr("connector_saved"),
                            notification::Notification::success(tr("connector_saved")),
                            cx,
                        );
                    }
                    Ok(Err(error)) => failure(error, window, cx),
                    _ => crate::feedback::error("", &tr("connector_unknown"), window, cx),
                }
                details.reconcile(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn configure(&self, info: Info, server: String, window: &mut Window, cx: &mut App) {
        if self.active(cx) {
            super::super::mcp::configure_server(
                self.owner.clone(),
                self.binding.clone(),
                info,
                server,
                window,
                cx,
            );
        }
    }
}

fn same_toggle(request: &Request, expected: &Reference, name: &str, value: bool) -> bool {
    matches!(&request.command, Command::SetPluginMcpAllowed { package, server, allowed }
        if package == expected && server == name && *allowed == value)
}

fn retain_drafts(
    drafts: &mut BTreeMap<String, bool>,
    pending: &BTreeSet<String>,
    requests: &BTreeMap<String, Request>,
    service: &catalog::Service,
) {
    if service.info.is_none() {
        return;
    }
    drafts.retain(|server, allowed| {
        pending.contains(server)
            || requests.contains_key(server)
            || service
                .servers
                .iter()
                .any(|current| current.name == *server && current.allowed != *allowed)
    });
}

#[cfg(test)]
mod tests;
