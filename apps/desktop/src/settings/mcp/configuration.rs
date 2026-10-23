//! A standard MCP document draft stays bound to its execution Node and package.
use super::*;
use gpui_kit::component::input::EditorState;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Fault,
    plugin::{
        Reference,
        mcp::{Configuration, Server},
    },
};

mod view;

pub(super) struct Editor {
    binding: Binding,
    owner: Option<Entity<Workspace>>,
    package: Reference,
    server: Option<String>,
    original: Option<Configuration>,
    document: Entity<EditorState>,
    loading: bool,
    loaded: bool,
    closed: bool,
    dialog: crate::ui::overlays::Owned,
    submission: Submission,
    stop: CancellationToken,
    task: Option<Task<()>>,
}

pub(super) fn bind_owner(
    editor: &Entity<Editor>,
    owner: &Entity<Workspace>,
    window: &mut Window,
    cx: &mut App,
) {
    editor.update(cx, |editor, cx| {
        editor.owner = Some(owner.clone());
        cx.observe_in(owner, window, |editor, _, window, cx| {
            if !editor.active(cx) {
                editor.close(window, cx);
                editor.dialog.close(window, cx);
            }
        })
        .detach();
        if !editor.active(cx) {
            editor.close(window, cx);
            editor.dialog.close(window, cx);
        }
    });
}

impl Drop for Editor {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub(super) fn open_server(
    binding: Binding,
    info: Info,
    server: String,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Editor> {
    mount(binding, info, Some(server), window, cx)
}

fn mount(
    binding: Binding,
    info: Info,
    server: Option<String>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Editor> {
    let editor = cx.new(|cx| {
        crate::feedback::observe(window, cx, |editor: &Editor, _| {
            editor.submission.error.into_iter().collect()
        });
        let document = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("json")
                .line_number(true)
        });
        cx.observe(&document, |_, _, cx| cx.notify()).detach();
        Editor {
            binding,
            owner: None,
            package: info.summary.reference(),
            server,
            original: None,
            document,
            loading: false,
            loaded: false,
            closed: false,
            dialog: Default::default(),
            submission: Submission::default(),
            stop: CancellationToken::new(),
            task: None,
        }
    });
    editor.update(cx, |editor, cx| editor.load(window, cx));
    view::open(editor.clone(), window, cx);
    editor
}

impl Editor {
    fn active(&self, cx: &App) -> bool {
        self.owner
            .as_ref()
            .is_none_or(|owner| current(owner, &self.binding, cx))
    }

