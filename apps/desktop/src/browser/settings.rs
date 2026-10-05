//! Controller-local profile import keeps paths, keychain data, and cookies native.
#[cfg(target_os = "macos")]
use crate::permissions::{self, Action as PermissionAction, Card, Failure, Resource, Status};
use gpui_kit::*;
use sailry_link::CancellationToken;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeMap};
#[cfg(target_os = "macos")]
use std::{
    rc::Rc,
    sync::{Arc, Mutex},
};

type Reply = tokio::sync::oneshot::Sender<Result<Value, String>>;
pub(crate) enum Action {
    Scan,
    Import(String),
}
pub(crate) struct Request {
    pub action: Action,
    pub stop: CancellationToken,
    pub reply: RefCell<Option<Reply>>,
}
pub(crate) struct Settings {
    #[cfg(target_os = "macos")]
    profiles: BTreeMap<String, super::chrome::Profile>,
    busy: bool,
}
impl EventEmitter<Request> for Settings {}

pub(crate) fn snapshot(cx: &App) -> Value {
    json!({
        "supported": cfg!(target_os="macos"),
        "enabled": enabled(cx),
        "persistent": crate::preferences::data(cx).browser_persistent.unwrap_or(true),
    })
}
fn enabled(cx: &App) -> bool {
    cx.try_global::<crate::preferences::Preferences>()
        .and_then(|prefs| prefs.directory())
        .is_some()
}
pub(crate) fn persistent(value: bool, cx: &mut App) -> Result<(), String> {
    if !enabled(cx) || !cfg!(target_os = "macos") {
        return Err("browser_import_unavailable".into());
    }
    crate::preferences::update(cx, |data| data.browser_persistent = Some(value));
    Ok(())
}
impl Settings {
    pub(crate) fn new(window: &Window, cx: &mut Context<Self>) -> Self {
        cx.subscribe_in(
            &cx.entity(),
            window,
            |this, _, request: &Request, window, cx| {
                let Some(reply) = request.reply.borrow_mut().take() else {
                    return;
                };
                if this.busy || request.stop.is_cancelled() || !enabled(cx) {
                    let _ = reply.send(Err("browser_import_unavailable".into()));
                    return;
                }
                #[cfg(target_os = "macos")]
                this.start(&request.action, request.stop.clone(), reply, window, cx);
                #[cfg(not(target_os = "macos"))]
                let _ = reply.send(Err("browser_import_unavailable".into()));
            },
        )
        .detach();
        Self {
            #[cfg(target_os = "macos")]
            profiles: BTreeMap::new(),
            busy: false,
        }
    }

