//! Captured draft commands are visible source, not hidden message prefixes.
use super::*;
use sailry_protocol::{conversation::Input, plugin::ui::CommandKind};
use serde_json::{Value, json};

impl View {
    pub(super) fn normalized_input(&self, mut input: Input) -> Input {
        if let Some((mode, _)) = references::commands::leading_mode(&input.text)
            && mode == self.current_mode()
        {
            input.text = references::commands::leading(&input.text)
                .unwrap()
                .1
                .to_owned();
        }
        input
    }

    pub(crate) fn composer_options(&self, cx: &App) -> Value {
        let compacting = self.history.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.page.runs.iter().any(|run| {
                run.kind == sailry_protocol::conversation::RunKind::Compaction
                    && matches!(
                        run.status,
                        Status::Queued | Status::Running | Status::Stopping
                    )
            })
        });
        let statistics = self.history.snapshot.as_ref().map(|snapshot| {
            crate::plugins::usage::exact(
                serde_json::to_value(&snapshot.statistics)
                    .expect("serializable conversation statistics"),
            )
        });
        let text = self.input.read(cx).value();
        let command = references::commands::leading(&text).and_then(|(name, arguments, _)| {
            self.references
                .command_keys
                .get(name)
                .filter(|key| self.contributions.read(cx).message_command(key, name, cx))
                .map(|key| json!({"name":name,"arguments":arguments,"package":key.package}))
        });
        json!({"command":command,"mode":self.current_mode(),"session":self.session(),"connected":self.connected(),
            "busy":self.busy(),"readonly":self.readonly(),"statistics":statistics,"context_limit":self.context_limit(),
            "compacting":compacting,"sidebar":self.sidebar,"has_messages":!self.rows.is_empty()})
    }

    pub(crate) fn set_composer_option(
        &mut self,
        owner: &sailry_protocol::plugin::Reference,
        key: &str,
        value: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.busy() || !self.connected() || self.readonly() {
            return Err("composer is unavailable".into());
        }
        match key {
            "mode" => {
                let mode = serde_json::from_value(value).map_err(|_| "invalid composer mode")?;
                self.select_mode(mode, window, cx);
                Ok(())
            }
            "command" => {
                if self.input.update(cx, |input, cx| {
                    input.marked_text_range(window, cx).is_some()
                }) {
                    return Err("composer is composing".into());
                }
                let entries = self.contributions.read(cx).commands(cx);
                let declared = |name: &str| {
                    entries.iter().any(|entry| {
                        entry.key.package == *owner
                            && entry.state.enabled
                            && entry.declaration.command.as_ref().is_some_and(|command| {
                                command.name == name && command.kind == CommandKind::Message
                            })
                    })
                };
                let name = if value.is_null() {
                    None
                } else {
                    let name = value.as_str().ok_or("composer command requires a string")?;
                    if !declared(name) {
                        return Err("composer command is unavailable".into());
                    }
                    Some(name)
                };
                let text = self.input.read(cx).value();
                let body = match references::commands::leading(&text) {
                    Some((current, arguments, _)) if declared(current) => arguments,
                    Some(_) => return Err("draft command belongs to another provider".into()),
                    None if name.is_none() => return Ok(()),
                    None => &text,
                };
                let end = text.len() - body.len();
                let prefix = name.map_or_else(String::new, |name| format!("/{name} "));
                self.input.update(cx, |input, cx| {
                    let selected = input.selected_range();
                    let shift = |offset: usize| offset.saturating_sub(end) + prefix.len();
                    if name.is_some() {
                        input
                            .replace_range_with_token(
                                0..end,
                                references::commands::token(prefix.trim()),
                                window,
                                cx,
                            )
                            .expect("command prefix is a valid input range");
                        input.replace(" ", window, cx);
                    } else {
                        input.set_selected_range(0..end, cx);
                        input.replace("", window, cx);
                    }
                    input.set_selected_range(shift(selected.start)..shift(selected.end), cx);
                });
                cx.notify();
                Ok(())
            }
            _ => Err("unknown composer option".into()),
        }
    }
}
