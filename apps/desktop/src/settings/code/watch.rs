use super::*;
use sailry_protocol::Update;
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
        let client = self.binding.client.clone();
        let node = self.binding.client.target();
        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
        self.binding.runtime.spawn(async move {
            loop {
                let stream = tokio::select! {
                    _ = stop.cancelled() => break,
                    stream = client.subscribe_code_installations() => stream,
                };
                if let Ok(mut stream) = stream {
                    loop {
                        let update = tokio::select! {
                            _ = stop.cancelled() => return,
                            update = stream.next() => update,
                        };
                        match update {
                            Ok(Update::CodeInstallations {
                                node: target,
                                installations,
                            }) if target == node => {
                                if sender.send(Some(installations)).await.is_err() {
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
                        if let Some(installations) = update {
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
                            let changed = installations.iter().map(|item| (&item.package, &item.receipt)).collect::<Vec<_>>()
                                != panel.installations.iter().map(|item| (&item.package, &item.receipt)).collect::<Vec<_>>();
                            panel.installations = installations;
                            if changed { panel.load(window, cx); }
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
