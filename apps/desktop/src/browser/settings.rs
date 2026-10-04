//! Controller-local profile import keeps paths, keychain data, and cookies native.
use gpui_kit::*;
use sailry_link::CancellationToken;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeMap};

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
        cx.subscribe_in(&cx.entity(), window, |this, _, request: &Request, _, cx| {
            let Some(reply) = request.reply.borrow_mut().take() else {
                return;
            };
            if this.busy || request.stop.is_cancelled() || !enabled(cx) {
                let _ = reply.send(Err("browser_import_unavailable".into()));
                return;
            }
            #[cfg(target_os = "macos")]
            this.start(&request.action, request.stop.clone(), reply, cx);
            #[cfg(not(target_os = "macos"))]
            let _ = reply.send(Err("browser_import_unavailable".into()));
        })
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
        cx: &mut Context<Self>,
    ) {
        match action {
            Action::Scan => {
                self.busy = true;
                let task = cx
                    .background_executor()
                    .spawn(async { super::chrome::profiles() });
                cx.spawn(async move |owner, cx| {
                    let result = task.await;
                    let _ = owner.update(cx, |this, cx| this.finish_scan(result, stop, reply, cx));
                })
                .detach();
            }
            Action::Import(id) => {
                let Some(source) = self.profiles.get(id).cloned() else {
                    let _ = reply.send(Err("browser_chrome_missing".into()));
                    return;
                };
                self.busy = true;
                let store = super::profile::store(cx);
                let task = cx.background_executor().spawn(async move {
                    let sites = super::chrome::sites(&source)?.into_iter().collect();
                    super::chrome::import(&source, &sites)
                });
                cx.spawn(async move |owner, cx| {
                    let result = async {
                        let import = task.await.map_err(str::to_owned)?;
                        for cookies in import.cookies.chunks(64) {
                            if stop.is_cancelled() { return Err("plugin view is closed".into()); }
                            super::profile::install(&store, cookies).await.map_err(|_| "browser_import_failed".to_string())?;
                        }
                        let count = super::profile::verify(&store, &import.cookies).await.map_err(|_| "browser_import_failed".to_string())?;
                        Ok(json!({"count":count,"skipped":import.skipped + import.cookies.len() - count}))
                    }.await;
                    let _ = owner.update(cx, |this, _| this.busy = false);
                    let _ = reply.send(result);
                }).detach();
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn finish_scan(
        &mut self,
        result: super::chrome::Result<Vec<super::chrome::Profile>>,
        stop: CancellationToken,
        reply: Reply,
        cx: &mut Context<Self>,
    ) {
        self.busy = false;
        if stop.is_cancelled() {
            let _ = reply.send(Err("plugin view is closed".into()));
        } else if matches!(result, Err("browser_chrome_access_denied")) {
            self.choose(stop, reply, cx);
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

    #[cfg(target_os = "macos")]
    fn choose(&mut self, stop: CancellationToken, reply: Reply, cx: &mut Context<Self>) {
        // macOS protects other apps' data. NSOpenPanel grants only the directory
        // the user chooses; no privacy settings are changed (WWDC23 10053).
        self.busy = true;
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(crate::tr("browser_chrome_folder")),
        });
        let executor = cx.background_executor().clone();
        cx.spawn(async move |owner, cx| {
            let result = match selected.await {
                Ok(Ok(Some(paths))) if !stop.is_cancelled() => match paths.into_iter().next() {
                    Some(path) => executor
                        .spawn(async move { super::chrome::discover(&path) })
                        .await
                        .map(Some),
                    None => Ok(None),
                },
                Ok(Ok(None)) => Ok(None),
                _ => Err("browser_chrome_access_denied"),
            };
            let result = owner
                .update(cx, |this, _| {
                    this.busy = false;
                    if stop.is_cancelled() {
                        return Err("plugin view is closed".into());
                    }
                    match result {
                        Ok(Some(profiles)) => this.scanned(Ok(profiles)),
                        Ok(None) => Ok(Value::Null),
                        Err(error) => Err(error.into()),
                    }
                })
                .unwrap_or_else(|_| Err("plugin view is closed".into()));
            let _ = reply.send(result);
        })
        .detach();
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests;
