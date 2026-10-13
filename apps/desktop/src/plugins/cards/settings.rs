//! Card configuration reuses the current same-package settings owner.
use super::*;

impl Card {
    pub(super) fn open_settings(
        &mut self,
        host: Arc<host::Host>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings_task.is_some() || !self.active(cx) {
            return;
        }
        let request = host.package().and_then(|package| {
            host.request(Command::ReadPluginConnection {
                package,
                card: host.declaration.key.clone(),
            })
        });
        let Ok(request) = request else { return };
        let worker = self.binding.client.clone();
        let runtime = self.binding.runtime.handle().clone();
        let job = runtime.spawn(async move {
            let Output::PluginConnection(status) = worker.execute(request).await? else {
                return Err(sailry_protocol::Fault::new(
                    sailry_protocol::ErrorCode::Internal,
                    "connection status response expected",
                ));
            };
            let Output::Plugin(info) = worker
                .execute(worker.prepare(Command::ReadPlugin {
                    name: status.package.name.clone(),
                }))
                .await?
            else {
                return Err(sailry_protocol::Fault::new(
                    sailry_protocol::ErrorCode::Internal,
                    "plugin metadata response expected",
                ));
            };
            if !info.summary.enabled || info.summary.reference() != status.package {
                return Err(sailry_protocol::Fault::new(
                    sailry_protocol::ErrorCode::RevisionConflict,
                    "plugin configuration changed",
                ));
            }
            Ok(info)
        });
        let binding = self.binding.clone();
        self.settings_task = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |card, window, cx| {
                card.settings_task = None;
                if !card.active(cx) {
                    return;
                }
                match result {
                    Ok(Ok(info)) => {
                        crate::settings::open_plugin_settings(binding, info, window, cx)
                    }
                    _ => card.toast(crate::tr("plugins_failed").to_string(), true, window, cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}
