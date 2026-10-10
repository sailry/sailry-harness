//! The package captures native controller resources; scripts never receive WebView handles.
use super::host::{
    Host,
    sdk::values::{decode, encode},
};
use crate::browser::{Browser, annotation, settings, state};
use gpui_kit::*;
use gpui_shell::{HostError, HostModule, HostValue};
use serde_json::Value;
use std::{cell::RefCell, sync::Arc};

#[derive(Clone)]
pub(super) struct Scope {
    pub browser: Option<Entity<Browser>>,
    pub read: bool,
    pub control: bool,
    pub settings: bool,
}

pub(super) fn surface(module: HostModule, scope: &Scope, host: Arc<Host>, cx: &App) -> HostModule {
    let browser = scope
        .browser
        .clone()
        .filter(|_| scope.read && scope.control);
    let preview_browser = browser.clone();
    let preview_host = host.clone();
    let lease = browser.as_ref().map(|browser| browser.read(cx).lease);
    module
        .component("BrowserSurface", move |args, window, cx| {
            let Some(browser) = browser.as_ref().filter(|_| host.check().is_ok()) else {
                return div().into_any_element();
            };
            let Some(lease) = lease.filter(|lease| browser.read(cx).lease == *lease) else {
                return div().into_any_element();
            };
            // Wry is an OS child view: embedded Kit dialogs cannot occlude it.
            // The package declares coverage; mounting and capture stay intact.
            let covered = args.props().get("covered").and_then(HostValue::as_bool) == Some(true);
            browser.update(cx, |browser, cx| browser.cover(lease, covered, window, cx));
            div()
                .size_full()
                .min_w_0()
                .min_h_0()
                .child(browser.clone())
                .into_any_element()
        })
        .component("BrowserAnnotationImage", move |args, _, cx| {
            // Captured native image handles are absent from Kit's script API. Keep
            // screenshot bytes private while the package composes its review form.
            let token = args
                .props()
                .get("token")
                .and_then(HostValue::as_str)
                .unwrap_or_default();
            let Some(preview) = preview_browser
                .as_ref()
                .filter(|browser| {
                    preview_host.check().is_ok() && Some(browser.read(cx).lease) == lease
                })
                .and_then(|browser| browser.read(cx).annotation_preview(token))
            else {
                return div().into_any_element();
            };
            img(preview)
                .w_full()
                .max_h(px(240.))
                .object_fit(ObjectFit::Contain)
                .into_any_element()
        })
}

fn permit(host: &Host, allowed: bool) -> Result<(), HostError> {
    host.check()?;
    allowed
        .then_some(())
        .ok_or_else(|| HostError::new("browser capability is unavailable in this view"))
}

