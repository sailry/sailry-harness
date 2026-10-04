//! Kit b79f4ce retained script inputs have no value accessor or change callback.
//! These view-local handles expose the existing native editing states. Kit owns
//! selection, undo, clipboard, focus, IME, accessibility and native context menus.
use gpui_kit::{component::Sizable, prelude::FluentBuilder};
use gpui_kit::{
    component::input::{Enter, Input, InputEvent, InputState, Textarea, TextareaState},
    *,
};
use gpui_shell::{HostError, HostModule, HostValue};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    sync::Arc,
};

const MAX_FIELDS: usize = 64;
mod date;
#[cfg(test)]
mod tests;

struct Draft {
    value: String,
    placeholder: String,
    label: String,
    multiline: bool,
    rows: Option<[usize; 2]>,
    input: Option<Text>,
    pending: Option<String>,
    focus: bool,
    focused: Rc<Cell<bool>>,
    subscription: Option<Subscription>,
}

struct Appearance {
    appearance: bool,
    bordered: bool,
    height: Option<f32>,
}

struct Options {
    disabled: bool,
    readonly: bool,
    small: bool,
    adornment: Option<AnyElement>,
    prefix: bool,
    style: Appearance,
}

enum Text {
    Input(Entity<InputState>),
    Area(Entity<TextareaState>),
}

impl Draft {
    fn value(&self, cx: &App) -> String {
        if let Some(value) = &self.pending {
            return value.clone();
        }
        match &self.input {
            Some(Text::Input(input)) => input.read(cx).value().to_string(),
            Some(Text::Area(input)) => input.read(cx).value().to_string(),
            None => self.value.clone(),
        }
    }

    fn render(
        &mut self,
        id: &str,
        events: &tokio::sync::mpsc::Sender<serde_json::Value>,
        options: Options,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let Options {
            disabled,
            readonly,
            small,
            adornment,
            prefix,
            style,
        } = options;
        let input = self.input.get_or_insert_with(|| {
            if self.multiline {
                Text::Area(cx.new(|cx| {
                    let mut state = TextareaState::new(window, cx);
                    if let Some([min, max]) = self.rows {
                        state = state.auto_grow(min, max);
                    }
                    state
                        .default_value(self.value.clone())
                        .placeholder(self.placeholder.clone())
                }))
            } else {
                Text::Input(cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(self.value.clone())
                        .placeholder(self.placeholder.clone())
                }))
            }
        });
        if self.subscription.is_none() {
            self.subscription = Some(match input {
                Text::Input(input) => observe(input, id, events, self.focused.clone(), cx),
                Text::Area(input) => observe(input, id, events, self.focused.clone(), cx),
            });
        }
        if let Some(value) = self.pending.take() {
            match input {
                Text::Input(input) => {
                    input.update(cx, |input, cx| input.set_value(value, window, cx))
                }
                Text::Area(input) => {
                    input.update(cx, |input, cx| input.set_value(value, window, cx))
                }
            }
        }
        if std::mem::take(&mut self.focus) {
            match input {
                Text::Input(input) => input.update(cx, |input, cx| input.focus(window, cx)),
                Text::Area(input) => input.update(cx, |input, cx| input.focus(window, cx)),
            }
        }
        match input {
            Text::Input(input) => Input::new(input)
                .appearance(style.appearance)
                .bordered(style.bordered)
                .when_some(style.height, |input, height| input.h(px(height)))
                .aria_label(self.label.clone())
                .when(small, |input| input.small())
                .when_some(adornment, |input, child| {
                    if prefix {
                        input.prefix(child)
                    } else {
                        input.suffix(child)
                    }
                })
                .disabled(disabled)
                .readonly(readonly)
                .into_any_element(),
            Text::Area(input) => Textarea::new(input)
                .appearance(style.appearance)
                .bordered(style.bordered)
                .aria_label(self.label.clone())
                .when(self.rows.is_none() && style.height.is_none(), |input| {
                    input.h_24()
                })
                .when_some(style.height, |input, height| input.h(px(height)))
                .disabled(disabled)
                .readonly(readonly)
                .into_any_element(),
        }
    }
}

#[derive(Default)]
struct Fields {
    sequence: u64,
    text: BTreeMap<String, Draft>,
}

