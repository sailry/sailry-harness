use super::*;

impl Controller {
    pub(super) fn watch_code(&mut self, cx: &mut Context<Self>) {
        if self.code_watch.is_some() {
            return;
        }
        let Some(worktree) = self.binding.worktree else {
            return;
        };
        let client = self.binding.client.clone();
        let stop = self.stop.child_token();
        let (send, mut receive) = tokio::sync::mpsc::channel(1);
        self.binding.runtime.spawn(async move {
            loop {
                let subscription = tokio::select! {
                    biased;
                    _ = stop.cancelled() => return,
                    result = client.subscribe_code_servers(worktree) => result,
                };
                if let Ok(mut subscription) = subscription {
                    loop {
                        let update = tokio::select! {
                            biased;
                            _ = stop.cancelled() => return,
                            result = subscription.next() => result,
                        };
                        let Ok(sailry_protocol::Update::CodeServers {
                            node,
                            worktree: tree,
                            servers,
                            installations,
                        }) = update
                        else {
                            break;
                        };
                        if node != client.target() || tree != worktree {
                            return;
                        }
                        tokio::select! {
                            biased;
                            _ = stop.cancelled() => return,
                            result = send.send((servers, installations)) => {
                                if result.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                }
                tokio::select! {
                    biased;
                    _ = stop.cancelled() => return,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
                }
            }
        });
        self.code_watch = Some(cx.spawn(async move |controller, cx| {
            let mut previous = None;
            while let Some((servers, installations)) = receive.recv().await {
                let servers = servers
                    .iter()
                    .map(|server| {
                        (
                            server.startup.revision.clone(),
                            (server.status, server.diagnostics_revision),
                        )
                    })
                    .collect::<BTreeMap<_, _>>();
                let installations = installations
                    .iter()
                    .map(|installation| {
                        (
                            installation.package.clone(),
                            (
                                installation.status,
                                installation
                                    .receipt
                                    .as_ref()
                                    .map(|receipt| receipt.revision.clone()),
                            ),
                        )
                    })
                    .collect::<BTreeMap<_, _>>();
                let current = (servers, installations);
                if previous.as_ref() == Some(&current) {
                    continue;
                }
                previous = Some(current);
                if controller
                    .update(cx, |controller, cx| {
                        for document in controller
                            .documents
                            .values()
                            .filter(|document| !document.dismissed)
                        {
                            if let Some(provider) = &document.code {
                                provider.observed.borrow_mut().take();
                            }
                        }
                        controller.refresh_code(cx);
                    })
                    .is_err()
                {
                    return;
                }
            }
        }));
    }
}
