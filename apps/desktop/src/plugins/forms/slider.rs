//! Kit 98a5e5d's styled script Slider lacks state binding and release callbacks.
//! This view-local bridge retains the styled native control and its event lifecycle.
use gpui_kit::{
    component::slider::{Slider, SliderEvent, SliderState},
    *,
};
use gpui_shell::{HostError, HostModule, HostValue};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};

struct Draft {
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    label: String,
    state: Option<Entity<SliderState>>,
    pending: Option<f32>,
    subscription: Option<Subscription>,
}

impl Draft {
    fn render(
        &mut self,
        id: &str,
        disabled: bool,
        events: &tokio::sync::mpsc::Sender<serde_json::Value>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let state = self.state.get_or_insert_with(|| {
            cx.new(|_| {
                SliderState::new()
                    .min(self.min)
                    .max(self.max)
                    .step(self.step)
                    .default_value(self.value)
            })
        });
        if self.subscription.is_none() {
            let id = id.to_owned();
            let events = events.clone();
            self.subscription = Some(cx.subscribe(state, move |_, event: &SliderEvent, cx| {
                let (kind, value) = match event {
                    SliderEvent::Change(value) => ("change", value.start()),
                    SliderEvent::Release(value) => ("release", value.start()),
                };
                let value = serde_json::json!({"id":id,"kind":kind,"value":value});
                if kind == "release" {
                    // A drag may fill the transient change queue; its final commit must survive.
                    let events = events.clone();
                    cx.spawn(async move |_| {
                        let _ = events.send(value).await;
                    })
                    .detach();
                } else {
                    let _ = events.try_send(value);
                }
            }));
        }
        if let Some(value) = self.pending.take() {
            state.update(cx, |state, cx| state.set_value(value, window, cx));
        }
        div()
            .id(SharedString::from(id.to_owned()))
            .debug_selector({
                let id = id.to_owned();
                move || id.clone()
            })
            .role(Role::Group)
            .aria_label(self.label.clone())
            .w_full()
            .min_w_0()
            .child(Slider::new(state).disabled(disabled))
            .into_any_element()
    }
}

#[derive(Default)]
struct Fields {
    sequence: u64,
    drafts: BTreeMap<String, Draft>,
}

fn invalid() -> HostError {
    HostError::new("slider requires finite ordered bounds and a positive step")
}

pub(super) fn extend(module: HostModule) -> HostModule {
    let fields = Rc::new(RefCell::new(Fields::default()));
    let create = fields.clone();
    let read = fields.clone();
    let update = fields.clone();
    let release = fields.clone();
    let (events, receiver) = tokio::sync::mpsc::channel(32);
    let receiver = Arc::new(tokio::sync::Mutex::new(receiver));
    module
        .function("createSlider", move |args| {
            let value = args.number(0)? as f32;
            let options = args.value(1)?.as_object().ok_or_else(invalid)?;
            let property = |name: &str| {
                options
                    .iter()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value)
            };
            let number = |name: &str| {
                property(name)
                    .and_then(HostValue::as_number)
                    .map(|value| value as f32)
                    .ok_or_else(invalid)
            };
            let (min, max, step) = (number("min")?, number("max")?, number("step")?);
            if !value.is_finite()
                || !min.is_finite()
                || !max.is_finite()
                || !step.is_finite()
                || min >= max
                || step <= 0.
            {
                return Err(invalid());
            }
            let mut fields = create.borrow_mut();
            if fields.drafts.len() >= super::MAX_FIELDS {
                return Err(HostError::new("plugin field capacity exhausted"));
            }
            fields.sequence += 1;
            let id = format!("slider-{}", fields.sequence);
            fields.drafts.insert(
                id.clone(),
                Draft {
                    value: value.clamp(min, max),
                    min,
                    max,
                    step,
                    label: property("label")
                        .and_then(HostValue::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    state: None,
                    pending: None,
                    subscription: None,
                },
            );
            Ok(HostValue::from(id))
        })
        .function("readSlider", move |args| {
            let fields = read.borrow();
            let draft = fields
                .drafts
                .get(args.string(0)?)
                .ok_or_else(|| HostError::new("plugin field is unavailable"))?;
            gpui_shell::with_current_app(|cx| {
                let value = draft.pending.unwrap_or_else(|| {
                    draft
                        .state
                        .as_ref()
                        .map_or(draft.value, |state| state.read(cx).value().start())
                });
                HostValue::Number(value as f64)
            })
            .ok_or_else(|| HostError::new("plugin field requires an active view"))
        })
        .function("setSlider", move |args| {
            let mut fields = update.borrow_mut();
            let draft = fields
                .drafts
                .get_mut(args.string(0)?)
                .ok_or_else(|| HostError::new("plugin field is unavailable"))?;
            let value = args.number(1)? as f32;
            if !value.is_finite() {
                return Err(invalid());
            }
            draft.pending = Some(value.clamp(draft.min, draft.max));
            Ok(HostValue::Null)
        })
        .function("releaseSlider", move |args| {
            release.borrow_mut().drafts.remove(args.string(0)?);
            Ok(HostValue::Null)
        })
        .async_function("nextSliderEvent", move |_| {
            let receiver = receiver.clone();
            Ok(async move {
                let event = receiver
                    .lock()
                    .await
                    .recv()
                    .await
                    .ok_or_else(|| HostError::new("plugin fields are closed"))?;
                super::super::host::sdk::values::encode(event)
            })
        })
        .component("SliderField", move |args, window, cx| {
            let mut fields = fields.borrow_mut();
            let Some(draft) = fields.drafts.get_mut(args.id()) else {
                return div().into_any_element();
            };
            let disabled = args.props().get("disabled").and_then(HostValue::as_bool) == Some(true);
            draft.render(args.id(), disabled, &events, window, cx)
        })
}
