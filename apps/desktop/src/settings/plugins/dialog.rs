use super::*;
use crate::settings::providers::Binding;
use gpui_kit::component::notification::Notification;
use sailry_protocol::{
    Command, Output, Request,
    plugin::{Origin, Summary, UploadSource},
};
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
pub(super) mod upload;

type Removed = Box<dyn FnOnce(&mut Window, &mut App)>;

pub(super) struct Editor {
    binding: Binding,
    original: Option<Summary>,
    origin: Option<Origin>,
    updating: bool,
    remove: bool,
    known: Vec<Summary>,
    package: Option<upload::Package>,
    loading: bool,
    stop: sailry_link::CancellationToken,
    error: Option<&'static str>,
    request: Option<Request>,
    pending: bool,
    closed: bool,
    on_removed: Option<Removed>,
    task: Option<Task<()>>,
}

pub(in crate::settings) fn open(
    owner: Entity<Workspace>,
    original: Option<Summary>,
    remove: bool,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(editor) = create(owner, original, remove, window, cx)
        && !remove
    {
        editor.update(cx, |editor, cx| editor.start(window, cx));
    }
}

fn create(
    owner: Entity<Workspace>,
    original: Option<Summary>,
    remove: bool,
    window: &mut Window,
    cx: &mut App,
) -> Option<Entity<Editor>> {
    let workspace = owner.read(cx);
    let Some(live) = &workspace.provider_link else {
        return None;
    };
    if !live.connected {
        return None;
    }
    let binding = live.binding.clone();
    let known = workspace.plugin_catalog.packages.clone();
    let origin = if !remove && let Some(original) = &original {
        let info = workspace
            .plugin_catalog
            .metadata
            .as_ref()?
            .read(cx)
            .entries
            .get(&original.name)?;
        if info.summary != *original {
            return None;
        }
        info.origin.clone()
    } else {
        None
    };
    if matches!(origin, Some(Origin::Bundled | Origin::Online { .. })) {
        owner.update(cx, |owner, cx| owner.check_plugin_updates(window, cx));
        return None;
    }
    let editor = new(binding, original, origin, remove, known, cx);
    if !remove {
        return Some(editor);
    }
    let result = editor.clone();
    editor.update(cx, |editor, cx| editor.prompt(window, cx));
    Some(result)
}

pub(super) fn remove(
    binding: Binding,
    original: Summary,
    window: &mut Window,
    cx: &mut App,
    on_removed: impl FnOnce(&mut Window, &mut App) + 'static,
) -> Entity<Editor> {
    let editor = new(binding, Some(original), None, true, Vec::new(), cx);
    editor.update(cx, |editor, cx| {
        editor.on_removed = Some(Box::new(on_removed));
        editor.prompt(window, cx);
    });
    editor
}

fn new(
    binding: Binding,
    original: Option<Summary>,
    origin: Option<Origin>,
    remove: bool,
    known: Vec<Summary>,
    cx: &mut App,
) -> Entity<Editor> {
    cx.new(|_| Editor {
        binding,
        updating: original.is_some(),
        original,
        origin,
        remove,
        known,
        package: None,
        loading: false,
        stop: sailry_link::CancellationToken::new(),
        error: None,
        request: None,
        pending: false,
        closed: false,
        on_removed: None,
        task: None,
    })
}

impl Editor {
    pub(super) fn pending(&self) -> bool {
        self.pending
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading || self.pending || self.request.is_some() || self.closed {
            return;
        }
        match &self.origin {
            Some(Origin::Worktree { .. }) => self.submit(window, cx),
            Some(Origin::Directory) => {
                let original = self
                    .original
                    .as_ref()
                    .expect("update has an installed package");
                if let Some(path) = crate::preferences::plugins::directory(
                    self.binding.client.target(),
                    &original.name,
                    &original.digest,
                    cx,
                ) {
                    self.load_directory(path, window, cx);
                } else {
                    self.choose(window, cx);
                }
            }
            _ => self.choose(window, cx),
        }
    }

    pub(super) fn prompt(&self, window: &mut Window, cx: &mut Context<Self>) {
        let plugin = self
            .original
            .as_ref()
            .expect("removal has a selected package");
        let detail = rust_i18n::t!("plugins_remove_effect", name = plugin.name);
        if let Some(error) = self.error {
            use gpui_kit::component::notification::Notification;
            crate::feedback::toast(
                window,
                tr(error),
                Notification::error(tr(error)).id1::<Self>(("error", cx.entity_id())),
                cx,
            );
        }
        let editor = cx.entity();
        crate::prompts::confirm(
            &tr("plugins_uninstall"),
            &detail,
            tr(if self.error.is_some() {
                "plugins_retry"
            } else {
                "plugins_uninstall"
            }),
            window,
            cx,
            move |window, cx| editor.update(cx, |editor, cx| editor.submit(window, cx)),
        );
    }

