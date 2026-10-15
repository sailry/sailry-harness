use super::{
    config::Channel,
    service::{self, Notice, Service, State},
};
use crate::{
    settings::group::{Group, Row},
    theme::DialogStyle as _,
    tr,
};
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        notification::Notification,
        progress::Progress,
        searchable_list::SearchableListItem,
        select::{Select, SelectEvent, SelectState},
        *,
    },
    prelude::FluentBuilder as _,
    *,
};

pub(crate) struct Panel {
    service: Entity<Service>,
    channel: Entity<SelectState<Vec<ChannelChoice>>>,
}

#[derive(Clone)]
struct ChannelChoice(Channel);

impl SearchableListItem for ChannelChoice {
    type Value = Channel;

    fn title(&self) -> SharedString {
        match self.0 {
            Channel::Stable => tr("updates_channel_stable"),
            Channel::Experience => tr("updates_channel_experience"),
        }
    }

    fn value(&self) -> &Self::Value {
        &self.0
    }
}

impl Panel {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let service = Service::shared(cx);
        let selected = crate::preferences::update_channel(cx);
        if crate::preferences::data(cx).update_channel.is_none()
            && cx.try_global::<crate::backend::Services>().is_some()
        {
            crate::preferences::update(cx, |data| data.update_channel = Some(selected));
        }
        let channel = cx.new(|cx| {
            SelectState::new(
                Channel::ALL
                    .into_iter()
                    .map(ChannelChoice)
                    .collect::<Vec<_>>(),
                Channel::ALL
                    .iter()
                    .position(|channel| *channel == selected)
                    .map(IndexPath::new),
                window,
                cx,
            )
        });
        cx.observe(&service, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(&service, window, |_, _, notice: &Notice, window, cx| {
            let summary = tr(notice.key);
            let message = if notice.error {
                Notification::error(summary.clone())
            } else {
                Notification::success(summary.clone())
            };
            crate::feedback::toast(window, summary, message, cx);
        })
        .detach();
        cx.subscribe_in(&channel, window, |panel, _, event, _, cx| {
            if cx.try_global::<crate::backend::Services>().is_none() {
                return;
            }
            let SelectEvent::Confirm(Some(channel)) = event else {
                return;
            };
            if crate::preferences::data(cx).update_channel == Some(*channel) {
                return;
            }
            crate::preferences::update(cx, |data| data.update_channel = Some(*channel));
            let recheck = panel
                .service
                .update(cx, |service, cx| service.channel_changed(cx));
            if recheck {
                panel.service.update(cx, |service, cx| service.check(cx));
            }
        })
        .detach();
        let panel = Self { service, channel };
        panel.receipt(window, cx);
        panel
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let service = self.service.clone();
        window.open_dialog(cx, move |dialog, _, cx| {
            let service = service.clone();
            dialog
                .form_title(tr("updates_install"), cx)
                .child(tr("updates_restart_description"))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(tr("updates_install"))
                        .cancel_text(tr("settings_cancel")),
                )
                .on_ok(move |_, window, cx| {
                    let service = service.clone();
                    window.defer(cx, move |window, cx| {
                        service.update(cx, |service, cx| service.install(window, cx))
                    });
                    true
                })
        });
    }

    fn receipt(&self, window: &Window, cx: &mut Context<Self>) {
        let Some(directory) = service::directory(cx) else {
            return;
        };
        let job = cx
            .background_executor()
            .spawn(async move { super::receipt::read(&directory) });
        let service = self.service.clone();
        cx.spawn_in(window, async move |_, cx| {
            let result = job.await;
            let _ = cx.update(|window, cx| {
                if let Ok(Some(key)) = result {
                    crate::feedback::toast(window, tr(key), Notification::success(tr(key)), cx);
                } else if let Err(error) = result {
                    eprintln!("desktop update receipt failed: {error}");
                    service.update(cx, |service, cx| {
                        service.failure = Some(error.clone());
                        cx.notify();
                    });
                    crate::feedback::toast(
                        window,
                        tr(error.key),
                        Notification::error(tr(error.key)),
                        cx,
                    );
                }
            });
        })
        .detach();
    }
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.service.read(cx).state.clone();
        let live = cx.try_global::<crate::backend::Services>().is_some();
        let channel_locked = matches!(
            &state,
            State::Checking
                | State::Downloading(_)
                | State::Installing
                | State::Restarting
                | State::Stopped
        );
        let status = match &state {
            State::Idle => env!("CARGO_PKG_VERSION").to_owned().into_any_element(),
            State::Checking => tr("updates_checking").into_any_element(),
            State::Available(version) | State::Ready(version) => version.clone().into_any_element(),
            State::Installing => tr("updates_installing").into_any_element(),
            State::Restarting => tr("updates_restarting").into_any_element(),
            State::Stopped => tr("updates_restart_needed").into_any_element(),
            State::Downloading(progress) => {
                let (percent, verifying) = match progress {
                    super::transfer::Progress::Downloading { copied, total } => (
                        total
                            .filter(|total| *total != 0)
                            .map(|total| *copied as f32 / total as f32 * 100.)
                            .unwrap_or(0.),
                        false,
                    ),
                    super::transfer::Progress::Verifying => (100., true),
                };
                v_flex()
                    .w_56()
                    .gap_1()
                    .child(
                        Progress::new("updates-download-progress")
                            .value(percent)
                            .loading(verifying)
                            .small(),
                    )
                    .child(if verifying {
                        tr("updates_verifying").to_string()
                    } else {
                        format!("{percent:.0}%")
                    })
                    .into_any_element()
            }
        };
        let action = match state {
            State::Idle | State::Available(_) => Button::new("updates-check")
                .label(tr("updates_check"))
                .disabled(!live)
                .debug_selector(|| "updates-check".into())
                .on_click(cx.listener(|panel, _, _, cx| {
                    panel.service.update(cx, |service, cx| service.check(cx))
                })),
            State::Checking | State::Downloading(_) => Button::new("updates-cancel")
                .label(tr("settings_cancel"))
                .debug_selector(|| "updates-cancel".into())
                .on_click(cx.listener(|panel, _, _, cx| {
                    panel.service.update(cx, |service, cx| service.cancel(cx))
                })),
            State::Ready(_) => Button::new("updates-install")
                .label(tr("updates_install"))
                .primary()
                .debug_selector(|| "updates-install".into())
                .on_click(cx.listener(|panel, _, window, cx| panel.confirm(window, cx))),
            State::Installing | State::Restarting => Button::new("updates-installing")
                .label(tr("updates_installing"))
                .disabled(true),
            State::Stopped => Button::new("updates-restart")
                .label(tr("updates_restart"))
                .on_click(cx.listener(|panel, _, _, cx| {
                    panel.service.update(cx, |service, cx| service.restart(cx))
                })),
        };
        Group::new("updates")
            .action(action)
            .when_some(self.service.read(cx).failure.clone(), |group, error| {
                group.action(
                    Button::new("updates-details")
                        .label(tr("updates_details"))
                        .on_click(move |_, window, cx| {
                            crate::ui::details::open(
                                tr("updates_details").to_string(),
                                String::new(),
                                error.detail.clone(),
                                window,
                                cx,
                            )
                        }),
                )
            })
            .when(matches!(state, State::Available(_)), |group| {
                group.action(
                    Button::new("updates-download")
                        .label(tr("updates_download"))
                        .debug_selector(|| "updates-download".into())
                        .primary()
                        .on_click(cx.listener(|panel, _, _, cx| {
                            panel.service.update(cx, |service, cx| service.download(cx))
                        })),
                )
            })
            .child(Row::new(
                "updates_channel",
                div()
                    .w_48()
                    .debug_selector(|| "updates-channel".into())
                    .child(
                        Select::new(&self.channel)
                            .id("updates-channel-select")
                            .accessibility_label(tr("updates_channel"))
                            .disabled(!live || channel_locked),
                    ),
            ))
            .child(Row::new("updates_version", status))
    }
}
