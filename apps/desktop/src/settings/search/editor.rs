use super::*;
use crate::{theme::DialogStyle as _, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
    form::{Field, Form},
    input::{Input, InputState},
    scroll::ScrollableElement,
    switch::Switch,
};
use sailry_protocol::{
    Command, ErrorCode, Output, Request, Secret, plugin::settings::SecretUpdate,
    search::Credentials,
};

#[cfg(test)]
mod tests;

pub(super) struct Editor {
    owner: Entity<Panel>,
    driver: Driver,
    settings: Settings,
    endpoint: Entity<InputState>,
    key: Entity<InputState>,
    username: Entity<InputState>,
    password: Entity<InputState>,
    remove_key: bool,
    remove_basic: bool,
    pending: bool,
    closed: bool,
    request: Option<Request>,
    error: Option<&'static str>,
}

pub(super) fn open(
    owner: Entity<Panel>,
    driver: Driver,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Editor> {
    let settings = owner.read(cx).saved.clone().unwrap_or_default();
    let config = settings.providers.get(&driver).cloned().unwrap_or_default();
    let editor = cx.new(|cx| {
        crate::feedback::observe_with(
            window,
            cx,
            |editor: &Editor, _| editor.error.into_iter().collect(),
            |_, key, cx| {
                let owner = cx.weak_entity();
                notification::Notification::error(tr(key)).action(move |_, _, cx| {
                    let owner = owner.clone();
                    Button::new("search-editor-reload")
                        .debug_selector(|| "search-editor-reload".into())
                        .label(tr(if key == "search_save_unknown" {
                            "search_retry"
                        } else {
                            "search_reload"
                        }))
                        .on_click(cx.listener(move |toast, _, window, cx| {
                            toast.dismiss(window, cx);
                            _ = owner.update(cx, |editor, cx| {
                                if key == "search_save_unknown" {
                                    editor.save(window, cx);
                                } else {
                                    editor.reload(window, cx);
                                }
                            });
                        }))
                })
            },
        );
        Editor {
            owner,
            driver,
            settings,
            endpoint: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(config.endpoint.unwrap_or_default())
                    .placeholder(tr("search_endpoint"))
            }),
            key: cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder(tr(if config.key_configured {
                        "search_secret_keep"
                    } else {
                        "search_api_key"
                    }))
            }),
            username: cx.new(|cx| {
                InputState::new(window, cx).masked(true).placeholder(tr(
                    if config.basic_configured {
                        "search_secret_keep"
                    } else {
                        "search_username"
                    },
                ))
            }),
            password: cx.new(|cx| {
                InputState::new(window, cx).masked(true).placeholder(tr(
                    if config.basic_configured {
                        "search_secret_keep"
                    } else {
                        "search_password"
                    },
                ))
            }),
            remove_key: false,
            remove_basic: false,
            pending: false,
            closed: false,
            request: None,
            error: None,
        }
    });
    let view = editor.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let save = editor.clone();
        let close = editor.clone();
        let cancel = editor.clone();
        dialog
            .form_title(tr(label(driver)))
            .w((window.viewport_size().width - px(48.)).min(px(560.)))
            .on_ok(|_, _, _| false)
            .on_close(move |_, window, cx| close.update(cx, |editor, cx| editor.close(window, cx)))
            .child(editor.clone())
            .footer(
                DialogFooter::new()
                    .w_full()
                    .child(
                        Button::new("search-editor-cancel")
                            .debug_selector(|| "search-editor-cancel".into())
                            .label(tr("settings_cancel"))
                            .on_click(move |_, window, cx| {
                                cancel.update(cx, |editor, cx| editor.close(window, cx));
                                window.close_dialog(cx);
                            }),
                    )
                    .child(
                        Button::new("search-editor-save")
                            .primary()
                            .debug_selector(|| "search-editor-save".into())
                            .label(tr(if editor.read(cx).request.is_some() {
                                "search_retry"
                            } else {
                                "settings_save"
                            }))
                            .disabled(editor.read(cx).pending)
                            .on_click(move |_, window, cx| {
                                save.update(cx, |editor, cx| editor.save(window, cx))
                            }),
                    ),
            )
    });
    view
}