    fn prepare(&self) -> Result<Command, &'static str> {
        if self.remove {
            let plugin = self
                .original
                .as_ref()
                .expect("removal has a selected package");
            return Ok(Command::RemovePlugin {
                name: plugin.name.clone(),
                expected_revision: plugin.revision,
            });
        }
        let revision = self.original.as_ref().map_or(0, |plugin| plugin.revision);
        if let Some(original) = &self.original
            && let Some(Origin::Worktree { worktree, path }) = &self.origin
        {
            return Ok(Command::InstallPlugin {
                worktree: *worktree,
                path: path.clone(),
                name: original.name.clone(),
                expected_revision: revision,
                expected_digest: None,
            });
        }
        let package = self.package.as_ref().ok_or("plugins_choose_package")?;
        Ok(Command::InstallPluginUpload {
            stream: package.upload.stream,
            name: package.info.summary.name.clone(),
            expected_revision: revision,
            source: if package.directory.is_some() {
                UploadSource::Directory
            } else {
                UploadSource::Archive
            },
        })
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.closed {
            return;
        }
        if self.request.is_none() {
            match self.prepare() {
                Ok(command) => self.request = Some(self.binding.client.prepare(command)),
                Err(error) => {
                    self.error = Some(error);
                    cx.notify();
                    return;
                }
            }
        }
        let request = self.request.clone().expect("plugin request is prepared");
        let binding = self.binding.clone();
        let retry = cx.entity();
        let captured = request.clone();
        let ticket = crate::installations::track(
            binding.client.clone(),
            binding.runtime.clone(),
            &request,
            false,
            move |attempt, window, cx| {
                retry.update(cx, |editor, cx| {
                    editor.closed = false;
                    if !editor.pending {
                        editor.request = match attempt {
                            crate::installations::Attempt::New => None,
                            crate::installations::Attempt::Recover => Some(captured.clone()),
                        };
                    }
                    if editor.request.is_some() {
                        editor.submit(window, cx);
                    } else {
                        editor.start(window, cx);
                    }
                })
            },
            window,
            cx,
        );
        self.pending = true;
        self.error = None;
        let job = binding
            .runtime
            .spawn(async move { binding.client.execute(request).await });
        let keep_alive = cx.entity();
        let origin = window.window_handle();
        self.task = Some(cx.spawn(async move |editor, cx| {
            let _keep_alive = keep_alive;
            let result = job.await;
            if let Some(ticket) = &ticket {
                cx.update(|cx| ticket.record(crate::installations::result(&result), cx));
            }
            let Some(target) = cx.update(|cx| crate::installations::target(origin, cx)) else {
                let _ = editor.update(cx, |editor, cx| {
                    editor.pending = false;
                    if !crate::installations::result(&result)
                        .as_ref()
                        .is_err_and(live::uncertain)
                    {
                        editor.request = None;
                    }
                    cx.notify();
                });
                return;
            };
            let _ = target.update(cx, |_, window, cx| {
                editor.update(cx, |editor, cx| {
                    editor.pending = false;
                    if editor.closed {
                        return;
                    }
                    match result {
                        Ok(Ok(Output::Plugin(info))) if !editor.remove => {
                            let node = editor.binding.client.target();
                            if let Some(path) = editor
                                .package
                                .as_ref()
                                .and_then(|package| package.directory.clone())
                            {
                                crate::preferences::plugins::remember(
                                    node,
                                    &info.summary.name,
                                    &info.summary.digest,
                                    info.summary.revision,
                                    path,
                                    cx,
                                );
                            } else {
                                crate::preferences::plugins::forget(
                                    node,
                                    &info.summary.name,
                                    info.summary.revision,
                                    cx,
                                );
                            }
                            editor.closed = true;
                            editor.request = None;
                            crate::feedback::status(
                                window,
                                tr(if editor.original.is_some() {
                                    "plugins_updated_success"
                                } else {
                                    "plugins_installed_success"
                                }),
                                gpui_kit::component::notification::NotificationType::Success,
                                Notification::success(tr(if editor.original.is_some() {
                                    "plugins_updated_success"
                                } else {
                                    "plugins_installed_success"
                                }))
                                .id1::<Editor>(("package", cx.entity_id())),
                                cx,
                            );
                            editor.package = None;
                        }
                        Ok(Ok(Output::Plugins(_))) if editor.remove => {
                            if let Some(original) = &editor.original {
                                crate::preferences::plugins::forget(
                                    editor.binding.client.target(),
                                    &original.name,
                                    original.revision,
                                    cx,
                                );
                            }
                            editor.closed = true;
                            editor.request = None;
                            if let Some(on_removed) = editor.on_removed.take() {
                                on_removed(window, cx);
                            }
                        }
                        Ok(Err(error)) => {
                            if !live::uncertain(&error) {
                                editor.request = None;
                            }
                            editor.error = Some(live::error_key(&error));
                            if editor.package.is_some()
                                && error.code == sailry_protocol::ErrorCode::NotFound
                            {
                                editor.package = None;
                                editor.error = Some("plugins_package_expired");
                            }
                        }
                        _ => editor.error = Some("plugins_unknown"),
                    }
                    if !editor.remove {
                        editor.report(window, cx);
                    } else if editor.error.is_some() {
                        editor.prompt(window, cx);
                    }
                    cx.notify();
                })
            });
            if let Some(ticket) = &ticket {
                let _ = target.update(cx, |_, window, cx| ticket.close_empty(window, cx));
            }
        }));
        cx.notify();
    }

    fn report(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(error) = self.error else { return };
        let owner = cx.entity();
        crate::feedback::toast(
            window,
            tr(error),
            Notification::error(tr(error))
                .id1::<Editor>(("package", cx.entity_id()))
                .action(move |_, _, _| {
                    let owner = owner.clone();
                    crate::feedback::retry("plugin-retry")
                        .debug_selector(|| "plugin-retry".into())
                        .on_click(move |_, window, cx| {
                            owner.update(cx, |editor, cx| {
                                if editor.request.is_some() {
                                    editor.submit(window, cx);
                                } else {
                                    editor.start(window, cx);
                                }
                            })
                        })
                }),
            cx,
        );
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
