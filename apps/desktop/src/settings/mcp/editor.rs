use super::*;
use sailry_protocol::{
    Fault, Secret,
    plugin::settings::{SecretUpdate, State},
};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;
mod view;

pub(super) struct Editor {
    binding: Binding,
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
    pending: bool,
    closed: bool,
    request: Option<Request>,
    error: Option<&'static str>,
    detail: Option<String>,
    details: bool,
    task: Option<Task<()>>,
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
            editor.error.into_iter().collect()
        });
        Editor {
            binding,
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
            pending: false,
            closed: false,
            request: None,
            error: None,
            detail: None,
            details: false,
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
    fn locked(&self) -> bool {
        self.loading || self.pending || self.request.is_some()
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
        if self.closed || self.locked() {
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
                if editor.closed {
                    return;
                }
                editor.loading = false;
                match result {
                    Ok(Ok(Output::PluginSettings(state))) => editor.accept(state),
                    Ok(Err(error)) => editor.failure(error),
                    _ => editor.error = Some("plugins_settings_read_failed"),
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
        self.error = None;
        self.detail = None;
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
        if self.pending || self.loading || self.closed || !self.loaded {
            return;
        }
        if self.request.is_none() {
            match self.prepare(cx) {
                Ok(command) => self.request = Some(self.binding.client.prepare(command)),
                Err(error) => {
                    self.error = Some(error);
                    cx.notify();
                    return;
                }
            }
        }
        let request = self.request.clone().expect("MCP request is prepared");
        let binding = self.binding.clone();
        self.pending = true;
        self.error = None;
        self.detail = None;
        let job = binding
            .runtime
            .spawn(async move { binding.client.execute(request).await });
        self.task = Some(cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            _ = editor.update_in(cx, |editor, window, cx| {
                editor.pending = false;
                if editor.closed {
                    return;
                }
                match result {
                    Ok(Ok(Output::Plugin(_))) => {
                        editor.close(window, cx);
                        window.close_dialog(cx);
                    }
                    Ok(Err(error)) => {
                        if !plugins::live::uncertain(&error) {
                            editor.request = None;
                        }
                        editor.failure(error);
                    }
                    _ => editor.error = Some("plugins_unknown"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn failure(&mut self, error: Fault) {
        self.error = Some(
            if error.code == sailry_protocol::ErrorCode::InvalidRequest {
                "mcp_invalid"
            } else {
                plugins::live::error_key(&error)
            },
        );
        self.detail = Some(error.message);
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.closed = true;
        self.request = None;
        for slot in &self.slots {
            slot.clear_value(window, cx);
        }
    }
}
