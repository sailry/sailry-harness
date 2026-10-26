//! Git source form. Inspection is read-only; installation uses durable Node admission.
use super::*;
use crate::{settings::providers::Binding, theme::DialogStyle as _};
use gpui_kit::component::{
    dialog::DialogFooter,
    form::Field,
    input::{Input, InputState},
};
use sailry_protocol::{Command, Output, Request, plugin::skills::Source};

struct Download {
    binding: Binding,
    known: Vec<sailry_protocol::plugin::Summary>,
    expected_name: Option<String>,
    fields: [Entity<InputState>; 3],
    request: Option<Request>,
    pending: bool,
    closed: bool,
    dialog: crate::ui::overlays::Owned,
    error: Option<&'static str>,
    required: Option<String>,
    task: Option<Task<()>>,
}

pub(in crate::settings) fn install(
    owner: Entity<Workspace>,
    source: Source,
    name: String,
    window: &mut Window,
    cx: &mut App,
) {
    mount(owner, Some(source), Some(name), window, cx);
}

pub(super) fn open(
    owner: Entity<Workspace>,
    source: Option<Source>,
    window: &mut Window,
    cx: &mut App,
) {
    mount(owner, source, None, window, cx);
}

fn mount(
    owner: Entity<Workspace>,
    source: Option<Source>,
    expected_name: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(live) = &owner.read(cx).provider_link else {
        return;
    };
    if !live.connected {
        return;
    }
    let binding = live.binding.clone();
    let known = owner.read(cx).plugin_catalog.packages.clone();
    let start = expected_name.is_some();
    let editor = cx.new(|cx| {
        let fields = [
            "skills_repository_hint",
            "skills_revision_hint",
            "plugins_path_hint",
        ]
        .map(|key| cx.new(|cx| InputState::new(window, cx).placeholder(tr(key))));
        if let Some(source) = &source {
            for (input, value) in fields.iter().zip([
                source.repository.clone(),
                source.git_ref.clone().unwrap_or_default(),
                source.path.clone().unwrap_or_default(),
            ]) {
                input.update(cx, |input, cx| input.set_value(value, window, cx));
            }
        }
        crate::feedback::observe_with(
            window,
            cx,
            |state: &Download, _| state.error.into_iter().collect(),
            |state, key, cx| {
                gpui_kit::component::notification::Notification::error(compatibility::error(
                    key,
                    state.required.as_deref(),
                    &state.binding,
                    cx,
                ))
            },
        );
        Download {
            binding,
            known,
            expected_name,
            fields,
            request: None,
            pending: false,
            closed: false,
            dialog: Default::default(),
            error: None,
            required: None,
            task: None,
        }
    });
    let active = editor.clone();
    present(editor, window, cx);
    if start {
        active.update(cx, |state, cx| state.submit(window, cx));
    }
}

fn present(editor: Entity<Download>, window: &mut Window, cx: &mut App) {
    let active = editor.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let close = editor.clone();
        let cancel = editor.clone();
        let submit = editor.clone();
        let state = editor.read(cx);
        dialog
            .form_title(tr("plugins_download"), cx)
            .w((window.viewport_size().width - px(48.)).min(px(600.)))
            .overlay_closable(false)
            .on_close(move |_, _, cx| {
                close.update(cx, |state, _| {
                    state.closed = true;
                    state.dialog.dismissed();
                })
            })
            .child(editor.clone())
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("plugin-download-cancel")
                            .label(tr("settings_cancel"))
                            .on_click(move |_, window, cx| {
                                cancel.update(cx, |state, _| {
                                    state.closed = true;
                                    state.dialog.dismissed();
                                });
                                window.close_dialog(cx);
                            }),
                    )
                    .child(
                        Button::new("plugin-download-submit")
                            .primary()
                            .debug_selector(|| "plugin-download-submit".into())
                            .loading(state.pending)
                            .disabled(state.pending)
                            .label(tr(label(state.pending, state.error.is_some())))
                            .on_click(move |_, window, cx| {
                                submit.update(cx, |state, cx| state.submit(window, cx))
                            }),
                    ),
            )
    });
    active.update(cx, |state, cx| {
        crate::ui::overlays::Owned::bind(state, |state| &mut state.dialog, window, cx)
    });
}

fn label(pending: bool, failed: bool) -> &'static str {
    if pending {
        "plugins_installing"
    } else if failed {
        "plugins_retry"
    } else {
        "plugins_install"
    }
}

