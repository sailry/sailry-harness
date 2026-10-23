use super::*;
use sailry_protocol::{
    Fault, Secret,
    plugin::settings::{SecretUpdate, State},
};
use std::collections::BTreeMap;

mod view;

pub(super) struct Editor {
    binding: Binding,
    owner: Option<Entity<Workspace>>,
    original: Option<Info>,
    name: Entity<InputState>,
    command: Entity<InputState>,
    url: Entity<InputState>,
    args: Vec<Entity<InputState>>,
    slots: Vec<Slot>,
    next_slot: usize,
    transport: McpTransport,
    loading: bool,
    loaded: bool,
    closed: bool,
    dialog: crate::ui::overlays::Owned,
    submission: Submission,
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

struct Slot {
    id: usize,
    name: Entity<InputState>,
    value: Entity<InputState>,
    original: Option<String>,
    keepable: bool,
    configured: bool,
    clear: bool,
}

pub(super) fn open(
    binding: Binding,
    original: Option<Info>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Editor> {
    let editor = cx.new(|cx| {
        let definition = original.as_ref().and_then(|info| info.mcp_source.as_ref());
        let (command, args, url) = match definition {
            Some(Definition::Stdio { command, args, .. }) => {
                (command.clone(), args.clone(), String::new())
            }
            Some(Definition::StreamableHttp { url, .. } | Definition::Sse { url, .. }) => {
                (String::new(), Vec::new(), url.clone())
            }
            None => (String::new(), Vec::new(), String::new()),
        };
        let name = original
            .as_ref()
            .map(|info| display_name(&info.summary.name))
            .unwrap_or_default();
        let transport = definition.map_or(McpTransport::Stdio, Definition::transport);
        let slots: Vec<_> = definition
            .into_iter()
            .flat_map(Definition::slots)
            .enumerate()
            .map(|(id, name)| Slot::new(id, Some(name.clone()), window, cx))
            .collect();
        let next_slot = slots.len();
        for slot in &slots {
            slot.observe(cx);
        }
        let loaded = original.is_none();
        crate::feedback::observe(window, cx, |editor: &Editor, _| {
            editor.submission.error.into_iter().collect()
        });
        Editor {
            binding,
            owner: None,
            original,
            name: text_input(name, "mcp_name_hint", window, cx),
            command: text_input(command, "mcp_command_hint", window, cx),
            url: text_input(url, "mcp_url_hint", window, cx),
            args: args
                .into_iter()
                .map(|value| text_input(value, "mcp_argument", window, cx))
                .collect(),
            slots,
            next_slot,
            transport,
            loading: false,
            loaded,
            closed: false,
            dialog: Default::default(),
            submission: Submission::default(),
            task: None,
        }
    });
    editor.update(cx, |editor, cx| editor.load(window, cx));
    view::open(editor.clone(), window, cx);
    editor
}

fn text_input(
    value: String,
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> Entity<InputState> {
    cx.new(|cx| {
        InputState::new(window, cx)
            .default_value(value)
            .placeholder(tr(placeholder))
    })
}

impl Slot {
    fn observe(&self, cx: &mut Context<Editor>) {
        for input in [&self.name, &self.value] {
            cx.subscribe(input, |_: &mut Editor, _, event, cx| {
                if matches!(event, gpui_kit::component::input::InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
        }
    }
    fn new(id: usize, original: Option<String>, window: &mut Window, cx: &mut App) -> Self {
        Self {
            id,
            name: text_input(
                original.clone().unwrap_or_default(),
                "mcp_slot_name",
                window,
                cx,
            ),
            value: cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder(tr("form_secret_hint"))
            }),
            original,
            keepable: false,
            configured: false,
            clear: false,
        }
    }

    fn clear_value(&self, window: &mut Window, cx: &mut App) {
        // Kit set_value clears both the input and its undo history.
        self.value
            .update(cx, |input, cx| input.set_value("", window, cx));
    }
}

impl Editor {
    fn active(&self, cx: &App) -> bool {
        self.owner
            .as_ref()
            .is_none_or(|owner| current(owner, &self.binding, cx))
    }

    fn locked(&self) -> bool {
        self.loading || self.submission.pending || self.submission.request.is_some()
    }

    fn same_kind(&self) -> bool {
        self.original
            .as_ref()
            .and_then(|info| info.mcp_source.as_ref())
            .is_some_and(|source| {
                (source.transport() == McpTransport::Stdio)
                    == (self.transport == McpTransport::Stdio)
            })
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || !self.active(cx) || self.locked() {
            return;
        }
        let Some(info) = &self.original else {
            return;
        };
        let package = info.summary.reference();
        let binding = self.binding.clone();
        self.loading = true;
        let job = binding.runtime.spawn(async move {
            binding
                .client
                .execute(
                    binding
                        .client
                        .prepare(Command::ReadPluginSettings { package }),
                )
                .await
        });
        self.task = Some(cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            _ = editor.update_in(cx, |editor, _, cx| {
                if editor.closed || !editor.active(cx) {
                    return;
                }
                editor.loading = false;
                match result {
                    Ok(Ok(Output::PluginSettings(state))) => editor.accept(state),
                    Ok(Err(error)) => editor.failure(error),
                    _ => editor.submission.error = Some("plugins_settings_read_failed"),
                }
                cx.notify();
            });
        }));
    }

    fn accept(&mut self, state: State) {
        self.loaded = true;
        let Some(schema) = self
            .original
            .as_ref()
            .and_then(|info| info.settings.as_ref())
        else {
            return;
        };
        for slot in &mut self.slots {
            let field = schema.properties.iter().find_map(|(name, field)| {
                let binding = field.secret.as_ref()?;
                (binding.env.as_ref().or(binding.header.as_ref()) == slot.original.as_ref())
                    .then_some(name)
            });
            slot.keepable = field.is_some_and(|field| state.keepable.contains(field));
            slot.configured = field.is_some_and(|field| state.configured.contains(field));
        }
        self.submission.error = None;
        self.submission.detail = None;
    }

    fn add_slot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.locked() || self.slots.len() >= sailry_protocol::plugin::settings::MAX_FIELDS {
            return;
        }
        let slot = Slot::new(self.next_slot, None, window, cx);
        slot.observe(cx);
        self.slots.push(slot);
        self.next_slot += 1;
        cx.notify();
    }

    fn prepare(&self, cx: &App) -> Result<Command, &'static str> {
        let name = self
            .original
            .as_ref()
            .map(|info| info.summary.name.clone())
            .unwrap_or_else(|| format!("mcp-{}", self.name.read(cx).value().trim()));
        if name == "mcp-" {
            return Err("mcp_invalid");
        }
        let mut secrets = BTreeMap::new();
        let mut slots = Vec::new();
        let same_kind = self.same_kind();
        for slot in &self.slots {
            let name = slot.name.read(cx).value().trim().to_string();
            if name.is_empty() || secrets.contains_key(&name) {
                return Err("mcp_invalid");
            }
            let value = slot.value.read(cx).value();
            let update = if !value.is_empty() {
                SecretUpdate::Replace(Secret::new(value.to_string()))
            } else if !slot.clear
                && same_kind
                && slot.keepable
                && slot.original.as_ref() == Some(&name)
            {
                SecretUpdate::Keep
            } else {
                SecretUpdate::Clear
            };
            slots.push(name.clone());
            secrets.insert(name, update);
        }
        let definition = match self.transport {
            McpTransport::Stdio => {
                let command = self.command.read(cx).value().trim().to_string();
                if command.is_empty() {
                    return Err("mcp_invalid");
                }
                Definition::Stdio {
                    command,
                    args: self
                        .args
                        .iter()
                        .map(|arg| arg.read(cx).value().to_string())
                        .collect(),
                    env: slots,
                }
            }
            McpTransport::StreamableHttp | McpTransport::Sse => {
                let url = self.url.read(cx).value().trim().to_string();
                if url.is_empty() {
                    return Err("mcp_invalid");
                }
                if self.transport == McpTransport::StreamableHttp {
                    Definition::StreamableHttp {
                        url,
                        headers: slots,
                    }
                } else {
                    Definition::Sse {
                        url,
                        headers: slots,
                    }
                }
            }
        };
        Ok(Command::InstallMcp {
            name,
            expected_revision: self
                .original
                .as_ref()
                .map_or(0, |info| info.summary.revision),
            definition,
            secrets,
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
        let retry = cx.entity();
        let captured = self.submission.request.clone();
        let ticket = self.submission.request.as_ref().and_then(|request| {
            crate::installations::track(
                self.binding.client.clone(),
                self.binding.runtime.clone(),
                request,
                false,
                move |attempt, window, cx| {
                    if !retry.read(cx).active(cx) {
                        return;
                    }
                    let show = retry.update(cx, |editor, cx| {
                        editor.closed = false;
                        if !editor.submission.pending
                            && let Some(captured) = &captured
                        {
                            editor.submission.request =
                                Some(attempt.request(&editor.binding.client, captured));
                        }
                        !editor.dialog.presented(cx)
                    });
                    if show {
                        view::open(retry.clone(), window, cx);
                    }
                    retry.update(cx, |editor, cx| editor.save(window, cx));
                },
                window,
                cx,
            )
        });
        let origin = window.window_handle();
        self.task = Some(cx.spawn(async move |editor, cx| {
            let result = job.await;
            if let Some(ticket) = &ticket {
                cx.update(|cx| ticket.record(crate::installations::result(&result), cx));
            }
            let Some(target) = cx.update(|cx| crate::installations::target(origin, cx)) else {
                let _ = editor.update(cx, |editor, cx| {
                    editor.submission.pending = false;
                    if let Ok(Err(error)) = result {
                        editor.failure(error);
                    }
                    cx.notify();
                });
                return;
            };
            _ = target.update(cx, |_, window, cx| {
                editor.update(cx, |editor, cx| {
                    editor.submission.pending = false;
                    if editor.closed || !editor.active(cx) {
                        cx.notify();
                        return;
                    }
                    match result {
                        Ok(Ok(Output::Plugin(_))) => {
                            editor.submission.request = None;
                            editor.close(window, cx);
                            editor.dialog.close(window, cx);
                        }
                        Ok(Err(error)) => {
                            editor.failure(error);
                        }
                        _ => editor.submission.error = Some("plugins_unknown"),
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

    fn failure(&mut self, error: Fault) {
        let key = if error.code == sailry_protocol::ErrorCode::InvalidRequest {
            "mcp_invalid"
        } else {
            plugins::live::error_key(&error)
        };
        self.submission.failure(error, key);
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.closed = true;
        if self.submission.pending || self.submission.request.is_some() {
            return;
        }
        for slot in &self.slots {
            slot.clear_value(window, cx);
        }
    }
}