impl Editor {
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.closed = true;
        self.request = None;
        for input in [&self.key, &self.username, &self.password] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        cx.notify();
    }

    fn update(&self, cx: &App) -> Result<Update, &'static str> {
        let mut settings = self.settings.clone();
        if self.driver == Driver::Searxng {
            let value = self.endpoint.read(cx).value().trim().to_owned();
            settings.providers.entry(self.driver).or_default().endpoint =
                (!value.is_empty()).then_some(value);
        }
        let key = self.key.read(cx).value().to_string();
        let username = self.username.read(cx).value().to_string();
        let password = self.password.read(cx).value().to_string();
        let key = if self.remove_key {
            SecretUpdate::Clear
        } else if key.is_empty() {
            SecretUpdate::Keep
        } else {
            SecretUpdate::Replace(Secret::new(key))
        };
        let basic = if self.remove_basic {
            SecretUpdate::Clear
        } else if username.is_empty() && password.is_empty() {
            SecretUpdate::Keep
        } else if username.is_empty() || password.is_empty() || username.contains(':') {
            return Err("search_basic_invalid");
        } else {
            SecretUpdate::Replace(Secret::new(format!("{username}:{password}")))
        };
        Ok(Update {
            settings,
            credentials: [(self.driver, Credentials { key, basic })].into(),
        })
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.closed {
            return;
        }
        if !self.owner.read(cx).active {
            self.error = Some("search_target_changed");
            cx.notify();
            return;
        }
        let update = match self.update(cx) {
            Ok(update) => update,
            Err(key) => {
                self.error = Some(key);
                cx.notify();
                return;
            }
        };
        let Some(binding) = self.owner.read(cx).binding.clone() else {
            self.owner
                .update(cx, |panel, cx| panel.apply(update.settings, window, cx));
            self.close(window, cx);
            window.close_dialog(cx);
            return;
        };
        let request = self
            .request
            .get_or_insert_with(|| binding.client.prepare(Command::SaveSearchSettings(update)))
            .clone();
        self.pending = true;
        self.error = None;
        let job = binding.runtime.spawn(async move {
            let result = binding.client.execute(request.clone()).await;
            if result.is_err() {
                match binding.client.outcome(&request).await {
                    Ok(sailry_protocol::RequestOutcome::Completed(result)) => (*result, true),
                    _ => (result, false),
                }
            } else {
                (result, true)
            }
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |editor, window, cx| {
                editor.pending = false;
                match result {
                    Ok((Ok(Output::SearchSettings(settings)), _)) => {
                        editor
                            .owner
                            .update(cx, |panel, cx| panel.apply(settings, window, cx));
                        crate::feedback::toast(
                            window,
                            tr("search_saved"),
                            notification::Notification::success(tr("search_saved")),
                            cx,
                        );
                        if !editor.closed {
                            editor.close(window, cx);
                            window.close_dialog(cx);
                        }
                    }
                    Ok((Err(error), confirmed)) => {
                        if confirmed && error.code != ErrorCode::OutcomeUnknown {
                            editor.request = None;
                        }
                        editor.error = Some(if confirmed {
                            super::requests::error_key(error.code, true, false)
                        } else {
                            "search_save_unknown"
                        });
                    }
                    _ => editor.error = Some("search_save_unknown"),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.request.is_some() || self.closed {
            return;
        }
        if !self.owner.read(cx).active {
            self.error = Some("search_target_changed");
            cx.notify();
            return;
        }
        let Some(binding) = self.owner.read(cx).binding.clone() else {
            return;
        };
        self.pending = true;
        self.error = None;
        let job = binding.runtime.spawn(async move {
            binding
                .client
                .execute(binding.client.prepare(Command::ReadSearchSettings))
                .await
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |editor, window, cx| {
                editor.pending = false;
                match result {
                    Ok(Ok(Output::SearchSettings(settings))) => {
                        editor
                            .owner
                            .update(cx, |panel, cx| panel.apply(settings.clone(), window, cx));
                        editor.settings = settings;
                    }
                    _ => editor.error = Some("search_request_failed"),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locked = self.pending || self.request.is_some();
        let config = self
            .settings
            .providers
            .get(&self.driver)
            .cloned()
            .unwrap_or_default();
        let key_label = if self.driver.key_required() {
            "search_api_key"
        } else {
            "search_optional_key"
        };
        let mut form = Form::vertical();
        if self.driver == Driver::Searxng {
            form = form.child(
                Field::new().label(tr("search_endpoint")).child(
                    div()
                        .debug_selector(|| "search-endpoint".into())
                        .w_full()
                        .min_w_0()
                        .child(
                            Input::new(&self.endpoint)
                                .disabled(locked)
                                .aria_label(tr("search_endpoint")),
                        ),
                ),
            );
        }
        form = form.child(
            Field::new().label(tr(key_label)).child(
                div()
                    .debug_selector(|| "search-api-key".into())
                    .w_full()
                    .min_w_0()
                    .child(
                        Input::new(&self.key)
                            .disabled(locked || self.remove_key)
                            .aria_label(tr(key_label)),
                    ),
            ),
        );
        if config.key_configured {
            form = form.child(
                Field::new().child(
                    div().debug_selector(|| "search-remove-key".into()).child(
                        Switch::new("search-remove-key")
                            .label(tr("search_remove_key"))
                            .checked(self.remove_key)
                            .disabled(locked)
                            .on_click(cx.listener(|editor, value, _, cx| {
                                editor.remove_key = *value;
                                cx.notify();
                            })),
                    ),
                ),
            );
        }
        if self.driver == Driver::Searxng {
            form = form
                .child(
                    Field::new().label(tr("search_username")).child(
                        div()
                            .debug_selector(|| "search-username".into())
                            .w_full()
                            .min_w_0()
                            .child(
                                Input::new(&self.username)
                                    .disabled(locked || self.remove_basic)
                                    .aria_label(tr("search_username")),
                            ),
                    ),
                )
                .child(
                    Field::new().label(tr("search_password")).child(
                        div()
                            .debug_selector(|| "search-password".into())
                            .w_full()
                            .min_w_0()
                            .child(
                                Input::new(&self.password)
                                    .disabled(locked || self.remove_basic)
                                    .aria_label(tr("search_password")),
                            ),
                    ),
                );
            if config.basic_configured {
                form = form.child(
                    Field::new().child(
                        div().debug_selector(|| "search-remove-basic".into()).child(
                            Switch::new("search-remove-basic")
                                .label(tr("search_remove_basic"))
                                .checked(self.remove_basic)
                                .disabled(locked)
                                .on_click(cx.listener(|editor, value, _, cx| {
                                    editor.remove_basic = *value;
                                    cx.notify();
                                })),
                        ),
                    ),
                );
            }
        }
        v_flex()
            .id("search-editor")
            .max_h((window.viewport_size().height - px(230.)).max(px(160.)))
            .overflow_y_scrollbar()
            .gap_4()
            .child(form)
    }
}
