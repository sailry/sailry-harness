//! Typed presentation drafts; the execution Node remains the schema validator.
use super::*;
use gpui_kit::component::{
    input::{InputEvent, InputState, NumberInputEvent, StepAction},
    searchable_list::SearchableListItem,
    select::{SelectEvent, SelectState},
};
use sailry_protocol::{
    Secret,
    plugin::models::Catalog,
    plugin::settings::{Field, Kind, SecretUpdate},
};
use serde_json::Value as Json;
mod model_view;
#[cfg(test)]
mod tests;
mod view;

#[derive(Clone)]
struct Item {
    label: SharedString,
    value: Json,
    disabled: bool,
}
impl SearchableListItem for Item {
    type Value = Json;
    fn title(&self) -> SharedString {
        self.label.clone()
    }
    fn value(&self) -> &Json {
        &self.value
    }
    fn disabled(&self) -> bool {
        self.disabled
    }
}
type Choices = Entity<SelectState<Vec<Item>>>;

enum Control {
    Text(Entity<InputState>),
    Number(Entity<InputState>),
    Bool(bool),
    Choice(Choices),
    Model {
        picker: Entity<crate::model_picker::Picker>,
        selected: Option<String>,
        catalog: Catalog,
    },
    Secret {
        input: Entity<InputState>,
        intent: Choices,
        configured: bool,
    },
}

pub(super) struct Setting {
    pub name: String,
    spec: Field,
    required: bool,
    present: bool,
    use_default: bool,
    pub dirty: bool,
    pub locked: bool,
    pub panel: bool,
    control: Control,
    _subscriptions: Vec<Subscription>,
}

pub(super) enum Value {
    Public(Option<Json>),
    Secret(SecretUpdate),
}

pub(super) struct Data<'a> {
    pub state: &'a State,
    pub models: &'a Catalog,
    pub labels: &'a std::collections::BTreeMap<String, String>,
}

