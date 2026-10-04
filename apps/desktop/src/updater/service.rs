use super::{
    Failure, Result,
    config::Config,
    install,
    manifest::Selection,
    recovery,
    transfer::{self, Progress, Staged},
};
use crate::{
    backend::{Lifecycle, Services},
    tr,
};
use gpui_kit::{
    component::{WindowExt, button::Button, progress::Progress as Indicator},
    prelude::FluentBuilder as _,
    *,
};
use sailry_link::CancellationToken;
use std::{fs, path::PathBuf, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum State {
    Idle,
    Checking,
    Available(String),
    Downloading(Progress),
    Ready(String),
    Installing,
    Restarting,
    Stopped,
}

pub(super) struct Notice {
    pub key: &'static str,
    pub error: bool,
}
pub(super) struct Service {
    pub state: State,
    pub failure: Option<Failure>,
    #[cfg(test)]
    pub(super) fixture: Option<Config>,
    selection: Option<Selection>,
    config: Option<Config>,
    staged: Option<Arc<Staged>>,
    restart: Option<(PathBuf, Vec<std::ffi::OsString>)>,
    stop: CancellationToken,
    task: Option<Task<()>>,
}
impl EventEmitter<Notice> for Service {}
impl Drop for Service {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

struct Shared(Entity<Service>);
impl Global for Shared {}

pub(crate) fn init(cx: &mut App) {
    recovery::init(cx);
    Service::shared(cx);
}

enum Update {
    Checked(Result<(Config, Option<Selection>)>),
    Progress(Progress),
    Downloaded(Result<Staged>),
}

impl Service {
    #[cfg(test)]
    pub(super) fn cancelled(&self) -> bool {
        self.stop.is_cancelled()
    }
    pub(super) fn shared(cx: &mut App) -> Entity<Self> {
        if !cx.has_global::<Shared>() {
            let service = cx.new(|_| Self {
                state: State::Idle,
                failure: None,
                #[cfg(test)]
                fixture: None,
                selection: None,
                config: None,
                staged: None,
                restart: None,
                stop: CancellationToken::new(),
                task: None,
            });
            cx.set_global(Shared(service));
        }
        cx.global::<Shared>().0.clone()
    }

    pub fn check(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.state, State::Idle | State::Available(_)) {
            return;
        }
        let Some(services) = cx.try_global::<Services>().cloned() else {
            self.notice("updates_preview_unavailable", true, cx);
            return;
        };
        self.stop = CancellationToken::new();
        self.failure = None;
        let stop = self.stop.clone();
        self.state = State::Checking;
        #[cfg(test)]
        let configured = self.fixture.clone();
        let job = services.runtime.spawn(async move {
            #[cfg(test)]
            let config = configured.map(Ok).unwrap_or_else(Config::release)?;
            #[cfg(not(test))]
            let config = Config::release()?;
            let selection = transfer::check(transfer::client()?, &config, &stop).await?;
            Ok((config, selection))
        });
        self.task = Some(cx.spawn(async move |owner, cx| {
            let result = job.await.unwrap_or_else(|_| {
                Err(Failure::new(
                    "updates_network_failed",
                    "update check task failed",
                ))
            });
            let _ = owner.update(cx, |this, cx| this.accept(Update::Checked(result), cx));
        }));
        cx.notify();
    }

    pub fn download(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.state, State::Available(_)) {
            return;
        }
        let (Some(config), Some(selection), Some(services)) = (
            self.config.clone(),
            self.selection.clone(),
            cx.try_global::<Services>().cloned(),
        ) else {
            return;
        };
        let Some(root) = directory(cx) else {
            self.notice("updates_io_failed", true, cx);
            return;
        };
        self.stop = CancellationToken::new();
        self.failure = None;
        let stop = self.stop.clone();
        self.state = State::Downloading(Progress::Downloading {
            copied: 0,
            total: Some(selection.release.size),
        });
        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
        services.runtime.spawn(async move {
            let changed = sender.clone();
            let result = async {
                transfer::download(
                    transfer::client()?,
                    config,
                    selection,
                    &root,
                    stop.clone(),
                    Arc::new(move |progress| {
                        let _ = changed.try_send(Update::Progress(progress));
                    }),
                )
                .await
            }
            .await;
            let _ = sender.send(Update::Downloaded(result)).await;
        });
        self.task = Some(cx.spawn(async move |owner, cx| {
            while let Some(update) = receiver.recv().await {
                let ended = matches!(update, Update::Downloaded(_));
                if owner
                    .update(cx, |this, cx| this.accept(update, cx))
                    .is_err()
                    || ended
                {
                    break;
                }
            }
        }));
        cx.notify();
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if matches!(self.state, State::Checking | State::Downloading(_)) {
            self.stop.cancel();
        }
        cx.notify();
    }

    fn accept(&mut self, update: Update, cx: &mut Context<Self>) {
        match update {
            Update::Progress(value) => self.state = State::Downloading(value),
            Update::Checked(Ok((config, selection))) => {
                self.config = Some(config);
                self.state = selection
                    .as_ref()
                    .map(|value| State::Available(value.release.version.clone()))
                    .unwrap_or(State::Idle);
                if selection.is_none() {
                    self.notice("updates_current", false, cx);
                }
                self.selection = selection;
                self.task = None;
            }
            Update::Downloaded(Ok(staged)) => {
                self.state = State::Ready(staged.selection.release.version.clone());
                self.staged = Some(Arc::new(staged));
                self.task = None;
            }
            Update::Checked(Err(error)) | Update::Downloaded(Err(error)) => {
                self.state = self
                    .selection
                    .as_ref()
                    .map(|value| State::Available(value.release.version.clone()))
                    .unwrap_or(State::Idle);
                self.task = None;
                if error.key != "updates_cancelled" {
                    eprintln!("desktop update failed: {error}");
                    self.failure = Some(error.clone());
                    self.notice(error.key, true, cx);
                }
            }
        }
        cx.notify();
    }

    pub(super) fn install(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.state, State::Ready(_)) {
            return;
        }
        let result = self.begin_install(window, cx);
        if let Err(error) = result {
            eprintln!("desktop update preparation failed: {error}");
            self.failure = Some(error.clone());
            self.notice(error.key, true, cx);
            cx.notify();
        }
    }

    fn begin_install(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<()> {
        let staged = self.staged.clone().ok_or_else(|| {
            Failure::new(
                "updates_package_invalid",
                "no authenticated staged update is available",
            )
        })?;
        let config = self.config.clone().ok_or_else(|| {
            Failure::new(
                "updates_source_unready",
                "update trust configuration is unavailable",
            )
        })?;
        let installed = install::installed()?;
        let root = directory(cx).ok_or_else(|| {
            Failure::new(
                "updates_io_failed",
                "the controller profile has no cache directory",
            )
        })?;
        let lifecycle = cx.global::<Lifecycle>().clone();
        install::separate_data(&installed, lifecycle.profile(), &root)?;
        let capture = recovery::capture(cx)?;
        let services = cx.global::<Services>().clone();
        let barriers = freeze(window, cx);
        self.failure = None;
        self.state = State::Installing;
        let arguments: Vec<_> = std::env::args_os().skip(1).collect();
        self.restart = Some((
            installed.join(&staged.selection.release.executable),
            arguments.clone(),
        ));
        let job = services.runtime.spawn(async move {
            let prepare = tokio::task::spawn_blocking(move || {
                super::manifest::revalidate(&staged.selection, &config)?;
                super::package::verify_archive(&staged.archive, &staged.selection.release, &config.keys)?;
                super::package::validate(&staged.directory.path().join("contents").join(&staged.selection.release.bundle), &staged.selection.release)?;
                fs::create_dir_all(&root).map_err(Failure::io)?;
                let receipt = root.join("receipt.json");
                if receipt.exists() {
                    return Err(Failure::new(
                        "updates_receipt_unread",
                        "an earlier update receipt has not been acknowledged",
                    ));
                }
                capture.save(&root)?;
                #[cfg(target_os = "windows")]
                {
                    let proof = serde_json::to_value(&staged.selection).map_err(|_| {
                        Failure::new(
                            "updates_package_invalid",
                            "the staged update proof could not be serialized",
                        )
                    })?;
                    let helper = super::windows::prepare(
                        &staged.archive,
                        proof,
                        &installed,
                        std::path::Path::new(&staged.selection.release.executable),
                        &arguments,
                        &receipt,
                    )
                    .map_err(|detail| Failure::new("updates_install_failed", detail))?;
                    Ok((helper, receipt))
                }
                #[cfg(not(target_os = "windows"))]
                {
                    Ok((staged, config, installed, arguments, receipt))
                }
            })
            .await
            .map_err(|_| Failure::new("updates_io_failed", "update preparation task failed"))
            .and_then(|result| result);
            let prepare = match prepare { Ok(prepare) => prepare, Err(error) => return (false, Err(error)) };
            if let Err(detail) = lifecycle.stop().await { return (true, Err(Failure::new("updates_shutdown_failed", detail))); }
            let result = tokio::task::spawn_blocking(move || {
                #[cfg(target_os = "windows")]
                {
                    prepare
                        .0
                        .launch()
                        .map_err(|detail| Failure::new("updates_install_failed", detail))
                }
                #[cfg(target_os = "macos")]
                {
                    let (staged, config, installed, arguments, receipt) = prepare;
                    let backup = match install::replace(&staged, &config, &installed) {
                        Ok(backup) => backup,
                        Err(error) => {
                            let _ = super::receipt::write(&receipt, &serde_json::json!({"version":1,"result":"failure","message":&error.detail,"uncertain":error.key == "updates_install_uncertain"}));
                            return Err(error);
                        }
                    };
                    super::receipt::write(
                        &receipt,
                        &serde_json::json!({"version":1,"result":"success","backup":&backup}),
                    ).map_err(|error| Failure::new("updates_receipt_failed", format!("application was installed but the update receipt could not be saved; original retained at {}: {error}", backup.display())))?;
                    install::restart(
                        &installed.join(&staged.selection.release.executable),
                        &arguments,
                    )
                }
                #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                {
                    let _ = prepare;
                    Err(Failure::new(
                        "updates_platform",
                        "desktop updates are supported on macOS and Windows",
                    ))
                }
            })
            .await
            .map_err(|_| {
                Failure::new("updates_install_failed", "update installation task failed")
            }).and_then(|result| result);
            (true, result)
        });
        self.task = Some(cx.spawn(async move |owner, cx| {
            let (stopped, result) = job.await.unwrap_or_else(|_| {
                (
                    true,
                    Err(Failure::new(
                        "updates_install_failed",
                        "update handoff task failed",
                    )),
                )
            });
            let _ = owner.update(cx, |this, cx| {
                if result.is_ok() {
                    cx.quit();
                    return;
                }
                if stopped {
                    // Keep every composer frozen after the local Node drained;
                    // the saved drafts must remain the final pre-restart input.
                    this.state = State::Stopped;
                } else {
                    thaw(&barriers, cx);
                    this.state = this
                        .staged
                        .as_ref()
                        .map(|staged| State::Ready(staged.selection.release.version.clone()))
                        .unwrap_or(State::Idle);
                }
                this.task = None;
                if let Err(error) = result {
                    eprintln!("desktop update install failed: {error}");
                    this.failure = Some(error.clone());
                    this.notice(error.key, true, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
        Ok(())
    }

    pub fn restart(&mut self, cx: &mut Context<Self>) {
        if self.state != State::Stopped {
            return;
        }
        let Some((executable, arguments)) = self.restart.clone() else {
            self.notice("updates_restart_failed", true, cx);
            return;
        };
        let lifecycle = cx.global::<Lifecycle>().clone();
        self.state = State::Restarting;
        let job = cx.global::<Services>().runtime.spawn(async move {
            lifecycle
                .stop()
                .await
                .map_err(|detail| Failure::new("updates_shutdown_failed", detail))?;
            install::restart(&executable, &arguments)
        });
        self.task = Some(cx.spawn(async move |owner, cx| {
            let result = job.await.unwrap_or_else(|_| {
                Err(Failure::new(
                    "updates_restart_failed",
                    "restart task failed",
                ))
            });
            let _ = owner.update(cx, |this, cx| match result {
                Ok(()) => cx.quit(),
                Err(error) => {
                    this.state = State::Stopped;
                    this.task = None;
                    this.failure = Some(error.clone());
                    this.notice(error.key, true, cx);
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    fn notice(&self, key: &'static str, error: bool, cx: &mut Context<Self>) {
        cx.emit(Notice { key, error });
    }
}

pub(super) fn directory(cx: &App) -> Option<PathBuf> {
    cx.try_global::<crate::preferences::Preferences>()?
        .directory()
        .map(|path| path.join("updater"))
}

fn freeze(window: &mut Window, cx: &mut App) -> Vec<AnyWindowHandle> {
    let handles = cx.windows();
    let current = window.window_handle();
    barrier(window, cx);
    for handle in handles.iter().filter(|handle| **handle != current) {
        let _ = handle.update(cx, |_, window, cx| barrier(window, cx));
    }
    handles
}

fn barrier(window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, |dialog, _, cx| {
        let service = cx.global::<Shared>().0.clone();
        let state = service.read(cx).state.clone();
        let content = if state == State::Stopped {
            gpui_kit::component::v_flex()
                .gap_2()
                .when_some(service.read(cx).failure.clone(), |content, error| {
                    content.child(
                        Button::new("update-handoff-details")
                            .label(tr("updates_details"))
                            .on_click(move |_, window, cx| {
                                crate::ui::details::open(
                                    tr("updates_details").to_string(),
                                    String::new(),
                                    error.detail.clone(),
                                    window,
                                    cx,
                                )
                            }),
                    )
                })
                .child(
                    Button::new("update-handoff-restart")
                        .label(tr("updates_restart"))
                        .on_click(move |_, _, cx| {
                            service.update(cx, |service, cx| service.restart(cx))
                        }),
                )
                .into_any_element()
        } else {
            Indicator::new("update-install-progress")
                .loading(true)
                .into_any_element()
        };
        dialog
            .title(tr(match state {
                State::Stopped => "updates_restart",
                State::Restarting => "updates_restarting",
                _ => "updates_installing",
            }))
            .close_button(false)
            .overlay_closable(false)
            .keyboard(false)
            .on_ok(|_, _, _| false)
            .on_cancel(|_, _, _| false)
            .child(content)
    });
}

fn thaw(handles: &[AnyWindowHandle], cx: &mut App) {
    for handle in handles {
        let _ = handle.update(cx, |_, window, cx| window.close_dialog(cx));
    }
}
