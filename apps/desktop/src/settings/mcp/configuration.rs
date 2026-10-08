//! A standard MCP document draft stays bound to its execution Node and package.
use super::*;
use gpui_kit::component::input::EditorState;
use sailry_link::CancellationToken;
use sailry_protocol::{Fault, plugin::Reference};

#[cfg(test)]
mod tests;
mod view;

pub(super) struct Editor {
    binding: Binding,
    package: Reference,
    document: Entity<EditorState>,
    loading: bool,
    loaded: bool,
    closed: bool,
    submission: Submission,
    stop: CancellationToken,
    task: Option<Task<()>>,
}

impl Drop for Editor {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub(super) fn open(
    binding: Binding,
    info: Info,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Editor> {
    let editor = cx.new(|cx| {
        crate::feedback::observe(window, cx, |editor: &Editor, _| {
            editor.submission.error.into_iter().collect()
        });
        Editor {
            binding,
            package: info.summary.reference(),
            document: cx.new(|cx| {
                EditorState::new(window, cx)
                    .language("json")
                    .line_number(true)
            }),
            loading: false,
            loaded: false,
            closed: false,
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
    fn locked(&self) -> bool {
        self.loading || !self.loaded || self.submission.pending || self.submission.request.is_some()
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed
            || self.loading
            || self.submission.pending
            || self.submission.request.is_some()
        {
            return;
        }
        let binding = self.binding.clone();
        let package = self.package.clone();
        let stop = self.stop.clone();
        self.loading = true;
        self.submission.error = None;
        self.submission.detail = None;
        let job = binding.runtime.spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => None,
                result = binding.client.execute(binding.client.prepare(Command::ReadPluginMcp { package })) => Some(result),
            }
        });
        self.task = Some(cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            _ = editor.update_in(cx, |editor, window, cx| {
                if editor.closed {
                    return;
                }
                editor.loading = false;
                match result {
                    Ok(Some(Ok(Output::PluginMcp(state)))) if state.package == editor.package => {
                        match serde_json::to_string_pretty(&state.configuration) {
                            Ok(source) => {
                                editor.document.update(cx, |document, cx| {
                                    document.set_value(source, window, cx)
                                });
                                editor.loaded = true;
                            }
                            Err(_) => {
                                editor.submission.error = Some("plugins_settings_read_failed")
                            }
                        }
                    }
                    Ok(Some(Ok(_))) => editor.submission.error = Some("plugins_settings_conflict"),
                    Ok(Some(Err(error))) => editor.failure(error),
                    _ => editor.submission.error = Some("plugins_settings_read_failed"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn prepare(&self, cx: &App) -> Result<Command, &'static str> {
        let configuration = serde_json::from_str(self.document.read(cx).value().as_str())
            .map_err(|_| "mcp_configuration_invalid")?;
        Ok(Command::SavePluginMcp {
            package: self.package.clone(),
            configuration,
        })
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.submission.pending || self.loading || self.closed || !self.loaded {
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
                if editor.closed {
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
                        window.close_dialog(cx);
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