impl Setting {
    pub fn new(
        name: String,
        spec: Field,
        required: bool,
        data: Data<'_>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let Data {
            state,
            models,
            labels,
        } = data;
        let value = state.values.get(&name);
        let mut subscriptions = Vec::new();
        let control = if spec.secret.is_some() {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder(tr("form_secret_hint"))
            });
            observe_input(&input, &mut subscriptions, window, cx);
            let keep = state.keepable.contains(&name);
            let intent = choices(
                intents(keep),
                keep.then_some(&Json::String("keep".into())),
                window,
                cx,
            );
            observe_choice(&intent, &mut subscriptions, cx);
            Control::Secret {
                input,
                intent,
                configured: state.configured.contains(&name),
            }
        } else if spec.model {
            let selected = value.and_then(Json::as_str).map(str::to_owned);
            let picker = cx.new(|cx| {
                let mut picker = crate::model_picker::Picker::new();
                picker.catalog(models, selected.as_deref(), cx);
                picker
            });
            subscriptions.push(cx.subscribe(
                &picker,
                |field, _, event: &crate::model_picker::Picked, cx| {
                    if field.locked {
                        return;
                    }
                    if let Control::Model { selected, .. } = &mut field.control {
                        *selected = Some(event.selection.model.clone());
                    }
                    field.edited(cx);
                },
            ));
            Control::Model {
                picker,
                selected,
                catalog: models.clone(),
            }
        } else if spec.effort.is_some() || spec.choices.is_some() {
            let mut items = options(&name, &spec, models, labels);
            if let Some(value) = value
                && !items.iter().any(|item| &item.value == value)
            {
                items.push(Item {
                    label: label(value),
                    value: value.clone(),
                    disabled: true,
                });
            }
            let choice = choices(items, value, window, cx);
            observe_choice(&choice, &mut subscriptions, cx);
            Control::Choice(choice)
        } else if spec.kind == Kind::Boolean {
            Control::Bool(value.and_then(Json::as_bool).unwrap_or(false))
        } else {
            let initial = value
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string())
                })
                .unwrap_or_default();
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(initial)
                    .placeholder(input_hint(&name, &spec))
            });
            observe_input(&input, &mut subscriptions, window, cx);
            if matches!(spec.kind, Kind::Integer | Kind::Number) {
                // Keep Kit's controls without converting integral drafts through f64.
                input.update(cx, |input, cx| input.set_step(None, window, cx));
                subscriptions.push(cx.subscribe_in(
                    &input,
                    window,
                    |field, input, event: &NumberInputEvent, window, cx| {
                        if field.locked {
                            return;
                        }
                        let NumberInputEvent::Step(action) = event;
                        if let Some(next) = stepped(&input.read(cx).value(), *action) {
                            input.update(cx, |input, cx| input.set_value(next, window, cx));
                            field.edited(cx);
                        }
                    },
                ));
                Control::Number(input)
            } else {
                Control::Text(input)
            }
        };
        let use_default = value.is_none() || value == spec.default.as_ref();
        Self {
            name,
            spec,
            use_default,
            required,
            present: required || value.is_some(),
            dirty: false,
            locked: false,
            panel: false,
            control,
            _subscriptions: subscriptions,
        }
    }

    pub fn compatible(&self, name: &str, spec: &Field) -> bool {
        self.name == name
            && self.spec.kind == spec.kind
            && self.spec.model == spec.model
            && self.spec.effort == spec.effort
            && self.spec.choices == spec.choices
            && self.spec.secret == spec.secret
    }

    pub fn refresh(
        &mut self,
        spec: Field,
        required: bool,
        data: Data<'_>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Data {
            state,
            models,
            labels,
        } = data;
        self.spec = spec;
        self.required = required;
        self.present |= required;
        if let Control::Text(input) | Control::Number(input) = &self.control {
            input.update(cx, |input, cx| {
                input.set_placeholder(input_hint(&self.name, &self.spec), window, cx);
            });
        }
        if let Control::Model {
            picker,
            selected,
            catalog,
            ..
        } = &mut self.control
        {
            *catalog = models.clone();
            picker.update(cx, |picker, cx| {
                picker.catalog(models, selected.as_deref(), cx)
            });
        }
        if self.spec.effort.is_none()
            && let Control::Choice(choice) = &self.control
        {
            let selected = choice.read(cx).selected_value().cloned();
            let mut items = options(&self.name, &self.spec, models, labels);
            if let Some(value) = &selected
                && !items.iter().any(|item| &item.value == value)
            {
                items.push(Item {
                    label: label(value),
                    value: value.clone(),
                    disabled: true,
                });
            }
            choice.update(cx, |choice, cx| {
                choice.set_items(items, window, cx);
                if let Some(selected) = selected {
                    choice.set_selected_value(&selected, window, cx);
                }
            });
        }
        if let Control::Secret {
            intent, configured, ..
        } = &mut self.control
        {
            *configured = state.configured.contains(&self.name);
            let selected = intent.read(cx).selected_value().cloned();
            intent.update(cx, |intent, cx| {
                intent.set_items(intents(state.keepable.contains(&self.name)), window, cx);
                if let Some(selected) = selected {
                    intent.set_selected_value(&selected, window, cx);
                }
            });
        }
        cx.notify();
    }

    pub fn boolean(&self) -> bool {
        matches!(self.control, Control::Bool(_))
    }

    pub fn model(&self) -> bool {
        self.spec.model
    }
    pub fn linked(&self) -> Option<&str> {
        self.spec.effort.as_deref()
    }

    pub fn sync_effort(
        &mut self,
        selected: Option<&str>,
        models: &Catalog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Control::Choice(choice) = &self.control else {
            return;
        };
        let mut items = vec![follow_model()];
        if let Some(model) = models
            .models
            .iter()
            .find(|model| Some(model.id.as_str()) == selected)
        {
            items.extend(model.efforts.iter().map(|effort| Item {
                label: crate::reasoning::label(*effort),
                value: Json::String(sailry_protocol::plugin::models::effort_key(*effort)),
                disabled: false,
            }));
        }
        let current = choice.read(cx).selected_value().cloned();
        let valid = current
            .as_ref()
            .is_some_and(|value| items.iter().any(|item| &item.value == value));
        let preferred = self.spec.default.as_ref().filter(|value| {
            self.use_default && !self.dirty && items.iter().any(|item| &item.value == *value)
        });
        let next = if let Some(preferred) = preferred {
            preferred.clone()
        } else if valid {
            current.clone().unwrap()
        } else {
            Json::String(String::new())
        };
        choice.update(cx, |choice, cx| {
            choice.set_items(items, window, cx);
            choice.set_selected_value(&next, window, cx);
        });
        if selected.is_some() && current.is_some() && !valid {
            self.edited(cx);
        }
        cx.notify();
    }

    fn edited(&mut self, cx: &mut Context<Self>) {
        self.use_default = false;
        self.dirty = true;
        self.present = true;
        cx.notify();
    }

    pub fn value(&self, cx: &App) -> Result<Value, &'static str> {
        if let Control::Secret { input, intent, .. } = &self.control {
            return Ok(Value::Secret(
                match intent.read(cx).selected_value().and_then(Json::as_str) {
                    Some("keep") => SecretUpdate::Keep,
                    Some("clear") => SecretUpdate::Clear,
                    Some("replace") => {
                        let value = input.read(cx).value();
                        if value.is_empty() {
                            return Err("plugins_settings_secret_required");
                        }
                        SecretUpdate::Replace(Secret::new(value.to_string()))
                    }
                    _ => return Err("plugins_settings_secret_intent"),
                },
            ));
        }
        if !self.present && !self.required {
            return Ok(Value::Public(None));
        }
        let value = match &self.control {
            Control::Text(input) => Json::String(input.read(cx).value().to_string()),
            Control::Number(input) => {
                let value: Json = serde_json::from_str(input.read(cx).value().trim())
                    .map_err(|_| "plugins_settings_number")?;
                if !value.is_number() {
                    return Err("plugins_settings_number");
                }
                value
            }
            Control::Bool(value) => Json::Bool(*value),
            Control::Choice(choice) => {
                return match choice.read(cx).selected_value() {
                    Some(value) => Ok(Value::Public(Some(value.clone()))),
                    None if !self.required => Ok(Value::Public(None)),
                    _ => Err("plugins_settings_required"),
                };
            }
            Control::Model { selected, .. } => {
                return match selected {
                    Some(value) => Ok(Value::Public(Some(Json::String(value.clone())))),
                    None if !self.required => Ok(Value::Public(None)),
                    _ => Err("plugins_settings_required"),
                };
            }
            Control::Secret { .. } => unreachable!("secret field handled before public values"),
        };
        Ok(Value::Public(Some(value)))
    }

    pub fn clear_secret(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Control::Secret { input, .. } = &self.control {
            // Kit set_value also clears undo history at the pinned revision.
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
    }

    fn unset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.required || self.locked {
            return;
        }
        match &mut self.control {
            Control::Text(input) | Control::Number(input) => {
                input.update(cx, |input, cx| input.set_value("", window, cx))
            }
            Control::Bool(value) => *value = false,
            Control::Choice(choice) => {
                choice.update(cx, |choice, cx| choice.set_selected_index(None, window, cx))
            }
            Control::Model { selected, .. } => *selected = None,
            Control::Secret { .. } => return,
        }
        self.present = false;
        self.dirty = true;
        cx.notify();
    }
}

