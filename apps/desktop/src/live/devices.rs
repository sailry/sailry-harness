use super::*;
impl Shell {
    pub(super) fn revoke_device(&self, node: NodeId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(live) = &self.live else {
            return;
        };
        if node == live.services.local.target() || live.pending {
            return;
        }
        let shell = cx.entity().downgrade();
        crate::prompts::confirm(
            &tr("live_revoke"),
            &rust_i18n::t!("live_revoke_confirm", name = live.name(node)),
            tr("live_revoke"),
            window,
            cx,
            move |_, cx| {
                let _ = shell.update(cx, |shell, cx| {
                    let Some(live) = &mut shell.live else {
                        return;
                    };
                    if live.pending {
                        return;
                    }
                    live.pending = true;
                    live.action_error = false;
                    let link = live.services.link.clone();
                    let job = live
                        .services
                        .runtime
                        .spawn(async move { link.set_trust(node, false).await });
                    cx.spawn(async move |shell, cx| {
                        let result = job.await;
                        let _ = shell.update(cx, |shell, cx| {
                            if let Some(live) = &mut shell.live {
                                live.pending = false;
                                live.action_error = !matches!(result, Ok(Ok(())));
                                cx.notify();
                            }
                        });
                    })
                    .detach();
                    cx.notify();
                });
            },
        );
    }
}

pub(super) fn observe(services: &Services, cx: &mut Context<Shell>) -> Task<()> {
    let link = services.link.clone();
    let mut changed = link.peer_changes();
    let local = services.local.target();
    let runtime = services.runtime.clone();
    let mut known = crate::preferences::data(cx).devices.unwrap_or_default();
    cx.spawn(async move |shell, cx| {
        loop {
            let handle = link.clone();
            let cached = known.clone();
            let addresses = runtime
                .spawn(async move {
                    use futures::{StreamExt, stream};
                    let addresses = handle.peers().await?;
                    let entries = stream::iter(addresses)
                        .map(|address| {
                            let handle = &handle;
                            let cached = &cached;
                            async move {
                                let key: String = address
                                    .id
                                    .as_bytes()
                                    .iter()
                                    .map(|byte| format!("{byte:02x}"))
                                    .collect();
                                let mut device = cached.get(&key).cloned();
                                if let Ok(Ok(info)) = tokio::time::timeout(
                                    std::time::Duration::from_secs(3),
                                    handle.inspect(&address),
                                )
                                .await
                                {
                                    let known = device.get_or_insert(crate::preferences::Device {
                                        name: None,
                                        execution: info.execution,
                                        platform: None,
                                    });
                                    known.execution = info.execution;
                                    known.platform = Some(info.platform);
                                    if info.name.is_some() {
                                        known.name = info.name;
                                    }
                                }
                                (address, key, device)
                            }
                        })
                        .buffer_unordered(4)
                        .collect::<Vec<_>>()
                        .await;
                    Ok::<_, sailry_protocol::Fault>(entries)
                })
                .await;
            let retry = addresses.as_ref().is_ok_and(|result| {
                result
                    .as_ref()
                    .is_ok_and(|entries| entries.iter().any(|(_, _, device)| device.is_none()))
            });
            if let Ok(Ok(entries)) = &addresses {
                known = entries
                    .iter()
                    .filter_map(|(_, key, device)| {
                        device.clone().map(|device| (key.clone(), device))
                    })
                    .collect();
            }
            if shell
                .update(cx, |shell, cx| {
                    let Some(live) = &mut shell.live else {
                        return;
                    };
                    if let Ok(Ok(addresses)) = addresses {
                        let mut devices = crate::preferences::data(cx).devices.unwrap_or_default();
                        for (_, key, device) in &addresses {
                            if let Some(device) = device {
                                devices
                                    .entry(key.clone())
                                    .and_modify(|known| {
                                        known.execution = device.execution;
                                        if device.name.is_some() {
                                            known.name = device.name.clone();
                                        }
                                        if device.platform.is_some() {
                                            known.platform = device.platform.clone();
                                        }
                                    })
                                    .or_insert_with(|| device.clone());
                            }
                        }
                        if crate::preferences::data(cx).devices.as_ref() != Some(&devices) {
                            crate::preferences::update(cx, |data| data.devices = Some(devices));
                        }
                        live.hosts = addresses
                            .into_iter()
                            .filter_map(|(address, _, device)| {
                                device
                                    .filter(|device| device.execution)
                                    .map(|_| (NodeId(*address.id.as_bytes()), address))
                            })
                            .collect();
                        live.hosts.insert(local, link.address());
                        if !live.hosts.contains_key(&live.selected) {
                            live.select(local, cx);
                            shell.page = crate::preview::Page::Host;
                        }
                    }
                    cx.notify();
                })
                .is_err()
            {
                return;
            }
            if !retry {
                if changed.changed().await.is_err() {
                    return;
                }
                continue;
            }
            let changed = changed.changed();
            let timer = cx
                .background_executor()
                .timer(std::time::Duration::from_secs(10));
            futures::pin_mut!(changed, timer);
            if let futures::future::Either::Left((Err(_), _)) =
                futures::future::select(changed, timer).await
            {
                return;
            }
        }
    })
}