    fn locked(&self) -> bool {
        self.loading || !self.loaded || self.submission.pending || self.submission.request.is_some()
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed
            || !self.active(cx)
            || self.loading
            || self.submission.pending
            || self.submission.request.is_some()
        {
            return;
        }
        let binding = self.binding.clone();
        let package = self.package.clone();
        let stop = self.stop.clone();
        let refresh = self.loaded || self.submission.error == Some("plugins_settings_conflict");
        let preserve = self.loaded;
        self.loading = true;
        self.submission.error = None;
        self.submission.detail = None;
        let job = binding.runtime.spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => None,
                result = async {
                    let package = if refresh {
                        let output = binding.client.execute(binding.client.prepare(Command::ReadPlugin { name: package.name.clone() })).await?;
                        let Output::Plugin(info) = output else {
                            return Err(Fault::invalid("plugin information expected"));
                        };
                        if info.summary.name != package.name {
                            return Err(Fault::invalid("plugin information changed"));
                        }
                        info.summary.reference()
                    } else { package };
                    let output = binding.client.execute(binding.client.prepare(Command::ReadPluginMcp { package: package.clone() })).await?;
                    let Output::PluginMcp(state) = output else {
                        return Err(Fault::invalid("MCP configuration expected"));
                    };
                    if state.package != package {
                        return Err(Fault::new(sailry_protocol::ErrorCode::RevisionConflict, "plugin configuration changed"));
                    }
                    Ok(state)
                } => Some(result),
            }
        });
        self.task = Some(cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            _ = editor.update_in(cx, |editor, window, cx| {
                if editor.closed || !editor.active(cx) {
                    return;
                }
                editor.loading = false;
                match result {
                    Ok(Some(Ok(state))) => {
                        match document(&state.configuration, editor.server.as_deref()) {
                            Ok(source) => {
                                if !preserve {
                                    editor.document.update(cx, |document, cx| {
                                        document.set_value(source, window, cx)
                                    });
                                }
                                editor.package = state.package;
                                editor.loaded = true;
                                editor.original = Some(state.configuration);
                            }
                            Err(_) => {
                                editor.submission.error = Some("plugins_settings_read_failed")
                            }
                        }
                    }
                    Ok(Some(Err(error))) => editor.failure(error),
                    _ => editor.submission.error = Some("plugins_settings_read_failed"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn prepare(&self, cx: &App) -> Result<Command, &'static str> {
        let configuration = merge(
            self.original.as_ref(),
            self.server.as_deref(),
            self.document.read(cx).value().as_str(),
        )?;
        Ok(Command::SavePluginMcp {
            package: self.package.clone(),
            configuration,
        })
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.submission.pending
            || self.loading
            || self.closed
            || !self.active(cx)
            || !self.loaded
        {
            return;
        }
        let command = self.submission.request.is_none().then(|| self.prepare(cx));
        let Some(job) = self.submission.begin(&self.binding, command) else {
            cx.notify();
            return;
        };
        self.task = Some(cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            _ = editor.update_in(cx, |editor, window, cx| {
                editor.submission.pending = false;
                if editor.closed || !editor.active(cx) {
                    return;
                }
                match result {
                    Ok(Ok(Output::Plugin(info))) if info.summary.name == editor.package.name => {
                        crate::feedback::toast(
                            window,
                            tr("mcp_configuration_saved"),
                            notification::Notification::success(tr("mcp_configuration_saved")),
                            cx,
                        );
                        editor.close(window, cx);
                        editor.dialog.close(window, cx);
                    }
                    Ok(Err(error)) => {
                        editor.failure(error);
                    }
                    _ => editor.submission.error = Some("plugins_unknown"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn failure(&mut self, error: Fault) {
        let key = match error.code {
            sailry_protocol::ErrorCode::InvalidRequest => "mcp_configuration_invalid",
            sailry_protocol::ErrorCode::RevisionConflict => "plugins_settings_conflict",
            _ => plugins::live::error_key(&error),
        };
        self.submission.failure(error, key);
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.closed = true;
        self.stop.cancel();
        self.submission.request = None;
        // Kit resets the value and its undo history together.
        self.document
            .update(cx, |document, cx| document.set_value("", window, cx));
    }
}

fn document(configuration: &Configuration, server: Option<&str>) -> Result<String, &'static str> {
    match server {
        Some(server) => serde_json::to_string_pretty(
            configuration
                .servers
                .get(server)
                .ok_or("plugins_settings_conflict")?,
        ),
        None => serde_json::to_string_pretty(configuration),
    }
    .map_err(|_| "mcp_configuration_invalid")
}

fn merge(
    original: Option<&Configuration>,
    server: Option<&str>,
    source: &str,
) -> Result<Configuration, &'static str> {
    let Some(server) = server else {
        return serde_json::from_str(source).map_err(|_| "mcp_configuration_invalid");
    };
    let selected: Server = serde_json::from_str(source).map_err(|_| "mcp_configuration_invalid")?;
    let mut configuration = original.cloned().ok_or("plugins_settings_conflict")?;
    let target = configuration
        .servers
        .get_mut(server)
        .ok_or("plugins_settings_conflict")?;
    *target = selected;
    Ok(configuration)
}

#[cfg(test)]
mod tests;