pub(super) fn module() -> HostModule {
    let fields = Rc::new(RefCell::new(Fields::default()));
    let (events, receiver) = tokio::sync::mpsc::channel(32);
    let receiver = Arc::new(tokio::sync::Mutex::new(receiver));
    let update = fields.clone();
    let focus = fields.clone();
    let focused = fields.clone();
    let create = fields.clone();
    let read = fields.clone();
    let release = fields.clone();
    let module = HostModule::new("sailry/forms")
        .function("createText", move |args| {
            let value = args.string(0)?.to_owned();
            let options = args.get(1).and_then(HostValue::as_object);
            let property = |name: &str| {
                options.and_then(|options| {
                    options
                        .iter()
                        .find(|(key, _)| key == name)
                        .map(|(_, value)| value)
                })
            };
            let text = |name: &str| {
                property(name)
                    .and_then(HostValue::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            let multiline = property("multiline")
                .and_then(HostValue::as_bool)
                .unwrap_or(false);
            let rows: Option<[usize; 2]> = property("rows")
                .map(|value| {
                    super::host::sdk::values::decode(value).and_then(|value| {
                        serde_json::from_value(value)
                            .map_err(|error| HostError::new(error.to_string()))
                    })
                })
                .transpose()?;
            if rows.is_some_and(|[min, max]| min == 0 || max < min) {
                return Err(HostError::new("text rows require positive ordered bounds"));
            }
            let mut fields = create.borrow_mut();
            if fields.text.len() >= MAX_FIELDS {
                return Err(HostError::new("plugin field capacity exhausted"));
            }
            fields.sequence += 1;
            let id = format!("field-{}", fields.sequence);
            fields.text.insert(
                id.clone(),
                Draft {
                    value,
                    placeholder: text("placeholder"),
                    label: text("label"),
                    multiline,
                    rows,
                    input: None,
                    pending: None,
                    focus: false,
                    focused: Rc::new(Cell::new(false)),
                    subscription: None,
                },
            );
            Ok(HostValue::from(id))
        })
        .function("readText", move |args| {
            let fields = read.borrow();
            let draft = fields
                .text
                .get(args.string(0)?)
                .ok_or_else(|| HostError::new("plugin field is unavailable"))?;
            gpui_shell::with_current_app(|cx| HostValue::from(draft.value(cx)))
                .ok_or_else(|| HostError::new("plugin field requires an active view"))
        })
        .function("setText", move |args| {
            let mut fields = update.borrow_mut();
            let draft = fields
                .text
                .get_mut(args.string(0)?)
                .ok_or_else(|| HostError::new("plugin field is unavailable"))?;
            draft.pending = Some(args.string(1)?.into());
            Ok(HostValue::Null)
        })
        .function("focusText", move |args| {
            let mut fields = focus.borrow_mut();
            let draft = fields
                .text
                .get_mut(args.string(0)?)
                .ok_or_else(|| HostError::new("plugin field is unavailable"))?;
            draft.focus = true;
            Ok(HostValue::Null)
        })
        .function("isTextFocused", move |args| {
            let fields = focused.borrow();
            let draft = fields
                .text
                .get(args.string(0)?)
                .ok_or_else(|| HostError::new("plugin field is unavailable"))?;
            Ok(HostValue::Bool(draft.focused.get()))
        })
        .async_function("nextTextEvent", move |_| {
            let receiver = receiver.clone();
            Ok(async move {
                let event = receiver
                    .lock()
                    .await
                    .recv()
                    .await
                    .ok_or_else(|| HostError::new("plugin fields are closed"))?;
                super::host::sdk::values::encode(event)
            })
        })
        .function("releaseText", move |args| {
            release.borrow_mut().text.remove(args.string(0)?);
            Ok(HostValue::Null)
        })
        .component("TextField", move |mut args, window, cx| {
            let adornment = args.take_children().into_iter().next();
            let mut fields = fields.borrow_mut();
            let Some(draft) = fields.text.get_mut(args.id()) else {
                return div().into_any_element();
            };
            let disabled = args.props().get("disabled").and_then(HostValue::as_bool) == Some(true);
            let view = div()
                .w_full()
                // Kit emits submission and propagates Enter; stop it before native text input.
                .when(!draft.multiline, |view| {
                    view.on_action(|_: &Enter, _, _| {})
                })
                .child(
                    draft.render(
                        args.id(),
                        &events,
                        Options {
                            disabled,
                            readonly: args.props().get("readonly").and_then(HostValue::as_bool)
                                == Some(true),
                            small: args.props().get("size").and_then(HostValue::as_str)
                                == Some("small"),
                            adornment,
                            prefix: args.props().get("adornment").and_then(HostValue::as_str)
                                == Some("prefix"),
                            style: Appearance {
                                appearance: args
                                    .props()
                                    .get("appearance")
                                    .and_then(HostValue::as_bool)
                                    .unwrap_or(true),
                                bordered: args
                                    .props()
                                    .get("bordered")
                                    .and_then(HostValue::as_bool)
                                    .unwrap_or(true),
                                height: args
                                    .props()
                                    .get("height")
                                    .and_then(HostValue::as_number)
                                    .map(|value| value as f32)
                                    .filter(|value| value.is_finite() && *value > 0.),
                            },
                        },
                        window,
                        cx,
                    ),
                );
            #[cfg(test)]
            let view = view.debug_selector(|| args.id().to_owned());
            view.into_any_element()
        });
    date::extend(module).declarations(include_str!("forms/api.d.ts"))
}

fn observe<T: EventEmitter<InputEvent> + 'static>(
    input: &Entity<T>,
    id: &str,
    events: &tokio::sync::mpsc::Sender<serde_json::Value>,
    focused: Rc<Cell<bool>>,
    cx: &mut App,
) -> Subscription {
    let id = id.to_owned();
    let events = events.clone();
    cx.subscribe(input, move |_, event: &InputEvent, _| {
        let kind = match event {
            InputEvent::PressEnter { .. } => "enter",
            InputEvent::Change => "change",
            InputEvent::Focus => {
                focused.set(true);
                "focus"
            }
            InputEvent::Blur => {
                focused.set(false);
                "blur"
            }
        };
        let _ = events.try_send(serde_json::json!({"id":id,"kind":kind}));
    })
}