impl Download {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.closed {
            return;
        }
        let values = self
            .fields
            .each_ref()
            .map(|input| input.read(cx).value().trim().to_owned());
        let source = Source {
            repository: values[0].clone(),
            git_ref: (!values[1].is_empty()).then(|| values[1].clone()),
            path: (!values[2].is_empty()).then(|| values[2].clone()),
        };
        if source.repository.is_empty() {
            self.error = Some("skills_source_invalid");
            cx.notify();
            return;
        }
        self.pending = true;
        self.error = None;
        let binding = self.binding.clone();
        let request = self.request.clone();
        let captured = request.clone();
        let retry = cx.entity();
        let ticket = request.as_ref().and_then(|request| {
            crate::installations::track(
                binding.client.clone(),
                binding.runtime.clone(),
                request,
                false,
                move |attempt, window, cx| {
                    let show = retry.update(cx, |state, cx| {
                        state.closed = false;
                        if !state.pending
                            && let Some(captured) = &captured
                        {
                            state.request = Some(attempt.request(&state.binding.client, captured));
                        }
                        !state.dialog.presented(cx)
                    });
                    if show {
                        present(retry.clone(), window, cx);
                    }
                    retry.update(cx, |state, cx| state.submit(window, cx));
                },
                window,
                cx,
            )
        });
        let job = binding.runtime.spawn(async move {
            if let Some(request) = request {
                return Ok::<_, sailry_protocol::Fault>((
                    None,
                    Some(binding.client.execute(request).await),
                ));
            }
            let output = binding
                .client
                .execute(
                    binding
                        .client
                        .prepare(Command::InspectPluginSource { source }),
                )
                .await?;
            match output {
                Output::PluginSource(selection) => Ok((Some(selection), None)),
                _ => Err(sailry_protocol::Fault::new(
                    sailry_protocol::ErrorCode::Internal,
                    "plugin inspection response expected",
                )),
            }
        });
        let keep_alive = cx.entity();
        let origin = window.window_handle();
        self.task = Some(cx.spawn(async move |state, cx| {
            let _keep_alive = keep_alive;
            let result = job.await;
            if let Some(ticket) = &ticket {
                let outcome = match &result {
                    Ok(Ok((_, Some(Ok(Output::Plugin(_)))))) => Ok(()),
                    Ok(Ok((_, Some(Err(error))))) | Ok(Err(error)) => Err(error.clone()),
                    _ => Err(sailry_protocol::Fault::new(
                        sailry_protocol::ErrorCode::OutcomeUnknown,
                        "installation result is unconfirmed",
                    )),
                };
                cx.update(|cx| ticket.record(outcome, cx));
            }
            let Some(target) = cx.update(|cx| crate::installations::target(origin, cx)) else {
                let _ = state.update(cx, |state, cx| {
                    state.pending = false;
                    if let Ok(Ok((_, Some(Err(error))))) | Ok(Err(error)) = &result
                        && !live::uncertain(error)
                    {
                        state.request = None;
                    }
                    cx.notify();
                });
                return;
            };
            _ = target.update(cx, |_, window, cx| {
                state.update(cx, |state, cx| {
                    state.pending = false;
                    match result {
                        Ok(Ok((Some(selection), _))) => {
                            if state.closed {
                                return;
                            }
                            if state
                                .expected_name
                                .as_ref()
                                .is_some_and(|name| *name != selection.info.summary.name)
                            {
                                state.error = Some("plugins_package_mismatch");
                                cx.notify();
                                return;
                            }
                            let expected_revision = if state.expected_name.is_some() {
                                0
                            } else {
                                state
                                    .known
                                    .iter()
                                    .find(|plugin| plugin.name == selection.info.summary.name)
                                    .map_or(0, |plugin| plugin.revision)
                            };
                            state.request =
                                Some(state.binding.client.prepare(Command::InstallPluginSource {
                                    source: selection.source.clone(),
                                    path: selection.path.clone(),
                                    name: selection.info.summary.name.clone(),
                                    expected_revision,
                                }));
                            state.submit(window, cx);
                        }
                        Ok(Ok((_, Some(Ok(Output::Plugin(_)))))) => {
                            state.closed = true;
                            state.request = None;
                            state.dialog.close(window, cx);
                        }
                        Ok(Ok((_, Some(Err(error))))) | Ok(Err(error)) => {
                            if !live::uncertain(&error) {
                                state.request = None;
                            }
                            state.required = compatibility::required(&error).map(str::to_owned);
                            state.error = Some(live::error_key(&error));
                        }
                        _ => state.error = Some("plugins_unknown"),
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
}

impl Render for Download {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex().gap_4().children(
            ["skills_repository", "skills_revision", "plugins_path"]
                .into_iter()
                .enumerate()
                .map(|(index, key)| {
                    Field::new().label(tr(key)).child(
                        div()
                            .debug_selector(move || format!("plugin-source-{index}"))
                            .child(
                                Input::new(&self.fields[index])
                                    .disabled(self.pending || self.request.is_some())
                                    .aria_label(tr(key)),
                            ),
                    )
                }),
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn labels_follow_operation_state() {
        assert_eq!(super::label(false, false), "plugins_install");
        assert_eq!(super::label(true, false), "plugins_installing");
        assert_eq!(super::label(true, true), "plugins_installing");
        assert_eq!(super::label(false, true), "plugins_retry");
    }
}
