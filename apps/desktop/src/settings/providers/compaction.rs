//! Autosave the selected execution Node's default compaction percentage.
use super::{Binding, Workspace};
use crate::{settings::group::Row, tr};
use gpui_kit::{
    component::{
        button::Button,
        notification::Notification,
        slider::{Slider, SliderEvent, SliderState},
        *,
    },
    *,
};
use sailry_protocol::{
    Command, ErrorCode, NodeId, Output, Request, conversation::compaction::Settings,
};
use std::collections::BTreeMap;

#[derive(Default)]
pub(in crate::settings) struct State {
    hosts: BTreeMap<NodeId, Entity<Host>>,
    preview: Option<Entity<Host>>,
}

impl Workspace {
    pub(super) fn compaction_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Host> {
        let state = &mut self.compaction;
        if let Some(live) = &self.provider_link {
            let binding = live.binding.clone();
            let host = state
                .hosts
                .entry(binding.client.target())
                .or_insert_with(|| cx.new(|cx| Host::new(Some(binding.clone()), window, cx)))
                .clone();
            host.update(cx, |host, cx| {
                host.binding = Some(binding);
                let connected = live.connected && live.snapshot.is_some();
                if host.connected != connected {
                    host.connected = connected;
                    host.observed = None;
                    cx.notify();
                }
                if let Some(snapshot) = live.snapshot.as_ref().filter(|_| connected) {
                    let revision = snapshot.compaction_settings_revision;
                    if !host.pending
                        && host.request.is_none()
                        && !host.dirty(cx)
                        && host.observed != Some(revision)
                    {
                        host.observed = Some(revision);
                        if host
                            .saved
                            .as_ref()
                            .is_none_or(|saved| saved.revision < revision)
                        {
                            host.load(window, cx);
                        }
                    }
                }
            });
            host
        } else {
            state
                .preview
                .get_or_insert_with(|| cx.new(|cx| Host::new(None, window, cx)))
                .clone()
        }
    }
}

pub(in crate::settings) struct Host {
    binding: Option<Binding>,
    saved: Option<Settings>,
    observed: Option<u64>,
    connected: bool,
    pending: bool,
    request: Option<Request>,
    slider: Entity<SliderState>,
}

const MIN: u8 = Settings::MIN;
const MAX: u8 = Settings::MAX;

impl Host {
    fn new(binding: Option<Binding>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let saved = binding.is_none().then(Settings::default);
        let slider = cx.new(|_| {
            SliderState::new()
                .min(MIN as f32)
                .max(MAX as f32)
                .step(1.)
                .default_value(position(
                    saved
                        .as_ref()
                        .map_or(Settings::default().percent, |saved| saved.percent),
                ))
        });
        // Dragging only previews; releasing the thumb saves like other settings sliders.
        cx.subscribe_in(
            &slider,
            window,
            |host, _, event: &SliderEvent, window, cx| match event {
                SliderEvent::Change(_) => cx.notify(),
                SliderEvent::Release(_) => host.save(window, cx),
            },
        )
        .detach();
        Self {
            binding,
            saved,
            observed: None,
            connected: false,
            pending: false,
            request: None,
            slider,
        }
    }

    fn value(&self, cx: &App) -> u8 {
        self.slider.read(cx).value().start().round() as u8
    }

    fn locked(&self) -> bool {
        !self.connected || self.pending || self.request.is_some() || self.saved.is_none()
    }

    fn dirty(&self, cx: &App) -> bool {
        self.saved
            .as_ref()
            .is_some_and(|saved| self.value(cx) != shown(saved.percent))
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || !self.connected || self.request.is_some() {
            return;
        }
        let Some(binding) = &self.binding else {
            return;
        };
        self.execute(
            binding.client.prepare(Command::ReadCompactionSettings),
            window,
            cx,
        );
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || !self.connected {
            return;
        }
        if let Some(request) = self.request.clone() {
            self.execute(request, window, cx);
            return;
        }
        let (Some(saved), Some(binding)) = (&self.saved, &self.binding) else {
            return;
        };
        let percent = self.value(cx);
        if shown(saved.percent) == percent {
            return;
        }
        let settings = Settings {
            revision: saved.revision,
            percent,
        };
        let request = binding
            .client
            .prepare(Command::SaveCompactionSettings(settings));
        self.request = Some(request.clone());
        self.execute(request, window, cx);
    }

    fn execute(&mut self, request: Request, window: &mut Window, cx: &mut Context<Self>) {
        let Some(binding) = self.binding.clone() else {
            return;
        };
        let writing = request.command.durable();
        let preserve = self.dirty(cx);
        self.pending = true;
        let runtime = binding.runtime.clone();
        let job = runtime
            .spawn(async move { crate::settings::requests::execute(&binding, request).await });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |host, window, cx| {
                host.pending = false;
                match result {
                    Ok((Ok(Output::CompactionSettings(settings)), _)) => {
                        host.request = None;
                        if writing || !preserve {
                            host.slider.update(cx, |slider, cx| {
                                slider.set_value(position(settings.percent), window, cx)
                            });
                        }
                        host.observed = Some(settings.revision);
                        host.saved = Some(settings);
                        if !writing && preserve {
                            host.save(window, cx);
                        }
                    }
                    result => {
                        let key = match result {
                            Ok((Err(error), true))
                                if !matches!(
                                    error.code,
                                    ErrorCode::OutcomeUnknown | ErrorCode::Unavailable
                                ) =>
                            {
                                host.request = None;
                                match error.code {
                                    _ if !writing => "compaction_settings_load_failed",
                                    ErrorCode::RevisionConflict => "compaction_settings_conflict",
                                    ErrorCode::InvalidRequest => "compaction_settings_failed",
                                    _ => "compaction_settings_failed",
                                }
                            }
                            _ if writing => "compaction_settings_unknown",
                            _ => "compaction_settings_load_failed",
                        };
                        host.fail(key, window, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn fail(&self, key: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let owner = cx.weak_entity();
        crate::feedback::toast(
            window,
            "compaction-settings-failed".into(),
            Notification::error(tr(key)).action(move |_, _, _| {
                let owner = owner.clone();
                Button::new("compaction-settings-retry")
                    .label(tr("settings_retry"))
                    .on_click(move |_, window, cx| {
                        _ = owner.update(cx, |host, cx| {
                            if host.request.is_some() {
                                host.save(window, cx);
                            } else {
                                host.load(window, cx);
                            }
                        });
                    })
            }),
            cx,
        );
    }
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Row::new(
            "provider_compaction_threshold",
            h_flex()
                .w(px(240.))
                .gap_3()
                .child(
                    div().flex_1().child(
                        Slider::new(&self.slider)
                            .accessibility_label(tr("provider_compaction_threshold"))
                            .disabled(self.locked()),
                    ),
                )
                .child(
                    div()
                        .w_10()
                        .text_right()
                        .child(format!("{}%", self.value(cx))),
                ),
        )
    }
}

fn shown(value: u8) -> u8 {
    value.clamp(MIN, MAX)
}

fn position(value: u8) -> f32 {
    shown(value) as f32
}
