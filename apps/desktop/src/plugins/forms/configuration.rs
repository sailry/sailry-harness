//! Kit scripts cannot mount the shared protected schema editor.
//! This bound action opens that editor without exposing its fields or credentials.
use super::*;
use crate::{conversation::live::Binding, plugins::host::Host};
use gpui_kit::base::Disableable as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use sailry_protocol::{Command, Output, plugin::desktop::Surface};

pub(super) fn extend(module: HostModule, host: Arc<Host>, binding: Binding) -> HostModule {
    let pending = Rc::new(Cell::new(false));
    module.component("ConfigurationButton", move |args, _, _| {
        let id = args.id().to_owned();
        let label = args
            .props()
            .get("label")
            .and_then(HostValue::as_str)
            .unwrap_or_default()
            .to_owned();
        let context = host.context();
        let disabled = args.props().get("disabled").and_then(HostValue::as_bool) == Some(true)
            || pending.get()
            || host.check().is_err()
            || context.surface != Surface::Settings
            || context.session.is_some()
            || context.turn.is_some()
            || context.invocation.is_some()
            || binding.client.target() != host.target();
        let (host, binding, pending) = (host.clone(), binding.clone(), pending.clone());
        Button::new(SharedString::from(id.clone()))
            .ghost()
            .small()
            .label(label.clone())
            .accessibility_label(label)
            .disabled(disabled)
            .debug_selector(move || id.clone())
            .on_click(move |_, window, cx| {
                if pending.get() || host.check().is_err() || host.settings_readable().is_err() {
                    return;
                }
                let source = host.context();
                if source.surface != Surface::Settings
                    || source.session.is_some()
                    || source.turn.is_some()
                    || source.invocation.is_some()
                {
                    return;
                }
                pending.set(true);
                window.refresh();
                let client = binding.client.clone();
                let name = source.package.name.clone();
                let job = binding.runtime.spawn(async move {
                    client
                        .execute(client.prepare(Command::ReadPlugin { name }))
                        .await
                });
                let (host, binding, pending) = (host.clone(), binding.clone(), pending.clone());
                let origin = window.window_handle();
                cx.spawn(async move |cx| {
                    let result = job.await;
                    let _ = cx.update_window(origin, |_, window, cx| {
                        pending.set(false);
                        window.refresh();
                        if host.check().is_err() {
                            return;
                        }
                        match result {
                            Ok(Ok(Output::Plugin(info)))
                                if info.summary.enabled
                                    && info.summary.reference() == source.package
                                    && host.context().package == source.package =>
                            {
                                crate::settings::open_plugin_configuration(
                                    binding,
                                    info.summary,
                                    window,
                                    cx,
                                );
                            }
                            _ => {
                                use gpui_kit::component::notification::Notification;
                                window.push_notification(
                                    Notification::error(crate::tr("plugins_settings_read_failed")),
                                    cx,
                                );
                            }
                        }
                    });
                })
                .detach();
            })
            .into_any_element()
    })
}
