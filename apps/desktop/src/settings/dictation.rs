use super::group::{Group, Row};
use crate::{dictation, preferences, tr};
use dictation::permission::{self, Status};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        menu::{DropdownMenu, PopupMenuItem},
        progress::Progress,
        *,
    },
    *,
};
use sailry_link::CancellationToken;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Default, PartialEq)]
enum State {
    #[default]
    Unavailable,
    Checking,
    Missing,
    Downloading(dictation::model::DownloadProgress),
    Ready,
    Failed,
}

pub(super) struct Panel {
    state: State,
    microphones: Vec<(String, String)>,
    cancel: CancellationToken,
    task: Option<Task<()>>,
    permission: Status,
    permission_task: Option<Task<()>>,
}

impl Drop for Panel {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

enum Update {
    Progress(dictation::model::DownloadProgress),
    Finished(dictation::Result<()>),
}

impl Panel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.observe_global::<preferences::Preferences>(|_, cx| cx.notify())
            .detach();
        Self {
            state: State::Unavailable,
            microphones: vec![],
            cancel: CancellationToken::new(),
            task: None,
            permission: Status::Unknown,
            permission_task: None,
        }
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if dictation::directory(cx).is_some() {
            self.permission = permission::status();
            cx.notify();
        }
        if matches!(self.state, State::Checking | State::Downloading(_)) {
            return;
        }
        let Some(directory) = dictation::directory(cx) else {
            return;
        };
        self.state = State::Checking;
        let job = cx.background_executor().spawn(async move {
            (
                dictation::model::ready(&directory),
                dictation::microphones(),
            )
        });
        self.task = Some(cx.spawn(async move |owner, cx| {
            let (ready, devices) = job.await;
            _ = owner.update(cx, |this, cx| {
                this.state = if ready { State::Ready } else { State::Missing };
                this.microphones = devices.unwrap_or_default();
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn authorize(&mut self, cx: &mut Context<Self>) {
        if self.permission_task.is_some() || dictation::directory(cx).is_none() {
            return;
        }
        self.permission = permission::status();
        if self.permission != Status::NotDetermined {
            if let Some(url) = permission::settings_url() {
                cx.open_url(url);
            }
            cx.notify();
            return;
        }
        let cancel = CancellationToken::new();
        let job = cx.background_executor().spawn(async move {
            let _ = permission::ensure(&cancel).await;
            permission::status()
        });
        self.permission_task = Some(cx.spawn(async move |owner, cx| {
            let status = job.await;
            _ = owner.update(cx, |this, cx| {
                this.finish_authorization(status, cx);
            });
        }));
        cx.notify();
    }

    fn finish_authorization(&mut self, status: Status, cx: &mut Context<Self>) {
        self.permission_task = None;
        self.permission = status;
        if status != Status::Granted
            && let Some(url) = permission::settings_url()
        {
            cx.open_url(url);
        }
        self.refresh(cx);
        cx.notify();
    }

    fn download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.state, State::Missing | State::Failed) {
            return;
        }
        let Some(directory) = dictation::directory(cx) else {
            return;
        };
        let Some(runtime) = cx
            .try_global::<crate::backend::Services>()
            .map(|services| services.runtime.clone())
        else {
            crate::feedback::info("", tr("dictation_unavailable").as_ref(), window, cx);
            return;
        };
        let owner = dictation::Service::shared(cx);
        self.cancel = CancellationToken::new();
        let cancel = self.cancel.clone();
        self.state = State::Downloading(Default::default());
        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
        runtime.spawn(async move {
            let result = async {
                let _guard = owner.try_lock_owned().map_err(|_| "dictation_busy")?;
                dictation::model::prepare_with_progress(&directory, &cancel, |value| {
                    let _ = sender.try_send(Update::Progress(value));
                })
                .await
                .map(|_| ())
                .map_err(dictation::error_key)
            }
            .await;
            let _ = sender.send(Update::Finished(result)).await;
        });
        self.task = Some(cx.spawn_in(window, async move |owner, cx| {
            while let Some(update) = receiver.recv().await {
                let ended = matches!(update, Update::Finished(_));
                if owner
                    .update_in(cx, |this, window, cx| this.accept(update, window, cx))
                    .is_err()
                    || ended
                {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn accept(&mut self, update: Update, window: &mut Window, cx: &mut Context<Self>) {
        match update {
            Update::Progress(value) => self.state = State::Downloading(value),
            Update::Finished(Ok(())) => self.state = State::Ready,
            Update::Finished(Err("dictation_cancelled")) => self.state = State::Missing,
            Update::Finished(Err(error)) => {
                self.state = State::Failed;
                crate::feedback::info("", tr(error).as_ref(), window, cx);
            }
        }
        cx.notify();
    }
}

fn language_key(language: preferences::Language) -> &'static str {
    match language {
        preferences::Language::Auto => "dictation_language_auto",
        preferences::Language::Chinese => "dictation_language_chinese",
        preferences::Language::English => "dictation_language_english",
    }
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let options = preferences::data(cx).dictation.unwrap_or_default();
        let selected = options.microphone.clone();
        let devices = self.microphones.clone();
        let microphone = selected
            .as_ref()
            .map(|id| {
                devices
                    .iter()
                    .find(|(key, _)| key == id)
                    .map(|(_, label)| label.clone())
                    .unwrap_or_else(|| {
                        tr(
                            if matches!(self.state, State::Unavailable | State::Checking) {
                                "dictation_device_loading"
                            } else {
                                "dictation_device_missing"
                            },
                        )
                        .to_string()
                    })
            })
            .unwrap_or_else(|| tr("dictation_microphone_default").to_string());
        let model_action = match self.state {
            State::Downloading(value) => h_flex()
                .w_full()
                .gap_2()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .debug_selector(|| "dictation-download-progress".into())
                                .child(
                                    Progress::new("dictation-progress")
                                        .value(value.percent)
                                        .small(),
                                ),
                        )
                        .child(
                            h_flex()
                                .justify_between()
                                .gap_2()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{:.0}%", value.percent))
                                .child(
                                    div()
                                        .debug_selector(|| "dictation-download-speed".into())
                                        .child(rust_i18n::t!(
                                            "dictation_download_speed",
                                            value = format!(
                                                "{:.2}",
                                                value.bytes_per_second / 1_000_000.
                                            )
                                        )),
                                ),
                        ),
                )
                .child(
                    Button::new("dictation-download-cancel")
                        .ghost()
                        .small()
                        .label(tr("settings_cancel"))
                        .disabled(self.cancel.is_cancelled())
                        .debug_selector(|| "dictation-download-cancel".into())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.cancel.cancel();
                            cx.notify();
                        })),
                )
                .into_any_element(),
            State::Missing | State::Failed => Button::new("dictation-download")
                .primary()
                .label(tr(if self.state == State::Failed {
                    "dictation_retry"
                } else {
                    "dictation_download_model"
                }))
                .debug_selector(|| "dictation-download".into())
                .on_click(cx.listener(|this, _, window, cx| this.download(window, cx)))
                .into_any_element(),
            state => div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .debug_selector(|| "dictation-model-status".into())
                .child(tr(match state {
                    State::Ready => "dictation_ready",
                    State::Checking => "dictation_checking",
                    _ => "dictation_unavailable",
                }))
                .into_any_element(),
        };
        v_flex()
            .gap_6()
            .child(
                Group::new("dictation_input")
                    .child(
                        Row::new(
                            "dictation_access",
                            h_flex().when(permission::settings_url().is_some(), |row| {
                                if self.permission == Status::Granted {
                                    return row.child(
                                        div()
                                            .text_color(cx.theme().muted_foreground)
                                            .debug_selector(|| "dictation-access-Granted".into())
                                            .child(tr("dictation_access_granted")),
                                    );
                                }
                                row.child(
                                    Button::new("dictation-authorize")
                                        .label(tr("permission_request"))
                                        .disabled(self.permission_task.is_some())
                                        .debug_selector(|| "dictation-authorize".into())
                                        .on_click(cx.listener(|this, _, _, cx| this.authorize(cx))),
                                )
                            }),
                        )
                        .description("dictation_access_hint")
                        .wide(),
                    )
                    .child(
                        Row::new(
                            "dictation_microphone_label",
                            Button::new("dictation-microphone")
                                .w_full()
                                .label(microphone)
                                .dropdown_caret(true)
                                .debug_selector(|| "dictation-microphone".into())
                                .dropdown_menu(move |menu, _, _| {
                                    std::iter::once((
                                        None,
                                        tr("dictation_microphone_default").to_string(),
                                    ))
                                    .chain(
                                        devices
                                            .iter()
                                            .map(|(id, name)| (Some(id.clone()), name.clone())),
                                    )
                                    .fold(
                                        menu,
                                        |menu, (id, name)| {
                                            menu.item(
                                                PopupMenuItem::new(name)
                                                    .checked(id == selected)
                                                    .on_click(move |_, _, cx| {
                                                        preferences::update(cx, |data| {
                                                            data.dictation
                                                                .get_or_insert_default()
                                                                .microphone = id.clone();
                                                        })
                                                    }),
                                            )
                                        },
                                    )
                                }),
                        )
                        .wide(),
                    )
                    .child(
                        Row::new(
                            "dictation_language",
                            Button::new("dictation-language")
                                .w_full()
                                .label(tr(language_key(options.language)))
                                .dropdown_caret(true)
                                .debug_selector(|| "dictation-language".into())
                                .dropdown_menu(move |menu, _, _| {
                                    preferences::Language::ALL.into_iter().fold(
                                        menu,
                                        |menu, language| {
                                            menu.item(
                                                PopupMenuItem::new(tr(language_key(language)))
                                                    .checked(language == options.language)
                                                    .on_click(move |_, _, cx| {
                                                        preferences::update(cx, |data| {
                                                            data.dictation
                                                                .get_or_insert_default()
                                                                .language = language;
                                                        })
                                                    }),
                                            )
                                        },
                                    )
                                }),
                        )
                        .wide(),
                    ),
            )
            .child(
                Group::new("dictation_local_model").child(
                    Row::new("dictation_model_name", model_action)
                        .description("dictation_model_size")
                        .wide(),
                ),
            )
    }
}
