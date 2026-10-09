use super::*;
use sailry_protocol::{Topic, Update};
use std::time::Duration;

impl Panel {
    pub(super) fn watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.stop.is_some() {
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        let stop = CancellationToken::new();
        self.stop = Some(stop.clone());
        let transport = self.transport.clone();
        let worktree = self.worktree;
        let node = self.binding.client.target();
        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
        self.binding.runtime.spawn(async move {
            loop {
                let stream = tokio::select! {
                    _ = stop.cancelled() => break,
                    stream = transport.subscribe(Topic::CodeServers(worktree)) => stream,
                };
                if let Ok(mut stream) = stream {
                    loop {
                        let update = tokio::select! {
                            _ = stop.cancelled() => return,
                            update = stream.next() => update,
                        };
                        match update {
                            Ok(Update::CodeServers {
                                node: target,
                                worktree: tree,
                                servers,
                                installations,
                            }) if target == node && tree == worktree => {
                                if sender.send(Some((servers, installations))).await.is_err() {
                                    return;
                                }
                            }
                            Ok(_) => {}
                            Err(_) => break,
                        }
                    }
                }
                if sender.send(None).await.is_err() {
                    break;
                }
                tokio::select! {
                    _ = stop.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                }
            }
        });
        cx.spawn_in(window, async move |owner, cx| {
            while let Some(update) = receiver.recv().await {
                if owner
                    .update_in(cx, |panel, window, cx| {
                        if !panel.active || panel.generation != generation {
                            return;
                        }
                        if let Some((servers, installations)) = update {
                            panel.servers = servers;
                            for installation in &installations {
                                let previous = panel
                                    .installations
                                    .iter()
                                    .find(|previous| previous.package == installation.package);
                                if previous
                                    .is_some_and(|previous| previous.status != installation.status)
                                    && !panel.request.as_ref().is_some_and(|request| {
                                        matches!(&request.command, sailry_protocol::Command::InstallCodeServer { install } if install.package == installation.package)
                                    })
                                {
                                    match installation.status {
                                        code::InstallStatus::Ready => window
                                            .push_notification(crate::tr("code_installed"), cx),
                                        code::InstallStatus::Failed => window.push_notification(
                                            gpui_kit::component::notification::Notification::error(
                                                crate::tr("code_failed"),
                                            ),
                                            cx,
                                        ),
                                        code::InstallStatus::Cancelled => window
                                            .push_notification(crate::tr("code_cancelled"), cx),
                                        _ => {}
                                    }
                                }
                            }
                            panel.installations = installations;
                        } else {
                            panel.error = Some("code_sync_failed");
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }
}