    #[cfg(target_os = "macos")]
    fn start(
        &mut self,
        action: &Action,
        stop: CancellationToken,
        reply: Reply,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            Action::Scan => {
                self.busy = true;
                let result = Arc::new(Mutex::new(None));
                let value = result.clone();
                let request: PermissionAction = Rc::new(move |cx, cancel| {
                    let value = value.clone();
                    cx.background_executor().spawn(async move {
                        if cancel.is_cancelled() {
                            return Err(access_failure("permission_cancelled"));
                        }
                        let profiles = super::chrome::profiles().map_err(access_failure)?;
                        *value.lock().unwrap() = Some(profiles);
                        Ok(vec![(Resource::Chrome, Status::Granted)])
                    })
                });
                let owner = cx.entity().downgrade();
                permissions::open(
                    vec![chrome_card(request)],
                    stop.clone(),
                    Box::new(move |granted, _, cx| {
                        let _ = owner.update(cx, |this, _| {
                            if granted {
                                this.finish_scan(
                                    Ok(result.lock().unwrap().take().unwrap_or_default()),
                                    stop,
                                    reply,
                                );
                            } else {
                                this.busy = false;
                                let _ = reply.send(Ok(Value::Null));
                            }
                        });
                    }),
                    window,
                    cx,
                );
            }
            Action::Import(id) => {
                let Some(source) = self.profiles.get(id).cloned() else {
                    let _ = reply.send(Err("browser_chrome_missing".into()));
                    return;
                };
                self.busy = true;
                let prepared: Arc<Mutex<Option<super::chrome::Prepared>>> = Default::default();
                let imported: Arc<Mutex<Option<super::chrome::Import>>> = Default::default();
                let data = prepared.clone();
                let output = imported.clone();
                let request: PermissionAction = Rc::new(move |cx, cancel| {
                    let source = source.clone();
                    let data = data.clone();
                    let output = output.clone();
                    cx.background_executor().spawn(async move {
                        if cancel.is_cancelled() {
                            return Err(access_failure("permission_cancelled"));
                        }
                        let sites = super::chrome::sites(&source)
                            .map_err(access_failure)?
                            .into_iter()
                            .collect();
                        let value =
                            super::chrome::prepare(&source, &sites).map_err(access_failure)?;
                        let needs_key = value.needs_key();
                        if !needs_key {
                            *output.lock().unwrap() = Some(value.unlock().map_err(access_failure)?);
                        }
                        *data.lock().unwrap() = Some(value);
                        Ok(vec![
                            (Resource::Chrome, Status::Granted),
                            (
                                Resource::Keychain,
                                if needs_key {
                                    Status::Required
                                } else {
                                    Status::NotNeeded
                                },
                            ),
                        ])
                    })
                });
                let data = prepared.clone();
                let output = imported.clone();
                let keychain: PermissionAction = Rc::new(move |cx, cancel| {
                    let data = data.clone();
                    let output = output.clone();
                    cx.background_executor().spawn(async move {
                        if cancel.is_cancelled() {
                            return Err(access_failure("permission_cancelled"));
                        }
                        let guard = data.lock().unwrap();
                        let value = guard
                            .as_ref()
                            .ok_or_else(|| access_failure("browser_import_unavailable"))?;
                        *output.lock().unwrap() = Some(value.unlock().map_err(access_failure)?);
                        Ok(vec![(Resource::Keychain, Status::Granted)])
                    })
                });
                let owner = cx.entity().downgrade();
                permissions::open(
                    vec![
                        chrome_card(request),
                        Card {
                            resource: Resource::Keychain,
                            status: Status::Unknown,
                            settings: None,
                            check: None,
                            request: Some(keychain),
                            requires: Some(Resource::Chrome),
                        },
                    ],
                    stop.clone(),
                    Box::new(move |granted, _, cx| {
                        let _ = owner.update(cx, |this, cx| {
                            if !granted {
                                this.busy = false;
                                let _ = reply.send(Ok(Value::Null));
                                return;
                            }
                            let Some(import) = imported.lock().unwrap().take() else {
                                this.busy = false;
                                let _ = reply.send(Err("browser_import_failed".into()));
                                return;
                            };
                            this.install(import, stop, reply, cx);
                        });
                    }),
                    window,
                    cx,
                );
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn install(
        &mut self,
        import: super::chrome::Import,
        stop: CancellationToken,
        reply: Reply,
        cx: &mut Context<Self>,
    ) {
        let store = super::profile::store(cx);
        cx.spawn(async move |owner, cx| {
            let result = async {
                for cookies in import.cookies.chunks(64) {
                    if stop.is_cancelled() {
                        return Err("plugin view is closed".into());
                    }
                    super::profile::install(&store, cookies)
                        .await
                        .map_err(|_| "browser_import_failed".to_string())?;
                }
                let count = super::profile::verify(&store, &import.cookies)
                    .await
                    .map_err(|_| "browser_import_failed".to_string())?;
                Ok(json!({"count":count,"skipped":import.skipped + import.cookies.len() - count}))
            }
            .await;
            let _ = owner.update(cx, |this, _| this.busy = false);
            let _ = reply.send(result);
        })
        .detach();
    }

    #[cfg(target_os = "macos")]
    fn finish_scan(
        &mut self,
        result: super::chrome::Result<Vec<super::chrome::Profile>>,
        stop: CancellationToken,
        reply: Reply,
    ) {
        self.busy = false;
        if stop.is_cancelled() {
            let _ = reply.send(Err("plugin view is closed".into()));
        } else {
            let _ = reply.send(self.scanned(result));
        }
    }

    #[cfg(target_os = "macos")]
    fn scanned(
        &mut self,
        result: super::chrome::Result<Vec<super::chrome::Profile>>,
    ) -> Result<Value, String> {
        let profiles = result.map_err(str::to_owned)?;
        self.profiles.clear();
        let entries = profiles
            .into_iter()
            .map(|profile| {
                let id = sailry_protocol::RequestId::new().to_string();
                let entry = json!({"id":id,"name":profile.name});
                self.profiles.insert(id, profile);
                entry
            })
            .collect::<Vec<_>>();
        Ok(json!(entries))
    }
}

#[cfg(target_os = "macos")]
fn access_failure(key: &'static str) -> Failure {
    Failure {
        key: key.into(),
        status: if matches!(
            key,
            "browser_chrome_access_denied" | "browser_chrome_key_denied"
        ) {
            Status::Denied
        } else {
            Status::Unknown
        },
    }
}
#[cfg(target_os = "macos")]
fn chrome_card(request: PermissionAction) -> Card {
    Card {
        resource: Resource::Chrome,
        status: Status::Unknown,
        // App-data consent has no public preflight or dedicated pane URL. Do not claim Full Disk Access is required.
        settings: Some("x-apple.systempreferences:com.apple.preference.security"),
        check: None,
        request: Some(request),
        requires: None,
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests;
