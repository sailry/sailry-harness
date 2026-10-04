use super::*;
use std::collections::BTreeMap;

impl Editor {
    pub(super) fn prepare(&self, cx: &App) -> Result<Command, &'static str> {
        if self
            .info
            .as_ref()
            .is_none_or(|info| info.settings.is_none())
        {
            return Err("plugins_settings_read_failed");
        }
        let mut values = BTreeMap::new();
        let mut secrets = BTreeMap::new();
        let models: std::collections::BTreeSet<_> = self.fields.iter().filter_map(|field| {
            let field = field.read(cx);
            (field.model() && matches!(field.value(cx), Ok(fields::Value::Public(Some(serde_json::Value::String(value)))) if !value.is_empty()))
                .then(|| field.name.clone())
        }).collect();
        for field in &self.fields {
            let field = field.read(cx);
            if field
                .linked()
                .is_some_and(|target| !models.contains(target))
            {
                values.insert(field.name.clone(), serde_json::Value::String(String::new()));
                continue;
            }
            match field.value(cx)? {
                fields::Value::Public(Some(value)) => {
                    values.insert(field.name.clone(), value);
                }
                fields::Value::Public(None) => {}
                fields::Value::Secret(update) => {
                    secrets.insert(field.name.clone(), update);
                }
            }
        }
        Ok(Command::SavePluginSettings {
            package: self.expected.reference(),
            values,
            secrets,
        })
    }

    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.closed || self.loading {
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
        let request = self.request.clone().expect("settings request is prepared");
        let binding = self.binding.clone();
        self.pending = true;
        self.error = None;
        self.detail = None;
        self.lock(true, cx);
        let job = binding
            .runtime
            .spawn(async move { binding.client.execute(request).await });
        self.task = Some(cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            let _ = editor.update_in(cx, |editor, window, cx| {
                editor.pending = false;
                if editor.closed {
                    return;
                }
                match result {
                    Ok(Ok(Output::PluginSettings(_))) => {
                        if editor.embedded {
                            editor.request = None;
                            for field in &editor.fields {
                                field.update(cx, |field, cx| {
                                    field.dirty = false;
                                    field.clear_secret(window, cx);
                                });
                            }
                            editor.load(true, window, cx);
                        } else {
                            editor.close(window, cx);
                            window.close_dialog(cx);
                        }
                    }
                    Ok(Err(error)) => {
                        if !live::uncertain(&error) {
                            editor.request = None;
                            editor.lock(false, cx);
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
}