fn choices(
    items: Vec<Item>,
    selected: Option<&Json>,
    window: &mut Window,
    cx: &mut App,
) -> Choices {
    let index = selected
        .and_then(|selected| items.iter().position(|item| &item.value == selected))
        .map(IndexPath::new);
    cx.new(|cx| SelectState::new(items, index, window, cx))
}

fn options(
    name: &str,
    spec: &Field,
    models: &sailry_protocol::plugin::models::Catalog,
    labels: &std::collections::BTreeMap<String, String>,
) -> Vec<Item> {
    let _ = models;
    if spec.effort.is_some() {
        return vec![follow_model()];
    }
    spec.choices
        .iter()
        .flatten()
        .map(|value| Item {
            label: value
                .as_str()
                .and_then(|value| labels.get(&format!("{name}.{value}")))
                .map(|label| label.clone().into())
                .unwrap_or_else(|| label(value)),
            value: value.clone(),
            disabled: false,
        })
        .collect()
}

fn intents(keep: bool) -> Vec<Item> {
    [
        ("keep", "plugins_settings_keep"),
        ("replace", "plugins_settings_replace"),
        ("clear", "plugins_settings_clear"),
    ]
    .into_iter()
    .filter(|(value, _)| *value != "keep" || keep)
    .map(|(value, key)| Item {
        label: tr(key),
        value: Json::String(value.into()),
        disabled: false,
    })
    .collect()
}

fn label(value: &Json) -> SharedString {
    match value {
        Json::String(value) => {
            if value.is_empty() {
                tr("plugins_settings_empty")
            } else {
                value.clone().into()
            }
        }
        Json::Bool(true) => tr("plugins_settings_yes"),
        Json::Bool(false) => tr("plugins_settings_no"),
        value => value.to_string().into(),
    }
}

fn observe_input(
    input: &Entity<InputState>,
    subscriptions: &mut Vec<Subscription>,
    window: &mut Window,
    cx: &mut Context<Setting>,
) {
    subscriptions.push(
        cx.subscribe_in(input, window, |field, _, event: &InputEvent, _, cx| {
            if matches!(event, InputEvent::Change) && !field.locked {
                field.edited(cx);
            }
        }),
    );
}

fn observe_choice(
    choice: &Choices,
    subscriptions: &mut Vec<Subscription>,
    cx: &mut Context<Setting>,
) {
    subscriptions.push(
        cx.subscribe(choice, |field, _, _: &SelectEvent<Vec<Item>>, cx| {
            if !field.locked {
                field.edited(cx);
            }
        }),
    );
}

fn stepped(text: &str, action: StepAction) -> Option<String> {
    if text.trim().is_empty() {
        return Some(
            if action == StepAction::Increment {
                "1"
            } else {
                "-1"
            }
            .into(),
        );
    }
    let value: serde_json::Number = serde_json::from_str(text.trim()).ok()?;
    let increment = action == StepAction::Increment;
    if let Some(integer) = value
        .as_i64()
        .map(i128::from)
        .or_else(|| value.as_u64().map(i128::from))
    {
        let next = integer + if increment { 1 } else { -1 };
        return (next >= i128::from(i64::MIN) && next <= i128::from(u64::MAX))
            .then(|| next.to_string());
    }
    serde_json::Number::from_f64(value.as_f64()? + if increment { 1. } else { -1. })
        .map(|value| value.to_string())
}

fn follow_model() -> Item {
    Item {
        label: tr("plugins_settings_follow_model"),
        value: Json::String(String::new()),
        disabled: false,
    }
}

fn input_hint(name: &str, spec: &Field) -> String {
    let label = spec
        .title
        .as_deref()
        .filter(|title| !title.is_empty())
        .unwrap_or(name);
    rust_i18n::t!("plugins_settings_input_hint", field = label).to_string()
}