pub(super) fn module(
    module: HostModule,
    scope: Scope,
    host: Arc<Host>,
    window: &Window,
    cx: &mut App,
) -> HostModule {
    let declarations = format!(
        "{}\n{}",
        module.declared().unwrap_or_default(),
        include_str!("browser/api.d.ts")
    );
    let changes = scope
        .browser
        .as_ref()
        .filter(|_| scope.read)
        .map(|browser| browser.read(cx).changes(cx));
    if let Some(browser) = &scope.browser {
        let lease = browser.update(cx, |browser, cx| browser.acquire_lease(cx));
        let browser = browser.downgrade();
        let stop = host.stop_token();
        window
            .spawn(cx, async move |cx| {
                stop.cancelled().await;
                let _ = browser.update_in(cx, |browser, window, cx| {
                    if browser.lease == lease {
                        browser.visibility(false, window, cx);
                    }
                });
            })
            .detach();
    }
    let settings = scope
        .settings
        .then(|| cx.new(|cx| settings::Settings::new(window, cx)));
    let read_browser = scope.browser.clone().filter(|_| scope.read);
    let submit_browser = scope
        .browser
        .clone()
        .filter(|_| scope.read && scope.control);
    let action_browser = scope.browser.filter(|_| scope.control);
    let read_host = host.clone();
    let changes_host = host.clone();
    let action_host = host.clone();
    let submit_host = host.clone();
    let settings_host = host.clone();
    let preferences_host = host.clone();
    let scan_host = host.clone();
    let import_host = host;
    let profiles = settings.clone();
    module.function("readBrowser", move |_| {
        read_host.check()?;
        let browser = read_browser.as_ref().ok_or_else(|| HostError::new("browser scope is unavailable"))?;
        gpui_shell::with_current_app(|cx| encode(browser.read(cx).snapshot(cx)))
            .ok_or_else(|| HostError::new("browser requires an active view"))?
    }).async_function("nextBrowserChange", move |args| {
        changes_host.check()?;
        let seen = args.string(0)?.to_owned();
        let mut changes = changes.clone().ok_or_else(|| HostError::new("browser scope is unavailable"))?;
        let stop = changes_host.stop_token();
        Ok(async move {
            loop {
                let value = changes.borrow_and_update().clone();
                if value["cursor"].as_str() != Some(&seen) { return encode(value); }
                tokio::select! {
                    biased;
                    _ = stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                    changed = changes.changed() => changed.map_err(|_| HostError::new("browser is closed"))?,
                }
            }
        })
    }).async_function("browserAction", move |args| {
        action_host.check()?;
        let browser = action_browser.clone().ok_or_else(|| HostError::new("browser control is unavailable"))?;
        let action: state::Action = serde_json::from_value(decode(args.value(0)?)?).map_err(|error| HostError::new(error.to_string()))?;
        let stop = action_host.stop_token();
        let (reply, receive) = tokio::sync::oneshot::channel();
        gpui_shell::with_current_app(|cx| cx.defer(move |cx| {
            browser.update(cx, |_, cx| cx.emit(state::Request { action, stop, reply: RefCell::new(Some(reply)) }));
        })).ok_or_else(|| HostError::new("browser requires an active view"))?;
        Ok(async move { receive.await.map_err(|_| HostError::new("browser is closed"))?.map_err(HostError::new).and_then(encode) })
    }).async_function("submitBrowserAnnotation", move |args| {
        submit_host.check()?;
        let browser = submit_browser.clone().ok_or_else(|| HostError::new("browser control is unavailable"))?;
        let token = args.string(0)?.to_owned();
        let reference = args.string(1)?.to_owned();
        let stop = submit_host.stop_token();
        let request_stop = stop.clone();
        let (reply, receive) = tokio::sync::oneshot::channel();
        let timer = gpui_shell::with_current_app(|cx| {
            let timer = cx.background_executor().timer(std::time::Duration::from_secs(5));
            cx.defer(move |cx| {
                browser.update(cx, |_, cx| cx.emit(annotation::Submit {
                    token, reference, stop:request_stop, reply:RefCell::new(Some(reply)),
                }));
            });
            timer
        }).ok_or_else(|| HostError::new("browser requires an active view"))?;
        Ok(async move {
            tokio::select! {
                biased;
                _ = stop.cancelled() => Err(HostError::new("plugin view is closed")),
                _ = timer => Err(HostError::new("browser_annotation_stale")),
                result = receive => result.map_err(|_| HostError::new("browser_annotation_stale"))?
                    .map_err(HostError::new).map(|_| HostValue::Null),
            }
        })
    }).function("readBrowserSettings", move |_| {
        permit(&settings_host, scope.settings && scope.read)?;
        gpui_shell::with_current_app(|cx| encode(settings::snapshot(cx)))
            .ok_or_else(|| HostError::new("browser settings require an active view"))?
    }).function("setBrowserPersistent", move |args| {
        permit(&preferences_host, scope.settings && scope.control)?;
        let value = args.value(0)?.as_bool().ok_or_else(|| HostError::new("boolean expected"))?;
        gpui_shell::with_current_app(|cx| settings::persistent(value,cx))
            .ok_or_else(|| HostError::new("browser settings require an active view"))?.map_err(HostError::new)?;
        Ok(HostValue::Null)
    }).async_function("listBrowserProfiles", move |_| {
        permit(&scan_host, scope.settings && scope.read)?;
        let receive = request_settings(&profiles, settings::Action::Scan, &scan_host)?;
        Ok(async move { receive.await.map_err(|_| HostError::new("browser settings are closed"))?.map_err(HostError::new).and_then(encode) })
    }).async_function("importBrowserProfile", move |args| {
        permit(&import_host, scope.settings && scope.control)?;
        let receive = request_settings(&settings, settings::Action::Import(args.string(0)?.into()), &import_host)?;
        Ok(async move { receive.await.map_err(|_| HostError::new("browser settings are closed"))?.map_err(HostError::new).and_then(encode) })
    }).declarations(declarations)
}

fn request_settings(
    settings: &Option<Entity<settings::Settings>>,
    action: settings::Action,
    host: &Host,
) -> Result<tokio::sync::oneshot::Receiver<Result<Value, String>>, HostError> {
    let settings = settings
        .clone()
        .ok_or_else(|| HostError::new("browser settings are unavailable"))?;
    let stop = host.stop_token();
    let (reply, receive) = tokio::sync::oneshot::channel();
    gpui_shell::with_current_app(|cx| {
        cx.defer(move |cx| {
            settings.update(cx, |_, cx| {
                cx.emit(settings::Request {
                    action,
                    stop,
                    reply: RefCell::new(Some(reply)),
                })
            });
        })
    })
    .ok_or_else(|| HostError::new("browser settings require an active view"))?;
    Ok(receive)
}
